// provenance: original
//! `cargo xtask release dist [--twice]`: the artifacts that ship, built under the heavy-job guard
//! (D-04) by one build function, [`build_artifacts`] (D-01).
//!
//! - R0: `ksci-component` built as std Rust for `wasm32-unknown-unknown` with `+simd128` only (never
//!   relaxed SIMD), then wrapped by the pinned `wasm-tools component new`. The component imports
//!   nothing (D-16).
//! - R2: `ksci-worker` built for the host triple.
//! - `digests.json`: the JCS of each artifact's SHA-256, the host triple, the toolchain and
//!   wasm-tools versions, and whether two builds were compared (`reproducible`).
//!
//! Every artifact lands under `target/dist/` of the checkout ([`workspace::root`]):
//! `science_cas.component.wasm`, `<host-triple>/ksci-worker` and `digests.json`. The conformance
//! host (`ksci-conform`) runs exactly these files.
//!
//! # Reproducibility (PLUG-01, research Pitfall I)
//!
//! Each cargo build is given its rustflags explicitly as `CARGO_ENCODED_RUSTFLAGS`, so neither the
//! caller's `RUSTFLAGS` nor its `CARGO_ENCODED_RUSTFLAGS` reaches a shipped artifact. The flags
//! remap the source tree to `/src` and `$CARGO_HOME` to `/cargo` (`--remap-path-prefix`), and the
//! target directory always lies inside the source tree, so no builder path is embedded.
//!
//! `dist --twice` exports the committed tree (`git ls-files`) into two fresh directories at
//! different absolute paths, builds each into its own target directory, and compares the SHA-256
//! of every artifact. Only when every pair matches, and no artifact holds a builder path ($HOME,
//! either source tree, `$CARGO_HOME`), are the artifacts copied to `target/dist/` with
//! `"reproducible":true`. A plain `dist` builds the checkout itself the same way and records
//! `"reproducible":false`: nothing was compared.

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use clap::Subcommand;
use serde_json::{Value, json};

use crate::Res;
use crate::depinfo::{R0_RUSTFLAGS, R0_TARGET};
use crate::host::{self, Cargo, heavy_job};
use crate::workspace;

/// `cargo xtask release ...`
#[derive(Subcommand)]
pub enum ReleaseCmd {
    /// Build the R0 component and the R2 worker into target/dist and write digests.json.
    Dist {
        /// Build the committed tree twice, from two different absolute paths, and ship the
        /// artifacts only if every digest matches.
        #[arg(long)]
        twice: bool,
    },
}

/// Runs one `release` command.
pub fn run(cmd: ReleaseCmd) -> Res<ExitCode> {
    match cmd {
        ReleaseCmd::Dist { twice: false } => dist().map(|()| ExitCode::SUCCESS),
        ReleaseCmd::Dist { twice: true } => dist_twice(),
    }
}

/// The shipped component's file name under `target/dist/`.
const COMPONENT: &str = "science_cas.component.wasm";

/// The worker's binary name.
const WORKER: &str = "ksci-worker";

/// The remapped name of the source tree in every shipped artifact.
const SRC_REMAP: &str = "/src";

/// The remapped name of `$CARGO_HOME` (registry sources) in every shipped artifact.
const CARGO_HOME_REMAP: &str = "/cargo";

/// The separator of `CARGO_ENCODED_RUSTFLAGS`.
const FLAG_SEP: char = '\u{1f}';

/// The pinned tools every build uses, resolved and version-checked once.
struct Toolchain {
    cargo: Cargo,
    wasm_tools: PathBuf,
    rustc_vv: String,
    wasm_tools_version: String,
    triple: String,
    cargo_home: PathBuf,
}

