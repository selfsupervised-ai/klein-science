// provenance: original
//! Review findings R1-R3 on plan 01-02: each test reproduces a bypass of an xtask guard by running
//! the built `xtask` binary as a child process.
//!
//! - R1: `check no-corpus` must judge the bytes git would commit (the index), in the repository it
//!   runs in, and fail closed on an entry it cannot size.
//! - R2: `ci` must never find `cargo` by a `PATH` lookup.
//! - R3: `tools check` must never find `rust-analyzer` (or `rustup`) by a `PATH` lookup.
//!
//! Follow-up review (R1', R2', G, W):
//!
//! - R1': the no-corpus policy directories match whatever the letter case of the path.
//! - R2': `$CARGO` must be the pinned toolchain's cargo file, not merely report its version.
//! - G: `git` is a pinned tool, resolved by absolute path, never by a `PATH` lookup.
//! - W: xtask acts on the klein-science checkout it runs in, and fails closed outside one.
//!
//! Review of plan 01-03:
//!
//! - W3: `check provenance` refuses a symbolic link anywhere under `crates/` or `standin/`, so a
//!   linked directory or file cannot bring unjudged source into a compiled crate.
//!
//! The tests keep `CARGO_MANIFEST_DIR` (via [`workspace_root`]) only to find this source tree's
//! own fixtures, and `CARGO_BIN_EXE_xtask` to find the binary under test.
#![cfg(unix)]

use std::env;
use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const KIB: usize = 1024;

fn xtask() -> Command {
    Command::new(env!("CARGO_BIN_EXE_xtask"))
}

fn workspace_root() -> PathBuf {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest.parent().unwrap_or(manifest).to_path_buf()
}

