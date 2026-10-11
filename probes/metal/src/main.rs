// provenance: original
//! `ksci-metal-probe`: the unshipped Metal risk probe (ACCEL-01), run by
//! `cargo xtask bench probe metal`, which attaches the fixed-box manifest digest.
//!
//! Modes (one per invocation, JSON on stdout):
//! - `--path <p> [--quick]`: the integer kernels (`mulhi_u32` only with `--quick`) and the
//!   fast-math probes on one Metal path;
//! - `--cpu-baselines --p-cores <n>`: the CPU Montgomery throughput on 1 and on `n` threads;
//! - `--path <p> --watchdog [--max-loops <n>]`: the watchdog search, each trial a fresh
//!   subprocess (`--watchdog-trial <loops>`) with a wall timeout.
//!
//! Every kernel is checked bit-exact against a CPU reference before any timing of it is accepted:
//! a mismatch exits 1 with the first differing index, and no timing is printed. Times are wall
//! clock around the launch and `client.sync()`. The cold time is the kernel's first dispatch in
//! the process (its compile included); the warm times are the dispatches after it.
//!
//! The CubeCL runtime configuration is pinned before any device exists: no autotune, throughput
//! or compilation cache, no environment records, and the environment root is a fresh scratch
//! directory that must still be empty when the probe ends. CubeCL 0.11.0 compiles its
//! `persistence` feature in through `cubecl-wgpu`'s default-featured `cubecl-cpp` dependency, so
//! the feature cannot be switched off from here; the pinned configuration and the empty-root check
//! are what keep the probe from writing a cache.
//!
//! On every OS but macOS the binary prints `ksci-metal-probe: macOS only` and exits 2.

#[cfg(target_os = "macos")]
mod kernels;

#[cfg(not(target_os = "macos"))]
fn main() -> std::process::ExitCode {
    eprintln!("ksci-metal-probe: macOS only");
    std::process::ExitCode::from(2)
}

#[cfg(target_os = "macos")]
fn main() -> std::process::ExitCode {
    mac::main()
}

#[cfg(target_os = "macos")]
#[allow(
    clippy::disallowed_methods,
    reason = "a timing probe reads the monotonic clock; its times are measurements, never results"
)]
mod mac {
    use std::fs;
    use std::io::Read;
    use std::path::{Path, PathBuf};
    use std::process::{Command, ExitCode, Stdio};
    use std::time::{Duration, Instant};

    use cubecl::client::Client;
    use cubecl::config::RuntimeConfig;
    use cubecl::config::cache::CacheConfig;
    use cubecl::config::{CubeClRuntimeConfig, environment::EnvironmentConfig};
    use cubecl::device::{MetalDevice, WgpuDeviceKind};
    use cubecl::prelude::*;
    use cubecl::records::RecordLevel;
    use cubecl::server::{CubeCountSelection, Handle};
    use cubecl::{CubeElement, Device};
    use serde_json::{Map, Value, json};

    use crate::kernels::{self, NEG_P_INV, P};

    /// Elements per kernel launch: 2^24 input pairs.
    pub const ELEMENTS: usize = 1 << 24;

    /// Warm dispatches timed per kernel, and runs per CPU baseline.
    pub const WARM_RUNS: usize = 30;

    /// Units per cube. 256 divides [`ELEMENTS`] and is within every Metal limit.
    const CUBE_DIM: u32 = 256;

    /// The fixed seed of the input generator. The probe is unshipped, so DET-08 does not apply,
    /// but its inputs are reproducible.
    const SEED: u64 = 0x9E37_79B9_7F4A_7C15;

    /// Elements of a watchdog trial: enough threads to fill the GPU, few enough that the loop
    /// count, not the element count, sets the dispatch time.
    const WATCHDOG_ELEMENTS: usize = 1 << 18;

    /// Elements of a watchdog trial whose output is checked against the CPU loop.
    const WATCHDOG_CHECKED: usize = 64;

    /// The first loop count of the watchdog search; it doubles until a trial fails.
    const WATCHDOG_START_LOOPS: u64 = 1 << 10;

    /// The wall timeout of one watchdog trial subprocess.
    const TRIAL_TIMEOUT: Duration = Duration::from_secs(120);

    /// The binary search stops once the bracket is within this many percent of the passing count.
    const SEARCH_TOLERANCE_PCT: u64 = 5;

    /// Lanes of the fast-math probes beyond the fixed ones.
    const FAST_MATH_RANDOM_LANES: usize = 4096;

    /// macOS QoS class `QOS_CLASS_USER_INTERACTIVE` (`<sys/qos.h>`).
    const QOS_CLASS_USER_INTERACTIVE: u32 = 0x21;

    unsafe extern "C" {
        /// `int pthread_set_qos_class_self_np(qos_class_t, int)` (`<pthread/qos.h>`).
        fn pthread_set_qos_class_self_np(qos_class: u32, relative_priority: i32) -> i32;
    }

    /// A failure that ends the probe with exit 1 and a message on stderr.
    pub type Res<T> = Result<T, String>;

    /// The two Metal paths, by the probe crate's feature names.
    #[derive(Clone, Copy, PartialEq, Eq)]
    pub enum MetalPath {
        /// CubeCL `metal`: the wgpu runtime with the MSL emitter.
        WgpuMsl,
        /// CubeCL `metal-native`: the cubecl-metal runtime.
        MetalNative,
    }

    impl MetalPath {
        fn parse(s: &str) -> Res<MetalPath> {
            match s {
                "wgpu-msl" => Ok(MetalPath::WgpuMsl),
                "metal-native" => Ok(MetalPath::MetalNative),
                other => Err(format!(
                    "unknown --path {other:?}; expected wgpu-msl or metal-native"
                )),
            }
        }

        /// The `--path` value.
        fn arg(self) -> &'static str {
            match self {
                MetalPath::WgpuMsl => "wgpu-msl",
                MetalPath::MetalNative => "metal-native",
            }
        }

