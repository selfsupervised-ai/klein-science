// provenance: original
//! The offline oracle (CONF-01): pinned images and the one hardened container runner (D-01).
//!
//! Untrusted programs (corpus programs, stand-in runs, census probes) run only through
//! [`run`]: a `docker run` with no network, a read-only root, a size-capped `/tmp`, no privilege
//! gain, capped pids and memory. The harness runs as uid 65534 holding only CAP_KILL, CAP_SETGID
//! and CAP_SETUID, and each case runs as its own unprivileged uid ([`HARDENING`]). Repository
//! inputs are mounted read-only; the only writable host path is a fresh output directory,
//! reachable only by the harness. Every run first
//! checks that the local image is the one pinned in `conformance/oracles/images.json` for that
//! platform, and holds [`host::heavy_job`] for its duration.
//!
//! The outputs are canonicalised on the host with [`ksci_wire::to_jcs`] (each line of a `.jsonl`
//! file, each `.json` file whole) and stored by digest in the artifact store, so neither the
//! worker count inside the container nor the order cases finish in changes the stored bytes.
//!
//! `images.json` is written only here, as the JCS bytes of [`Pins`].
//!
//! Goldens (CONF-01, CONF-02, D-10) are produced by `oracle golden`, which calls [`run_script`]
//! once per platform, hash seed and repetition, and admits a case only when its canonical record
//! is byte-identical in every run; every other case is listed for adjudication with a reason.
//! The golden records live in the store by digest. Git holds only the manifest
//! ([`GoldenManifest`]), which `oracle golden --verify` re-derives from the stored run blobs.
//!
//! The stand-in baseline (CONF-03, GUEST-02) is produced by `oracle standin`: the same
//! [`run_script`] and run_programs.py run the stand-in on a verified manifest's programs, the
//! three-level scorer ([`SCORER`]) compares its records with the goldens, and git holds only the
//! aggregate and its digests, labelled [`STANDIN_LABEL`].

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

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
    /// Pull (`base`) or build (conformance/oracles/<name>/Containerfile) an oracle image for each
    /// platform and pin the identity docker reports; a built image is also saved to the store.
    BuildImage {
        /// Image name: a key of conformance/oracles/images.json.
        name: String,
        /// Platforms (repeatable), e.g. linux/arm64.
        #[arg(long = "platform", required = true)]
        platforms: Vec<String>,
        /// Do not build: obtain the pinned image (`docker load` of the stored tar when it is
        /// absent locally) and check its identity against the pin. images.json is not written.
        #[arg(long)]
        from_store: bool,
    },
    /// Run a script in a pinned oracle image inside the hardened container, then canonicalise
    /// and store its outputs.
    Run(RunArgs),
    /// Produce the SymPy introspection table in the sympy image on every platform, require the
    /// canonical outputs to be identical, store it and pin its digest.
    Introspect,
    /// Run programs in the sympy image under every hash seed, repetition and platform (D-10),
    /// store the cases whose records agree in every run as goldens, and write
    /// conformance/golden/<name>.golden.json; or, with --verify, re-derive a manifest.
    Golden(GoldenArgs),
    /// Run the stand-in on a golden manifest's programs (the base image, standin/ mounted
    /// read-only, the host_shim prelude, seed 0), score the records against the goldens on three
    /// levels, and write conformance/golden/scores/<name>.m0-standin.json (labelled proxy).
    Standin(StandinArgs),
}

/// `cargo xtask oracle standin ...`
#[derive(Args)]
pub struct StandinArgs {
    /// The golden manifest whose programs are run and whose goldens are the reference.
    #[arg(long)]
    pub golden_manifest: PathBuf,
    /// Score file stem ([a-z0-9-]+): writes conformance/golden/scores/<name>.m0-standin.json.
    #[arg(long)]
    pub name: String,
    /// Platform to run (the base image must be pinned for it).
    #[arg(long, default_value = "linux/arm64")]
    pub platform: String,
    /// Worker count requested inside the container (default: the manifest's).
    #[arg(long)]
    pub jobs: Option<u32>,
    /// CPU limit per case in seconds (default: the manifest's).
    #[arg(long)]
    pub cpu_seconds: Option<u32>,
}

/// `cargo xtask oracle golden ...`
#[derive(Args)]
pub struct GoldenArgs {
    /// Re-derive this manifest from its stored run blobs (no container runs) and fail unless the
    /// result is byte-identical to the file.
    #[arg(long, value_name = "MANIFEST", conflicts_with_all = ["name", "input"])]
    pub verify: Option<PathBuf>,
    /// Manifest stem ([a-z0-9-]+): writes conformance/golden/<name>.golden.json.
    #[arg(long, required_unless_present = "verify")]
    pub name: Option<String>,
    /// The programs (JSON Lines of {case_id, code}): a path, or the sha256 of a stored blob.
    #[arg(long, required_unless_present = "verify")]
    pub input: Option<String>,
    /// PYTHONHASHSEED values, comma-separated.
    #[arg(long, value_delimiter = ',', default_values_t = GOLDEN_SEEDS)]
    pub seeds: Vec<u32>,
    /// Repetitions of each (platform, seed).
    #[arg(long, default_value_t = 2)]
    pub runs: u32,
    /// Platforms, comma-separated.
    #[arg(long, value_delimiter = ',', default_values_t = PLATFORMS.iter().map(|p| (*p).to_owned()))]
    pub platforms: Vec<String>,
    /// Worker count requested inside the container (run_programs.py --jobs).
    #[arg(long, default_value_t = 4)]
    pub jobs: u32,
    /// CPU limit per case in seconds (run_programs.py --cpu-seconds).
    #[arg(long, default_value_t = 60)]
    pub cpu_seconds: u32,
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
        OracleCmd::BuildImage {
            name,
            platforms,
            from_store,
        } => if from_store {
            obtain_pinned(&name, &platforms)
        } else {
            build_image(&name, &platforms)
        }
        .map(|()| ExitCode::SUCCESS),
        OracleCmd::Run(args) => run_script(&args).map(|(code, _)| code),
        OracleCmd::Introspect => introspect().map(|()| ExitCode::SUCCESS),
        OracleCmd::Golden(args) => match &args.verify {
            Some(manifest) => verify_golden(manifest),
            None => golden(&args),
        }
        .map(|()| ExitCode::SUCCESS),
        OracleCmd::Standin(args) => standin(&args).map(|()| ExitCode::SUCCESS),
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
    /// The reference docker runs: the base image's digest reference, or the local per-platform
    /// tag of a built image (`ksci-oracle-<name>:linux-arm64`).
    pub reference: String,
    /// The local image ID (`docker image inspect <reference>` `.Id`, `sha256:<hex>`). It is
    /// content-addressed, and it is what `docker run` executes: never the reference, which is a
    /// tag a concurrent build can move (review W1). For `base` it is the index digest.
    pub identity: String,
    /// The platform manifest the identity resolves to on this platform
    /// (`docker image inspect --platform <p> <identity>`), checked before every run.
    pub manifest: String,
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

/// `.Id` from `docker image inspect [--platform <p>] <image>`, which must be `sha256:<hex>`.
fn inspect_id(image: &str, platform: Option<&str>) -> Res<String> {
    let mut args = vec!["image", "inspect"];
    if let Some(p) = platform {
        args.extend(["--platform", p]);
    }
    args.extend(["--format", "{{.Id}}", image]);
    let id = docker_output(&args)?;
    match id.strip_prefix("sha256:") {
        Some(hex) if store::check_digest(hex).is_ok() => Ok(id),
        _ => Err(format!(
            "docker reports {id:?} for {image}; expected sha256:<hex>"
        )),
    }
}

/// The local image ID of `reference`: the content-addressed identity `docker run` executes.
fn image_identity(reference: &str) -> Res<String> {
    inspect_id(reference, None)
}

/// The platform manifest digest the image `identity` resolves to on `platform`.
fn platform_manifest(identity: &str, platform: &str) -> Res<String> {
    inspect_id(identity, Some(platform))
}

/// Checks that the pinned identity is present locally and resolves to the pinned manifest on
/// `platform`; the caller then runs exactly `pin.identity`.
fn verify_pin(name: &str, platform: &str, pin: &PlatformPin) -> Res<()> {
    let manifest = platform_manifest(&pin.identity, platform).map_err(|e| {
        format!(
            "image {name} ({platform}) {} is not available locally ({e}); run: cargo xtask oracle build-image {name} --platform {platform} --from-store",
            pin.identity
        )
    })?;
    if manifest != pin.manifest {
        return Err(format!(
            "image {name} ({platform}) {} resolves to manifest {manifest} locally but is pinned to {}; refusing to run",
            pin.identity, pin.manifest
        ));
    }
    Ok(())
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

/// Image names (directory names under conformance/oracles/) and golden manifest stems:
/// `[a-z0-9-]+`. `what` names the kind in the error.
fn check_name(what: &str, name: &str) -> Res<()> {
    if !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    {
        Ok(())
    } else {
        Err(format!("{what} must match [a-z0-9-]+: {name:?}"))
    }
}

/// The local tag of a built image on one platform: `ksci-oracle-sympy:linux-arm64`.
pub fn built_tag(name: &str, platform: &str) -> String {
    format!("ksci-oracle-{name}:{}", platform.replace('/', "-"))
}

/// Runs the pinned docker with `args`, its output passed through; a non-zero exit is an error.
fn docker_status(args: &[&str]) -> Res<()> {
    let status = host::docker()?
        .args(args)
        .stdin(Stdio::null())
        .status()
        .map_err(|e| format!("run docker {}: {e}", args.join(" ")))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("docker {}: {status}", args.join(" ")))
    }
}

