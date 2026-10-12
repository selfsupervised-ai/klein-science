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
//! pinned toolchain (rust-toolchain.toml), not a cargo install. [`toolchain_bin`] resolves it
//! through rustup (itself run as `$CARGO_HOME/bin/rustup`), checks the path lies in that
//! toolchain's directory, and xtask runs it by that absolute path. It must report that
//! toolchain's version.
//!
//! Every cargo that xtask starts is [`cargo`]: the pinned toolchain's cargo resolved through
//! rustup. A `$CARGO` that names any other file is refused, and the cargo must report the pinned
//! toolchain's version. No pinned binary is ever found by a `PATH` lookup.
//!
//! `git`, which the repository guards depend on, is a system tool in the same table
//! ([`Install::System`]): xtask does not install it, but runs it only from a fixed list of
//! absolute paths, and only once it reports at least the minimum version ([`git`]). `docker`,
//! which the offline oracle runs in, is pinned the same way ([`docker`]). `curl`,
//! `tar` and `ps` stay trusted system tools found on `PATH` (operator decision, 2026-10-11).

use std::env;
use std::fs::{self, File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::sync::OnceLock;
use std::thread::sleep;
use std::time::Duration;

use clap::Subcommand;

use crate::Res;
use crate::store::{self, Cache};
use crate::workspace;

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
    /// Create `.venv` (CPython [`VENV_PYTHON`], uv-managed) if it is absent, then sync it to the
    /// hash-locked dev test oracles ([`DEV_LOCK`]). The one way any worktree, CI job or plan gets
    /// the dev venv; idempotent.
    Venv,
}

/// The exact CPython of the dev venv. uv fetches it as a uv-managed interpreter, so a system
/// Python never stands in, and every worktree and CI job runs the same patch release.
pub const VENV_PYTHON: &str = "3.13.16";

/// The dev test oracles (rfc8785, pydantic-monty and their transitive packages), hash-locked and
/// approved by the operator on 2026-10-12 (plan 01-08, Task 2). Never shipped.
pub const DEV_LOCK: &str = "conformance/python/dev.lock";

/// The dev venv's directory, relative to the checkout root.
const VENV_DIR: &str = ".venv";

/// The dev venv's interpreter, `.venv/bin/python` under the checkout `root`: the one definition
/// of that path. `tools venv` creates and syncs it; `ci` and `oracle standin` run it.
pub fn venv_python_path(root: &Path) -> PathBuf {
    root.join(VENV_DIR).join("bin").join("python")
}

/// Runs one `tools` command.
pub fn run(cmd: ToolsCmd) -> Res<ExitCode> {
    match cmd {
        ToolsCmd::Install => install().map(|()| ExitCode::SUCCESS),
        ToolsCmd::Check => check(),
        ToolsCmd::Guard { cmd } => guard(&cmd),
        ToolsCmd::Venv => venv(),
    }
}

/// How a pinned tool is installed.
pub enum Install {
    /// `cargo install --locked <name>@<version>`, with these features enabled.
    Crate(&'static [&'static str]),
    /// A release tarball per platform, checked against its approved SHA-256.
    Tarball(&'static [Asset]),
    /// A system package that xtask does not install. xtask runs the first of these absolute
    /// paths that exists, and only if it reports at least the tool's `version`.
    System(&'static [&'static str]),
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
    /// Binary name in `$CARGO_HOME/bin` (for [`Install::System`], the name on `PATH`).
    pub bin: &'static str,
    /// Exact pinned version; for [`Install::System`], the minimum version.
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
    Tool {
        name: "git",
        bin: "git",
        // A floor, not a feature need: every git command xtask runs predates it. It admits both
        // gits on the Mac (Homebrew 2.55.0, Apple 2.50.1) and current Debian and Ubuntu LTS
        // packages, and rejects an ancient git or a stand-in that prints no version.
        version: "2.39.0",
        // The default system `PATH` order on macOS (Apple silicon and Intel Homebrew first) and
        // on Linux, frozen as absolute paths so no `PATH` entry can stand in for git.
        install: Install::System(&[
            "/opt/homebrew/bin/git",
            "/usr/local/bin/git",
            "/usr/bin/git",
        ]),
        probe: &["--version"],
    },
    Tool {
        name: "docker",
        bin: "docker",
        // The client the oracle runner's flags were verified against (`image inspect --platform`,
        // `run --pull=never --init`); an older client is unverified, so it is refused.
        version: "28.3.3",
        // Docker Desktop's symlink and app bundle on macOS, Homebrew, then the Linux package,
        // frozen as absolute paths so no `PATH` entry can stand in for docker.
        install: Install::System(&[
            "/usr/local/bin/docker",
            "/opt/homebrew/bin/docker",
            "/Applications/Docker.app/Contents/Resources/bin/docker",
            "/usr/bin/docker",
        ]),
        // `docker --version` prints `28.3.3,`; the bare client version needs a template.
        probe: &["version", "--format", "{{.Client.Version}}"],
    },
];

