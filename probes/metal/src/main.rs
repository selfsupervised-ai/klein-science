// provenance: original
//! `ksci-metal-probe`: the unshipped Metal risk probe (ACCEL-01), run by
//! `cargo xtask bench probe metal`, which attaches the fixed-box manifest digest.
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
    use std::path::{Path, PathBuf};
    use std::process::ExitCode;
    use std::time::Instant;

    use cubecl::client::Client;
    use cubecl::config::RuntimeConfig;
    use cubecl::config::cache::CacheConfig;
    use cubecl::config::{CubeClRuntimeConfig, environment::EnvironmentConfig};
    use cubecl::device::{MetalDevice, WgpuDeviceKind};
    use cubecl::prelude::*;
    use cubecl::records::RecordLevel;
    use cubecl::server::CubeCountSelection;
    use cubecl::{CubeElement, Device};
    use serde_json::{Value, json};

    use crate::kernels;

    /// Elements per kernel launch: 2^24 input pairs.
    pub const ELEMENTS: usize = 1 << 24;

    /// Warm dispatches timed per kernel.
    pub const WARM_RUNS: usize = 30;

    /// Units per cube. 256 divides [`ELEMENTS`] and is within every Metal limit.
    const CUBE_DIM: u32 = 256;

    /// The fixed seed of the input generator. The probe is unshipped, so DET-08 does not apply,
    /// but its inputs are reproducible.
    const SEED: u64 = 0x9E37_79B9_7F4A_7C15;

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

    /// Parsed command line.
    struct Args {
        path: MetalPath,
        quick: bool,
    }

    fn parse_args() -> Res<Args> {
        let mut path = None;
        let mut quick = false;
        let mut it = std::env::args().skip(1);
        while let Some(arg) = it.next() {
            match arg.as_str() {
                "--path" => {
                    let v = it.next().ok_or("--path needs a value")?;
                    path = Some(MetalPath::parse(&v)?);
                }
                "--quick" => quick = true,
                other => return Err(format!("unknown argument {other:?}")),
            }
        }
        Ok(Args {
            path: path.ok_or("--path wgpu-msl|metal-native is required")?,
            quick,
        })
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
        let args = parse_args()?;
        let env_root = scratch_env_root()?;
        pin_cubecl_config(&env_root);
        let result = measure_path(args.path, args.quick);
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
    }

    /// Times one kernel: one cold dispatch, the bit-exact check of its output, then
    /// [`WARM_RUNS`] warm dispatches and a second check of the output.
    fn time_kernel<T: CubeElement + PartialEq + std::fmt::Debug>(
        client: &Client,
        name: &str,
        elements: usize,
        output: &cubecl::server::Handle,
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
        let ops_per_s = (elements as u128 * 1_000_000_000 / u128::from(stats.median.max(1))) as u64;
        Ok(json!({
            "exact": true,
            "elements": elements,
            "cold_ns": cold_ns,
            "warm_ns": stats.to_json(),
            "ops_per_s": ops_per_s,
        }))
    }

    fn sync(client: &Client) -> Res<()> {
        cubecl::future::block_on(client.sync()).map_err(|e| format!("sync: {e:?}"))
    }

    /// Reads `output` back and compares it word for word with `expected`.
    fn check_exact<T: CubeElement + PartialEq + std::fmt::Debug>(
        client: &Client,
        name: &str,
        output: &cubecl::server::Handle,
        expected: &[T],
    ) -> Res<()> {
        let bytes = client
            .read_one(output.clone())
            .map_err(|e| format!("{name}: read back: {e:?}"))?;
        let actual = T::from_bytes(&bytes);
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

    /// One path: device creation, then each kernel.
    fn measure_path(path: MetalPath, quick: bool) -> Res<Value> {
        let start = Instant::now();
        let device = path.device()?;
        let client = device.client();
        let init_ns = start.elapsed().as_nanos() as u64;

        let mut kernels_out = serde_json::Map::new();
        kernels_out.insert("mulhi_u32".into(), probe_mulhi_u32(&client)?);

        Ok(json!({
            "path": path.name(),
            "device_init_ns": init_ns,
            "elements": ELEMENTS,
            "warm_runs": WARM_RUNS,
            "cube_dim": CUBE_DIM,
            "mode": if quick { "quick" } else { "standard" },
            "inputs": "xorshift64* from seed 0x9e3779b97f4a7c15",
            "timing": "wall clock around launch and client.sync(); cold = first dispatch of the kernel in the process, compile included",
            "kernels": Value::Object(kernels_out),
        }))
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
        let lhs_h = client.create_from_slice(u32::as_bytes(&lhs));
        let rhs_h = client.create_from_slice(u32::as_bytes(&rhs));
        let out_h = client.empty(ELEMENTS * size_of::<u32>());
        let count = cube_count(client, ELEMENTS);
        let launch = || unsafe {
            kernels::mulhi_u32::launch_unchecked(
                client,
                count.clone(),
                CubeDim::new_1d(CUBE_DIM),
                BufferArg::from_raw_parts(lhs_h.clone(), ELEMENTS),
                BufferArg::from_raw_parts(rhs_h.clone(), ELEMENTS),
                BufferArg::from_raw_parts(out_h.clone(), ELEMENTS),
            )
        };
        time_kernel(client, "mulhi_u32", ELEMENTS, &out_h, &expected, &launch)
    }
}
