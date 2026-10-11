// provenance: original
//! The offline oracle (CONF-01): pinned images and the one hardened container runner (D-01).
//!
//! Untrusted programs (corpus programs, stand-in runs, census probes) run only through
//! [`run`]: a `docker run` with no network, a read-only root, a size-capped `/tmp`, no
//! capabilities, no privilege gain, capped pids and memory, and uid 65534. Repository inputs are
//! mounted read-only; the only writable host path is a fresh output directory. Every run first
//! checks that the local image is the one pinned in `conformance/oracles/images.json` for that
//! platform, and holds [`host::heavy_job`] for its duration.
//!
//! The outputs are canonicalised on the host with [`ksci_wire::to_jcs`] (each line of a `.jsonl`
//! file, each `.json` file whole) and stored by digest in the artifact store, so neither the
//! worker count inside the container nor the order cases finish in changes the stored bytes.
//!
//! `images.json` is written only here, as the JCS bytes of [`Pins`].

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::{ExitCode, Stdio};

use clap::{Args, Subcommand};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::Res;
use crate::host;
use crate::store::{self, Cache, Remote};
use crate::workspace;

/// The pinned base image: `python:3.13-slim-trixie` by its multi-platform index digest.
pub const BASE_REFERENCE: &str = "python:3.13-slim-trixie@sha256:70729b46c69b4f1e97c4822c1af3df53a1476cf5ddc6c087c0c10bc3a5678c2f";
/// The image pins, relative to the checkout root.
pub const PINS_PATH: &str = "conformance/oracles/images.json";
/// The schema tag of `images.json`.
pub const PINS_SCHEMA: &str = "ksci.oracle-images.v1";
/// The platforms an oracle image is built and run for (D-10).
pub const PLATFORMS: &[&str] = &["linux/arm64", "linux/amd64"];
/// Where the checkout's `conformance/` directory is mounted (read-only).
const CONFORMANCE_MOUNT: &str = "/opt/ksci/conformance";
/// Where the checkout's `standin/` directory is mounted (read-only), when requested.
const STANDIN_MOUNT: &str = "/opt/ksci/standin";

/// `cargo xtask oracle ...`
#[derive(Subcommand)]
pub enum OracleCmd {
    /// Pull (`base`) an oracle image for each platform and pin the identity docker reports.
    BuildImage {
        /// Image name: a key of conformance/oracles/images.json.
        name: String,
        /// Platforms to pin (repeatable), e.g. linux/arm64.
        #[arg(long = "platform", required = true)]
        platforms: Vec<String>,
    },
    /// Run a script in a pinned oracle image inside the hardened container, then canonicalise
    /// and store its outputs.
    Run(RunArgs),
}

/// `cargo xtask oracle run ...`
#[derive(Args)]
pub struct RunArgs {
    /// Pinned image name (a key of conformance/oracles/images.json).
    #[arg(long)]
    pub image: String,
    /// Platform to run, e.g. linux/arm64.
    #[arg(long)]
    pub platform: String,
    /// The script, relative to the checkout: under conformance/ (or standin/ with
    /// --mount-standin).
    #[arg(long)]
    pub script: PathBuf,
    /// An input file: a path, or the sha256 of a blob in the artifact store (repeatable).
    #[arg(long = "input")]
    pub inputs: Vec<String>,
    /// PYTHONHASHSEED for the run.
    #[arg(long, default_value_t = 0)]
    pub seed: u32,
    /// Passed to the script as --jobs.
    #[arg(long)]
    pub jobs: Option<u32>,
    /// Passed to the script as --cpu-seconds.
    #[arg(long)]
    pub cpu_seconds: Option<u32>,
    /// Passed to the script as --prelude (a dotted module name).
    #[arg(long)]
    pub prelude: Option<String>,
    /// Also mount the checkout's standin/ read-only at /opt/ksci/standin.
    #[arg(long)]
    pub mount_standin: bool,
    /// Passed to the script as --require-sympy-env.
    #[arg(long)]
    pub require_sympy_env: bool,
    /// Further arguments for the script (after `--`).
    #[arg(last = true)]
    pub script_args: Vec<String>,
}