impl Toolchain {
    fn resolve() -> Res<Toolchain> {
        let cargo = host::cargo()?;
        let rustc = host::toolchain_bin("rustc")?;
        let wasm_tools = pinned_tool("wasm-tools")?;
        let rustc_vv = output_text(Command::new(&rustc).arg("-vV"), "rustc -vV")?;
        let triple = host_triple(&rustc_vv)?;
        let wasm_tools_version = output_text(
            Command::new(&wasm_tools).arg("--version"),
            "wasm-tools --version",
        )?;
        let cargo_home = host::cargo_home()?;
        Ok(Toolchain {
            cargo,
            wasm_tools,
            rustc_vv: rustc_vv.trim_end().to_owned(),
            wasm_tools_version: wasm_tools_version.trim_end().to_owned(),
            triple,
            cargo_home,
        })
    }
}

/// What one [`build_artifacts`] run produced.
struct Built {
    /// Every shipped artifact, by its path relative to the output directory (`/` separators).
    artifacts: BTreeMap<String, PathBuf>,
}

/// The target directory of every release build, relative to the tree it builds. It lies inside
/// the tree, so the source remap covers it, and it is the same relative path in every build: the
/// native link records object paths relative to the tree (`-oso_prefix`), so a different relative
/// target directory alone gives a different worker. It is apart from `target/`, so the release
/// rustflags never invalidate the developer's own builds.
const BUILD_DIR: &str = "target/dist-build";

/// The one build path of the shipped artifacts (D-01): builds the tree at `src_root` into
/// `target_dir`, which must be `src_root/`[`BUILD_DIR`], and writes the component and the host
/// worker into `out_dir`.
fn build_artifacts(
    tc: &Toolchain,
    src_root: &Path,
    target_dir: &Path,
    out_dir: &Path,
) -> Res<Built> {
    if target_dir != src_root.join(BUILD_DIR) {
        return Err(format!(
            "target dir {} is not {BUILD_DIR} of the source tree {}; its paths would not \
             reproduce",
            target_dir.display(),
            src_root.display()
        ));
    }
    let remap = remap_flags(src_root, &tc.cargo_home)?;
    if out_dir.exists() {
        fs::remove_dir_all(out_dir).map_err(|e| format!("remove {}: {e}", out_dir.display()))?;
    }
    fs::create_dir_all(out_dir).map_err(|e| format!("create {}: {e}", out_dir.display()))?;
    let mut artifacts = BTreeMap::new();

    // a. The R0 core module.
    let mut r0_flags = remap.clone();
    r0_flags.push(R0_RUSTFLAGS.to_owned());
    let mut build = cargo_build(tc, src_root, target_dir, &r0_flags)?;
    build.args(["-p", "ksci-component", "--target", R0_TARGET]);
    run_checked(&mut build, "cargo build -p ksci-component")?;
    let core = target_dir
        .join(R0_TARGET)
        .join("release")
        .join("ksci_component.wasm");

    // b. The component: the core module wrapped by the pinned wasm-tools.
    let component = out_dir.join(COMPONENT);
    let mut wrap = Command::new(&tc.wasm_tools);
    wrap.arg("component")
        .arg("new")
        .arg(&core)
        .arg("-o")
        .arg(&component);
    run_checked(&mut wrap, "wasm-tools component new")?;
    artifacts.insert(COMPONENT.to_owned(), component);

    // c. The R2 worker for the host triple.
    let mut worker_flags = remap;
    worker_flags.extend(native_link_flags(&tc.triple, src_root)?);
    let mut build = cargo_build(tc, src_root, target_dir, &worker_flags)?;
    build.args(["-p", WORKER]);
    run_checked(&mut build, "cargo build -p ksci-worker")?;
    let exe = format!("{WORKER}{}", std::env::consts::EXE_SUFFIX);
    let worker_dir = out_dir.join(&tc.triple);
    fs::create_dir_all(&worker_dir).map_err(|e| format!("create {}: {e}", worker_dir.display()))?;
    let worker = worker_dir.join(&exe);
    let built = target_dir.join("release").join(&exe);
    fs::copy(&built, &worker)
        .map_err(|e| format!("copy {} to {}: {e}", built.display(), worker.display()))?;
    artifacts.insert(format!("{}/{exe}", tc.triple), worker);

    Ok(Built { artifacts })
}

