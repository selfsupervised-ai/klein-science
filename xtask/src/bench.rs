// provenance: original
//! `cargo xtask bench`: the one benchmark entry point (D-01), and the fixed-box manifest every
//! measurement carries (CONF-06, CLAUDE.md rule 5).
//!
//! - `bench box-manifest --box <label>` takes the box readings, [`admit`]s them and writes
//!   `conformance/boxes/<label>/box.json`: the JCS of a `ksci.box.v1` manifest.
//! - `bench probe metal --box <label>` builds `ksci-metal-probe`, re-takes and re-admits the
//!   readings, rewrites `box.json`, runs the probe on each Metal path under the heavy-job guard,
//!   and writes `conformance/boxes/<label>/probe-metal.json` (JCS) carrying `box_manifest_sha256`,
//!   the SHA-256 of the `box.json` bytes it was measured under.
//!
//! Admission refuses to record when the box is not on AC power, when a klein-rs build runs, when
//! the 1-minute load average is above 2.0 (2.0 itself is admitted), when the CPU speed limit is
//! below 100, or when any reading is missing or empty. Nothing is written on a refusal.
//!
//! The repository is public: the manifest holds the operator-chosen label, the model identifier,
//! the chip and the counts, never a serial number, hardware UUID, user name or host name. The GPU
//! reading takes two fields (name, core count) out of `system_profiler`'s display report, which
//! also lists the display's serial number; nothing else of that report is kept.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use clap::{Subcommand, ValueEnum};
use serde_json::{Map, Value, json};

use crate::Res;
use crate::host::{self, heavy_job};
use crate::workspace;

/// The manifest schema tag.
pub const BOX_SCHEMA: &str = "ksci.box.v1";
/// The Metal probe report schema tag.
pub const PROBE_METAL_SCHEMA: &str = "ksci.probe.metal.v1";
/// The QoS class the measured threads run under (assumption A9: macOS steers by QoS and cannot
/// hard-pin a thread to a P-core).
pub const QOS_CLASS: &str = "user-interactive";
/// The power source admission requires, as `pmset -g batt` names it.
pub const AC_POWER: &str = "AC Power";
/// The probe crate.
const PROBE_CRATE: &str = "ksci-metal-probe";
/// The cargo profile the probe is built with (root Cargo.toml).
const PROBE_PROFILE: &str = "probe";
/// The Metal paths, by the probe's `--path` value and its report key (the crate feature).
const METAL_PATHS: &[(&str, &str)] = &[
    ("wgpu-msl", "path-wgpu-msl"),
    ("metal-native", "path-metal-native"),
];

/// `cargo xtask bench ...`
#[derive(Subcommand)]
pub enum BenchCmd {
    /// Take the fixed-box readings, admit them, and write conformance/boxes/<label>/box.json.
    BoxManifest {
        /// The box label: lower-case letters, digits and `-`.
        #[arg(long = "box")]
        box_label: String,
    },
    /// Run a probe under a freshly admitted box manifest and write its report beside it.
    Probe {
        /// The probe.
        target: ProbeTarget,
        /// The box label: lower-case letters, digits and `-`.
        #[arg(long = "box")]
        box_label: String,
        /// One kernel (u32 mul_hi) per path: the measurement path end to end, nothing more.
        #[arg(long)]
        quick: bool,
    },
}

/// The probes `bench probe` runs.
#[derive(Clone, Copy, ValueEnum)]
pub enum ProbeTarget {
    /// The Metal GPU probe (ACCEL-01), macOS only.
    Metal,
}

/// Runs one `bench` command.
pub fn run(cmd: BenchCmd) -> Res<ExitCode> {
    match cmd {
        BenchCmd::BoxManifest { box_label } => {
            let root = workspace::root()?;
            check_label(&box_label)?;
            let (path, digest) = record_manifest(&root, &box_label)?;
            println!("bench: wrote {} (sha256:{digest})", rel(&root, &path));
            Ok(ExitCode::SUCCESS)
        }
        BenchCmd::Probe {
            target: ProbeTarget::Metal,
            box_label,
            quick,
        } => probe_metal(&box_label, quick).map(|()| ExitCode::SUCCESS),
    }
}