/// Runs one `oracle` command.
pub fn run(cmd: OracleCmd) -> Res<ExitCode> {
    match cmd {
        OracleCmd::BuildImage { name, platforms } => {
            build_image(&name, &platforms).map(|()| ExitCode::SUCCESS)
        }
        OracleCmd::Run(args) => run_script(&args).map(|(code, _)| code),
    }
}

/// `conformance/oracles/images.json`.
#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Pins {
    /// [`PINS_SCHEMA`].
    pub schema: String,
    /// Image name to pin.
    pub images: BTreeMap<String, ImagePin>,
}

/// One oracle image.
#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ImagePin {
    /// The reference docker runs (a digest reference, or a local tag for a built image).
    pub reference: String,
    /// Platform (`linux/arm64`) to its pin.
    pub platforms: BTreeMap<String, PlatformPin>,
    /// sha256 of the stored introspection table (the sympy image).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub introspection_sha256: Option<String>,
}

/// One platform of an oracle image.
#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PlatformPin {
    /// The identity `docker image inspect --platform <p>` reports (`sha256:<hex>`).
    pub identity: String,
    /// Whether the platform was pinned under emulation on the pinning host.
    pub emulated: bool,
    /// sha256 of the saved image tar in the artifact store (images built locally).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tar_sha256: Option<String>,
}

impl Pins {
    fn empty() -> Pins {
        Pins {
            schema: PINS_SCHEMA.to_owned(),
            images: BTreeMap::new(),
        }
    }

    /// Parses `images.json` bytes; the schema tag must match.
    pub fn parse(bytes: &[u8]) -> Res<Pins> {
        let pins: Pins = serde_json::from_slice(bytes).map_err(|e| format!("{PINS_PATH}: {e}"))?;
        if pins.schema != PINS_SCHEMA {
            return Err(format!(
                "{PINS_PATH}: schema {:?}, expected {PINS_SCHEMA:?}",
                pins.schema
            ));
        }
        Ok(pins)
    }

    /// The JCS bytes written to `images.json` (the one writer).
    pub fn to_bytes(&self) -> Res<Vec<u8>> {
        ksci_wire::to_jcs(self).map_err(|e| format!("encode {PINS_PATH}: {e}"))
    }
}

fn load_pins(root: &Path) -> Res<Pins> {
    let path = root.join(PINS_PATH);
    match fs::read(&path) {
        Ok(bytes) => Pins::parse(&bytes),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Pins::empty()),
        Err(e) => Err(format!("read {}: {e}", path.display())),
    }
}

fn save_pins(root: &Path, pins: &Pins) -> Res<()> {
    let path = root.join(PINS_PATH);
    let partial = path.with_extension(format!("json.partial.{}", std::process::id()));
    fs::write(&partial, pins.to_bytes()?)
        .map_err(|e| format!("write {}: {e}", partial.display()))?;
    fs::rename(&partial, &path).map_err(|e| format!("rename into {}: {e}", path.display()))
}

fn check_platform(platform: &str) -> Res<()> {
    if PLATFORMS.contains(&platform) {
        Ok(())
    } else {
        Err(format!(
            "unsupported platform {platform:?}; expected one of {}",
            PLATFORMS.join(", ")
        ))
    }
}