fn build_image(name: &str, platforms: &[String]) -> Res<()> {
    check_name("image name", name)?;
    for p in platforms {
        check_platform(p)?;
    }
    let root = workspace::root()?;
    let context = root.join("conformance").join("oracles").join(name);
    let containerfile = context.join("Containerfile");
    if name != "base" && !containerfile.is_file() {
        return Err(format!(
            "no image definition {}",
            containerfile
                .strip_prefix(&root)
                .unwrap_or(&containerfile)
                .display()
        ));
    }
    let native = native_platform()?;
    let cache = Cache::from_env()?;
    let mut pins = load_pins(&root)?;
    for platform in platforms {
        let pin = if name == "base" {
            docker_status(&["pull", "--platform", platform, BASE_REFERENCE])?;
            let identity = image_identity(BASE_REFERENCE)?;
            PlatformPin {
                reference: BASE_REFERENCE.to_owned(),
                manifest: platform_manifest(&identity, platform)?,
                identity,
                emulated: *platform != native,
                tar_sha256: None,
            }
        } else {
            let tag = built_tag(name, platform);
            let context = path_str(&context)?;
            let containerfile = path_str(&containerfile)?;
            let held = host::heavy_job()?;
            docker_status(&[
                "buildx",
                "build",
                "--platform",
                platform,
                "--load",
                "--provenance=false",
                "--sbom=false",
                "-t",
                &tag,
                "-f",
                containerfile,
                context,
            ])?;
            drop(held);
            // From here on only the content-addressed identity is used, so a concurrent retag
            // cannot change what is saved or pinned.
            let identity = image_identity(&tag)?;
            let manifest = platform_manifest(&identity, platform)?;
            let tar = store::target_dir()?
                .join("oracle")
                .join(format!("save-{}.tar", std::process::id()));
            if let Some(parent) = tar.parent() {
                fs::create_dir_all(parent)
                    .map_err(|e| format!("create {}: {e}", parent.display()))?;
            }
            let saved = docker_status(&["save", "-o", path_str(&tar)?, &identity])
                .and_then(|()| store::put(&cache, &tar));
            let _ = fs::remove_file(&tar);
            PlatformPin {
                reference: tag,
                identity,
                manifest,
                emulated: *platform != native,
                tar_sha256: Some(saved?),
            }
        };
        println!(
            "{name} {platform} identity={} manifest={}{}",
            pin.identity,
            pin.manifest,
            pin.tar_sha256
                .as_deref()
                .map(|t| format!(" tar_sha256={t}"))
                .unwrap_or_default()
        );
        pins.images
            .entry(name.to_owned())
            .or_insert_with(|| ImagePin {
                platforms: BTreeMap::new(),
                introspection_sha256: None,
            })
            .platforms
            .insert(platform.clone(), pin);
    }
    // A rebuilt image may differ from the one the introspection table came from.
    if name != "base"
        && let Some(image) = pins.images.get_mut(name)
    {
        image.introspection_sha256 = None;
    }
    save_pins(&root, &pins)?;
    println!("wrote {PINS_PATH}");
    Ok(())
}

/// Makes the pinned image available on each platform without building it: pulled by digest
/// (`base`), or `docker load`ed from its stored tar when absent locally. Either way the pinned
/// identity must then be present and resolve to the pinned manifest ([`verify_pin`]).
fn obtain_pinned(name: &str, platforms: &[String]) -> Res<()> {
    check_name("image name", name)?;
    let root = workspace::root()?;
    let pins = load_pins(&root)?;
    let image = pins
        .images
        .get(name)
        .ok_or_else(|| format!("image {name:?} is not pinned in {PINS_PATH}"))?;
    let cache = Cache::from_env()?;
    for platform in platforms {
        check_platform(platform)?;
        let pin = image
            .platforms
            .get(platform)
            .ok_or_else(|| format!("image {name:?} has no pin for {platform} in {PINS_PATH}"))?;
        if image_identity(&pin.identity).is_err() {
            match &pin.tar_sha256 {
                None => docker_status(&["pull", "--platform", platform, &pin.reference])?,
                Some(hex) => {
                    let (source, tar) =
                        store::fetch_blob(&cache, hex, None, Some(&Remote::from_env()))?;
                    println!("{name} {platform}: loading sha256:{hex} ({source})");
                    docker_status(&["load", "-i", path_str(&tar)?])?;
                }
            }
        }
        verify_pin(name, platform, pin)?;
        println!(
            "{name} {platform} identity={} manifest={} ok",
            pin.identity, pin.manifest
        );
    }
    Ok(())
}

fn path_str(path: &Path) -> Res<&str> {
    path.to_str()
        .ok_or_else(|| format!("path is not UTF-8: {}", path.display()))
}

/// The introspection script, relative to the checkout.
pub const INTROSPECT_SCRIPT: &str = "conformance/oracles/sympy/introspect.py";

/// Runs introspect.py in the sympy image on every platform; the canonical names.json must be
/// byte-identical across them. Stores it and pins its digest under the sympy entry.
fn introspect() -> Res<()> {
    let mut results: Vec<(&str, String)> = Vec::new();
    for platform in PLATFORMS {
        let args = RunArgs {
            image: "sympy".to_owned(),
            platform: (*platform).to_owned(),
            script: PathBuf::from(INTROSPECT_SCRIPT),
            inputs: Vec::new(),
            seed: 0,
            jobs: None,
            cpu_seconds: None,
            prelude: None,
            mount_standin: false,
            require_sympy_env: true,
            script_args: Vec::new(),
        };
        let (_, outputs) = run_script(&args)?;
        let hex = outputs
            .get("names.json")
            .ok_or_else(|| format!("introspect.py produced no names.json on {platform}"))?;
        println!("{platform} names.json sha256={hex}");
        results.push((platform, hex.clone()));
    }
    let cache = Cache::from_env()?;
    let (first_platform, first) = &results[0];
    let first_bytes = read_blob(&cache, first)?;
    for (platform, hex) in &results[1..] {
        if read_blob(&cache, hex)? != first_bytes {
            return Err(format!(
                "introspection differs: {first_platform} sha256={first}, {platform} sha256={hex}"
            ));
        }
    }
    let root = workspace::root()?;
    let mut pins = load_pins(&root)?;
    let image = pins
        .images
        .get_mut("sympy")
        .ok_or_else(|| format!("image \"sympy\" is not pinned in {PINS_PATH}"))?;
    image.introspection_sha256 = Some(first.clone());
    save_pins(&root, &pins)?;
    println!("introspection sha256={first}");
    Ok(())
}

/// The verified bytes of a cached blob.
fn read_blob(cache: &Cache, hex: &str) -> Res<Vec<u8>> {
    let blob = cache
        .lookup(hex)?
        .ok_or_else(|| format!("sha256:{hex} is not in the cache"))?;
    fs::read(&blob).map_err(|e| format!("read {}: {e}", blob.display()))
}

/// The hardening flags of every oracle container (T-01-13). Asserted by the acceptance greps and
/// by `oracle_run_flags_are_hardened`.
///
/// The container starts as root holding exactly CAP_KILL, CAP_SETGID and CAP_SETUID, and runs
/// only [`LAUNCHER`]. Before any script code runs, the launcher drops to the harness identity
/// (uid and gid 65534, no groups), keeping those three capabilities. The harness needs them to
/// run each case as its own uid and to kill and reap any case's processes (review R1, operator
/// decision 2026-10-11). `/harness` (mode 0700, owned by the harness) holds the inputs and
/// outputs, so cases cannot reach them.
const HARDENING: &[&str] = &[
    "--rm",
    "--pull=never",
    "--init",
    "--network=none",
    "--read-only",
    "--tmpfs",
    "/tmp:rw,size=1g,mode=1777",
    "--tmpfs",
    "/harness:rw,size=1m,mode=0700,uid=65534,gid=65534",
    "--cap-drop=ALL",
    "--cap-add=SETUID",
    "--cap-add=SETGID",
    "--cap-add=KILL",
    "--security-opt=no-new-privileges",
    "--memory=8g",
    "--user",
    "0:0",
    "--hostname",
    "ksci-oracle",
];