/// The link flags a native worker for `triple` needs to be reproducible across source paths.
///
/// Apple's ld derives `LC_UUID` from the whole link, including the debug-map (`N_OSO`) entries
/// that name each object file by its absolute path in the target directory. The LTO object
/// carries std's line tables, so the entry exists; stripping later removes it but not the UUID,
/// and the ad-hoc code signature hashes the page holding the UUID. Two trees at different paths
/// therefore gave workers that differed only in those bytes (plan 01-09, assumption A6).
/// `-oso_prefix <src_root>/` makes the entries relative to the tree, so the UUID is the same.
fn native_link_flags(triple: &str, src_root: &Path) -> Res<Vec<String>> {
    if !triple.contains("-apple-") {
        return Ok(Vec::new());
    }
    let text = src_root
        .to_str()
        .ok_or_else(|| format!("{} is not UTF-8", src_root.display()))?;
    if text.contains(',') {
        return Err(format!("{text} contains ','; it cannot pass through -Wl"));
    }
    Ok(vec![format!("-Clink-arg=-Wl,-oso_prefix,{text}/")])
}

/// A `cargo build --locked --release` of the tree at `src_root` into `target_dir`, with exactly
/// `rustflags` as `CARGO_ENCODED_RUSTFLAGS`.
fn cargo_build(
    tc: &Toolchain,
    src_root: &Path,
    target_dir: &Path,
    rustflags: &[String],
) -> Res<Command> {
    if let Some(flag) = rustflags.iter().find(|f| f.contains(FLAG_SEP)) {
        return Err(format!("rustflag {flag:?} contains the 0x1f separator"));
    }
    let encoded = rustflags.join(&FLAG_SEP.to_string());
    let mut cmd = tc.cargo.command();
    cmd.current_dir(src_root)
        .env("CARGO_ENCODED_RUSTFLAGS", encoded)
        .env("CARGO_TARGET_DIR", target_dir)
        .env_remove("RUSTFLAGS")
        .args(["build", "--locked", "--release"]);
    Ok(cmd)
}

/// The `--remap-path-prefix` flags for a build of `src_root`: the source tree to [`SRC_REMAP`] and
/// `cargo_home` to [`CARGO_HOME_REMAP`], each under its given and its canonical spelling.
fn remap_flags(src_root: &Path, cargo_home: &Path) -> Res<Vec<String>> {
    let mut flags = Vec::new();
    for (from, to) in [(cargo_home, CARGO_HOME_REMAP), (src_root, SRC_REMAP)] {
        if !from.is_absolute() {
            return Err(format!("{} is not an absolute path", from.display()));
        }
        let mut spellings = vec![from.to_path_buf()];
        if let Ok(real) = fs::canonicalize(from)
            && real != from
        {
            spellings.push(real);
        }
        for path in spellings {
            let text = path
                .to_str()
                .ok_or_else(|| format!("{} is not UTF-8", path.display()))?;
            if text.contains('=') {
                return Err(format!("{text} contains '='; it cannot be remapped"));
            }
            flags.push(format!("--remap-path-prefix={text}={to}"));
        }
    }
    Ok(flags)
}

/// `dist`: one build of the checkout itself, into [`BUILD_DIR`].
fn dist() -> Res<()> {
    let root = workspace::root()?;
    let tc = Toolchain::resolve()?;
    let _held = heavy_job()?;
    let dist = root.join("target").join("dist");
    let built = build_artifacts(&tc, &root, &root.join(BUILD_DIR), &dist)?;
    write_digests(&tc, &dist, &built, false)?;
    Ok(())
}