/// The readings a manifest is made of. `None` is a reading that could not be taken.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Readings {
    /// `sysctl hw.model`, e.g. `Mac15,11`.
    pub model_identifier: Option<String>,
    /// `sysctl machdep.cpu.brand_string`, e.g. `Apple M3 Max`.
    pub chip: Option<String>,
    /// `sysctl hw.perflevel0.physicalcpu`: performance cores.
    pub cpu_p_cores: Option<u32>,
    /// `sysctl hw.perflevel1.physicalcpu`: efficiency cores.
    pub cpu_e_cores: Option<u32>,
    /// `sysctl hw.memsize`, bytes.
    pub memory_bytes: Option<u64>,
    /// The GPU's name in `system_profiler SPDisplaysDataType`.
    pub gpu_name: Option<String>,
    /// The GPU's core count in `system_profiler SPDisplaysDataType`.
    pub gpu_cores: Option<u32>,
    /// `sw_vers -productVersion`.
    pub macos_version: Option<String>,
    /// `sw_vers -buildVersion`.
    pub macos_build: Option<String>,
    /// The first line of the pinned toolchain's `rustc -vV`.
    pub rustc: Option<String>,
    /// `git rev-parse HEAD` of the checkout.
    pub commit: Option<String>,
    /// The power source `pmset -g batt` reports, e.g. `AC Power`.
    pub power_source: Option<String>,
    /// The CPU speed limit (percent) `pmset -g therm` reports.
    pub thermal_cpu_speed_limit: Option<u32>,
    /// The 1-minute load average as `sysctl vm.loadavg` prints it (decimal text, kept exact).
    pub load_average_1m: Option<String>,
    /// Whether a klein-rs `cargo` or `rustc` runs ([`host::klein_rs_build_running`]).
    pub klein_rs_build_running: Option<bool>,
}

/// Why admission refused a set of readings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// A reading is missing or empty.
    Missing(&'static str),
    /// The box is not on AC power.
    NotOnAc(String),
    /// A klein-rs build is running.
    KleinRsBuild,
    /// The 1-minute load average is above 2.0.
    Load(String),
    /// The 1-minute load average is not a decimal number.
    BadLoad(String),
    /// The CPU speed limit is below 100.
    Thermal(u32),
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refusal::Missing(field) => write!(f, "reading {field} is missing or empty"),
            Refusal::NotOnAc(source) => {
                write!(
                    f,
                    "power source is {source:?}, not {AC_POWER:?}; connect AC power"
                )
            }
            Refusal::KleinRsBuild => write!(f, "a klein-rs build is running; wait for it to end"),
            Refusal::Load(load) => write!(
                f,
                "1-minute load average is {load}, above 2.0; close other work and rerun"
            ),
            Refusal::BadLoad(load) => write!(f, "1-minute load average {load:?} is not a decimal"),
            Refusal::Thermal(limit) => write!(f, "CPU speed limit is {limit}, below 100"),
        }
    }
}

/// A present, non-empty text reading.
fn text<'a>(field: &'static str, value: &'a Option<String>) -> Result<&'a str, Refusal> {
    match value.as_deref() {
        Some(v) if !v.trim().is_empty() => Ok(v),
        _ => Err(Refusal::Missing(field)),
    }
}

/// A present number reading.
fn num<T: Copy>(field: &'static str, value: &Option<T>) -> Result<T, Refusal> {
    value.ok_or(Refusal::Missing(field))
}

/// Whether the decimal text `load` is at most 2.0, compared exactly (no floating point).
/// `None` when `load` is not a plain decimal (`digits[.digits]`).
pub fn load_at_most_two(load: &str) -> Option<bool> {
    let (int, frac) = load.split_once('.').unwrap_or((load, ""));
    let digits = |s: &str| s.bytes().all(|b| b.is_ascii_digit());
    if int.is_empty() || !digits(int) || !digits(frac) || (load.contains('.') && frac.is_empty()) {
        return None;
    }
    let int = int.trim_start_matches('0');
    let frac_zero = frac.bytes().all(|b| b == b'0');
    Some(int.is_empty() || int == "1" || (int == "2" && frac_zero))
}