/// The container's task limit (`--pids-limit`). It is the one source: the same value reaches the
/// container as `KSCI_PIDS_LIMIT`, and run_programs.py sizes its worker count and per-case
/// RLIMIT_NPROC so that the budget can never be exceeded (review round 3).
const PIDS_LIMIT: u32 = 512;

/// The trusted entry point that drops to the harness identity, then runs the script in-process.
const LAUNCHER: &str = "/opt/ksci/conformance/oracles/launch.py";
/// The harness-only mount points (under the 0700 `/harness` tmpfs).
const OUT_MOUNT: &str = "/harness/out";
const IN_MOUNT: &str = "/harness/in";

/// One read-only bind mount.
struct Mount {
    source: PathBuf,
    target: String,
}

/// The `docker run` argv for one oracle run, without the docker binary itself. `command` is the
/// container script and its arguments; the container always enters through [`LAUNCHER`].
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
    argv.push(format!("--pids-limit={PIDS_LIMIT}").into());
    argv.push("-e".into());
    argv.push(format!("KSCI_PIDS_LIMIT={PIDS_LIMIT}").into());
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
    argv.push(mount(out, OUT_MOUNT, false)?);
    for (k, v) in env {
        argv.push("-e".into());
        argv.push(format!("{k}={v}").into());
    }
    argv.push(reference.into());
    argv.push("python3".into());
    argv.push(LAUNCHER.into());
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

/// What `docker run` executes for a pinned platform: the content-addressed identity that
/// [`verify_pin`] checked, never the reference (review W1).
fn run_image(pin: &PlatformPin) -> &str {
    &pin.identity
}

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
    verify_pin(&args.image, &args.platform, pinned)?;

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
    let mut command = vec![script, "--out".to_owned(), OUT_MOUNT.to_owned()];
    let mut names = Vec::new();
    for spec in &args.inputs {
        let (source, name) = resolve_input(&cache, spec)?;
        if names.contains(&name) {
            return Err(format!("two inputs share the file name {name:?}"));
        }
        let target = format!("{IN_MOUNT}/{name}");
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
        run_image(pinned),
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
/// container left in the output directory is never followed on the host.
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

/// The case statuses run_programs.py records, in the order of [`record_counts`].
const STATUSES: [&str; 3] = ["ok", "error", "timeout"];

/// One line of canonical `records.jsonl` bytes.
struct Record<'a> {
    case_id: String,
    /// Index into [`STATUSES`].
    status: usize,
    /// The canonical line, without its `\n`.
    line: &'a [u8],
}

/// The records of canonical `records.jsonl` bytes, which must hold one record per case in
/// strictly ascending `case_id` order with a known status.
fn parse_records(canonical: &[u8]) -> Res<Vec<Record<'_>>> {
    let mut records: Vec<Record<'_>> = Vec::new();
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
        if records
            .last()
            .is_some_and(|prev| prev.case_id.as_str() >= case_id)
        {
            return Err(format!(
                "records.jsonl line {}: case_id {case_id} is not after the previous one",
                i + 1
            ));
        }
        let status = record.get("status").and_then(Value::as_str);
        let status = STATUSES
            .iter()
            .position(|s| Some(*s) == status)
            .ok_or_else(|| format!("records.jsonl line {}: unknown status {status:?}", i + 1))?;
        records.push(Record {
            case_id: case_id.to_owned(),
            status,
            line,
        });
    }
    Ok(records)
}

/// Counts by status ([`STATUSES`] order) of canonical `records.jsonl` bytes ([`parse_records`]).
fn record_counts(canonical: &[u8]) -> Res<[usize; 3]> {
    let mut counts = [0; 3];
    for record in parse_records(canonical)? {
        counts[record.status] += 1;
    }
    Ok(counts)
}

// Goldens (CONF-01, CONF-02, D-10).

/// The schema tag of a golden manifest.
pub const GOLDEN_SCHEMA: &str = "ksci.golden-manifest.v1";
/// Where golden manifests live, relative to the checkout.
pub const GOLDEN_DIR: &str = "conformance/golden";
/// The one program executor (goldens, stand-in baseline, census), relative to the checkout.
pub const RUN_PROGRAMS: &str = "conformance/oracles/run_programs.py";
/// The image goldens are produced in.
const GOLDEN_IMAGE: &str = "sympy";
/// The D-10 hash seeds: 0 (the production value), 1, 2, and 4294967295, the largest value
/// PYTHONHASHSEED accepts.
const GOLDEN_SEEDS: [u32; 4] = [0, 1, 2, 4294967295];
/// RLIMIT_AS of each case. xtask passes it to run_programs.py as --mem-bytes, so the manifest
/// records the value that was set rather than one assumed from the script's default.
const GOLDEN_MEM_BYTES: u64 = 2_147_483_648;
/// Why a case is not golden, in precedence order: its status differs between runs; else its
/// record differs between the platforms for some (seed, run); else between seeds for some
/// (platform, run); else only between repetitions of one (platform, seed).
const REASONS: [&str; 4] = ["status", "platform", "seed", "run"];

/// `conformance/golden/<name>.golden.json`: the JCS bytes of this struct, and nothing else.
/// Golden and run bytes are only in the store, named here by sha256 (CLAUDE.md rule 6).
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
#[serde(deny_unknown_fields)]
pub struct GoldenManifest {
    /// [`GOLDEN_SCHEMA`].
    pub schema: String,
    /// The manifest stem.
    pub name: String,
    /// sha256 of the programs (JSON Lines of {case_id, code}).
    pub input_sha256: String,
    /// Platform to the sympy image that ran there.
    pub images: BTreeMap<String, GoldenImage>,
    /// `git hash-object` of [`RUN_PROGRAMS`] as it was mounted.
    pub run_programs_blob: String,
    /// The matrix and limits.
    pub params: GoldenParams,
    /// Every run, sorted by (platform, seed, run).
    pub runs: Vec<GoldenRun>,
    /// Derived from the runs ([`finalise`]).
    pub counts: GoldenCounts,
    /// Derived: sha256 of the golden JSON Lines (the agreeing records, by case_id).
    pub golden_sha256: String,
    /// Derived: every case that is not golden, by case_id.
    pub adjudication: Vec<Adjudication>,
}

/// The sympy image of one platform.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
#[serde(deny_unknown_fields)]
pub struct GoldenImage {
    /// The pinned image identity that ran.
    pub identity: String,
    /// sha256 of its saved tar in the store.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tar_sha256: Option<String>,
    /// Whether the platform ran under emulation on the golden host.
    pub emulated: bool,
}

/// The run matrix and the limits of every run.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
#[serde(deny_unknown_fields)]
pub struct GoldenParams {
    /// Hash seeds, strictly ascending.
    pub seeds: Vec<u32>,
    /// Repetitions of each (platform, seed).
    pub runs: u32,
    /// Platforms, strictly ascending.
    pub platforms: Vec<String>,
    /// CPU limit per case.
    pub cpu_seconds: u32,
    /// RLIMIT_AS per case.
    pub mem_bytes: u64,
    /// Requested worker count.
    pub jobs: u32,
}

/// One run of the matrix and its stored outputs.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
#[serde(deny_unknown_fields)]
pub struct GoldenRun {
    /// Platform.
    pub platform: String,
    /// PYTHONHASHSEED.
    pub seed: u32,
    /// Repetition, from 1.
    pub run: u32,
    /// sha256 of the run's canonical records.jsonl.
    pub records_sha256: String,
    /// sha256 of the run's canonical run.json (interpreter, versions, seed, limits, inputs).
    pub run_sha256: String,
}

/// Counts derived from the runs.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Default)]
#[serde(deny_unknown_fields)]
pub struct GoldenCounts {
    /// Cases in every run.
    pub cases: usize,
    /// Cases whose records agree in every run.
    pub golden: usize,
    /// The other cases.
    pub adjudication: usize,
    /// Golden cases by status (every status, zeros included).
    pub golden_by_status: BTreeMap<String, usize>,
    /// Adjudicated cases by reason (every reason, zeros included).
    pub adjudication_by_reason: BTreeMap<String, usize>,
}

/// A case that is not golden.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
#[serde(deny_unknown_fields)]
pub struct Adjudication {
    /// The case.
    pub case_id: String,
    /// One of [`REASONS`].
    pub reason: String,
}

/// What [`judge`] derives from the runs.
struct Judgement {
    golden: Vec<u8>,
    counts: GoldenCounts,
    adjudication: Vec<Adjudication>,
}

