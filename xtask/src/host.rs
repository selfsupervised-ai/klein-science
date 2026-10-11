// provenance: original
//! Host tools and the heavy-job guard.
//!
//! [`TOOLS`] is the single pin table. Pinned tools live in `$CARGO_HOME/bin`, and xtask runs them
//! by that absolute path ([`tool_path`]), so an older copy earlier on `PATH` never stands in for
//! the pinned one. Cargo finds its subcommands (`cargo nextest`, `cargo deny`, `cargo about`) in
//! `$CARGO_HOME/bin` before `PATH` as well.
//!
//! The pins and tarball digests were approved by the operator on 2026-10-11 (plan 01-02, Task 2);
//! ast-grep and tree-sitter-cli were added the same day as the tree-sitter code oracle. Each
//! crate's approved `.crate` checksum is recorded beside its pin; `cargo install --locked`
//! verifies the download against the registry index.
//!
//! `tools check` also checks the other code oracle, rust-analyzer: a rustup component of the
//! pinned toolchain (rust-toolchain.toml), not a cargo install. It must report that toolchain's
//! version.

use std::env;
use std::fs::{self, File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::thread::sleep;
use std::time::Duration;

use clap::Subcommand;

use crate::Res;
use crate::store::{self, Cache, workspace_root};

/// `cargo xtask tools ...`
#[derive(Subcommand)]
pub enum ToolsCmd {
    /// Install every pinned tool that is missing or at another version.
    Install,
    /// Print `name pinned installed` per tool; exit non-zero on any mismatch.
    Check,
    /// Run one command under the heavy-job guard and pass its exit code through.
    Guard {
        /// The program and its arguments (after `--`).
        #[arg(required = true, trailing_var_arg = true, allow_hyphen_values = true)]
        cmd: Vec<String>,
    },
}

/// Runs one `tools` command.
pub fn run(cmd: ToolsCmd) -> Res<ExitCode> {
    match cmd {
        ToolsCmd::Install => install().map(|()| ExitCode::SUCCESS),
        ToolsCmd::Check => check(),
        ToolsCmd::Guard { cmd } => guard(&cmd),
    }
}

/// How a pinned tool is installed.
pub enum Install {
    /// `cargo install --locked <name>@<version>`, with these features enabled.
    Crate(&'static [&'static str]),
    /// A release tarball per platform, checked against its approved SHA-256.
    Tarball(&'static [Asset]),
}

/// One platform's release tarball.
pub struct Asset {
    /// `std::env::consts::OS`.
    pub os: &'static str,
    /// `std::env::consts::ARCH`.
    pub arch: &'static str,
    /// `https://` download URL.
    pub url: &'static str,
    /// Approved SHA-256 of the tarball.
    pub sha256: &'static str,
    /// Path of the binary inside the tarball.
    pub member: &'static str,
}

/// One pinned host tool.
pub struct Tool {
    /// Tool name (the crate name for a crate install).
    pub name: &'static str,
    /// Binary name in `$CARGO_HOME/bin`.
    pub bin: &'static str,
    /// Exact pinned version.
    pub version: &'static str,
    /// Install method.
    pub install: Install,
    /// Arguments that make the binary print its version.
    pub probe: &'static [&'static str],
}