/// Fixed-box admission (CONF-06): every reading present and non-empty, AC power, no klein-rs
/// build, a 1-minute load of at most 2.0, and a CPU speed limit of 100.
pub fn admit(r: &Readings) -> Result<(), Refusal> {
    text("model_identifier", &r.model_identifier)?;
    text("chip", &r.chip)?;
    num("cpu_p_cores", &r.cpu_p_cores)?;
    num("cpu_e_cores", &r.cpu_e_cores)?;
    num("memory_bytes", &r.memory_bytes)?;
    text("gpu_name", &r.gpu_name)?;
    num("gpu_cores", &r.gpu_cores)?;
    text("macos_version", &r.macos_version)?;
    text("macos_build", &r.macos_build)?;
    text("rustc", &r.rustc)?;
    text("commit", &r.commit)?;
    let power = text("power_source", &r.power_source)?;
    let limit = num("thermal_cpu_speed_limit", &r.thermal_cpu_speed_limit)?;
    let load = text("load_average_1m", &r.load_average_1m)?;
    let klein = num("klein_rs_build_running", &r.klein_rs_build_running)?;
    if power != AC_POWER {
        return Err(Refusal::NotOnAc(power.to_owned()));
    }
    if klein {
        return Err(Refusal::KleinRsBuild);
    }
    match load_at_most_two(load) {
        None => return Err(Refusal::BadLoad(load.to_owned())),
        Some(false) => return Err(Refusal::Load(load.to_owned())),
        Some(true) => {}
    }
    if limit < 100 {
        return Err(Refusal::Thermal(limit));
    }
    Ok(())
}

/// The JCS bytes of the `ksci.box.v1` manifest of admitted readings.
pub fn manifest_bytes(label: &str, r: &Readings) -> Res<Vec<u8>> {
    admit(r).map_err(|why| format!("box {label}: refused to record: {why}"))?;
    let value = json!({
        "schema": BOX_SCHEMA,
        "box_label": label,
        "model_identifier": r.model_identifier,
        "chip": r.chip,
        "cpu_p_cores": r.cpu_p_cores,
        "cpu_e_cores": r.cpu_e_cores,
        "memory_bytes": r.memory_bytes,
        "gpu_name": r.gpu_name,
        "gpu_cores": r.gpu_cores,
        "macos_version": r.macos_version,
        "macos_build": r.macos_build,
        "rustc": r.rustc,
        "commit": r.commit,
        "power_source": r.power_source,
        "thermal_cpu_speed_limit": r.thermal_cpu_speed_limit,
        "load_average_1m": r.load_average_1m,
        "klein_rs_build_running": r.klein_rs_build_running,
        "qos_class": QOS_CLASS,
    });
    ksci_wire::to_jcs(&value).map_err(|e| format!("box manifest: {e}"))
}

/// Checks a box label: 1 to 64 of `[a-z0-9-]`, not starting with `-`. It names a directory.
fn check_label(label: &str) -> Res<()> {
    let ok = !label.is_empty()
        && label.len() <= 64
        && !label.starts_with('-')
        && label
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    if ok {
        Ok(())
    } else {
        Err(format!(
            "box label {label:?}: use 1 to 64 of a-z, 0-9 and -, not starting with -"
        ))
    }
}

/// `conformance/boxes/<label>` of the checkout.
fn box_dir(root: &Path, label: &str) -> PathBuf {
    root.join("conformance").join("boxes").join(label)
}