/// Admits each case whose canonical record is byte-identical in every run; every other case is
/// adjudicated with the first [`REASONS`] entry that applies. Every run must hold the same cases.
/// The golden bytes are the agreeing lines in case_id order, each followed by `\n`.
fn judge(runs: &[(&GoldenRun, &[u8])]) -> Res<Judgement> {
    let label = |r: &GoldenRun| format!("{} seed {} run {}", r.platform, r.seed, r.run);
    let parsed = runs
        .iter()
        .map(|(r, bytes)| parse_records(bytes).map_err(|e| format!("{}: {e}", label(r))))
        .collect::<Res<Vec<_>>>()?;
    let ((first_run, _), first) = runs.first().zip(parsed.first()).ok_or("no runs to judge")?;
    for ((r, _), records) in runs.iter().zip(&parsed) {
        if records.len() != first.len()
            || records
                .iter()
                .zip(first)
                .any(|(a, b)| a.case_id != b.case_id)
        {
            return Err(format!(
                "{} and {} hold different cases; records never drop a case, so the harness failed",
                label(first_run),
                label(r)
            ));
        }
    }
    // Whether the case's record differs between two runs that share `key`.
    let varies = |case: usize, key: &dyn Fn(&GoldenRun) -> (String, u32)| {
        let mut seen: BTreeMap<(String, u32), &[u8]> = BTreeMap::new();
        runs.iter().zip(&parsed).any(|((r, _), records)| {
            let line = records[case].line;
            *seen.entry(key(r)).or_insert(line) != line
        })
    };
    let mut golden = Vec::new();
    let mut counts = GoldenCounts {
        cases: first.len(),
        golden_by_status: STATUSES.iter().map(|s| ((*s).to_owned(), 0)).collect(),
        adjudication_by_reason: REASONS.iter().map(|r| ((*r).to_owned(), 0)).collect(),
        ..GoldenCounts::default()
    };
    let mut adjudication = Vec::new();
    for (case, base) in first.iter().enumerate() {
        let reason = if parsed.iter().any(|p| p[case].status != base.status) {
            "status"
        } else if parsed.iter().all(|p| p[case].line == base.line) {
            golden.extend_from_slice(base.line);
            golden.push(b'\n');
            counts.golden += 1;
            *counts
                .golden_by_status
                .entry(STATUSES[base.status].to_owned())
                .or_default() += 1;
            continue;
        } else if varies(case, &|r| (format!("{}", r.seed), r.run)) {
            "platform"
        } else if varies(case, &|r| (r.platform.clone(), r.run)) {
            "seed"
        } else {
            "run"
        };
        counts.adjudication += 1;
        *counts
            .adjudication_by_reason
            .entry(reason.to_owned())
            .or_default() += 1;
        adjudication.push(Adjudication {
            case_id: base.case_id.clone(),
            reason: reason.to_owned(),
        });
    }
    Ok(Judgement {
        golden,
        counts,
        adjudication,
    })
}

/// Requires the params to be a valid, normalised matrix and `runs` to be exactly its runs in
/// (platform, seed, run) order.
fn check_matrix(params: &GoldenParams, runs: &[GoldenRun]) -> Res<()> {
    if params.seeds.is_empty() || !params.seeds.windows(2).all(|w| w[0] < w[1]) {
        return Err(format!(
            "seeds must be non-empty and strictly ascending: {:?}",
            params.seeds
        ));
    }
    if params.platforms.is_empty() || !params.platforms.windows(2).all(|w| w[0] < w[1]) {
        return Err(format!(
            "platforms must be non-empty and strictly ascending: {:?}",
            params.platforms
        ));
    }
    for p in &params.platforms {
        check_platform(p)?;
    }
    if params.runs == 0 {
        return Err("runs must be at least 1".to_owned());
    }
    let expected = matrix_runs(params);
    let labels = |rs: &[GoldenRun]| -> Vec<(String, u32, u32)> {
        rs.iter()
            .map(|r| (r.platform.clone(), r.seed, r.run))
            .collect()
    };
    if labels(runs) != labels(&expected) {
        return Err(format!(
            "the runs are not the {} runs of the matrix (platforms × seeds × runs, in that order)",
            expected.len()
        ));
    }
    Ok(())
}

/// What run_programs.py reports as `platform.machine()` on each platform.
fn machine(platform: &str) -> Res<&'static str> {
    match platform {
        "linux/arm64" => Ok("aarch64"),
        "linux/amd64" => Ok("x86_64"),
        other => Err(format!("no machine name for platform {other:?}")),
    }
}

/// Requires a run's run.json to show the run the manifest says it is: its seed, platform,
/// limits, worker request, sole input and the pure-Python SymPy environment.
fn check_run_report(
    run: &GoldenRun,
    params: &GoldenParams,
    input_sha256: &str,
    report: &Value,
) -> Res<()> {
    check_report(
        &format!(
            "run.json sha256:{} of {} seed {} run {}",
            run.run_sha256, run.platform, run.seed, run.run
        ),
        report,
        &[
            ("/python_hash_seed", run.seed.to_string().into()),
            ("/machine", machine(&run.platform)?.into()),
            ("/limits/cpu_seconds", params.cpu_seconds.into()),
            ("/limits/mem_bytes", params.mem_bytes.into()),
            ("/pids/jobs_requested", params.jobs.into()),
            ("/sympy/ground_types", "python".into()),
            ("/sympy/mpmath_backend", "python".into()),
            ("/inputs/0/sha256", input_sha256.into()),
        ],
    )
}

/// Requires every `(pointer, value)` of `expected` in a run_programs.py run.json, and exactly
/// one input. `what` names the report in errors.
fn check_report(what: &str, report: &Value, expected: &[(&str, Value)]) -> Res<()> {
    for (pointer, value) in expected {
        match report.pointer(pointer) {
            Some(found) if found == value => {}
            found => {
                return Err(format!(
                    "{what}: {pointer} is {}, expected {value}",
                    found.map_or_else(|| "absent".to_owned(), Value::to_string)
                ));
            }
        }
    }
    let inputs = report
        .pointer("/inputs")
        .and_then(Value::as_array)
        .map(Vec::len);
    if inputs != Some(1) {
        return Err(format!("{what}: {inputs:?} inputs, expected exactly one"));
    }
    Ok(())
}

/// The verified bytes of a stored blob (cache, else the remote store).
fn fetch_bytes(cache: &Cache, remote: &Remote, hex: &str) -> Res<Vec<u8>> {
    let (_, blob) = store::fetch_blob(cache, hex, None, Some(remote))?;
    fs::read(&blob).map_err(|e| format!("read {}: {e}", blob.display()))
}

/// Re-derives the manifest's derived fields (counts, golden_sha256, adjudication) from its runs'
/// stored blobs, after checking the matrix and each run's report. Returns the golden bytes. The
/// one path both `golden` and `--verify` take, so finalisation is idempotent.
fn finalise(cache: &Cache, manifest: &mut GoldenManifest) -> Res<Vec<u8>> {
    if manifest.schema != GOLDEN_SCHEMA {
        return Err(format!(
            "golden manifest schema {:?}, expected {GOLDEN_SCHEMA:?}",
            manifest.schema
        ));
    }
    check_name("golden manifest name", &manifest.name)?;
    store::check_digest(&manifest.input_sha256)?;
    check_matrix(&manifest.params, &manifest.runs)?;
    let remote = Remote::from_env();
    let mut blobs = Vec::with_capacity(manifest.runs.len());
    for run in &manifest.runs {
        let report = fetch_bytes(cache, &remote, &run.run_sha256)?;
        let report: Value = serde_json::from_slice(&report)
            .map_err(|e| format!("run.json sha256:{}: {e}", run.run_sha256))?;
        check_run_report(run, &manifest.params, &manifest.input_sha256, &report)?;
        blobs.push(fetch_bytes(cache, &remote, &run.records_sha256)?);
    }
    let labelled: Vec<(&GoldenRun, &[u8])> = manifest
        .runs
        .iter()
        .zip(blobs.iter().map(Vec::as_slice))
        .collect();
    let judgement = judge(&labelled)?;
    manifest.counts = judgement.counts;
    manifest.golden_sha256 = ksci_wire::sha256_hex(&judgement.golden);
    manifest.adjudication = judgement.adjudication;
    Ok(judgement.golden)
}

/// The manifest file of `name`.
fn golden_path(root: &Path, name: &str) -> PathBuf {
    root.join(GOLDEN_DIR).join(format!("{name}.golden.json"))
}

/// The normalised params of a golden run: seeds and platforms sorted and deduplicated.
fn golden_params(args: &GoldenArgs) -> Res<GoldenParams> {
    let mut seeds = args.seeds.clone();
    seeds.sort_unstable();
    seeds.dedup();
    let mut platforms = args.platforms.clone();
    platforms.sort();
    platforms.dedup();
    if args.jobs == 0 || args.cpu_seconds == 0 {
        return Err("--jobs and --cpu-seconds must be positive".to_owned());
    }
    Ok(GoldenParams {
        seeds,
        runs: args.runs,
        platforms,
        cpu_seconds: args.cpu_seconds,
        mem_bytes: GOLDEN_MEM_BYTES,
        jobs: args.jobs,
    })
}

/// The sha256 of the programs: a path is put into the store, a digest must be fetchable.
fn store_input(cache: &Cache, spec: &str) -> Res<String> {
    let path = Path::new(spec);
    if path.exists() {
        return store::put(cache, path);
    }
    store::check_digest(spec).map_err(|e| format!("--input {spec}: not a file, and {e}"))?;
    store::fetch_blob(cache, spec, None, Some(&Remote::from_env()))?;
    Ok(spec.to_owned())
}