/// The single pin table (operator-approved 2026-10-11).
pub const TOOLS: &[Tool] = &[
    Tool {
        name: "wasm-tools",
        bin: "wasm-tools",
        version: "1.261.0",
        // crate sha256 1e35666fc3fe1038cabc454c51fbeec5d231c42f87cecb67df1dcf75862370f1
        install: Install::Crate(&[]),
        probe: &["--version"],
    },
    Tool {
        name: "cargo-deny",
        bin: "cargo-deny",
        version: "0.20.2",
        // crate sha256 e528dfcbe739af7ce37a77d3d6df1b29dd6887b1c701d888820c0f16b864f737
        install: Install::Crate(&[]),
        probe: &["--version"],
    },
    Tool {
        name: "cargo-nextest",
        bin: "cargo-nextest",
        version: "0.9.148",
        // crate sha256 7023461ee82c0c2864df74ee340fb7777c13fc556f99e913646ab6ddccd05dd4
        install: Install::Crate(&[]),
        probe: &["nextest", "--version"],
    },
    Tool {
        name: "cargo-about",
        bin: "cargo-about",
        version: "0.9.2",
        // crate sha256 0cd19d99696eb83f0a2d6ab7a347b14968d2980416c8cca827ded220e6e9c4bb
        // 0.9.2 builds its binary only with the non-default `cli` feature.
        install: Install::Crate(&["cli"]),
        probe: &["--version"],
    },
    Tool {
        name: "oras",
        bin: "oras",
        version: "1.3.4",
        install: Install::Tarball(&[
            Asset {
                os: "macos",
                arch: "aarch64",
                url: "https://github.com/oras-project/oras/releases/download/v1.3.4/oras_1.3.4_darwin_arm64.tar.gz",
                sha256: "217761a9500242ff473de8656b5aca21136ff39e17e9e61fd8936bbfd902704c",
                member: "oras",
            },
            Asset {
                os: "linux",
                arch: "x86_64",
                url: "https://github.com/oras-project/oras/releases/download/v1.3.4/oras_1.3.4_linux_amd64.tar.gz",
                sha256: "f27adb935022d94df8dc77719c322dda592c78a0d57a6f7dcdd8d900b248c454",
                member: "oras",
            },
        ]),
        probe: &["version"],
    },
    Tool {
        name: "uv",
        bin: "uv",
        version: "0.13.0",
        install: Install::Tarball(&[
            Asset {
                os: "macos",
                arch: "aarch64",
                url: "https://github.com/astral-sh/uv/releases/download/0.13.0/uv-aarch64-apple-darwin.tar.gz",
                sha256: "a9c1b29002cf3c83f07fa9cd8a887a3be0107d90e23189721221e7257db8e3d6",
                member: "uv-aarch64-apple-darwin/uv",
            },
            Asset {
                os: "linux",
                arch: "x86_64",
                url: "https://github.com/astral-sh/uv/releases/download/0.13.0/uv-x86_64-unknown-linux-gnu.tar.gz",
                sha256: "1468ebd5a5541121837c5a2817b9972ba6090fa6caa3d142620850a47fb75154",
                member: "uv-x86_64-unknown-linux-gnu/uv",
            },
        ]),
        probe: &["--version"],
    },
    Tool {
        name: "ast-grep",
        bin: "ast-grep",
        version: "0.45.3",
        // crate sha256 060d189f7aa2050db49a18f8d7fdf1a59fdd0bdfe8eea67b79c2674b4e9df090
        install: Install::Crate(&[]),
        probe: &["--version"],
    },
    Tool {
        name: "tree-sitter-cli",
        bin: "tree-sitter",
        version: "0.27.0",
        // crate sha256 a65e581fcfdbc6495d05deceddedf4016dc103486e097c6bdaea6db8f5ad0075
        install: Install::Crate(&[]),
        probe: &["--version"],
    },
];

/// `$CARGO_HOME/bin` (default `$HOME/.cargo/bin`).
pub fn bin_dir() -> Res<PathBuf> {
    if let Some(home) = env::var_os("CARGO_HOME") {
        return Ok(PathBuf::from(home).join("bin"));
    }
    let home = env::var_os("HOME").ok_or("neither CARGO_HOME nor HOME is set")?;
    Ok(PathBuf::from(home).join(".cargo").join("bin"))
}

/// The absolute path of a pinned tool's binary. Every xtask call of a pinned tool goes here.
pub fn tool_path(name: &str) -> Res<PathBuf> {
    Ok(bin_dir()?.join(name))
}

/// The first `x.y.z` token (optionally `v`-prefixed) of a version probe's output.
pub fn parse_version(output: &str) -> Option<&str> {
    output
        .split_whitespace()
        .map(|t| t.strip_prefix('v').unwrap_or(t))
        .find(|t| {
            let parts: Vec<&str> = t.split('.').collect();
            parts.len() == 3
                && parts
                    .iter()
                    .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
        })
}