/// Runs the pinned docker with `args` and returns its trimmed stdout; a non-zero exit is an error.
fn docker_output(args: &[&str]) -> Res<String> {
    let out = host::docker()?
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("run docker {}: {e}", args.join(" ")))?;
    if !out.status.success() {
        return Err(format!(
            "docker {}: {}: {}",
            args.join(" "),
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    String::from_utf8(out.stdout)
        .map(|s| s.trim().to_owned())
        .map_err(|e| format!("docker {}: {e}", args.join(" ")))
}

/// The identity docker reports for `reference` on `platform`: the platform's manifest digest.
fn image_identity(reference: &str, platform: &str) -> Res<String> {
    let id = docker_output(&[
        "image",
        "inspect",
        "--platform",
        platform,
        "--format",
        "{{.Id}}",
        reference,
    ])?;
    match id.strip_prefix("sha256:") {
        Some(hex) if store::check_digest(hex).is_ok() => Ok(id),
        _ => Err(format!(
            "docker reports identity {id:?} for {reference} on {platform}; expected sha256:<hex>"
        )),
    }
}

/// The daemon's native platform (`linux/arm64` on the Mac), for the `emulated` flag.
fn native_platform() -> Res<String> {
    let arch = docker_output(&["info", "--format", "{{.Architecture}}"])?;
    let arch = match arch.as_str() {
        "aarch64" | "arm64" => "arm64",
        "x86_64" | "amd64" => "amd64",
        other => other,
    };
    Ok(format!("linux/{arch}"))
}

fn build_image(name: &str, platforms: &[String]) -> Res<()> {
    for p in platforms {
        check_platform(p)?;
    }
    if name != "base" {
        return Err(format!(
            "oracle build-image {name}: only `base` (the pinned python image) can be pinned so far"
        ));
    }
    let root = workspace::root()?;
    let native = native_platform()?;
    let mut pins = load_pins(&root)?;
    for platform in platforms {
        let status = host::docker()?
            .args(["pull", "--platform", platform, BASE_REFERENCE])
            .stdin(Stdio::null())
            .status()
            .map_err(|e| format!("run docker pull: {e}"))?;
        if !status.success() {
            return Err(format!(
                "docker pull {BASE_REFERENCE} ({platform}): {status}"
            ));
        }
        let identity = image_identity(BASE_REFERENCE, platform)?;
        println!("base {platform} identity={identity}");
        let image = pins
            .images
            .entry(name.to_owned())
            .or_insert_with(|| ImagePin {
                reference: BASE_REFERENCE.to_owned(),
                platforms: BTreeMap::new(),
                introspection_sha256: None,
            });
        image.reference = BASE_REFERENCE.to_owned();
        image.platforms.insert(
            platform.clone(),
            PlatformPin {
                identity,
                emulated: *platform != native,
                tar_sha256: None,
            },
        );
    }
    save_pins(&root, &pins)?;
    println!("wrote {PINS_PATH}");
    Ok(())
}

/// The hardening flags of every oracle container (T-01-13). Asserted by the acceptance greps and
/// by `oracle_run_flags_are_hardened`.
const HARDENING: &[&str] = &[
    "--rm",
    "--pull=never",
    "--init",
    "--network=none",
    "--read-only",
    "--tmpfs",
    "/tmp:rw,size=1g,mode=1777",
    "--cap-drop=ALL",
    "--security-opt=no-new-privileges",
    "--pids-limit=512",
    "--memory=8g",
    "--user",
    "65534:65534",
    "--hostname",
    "ksci-oracle",
];

/// One read-only bind mount.
struct Mount {
    source: PathBuf,
    target: String,
}

/// The `docker run` argv for one oracle run, without the docker binary itself.
fn run_argv(
    platform: &str,
    reference: &str,
    read_only: &[Mount],
    out: &Path,
    env: &[(String, String)],
    command: &[String],
) -> Res<Vec<OsString>> {
    let mut argv: Vec<OsString> = vec!["run".into(), "--platform".into(), platform.into()];
    argv.extend(HARDENING.iter().map(OsString::from));
    let mount = |source: &Path, target: &str, readonly: bool| -> Res<OsString> {
        let src = source
            .to_str()
            .filter(|s| !s.contains(','))
            .ok_or_else(|| {
                format!(
                    "cannot mount {}: not UTF-8 or contains ','",
                    source.display()
                )
            })?;
        let mut spec = format!("type=bind,source={src},target={target}");
        if readonly {
            spec.push_str(",readonly");
        }
        Ok(spec.into())
    };
    for m in read_only {
        argv.push("--mount".into());
        argv.push(mount(&m.source, &m.target, true)?);
    }
    argv.push("--mount".into());
    argv.push(mount(out, "/out", false)?);
    for (k, v) in env {
        argv.push("-e".into());
        argv.push(format!("{k}={v}").into());
    }
    argv.push(reference.into());
    argv.extend(command.iter().map(OsString::from));
    Ok(argv)
}

/// The script path inside the container, for a checkout-relative `script` under an allowed root.
fn container_script(script: &Path, mount_standin: bool) -> Res<String> {
    let mut parts = Vec::new();
    for c in script.components() {
        match c {
            Component::Normal(p) => parts.push(
                p.to_str()
                    .ok_or_else(|| format!("script path is not UTF-8: {}", script.display()))?,
            ),
            _ => {
                return Err(format!(
                    "script must be a relative path with normal components: {}",
                    script.display()
                ));
            }
        }
    }
    match parts.first() {
        Some(&"conformance") if parts.len() > 1 => {}
        Some(&"standin") if parts.len() > 1 && mount_standin => {}
        _ => {
            return Err(format!(
                "script must lie under conformance/ (or standin/ with --mount-standin): {}",
                script.display()
            ));
        }
    }
    Ok(format!("/opt/ksci/{}", parts.join("/")))
}

/// One input: the host file and its name under `/in`.
fn resolve_input(cache: &Cache, spec: &str) -> Res<(PathBuf, String)> {
    let path = Path::new(spec);
    if !path.exists() && store::check_digest(spec).is_ok() {
        let (_, blob) = store::fetch_blob(cache, spec, None, Some(&Remote::from_env()))?;
        return Ok((blob, spec.to_owned()));
    }
    let abs = path
        .canonicalize()
        .map_err(|e| format!("input {spec}: {e}"))?;
    if !abs.is_file() {
        return Err(format!("input {spec} is not a file"));
    }
    let name = abs
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| format!("input {spec}: no UTF-8 file name"))?
        .to_owned();
    Ok((abs, name))
}

/// The outputs of one run: file name to the sha256 of its stored, canonical bytes.
pub type Outputs = BTreeMap<String, String>;

/// Runs `args.script` in the pinned image; on success, canonicalises and stores every output.
/// Returns the exit code (the container's, when it failed) and the stored outputs.
pub fn run_script(args: &RunArgs) -> Res<(ExitCode, Outputs)> {
    check_platform(&args.platform)?;
    let root = workspace::root()?;
    let pins = load_pins(&root)?;
    let image = pins.images.get(&args.image).ok_or_else(|| {
        format!(
            "image {:?} is not pinned in {PINS_PATH}; run: cargo xtask oracle build-image {} --platform {}",
            args.image, args.image, args.platform
        )
    })?;
    let pinned = image.platforms.get(&args.platform).ok_or_else(|| {
        format!(
            "image {:?} has no pin for {} in {PINS_PATH}",
            args.image, args.platform
        )
    })?;
    let local = image_identity(&image.reference, &args.platform)?;
    if local != pinned.identity {
        return Err(format!(
            "image {} ({}) is {local} locally but pinned as {}; refusing to run",
            args.image, args.platform, pinned.identity
        ));
    }

    let script = container_script(&args.script, args.mount_standin)?;
    if !root.join(&args.script).is_file() {
        return Err(format!("script {} does not exist", args.script.display()));
    }
    if let Some(prelude) = &args.prelude
        && !prelude
            .split('.')
            .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
    {
        return Err(format!(
            "--prelude must be a dotted module name: {prelude:?}"
        ));
    }

    let cache = Cache::from_env()?;
    let mut mounts = vec![Mount {
        source: root.join("conformance"),
        target: CONFORMANCE_MOUNT.to_owned(),
    }];
    let mut pythonpath = vec![CONFORMANCE_MOUNT];
    if args.mount_standin {
        mounts.push(Mount {
            source: root.join("standin"),
            target: STANDIN_MOUNT.to_owned(),
        });
        pythonpath.push(STANDIN_MOUNT);
    }
    let mut command = vec![
        "python3".to_owned(),
        script,
        "--out".to_owned(),
        "/out".to_owned(),
    ];
    let mut names = Vec::new();
    for spec in &args.inputs {
        let (source, name) = resolve_input(&cache, spec)?;
        if names.contains(&name) {
            return Err(format!("two inputs share the file name {name:?}"));
        }
        let target = format!("/in/{name}");
        command.push("--input".to_owned());
        command.push(target.clone());
        mounts.push(Mount { source, target });
        names.push(name);
    }
    if let Some(jobs) = args.jobs {
        command.extend(["--jobs".to_owned(), jobs.to_string()]);
    }
    if let Some(cpu) = args.cpu_seconds {
        command.extend(["--cpu-seconds".to_owned(), cpu.to_string()]);
    }
    if let Some(prelude) = &args.prelude {
        command.extend(["--prelude".to_owned(), prelude.clone()]);
    }
    if args.require_sympy_env {
        command.push("--require-sympy-env".to_owned());
    }
    command.extend(args.script_args.iter().cloned());
    let env = [
        ("PYTHONHASHSEED".to_owned(), args.seed.to_string()),
        ("PYTHONPATH".to_owned(), pythonpath.join(":")),
    ];

    let held = host::heavy_job()?;
    let out = fresh_out_dir()?;
    let argv = run_argv(
        &args.platform,
        &image.reference,
        &mounts,
        &out,
        &env,
        &command,
    )?;
    let status = host::docker()?
        .args(&argv)
        .stdin(Stdio::null())
        .status()
        .map_err(|e| format!("run docker run: {e}"))?;
    drop(held);
    if !status.success() {
        let _ = fs::remove_dir_all(&out);
        eprintln!("oracle run: the container exited with {status}; nothing stored");
        let code = status.code().unwrap_or(1);
        return Ok((
            ExitCode::from(u8::try_from(code).unwrap_or(1)),
            Outputs::new(),
        ));
    }
    let stored = store_outputs(&cache, &out);
    let _ = fs::remove_dir_all(&out);
    Ok((ExitCode::SUCCESS, stored?))
}

/// A fresh, empty, world-writable output directory under `<target>/oracle/` (uid 65534 in the
/// container writes it).
fn fresh_out_dir() -> Res<PathBuf> {
    let dir = store::target_dir()?
        .join("oracle")
        .join(format!("run-{}", std::process::id()));
    if dir.exists() {
        fs::remove_dir_all(&dir).map_err(|e| format!("remove {}: {e}", dir.display()))?;
    }
    fs::create_dir_all(&dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    set_world_writable(&dir)?;
    Ok(dir)
}

#[cfg(unix)]
fn set_world_writable(dir: &Path) -> Res<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(dir, fs::Permissions::from_mode(0o777))
        .map_err(|e| format!("chmod {}: {e}", dir.display()))
}

#[cfg(not(unix))]
fn set_world_writable(_dir: &Path) -> Res<()> {
    Ok(())
}

/// Canonicalises each output file in place, stores it, and prints `<file> sha256=<hex>`; for
/// `records.jsonl` also `ok=N error=N timeout=N`. Only regular files are accepted: a symlink the
/// container left in `/out` is never followed on the host.
fn store_outputs(cache: &Cache, out: &Path) -> Res<Outputs> {
    let mut entries = Vec::new();
    for entry in fs::read_dir(out).map_err(|e| format!("read {}: {e}", out.display()))? {
        let entry = entry.map_err(|e| format!("read {}: {e}", out.display()))?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|n| format!("output name is not UTF-8: {n:?}"))?;
        let meta =
            fs::symlink_metadata(entry.path()).map_err(|e| format!("stat output {name}: {e}"))?;
        if !meta.file_type().is_file() {
            return Err(format!("output {name} is not a regular file; refusing it"));
        }
        entries.push(name);
    }
    entries.sort();
    let mut stored = Outputs::new();
    for name in entries {
        let path = out.join(&name);
        let bytes = fs::read(&path).map_err(|e| format!("read output {name}: {e}"))?;
        let (canonical, counts) = if name.ends_with(".jsonl") {
            let canonical = canonical_jsonl(&name, &bytes)?;
            let counts = (name == "records.jsonl")
                .then(|| record_counts(&canonical))
                .transpose()?;
            (canonical, counts)
        } else if name.ends_with(".json") {
            let value: Value =
                serde_json::from_slice(&bytes).map_err(|e| format!("output {name}: {e}"))?;
            let canonical = ksci_wire::to_jcs(&value).map_err(|e| format!("output {name}: {e}"))?;
            (canonical, None)
        } else {
            (bytes, None)
        };
        fs::write(&path, &canonical).map_err(|e| format!("write output {name}: {e}"))?;
        let hex = store::put(cache, &path)?;
        println!("{name} sha256={hex}");
        if let Some([ok, error, timeout]) = counts {
            println!("ok={ok} error={error} timeout={timeout}");
        }
        stored.insert(name, hex);
    }
    Ok(stored)
}