/// The sympy pins of `platforms` as the manifest records them.
fn golden_images(
    pins: &Pins,
    platforms: &[String],
    native: &str,
) -> Res<BTreeMap<String, GoldenImage>> {
    let image = pins
        .images
        .get(GOLDEN_IMAGE)
        .ok_or_else(|| format!("image {GOLDEN_IMAGE:?} is not pinned in {PINS_PATH}"))?;
    platforms
        .iter()
        .map(|p| {
            let pin = image.platforms.get(p).ok_or_else(|| {
                format!("image {GOLDEN_IMAGE:?} has no pin for {p} in {PINS_PATH}")
            })?;
            Ok((
                p.clone(),
                GoldenImage {
                    identity: pin.identity.clone(),
                    tar_sha256: pin.tar_sha256.clone(),
                    emulated: p != native,
                },
            ))
        })
        .collect()
}

/// `oracle golden`: runs the matrix, judges it, stores the goldens and writes the manifest. The
/// manifest goes to a temporary file renamed into place only after every run completed and the
/// goldens are stored, so an interrupted or failed run leaves no manifest.
fn golden(args: &GoldenArgs) -> Res<()> {
    let name = args.name.as_deref().ok_or("--name is required")?;
    let input = args.input.as_deref().ok_or("--input is required")?;
    check_name("golden manifest name", name)?;
    let params = golden_params(args)?;
    check_matrix(&params, &matrix_runs(&params))?;
    let root = workspace::root()?;
    let cache = Cache::from_env()?;
    let input_sha256 = store_input(&cache, input)?;
    obtain_pinned(GOLDEN_IMAGE, &params.platforms)?;
    let native = native_platform()?;
    let images = golden_images(&load_pins(&root)?, &params.platforms, &native)?;
    let blob = host::git_output(&root, &["hash-object", "--", RUN_PROGRAMS])?;
    let run_programs_blob = String::from_utf8(blob)
        .map_err(|e| format!("git hash-object {RUN_PROGRAMS}: {e}"))?
        .trim()
        .to_owned();

    let total = matrix_runs(&params).len();
    let mut runs = Vec::with_capacity(total);
    // Repetition-major, so the two runs of one (platform, seed) are far apart in time.
    for run in 1..=params.runs {
        for platform in &params.platforms {
            for &seed in &params.seeds {
                println!(
                    "golden {name}: run {}/{total}: {platform} seed {seed} run {run}",
                    runs.len() + 1
                );
                let (_, outputs) = run_script(&RunArgs {
                    image: GOLDEN_IMAGE.to_owned(),
                    platform: platform.clone(),
                    script: PathBuf::from(RUN_PROGRAMS),
                    inputs: vec![input_sha256.clone()],
                    seed,
                    jobs: Some(params.jobs),
                    cpu_seconds: Some(params.cpu_seconds),
                    prelude: None,
                    mount_standin: false,
                    require_sympy_env: true,
                    script_args: vec!["--mem-bytes".to_owned(), params.mem_bytes.to_string()],
                })?;
                let output = |file: &str| {
                    outputs.get(file).cloned().ok_or_else(|| {
                        format!(
                            "{platform} seed {seed} run {run} stored no {file} (the run failed); no manifest written"
                        )
                    })
                };
                runs.push(GoldenRun {
                    platform: platform.clone(),
                    seed,
                    run,
                    records_sha256: output("records.jsonl")?,
                    run_sha256: output("run.json")?,
                });
            }
        }
    }
    runs.sort_by(|a, b| (&a.platform, a.seed, a.run).cmp(&(&b.platform, b.seed, b.run)));
    // The pins must not have moved while the runs were going on.
    if golden_images(&load_pins(&root)?, &params.platforms, &native)? != images {
        return Err(format!(
            "the {GOLDEN_IMAGE} pins in {PINS_PATH} changed during the runs; no manifest written"
        ));
    }

    let mut manifest = GoldenManifest {
        schema: GOLDEN_SCHEMA.to_owned(),
        name: name.to_owned(),
        input_sha256,
        images,
        run_programs_blob,
        params,
        runs,
        counts: GoldenCounts::default(),
        golden_sha256: String::new(),
        adjudication: Vec::new(),
    };
    let golden = finalise(&cache, &mut manifest)?;
    let scratch = store::target_dir()?
        .join("oracle")
        .join(format!("golden-{}.jsonl", std::process::id()));
    if let Some(parent) = scratch.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("create {}: {e}", parent.display()))?;
    }
    fs::write(&scratch, &golden).map_err(|e| format!("write {}: {e}", scratch.display()))?;
    let stored = store::put(&cache, &scratch);
    let _ = fs::remove_file(&scratch);
    if stored? != manifest.golden_sha256 {
        return Err("the stored goldens do not hash to golden_sha256".to_owned());
    }

    let bytes = ksci_wire::to_jcs(&manifest).map_err(|e| format!("encode manifest: {e}"))?;
    let path = golden_path(&root, name);
    let partial = path.with_extension(format!("json.partial.{}", std::process::id()));
    fs::write(&partial, &bytes).map_err(|e| format!("write {}: {e}", partial.display()))?;
    fs::rename(&partial, &path).map_err(|e| format!("rename into {}: {e}", path.display()))?;
    print_golden(&manifest);
    println!("wrote {GOLDEN_DIR}/{name}.golden.json");
    Ok(())
}

/// Every run of the matrix, in (platform, seed, run) order, with empty digests.
fn matrix_runs(params: &GoldenParams) -> Vec<GoldenRun> {
    let mut runs = Vec::new();
    for platform in &params.platforms {
        for &seed in &params.seeds {
            for run in 1..=params.runs {
                runs.push(GoldenRun {
                    platform: platform.clone(),
                    seed,
                    run,
                    records_sha256: String::new(),
                    run_sha256: String::new(),
                });
            }
        }
    }
    runs
}

fn print_golden(manifest: &GoldenManifest) {
    let c = &manifest.counts;
    println!(
        "golden {}: cases={} golden={} adjudication={} golden_sha256={}",
        manifest.name, c.cases, c.golden, c.adjudication, manifest.golden_sha256
    );
    let join = |m: &BTreeMap<String, usize>| {
        m.iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join(" ")
    };
    println!("golden by status: {}", join(&c.golden_by_status));
    println!(
        "adjudication by reason: {}",
        join(&c.adjudication_by_reason)
    );
}

/// `oracle golden --verify <manifest>`: re-derives the manifest from its stored run blobs and
/// requires byte-identical JCS. On a difference the regenerated bytes are left in
/// `<target>/oracle/` for inspection.
fn verify_golden(path: &Path) -> Res<()> {
    let bytes = fs::read(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let manifest: GoldenManifest =
        serde_json::from_slice(&bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    let cache = Cache::from_env()?;
    let mut derived = manifest.clone();
    finalise(&cache, &mut derived)?;
    // The goldens themselves must be retrievable (CONF-02); the fetch re-hashes them.
    fetch_bytes(&cache, &Remote::from_env(), &derived.golden_sha256)?;
    let regenerated = ksci_wire::to_jcs(&derived).map_err(|e| format!("encode manifest: {e}"))?;
    if regenerated != bytes {
        let out = store::target_dir()?
            .join("oracle")
            .join(format!("verify-{}.golden.json", std::process::id()));
        if let Some(parent) = out.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("create {}: {e}", parent.display()))?;
        }
        fs::write(&out, &regenerated).map_err(|e| format!("write {}: {e}", out.display()))?;
        return Err(format!(
            "{}: the regenerated manifest differs (regenerated bytes in {}){}",
            path.display(),
            out.display(),
            if derived == manifest {
                "; the fields agree, so the file is not in JCS form"
            } else {
                ""
            }
        ));
    }
    print_golden(&derived);
    println!(
        "verified {}: the manifest re-derives byte for byte from {} stored runs",
        path.display(),
        derived.runs.len()
    );
    Ok(())
}

// The stand-in baseline (CONF-03, GUEST-02).

/// Where stand-in score files live, relative to the checkout.
pub const SCORES_DIR: &str = "conformance/golden/scores";
/// The schema tag of a stand-in score file: the scorer's aggregate (its keys, including the three
/// levels and the stricter `level1_message_equal`, are documented in [`SCORER`]) plus the digests,
/// label and parameters `oracle standin` adds beside them.
pub const STANDIN_SCHEMA: &str = "ksci.standin-score.v1";
/// The three-level scorer, relative to the checkout.
pub const SCORER: &str = "conformance/runner/score.py";
/// What every stand-in score is until Klein runs the guest under Monty (klein-rs D4).
pub const STANDIN_LABEL: &str = "proxy: CPython host_shim until klein-rs D4";
/// The image the stand-in runs in: the pinned base image, which has no SymPy.
const STANDIN_IMAGE: &str = "base";
/// The prelude that installs the CPython tool transport. No KSCI_WORKER reaches the container,
/// so the installed transport refuses every tool call.
const STANDIN_PRELUDE: &str = "host_shim";
/// The hash seed of a stand-in run (the production value).
const STANDIN_SEED: u32 = 0;