/// The installed version of `tool` in `$CARGO_HOME/bin`, or `None` when it is missing.
fn installed_version(tool: &Tool) -> Res<Option<String>> {
    let path = tool_path(tool.bin)?;
    if !path.is_file() {
        return Ok(None);
    }
    let out = Command::new(&path)
        .args(tool.probe)
        .output()
        .map_err(|e| format!("run {}: {e}", path.display()))?;
    let text = String::from_utf8_lossy(&out.stdout);
    Ok(parse_version(&text).map(str::to_owned))
}

/// Notes (does not fail on) a different binary of the same name earlier on `PATH`.
fn path_shadow(tool: &Tool) -> Res<Option<PathBuf>> {
    let pinned = tool_path(tool.bin)?;
    let Some(path) = env::var_os("PATH") else {
        return Ok(None);
    };
    for dir in env::split_paths(&path) {
        let candidate = dir.join(tool.bin);
        if candidate.is_file() {
            return Ok((candidate != pinned).then_some(candidate));
        }
    }
    Ok(None)
}

fn check() -> Res<ExitCode> {
    let mut ok = true;
    for tool in TOOLS {
        let installed = installed_version(tool)?;
        let shown = installed.as_deref().unwrap_or("missing");
        let good = installed.as_deref() == Some(tool.version);
        ok &= good;
        println!(
            "{:<14} {:<9} {:<9} {}",
            tool.name,
            tool.version,
            shown,
            if good { "ok" } else { "MISMATCH" }
        );
        if let Some(other) = path_shadow(tool)? {
            println!(
                "{:<14} note: PATH resolves {} first; xtask runs {}",
                "",
                other.display(),
                tool_path(tool.bin)?.display()
            );
        }
    }
    let channel = toolchain_channel()?;
    let analyzer = rust_analyzer_version()?;
    let good = analyzer.as_deref() == Some(channel.as_str());
    ok &= good;
    println!(
        "{:<14} {:<9} {:<9} {} (toolchain component)",
        "rust-analyzer",
        channel,
        analyzer.as_deref().unwrap_or("missing"),
        if good { "ok" } else { "MISMATCH" }
    );
    if ok {
        Ok(ExitCode::SUCCESS)
    } else {
        eprintln!("error: host tools differ from their pins; run: cargo xtask tools install");
        Ok(ExitCode::FAILURE)
    }
}

/// `toolchain.channel` from the workspace's rust-toolchain.toml.
fn toolchain_channel() -> Res<String> {
    let path = workspace_root().join("rust-toolchain.toml");
    let text = fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let doc: toml::Table = toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    doc.get("toolchain")
        .and_then(|t| t.get("channel"))
        .and_then(|c| c.as_str())
        .map(str::to_owned)
        .ok_or_else(|| format!("{}: no toolchain.channel", path.display()))
}

/// `rust-analyzer --version` run from the workspace root, so rustup resolves the pinned
/// toolchain's component. `None` when it does not run or prints no version.
fn rust_analyzer_version() -> Res<Option<String>> {
    let out = match Command::new("rust-analyzer")
        .arg("--version")
        .current_dir(workspace_root())
        .output()
    {
        Ok(out) if out.status.success() => out,
        _ => return Ok(None),
    };
    let text = String::from_utf8_lossy(&out.stdout);
    Ok(parse_version(&text).map(str::to_owned))
}

fn install() -> Res<()> {
    let cache = Cache::from_env()?;
    let mut guard = None;
    for tool in TOOLS {
        if installed_version(tool)?.as_deref() == Some(tool.version) {
            println!("{:<14} {} already installed", tool.name, tool.version);
            continue;
        }
        match &tool.install {
            Install::Crate(features) => {
                // Building from source is a heavy job (D-04).
                if guard.is_none() {
                    guard = Some(heavy_job()?);
                }
                let spec = format!("{}@{}", tool.name, tool.version);
                let mut args = vec!["install", "--locked", spec.as_str()];
                let joined = features.join(",");
                if !features.is_empty() {
                    args.extend(["--features", joined.as_str()]);
                }
                println!("{:<14} cargo {}", tool.name, args.join(" "));
                let status = Command::new("cargo")
                    .args(&args)
                    .status()
                    .map_err(|e| format!("run cargo install: {e}"))?;
                if !status.success() {
                    return Err(format!("cargo install {spec}: {status}"));
                }
            }
            Install::Tarball(assets) => install_tarball(&cache, tool, assets)?,
        }
        let now = installed_version(tool)?;
        if now.as_deref() != Some(tool.version) {
            return Err(format!(
                "{} reports {} after install, pinned {}",
                tool.name,
                now.as_deref().unwrap_or("nothing"),
                tool.version
            ));
        }
    }
    Ok(())
}