/// Each line of a JSON Lines file re-encoded with JCS, line order preserved, `\n` after each.
fn canonical_jsonl(name: &str, bytes: &[u8]) -> Res<Vec<u8>> {
    let text = std::str::from_utf8(bytes).map_err(|e| format!("output {name}: {e}"))?;
    let body = text.strip_suffix('\n').unwrap_or(text);
    let mut canonical = Vec::with_capacity(bytes.len());
    if body.is_empty() {
        return Ok(canonical);
    }
    for (i, line) in body.split('\n').enumerate() {
        let value: Value =
            serde_json::from_str(line).map_err(|e| format!("output {name} line {}: {e}", i + 1))?;
        canonical.extend(
            ksci_wire::to_jcs(&value).map_err(|e| format!("output {name} line {}: {e}", i + 1))?,
        );
        canonical.push(b'\n');
    }
    Ok(canonical)
}

/// Counts by status of canonical `records.jsonl` bytes, which must hold one record per case in
/// strictly ascending `case_id` order with a known status.
fn record_counts(canonical: &[u8]) -> Res<[usize; 3]> {
    let mut counts = [0; 3];
    let mut last: Option<String> = None;
    for (i, line) in canonical.split(|b| *b == b'\n').enumerate() {
        if line.is_empty() {
            continue;
        }
        let record: Value = serde_json::from_slice(line)
            .map_err(|e| format!("records.jsonl line {}: {e}", i + 1))?;
        let case_id = record
            .get("case_id")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("records.jsonl line {}: no case_id", i + 1))?;
        if last.as_deref().is_some_and(|prev| prev >= case_id) {
            return Err(format!(
                "records.jsonl line {}: case_id {case_id} is not after the previous one",
                i + 1
            ));
        }
        last = Some(case_id.to_owned());
        match record.get("status").and_then(Value::as_str) {
            Some("ok") => counts[0] += 1,
            Some("error") => counts[1] += 1,
            Some("timeout") => counts[2] += 1,
            other => {
                return Err(format!(
                    "records.jsonl line {}: unknown status {other:?}",
                    i + 1
                ));
            }
        }
    }
    Ok(counts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oracle_run_flags_are_hardened() {
        let mounts = [Mount {
            source: PathBuf::from("/repo/conformance"),
            target: CONFORMANCE_MOUNT.to_owned(),
        }];
        let env = [("PYTHONHASHSEED".to_owned(), "0".to_owned())];
        let command = [
            "python3".to_owned(),
            "/opt/ksci/conformance/x.py".to_owned(),
        ];
        let argv = run_argv(
            "linux/arm64",
            BASE_REFERENCE,
            &mounts,
            Path::new("/tmp/out"),
            &env,
            &command,
        )
        .unwrap();
        let argv: Vec<&str> = argv.iter().map(|a| a.to_str().unwrap()).collect();
        for flag in [
            "--network=none",
            "--read-only",
            "--cap-drop=ALL",
            "--security-opt=no-new-privileges",
            "--pids-limit=512",
            "--memory=8g",
            "--pull=never",
        ] {
            assert!(argv.contains(&flag), "{flag} missing from {argv:?}");
        }
        let after = |flag: &str| argv[argv.iter().position(|a| *a == flag).unwrap() + 1];
        assert_eq!(after("--user"), "65534:65534");
        assert_eq!(after("--tmpfs"), "/tmp:rw,size=1g,mode=1777");
        assert_eq!(after("--platform"), "linux/arm64");
        assert!(
            argv.contains(
                &"type=bind,source=/repo/conformance,target=/opt/ksci/conformance,readonly"
            )
        );
        // The output directory is the only writable mount.
        let writable: Vec<&&str> = argv
            .iter()
            .filter(|a| a.starts_with("type=bind") && !a.ends_with(",readonly"))
            .collect();
        assert_eq!(writable, [&"type=bind,source=/tmp/out,target=/out"]);
        // The image comes after every flag, then the command.
        let image = argv.iter().position(|a| *a == BASE_REFERENCE).unwrap();
        assert_eq!(
            &argv[image + 1..],
            ["python3", "/opt/ksci/conformance/x.py"]
        );
        assert!(
            run_argv(
                "linux/arm64",
                BASE_REFERENCE,
                &[],
                Path::new("/tmp/a,b"),
                &[],
                &[]
            )
            .is_err()
        );
    }

    #[test]
    fn oracle_script_paths() {
        let ok = container_script(Path::new("conformance/oracles/run_programs.py"), false);
        assert_eq!(ok.unwrap(), "/opt/ksci/conformance/oracles/run_programs.py");
        assert!(container_script(Path::new("standin/x.py"), false).is_err());
        assert_eq!(
            container_script(Path::new("standin/x.py"), true).unwrap(),
            "/opt/ksci/standin/x.py"
        );
        for bad in [
            "/abs/conformance/x.py",
            "conformance/../x.py",
            "./conformance/x.py",
            "conformance",
            "xtask/src/main.rs",
        ] {
            assert!(container_script(Path::new(bad), true).is_err(), "{bad}");
        }
    }

    #[test]
    fn oracle_pins_roundtrip_as_jcs() {
        let mut pins = Pins::empty();
        let mut platforms = BTreeMap::new();
        platforms.insert(
            "linux/arm64".to_owned(),
            PlatformPin {
                identity: format!("sha256:{}", "a".repeat(64)),
                emulated: false,
                tar_sha256: None,
            },
        );
        pins.images.insert(
            "base".to_owned(),
            ImagePin {
                reference: BASE_REFERENCE.to_owned(),
                platforms,
                introspection_sha256: None,
            },
        );
        let bytes = pins.to_bytes().unwrap();
        let text = std::str::from_utf8(&bytes).unwrap();
        assert!(text.starts_with(r#"{"images":{"base":{"platforms":{"linux/arm64":{"emulated":false,"identity":"sha256:aaaa"#), "{text}");
        assert_eq!(Pins::parse(&bytes).unwrap(), pins);
        let other = text.replace(PINS_SCHEMA, "ksci.oracle-images.v0");
        assert!(Pins::parse(other.as_bytes()).is_err());
        let unknown = text.replace(r#""emulated""#, r#""extra":1,"emulated""#);
        assert!(Pins::parse(unknown.as_bytes()).is_err());
    }

    #[test]
    fn oracle_records_are_canonical_and_ordered() {
        let raw = b"{\"status\": \"ok\", \"case_id\": \"a\"}\n{\"case_id\":\"b\",\"status\":\"timeout\"}\n";
        let canonical = canonical_jsonl("records.jsonl", raw).unwrap();
        assert_eq!(
            canonical,
            b"{\"case_id\":\"a\",\"status\":\"ok\"}\n{\"case_id\":\"b\",\"status\":\"timeout\"}\n"
        );
        assert_eq!(record_counts(&canonical).unwrap(), [1, 0, 1]);
        let unordered = canonical_jsonl(
            "records.jsonl",
            b"{\"case_id\":\"b\",\"status\":\"ok\"}\n{\"case_id\":\"a\",\"status\":\"ok\"}\n",
        )
        .unwrap();
        assert!(record_counts(&unordered).is_err());
        let duplicate = canonical_jsonl(
            "records.jsonl",
            b"{\"case_id\":\"a\",\"status\":\"ok\"}\n{\"case_id\":\"a\",\"status\":\"ok\"}\n",
        )
        .unwrap();
        assert!(record_counts(&duplicate).is_err());
        let unknown = canonical_jsonl(
            "records.jsonl",
            b"{\"case_id\":\"a\",\"status\":\"skipped\"}\n",
        )
        .unwrap();
        assert!(record_counts(&unknown).is_err());
        assert!(canonical_jsonl("x.jsonl", b"{\"a\":1}\n\n").is_err());
    }
}