/// The trimmed UTF-8 stdout of the pinned `git -C <root> <args>`.
fn git_text(root: &Path, args: &[&str]) -> Res<String> {
    let out = host::git_output(root, args)?;
    String::from_utf8(out)
        .map(|s| s.trim().to_owned())
        .map_err(|e| format!("git {}: {e}", args.join(" ")))
}

/// `git status --porcelain` of standin/, untracked files included: empty when the mounted tree
/// is the committed tree the score names.
fn standin_changes(root: &Path) -> Res<String> {
    git_text(
        root,
        &[
            "status",
            "--porcelain",
            "--untracked-files=all",
            "--",
            "standin",
        ],
    )
}

/// The score file: the scorer's aggregate (it owns those keys) with `fields` added. A field the
/// aggregate already has is an error, never an overwrite.
fn merge_score(aggregate: Value, fields: serde_json::Map<String, Value>) -> Res<Value> {
    let Value::Object(mut score) = aggregate else {
        return Err("the scorer's aggregate is not a JSON object".to_owned());
    };
    for (key, value) in fields {
        if score.contains_key(&key) {
            return Err(format!(
                "the scorer's aggregate already has the key {key:?}"
            ));
        }
        score.insert(key, value);
    }
    Ok(Value::Object(score))
}

/// Runs the dev venv's Python with `args` from the checkout root, with PYTHONPATH set to the
/// checkout directories `pythonpath`; returns its exit code (None when killed by a signal).
fn venv_python(root: &Path, pythonpath: &[&str], args: &[&OsStr]) -> Res<Option<i32>> {
    let python = host::venv_python_path(root);
    if !python.is_file() {
        return Err(format!(
            "{} is missing; run: cargo xtask tools venv",
            python.display()
        ));
    }
    let paths: Vec<PathBuf> = pythonpath.iter().map(|p| root.join(p)).collect();
    let pythonpath =
        std::env::join_paths(&paths).map_err(|e| format!("PYTHONPATH {paths:?}: {e}"))?;
    let status = Command::new(&python)
        .args(args)
        .env("PYTHONPATH", pythonpath)
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .current_dir(root)
        .stdin(Stdio::null())
        .status()
        .map_err(|e| format!("run {}: {e}", python.display()))?;
    Ok(status.code())
}