/// Fetches the platform tarball through the store (re-hashed against the approved digest),
/// extracts it into the cache's scratch space and renames the binary into `$CARGO_HOME/bin`.
fn install_tarball(cache: &Cache, tool: &Tool, assets: &[Asset]) -> Res<()> {
    let (os, arch) = (env::consts::OS, env::consts::ARCH);
    let asset = assets
        .iter()
        .find(|a| a.os == os && a.arch == arch)
        .ok_or_else(|| format!("{}: no pinned tarball for {os}/{arch}", tool.name))?;
    let (source, blob) = store::fetch_blob(cache, asset.sha256, Some(asset.url), None)?;
    println!(
        "{:<14} {source} sha256:{} {}",
        tool.name, asset.sha256, asset.url
    );

    let work = cache
        .partial_path(asset.sha256)?
        .with_extension(format!("x{}", std::process::id()));
    let _ = fs::remove_dir_all(&work);
    fs::create_dir_all(&work).map_err(|e| format!("create {}: {e}", work.display()))?;
    let result = (|| {
        let status = Command::new("tar")
            .arg("-xzf")
            .arg(&blob)
            .arg("-C")
            .arg(&work)
            .arg(asset.member)
            .status()
            .map_err(|e| format!("run tar: {e}"))?;
        if !status.success() {
            return Err(format!(
                "tar -xzf {} {}: {status}",
                blob.display(),
                asset.member
            ));
        }
        let bin = bin_dir()?;
        fs::create_dir_all(&bin).map_err(|e| format!("create {}: {e}", bin.display()))?;
        let dest = tool_path(tool.bin)?;
        let partial = bin.join(format!("{}.partial.{}", tool.bin, std::process::id()));
        fs::copy(work.join(asset.member), &partial)
            .map_err(|e| format!("copy {}: {e}", tool.name))?;
        set_executable(&partial)?;
        fs::rename(&partial, &dest).map_err(|e| format!("rename into {}: {e}", dest.display()))
    })();
    let _ = fs::remove_dir_all(&work);
    result
}

#[cfg(unix)]
fn set_executable(path: &Path) -> Res<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))
        .map_err(|e| format!("chmod {}: {e}", path.display()))
}

#[cfg(not(unix))]
fn set_executable(_path: &Path) -> Res<()> {
    Ok(())
}

/// Held for the duration of a heavy job; dropping it releases `$KSCI_CACHE/heavy.lock`.
pub struct HeavyGuard {
    _lock: File,
}

/// How often the guard re-checks for a klein-rs build.
const POLL: Duration = Duration::from_secs(30);

/// Takes the exclusive host lock, then waits while a klein-rs `cargo` or `rustc` runs (D-04).
pub fn heavy_job() -> Res<HeavyGuard> {
    let cache = Cache::from_env()?;
    fs::create_dir_all(cache.root())
        .map_err(|e| format!("create {}: {e}", cache.root().display()))?;
    let path = cache.root().join("heavy.lock");
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
        .map_err(|e| format!("open {}: {e}", path.display()))?;
    match file.try_lock() {
        Ok(()) => {}
        Err(TryLockError::WouldBlock) => {
            println!(
                "heavy-job: waiting for {} (another heavy job holds it)",
                path.display()
            );
            file.lock()
                .map_err(|e| format!("lock {}: {e}", path.display()))?;
        }
        Err(TryLockError::Error(e)) => return Err(format!("lock {}: {e}", path.display())),
    }
    loop {
        let builds = klein_rs_builds()?;
        if builds.is_empty() {
            return Ok(HeavyGuard { _lock: file });
        }
        println!(
            "heavy-job: waiting {}s, klein-rs build running: {}",
            POLL.as_secs(),
            builds[0]
        );
        sleep(POLL);
    }
}

/// Whether a klein-rs `cargo` or `rustc` process is running (the box manifest records this).
#[expect(dead_code, reason = "API for plan 01-11's fixed-box manifest")]
pub fn klein_rs_build_running() -> Res<bool> {
    Ok(!klein_rs_builds()?.is_empty())
}