        /// The report name: the probe crate's feature.
        pub fn name(self) -> &'static str {
            match self {
                MetalPath::WgpuMsl => "path-wgpu-msl",
                MetalPath::MetalNative => "path-metal-native",
            }
        }

        fn device(self) -> Res<Device> {
            let device = match self {
                MetalPath::WgpuMsl => Device::metal_msl(WgpuDeviceKind::DefaultDevice),
                MetalPath::MetalNative => Device::metal(MetalDevice::DefaultDevice),
            };
            device.map_err(|e| format!("{}: {e}", self.name()))
        }
    }

    /// What one invocation does.
    enum Mode {
        Path {
            path: MetalPath,
            quick: bool,
        },
        CpuBaselines {
            p_cores: usize,
        },
        Watchdog {
            path: MetalPath,
            max_loops: Option<u64>,
        },
        WatchdogTrial {
            path: MetalPath,
            loops: u32,
        },
    }

    fn parse_args() -> Res<Mode> {
        let mut path = None;
        let mut quick = false;
        let mut cpu = false;
        let mut p_cores = None;
        let mut watchdog = false;
        let mut max_loops = None;
        let mut trial = None;
        let mut it = std::env::args().skip(1);
        let value = |it: &mut std::iter::Skip<std::env::Args>, flag: &str| {
            it.next().ok_or(format!("{flag} needs a value"))
        };
        while let Some(arg) = it.next() {
            match arg.as_str() {
                "--path" => path = Some(MetalPath::parse(&value(&mut it, "--path")?)?),
                "--quick" => quick = true,
                "--cpu-baselines" => cpu = true,
                "--p-cores" => p_cores = Some(parse_num(&value(&mut it, "--p-cores")?)?),
                "--watchdog" => watchdog = true,
                "--max-loops" => max_loops = Some(parse_num(&value(&mut it, "--max-loops")?)?),
                "--watchdog-trial" => {
                    trial = Some(parse_num(&value(&mut it, "--watchdog-trial")?)?)
                }
                other => return Err(format!("unknown argument {other:?}")),
            }
        }
        let need_path = || path.ok_or("--path wgpu-msl|metal-native is required".to_owned());
        match (cpu, watchdog, trial) {
            (true, false, None) => {
                let p_cores = p_cores.ok_or("--cpu-baselines needs --p-cores <n>")?;
                if p_cores == 0 {
                    return Err("--p-cores must be at least 1".to_owned());
                }
                Ok(Mode::CpuBaselines {
                    p_cores: p_cores as usize,
                })
            }
            (false, true, None) => Ok(Mode::Watchdog {
                path: need_path()?,
                max_loops,
            }),
            (false, false, Some(loops)) => Ok(Mode::WatchdogTrial {
                path: need_path()?,
                loops: u32::try_from(loops).map_err(|_| "--watchdog-trial exceeds u32")?,
            }),
            (false, false, None) => Ok(Mode::Path {
                path: need_path()?,
                quick,
            }),
            _ => Err("use one of --path, --cpu-baselines, --watchdog, --watchdog-trial".into()),
        }
    }

    fn parse_num(s: &str) -> Res<u64> {
        s.parse().map_err(|_| format!("{s:?} is not a number"))
    }

    pub fn main() -> ExitCode {
        match run() {
            Ok(report) => {
                println!("{report}");
                ExitCode::SUCCESS
            }
            Err(msg) => {
                eprintln!("ksci-metal-probe: {msg}");
                ExitCode::FAILURE
            }
        }
    }

    fn run() -> Res<Value> {
        match parse_args()? {
            Mode::CpuBaselines { p_cores } => cpu_baselines(p_cores),
            // The parent of the trials touches no GPU; each trial pins its own configuration.
            Mode::Watchdog { path, max_loops } => watchdog_search(path, max_loops),
            Mode::Path { path, quick } => with_pinned_cubecl(|| measure_path(path, quick)),
            Mode::WatchdogTrial { path, loops } => {
                with_pinned_cubecl(|| watchdog_trial(path, loops))
            }
        }
    }

    /// Runs `body` under the pinned CubeCL configuration, then fails if CubeCL wrote anything
    /// under its environment root.
    fn with_pinned_cubecl(body: impl FnOnce() -> Res<Value>) -> Res<Value> {
        let env_root = scratch_env_root()?;
        pin_cubecl_config(&env_root);
        let result = body();
        let writes = files_under(&env_root);
        let _ = fs::remove_dir_all(&env_root);
        let mut report = result?;
        let writes = writes?;
        if !writes.is_empty() {
            return Err(format!(
                "CubeCL wrote under its pinned environment root although every cache is off: {}",
                writes.join(", ")
            ));
        }
        report["cubecl_persistence_writes"] = json!(writes);
        Ok(report)
    }

    /// A fresh, empty directory for CubeCL's environment root.
    fn scratch_env_root() -> Res<PathBuf> {
        let dir = std::env::temp_dir().join(format!("ksci-metal-probe-{}", std::process::id()));
        if dir.exists() {
            fs::remove_dir_all(&dir).map_err(|e| format!("remove {}: {e}", dir.display()))?;
        }
        fs::create_dir_all(&dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
        Ok(dir)
    }

    /// Pins CubeCL's runtime configuration before any device exists, so neither a `cubecl.toml`
    /// nor a `CUBECL_*` variable reaches the probe, and no cache is kept.
    fn pin_cubecl_config(env_root: &Path) {
        let mut config = CubeClRuntimeConfig::default();
        config.autotune.disable_cache = true;
        config.throughput.disable_cache = true;
        config.compilation.cache = false;
        config.environment = EnvironmentConfig {
            path: CacheConfig::Directory(env_root.to_path_buf()),
            ..EnvironmentConfig::default()
        };
        config.environment.records.level = RecordLevel::Off;
        CubeClRuntimeConfig::set(config);
    }

    /// Every file below `dir`, as paths relative to it, sorted.
    fn files_under(dir: &Path) -> Res<Vec<String>> {
        let mut out = Vec::new();
        let mut stack = vec![dir.to_path_buf()];
        while let Some(d) = stack.pop() {
            for entry in fs::read_dir(&d).map_err(|e| format!("read {}: {e}", d.display()))? {
                let entry = entry.map_err(|e| format!("read {}: {e}", d.display()))?;
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                } else {
                    let rel = path.strip_prefix(dir).unwrap_or(&path);
                    out.push(rel.display().to_string());
                }
            }
        }
        out.sort();
        Ok(out)
    }

    /// The probe's input words: the xorshift64* sequence from [`SEED`].
    pub struct Inputs {
        state: u64,
    }

    impl Inputs {
        pub fn new() -> Inputs {
            Inputs { state: SEED }
        }

        pub fn next_u64(&mut self) -> u64 {
            let mut x = self.state;
            x ^= x >> 12;
            x ^= x << 25;
            x ^= x >> 27;
            self.state = x;
            x.wrapping_mul(0x2545_F491_4F6C_DD1D)
        }

        /// The high 32 bits of the next word (xorshift64*'s strongest bits).
        pub fn next_u32(&mut self) -> u32 {
            (self.next_u64() >> 32) as u32
        }

        pub fn u32s(&mut self, n: usize) -> Vec<u32> {
            (0..n).map(|_| self.next_u32()).collect()
        }

        pub fn u64s(&mut self, n: usize) -> Vec<u64> {
            (0..n).map(|_| self.next_u64()).collect()
        }

        /// Residues below [`P`], for the Montgomery kernels.
        pub fn residues(&mut self, n: usize) -> Vec<u32> {
            (0..n).map(|_| self.next_u32() % P).collect()
        }
    }

    /// `2^-32 mod P`, from Fermat: `(2^32 mod P)^(P-2) mod P`.
    fn r_inv_mod_p() -> u64 {
        let p = u64::from(P);
        let (mut base, mut exp, mut acc) = ((1u64 << 32) % p, p - 2, 1u64);
        while exp > 0 {
            if exp & 1 == 1 {
                acc = acc * base % p;
            }
            base = base * base % p;
            exp >>= 1;
        }
        acc
    }

    /// The reference Montgomery product, `a * b * 2^-32 mod P`, in u128 arithmetic: independent
    /// of the word-level algorithm the kernel and [`mont_mul_cpu`] share.
    fn mont_ref(a: u32, b: u32, r_inv: u64) -> u32 {
        let t = u128::from(a) * u128::from(b) % u128::from(P);
        (t * u128::from(r_inv) % u128::from(P)) as u32
    }

    /// The u32 Montgomery product on the CPU: the kernel's algorithm (see `kernels::mont_mul`).
    #[inline(always)]
    pub fn mont_mul_cpu(a: u32, b: u32) -> u32 {
        let t = u64::from(a) * u64::from(b);
        let lo = t as u32;
        let hi = (t >> 32) as u32;
        let m = lo.wrapping_mul(NEG_P_INV);
        let mp_hi = ((u64::from(m) * u64::from(P)) >> 32) as u32;
        let r = hi + mp_hi + u32::from(lo != 0);
        if r >= P { r - P } else { r }
    }

    /// The u32 high word from 16-bit limbs on the CPU, for no other purpose than to show the
    /// limb formula equals the 64-bit one before the kernel is trusted with it.
    fn mulhi_limb16_cpu(a: u32, b: u32) -> u32 {
        let (a0, a1, b0, b1) = (a & 0xFFFF, a >> 16, b & 0xFFFF, b >> 16);
        let (p00, p01, p10, p11) = (a0 * b0, a0 * b1, a1 * b0, a1 * b1);
        let mid = (p00 >> 16) + (p01 & 0xFFFF) + (p10 & 0xFFFF);
        p11 + (p01 >> 16) + (p10 >> 16) + (mid >> 16)
    }

    /// Times one kernel: one cold dispatch, the bit-exact check of its output, then
    /// [`WARM_RUNS`] warm dispatches and a second check of the output.
    fn time_kernel<T: CubeElement + PartialEq + std::fmt::Debug>(
        client: &Client,
        name: &str,
        elements: usize,
        output: &Handle,
        expected: &[T],
        launch: &dyn Fn(),
    ) -> Res<Value> {
        let start = Instant::now();
        launch();
        sync(client)?;
        let cold_ns = start.elapsed().as_nanos() as u64;
        check_exact(client, name, output, expected)?;

        let mut warm = Vec::with_capacity(WARM_RUNS);
        for _ in 0..WARM_RUNS {
            let start = Instant::now();
            launch();
            sync(client)?;
            warm.push(start.elapsed().as_nanos() as u64);
        }
        check_exact(client, name, output, expected)?;
        let stats = stats(&mut warm);
        Ok(json!({
            "exact": true,
            "elements": elements,
            "cold_ns": cold_ns,
            "warm_ns": stats.to_json(),
            "ops_per_s": per_second(elements as u64, stats.median),
        }))
    }

    /// `count` operations in `ns` nanoseconds, per second.
    fn per_second(count: u64, ns: u64) -> u64 {
        (u128::from(count) * 1_000_000_000 / u128::from(ns.max(1))) as u64
    }

    fn sync(client: &Client) -> Res<()> {
        cubecl::future::block_on(client.sync()).map_err(|e| format!("sync: {e:?}"))
    }

    /// The words of `output`, read back.
    fn read_back<T: CubeElement>(client: &Client, name: &str, output: &Handle) -> Res<Vec<T>> {
        let bytes = client
            .read_one(output.clone())
            .map_err(|e| format!("{name}: read back: {e:?}"))?;
        Ok(T::from_bytes(&bytes).to_vec())
    }

    /// Reads `output` back and compares it word for word with `expected`.
    fn check_exact<T: CubeElement + PartialEq + std::fmt::Debug>(
        client: &Client,
        name: &str,
        output: &Handle,
        expected: &[T],
    ) -> Res<()> {
        let actual = read_back::<T>(client, name, output)?;
        if actual.len() < expected.len() {
            return Err(format!(
                "{name}: read back {} words, expected {}",
                actual.len(),
                expected.len()
            ));
        }
        match actual.iter().zip(expected).position(|(a, e)| a != e) {
            None => Ok(()),
            Some(i) => Err(format!(
                "{name}: NOT EXACT at index {i}: device {:?}, CPU reference {:?}",
                actual[i], expected[i]
            )),
        }
    }

    /// Median and spread of a set of times.
    pub struct Stats {
        pub median: u64,
        pub min: u64,
        pub max: u64,
        pub p10: u64,
        pub p90: u64,
    }

    impl Stats {
        pub fn to_json(&self) -> Value {
            json!({
                "median": self.median,
                "min": self.min,
                "max": self.max,
                "p10": self.p10,
                "p90": self.p90,
            })
        }
    }

    /// Nearest-rank statistics of `samples` (sorted in place; at least one sample).
    pub fn stats(samples: &mut [u64]) -> Stats {
        samples.sort_unstable();
        let n = samples.len();
        let rank = |p: usize| samples[((p * n).div_ceil(100)).clamp(1, n) - 1];
        Stats {
            median: rank(50),
            min: samples[0],
            max: samples[n - 1],
            p10: rank(10),
            p90: rank(90),
        }
    }

    /// A cube count covering `elements` units of [`CUBE_DIM`], spread to the device limits.
    fn cube_count(client: &Client, elements: usize) -> CubeCount {
        let cubes = elements.div_ceil(CUBE_DIM as usize) as u32;
        CubeCountSelection::new(client, cubes).cube_count()
    }

    /// One path: device creation, then each kernel, then the fast-math probes.
    fn measure_path(path: MetalPath, quick: bool) -> Res<Value> {
        let start = Instant::now();
        let device = path.device()?;
        let client = device.client();
        let init_ns = start.elapsed().as_nanos() as u64;

        let mut kernels_out = Map::new();
        kernels_out.insert("mulhi_u32".into(), probe_mulhi_u32(&client)?);
        let mut out = json!({
            "path": path.name(),
            "device_init_ns": init_ns,
            "elements": ELEMENTS,
            "warm_runs": WARM_RUNS,
            "cube_dim": CUBE_DIM,
            "mode": if quick { "quick" } else { "standard" },
            "inputs": "xorshift64* from seed 0x9e3779b97f4a7c15",
            "timing": "wall clock around launch and client.sync(); cold = first dispatch of the kernel in the process, compile included",
        });
        if !quick {
            kernels_out.insert("mulhi_u32_limb16".into(), probe_mulhi_u32_limb16(&client)?);
            kernels_out.insert("mul_u64_wrapping".into(), probe_mul_u64(&client)?);
            kernels_out.insert("mont_mul_u32".into(), probe_mont_mul_u32(&client)?);
            out["fast_math"] = probe_fast_math(&client)?;
        }
        out["kernels"] = Value::Object(kernels_out);
        Ok(out)
    }

    /// Uploads two u32 operand arrays and times `kernel` over them against `expected`.
    fn probe_binary_u32(
        client: &Client,
        name: &str,
        lhs: &[u32],
        rhs: &[u32],
        expected: &[u32],
        kernel: unsafe fn(&Client, CubeCount, CubeDim, BufferArg, BufferArg, BufferArg),
    ) -> Res<Value> {
        let n = lhs.len();
        let lhs_h = client.create_from_slice(u32::as_bytes(lhs));
        let rhs_h = client.create_from_slice(u32::as_bytes(rhs));
        let out_h = client.empty(size_of_val(lhs));
        let count = cube_count(client, n);
        let launch = || unsafe {
            kernel(
                client,
                count.clone(),
                CubeDim::new_1d(CUBE_DIM),
                BufferArg::from_raw_parts(lhs_h.clone(), n),
                BufferArg::from_raw_parts(rhs_h.clone(), n),
                BufferArg::from_raw_parts(out_h.clone(), n),
            )
        };
        time_kernel(client, name, n, &out_h, expected, &launch)
    }

    /// u32 `mul_hi` as CubeCL lowers it, against `((a as u64 * b as u64) >> 32) as u32`.
    fn probe_mulhi_u32(client: &Client) -> Res<Value> {
        let mut inputs = Inputs::new();
        let lhs = inputs.u32s(ELEMENTS);
        let rhs = inputs.u32s(ELEMENTS);
        let expected: Vec<u32> = lhs
            .iter()
            .zip(&rhs)
            .map(|(&a, &b)| ((u64::from(a) * u64::from(b)) >> 32) as u32)
            .collect();
        probe_binary_u32(
            client,
            "mulhi_u32",
            &lhs,
            &rhs,
            &expected,
            |c, n, d, l, r, o| unsafe { kernels::mulhi_u32::launch_unchecked(c, n, d, l, r, o) },
        )
    }

    /// The 16-bit-limb u32 mulhi, against the same 64-bit reference.
    fn probe_mulhi_u32_limb16(client: &Client) -> Res<Value> {
        let mut inputs = Inputs::new();
        let lhs = inputs.u32s(ELEMENTS);
        let rhs = inputs.u32s(ELEMENTS);
        let expected: Vec<u32> = lhs
            .iter()
            .zip(&rhs)
            .map(|(&a, &b)| ((u64::from(a) * u64::from(b)) >> 32) as u32)
            .collect();
        if let Some(i) = (0..ELEMENTS).find(|&i| mulhi_limb16_cpu(lhs[i], rhs[i]) != expected[i]) {
            return Err(format!(
                "limb16 formula differs from the 64-bit one at index {i}"
            ));
        }
        probe_binary_u32(
            client,
            "mulhi_u32_limb16",
            &lhs,
            &rhs,
            &expected,
            |c, n, d, l, r, o| unsafe {
                kernels::mulhi_u32_limb16::launch_unchecked(c, n, d, l, r, o)
            },
        )
    }

    /// u64 `wrapping_mul`.
    fn probe_mul_u64(client: &Client) -> Res<Value> {
        let mut inputs = Inputs::new();
        let lhs = inputs.u64s(ELEMENTS);
        let rhs = inputs.u64s(ELEMENTS);
        let expected: Vec<u64> = lhs
            .iter()
            .zip(&rhs)
            .map(|(&a, &b)| a.wrapping_mul(b))
            .collect();
        let lhs_h = client.create_from_slice(u64::as_bytes(&lhs));
        let rhs_h = client.create_from_slice(u64::as_bytes(&rhs));
        let out_h = client.empty(ELEMENTS * size_of::<u64>());
        let count = cube_count(client, ELEMENTS);
        let launch = || unsafe {
            kernels::mul_u64_wrapping::launch_unchecked(
                client,
                count.clone(),
                CubeDim::new_1d(CUBE_DIM),
                BufferArg::from_raw_parts(lhs_h.clone(), ELEMENTS),
                BufferArg::from_raw_parts(rhs_h.clone(), ELEMENTS),
                BufferArg::from_raw_parts(out_h.clone(), ELEMENTS),
            )
        };
        time_kernel(
            client,
            "mul_u64_wrapping",
            ELEMENTS,
            &out_h,
            &expected,
            &launch,
        )
    }

    /// The u32 Montgomery multiply mod 2013265921, against the u128 reference.
    fn probe_mont_mul_u32(client: &Client) -> Res<Value> {
        let mut inputs = Inputs::new();
        let lhs = inputs.residues(ELEMENTS);
        let rhs = inputs.residues(ELEMENTS);
        let r_inv = r_inv_mod_p();
        let expected: Vec<u32> = lhs
            .iter()
            .zip(&rhs)
            .map(|(&a, &b)| mont_ref(a, b, r_inv))
            .collect();
        if let Some(i) = (0..ELEMENTS).find(|&i| mont_mul_cpu(lhs[i], rhs[i]) != expected[i]) {
            return Err(format!(
                "CPU Montgomery differs from the u128 reference at index {i}"
            ));
        }
        let mut out = probe_binary_u32(
            client,
            "mont_mul_u32",
            &lhs,
            &rhs,
            &expected,
            |c, n, d, l, r, o| unsafe { kernels::mont_mul_u32::launch_unchecked(c, n, d, l, r, o) },
        )?;
        out["modulus"] = json!(P);
        Ok(out)
    }

    /// One fast-math probe input lane.
    struct Lane {
        a: f32,
        b: f32,
        c: f32,
    }

    /// The FMA-contraction lanes: the plan's exact-product lane, two whose rounded product
    /// cancels against `c`, and random lanes `a, b` in [1, 2) with `c = -round(a * b)`.
    fn fma_lanes() -> Vec<Lane> {
        let e = |k: i32| 2f32.powi(k);
        let mut lanes = vec![
            // (1 + 2^-12)(1 - 2^-12) = 1 - 2^-24 is exact in f32: fused and unfused agree.
            Lane {
                a: 1.0 + e(-12),
                b: 1.0 - e(-12),
                c: -1.0,
            },
            // (1 + 2^-23)^2 = 1 + 2^-22 + 2^-46: unfused 0, fused 2^-46.
            Lane {
                a: 1.0 + e(-23),
                b: 1.0 + e(-23),
                c: -(1.0 + e(-22)),
            },
            // (1 + 2^-23)(1 - 2^-23) = 1 - 2^-46 rounds to 1: unfused 0, fused -2^-46.
            Lane {
                a: 1.0 + e(-23),
                b: 1.0 - e(-23),
                c: -1.0,
            },
        ];
        let mut inputs = Inputs::new();
        let unit = |w: u32| f32::from_bits(0x3F80_0000 | (w >> 9)); // [1, 2)
        for _ in 0..FAST_MATH_RANDOM_LANES {
            let a = unit(inputs.next_u32());
            let b = unit(inputs.next_u32());
            lanes.push(Lane { a, b, c: -(a * b) });
        }
        lanes
    }

    /// The reassociation lanes: `a = 2^24, b = 1, c = -2^24` (written order 0, reassociated 1),
    /// its mirror, and random lanes with cancelling magnitudes.
    fn reassoc_lanes() -> Vec<Lane> {
        let big = 2f32.powi(24);
        let mut lanes = vec![
            Lane {
                a: big,
                b: 1.0,
                c: -big,
            },
            Lane {
                a: -big,
                b: -1.0,
                c: big,
            },
            Lane {
                a: 1.0e8,
                b: 3.0,
                c: -1.0e8,
            },
        ];
        let mut inputs = Inputs::new();
        for _ in 0..FAST_MATH_RANDOM_LANES {
            let mag = 2f32.powi((inputs.next_u32() % 16) as i32 + 16);
            let a = mag * (1.0 + (inputs.next_u32() >> 9) as f32 * 2f32.powi(-23));
            let b = (inputs.next_u32() >> 20) as f32 * 0.25;
            lanes.push(Lane { a, b, c: -a });
        }
        lanes
    }

    /// Classifies device bits against the written-order IEEE result and the transformed one, on
    /// the lanes where the two differ: `(written, transformed, neither, discriminating)`.
    fn classify(device: &[f32], written: &[f32], transformed: &[f32]) -> (u64, u64, u64, u64) {
        let (mut w, mut t, mut n, mut d) = (0, 0, 0, 0);
        for i in 0..device.len() {
            if written[i].to_bits() == transformed[i].to_bits() {
                if device[i].to_bits() != written[i].to_bits() {
                    n += 1;
                }
                continue;
            }
            d += 1;
            match device[i].to_bits() {
                x if x == written[i].to_bits() => w += 1,
                x if x == transformed[i].to_bits() => t += 1,
                _ => n += 1,
            }
        }
        (w, t, n, d)
    }

    /// The FMA verdict from [`classify`]: `fused` when every discriminating lane shows the fused
    /// result, `unfused` when every one shows the unfused result, and `mixed` otherwise or when a
    /// lane matches neither.
    fn fma_verdict(counts: (u64, u64, u64, u64)) -> Res<&'static str> {
        let (w, t, n, d) = counts;
        if d == 0 {
            return Err("no FMA lane discriminates; the probe inputs are wrong".into());
        }
        Ok(match (w, t, n) {
            (_, 0, 0) => "unfused",
            (0, _, 0) => "fused",
            _ => "mixed",
        })
    }

    /// The reassociation verdict from [`classify`]: `observed` when any discriminating lane shows
    /// the reassociated or simplified result (the compiler applies it by expression shape, so one
    /// shape is enough), `not-observed` when none does.
    fn reassoc_verdict(counts: (u64, u64, u64, u64)) -> Res<&'static str> {
        let (_, t, _, d) = counts;
        if d == 0 {
            return Err("no reassociation lane discriminates; the probe inputs are wrong".into());
        }
        Ok(if t > 0 { "observed" } else { "not-observed" })
    }

    /// The fast-math default of this path, measured: FMA contraction and reassociation, by bit
    /// comparison of f32 results with the CPU's unfused IEEE evaluation and with `f32::mul_add`.
    fn probe_fast_math(client: &Client) -> Res<Value> {
        let fma = fma_lanes();
        let n = fma.len();
        let a: Vec<f32> = fma.iter().map(|l| l.a).collect();
        let b: Vec<f32> = fma.iter().map(|l| l.b).collect();
        let c: Vec<f32> = fma.iter().map(|l| l.c).collect();
        let unfused: Vec<f32> = fma.iter().map(|l| l.a * l.b + l.c).collect();
        let fused: Vec<f32> = fma.iter().map(|l| l.a.mul_add(l.b, l.c)).collect();
        let (ah, bh, ch) = (
            client.create_from_slice(f32::as_bytes(&a)),
            client.create_from_slice(f32::as_bytes(&b)),
            client.create_from_slice(f32::as_bytes(&c)),
        );
        let out_h = client.empty(n * size_of::<f32>());
        unsafe {
            kernels::fma_probe_f32::launch_unchecked(
                client,
                cube_count(client, n),
                CubeDim::new_1d(CUBE_DIM),
                BufferArg::from_raw_parts(ah, n),
                BufferArg::from_raw_parts(bh, n),
                BufferArg::from_raw_parts(ch, n),
                BufferArg::from_raw_parts(out_h.clone(), n),
            )
        };
        sync(client)?;
        let device = read_back::<f32>(client, "fma_probe_f32", &out_h)?;
        let fma_counts = classify(&device[..n], &unfused, &fused);

        let re = reassoc_lanes();
        let m = re.len();
        let a: Vec<f32> = re.iter().map(|l| l.a).collect();
        let b: Vec<f32> = re.iter().map(|l| l.b).collect();
        let c: Vec<f32> = re.iter().map(|l| l.c).collect();
        let left_ieee: Vec<f32> = re.iter().map(|l| (l.a + l.b) + l.c).collect();
        let left_re: Vec<f32> = re.iter().map(|l| l.a + (l.b + l.c)).collect();
        let cancel_ieee: Vec<f32> = re.iter().map(|l| (l.a + l.b) - l.a).collect();
        let cancel_re: Vec<f32> = re.iter().map(|l| l.b).collect();
        let (ah, bh, ch) = (
            client.create_from_slice(f32::as_bytes(&a)),
            client.create_from_slice(f32::as_bytes(&b)),
            client.create_from_slice(f32::as_bytes(&c)),
        );
        let left_h = client.empty(m * size_of::<f32>());
        let cancel_h = client.empty(m * size_of::<f32>());
        unsafe {
            kernels::reassoc_probe_f32::launch_unchecked(
                client,
                cube_count(client, m),
                CubeDim::new_1d(CUBE_DIM),
                BufferArg::from_raw_parts(ah, m),
                BufferArg::from_raw_parts(bh, m),
                BufferArg::from_raw_parts(ch, m),
                BufferArg::from_raw_parts(left_h.clone(), m),
                BufferArg::from_raw_parts(cancel_h.clone(), m),
            )
        };
        sync(client)?;
        let left = read_back::<f32>(client, "reassoc_probe_f32", &left_h)?;
        let cancel = read_back::<f32>(client, "reassoc_probe_f32", &cancel_h)?;
        let left_counts = classify(&left[..m], &left_ieee, &left_re);
        let cancel_counts = classify(&cancel[..m], &cancel_ieee, &cancel_re);
        let joint = (
            left_counts.0 + cancel_counts.0,
            left_counts.1 + cancel_counts.1,
            left_counts.2 + cancel_counts.2,
            left_counts.3 + cancel_counts.3,
        );
        let counts = |(w, t, n, d): (u64, u64, u64, u64), wn: &str, tn: &str| json!({ "discriminating_lanes": d, wn: w, tn: t, "neither": n });
        Ok(json!({
            "fma_contraction": fma_verdict(fma_counts)?,
            "reassociation": reassoc_verdict(joint)?,
            "fma_lanes": counts(fma_counts, "unfused", "fused"),
            "reassoc_sum_lanes": counts(left_counts, "written_order", "reassociated"),
            "reassoc_cancel_lanes": counts(cancel_counts, "written_order", "simplified"),
            "method": "f32 bit patterns of a*b+c against CPU unfused IEEE and f32::mul_add; (a+b)+c against a+(b+c); (a+b)-a against b; counted on the lanes where the two CPU results differ",
        }))
    }

    /// Sets the calling thread's QoS class to user-interactive (assumption A9: steering, not a
    /// hard pin to a P-core).
    fn set_qos_user_interactive() -> Res<()> {
        // SAFETY: a libSystem call on the calling thread with a valid QoS class constant.
        let rc = unsafe { pthread_set_qos_class_self_np(QOS_CLASS_USER_INTERACTIVE, 0) };
        if rc == 0 {
            Ok(())
        } else {
            Err(format!("pthread_set_qos_class_self_np: {rc}"))
        }
    }

    /// `mont_mul_cpu` over `lhs`/`rhs` into `out`, on `threads` scoped threads over equal
    /// contiguous chunks, each at QoS user-interactive. Returns the wall time in nanoseconds.
    fn mont_cpu_run(lhs: &[u32], rhs: &[u32], out: &mut [u32], threads: usize) -> Res<u64> {
        let chunk = lhs.len().div_ceil(threads);
        let start = Instant::now();
        let results: Vec<Res<()>> = std::thread::scope(|s| {
            let handles: Vec<_> = out
                .chunks_mut(chunk)
                .zip(lhs.chunks(chunk).zip(rhs.chunks(chunk)))
                .map(|(o, (l, r))| {
                    s.spawn(move || {
                        set_qos_user_interactive()?;
                        for i in 0..o.len() {
                            o[i] = mont_mul_cpu(l[i], r[i]);
                        }
                        Ok(())
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|h| h.join().unwrap_or_else(|_| Err("thread panicked".into())))
                .collect()
        });
        let ns = start.elapsed().as_nanos() as u64;
        results.into_iter().collect::<Res<Vec<()>>>()?;
        Ok(ns)
    }

    /// The CPU Montgomery baselines: [`WARM_RUNS`] runs on 1 thread and on `p_cores` threads,
    /// each output checked against the u128 reference.
    fn cpu_baselines(p_cores: usize) -> Res<Value> {
        let mut inputs = Inputs::new();
        let lhs = inputs.residues(ELEMENTS);
        let rhs = inputs.residues(ELEMENTS);
        let r_inv = r_inv_mod_p();
        let expected: Vec<u32> = lhs
            .iter()
            .zip(&rhs)
            .map(|(&a, &b)| mont_ref(a, b, r_inv))
            .collect();
        let one = |threads: usize| -> Res<Value> {
            let mut out = vec![0u32; ELEMENTS];
            let cold = mont_cpu_run(&lhs, &rhs, &mut out, threads)?;
            if let Some(i) = (0..ELEMENTS).find(|&i| out[i] != expected[i]) {
                return Err(format!("CPU Montgomery NOT EXACT at index {i}"));
            }
            let mut times = Vec::with_capacity(WARM_RUNS);
            for _ in 0..WARM_RUNS {
                times.push(mont_cpu_run(&lhs, &rhs, &mut out, threads)?);
            }
            if out != expected {
                return Err("CPU Montgomery output changed between runs".into());
            }
            let s = stats(&mut times);
            Ok(json!({
                "threads": threads,
                "exact": true,
                "elements": ELEMENTS,
                "cold_ns": cold,
                "runs_ns": s.to_json(),
                "ops_per_s": per_second(ELEMENTS as u64, s.median),
            }))
        };
        let one_p = one(1)?;
        let all_p = one(p_cores)?;
        Ok(json!({
            "kernel": "mont_mul_u32 (CPU, the kernel's word algorithm)",
            "modulus": P,
            "qos_class": "user-interactive",
            "pinning": "macOS cannot hard-pin a thread to a P-core; each thread sets QoS user-interactive (pthread_set_qos_class_self_np), which steers it to the P-cores (assumption A9)",
            "timing": "wall clock from thread spawn to join; equal contiguous chunks",
            "one_p_core": one_p,
            "all_p_cores": all_p,
        }))
    }

    /// One watchdog trial: a warm-up dispatch (compiles the kernel), then one dispatch of the
    /// Montgomery loop with `loops` iterations per element, timed, and its first
    /// [`WATCHDOG_CHECKED`] outputs checked against the CPU loop.
    fn watchdog_trial(path: MetalPath, loops: u32) -> Res<Value> {
        let device = path.device()?;
        let client = device.client();
        let mut inputs = Inputs::new();
        let lhs = inputs.residues(WATCHDOG_ELEMENTS);
        let rhs = inputs.residues(WATCHDOG_ELEMENTS);
        let lhs_h = client.create_from_slice(u32::as_bytes(&lhs));
        let rhs_h = client.create_from_slice(u32::as_bytes(&rhs));
        let out_h = client.empty(WATCHDOG_ELEMENTS * size_of::<u32>());
        let count = cube_count(&client, WATCHDOG_ELEMENTS);
        let launch = |loops: u32| unsafe {
            kernels::mont_loop_u32::launch_unchecked(
                &client,
                count.clone(),
                CubeDim::new_1d(CUBE_DIM),
                BufferArg::from_raw_parts(lhs_h.clone(), WATCHDOG_ELEMENTS),
                BufferArg::from_raw_parts(rhs_h.clone(), WATCHDOG_ELEMENTS),
                BufferArg::from_raw_parts(out_h.clone(), WATCHDOG_ELEMENTS),
                loops,
            )
        };
        launch(1);
        sync(&client)?;
        let start = Instant::now();
        launch(loops);
        sync(&client)?;
        let kernel_ns = start.elapsed().as_nanos() as u64;
        let device_out = read_back::<u32>(&client, "mont_loop_u32", &out_h)?;
        for i in 0..WATCHDOG_CHECKED {
            let mut x = lhs[i];
            for _ in 0..loops {
                x = mont_mul_cpu(x, rhs[i]);
            }
            if device_out.get(i) != Some(&x) {
                return Err(format!(
                    "mont_loop_u32 NOT EXACT at index {i} after {loops} loops: device {:?}, CPU {x}",
                    device_out.get(i)
                ));
            }
        }
        Ok(json!({
            "loops": loops,
            "elements": WATCHDOG_ELEMENTS,
            "kernel_ns": kernel_ns,
            "exact_checked": WATCHDOG_CHECKED,
        }))
    }

    /// The outcome of one trial subprocess.
    struct Trial {
        loops: u64,
        pass: bool,
        kernel_ns: Option<u64>,
        wall_ns: u64,
        failure: Option<String>,
    }

    impl Trial {
        fn to_json(&self) -> Value {
            json!({
                "loops": self.loops,
                "pass": self.pass,
                "kernel_ns": self.kernel_ns,
                "wall_ns": self.wall_ns,
                "failure": self.failure,
            })
        }
    }

    /// The last `max` bytes of `text`, on a character boundary.
    fn tail(text: &str, max: usize) -> &str {
        let mut start = text.len().saturating_sub(max);
        while !text.is_char_boundary(start) {
            start += 1;
        }
        &text[start..]
    }

    /// Runs one trial in a fresh subprocess with [`TRIAL_TIMEOUT`]. A non-zero exit, a timeout,
    /// or output that is not a trial report is a failure; its text is kept.
    fn run_trial(path: MetalPath, loops: u64) -> Res<Trial> {
        let exe = std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
        let start = Instant::now();
        let mut child = Command::new(&exe)
            .args(["--path", path.arg(), "--watchdog-trial", &loops.to_string()])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("spawn {}: {e}", exe.display()))?;
        let mut stdout = child.stdout.take().ok_or("no trial stdout")?;
        let mut stderr = child.stderr.take().ok_or("no trial stderr")?;
        let out_reader = std::thread::spawn(move || {
            let mut s = String::new();
            let _ = stdout.read_to_string(&mut s);
            s
        });
        let err_reader = std::thread::spawn(move || {
            let mut s = String::new();
            let _ = stderr.read_to_string(&mut s);
            s
        });
        let mut timed_out = false;
        let status = loop {
            if let Some(status) = child.try_wait().map_err(|e| format!("wait trial: {e}"))? {
                break Some(status);
            }
            if start.elapsed() >= TRIAL_TIMEOUT {
                timed_out = true;
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        let wall_ns = start.elapsed().as_nanos() as u64;
        let out = out_reader.join().unwrap_or_default();
        let err = err_reader.join().unwrap_or_default();
        let parsed: Option<Value> = serde_json::from_str(out.trim()).ok();
        let kernel_ns = parsed
            .as_ref()
            .and_then(|v| v.get("kernel_ns"))
            .and_then(Value::as_u64);
        let failure = match (timed_out, status) {
            (true, _) => Some(format!(
                "wall timeout after {} s; killed. stderr: {}",
                TRIAL_TIMEOUT.as_secs(),
                tail(err.trim(), 2000)
            )),
            (false, Some(st)) if !st.success() => {
                Some(format!("exit {st}; stderr: {}", tail(err.trim(), 2000)))
            }
            (false, _) if kernel_ns.is_none() => Some(format!(
                "no trial report on stdout; stderr: {}",
                tail(err.trim(), 2000)
            )),
            _ => None,
        };
        Ok(Trial {
            loops,
            pass: failure.is_none(),
            kernel_ns,
            wall_ns,
            failure,
        })
    }

    /// The watchdog search: trials from [`WATCHDOG_START_LOOPS`], doubling until one fails (or
    /// `max_loops` is passed), then a binary search between the last pass and the first fail until
    /// the bracket is within [`SEARCH_TOLERANCE_PCT`] percent.
    fn watchdog_search(path: MetalPath, max_loops: Option<u64>) -> Res<Value> {
        let mut trials: Vec<Trial> = Vec::new();
        let mut log = |t: Trial| {
            eprintln!(
                "watchdog: loops {} {} (kernel {:?} ns, wall {} ns){}",
                t.loops,
                if t.pass { "pass" } else { "FAIL" },
                t.kernel_ns,
                t.wall_ns,
                t.failure
                    .as_deref()
                    .map(|f| format!(": {f}"))
                    .unwrap_or_default()
            );
            let pass = t.pass;
            trials.push(t);
            pass
        };
        let mut lo: Option<u64> = None;
        let mut hi: Option<u64> = None;
        let mut loops = WATCHDOG_START_LOOPS;
        let mut reached_cap = false;
        loop {
            if max_loops.is_some_and(|cap| loops > cap) {
                reached_cap = true;
                break;
            }
            if loops > u64::from(u32::MAX) {
                return Err("watchdog: no failure below 2^32 loops".into());
            }
            if log(run_trial(path, loops)?) {
                lo = Some(loops);
                loops *= 2;
            } else {
                hi = Some(loops);
                break;
            }
        }
        if let Some(mut fail) = hi {
            let mut pass = lo.unwrap_or(0);
            while fail - pass > 1 && (fail - pass) * 100 > pass * SEARCH_TOLERANCE_PCT {
                let mid = pass + (fail - pass) / 2;
                if log(run_trial(path, mid)?) {
                    pass = mid;
                } else {
                    fail = mid;
                }
            }
            lo = (pass > 0).then_some(pass);
            hi = Some(fail);
        }
        let at = |loops: Option<u64>| {
            loops.and_then(|l| {
                trials
                    .iter()
                    .rev()
                    .find(|t| t.loops == l)
                    .map(Trial::to_json)
            })
        };
        let limit_trial = at(if hi.is_some() { lo } else { None });
        let failing_trial = at(hi);
        let limit_kernel_ns = limit_trial
            .as_ref()
            .and_then(|t| t.get("kernel_ns").cloned())
            .unwrap_or(Value::Null);
        let failure_mode = failing_trial
            .as_ref()
            .and_then(|t| t.get("failure").cloned())
            .unwrap_or(Value::Null);
        Ok(json!({
            "path": path.name(),
            "kernel": "mont_loop_u32",
            "elements": WATCHDOG_ELEMENTS,
            "trial_timeout_s": TRIAL_TIMEOUT.as_secs(),
            "start_loops": WATCHDOG_START_LOOPS,
            "max_loops": max_loops,
            "reached_cap": reached_cap,
            "tolerance_pct": SEARCH_TOLERANCE_PCT,
            "limit_loops": if hi.is_some() { lo.map(Value::from).unwrap_or(Value::Null) } else { Value::Null },
            "limit_kernel_ns": limit_kernel_ns,
            "passing_trial": limit_trial,
            "failing_trial": failing_trial,
            "failure_mode": failure_mode,
            "trials": trials.iter().map(Trial::to_json).collect::<Vec<_>>(),
        }))
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn neg_p_inv_inverts_p() {
            assert_eq!(P.wrapping_mul(NEG_P_INV), u32::MAX);
        }

        #[test]
        fn mont_mul_cpu_matches_reference() {
            let r_inv = r_inv_mod_p();
            let mut inputs = Inputs::new();
            for _ in 0..100_000 {
                let (a, b) = (inputs.next_u32() % P, inputs.next_u32() % P);
                assert_eq!(mont_mul_cpu(a, b), mont_ref(a, b, r_inv), "{a} {b}");
            }
            for (a, b) in [(0, 0), (P - 1, P - 1), (1, P - 1), (0, P - 1)] {
                assert_eq!(mont_mul_cpu(a, b), mont_ref(a, b, r_inv), "{a} {b}");
            }
        }

        #[test]
        fn limb16_matches_wide_multiply() {
            let mut inputs = Inputs::new();
            for _ in 0..100_000 {
                let (a, b) = (inputs.next_u32(), inputs.next_u32());
                assert_eq!(
                    mulhi_limb16_cpu(a, b),
                    ((u64::from(a) * u64::from(b)) >> 32) as u32
                );
            }
            assert_eq!(mulhi_limb16_cpu(u32::MAX, u32::MAX), u32::MAX - 1);
        }

        #[test]
        fn fast_math_lanes_discriminate() {
            let fma = fma_lanes();
            assert_eq!(
                fma[0].a * fma[0].b + fma[0].c,
                fma[0].a.mul_add(fma[0].b, fma[0].c)
            );
            for l in &fma[1..3] {
                assert_ne!((l.a * l.b + l.c).to_bits(), l.a.mul_add(l.b, l.c).to_bits());
            }
            let re = reassoc_lanes();
            assert_ne!(
                ((re[0].a + re[0].b) + re[0].c).to_bits(),
                (re[0].a + (re[0].b + re[0].c)).to_bits()
            );
        }

        #[test]
        fn stats_nearest_rank() {
            let mut v: Vec<u64> = (1..=30).rev().collect();
            let s = stats(&mut v);
            assert_eq!((s.min, s.p10, s.median, s.p90, s.max), (1, 3, 15, 27, 30));
        }
    }
}