/// `oracle standin`: runs the stand-in on the programs of a verified golden manifest through
/// [`run_script`] (base image, standin/ read-only, host_shim prelude, seed 0, the manifest's
/// memory limit), scores the records with [`SCORER`], stores the per-case results, runs the
/// Monty-subset lint, and writes the aggregate with its digests to
/// `conformance/golden/scores/<name>.m0-standin.json`, labelled [`STANDIN_LABEL`]. Only the
/// aggregate and digests go in git; records and per-case results stay in the store.
fn standin(args: &StandinArgs) -> Res<()> {
    check_name("score name", &args.name)?;
    check_platform(&args.platform)?;
    let root = workspace::root()?;
    // The score names the stand-in by its committed tree, so the mounted tree must be that tree.
    let changes = standin_changes(&root)?;
    if !changes.is_empty() {
        return Err(format!(
            "standin/ differs from HEAD, so the score could not name it; commit it first:\n{changes}"
        ));
    }
    let standin_tree = git_text(&root, &["rev-parse", "HEAD:standin"])?;
    let run_programs_blob = git_text(&root, &["hash-object", "--", RUN_PROGRAMS])?;
    let scorer_blob = git_text(&root, &["hash-object", "--", SCORER])?;

    // The reference: a manifest that re-derives byte for byte from its stored runs.
    verify_golden(&args.golden_manifest)?;
    let manifest_bytes = fs::read(&args.golden_manifest)
        .map_err(|e| format!("read {}: {e}", args.golden_manifest.display()))?;
    let manifest: GoldenManifest = serde_json::from_slice(&manifest_bytes)
        .map_err(|e| format!("{}: {e}", args.golden_manifest.display()))?;
    let cpu_seconds = args.cpu_seconds.unwrap_or(manifest.params.cpu_seconds);
    let jobs = args.jobs.unwrap_or(manifest.params.jobs);
    if cpu_seconds == 0 || jobs == 0 {
        return Err("--jobs and --cpu-seconds must be positive".to_owned());
    }
    let mem_bytes = manifest.params.mem_bytes;
    let cache = Cache::from_env()?;
    let remote = Remote::from_env();
    let (_, golden_blob) = store::fetch_blob(&cache, &manifest.golden_sha256, None, Some(&remote))?;

    let platforms = [args.platform.clone()];
    obtain_pinned(STANDIN_IMAGE, &platforms)?;
    let identity = |pins: &Pins| {
        pins.images
            .get(STANDIN_IMAGE)
            .and_then(|i| i.platforms.get(&args.platform))
            .map(|p| p.identity.clone())
            .ok_or_else(|| format!("image {STANDIN_IMAGE:?} has no pin for {}", args.platform))
    };
    let image = identity(&load_pins(&root)?)?;
    let (_, outputs) = run_script(&RunArgs {
        image: STANDIN_IMAGE.to_owned(),
        platform: args.platform.clone(),
        script: PathBuf::from(RUN_PROGRAMS),
        inputs: vec![manifest.input_sha256.clone()],
        seed: STANDIN_SEED,
        jobs: Some(jobs),
        cpu_seconds: Some(cpu_seconds),
        prelude: Some(STANDIN_PRELUDE.to_owned()),
        mount_standin: true,
        require_sympy_env: false,
        script_args: vec!["--mem-bytes".to_owned(), mem_bytes.to_string()],
    })?;
    let output = |file: &str| {
        outputs.get(file).cloned().ok_or_else(|| {
            format!("the stand-in run stored no {file} (the run failed); no score written")
        })
    };
    let candidate_sha256 = output("records.jsonl")?;
    let candidate_run_sha256 = output("run.json")?;
    // Neither the stand-in nor its pin may have moved while it ran.
    if !standin_changes(&root)?.is_empty()
        || git_text(&root, &["rev-parse", "HEAD:standin"])? != standin_tree
    {
        return Err("standin/ changed during the run; no score written".to_owned());
    }
    if identity(&load_pins(&root)?)? != image {
        return Err(format!(
            "the {STANDIN_IMAGE} pin in {PINS_PATH} changed during the run; no score written"
        ));
    }
    let report: Value = serde_json::from_slice(&read_blob(&cache, &candidate_run_sha256)?)
        .map_err(|e| format!("run.json sha256:{candidate_run_sha256}: {e}"))?;
    check_standin_report(
        &format!("stand-in run.json sha256:{candidate_run_sha256}"),
        &report,
        &args.platform,
        cpu_seconds,
        mem_bytes,
        jobs,
        &manifest.input_sha256,
    )?;

    // Score: the goldens against the stand-in's records, the adjudicated cases excluded.
    let scratch = store::target_dir()?
        .join("oracle")
        .join(format!("standin-{}", std::process::id()));
    if scratch.exists() {
        fs::remove_dir_all(&scratch).map_err(|e| format!("remove {}: {e}", scratch.display()))?;
    }
    fs::create_dir_all(&scratch).map_err(|e| format!("create {}: {e}", scratch.display()))?;
    let exclude = scratch.join("exclude.txt");
    let excluded: String = manifest
        .adjudication
        .iter()
        .map(|a| format!("{}\n", a.case_id))
        .collect();
    fs::write(&exclude, excluded).map_err(|e| format!("write {}: {e}", exclude.display()))?;
    let aggregate_path = scratch.join("aggregate.json");
    let percase_path = scratch.join("percase.jsonl");
    let candidate_blob = cache
        .lookup(&candidate_sha256)?
        .ok_or_else(|| format!("sha256:{candidate_sha256} is not in the cache"))?;
    let scorer_args: Vec<&OsStr> = vec![
        SCORER.as_ref(),
        "--golden".as_ref(),
        golden_blob.as_os_str(),
        "--candidate".as_ref(),
        candidate_blob.as_os_str(),
        "--exclude".as_ref(),
        exclude.as_os_str(),
        "--out".as_ref(),
        aggregate_path.as_os_str(),
        "--cases-out".as_ref(),
        percase_path.as_os_str(),
    ];
    if venv_python(&root, &["standin", "conformance"], &scorer_args)? != Some(0) {
        return Err(format!("{SCORER} failed; no score written"));
    }
    let aggregate: Value = serde_json::from_slice(
        &fs::read(&aggregate_path)
            .map_err(|e| format!("read {}: {e}", aggregate_path.display()))?,
    )
    .map_err(|e| format!("{}: {e}", aggregate_path.display()))?;
    let percase_sha256 = store::put(&cache, &percase_path)?;
    let _ = fs::remove_dir_all(&scratch);

    // GUEST-02: the stand-in holds to the Monty subset.
    let lint_args: Vec<&OsStr> = vec!["-m".as_ref(), "lint".as_ref(), "standin/sympy".as_ref()];
    let lint = match venv_python(&root, &["standin"], &lint_args)? {
        Some(0) => "pass",
        Some(1) => "fail",
        other => {
            return Err(format!(
                "the Monty-subset lint did not run (exit {other:?})"
            ));
        }
    };

    let mut fields = serde_json::Map::new();
    let mut put = |key: &str, value: Value| fields.insert(key.to_owned(), value);
    put("schema", STANDIN_SCHEMA.into());
    put("name", args.name.clone().into());
    put("label", STANDIN_LABEL.into());
    put("golden_manifest", manifest.name.clone().into());
    put(
        "golden_manifest_sha256",
        ksci_wire::sha256_hex(&manifest_bytes).into(),
    );
    put("golden_sha256", manifest.golden_sha256.clone().into());
    put("input_sha256", manifest.input_sha256.clone().into());
    put("candidate_sha256", candidate_sha256.into());
    put("candidate_run_sha256", candidate_run_sha256.into());
    put("percase_sha256", percase_sha256.into());
    put("standin_tree", standin_tree.into());
    put("run_programs_blob", run_programs_blob.into());
    put("scorer_blob", scorer_blob.into());
    put("lint", lint.into());
    put("platform", args.platform.clone().into());
    put("image", image.into());
    put(
        "params",
        serde_json::json!({
            "seed": STANDIN_SEED,
            "cpu_seconds": cpu_seconds,
            "mem_bytes": mem_bytes,
            "jobs": jobs,
            "prelude": STANDIN_PRELUDE,
        }),
    );
    let score = merge_score(aggregate, fields)?;
    let bytes = ksci_wire::to_jcs(&score).map_err(|e| format!("encode score: {e}"))?;
    let dir = root.join(SCORES_DIR);
    fs::create_dir_all(&dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    let path = dir.join(format!("{}.m0-standin.json", args.name));
    let partial = path.with_extension(format!("json.partial.{}", std::process::id()));
    fs::write(&partial, &bytes).map_err(|e| format!("write {}: {e}", partial.display()))?;
    fs::rename(&partial, &path).map_err(|e| format!("rename into {}: {e}", path.display()))?;
    let field = |key: &str| match score.get(key) {
        Some(Value::String(s)) => s.clone(),
        Some(v) => v.to_string(),
        None => String::new(),
    };
    println!(
        "standin {} ({}): scored={} excluded={} missing={} level1={}% level2={}% level3={}% level1_message_equal={}% error_message_equal={}/{} lint={lint}",
        args.name,
        STANDIN_LABEL,
        field("scored"),
        field("excluded"),
        field("missing"),
        field("level1_percent"),
        field("level2_percent"),
        field("level3_percent"),
        field("level1_message_equal_percent"),
        field("error_message_equal"),
        field("error_cases"),
    );
    println!("wrote {SCORES_DIR}/{}.m0-standin.json", args.name);
    Ok(())
}

/// Requires a stand-in run.json to show the run `standin` asked for: seed 0, the platform, the
/// limits, the host_shim prelude, the worker request, the manifest's sole input, and no SymPy
/// environment in the image (`sympy` is null: what ran is the stand-in, not a real SymPy).
fn check_standin_report(
    what: &str,
    report: &Value,
    platform: &str,
    cpu_seconds: u32,
    mem_bytes: u64,
    jobs: u32,
    input_sha256: &str,
) -> Res<()> {
    check_report(
        what,
        report,
        &[
            ("/python_hash_seed", STANDIN_SEED.to_string().into()),
            ("/machine", machine(platform)?.into()),
            ("/limits/cpu_seconds", cpu_seconds.into()),
            ("/limits/mem_bytes", mem_bytes.into()),
            ("/limits/prelude", STANDIN_PRELUDE.into()),
            ("/pids/jobs_requested", jobs.into()),
            ("/sympy", Value::Null),
            ("/inputs/0/sha256", input_sha256.into()),
        ],
    )
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
        let command = ["/opt/ksci/conformance/x.py".to_owned()];
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
        // One source for the task limit: the flag and the value the harness budgets against.
        assert!(argv.contains(&"KSCI_PIDS_LIMIT=512"), "{argv:?}");
        let after = |flag: &str| argv[argv.iter().position(|a| *a == flag).unwrap() + 1];
        // Root at start, but only to hand exactly three capabilities to the launcher (review R1).
        assert_eq!(after("--user"), "0:0");
        let added: Vec<&&str> = argv.iter().filter(|a| a.starts_with("--cap-add")).collect();
        assert_eq!(
            added,
            [&"--cap-add=SETUID", &"--cap-add=SETGID", &"--cap-add=KILL"]
        );
        let tmpfs: Vec<&str> = argv
            .iter()
            .enumerate()
            .filter(|(_, a)| **a == "--tmpfs")
            .map(|(i, _)| argv[i + 1])
            .collect();
        assert_eq!(
            tmpfs,
            [
                "/tmp:rw,size=1g,mode=1777",
                "/harness:rw,size=1m,mode=0700,uid=65534,gid=65534"
            ]
        );
        assert_eq!(after("--platform"), "linux/arm64");
        assert!(
            argv.contains(
                &"type=bind,source=/repo/conformance,target=/opt/ksci/conformance,readonly"
            )
        );
        // The output directory is the only writable mount, and it lies in the harness's tmpfs.
        let writable: Vec<&&str> = argv
            .iter()
            .filter(|a| a.starts_with("type=bind") && !a.ends_with(",readonly"))
            .collect();
        assert_eq!(writable, [&"type=bind,source=/tmp/out,target=/harness/out"]);
        // The image comes after every flag, then the launcher, then the script.
        let image = argv.iter().position(|a| *a == BASE_REFERENCE).unwrap();
        assert_eq!(
            &argv[image + 1..],
            ["python3", LAUNCHER, "/opt/ksci/conformance/x.py"]
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

    /// Review W1: what runs is the verified, content-addressed identity, never the mutable tag a
    /// concurrent build could move between the check and the run.
    #[test]
    fn oracle_runs_the_pinned_identity_not_the_tag() {
        let pin = PlatformPin {
            reference: built_tag("sympy", "linux/arm64"),
            identity: format!("sha256:{}", "a".repeat(64)),
            manifest: format!("sha256:{}", "d".repeat(64)),
            emulated: false,
            tar_sha256: None,
        };
        assert_eq!(run_image(&pin), pin.identity);
        let argv = run_argv(
            "linux/arm64",
            run_image(&pin),
            &[],
            Path::new("/tmp/out"),
            &[],
            &["python3".to_owned()],
        )
        .unwrap();
        assert!(
            argv.iter()
                .any(|a| a.to_str() == Some(pin.identity.as_str()))
        );
        assert!(
            !argv
                .iter()
                .any(|a| a.to_str() == Some(pin.reference.as_str()))
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
                reference: built_tag("sympy", "linux/arm64"),
                identity: format!("sha256:{}", "a".repeat(64)),
                manifest: format!("sha256:{}", "d".repeat(64)),
                emulated: false,
                tar_sha256: Some("b".repeat(64)),
            },
        );
        pins.images.insert(
            "sympy".to_owned(),
            ImagePin {
                platforms,
                introspection_sha256: Some("c".repeat(64)),
            },
        );
        let bytes = pins.to_bytes().unwrap();
        let text = std::str::from_utf8(&bytes).unwrap();
        assert!(
            text.starts_with(r#"{"images":{"sympy":{"introspection_sha256":"cccc"#),
            "{text}"
        );
        assert!(
            text.contains(r#""linux/arm64":{"emulated":false,"identity":"sha256:aaaa"#),
            "{text}"
        );
        assert!(
            text.contains(r#""reference":"ksci-oracle-sympy:linux-arm64""#),
            "{text}"
        );
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

    fn params() -> GoldenParams {
        GoldenParams {
            seeds: GOLDEN_SEEDS.to_vec(),
            runs: 2,
            platforms: vec!["linux/amd64".to_owned(), "linux/arm64".to_owned()],
            cpu_seconds: 60,
            mem_bytes: GOLDEN_MEM_BYTES,
            jobs: 4,
        }
    }

    /// The canonical records of one run, from (case_id, status, stdout) triples.
    fn records(cases: &[(&str, &str, String)]) -> Vec<u8> {
        let mut out = Vec::new();
        for (case_id, status, stdout) in cases {
            let value = serde_json::json!({"case_id": case_id, "status": status, "stdout": stdout});
            out.extend(ksci_wire::to_jcs(&value).unwrap());
            out.push(b'\n');
        }
        out
    }

    /// The D-10 matrix over six cases, one per outcome: `a` and `f` agree everywhere, `b` prints
    /// in a seed-dependent order, `c` differs by platform, `d` times out only on amd64, and `e`
    /// differs only between repetitions.
    #[test]
    fn golden_judge_admits_only_agreeing_cases() {
        let runs = matrix_runs(&params());
        assert_eq!(runs.len(), 16);
        let blobs: Vec<Vec<u8>> = runs
            .iter()
            .map(|r| {
                let amd = r.platform == "linux/amd64";
                records(&[
                    ("a", "ok", "1".to_owned()),
                    ("b", "ok", format!("{{order {}}}", r.seed % 2)),
                    ("c", "ok", r.platform.clone()),
                    ("d", if amd { "timeout" } else { "ok" }, String::new()),
                    ("e", "ok", format!("run {}", r.run)),
                    ("f", "timeout", String::new()),
                ])
            })
            .collect();
        let labelled: Vec<(&GoldenRun, &[u8])> =
            runs.iter().zip(blobs.iter().map(Vec::as_slice)).collect();
        let j = judge(&labelled).unwrap();
        let reasons: Vec<(&str, &str)> = j
            .adjudication
            .iter()
            .map(|a| (a.case_id.as_str(), a.reason.as_str()))
            .collect();
        assert_eq!(
            reasons,
            [
                ("b", "seed"),
                ("c", "platform"),
                ("d", "status"),
                ("e", "run")
            ]
        );
        assert_eq!(
            j.golden,
            records(&[("a", "ok", "1".to_owned()), ("f", "timeout", String::new())])
        );
        assert_eq!(
            (j.counts.cases, j.counts.golden, j.counts.adjudication),
            (6, 2, 4)
        );
        let by_status: Vec<(&str, usize)> = j
            .counts
            .golden_by_status
            .iter()
            .map(|(k, v)| (k.as_str(), *v))
            .collect();
        assert_eq!(by_status, [("error", 0), ("ok", 1), ("timeout", 1)]);
        assert_eq!(j.counts.adjudication_by_reason.len(), REASONS.len());
        assert!(j.counts.adjudication_by_reason.values().all(|n| *n == 1));
    }

    /// A run that lacks a case (or holds an extra one) is a harness fault, never a judgement.
    #[test]
    fn golden_judge_requires_the_same_cases_in_every_run() {
        let runs = matrix_runs(&params());
        let full = records(&[("a", "ok", String::new()), ("b", "ok", String::new())]);
        let short = records(&[("a", "ok", String::new())]);
        let mut labelled: Vec<(&GoldenRun, &[u8])> =
            runs.iter().map(|r| (r, full.as_slice())).collect();
        labelled[5].1 = &short;
        assert!(judge(&labelled).is_err());
        assert!(judge(&[]).is_err());
    }

    #[test]
    fn golden_matrix_is_exact() {
        let p = params();
        let runs = matrix_runs(&p);
        check_matrix(&p, &runs).unwrap();
        let mut missing = runs.clone();
        missing.pop();
        assert!(check_matrix(&p, &missing).is_err());
        let mut swapped = runs.clone();
        swapped.swap(0, 1);
        assert!(check_matrix(&p, &swapped).is_err());
        let mut unsorted = p.clone();
        unsorted.seeds.reverse();
        assert!(check_matrix(&unsorted, &matrix_runs(&unsorted)).is_err());
        let mut unknown = p.clone();
        unknown.platforms = vec!["linux/riscv64".to_owned()];
        assert!(check_matrix(&unknown, &matrix_runs(&unknown)).is_err());
        let mut none = p;
        none.runs = 0;
        assert!(check_matrix(&none, &matrix_runs(&none)).is_err());
    }

    #[test]
    fn golden_run_report_must_match_the_run() {
        let p = params();
        let input = "a".repeat(64);
        let run = GoldenRun {
            platform: "linux/amd64".to_owned(),
            seed: 4294967295,
            run: 2,
            records_sha256: "b".repeat(64),
            run_sha256: "c".repeat(64),
        };
        let report = serde_json::json!({
            "python_hash_seed": "4294967295",
            "machine": "x86_64",
            "limits": {"cpu_seconds": 60, "mem_bytes": GOLDEN_MEM_BYTES},
            "pids": {"jobs_requested": 4},
            "inputs": [{"name": input, "sha256": input, "cases": 8}],
            "sympy": {"ground_types": "python", "mpmath_backend": "python"},
        });
        check_run_report(&run, &p, &input, &report).unwrap();
        let broken = |pointer: &str, value: Value| {
            let mut r = report.clone();
            *r.pointer_mut(pointer).unwrap() = value;
            check_run_report(&run, &p, &input, &r).is_err()
        };
        assert!(broken("/python_hash_seed", "0".into()));
        assert!(broken("/machine", "aarch64".into()));
        assert!(broken("/limits/cpu_seconds", 2.into()));
        assert!(broken("/limits/mem_bytes", 1.into()));
        assert!(broken("/pids/jobs_requested", 8.into()));
        assert!(broken("/inputs/0/sha256", "d".repeat(64).into()));
        assert!(broken("/sympy/ground_types", "gmpy".into()));
        assert!(broken("/sympy/mpmath_backend", "gmpy".into()));
        let mut two = report.clone();
        let first = two["inputs"][0].clone();
        two["inputs"].as_array_mut().unwrap().push(first);
        assert!(check_run_report(&run, &p, &input, &two).is_err());
    }

    #[test]
    fn golden_manifest_roundtrips_as_jcs() {
        let mut images = BTreeMap::new();
        images.insert(
            "linux/arm64".to_owned(),
            GoldenImage {
                identity: format!("sha256:{}", "a".repeat(64)),
                tar_sha256: Some("b".repeat(64)),
                emulated: false,
            },
        );
        let manifest = GoldenManifest {
            schema: GOLDEN_SCHEMA.to_owned(),
            name: "smoke-v1".to_owned(),
            input_sha256: "c".repeat(64),
            images,
            run_programs_blob: "d".repeat(40),
            params: params(),
            runs: matrix_runs(&params()),
            counts: GoldenCounts::default(),
            golden_sha256: "e".repeat(64),
            adjudication: vec![Adjudication {
                case_id: "f".repeat(64),
                reason: "seed".to_owned(),
            }],
        };
        let bytes = ksci_wire::to_jcs(&manifest).unwrap();
        let text = std::str::from_utf8(&bytes).unwrap();
        assert!(
            text.starts_with(r#"{"adjudication":[{"case_id":"ffff"#),
            "{text}"
        );
        assert!(text.contains(r#""seeds":[0,1,2,4294967295]"#), "{text}");
        let back: GoldenManifest = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(back, manifest);
        let unknown = text.replace(r#""name""#, r#""stdout":"x","name""#);
        assert!(serde_json::from_str::<GoldenManifest>(&unknown).is_err());
    }

    /// The scorer owns its keys: xtask adds digests and the label beside them, never over them.
    #[test]
    fn standin_score_merge_never_overwrites() {
        let aggregate = serde_json::json!({"scored": 9, "level1": 9, "level1_percent": "100.0"});
        let mut fields = serde_json::Map::new();
        fields.insert("label".to_owned(), STANDIN_LABEL.into());
        let merged = merge_score(aggregate.clone(), fields).unwrap();
        assert_eq!(merged["scored"], 9);
        assert_eq!(merged["label"], STANDIN_LABEL);
        let text = String::from_utf8(ksci_wire::to_jcs(&merged).unwrap()).unwrap();
        assert_eq!(text.matches("proxy").count(), 1, "{text}");
        let mut clash = serde_json::Map::new();
        clash.insert("scored".to_owned(), 0.into());
        assert!(merge_score(aggregate, clash).is_err());
        assert!(merge_score(Value::Null, serde_json::Map::new()).is_err());
    }

    /// A stand-in run must be the one asked for, and must not have found a real SymPy.
    #[test]
    fn standin_run_report_must_match_the_run() {
        let input = "a".repeat(64);
        let report = serde_json::json!({
            "python_hash_seed": "0",
            "machine": "aarch64",
            "limits": {"cpu_seconds": 10, "mem_bytes": GOLDEN_MEM_BYTES, "prelude": "host_shim"},
            "pids": {"jobs_requested": 4},
            "inputs": [{"name": input, "sha256": input, "cases": 9}],
            "sympy": null,
        });
        let check = |r: &Value| {
            check_standin_report("t", r, "linux/arm64", 10, GOLDEN_MEM_BYTES, 4, &input)
        };
        check(&report).unwrap();
        for (pointer, value) in [
            ("/python_hash_seed", Value::from("1")),
            ("/machine", "x86_64".into()),
            ("/limits/cpu_seconds", 60.into()),
            ("/limits/prelude", Value::Null),
            ("/pids/jobs_requested", 8.into()),
            ("/inputs/0/sha256", "d".repeat(64).into()),
            ("/sympy", serde_json::json!({"sympy_version": "1.14.0"})),
        ] {
            let mut r = report.clone();
            *r.pointer_mut(pointer).unwrap() = value;
            assert!(check(&r).is_err(), "{pointer}");
        }
    }
}