/// Takes the readings, admits them and writes `box.json`; returns its path and SHA-256.
fn record_manifest(root: &Path, label: &str) -> Res<(PathBuf, String)> {
    let readings = take_readings(root)?;
    let bytes = manifest_bytes(label, &readings)?;
    let dir = box_dir(root, label);
    fs::create_dir_all(&dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    let path = dir.join("box.json");
    fs::write(&path, &bytes).map_err(|e| format!("write {}: {e}", path.display()))?;
    Ok((path, ksci_wire::sha256_hex(&bytes)))
}

/// `path` relative to the checkout root, for messages.
fn rel(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
}

/// Runs a fixed-location system tool and returns its stdout, or `None` if it fails.
fn system_output(program: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(program).args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8(out.stdout).ok()
}

/// `sysctl -n <key>`, trimmed.
fn sysctl(key: &str) -> Option<String> {
    system_output("/usr/sbin/sysctl", &["-n", key]).map(|s| s.trim().to_owned())
}

/// The 1-minute figure of `sysctl -n vm.loadavg` (`{ 1.23 4.56 7.89 }`).
pub fn parse_loadavg(text: &str) -> Option<String> {
    text.trim()
        .trim_start_matches('{')
        .split_whitespace()
        .next()
        .map(str::to_owned)
}

/// The power source in `pmset -g batt` (`Now drawing from 'AC Power'`).
pub fn parse_power_source(text: &str) -> Option<String> {
    let line = text.lines().find(|l| l.contains("drawing from"))?;
    let start = line.find('\'')? + 1;
    let len = line[start..].find('\'')?;
    Some(line[start..start + len].to_owned())
}

/// The CPU speed limit in `pmset -g therm`: the `CPU_Speed_Limit = N` line when macOS has
/// recorded a CPU power status, and 100 when it reports `No CPU power status has been recorded`
/// (no thermal or power limit has been applied since boot). Anything else is `None`.
pub fn parse_cpu_speed_limit(text: &str) -> Option<u32> {
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("CPU_Speed_Limit") {
            return rest.trim().trim_start_matches('=').trim().parse().ok();
        }
    }
    text.contains("No CPU power status has been recorded")
        .then_some(100)
}