fn klein_rs_builds() -> Res<Vec<String>> {
    let marker = env::var("KSCI_KLEIN_RS_MARKER").unwrap_or_else(|_| "klein-rs".to_owned());
    let out = Command::new("ps")
        .args(["-axo", "pid=,args="])
        .output()
        .map_err(|e| format!("run ps: {e}"))?;
    if !out.status.success() {
        return Err(format!("ps: {}", out.status));
    }
    let text = String::from_utf8_lossy(&out.stdout);
    Ok(matching_builds(&text, &marker, std::process::id())
        .into_iter()
        .map(str::to_owned)
        .collect())
}

/// Lines of `ps -axo pid=,args=` whose program is `cargo` or `rustc` and whose argv names `marker`.
pub fn matching_builds<'a>(ps: &'a str, marker: &str, own_pid: u32) -> Vec<&'a str> {
    ps.lines()
        .map(str::trim)
        .filter(|line| {
            let mut words = line.split_whitespace();
            let pid = words.next().and_then(|p| p.parse::<u32>().ok());
            let program = words.next().unwrap_or("");
            let base = program.rsplit('/').next().unwrap_or(program);
            pid != Some(own_pid) && matches!(base, "cargo" | "rustc") && line.contains(marker)
        })
        .collect()
}

fn guard(cmd: &[String]) -> Res<ExitCode> {
    let (program, args) = cmd.split_first().ok_or("tools guard needs a command")?;
    let held = heavy_job()?;
    let status = Command::new(program)
        .args(args)
        .status()
        .map_err(|e| format!("run {program}: {e}"))?;
    drop(held);
    let code = status.code().unwrap_or(1);
    Ok(ExitCode::from(u8::try_from(code).unwrap_or(1)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heavy_detects_klein_rs_build() {
        let ps = "\
  101 /Users/x/.rustup/toolchains/1.96.0/bin/cargo build --manifest-path /src/klein-rs/Cargo.toml
  102 rustc --crate-name klein_core /src/klein-rs/crates/core/src/lib.rs
  103 /usr/bin/vim /src/klein-rs/notes.md
  104 cargo build --manifest-path /src/klein-science/Cargo.toml
  105 cargo test klein-rs";
        assert_eq!(
            matching_builds(ps, "klein-rs", 105),
            vec![
                "101 /Users/x/.rustup/toolchains/1.96.0/bin/cargo build --manifest-path /src/klein-rs/Cargo.toml",
                "102 rustc --crate-name klein_core /src/klein-rs/crates/core/src/lib.rs",
            ]
        );
    }

    #[test]
    fn tools_parse_version() {
        assert_eq!(
            parse_version("wasm-tools 1.261.0 (abc 2026-10-02)"),
            Some("1.261.0")
        );
        assert_eq!(
            parse_version("Version:        1.3.4\nGo version:     go1.24.5"),
            Some("1.3.4")
        );
        assert_eq!(
            parse_version("cargo-nextest 0.9.148 (cd1d6d546 2026-10-08)\nrelease: 0.9.148"),
            Some("0.9.148")
        );
        assert_eq!(parse_version("cargo-deny v0.20.2"), Some("0.20.2"));
        assert_eq!(parse_version("no version here 1.2"), None);
    }

    #[test]
    fn tools_table_pins() {
        let pins: Vec<(&str, &str)> = TOOLS.iter().map(|t| (t.name, t.version)).collect();
        assert_eq!(
            pins,
            [
                ("wasm-tools", "1.261.0"),
                ("cargo-deny", "0.20.2"),
                ("cargo-nextest", "0.9.148"),
                ("cargo-about", "0.9.2"),
                ("oras", "1.3.4"),
                ("uv", "0.13.0"),
                ("ast-grep", "0.45.3"),
                ("tree-sitter-cli", "0.27.0"),
            ]
        );
        for tool in TOOLS {
            if let Install::Tarball(assets) = &tool.install {
                for a in *assets {
                    store::check_digest(a.sha256).unwrap();
                    assert!(a.url.starts_with("https://"));
                }
            }
        }
    }
}