/// `dist --twice`: two builds of the committed tree from two different absolute paths; the
/// artifacts ship only when every digest matches and no artifact holds a builder path.
fn dist_twice() -> Res<ExitCode> {
    let root = workspace::root()?;
    let tc = Toolchain::resolve()?;
    let _held = heavy_job()?;
    let changed = host::git_output(&root, &["status", "--porcelain", "--untracked-files=no"])?;
    if !changed.is_empty() {
        return Err(format!(
            "dist --twice builds exactly what is committed, and tracked files have uncommitted \
             changes:\n{}",
            String::from_utf8_lossy(&changed).trim_end()
        ));
    }
    let tmp = env::temp_dir();
    let copies = [
        (tmp.join("ksci-repro-a"), PathBuf::from("src")),
        (tmp.join("ksci-repro-b"), Path::new("nested").join("src")),
    ];
    let mut runs = Vec::new();
    for (base, src) in &copies {
        if base.exists() {
            fs::remove_dir_all(base).map_err(|e| format!("remove {}: {e}", base.display()))?;
        }
        let src_root = base.join(src);
        export_tree(&root, &src_root)?;
        let src_root =
            fs::canonicalize(&src_root).map_err(|e| format!("{}: {e}", src_root.display()))?;
        println!("dist --twice: building {}", src_root.display());
        let out = base.join("dist");
        let built = build_artifacts(&tc, &src_root, &src_root.join(BUILD_DIR), &out)?;
        runs.push((src_root, built));
    }

    // Compare every artifact's digest across the two builds.
    let (a, b) = (&runs[0], &runs[1]);
    let names: Vec<&String> = a.1.artifacts.keys().collect();
    if names != b.1.artifacts.keys().collect::<Vec<_>>() {
        return Err("the two builds produced different artifact sets".to_owned());
    }
    let mut same = true;
    for name in &names {
        let da = file_sha256(&a.1.artifacts[*name])?;
        let db = file_sha256(&b.1.artifacts[*name])?;
        println!("dist --twice: {name}\n  a sha256:{da}\n  b sha256:{db}");
        if da != db {
            eprintln!("dist --twice: DIFFERENT digests for {name}");
            same = false;
        }
    }
    if !same {
        eprintln!(
            "dist --twice: the builds differ; both trees are kept under {}",
            tmp.display()
        );
        return Ok(ExitCode::FAILURE);
    }

    // No builder path in any artifact.
    let mut needles: Vec<String> = Vec::new();
    if let Some(home) = env::var_os("HOME").and_then(|h| h.into_string().ok())
        && home.len() > 1
    {
        needles.push(home);
    }
    needles.extend(
        runs.iter()
            .filter_map(|(src, _)| src.to_str().map(str::to_owned)),
    );
    needles.extend(tc.cargo_home.to_str().map(str::to_owned));
    for path in a.1.artifacts.values() {
        let bytes = fs::read(path).map_err(|e| format!("read {}: {e}", path.display()))?;
        for needle in &needles {
            if contains(&bytes, needle.as_bytes()) {
                eprintln!(
                    "dist --twice: {} contains the builder path {needle}",
                    path.display()
                );
                return Ok(ExitCode::FAILURE);
            }
        }
    }

    // Ship the compared bytes.
    let dist = root.join("target").join("dist");
    if dist.exists() {
        fs::remove_dir_all(&dist).map_err(|e| format!("remove {}: {e}", dist.display()))?;
    }
    let mut shipped = BTreeMap::new();
    for (name, path) in &a.1.artifacts {
        let to = dist.join(name);
        if let Some(parent) = to.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("create {}: {e}", parent.display()))?;
        }
        fs::copy(path, &to)
            .map_err(|e| format!("copy {} to {}: {e}", path.display(), to.display()))?;
        shipped.insert(name.clone(), to);
    }
    write_digests(&tc, &dist, &Built { artifacts: shipped }, true)?;
    for (base, _) in &copies {
        fs::remove_dir_all(base).map_err(|e| format!("remove {}: {e}", base.display()))?;
    }
    println!("dist --twice: reproducible, {} artifacts", names.len());
    Ok(ExitCode::SUCCESS)
}

