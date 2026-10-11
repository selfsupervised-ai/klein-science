// provenance: original
//! `cargo xtask release dist`: the artifacts that ship, built under the heavy-job guard (D-04).
//!
//! - R0: `ksci-component` built as std Rust for `wasm32-unknown-unknown` with `+simd128` only (never
//!   relaxed SIMD), then wrapped by the pinned `wasm-tools component new`. The component imports
//!   nothing (D-16).
//! - R2: `ksci-worker` built for the host triple.
//! - `digests.json`: the JCS of each artifact's SHA-256 plus the toolchain and wasm-tools versions.
//!
//! Every artifact lands under `target/dist/` of the checkout ([`workspace::root`]):
//! `science_cas.component.wasm`, `<host-triple>/ksci-worker` and `digests.json`. The conformance
//! host (`ksci-conform`) runs exactly these files.
//!
//! Each cargo build is given its rustflags explicitly as `CARGO_ENCODED_RUSTFLAGS` (research
//! Pitfall I), so neither the caller's `RUSTFLAGS` nor its `CARGO_ENCODED_RUSTFLAGS` reaches a
//! shipped artifact.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use clap::Subcommand;
use serde_json::{Value, json};

use crate::Res;
use crate::host::{self, heavy_job};
use crate::workspace;

/// `cargo xtask release ...`
#[derive(Subcommand)]
pub enum ReleaseCmd {
    /// Build the R0 component and the R2 worker into target/dist and write digests.json.
    Dist,
}

/// Runs one `release` command.
pub fn run(cmd: ReleaseCmd) -> Res<ExitCode> {
    match cmd {
        ReleaseCmd::Dist => dist().map(|()| ExitCode::SUCCESS),
    }
}

/// The wasm target of the R0 core module (D-16).
const R0_TARGET: &str = "wasm32-unknown-unknown";

/// The rustflags of the R0 build: 128-bit SIMD and nothing else. Relaxed SIMD is never enabled; its
/// results are implementation-defined and would break replay.
const R0_RUSTFLAGS: &str = "-Ctarget-feature=+simd128";

/// The shipped component's file name under `target/dist/`.
const COMPONENT: &str = "science_cas.component.wasm";

/// The worker's binary name.
const WORKER: &str = "ksci-worker";

fn dist() -> Res<()> {
    let root = workspace::root()?;
    let cargo = host::cargo()?;
    let rustc = host::toolchain_bin("rustc")?;
    let wasm_tools = pinned_tool("wasm-tools")?;
    let _held = heavy_job()?;

    let target = root.join("target");
    let dist = target.join("dist");
    if dist.exists() {
        fs::remove_dir_all(&dist).map_err(|e| format!("remove {}: {e}", dist.display()))?;
    }

    // a. The R0 core module.
    let mut build = cargo.command();
    build
        .current_dir(&root)
        .env("CARGO_ENCODED_RUSTFLAGS", R0_RUSTFLAGS)
        .args(["build", "--locked", "--release", "-p", "ksci-component"])
        .args(["--target", R0_TARGET, "--target-dir"])
        .arg(&target);
    run_checked(&mut build, "cargo build -p ksci-component")?;
    let core = target
        .join(R0_TARGET)
        .join("release")
        .join("ksci_component.wasm");

    // b. The component: the core module wrapped by the pinned wasm-tools.
    fs::create_dir_all(&dist).map_err(|e| format!("create {}: {e}", dist.display()))?;
    let component = dist.join(COMPONENT);
    let mut wrap = Command::new(&wasm_tools);
    wrap.arg("component")
        .arg("new")
        .arg(&core)
        .arg("-o")
        .arg(&component);
    run_checked(&mut wrap, "wasm-tools component new")?;

    // c. The R2 worker for the host triple.
    let rustc_vv = output_text(Command::new(&rustc).arg("-vV"), "rustc -vV")?;
    let triple = host_triple(&rustc_vv)?;
    let mut build = cargo.command();
    build
        .current_dir(&root)
        .env("CARGO_ENCODED_RUSTFLAGS", "")
        .args([
            "build",
            "--locked",
            "--release",
            "-p",
            WORKER,
            "--target-dir",
        ])
        .arg(&target);
    run_checked(&mut build, "cargo build -p ksci-worker")?;
    let exe = format!("{WORKER}{}", std::env::consts::EXE_SUFFIX);
    let worker_dir = dist.join(&triple);
    fs::create_dir_all(&worker_dir).map_err(|e| format!("create {}: {e}", worker_dir.display()))?;
    let worker = worker_dir.join(&exe);
    let built = target.join("release").join(&exe);
    fs::copy(&built, &worker)
        .map_err(|e| format!("copy {} to {}: {e}", built.display(), worker.display()))?;

    // d. digests.json, in JCS.
    let wasm_tools_version = output_text(
        Command::new(&wasm_tools).arg("--version"),
        "wasm-tools --version",
    )?;
    let mut artifacts = BTreeMap::new();
    for path in [&component, &worker] {
        let rel = relative(&dist, path)?;
        let bytes = fs::read(path).map_err(|e| format!("read {}: {e}", path.display()))?;
        let digest = ksci_wire::sha256_hex(&bytes);
        println!("sha256:{digest} target/dist/{rel}");
        artifacts.insert(rel, Value::String(digest));
    }
    let record = json!({
        "artifacts": artifacts,
        "rustc -vV": rustc_vv.trim_end(),
        "wasm-tools --version": wasm_tools_version.trim_end(),
    });
    let jcs = ksci_wire::to_jcs(&record).map_err(|e| format!("digests.json: {e}"))?;
    let digests = dist.join("digests.json");
    fs::write(&digests, &jcs).map_err(|e| format!("write {}: {e}", digests.display()))?;
    println!(
        "sha256:{} target/dist/digests.json",
        ksci_wire::sha256_hex(&jcs)
    );
    Ok(())
}