/// A fresh, empty scratch directory for one test.
fn scratch(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("review-bypass-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// `toolchain.channel` of the workspace's rust-toolchain.toml.
fn channel() -> String {
    let text = fs::read_to_string(workspace_root().join("rust-toolchain.toml")).unwrap();
    text.lines()
        .find_map(|l| l.trim().strip_prefix("channel = \""))
        .and_then(|rest| rest.strip_suffix('"'))
        .expect("rust-toolchain.toml has a channel")
        .to_owned()
}

/// Writes an executable `/bin/sh` script.
fn script(path: &Path, body: &str) {
    fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

/// `PATH` with `first` prepended.
fn path_with(first: &Path) -> OsString {
    let rest = env::var_os("PATH").unwrap_or_default();
    let mut dirs = vec![first.to_path_buf()];
    dirs.extend(env::split_paths(&rest));
    env::join_paths(dirs).unwrap()
}

fn text(out: &Output) -> String {
    format!(
        "status: {}\nstdout:\n{}\nstderr:\n{}",
        out.status,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

// ---------------------------------------------------------------------------------------------
// R1: check no-corpus
// ---------------------------------------------------------------------------------------------

/// Runs `git` in `repo`, isolated from any hook environment of the test run itself.
fn git(repo: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(repo)
        .env_remove("GIT_DIR")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_WORK_TREE")
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {}", text(&out));
}

/// The workspace markers the runtime workspace resolver requires: a `Cargo.toml` whose workspace
/// has an `xtask` member, and a `rust-toolchain.toml`.
fn write_markers(repo: &Path, channel: &str) {
    fs::write(
        repo.join("Cargo.toml"),
        "[workspace]\nresolver = \"3\"\nmembers = [\"xtask\"]\n",
    )
    .unwrap();
    fs::write(
        repo.join("rust-toolchain.toml"),
        format!("[toolchain]\nchannel = \"{channel}\"\n"),
    )
    .unwrap();
}

/// A fresh git repository shaped like a klein-science workspace (the markers are untracked).
fn klein_repo(name: &str) -> PathBuf {
    let repo = scratch(name);
    git(&repo, &["init", "-q"]);
    write_markers(&repo, &channel());
    repo
}

/// A temporary git repository with a 300 KiB `conformance/corpora/leak.bin` staged.
fn repo_with_staged_leak(name: &str) -> (PathBuf, PathBuf) {
    let repo = klein_repo(name);
    let dir = repo.join("conformance/corpora");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("rfc8785.toml"), "name = \"x\"\n").unwrap();
    let leak = dir.join("leak.bin");
    fs::write(&leak, vec![0x5a_u8; 300 * KIB]).unwrap();
    git(&repo, &["add", "conformance"]);
    (repo, leak)
}

fn no_corpus(repo: &Path) -> Output {
    xtask()
        .args(["check", "no-corpus"])
        .current_dir(repo)
        .env_remove("GIT_DIR")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_WORK_TREE")
        .output()
        .unwrap()
}

fn assert_staged_leak_caught(out: &Output) {
    let shown = text(out);
    assert!(!out.status.success(), "no-corpus passed:\n{shown}");
    assert!(
        shown.contains("conformance/corpora/leak.bin: 307200 bytes"),
        "no-corpus did not name the staged 300 KiB blob:\n{shown}"
    );
}

#[test]
fn no_corpus_catches_staged_file_in_the_repository_it_runs_in() {
    let (repo, _leak) = repo_with_staged_leak("intact");
    assert_staged_leak_caught(&no_corpus(&repo));
}

#[test]
fn no_corpus_judges_index_bytes_when_working_copy_truncated() {
    let (repo, leak) = repo_with_staged_leak("truncated");
    fs::write(&leak, b"x").unwrap();
    assert_staged_leak_caught(&no_corpus(&repo));
}

#[test]
fn no_corpus_judges_index_bytes_when_working_copy_deleted() {
    let (repo, leak) = repo_with_staged_leak("deleted");
    fs::remove_file(&leak).unwrap();
    assert_staged_leak_caught(&no_corpus(&repo));
}

#[test]
fn no_corpus_judges_untracked_file() {
    let repo = klein_repo("untracked");
    fs::write(repo.join("big.txt"), vec![b'a'; 300 * KIB]).unwrap();
    let out = no_corpus(&repo);
    let shown = text(&out);
    assert!(!out.status.success(), "no-corpus passed:\n{shown}");
    assert!(shown.contains("big.txt: 307200 bytes"), "{shown}");
}

#[test]
fn no_corpus_corpora_holds_only_recipes() {
    let repo = klein_repo("corpora-ext");
    let dir = repo.join("conformance/corpora");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("rfc8785.toml"), "name = \"x\"\n").unwrap();
    fs::write(dir.join("notes.md"), "notes\n").unwrap();
    fs::write(dir.join("vectors.bin"), vec![1_u8; KIB]).unwrap();
    git(&repo, &["add", "conformance"]);
    let out = no_corpus(&repo);
    let shown = text(&out);
    assert!(!out.status.success(), "no-corpus passed:\n{shown}");
    assert!(
        shown.contains("conformance/corpora/vectors.bin: conformance/corpora/ holds only"),
        "{shown}"
    );
    assert!(!shown.contains("rfc8785.toml:"), "{shown}");
    assert!(!shown.contains("notes.md:"), "{shown}");
}

#[test]
fn no_corpus_fails_closed_on_unsized_index_entry() {
    let repo = klein_repo("gitlink");
    // A gitlink (submodule) entry: its object is not in this repository, so its bytes cannot be
    // judged. The guard must refuse, not skip it.
    git(
        &repo,
        &[
            "update-index",
            "--add",
            "--cacheinfo",
            "160000,1111111111111111111111111111111111111111,vendor/sub",
        ],
    );
    let out = no_corpus(&repo);
    let shown = text(&out);
    assert!(!out.status.success(), "no-corpus passed:\n{shown}");
    assert!(shown.contains("vendor/sub"), "{shown}");
}

#[test]
fn no_corpus_fails_closed_on_missing_blob() {
    let repo = klein_repo("missing-blob");
    // A file entry whose blob is not in the object store: no bytes to size, so refuse.
    git(
        &repo,
        &[
            "update-index",
            "--add",
            "--cacheinfo",
            "100644,2222222222222222222222222222222222222222,ghost.txt",
        ],
    );
    let out = no_corpus(&repo);
    let shown = text(&out);
    assert!(!out.status.success(), "no-corpus passed:\n{shown}");
    assert!(
        shown.contains(
            "ghost.txt: staged object 2222222222222222222222222222222222222222 is missing"
        ),
        "{shown}"
    );
}

// ---------------------------------------------------------------------------------------------
// R2: ci never runs `cargo` from PATH
// ---------------------------------------------------------------------------------------------

/// A fake `cargo` first on `PATH` that records each call in `marker` and exits 0.
fn fake_on_path(dir: &Path, bin: &str, marker: &Path, stdout: &str) {
    script(
        &dir.join(bin),
        &format!(
            "echo \"$0 $*\" >> '{}'\necho '{stdout}'\nexit 0",
            marker.display()
        ),
    );
}

/// Runs `xtask ci` with a fake `cargo` first on `PATH` and `CARGO` set to `cargo_env`.
fn ci_with(name: &str, cargo_env: &OsString) -> (Output, PathBuf) {
    let dir = scratch(name);
    let fakes = dir.join("fakes");
    fs::create_dir_all(&fakes).unwrap();
    let marker = dir.join("path-cargo-ran");
    fake_on_path(&fakes, "cargo", &marker, "");
    let out = xtask()
        .arg("ci")
        .current_dir(workspace_root())
        .env("PATH", path_with(&fakes))
        .env("CARGO", cargo_env)
        .env("KSCI_CACHE", dir.join("cache"))
        .env("KSCI_KLEIN_RS_MARKER", format!("no-such-marker-{name}"))
        .output()
        .unwrap();
    (out, marker)
}

/// A stand-in for the invoking cargo: reports `version`, logs every other call and exits 0.
fn recording_cargo(dir: &Path, version: &str) -> (PathBuf, PathBuf) {
    let bin = dir.join("recording-cargo");
    let log = dir.join("recording-cargo.log");
    script(
        &bin,
        &format!(
            "if [ \"$1\" = \"--version\" ]; then echo 'cargo {version} (recording stub)'; exit 0; fi\n\
             echo \"$*\" >> '{}'\nexit 0",
            log.display()
        ),
    );
    (bin, log)
}

/// `$CARGO_HOME/bin/rustup` (default `$HOME/.cargo/bin/rustup`).
fn rustup() -> PathBuf {
    let home = env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env::var_os("HOME").unwrap()).join(".cargo"));
    home.join("bin").join("rustup")
}

/// The canonical path of the pinned toolchain's cargo, as rustup names it.
fn pinned_cargo() -> PathBuf {
    let out = Command::new(rustup())
        .args(["which", "cargo", "--toolchain", &channel()])
        .output()
        .unwrap();
    assert!(out.status.success(), "rustup which cargo: {}", text(&out));
    let named = PathBuf::from(String::from_utf8_lossy(&out.stdout).trim());
    fs::canonicalize(named).unwrap()
}

/// R2': a `$CARGO` that reports the pinned version but is not the pinned toolchain's cargo file
/// is refused before the first step, and the error names both paths. Nothing on `PATH` runs.
#[test]
fn ci_rejects_cargo_that_is_not_the_pinned_toolchain_file() {
    let dir = scratch("ci-stub-home");
    let (stub, log) = recording_cargo(&dir, &channel());
    let (out, marker) = ci_with("ci-path", &stub.clone().into_os_string());
    let shown = text(&out);
    assert!(
        !marker.exists(),
        "ci ran the cargo found on PATH ({}):\n{shown}",
        fs::read_to_string(&marker).unwrap_or_default()
    );
    assert!(
        !out.status.success(),
        "ci passed with a stub cargo:\n{shown}"
    );
    assert!(
        !log.exists(),
        "ci ran a step with the stub cargo ({}):\n{shown}",
        fs::read_to_string(&log).unwrap_or_default()
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    let stub_real = fs::canonicalize(&stub).unwrap();
    assert!(
        stderr.contains(&stub_real.display().to_string()),
        "error does not name $CARGO:\n{shown}"
    );
    assert!(
        stderr.contains(&pinned_cargo().display().to_string()),
        "error does not name the pinned toolchain's cargo:\n{shown}"
    );
}

#[test]
fn ci_rejects_cargo_of_another_toolchain() {
    let dir = scratch("ci-stub-other");
    let (stub, log) = recording_cargo(&dir, "0.0.1");
    let (out, marker) = ci_with("ci-other", &stub.into_os_string());
    let shown = text(&out);
    assert!(!out.status.success(), "ci passed:\n{shown}");
    assert!(!marker.exists(), "ci ran the cargo found on PATH:\n{shown}");
    assert!(
        !log.exists(),
        "ci ran a step with the wrong cargo:\n{shown}"
    );
}

#[test]
fn ci_rejects_relative_cargo() {
    let (out, marker) = ci_with("ci-relative", &OsString::from("cargo"));
    let shown = text(&out);
    assert!(!out.status.success(), "ci passed:\n{shown}");
    assert!(!marker.exists(), "ci ran the cargo found on PATH:\n{shown}");
}

// ---------------------------------------------------------------------------------------------
// R3: tools check never runs rust-analyzer (or rustup) from PATH
// ---------------------------------------------------------------------------------------------

#[test]
fn tools_check_never_runs_rust_analyzer_from_path() {
    let dir = scratch("tools-ra");
    let fakes = dir.join("fakes");
    fs::create_dir_all(&fakes).unwrap();
    let marker = dir.join("path-tool-ran");
    let ch = channel();
    fake_on_path(
        &fakes,
        "rust-analyzer",
        &marker,
        &format!("rust-analyzer {ch}"),
    );
    fake_on_path(
        &fakes,
        "rustup",
        &marker,
        &fakes.join("rust-analyzer").display().to_string(),
    );
    let out = xtask()
        .args(["tools", "check"])
        .current_dir(workspace_root())
        .env("PATH", path_with(&fakes))
        .output()
        .unwrap();
    let shown = text(&out);
    assert!(
        !marker.exists(),
        "tools check ran a binary found on PATH ({}):\n{shown}",
        fs::read_to_string(&marker).unwrap_or_default()
    );
    let line = String::from_utf8_lossy(&out.stdout)
        .lines()
        .find(|l| l.starts_with("rust-analyzer"))
        .map(str::to_owned)
        .unwrap_or_default();
    assert!(line.contains(" ok "), "rust-analyzer line: {line}\n{shown}");
}

// ---------------------------------------------------------------------------------------------
// R1': no-corpus policy directories ignore letter case
// ---------------------------------------------------------------------------------------------

/// On a case-insensitive file system `Conformance/Corpora/` is the same directory as
/// `conformance/corpora/`, so the policy must hold for every case variant.
#[test]
fn no_corpus_policy_dirs_ignore_case() {
    let repo = klein_repo("policy-case");
    let corpora = repo.join("Conformance/Corpora");
    fs::create_dir_all(&corpora).unwrap();
    fs::write(corpora.join("vectors.bin"), vec![0_u8; KIB]).unwrap();
    let golden = repo.join("Conformance/GOLDEN");
    fs::create_dir_all(&golden).unwrap();
    fs::write(golden.join("cases.txt"), b"x\n").unwrap();
    git(&repo, &["add", "Conformance"]);
    let out = no_corpus(&repo);
    let shown = text(&out);
    assert!(!out.status.success(), "no-corpus passed:\n{shown}");
    assert!(
        shown.contains("Conformance/Corpora/vectors.bin: conformance/corpora/ holds only"),
        "{shown}"
    );
    assert!(
        shown.contains("Conformance/GOLDEN/cases.txt: conformance/golden/ holds only"),
        "{shown}"
    );
}

// ---------------------------------------------------------------------------------------------
// G: git is pinned and never found on PATH
// ---------------------------------------------------------------------------------------------

/// A fake `git` first on `PATH` answers `rev-parse` truthfully and lists nothing: run by the
/// guard, it would hide the staged leak.
#[test]
fn no_corpus_never_runs_git_from_path() {
    let (repo, _leak) = repo_with_staged_leak("git-path");
    let dir = scratch("git-path-fakes");
    let marker = dir.join("path-git-ran");
    script(
        &dir.join("git"),
        &format!(
            "echo \"$0 $*\" >> '{}'\ncase \"$*\" in *rev-parse*) echo '{}';; esac\nexit 0",
            marker.display(),
            repo.display()
        ),
    );
    let out = xtask()
        .args(["check", "no-corpus"])
        .current_dir(&repo)
        .env("PATH", path_with(&dir))
        .env_remove("GIT_DIR")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_WORK_TREE")
        .output()
        .unwrap();
    let shown = text(&out);
    assert!(
        !marker.exists(),
        "no-corpus ran the git found on PATH ({}):\n{shown}",
        fs::read_to_string(&marker).unwrap_or_default()
    );
    assert_staged_leak_caught(&out);
}

// ---------------------------------------------------------------------------------------------
// W: xtask acts on the checkout it runs in
// ---------------------------------------------------------------------------------------------

/// A recipe at `<dir>/conformance/corpora/w-probe.toml` for one small blob, which is put into the
/// scratch cache `cache` first, so the fetch is a cache hit and touches no network.
fn probe_recipe(dir: &Path, cache: &Path, tag: &str) -> PathBuf {
    let blob = dir.join("probe-blob.txt");
    fs::write(&blob, format!("workspace probe {tag}\n")).unwrap();
    let out = xtask()
        .args(["store", "put"])
        .arg(&blob)
        .env("KSCI_CACHE", cache)
        .output()
        .unwrap();
    assert!(out.status.success(), "store put: {}", text(&out));
    let digest = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    fs::remove_file(&blob).unwrap();
    let corpora = dir.join("conformance/corpora");
    fs::create_dir_all(&corpora).unwrap();
    let recipe = corpora.join("w-probe.toml");
    fs::write(
        &recipe,
        format!(
            "schema = \"ksci.recipe.v1\"\nname = \"w-probe-{tag}\"\n\n[[file]]\n\
             path = \"probe.txt\"\nsha256 = \"{digest}\"\n"
        ),
    )
    .unwrap();
    recipe
}

/// Runs `xtask <args>` in `dir` with no corpora or target-directory override.
fn xtask_in(dir: &Path, args: &[&str], cache: &Path) -> Output {
    xtask()
        .args(args)
        .current_dir(dir)
        .env("KSCI_CACHE", cache)
        .env_remove("KSCI_CORPORA")
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("GIT_DIR")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_WORK_TREE")
        .output()
        .unwrap()
}

#[test]
fn workspace_is_the_checkout_xtask_runs_in() {
    let dir = scratch("second-clone");
    let clone = dir.join("clone");
    let root = workspace_root();
    let (src, dst) = (root.display().to_string(), clone.display().to_string());
    git(&dir, &["clone", "-q", &src, &dst]);
    // A toolchain channel that only the second clone names.
    let toolchain = clone.join("rust-toolchain.toml");
    let before = fs::read_to_string(&toolchain).unwrap();
    let pinned = format!("channel = \"{}\"", channel());
    assert!(before.contains(&pinned), "{before}");
    fs::write(
        &toolchain,
        before.replace(&pinned, "channel = \"ksci-second-clone\""),
    )
    .unwrap();
    let cache = dir.join("cache");

    let out = xtask_in(&clone, &["tools", "check"], &cache);
    let shown = text(&out);
    let line = String::from_utf8_lossy(&out.stdout)
        .lines()
        .find(|l| l.starts_with("rust-analyzer"))
        .map(str::to_owned)
        .unwrap_or_default();
    assert!(
        line.contains("ksci-second-clone"),
        "tools check did not read the second clone's rust-toolchain.toml: {line:?}\n{shown}"
    );

    let tag = format!("clone-{}", std::process::id());
    let recipe = probe_recipe(&clone, &cache, &tag);
    let out = xtask_in(
        &clone,
        &["store", "fetch", &recipe.display().to_string()],
        &cache,
    );
    let shown = text(&out);
    let rel = Path::new("target/corpora")
        .join(format!("w-probe-{tag}"))
        .join("probe.txt");
    let in_original = root.join(&rel);
    let wrong = in_original.exists();
    let _ = fs::remove_dir_all(in_original.parent().unwrap());
    assert!(out.status.success(), "store fetch: {shown}");
    assert!(
        !wrong,
        "store fetch wrote into the checkout xtask was built in:\n{shown}"
    );
    assert!(
        clone.join(&rel).is_file(),
        "store fetch did not materialise into the second clone's target/corpora:\n{shown}"
    );
}

#[test]
fn workspace_outside_a_checkout_fails_closed() {
    // Outside every git work tree: the scratch directory lies inside this checkout's target/.
    let base = env::temp_dir().join(format!("ksci-xtask-outside-{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    let plain = base.join("plain");
    let foreign = base.join("foreign");
    let no_xtask = base.join("no-xtask");
    for d in [&plain, &foreign, &no_xtask] {
        fs::create_dir_all(d).unwrap();
    }
    git(&foreign, &["init", "-q"]);
    git(&no_xtask, &["init", "-q"]);
    write_markers(&no_xtask, &channel());
    fs::write(
        no_xtask.join("Cargo.toml"),
        "[workspace]\nmembers = [\"crates/*\"]\n",
    )
    .unwrap();
    let cache = base.join("cache");
    for (i, d) in [&plain, &foreign, &no_xtask].into_iter().enumerate() {
        let tag = format!("outside-{i}-{}", std::process::id());
        let recipe = probe_recipe(d, &cache, &tag);
        let recipe = recipe.display().to_string();
        let leaked = workspace_root()
            .join("target/corpora")
            .join(format!("w-probe-{tag}"));
        for args in [
            &["tools", "check"][..],
            &["check", "no-corpus"][..],
            &["store", "fetch", recipe.as_str()][..],
        ] {
            let out = xtask_in(d, args, &cache);
            let shown = text(&out);
            let wrote = leaked.exists();
            let _ = fs::remove_dir_all(&leaked);
            assert!(
                !wrote,
                "{args:?} in {} wrote into {}",
                d.display(),
                leaked.display()
            );
            assert!(
                !out.status.success(),
                "{args:?} in {} passed:\n{shown}",
                d.display()
            );
            assert!(
                String::from_utf8_lossy(&out.stderr).contains("klein-science"),
                "{args:?} in {}: no clear klein-science workspace error:\n{shown}",
                d.display()
            );
        }
    }
    let _ = fs::remove_dir_all(&base);
}

// ---------------------------------------------------------------------------------------------
// Review of plan 01-03, W3: check provenance and symbolic links
// ---------------------------------------------------------------------------------------------

/// A klein repository with a NOTICE, one headed source file in `crates/k/src/`, and a headerless
/// `review-fixture/witness.rs` (plus a headed `headed.rs`) outside every checked tree.
fn repo_with_outside_witness(name: &str) -> PathBuf {
    let repo = klein_repo(name);
    fs::write(repo.join("NOTICE"), "klein-science\n").unwrap();
    let src = repo.join("crates/k/src");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("lib.rs"), "// provenance: original\n").unwrap();
    let fixture = repo.join("review-fixture");
    fs::create_dir_all(&fixture).unwrap();
    fs::write(fixture.join("witness.rs"), "pub fn witness() {}\n").unwrap();
    fs::write(
        fixture.join("headed.rs"),
        "// provenance: original\npub fn headed() {}\n",
    )
    .unwrap();
    repo
}

fn provenance(repo: &Path) -> Output {
    xtask()
        .args(["check", "provenance"])
        .current_dir(repo)
        .env_remove("GIT_DIR")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_WORK_TREE")
        .output()
        .unwrap()
}

fn assert_symlink_rejected(out: &Output, path: &str) {
    let shown = text(out);
    assert!(!out.status.success(), "check provenance passed:\n{shown}");
    assert!(
        String::from_utf8_lossy(&out.stdout).contains(&format!(
            "{path}: symbolic link inside a checked source tree"
        )),
        "check provenance did not name the symbolic link {path}:\n{shown}"
    );
}

#[test]
fn provenance_rejects_directory_symlink_in_source_tree() {
    // Git lists a directory symlink as one extension-less path; `#[path = "imported/witness.rs"]`
    // would compile the headerless file behind it.
    let repo = repo_with_outside_witness("prov-dir-link");
    std::os::unix::fs::symlink(
        "../../../review-fixture",
        repo.join("crates/k/src/imported"),
    )
    .unwrap();
    git(&repo, &["add", "-N", "."]);
    assert_symlink_rejected(&provenance(&repo), "crates/k/src/imported");
}

#[test]
fn provenance_rejects_file_symlink_in_source_tree() {
    // Even a link to a headed file is refused: links are not judged through.
    let repo = repo_with_outside_witness("prov-file-link");
    std::os::unix::fs::symlink(
        "../../../review-fixture/headed.rs",
        repo.join("crates/k/src/alias.rs"),
    )
    .unwrap();
    assert_symlink_rejected(&provenance(&repo), "crates/k/src/alias.rs");
}

#[test]
fn provenance_rejects_symlinked_standin_tree() {
    // A link anywhere under a checked tree, the tree root included, is refused.
    let repo = repo_with_outside_witness("prov-tree-link");
    std::os::unix::fs::symlink("review-fixture", repo.join("standin")).unwrap();
    git(&repo, &["add", "-N", "."]);
    assert_symlink_rejected(&provenance(&repo), "standin");
}