/// Copies every tracked file (`git ls-files -z`) of `root` to the same path under `to`, which
/// must not exist yet. Symbolic links are recreated as links.
fn export_tree(root: &Path, to: &Path) -> Res<()> {
    if to.exists() {
        return Err(format!("{} already exists", to.display()));
    }
    fs::create_dir_all(to).map_err(|e| format!("create {}: {e}", to.display()))?;
    let listed = host::git_output(root, &["ls-files", "-z"])?;
    for rel in listed.split(|&b| b == 0).filter(|p| !p.is_empty()) {
        let rel = std::str::from_utf8(rel).map_err(|e| format!("git ls-files: {e}"))?;
        let from = root.join(rel);
        let dest = to.join(rel);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("create {}: {e}", parent.display()))?;
        }
        let meta = fs::symlink_metadata(&from).map_err(|e| format!("{}: {e}", from.display()))?;
        if meta.file_type().is_symlink() {
            let link = fs::read_link(&from).map_err(|e| format!("{}: {e}", from.display()))?;
            symlink(&link, &dest)?;
        } else if meta.is_file() {
            fs::copy(&from, &dest)
                .map_err(|e| format!("copy {} to {}: {e}", from.display(), dest.display()))?;
        } else {
            return Err(format!("{rel}: tracked entry is neither a file nor a link"));
        }
    }
    Ok(())
}

#[cfg(unix)]
fn symlink(link: &Path, dest: &Path) -> Res<()> {
    std::os::unix::fs::symlink(link, dest).map_err(|e| format!("link {}: {e}", dest.display()))
}

#[cfg(not(unix))]
fn symlink(_link: &Path, dest: &Path) -> Res<()> {
    Err(format!(
        "{}: symbolic links are not exported on this host",
        dest.display()
    ))
}