/// The absolute path of a tool in [`host::TOOLS`], once it reports exactly its pinned version.
fn pinned_tool(name: &str) -> Res<PathBuf> {
    let tool = host::TOOLS
        .iter()
        .find(|t| t.name == name)
        .ok_or_else(|| format!("{name} is not in the pin table"))?;
    let path = host::tool_path(tool.bin)?;
    let text = output_text(Command::new(&path).args(tool.probe), name)?;
    match host::parse_version(&text) {
        Some(v) if v == tool.version => Ok(path),
        found => Err(format!(
            "{} reports {found:?}; the pin is {}; run: cargo xtask tools install",
            path.display(),
            tool.version
        )),
    }
}

/// The `host:` line of `rustc -vV`.
fn host_triple(rustc_vv: &str) -> Res<String> {
    rustc_vv
        .lines()
        .find_map(|l| l.strip_prefix("host: "))
        .map(|t| t.trim().to_owned())
        .filter(|t| !t.is_empty())
        .ok_or_else(|| "rustc -vV printed no host: line".to_owned())
}

/// `path` relative to `dir`, with `/` separators.
fn relative(dir: &Path, path: &Path) -> Res<String> {
    let rel = path
        .strip_prefix(dir)
        .map_err(|_| format!("{} is not under {}", path.display(), dir.display()))?;
    let parts: Vec<String> = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    Ok(parts.join("/"))
}

/// Runs `cmd`, inheriting stdio; a non-zero exit is an error naming `what`.
fn run_checked(cmd: &mut Command, what: &str) -> Res<()> {
    let status = cmd.status().map_err(|e| format!("run {what}: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{what}: {status}"))
    }
}

/// Runs `cmd` and returns its stdout as UTF-8; a non-zero exit is an error naming `what`.
fn output_text(cmd: &mut Command, what: &str) -> Res<String> {
    let out = cmd.output().map_err(|e| format!("run {what}: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "{what}: {}: {}",
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    String::from_utf8(out.stdout).map_err(|e| format!("{what}: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_host_triple_from_rustc() {
        let vv = "rustc 1.99.0 (b940084d7 2026-09-28)\nbinary: rustc\nhost: aarch64-apple-darwin\nrelease: 1.99.0\n";
        assert_eq!(host_triple(vv).unwrap(), "aarch64-apple-darwin");
        assert!(host_triple("rustc 1.99.0\n").is_err());
    }

    #[test]
    fn release_r0_never_relaxed_simd() {
        assert!(R0_RUSTFLAGS.contains("+simd128"));
        assert!(!R0_RUSTFLAGS.contains("relaxed"));
    }
}