/// `$CARGO_HOME` (default `$HOME/.cargo`): the pinned tools and the registry sources.
pub fn cargo_home() -> Res<PathBuf> {
    if let Some(home) = env::var_os("CARGO_HOME") {
        return Ok(PathBuf::from(home));
    }
    let home = env::var_os("HOME").ok_or("neither CARGO_HOME nor HOME is set")?;
    Ok(PathBuf::from(home).join(".cargo"))
}

/// `$CARGO_HOME/bin` (default `$HOME/.cargo/bin`).
pub fn bin_dir() -> Res<PathBuf> {
    Ok(cargo_home()?.join("bin"))
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

/// Whether `version` is at least `min` (both `x.y.z`); anything unparsable is not.
pub fn version_at_least(version: &str, min: &str) -> bool {
    fn parts(v: &str) -> Option<[u64; 3]> {
        let mut it = v.split('.').map(|p| p.parse::<u64>().ok());
        let parts = [it.next()??, it.next()??, it.next()??];
        it.next().is_none().then_some(parts)
    }
    matches!((parts(version), parts(min)), (Some(v), Some(m)) if v >= m)
}

/// Whether an installed version meets `tool`'s pin: equal to it, or for a system tool at least it.
fn satisfies(tool: &Tool, installed: &str) -> bool {
    match tool.install {
        Install::System(_) => version_at_least(installed, tool.version),
        Install::Crate(_) | Install::Tarball(_) => installed == tool.version,
    }
}

/// The absolute path xtask runs `tool` from, when it exists: `$CARGO_HOME/bin/<bin>` for a tool
/// xtask installs, the first existing path of the list for a system tool. Never a `PATH` lookup.
fn locate(tool: &Tool) -> Res<Option<PathBuf>> {
    match tool.install {
        Install::System(paths) => Ok(paths.iter().map(PathBuf::from).find(|p| p.is_file())),
        Install::Crate(_) | Install::Tarball(_) => {
            let path = tool_path(tool.bin)?;
            Ok(path.is_file().then_some(path))
        }
    }
}

/// The version `tool`'s probe prints when run from `path`.
fn probe_version(tool: &Tool, path: &Path) -> Res<Option<String>> {
    let out = Command::new(path)
        .args(tool.probe)
        .output()
        .map_err(|e| format!("run {}: {e}", path.display()))?;
    let text = String::from_utf8_lossy(&out.stdout);
    Ok(parse_version(&text).map(str::to_owned))
}

/// The installed version of `tool` at its [`locate`]d path, or `None` when it is missing.
fn installed_version(tool: &Tool) -> Res<Option<String>> {
    match locate(tool)? {
        Some(path) => probe_version(tool, &path),
        None => Ok(None),
    }
}

/// The absolute path of the pinned git, resolved and version-checked once per process.
pub fn git_path() -> Res<PathBuf> {
    static GIT: OnceLock<Result<PathBuf, String>> = OnceLock::new();
    GIT.get_or_init(|| resolve_system("git")).clone()
}

/// A command for the pinned docker CLI, resolved and version-checked once per process like
/// [`git`]. Every docker that xtask runs starts here.
pub fn docker() -> Res<Command> {
    static DOCKER: OnceLock<Result<PathBuf, String>> = OnceLock::new();
    let path = DOCKER.get_or_init(|| resolve_system("docker")).clone()?;
    Ok(Command::new(path))
}

/// A command for the pinned git. Every git that xtask runs starts here.
pub fn git() -> Res<Command> {
    Ok(Command::new(git_path()?))
}

/// Runs the pinned `git -C <dir> <args>` and returns its stdout; a non-zero exit is an error.
pub fn git_output(dir: &Path, args: &[&str]) -> Res<Vec<u8>> {
    let out = git()?
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .map_err(|e| format!("run git {}: {e}", args.join(" ")))?;
    if !out.status.success() {
        return Err(format!(
            "git {}: {}: {}",
            args.join(" "),
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(out.stdout)
}

/// The first existing absolute path of the system tool `name` in [`TOOLS`], once it reports at
/// least the pinned minimum version. Never a `PATH` lookup.
fn resolve_system(name: &str) -> Res<PathBuf> {
    let tool = TOOLS
        .iter()
        .find(|t| t.name == name)
        .ok_or_else(|| format!("{name} is missing from the pin table"))?;
    let Install::System(paths) = tool.install else {
        return Err(format!("{name} must be a system tool in the pin table"));
    };
    let path = locate(tool)?.ok_or_else(|| {
        format!(
            "{name}: none of {} exists; install {name} >= {}",
            paths.join(", "),
            tool.version
        )
    })?;
    match probe_version(tool, &path)? {
        Some(v) if satisfies(tool, &v) => Ok(path),
        found => Err(format!(
            "{} reports version {}; xtask needs {name} >= {}",
            path.display(),
            found.as_deref().unwrap_or("none"),
            tool.version
        )),
    }
}

/// Notes (does not fail on) a different binary of the same name earlier on `PATH`.
fn path_shadow(tool: &Tool) -> Res<Option<PathBuf>> {
    let Some(pinned) = locate(tool)? else {
        return Ok(None);
    };
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
        let good = installed.as_deref().is_some_and(|v| satisfies(tool, v));
        ok &= good;
        let pinned = match tool.install {
            Install::System(_) => format!(">={}", tool.version),
            Install::Crate(_) | Install::Tarball(_) => tool.version.to_owned(),
        };
        let located = locate(tool)?;
        let runs = match (&tool.install, &located) {
            (Install::System(_), Some(path)) => format!(" ({})", path.display()),
            _ => String::new(),
        };
        println!(
            "{:<14} {:<9} {:<9} {}{runs}",
            tool.name,
            pinned,
            shown,
            if good { "ok" } else { "MISMATCH" }
        );
        if let Some(other) = path_shadow(tool)? {
            println!(
                "{:<14} note: PATH resolves {} first; xtask runs {}",
                "",
                other.display(),
                located.unwrap_or_default().display()
            );
        }
    }
    let channel = toolchain_channel()?;
    let analyzer = rust_analyzer_version();
    let good = analyzer.as_ref().ok().map(|(v, _)| v.as_str()) == Some(channel.as_str());
    ok &= good;
    let (shown, source) = match &analyzer {
        Ok((v, path)) => (v.as_str(), path.display().to_string()),
        Err(why) => ("missing", why.clone()),
    };
    println!(
        "{:<14} {:<9} {:<9} {} (toolchain component: {source})",
        "rust-analyzer",
        channel,
        shown,
        if good { "ok" } else { "MISMATCH" }
    );
    if ok {
        Ok(ExitCode::SUCCESS)
    } else {
        eprintln!("error: host tools differ from their pins; run: cargo xtask tools install");
        Ok(ExitCode::FAILURE)
    }
}

/// `toolchain.channel` from the rust-toolchain.toml of the checkout xtask runs in.
pub fn toolchain_channel() -> Res<String> {
    let path = workspace::root()?.join("rust-toolchain.toml");
    let text = fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let doc: toml::Table = toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    doc.get("toolchain")
        .and_then(|t| t.get("channel"))
        .and_then(|c| c.as_str())
        .map(str::to_owned)
        .ok_or_else(|| format!("{}: no toolchain.channel", path.display()))
}

/// `$RUSTUP_HOME` (default `$HOME/.rustup`).
fn rustup_home() -> Res<PathBuf> {
    if let Some(home) = env::var_os("RUSTUP_HOME") {
        return Ok(PathBuf::from(home));
    }
    let home = env::var_os("HOME").ok_or("neither RUSTUP_HOME nor HOME is set")?;
    Ok(PathBuf::from(home).join(".rustup"))
}

/// The absolute path of `bin` in the pinned toolchain (rust-toolchain.toml), resolved by
/// `$CARGO_HOME/bin/rustup which --toolchain <channel>` and checked to lie in
/// `$RUSTUP_HOME/toolchains/<channel>-<host>/`. Never a `PATH` lookup.
pub fn toolchain_bin(bin: &str) -> Res<PathBuf> {
    let channel = toolchain_channel()?;
    let rustup = tool_path("rustup")?;
    if !rustup.is_file() {
        return Err(format!("{} not found", rustup.display()));
    }
    let out = Command::new(&rustup)
        .args(["which", bin, "--toolchain", &channel])
        .output()
        .map_err(|e| format!("run {}: {e}", rustup.display()))?;
    if !out.status.success() {
        return Err(format!(
            "rustup which {bin} --toolchain {channel}: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    let named = PathBuf::from(String::from_utf8_lossy(&out.stdout).trim());
    let real = fs::canonicalize(&named).map_err(|e| format!("{}: {e}", named.display()))?;
    let toolchains = rustup_home()?.join("toolchains");
    let toolchains =
        fs::canonicalize(&toolchains).map_err(|e| format!("{}: {e}", toolchains.display()))?;
    if !in_toolchain(&real, &toolchains, &channel) {
        return Err(format!(
            "{} is not inside {}/{channel}-<host>",
            real.display(),
            toolchains.display()
        ));
    }
    Ok(real)
}

/// Whether `path` lies in `<toolchains>/<channel>-<host>/`.
fn in_toolchain(path: &Path, toolchains: &Path, channel: &str) -> bool {
    path.strip_prefix(toolchains)
        .ok()
        .and_then(|rel| rel.components().next())
        .and_then(|c| c.as_os_str().to_str())
        .is_some_and(|dir| dir.starts_with(&format!("{channel}-")))
}

/// The version and absolute path of the pinned toolchain's rust-analyzer, or why it is missing.
fn rust_analyzer_version() -> Result<(String, PathBuf), String> {
    let path = toolchain_bin("rust-analyzer")?;
    let out = Command::new(&path)
        .arg("--version")
        .output()
        .map_err(|e| format!("run {}: {e}", path.display()))?;
    if !out.status.success() {
        return Err(format!("{} --version: {}", path.display(), out.status));
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let version =
        parse_version(&text).ok_or_else(|| format!("{} printed no version", path.display()))?;
    Ok((version.to_owned(), path))
}

/// The resolved cargo: an absolute path that reported the pinned toolchain's version.
pub struct Cargo {
    /// Absolute path of the cargo binary.
    pub path: PathBuf,
    channel: String,
}

impl Cargo {
    /// A command for this cargo. `RUSTUP_TOOLCHAIN` pins every rustup proxy it reaches (such as
    /// `rustc` run in a dependency's directory, where rust-toolchain.toml is out of sight) to the
    /// pinned toolchain, as the rustup proxy itself does when it starts cargo.
    pub fn command(&self) -> Command {
        let mut cmd = Command::new(&self.path);
        cmd.env("RUSTUP_TOOLCHAIN", &self.channel);
        cmd
    }
}

/// The cargo every xtask child runs: the pinned toolchain's cargo, resolved through rustup
/// ([`toolchain_bin`]). `$CARGO` (set by cargo for `cargo run` and aliases such as `cargo xtask`)
/// is checked when set: it must be an absolute path that canonicalises to that same file
/// ([`check_cargo_env`]). So neither a `cargo` earlier on `PATH`, nor a stale `$CARGO` of another
/// toolchain, nor a stand-in that prints the pinned version, ever runs. The cargo must also
/// report the pinned toolchain's version.
pub fn cargo() -> Res<Cargo> {
    let channel = toolchain_channel()?;
    let pinned = toolchain_bin("cargo")?;
    let path = match env::var_os("CARGO") {
        Some(c) => check_cargo_env(Path::new(&c), &pinned)?,
        None => pinned,
    };
    let out = Command::new(&path)
        .arg("--version")
        .output()
        .map_err(|e| format!("run {}: {e}", path.display()))?;
    let text = String::from_utf8_lossy(&out.stdout);
    let version = text.strip_prefix("cargo ").and_then(parse_version);
    if !out.status.success() || version != Some(channel.as_str()) {
        return Err(format!(
            "{} reports {:?}; the pinned toolchain is {channel}",
            path.display(),
            text.trim()
        ));
    }
    Ok(Cargo { path, channel })
}

/// The pinned cargo, once `$CARGO` (given as `cargo`) is an absolute path that canonicalises to
/// `pinned`, the canonical path of the pinned toolchain's cargo. The error names both files.
fn check_cargo_env(cargo: &Path, pinned: &Path) -> Res<PathBuf> {
    if !cargo.is_absolute() {
        return Err(format!(
            "CARGO={} is not an absolute path; xtask never looks cargo up on PATH",
            cargo.display()
        ));
    }
    let real = fs::canonicalize(cargo).map_err(|e| format!("CARGO={}: {e}", cargo.display()))?;
    if real != pinned {
        return Err(format!(
            "CARGO={} is {}, not the pinned toolchain's cargo {}; run xtask through \
             `cargo xtask` with the pinned toolchain, or unset CARGO",
            cargo.display(),
            real.display(),
            pinned.display()
        ));
    }
    Ok(real)
}

fn install() -> Res<()> {
    let cache = Cache::from_env()?;
    let mut guard = None;
    for tool in TOOLS {
        if let Some(v) = installed_version(tool)?.filter(|v| satisfies(tool, v)) {
            println!("{:<14} {v} already installed", tool.name);
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
                let cargo = cargo()?;
                println!(
                    "{:<14} {} {}",
                    tool.name,
                    cargo.path.display(),
                    args.join(" ")
                );
                let status = cargo
                    .command()
                    .args(&args)
                    .status()
                    .map_err(|e| format!("run cargo install: {e}"))?;
                if !status.success() {
                    return Err(format!("cargo install {spec}: {status}"));
                }
            }
            Install::Tarball(assets) => install_tarball(&cache, tool, assets)?,
            Install::System(paths) => {
                return Err(format!(
                    "{}: needs {} >= {} at one of {}; xtask does not install system tools, \
                     install it with the system package manager",
                    tool.name,
                    tool.bin,
                    tool.version,
                    paths.join(", ")
                ));
            }
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

/// Held for the duration of a heavy job; dropping it releases `$KSCI_CACHE/heavy.lock`. A guard
/// taken under a parent that holds the lock ([`HEAVY_JOB_HELD`]) holds nothing itself.
pub struct HeavyGuard {
    _lock: Option<File>,
}

/// Set by a heavy-job holder (`cargo xtask ci`) on the steps it runs. A child that finds the lock
/// taken and this variable set runs under its parent's hold instead of waiting for it: the
/// parent would otherwise wait for the child, and the child for the parent, forever. A child
/// that finds the lock free takes it as usual.
pub const HEAVY_JOB_HELD: &str = "KSCI_HEAVY_JOB_HELD";

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
        Err(TryLockError::WouldBlock) if env::var_os(HEAVY_JOB_HELD).is_some() => {
            println!(
                "heavy-job: {} is held by the parent job ({HEAVY_JOB_HELD})",
                path.display()
            );
            return Ok(HeavyGuard { _lock: None });
        }
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
            return Ok(HeavyGuard { _lock: Some(file) });
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

/// `tools venv`: `.venv` at the checkout root, on the pinned uv only.
///
/// - `uv venv --managed-python --python <VENV_PYTHON> .venv` when `.venv` is absent;
/// - the venv's own Python must report exactly [`VENV_PYTHON`]; a `.venv` on another interpreter
///   fails with the command that replaces it;
/// - always `uv pip sync --require-hashes --no-build` against [`DEV_LOCK`], so every installed
///   file matches an approved sha256 and no package code runs at install time.
fn venv() -> Res<ExitCode> {
    let root = workspace::root()?;
    let uv = tool_path("uv")?;
    if !uv.is_file() {
        return Err(format!(
            "{} is missing; run: cargo xtask tools install",
            uv.display()
        ));
    }
    let run_uv = |args: &[&str]| -> Res<()> {
        let status = Command::new(&uv)
            .args(args)
            .current_dir(&root)
            .status()
            .map_err(|e| format!("run {}: {e}", uv.display()))?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("uv {} failed: {status}", args.join(" ")))
        }
    };
    if fs::symlink_metadata(root.join(VENV_DIR)).is_err() {
        run_uv(&[
            "venv",
            "--managed-python",
            "--python",
            VENV_PYTHON,
            VENV_DIR,
        ])?;
    }
    let python = venv_python_path(&root);
    let out = Command::new(&python)
        .arg("--version")
        .output()
        .map_err(|e| format!("run {}: {e}", python.display()))?;
    let reported = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    if !out.status.success() || reported != format!("Python {VENV_PYTHON}") {
        return Err(format!(
            "{} reports {reported:?}, not Python {VENV_PYTHON}; remove .venv and run: cargo xtask tools venv",
            python.display()
        ));
    }
    // uv runs from the checkout root, so it gets the same interpreter path relative to it.
    let relative = venv_python_path(Path::new(""));
    let relative = relative
        .to_str()
        .ok_or("the dev venv interpreter path is not UTF-8")?;
    run_uv(&[
        "pip",
        "sync",
        "--require-hashes",
        "--no-build",
        "--python",
        relative,
        DEV_LOCK,
    ])?;
    println!("{reported} at {}, synced to {DEV_LOCK}", python.display());
    Ok(ExitCode::SUCCESS)
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
    fn tools_toolchain_bin_is_in_pinned_toolchain() {
        let channel = toolchain_channel().unwrap();
        let cargo = toolchain_bin("cargo").unwrap();
        assert!(cargo.is_absolute(), "{}", cargo.display());
        let dir = cargo
            .parent()
            .and_then(Path::parent)
            .and_then(Path::file_name)
            .and_then(|n| n.to_str())
            .unwrap_or("");
        assert!(
            dir.starts_with(&format!("{channel}-")),
            "{}",
            cargo.display()
        );
        assert!(toolchain_bin("no-such-component-binary").is_err());
    }

    #[test]
    fn tools_in_toolchain() {
        let tc = Path::new("/h/.rustup/toolchains");
        let ok = Path::new("/h/.rustup/toolchains/1.99.0-aarch64-apple-darwin/bin/rust-analyzer");
        assert!(in_toolchain(ok, tc, "1.99.0"));
        for bad in [
            "/h/.rustup/toolchains/1.97.0-aarch64-apple-darwin/bin/rust-analyzer",
            "/h/.rustup/toolchains/1.99.0/bin/rust-analyzer",
            "/h/.rustup/toolchains/1.99.01-x/bin/rust-analyzer",
            "/tmp/fakes/rust-analyzer",
            "/h/.cargo/bin/rust-analyzer",
        ] {
            assert!(!in_toolchain(Path::new(bad), tc, "1.99.0"), "{bad}");
        }
    }

    #[test]
    fn tools_cargo_pins_toolchain_for_children() {
        let channel = toolchain_channel().unwrap();
        let cargo = cargo().unwrap();
        assert!(cargo.path.is_absolute(), "{}", cargo.path.display());
        let cmd = cargo.command();
        assert_eq!(cmd.get_program(), cargo.path.as_os_str());
        let envs: Vec<_> = cmd.get_envs().collect();
        assert_eq!(
            envs,
            [(
                std::ffi::OsStr::new("RUSTUP_TOOLCHAIN"),
                Some(std::ffi::OsStr::new(channel.as_str()))
            )]
        );
    }

    #[test]
    fn tools_cargo_env_must_be_the_pinned_file() {
        let pinned = toolchain_bin("cargo").unwrap();
        assert_eq!(check_cargo_env(&pinned, &pinned).unwrap(), pinned);
        let dir = env::temp_dir().join(format!("ksci-xtask-cargo-env-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        // A link to the pinned cargo names the same file.
        #[cfg(unix)]
        {
            let link = dir.join("cargo-link");
            std::os::unix::fs::symlink(&pinned, &link).unwrap();
            assert_eq!(check_cargo_env(&link, &pinned).unwrap(), pinned);
        }
        // Another file is refused whatever it prints, and the error names both files.
        let stub = dir.join("cargo");
        fs::write(&stub, "#!/bin/sh\necho 'cargo 1.99.0'\n").unwrap();
        let err = check_cargo_env(&stub, &pinned).unwrap_err();
        let stub_real = fs::canonicalize(&stub).unwrap();
        assert!(err.contains(&stub_real.display().to_string()), "{err}");
        assert!(err.contains(&pinned.display().to_string()), "{err}");
        assert!(check_cargo_env(&dir.join("missing"), &pinned).is_err());
        let err = check_cargo_env(Path::new("cargo"), &pinned).unwrap_err();
        assert!(err.contains("not an absolute path"), "{err}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn tools_version_at_least() {
        assert!(version_at_least("2.39.0", "2.39.0"));
        assert!(version_at_least("2.55.0", "2.39.0"));
        assert!(version_at_least("10.0.0", "2.39.0"));
        assert!(version_at_least("2.39.10", "2.39.9"));
        for low in [
            "2.38.9", "2.9.99", "1.99.99", "2.39", "2.39.0.1", "", "x.y.z",
        ] {
            assert!(!version_at_least(low, "2.39.0"), "{low}");
        }
    }

    #[test]
    fn tools_git_is_pinned_by_absolute_path() {
        let git_tool = TOOLS.iter().find(|t| t.name == "git").unwrap();
        let Install::System(paths) = git_tool.install else {
            panic!("git must be a system tool");
        };
        assert!(
            paths.iter().all(|p| Path::new(p).is_absolute()),
            "{paths:?}"
        );
        let path = git_path().unwrap();
        assert!(path.is_absolute(), "{}", path.display());
        assert!(
            paths.contains(&path.to_str().unwrap()),
            "{}",
            path.display()
        );
        assert_eq!(git().unwrap().get_program(), path.as_os_str());
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
                ("git", "2.39.0"),
                ("docker", "28.3.3"),
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

    #[test]
    fn venv_python_path_is_dot_venv_bin_python() {
        assert_eq!(
            venv_python_path(Path::new("/checkout")),
            Path::new("/checkout/.venv/bin/python")
        );
        // `tools venv` hands uv this relative form; it must stay the path uv always got.
        assert_eq!(
            venv_python_path(Path::new("")).to_str(),
            Some(".venv/bin/python")
        );
    }
}