/// Writes `digests.json` (JCS) into `dist` for `built`, whose artifacts lie under `dist`.
fn write_digests(tc: &Toolchain, dist: &Path, built: &Built, reproducible: bool) -> Res<()> {
    let mut artifacts = BTreeMap::new();
    for (name, path) in &built.artifacts {
        let digest = file_sha256(path)?;
        println!("sha256:{digest} target/dist/{name}");
        artifacts.insert(name.clone(), Value::String(digest));
    }
    let record = json!({
        "artifacts": artifacts,
        "host": tc.triple,
        "reproducible": reproducible,
        "rustc -vV": tc.rustc_vv,
        "wasm-tools --version": tc.wasm_tools_version,
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

/// The zero-import gate (D-16). Not implemented yet (RED).
fn gate_imports(_wasm_tools: &Path, _artifact: &Path) -> Res<()> {
    Ok(())
}

/// The relaxed-SIMD gate. Not implemented yet (RED).
fn gate_relaxed(_wasm_tools: &Path, _artifact: &Path) -> Res<()> {
    Ok(())
}

/// The SHA-256 of a file, as lowercase hex.
fn file_sha256(path: &Path) -> Res<String> {
    let bytes = fs::read(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    Ok(ksci_wire::sha256_hex(&bytes))
}

/// Whether `needle` (non-empty) occurs in `hay`.
fn contains(hay: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && hay.windows(needle.len()).any(|w| w == needle)
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

    #[test]
    fn release_remap_flags_cover_src_and_cargo_home() {
        let flags = remap_flags(Path::new("/x/src"), Path::new("/h/.cargo")).unwrap();
        assert!(
            flags.contains(&"--remap-path-prefix=/x/src=/src".to_owned()),
            "{flags:?}"
        );
        assert!(
            flags.contains(&"--remap-path-prefix=/h/.cargo=/cargo".to_owned()),
            "{flags:?}"
        );
        assert!(remap_flags(Path::new("rel"), Path::new("/h")).is_err());
        assert!(remap_flags(Path::new("/a=b"), Path::new("/h")).is_err());
    }

    #[test]
    fn release_native_link_flags_apple_only() {
        let apple = native_link_flags("aarch64-apple-darwin", Path::new("/t/src")).unwrap();
        assert_eq!(apple, ["-Clink-arg=-Wl,-oso_prefix,/t/src/"]);
        let linux = native_link_flags("x86_64-unknown-linux-gnu", Path::new("/t/src")).unwrap();
        assert!(linux.is_empty());
        assert!(native_link_flags("aarch64-apple-darwin", Path::new("/a,b")).is_err());
    }

    #[test]
    fn release_contains_finds_needles() {
        assert!(contains(b"abc/Users/x/def", b"/Users/x"));
        assert!(!contains(b"abc", b"abcd"));
        assert!(!contains(b"abc", b""));
    }

    /// A fresh scratch directory per test, removed on drop.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Scratch {
            let dir =
                env::temp_dir().join(format!("ksci-xtask-gates-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Scratch(dir)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn wasm_tools() -> PathBuf {
        pinned_tool("wasm-tools").unwrap()
    }

    /// The fixture `name` of xtask/tests/fixtures/gates, assembled by `wasm-tools parse` into
    /// `dir`.
    fn assemble(dir: &Scratch, name: &str) -> PathBuf {
        let wat = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("gates")
            .join(name);
        let wasm = dir.0.join(name).with_extension("wasm");
        let mut parse = Command::new(wasm_tools());
        parse.arg("parse").arg(&wat).arg("-o").arg(&wasm);
        run_checked(&mut parse, "wasm-tools parse").unwrap();
        wasm
    }

    /// The shipped component of this checkout; it must have been built.
    fn dist_component() -> PathBuf {
        let path = workspace::root()
            .unwrap()
            .join("target")
            .join("dist")
            .join(COMPONENT);
        assert!(
            path.is_file(),
            "{} is missing; run: cargo xtask release dist",
            path.display()
        );
        path
    }

    #[test]
    fn gate_imports_rejects_fixture() {
        let dir = Scratch::new("imports");
        let wasm = assemble(&dir, "imports.wat");
        let err = gate_imports(&wasm_tools(), &wasm).expect_err("a component with an import");
        assert!(err.contains("import f: func();"), "{err}");
    }

    #[test]
    fn gate_imports_accepts_dist() {
        gate_imports(&wasm_tools(), &dist_component()).unwrap();
    }

    #[test]
    fn gate_relaxed_rejects_fixture() {
        let dir = Scratch::new("relaxed");
        let wasm = assemble(&dir, "relaxed-simd.wat");
        let err = gate_relaxed(&wasm_tools(), &wasm).expect_err("a module using relaxed SIMD");
        assert!(err.contains("relaxed SIMD"), "{err}");
    }

    #[test]
    fn gate_relaxed_accepts_dist() {
        let component = dist_component();
        gate_relaxed(&wasm_tools(), &component).unwrap();
        // The core module inside it, extracted on its own.
        let dir = Scratch::new("unbundle");
        let mut unbundle = Command::new(wasm_tools());
        unbundle
            .args(["component", "unbundle", "--threshold", "0", "--module-dir"])
            .arg(&dir.0)
            .arg(&component)
            .arg("-o")
            .arg(dir.0.join("outer.wasm"));
        run_checked(&mut unbundle, "wasm-tools component unbundle").unwrap();
        let mut modules = 0;
        for entry in fs::read_dir(&dir.0).unwrap() {
            let path = entry.unwrap().path();
            if path
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("unbundled-module")
            {
                gate_relaxed(&wasm_tools(), &path).unwrap();
                modules += 1;
            }
        }
        assert!(
            modules >= 1,
            "no core module unbundled from {}",
            component.display()
        );
    }
}