/// The GPU name and core count in `system_profiler SPDisplaysDataType -json`: the first entry
/// whose device type is a GPU. Only these two fields are read.
pub fn parse_gpu(text: &str) -> (Option<String>, Option<u32>) {
    let Ok(doc) = serde_json::from_str::<Value>(text) else {
        return (None, None);
    };
    let gpu = doc
        .get("SPDisplaysDataType")
        .and_then(Value::as_array)
        .and_then(|items| {
            items.iter().find(|i| {
                i.get("sppci_device_type").and_then(Value::as_str) == Some("spdisplays_gpu")
            })
        });
    let Some(gpu) = gpu else {
        return (None, None);
    };
    let name = gpu
        .get("sppci_model")
        .or_else(|| gpu.get("_name"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    let cores = gpu
        .get("sppci_cores")
        .and_then(Value::as_str)
        .and_then(|c| c.parse().ok());
    (name, cores)
}

/// Takes every reading on this box. A reading that cannot be taken is `None` (and refused).
fn take_readings(root: &Path) -> Res<Readings> {
    let (gpu_name, gpu_cores) = system_output(
        "/usr/sbin/system_profiler",
        &["SPDisplaysDataType", "-json"],
    )
    .map(|t| parse_gpu(&t))
    .unwrap_or((None, None));
    let rustc = host::toolchain_bin("rustc").ok().and_then(|rustc| {
        let out = Command::new(rustc).arg("-vV").output().ok()?;
        let text = String::from_utf8(out.stdout).ok()?;
        out.status
            .success()
            .then(|| text.lines().next().unwrap_or("").trim().to_owned())
    });
    let commit = host::git_output(root, &["rev-parse", "HEAD"])
        .ok()
        .and_then(|b| String::from_utf8(b).ok())
        .map(|s| s.trim().to_owned());
    Ok(Readings {
        model_identifier: sysctl("hw.model"),
        chip: sysctl("machdep.cpu.brand_string"),
        cpu_p_cores: sysctl("hw.perflevel0.physicalcpu").and_then(|s| s.parse().ok()),
        cpu_e_cores: sysctl("hw.perflevel1.physicalcpu").and_then(|s| s.parse().ok()),
        memory_bytes: sysctl("hw.memsize").and_then(|s| s.parse().ok()),
        gpu_name,
        gpu_cores,
        macos_version: system_output("/usr/bin/sw_vers", &["-productVersion"])
            .map(|s| s.trim().to_owned()),
        macos_build: system_output("/usr/bin/sw_vers", &["-buildVersion"])
            .map(|s| s.trim().to_owned()),
        rustc,
        commit,
        power_source: system_output("/usr/bin/pmset", &["-g", "batt"])
            .and_then(|t| parse_power_source(&t)),
        thermal_cpu_speed_limit: system_output("/usr/bin/pmset", &["-g", "therm"])
            .and_then(|t| parse_cpu_speed_limit(&t)),
        load_average_1m: sysctl("vm.loadavg").and_then(|t| parse_loadavg(&t)),
        klein_rs_build_running: Some(host::klein_rs_build_running()?),
    })
}

/// The cubecl version in Cargo.lock: the version the probe was built against.
fn locked_version(root: &Path, krate: &str) -> Res<String> {
    let path = root.join("Cargo.lock");
    let text = fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let lock: toml::Table = toml::from_str(&text).map_err(|e| format!("Cargo.lock: {e}"))?;
    let versions: Vec<&str> = lock
        .get("package")
        .and_then(|p| p.as_array())
        .into_iter()
        .flatten()
        .filter(|p| p.get("name").and_then(|n| n.as_str()) == Some(krate))
        .filter_map(|p| p.get("version").and_then(|v| v.as_str()))
        .collect();
    match versions.as_slice() {
        [one] => Ok((*one).to_owned()),
        other => Err(format!(
            "Cargo.lock lists {krate} {other:?}; expected one version"
        )),
    }
}

/// `bench probe metal`: build, admit, measure each path, write the report.
fn probe_metal(label: &str, quick: bool) -> Res<()> {
    check_label(label)?;
    if !cfg!(target_os = "macos") {
        return Err("bench probe metal runs on macOS only".to_owned());
    }
    let root = workspace::root()?;
    let cargo = host::cargo()?;
    let _held = heavy_job()?;

    // Build first: the build's own load must not be what admission measures, and a refused
    // run reruns from the built binary.
    let target = root.join("target");
    let status = cargo
        .command()
        .current_dir(&root)
        .args([
            "build",
            "--locked",
            "--profile",
            PROBE_PROFILE,
            "-p",
            PROBE_CRATE,
        ])
        .args([
            "--features",
            "path-wgpu-msl,path-metal-native",
            "--target-dir",
        ])
        .arg(&target)
        .status()
        .map_err(|e| format!("run cargo build -p {PROBE_CRATE}: {e}"))?;
    if !status.success() {
        return Err(format!("cargo build -p {PROBE_CRATE}: {status}"));
    }
    let exe = target.join(PROBE_PROFILE).join(PROBE_CRATE);

    let (box_path, box_digest) = record_manifest(&root, label)?;
    println!(
        "bench: admitted; wrote {} (sha256:{box_digest})",
        rel(&root, &box_path)
    );

    let mut report = Map::new();
    report.insert("schema".into(), json!(PROBE_METAL_SCHEMA));
    report.insert("box_label".into(), json!(label));
    report.insert("box_manifest_sha256".into(), json!(box_digest));
    report.insert(
        "cubecl_version".into(),
        json!(locked_version(&root, "cubecl")?),
    );
    report.insert(
        "mode".into(),
        json!(if quick { "quick" } else { "standard" }),
    );
    for (arg, key) in METAL_PATHS {
        let mut args = vec!["--path", arg];
        if quick {
            args.push("--quick");
        }
        println!("bench: probe {key}");
        report.insert((*key).into(), run_probe(&exe, &args)?);
    }

    let bytes = ksci_wire::to_jcs(&Value::Object(report)).map_err(|e| format!("report: {e}"))?;
    let out = box_dir(&root, label).join("probe-metal.json");
    fs::write(&out, &bytes).map_err(|e| format!("write {}: {e}", out.display()))?;
    println!(
        "bench: wrote {} (sha256:{}), measured under box sha256:{box_digest}",
        rel(&root, &out),
        ksci_wire::sha256_hex(&bytes)
    );
    Ok(())
}

/// Runs the probe binary once and parses the JSON it prints; a non-zero exit is an error that
/// carries its stderr (a NOT EXACT kernel among them).
fn run_probe(exe: &Path, args: &[&str]) -> Res<Value> {
    let out = Command::new(exe)
        .args(args)
        .output()
        .map_err(|e| format!("run {}: {e}", exe.display()))?;
    if !out.status.success() {
        return Err(format!(
            "{PROBE_CRATE} {}: {}: {}",
            args.join(" "),
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    serde_json::from_slice(&out.stdout)
        .map_err(|e| format!("{PROBE_CRATE} {}: output is not JSON: {e}", args.join(" ")))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Readings that admission accepts.
    fn good() -> Readings {
        Readings {
            model_identifier: Some("Mac15,11".into()),
            chip: Some("Apple M3 Max".into()),
            cpu_p_cores: Some(10),
            cpu_e_cores: Some(4),
            memory_bytes: Some(38_654_705_664),
            gpu_name: Some("Apple M3 Max".into()),
            gpu_cores: Some(30),
            macos_version: Some("26.5.2".into()),
            macos_build: Some("25F84".into()),
            rustc: Some("rustc 1.99.0 (b940084d7 2026-09-28)".into()),
            commit: Some("229fc4c86235b54a2c41b5e004faa51bdf6383c5".into()),
            power_source: Some(AC_POWER.into()),
            thermal_cpu_speed_limit: Some(100),
            load_average_1m: Some("1.25".into()),
            klein_rs_build_running: Some(false),
        }
    }

    #[test]
    fn box_manifest_admits_complete_readings() {
        assert_eq!(admit(&good()), Ok(()));
        let bytes = manifest_bytes("mac-m3max", &good()).unwrap();
        let text = String::from_utf8(bytes).unwrap();
        assert!(text.contains(r#""schema":"ksci.box.v1""#), "{text}");
        assert!(text.contains(r#""qos_class":"user-interactive""#), "{text}");
    }

    #[test]
    fn box_manifest_refuses_battery() {
        let r = Readings {
            power_source: Some("Battery Power".into()),
            ..good()
        };
        assert_eq!(admit(&r), Err(Refusal::NotOnAc("Battery Power".into())));
        assert!(manifest_bytes("mac-m3max", &r).is_err());
    }

    #[test]
    fn box_manifest_refuses_klein_rs_build() {
        let r = Readings {
            klein_rs_build_running: Some(true),
            ..good()
        };
        assert_eq!(admit(&r), Err(Refusal::KleinRsBuild));
    }

    #[test]
    fn box_manifest_load_two_admitted_above_refused() {
        for admitted in ["2.0", "2.00", "2", "0.00", "1.99", "02.00"] {
            let r = Readings {
                load_average_1m: Some(admitted.into()),
                ..good()
            };
            assert_eq!(admit(&r), Ok(()), "{admitted}");
        }
        for refused in ["2.01", "2.001", "3.18", "10.00"] {
            let r = Readings {
                load_average_1m: Some(refused.into()),
                ..good()
            };
            assert_eq!(admit(&r), Err(Refusal::Load(refused.into())), "{refused}");
        }
        for bad in ["-1", "2.", ".5", "2,00", "nan", "1e0"] {
            let r = Readings {
                load_average_1m: Some(bad.into()),
                ..good()
            };
            assert_eq!(admit(&r), Err(Refusal::BadLoad(bad.into())), "{bad}");
        }
    }

    #[test]
    fn box_manifest_refuses_thermal_limit() {
        let r = Readings {
            thermal_cpu_speed_limit: Some(99),
            ..good()
        };
        assert_eq!(admit(&r), Err(Refusal::Thermal(99)));
    }

    #[test]
    fn box_manifest_refuses_missing_or_empty_field() {
        type Clear = fn(&mut Readings);
        let cases: &[(&str, Clear)] = &[
            ("model_identifier", |r| r.model_identifier = None),
            ("chip", |r| r.chip = None),
            ("cpu_p_cores", |r| r.cpu_p_cores = None),
            ("cpu_e_cores", |r| r.cpu_e_cores = None),
            ("memory_bytes", |r| r.memory_bytes = None),
            ("gpu_name", |r| r.gpu_name = None),
            ("gpu_cores", |r| r.gpu_cores = None),
            ("macos_version", |r| r.macos_version = None),
            ("macos_build", |r| r.macos_build = None),
            ("rustc", |r| r.rustc = None),
            ("commit", |r| r.commit = None),
            ("power_source", |r| r.power_source = None),
            ("thermal_cpu_speed_limit", |r| {
                r.thermal_cpu_speed_limit = None
            }),
            ("load_average_1m", |r| r.load_average_1m = None),
            ("klein_rs_build_running", |r| {
                r.klein_rs_build_running = None
            }),
            // Empty text is refused like a missing reading (an unknown power source, say).
            ("power_source", |r| r.power_source = Some(String::new())),
            ("chip", |r| r.chip = Some("  ".into())),
            ("commit", |r| r.commit = Some(String::new())),
        ];
        for (field, clear) in cases {
            let mut r = good();
            clear(&mut r);
            assert_eq!(admit(&r), Err(Refusal::Missing(field)), "{field}");
            assert!(manifest_bytes("mac-m3max", &r).is_err(), "{field}");
        }
    }

    #[test]
    fn box_manifest_identical_readings_identical_bytes() {
        let a = manifest_bytes("mac-m3max", &good()).unwrap();
        let b = manifest_bytes("mac-m3max", &good()).unwrap();
        assert_eq!(a, b);
        assert_eq!(ksci_wire::sha256_hex(&a), ksci_wire::sha256_hex(&b));
        // JCS: keys in sorted order, so the bytes do not depend on construction order.
        let text = String::from_utf8(a).unwrap();
        let keys: Vec<&str> = text
            .trim_matches(|c| c == '{' || c == '}')
            .split(",\"")
            .map(|kv| kv.trim_start_matches('"').split('"').next().unwrap())
            .collect();
        let mut sorted = keys.clone();
        sorted.sort_unstable();
        assert_eq!(keys, sorted);
        let other = manifest_bytes(
            "mac-m3max",
            &Readings {
                load_average_1m: Some("1.26".into()),
                ..good()
            },
        )
        .unwrap();
        assert_ne!(text.as_bytes(), other.as_slice());
    }

    #[test]
    fn box_manifest_has_no_serial_or_uuid() {
        let text = String::from_utf8(manifest_bytes("mac-m3max", &good()).unwrap()).unwrap();
        let lower = text.to_lowercase();
        assert!(
            !lower.contains("serial") && !lower.contains("uuid"),
            "{text}"
        );
    }

    #[test]
    fn box_manifest_parses_readings() {
        assert_eq!(parse_loadavg("{ 3.18 4.26 4.83 }\n"), Some("3.18".into()));
        assert_eq!(parse_loadavg(""), None);
        let batt = "Now drawing from 'AC Power'\n -InternalBattery-0 (id=1)\t100%; charged\n";
        assert_eq!(parse_power_source(batt), Some("AC Power".into()));
        let batt = "Now drawing from 'Battery Power'\n";
        assert_eq!(parse_power_source(batt), Some("Battery Power".into()));
        assert_eq!(parse_power_source("garbage"), None);
        let therm = "Note: No thermal warning level has been recorded\nNote: No CPU power status has been recorded\n";
        assert_eq!(parse_cpu_speed_limit(therm), Some(100));
        let therm = "CPU Power notify\n\tCPU_Scheduler_Limit \t= 100\n\tCPU_Available_CPUs \t= 14\n\tCPU_Speed_Limit \t= 72\n";
        assert_eq!(parse_cpu_speed_limit(therm), Some(72));
        assert_eq!(parse_cpu_speed_limit(""), None);
        let displays = r#"{"SPDisplaysDataType":[{"_name":"Apple M3 Max","spdisplays_ndrvs":[{"_spdisplays_display-serial-number":"x"}],"sppci_cores":"30","sppci_device_type":"spdisplays_gpu","sppci_model":"Apple M3 Max"}]}"#;
        assert_eq!(parse_gpu(displays), (Some("Apple M3 Max".into()), Some(30)));
        assert_eq!(parse_gpu("{}"), (None, None));
    }

    #[test]
    fn box_manifest_label_is_a_plain_name() {
        assert!(check_label("mac-m3max").is_ok());
        for bad in ["", "-x", "../x", "Mac", "a/b", "a b", &"a".repeat(65)] {
            assert!(check_label(bad).is_err(), "{bad}");
        }
    }
}
