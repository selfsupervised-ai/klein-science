// provenance: original
//! Review findings R1-R3 on plan 01-02: each test reproduces a bypass of an xtask guard by running
//! the built `xtask` binary as a child process.
//!
//! - R1: `check no-corpus` must judge the bytes git would commit (the index), in the repository it
//!   runs in, and fail closed on an entry it cannot size.
//! - R2: `ci` must never find `cargo` by a `PATH` lookup.
//! - R3: `tools check` must never find `rust-analyzer` (or `rustup`) by a `PATH` lookup.
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

/// A temporary git repository with a 300 KiB `conformance/corpora/leak.bin` staged.
fn repo_with_staged_leak(name: &str) -> (PathBuf, PathBuf) {
    let repo = scratch(name);
    git(&repo, &["init", "-q"]);
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
    let repo = scratch("untracked");
    git(&repo, &["init", "-q"]);
    fs::write(repo.join("big.txt"), vec![b'a'; 300 * KIB]).unwrap();
    let out = no_corpus(&repo);
    let shown = text(&out);
    assert!(!out.status.success(), "no-corpus passed:\n{shown}");
    assert!(shown.contains("big.txt: 307200 bytes"), "{shown}");
}

#[test]
fn no_corpus_corpora_holds_only_recipes() {
    let repo = scratch("corpora-ext");
    git(&repo, &["init", "-q"]);
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
    let repo = scratch("gitlink");
    git(&repo, &["init", "-q"]);
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
    let repo = scratch("missing-blob");
    git(&repo, &["init", "-q"]);
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

#[test]
fn ci_never_runs_cargo_from_path() {
    let dir = scratch("ci-stub-home");
    let (stub, log) = recording_cargo(&dir, &channel());
    let (out, marker) = ci_with("ci-path", &stub.into_os_string());
    let shown = text(&out);
    assert!(
        !marker.exists(),
        "ci ran the cargo found on PATH ({}):\n{shown}",
        fs::read_to_string(&marker).unwrap_or_default()
    );
    assert!(out.status.success(), "{shown}");
    let calls = fs::read_to_string(&log).unwrap_or_default();
    let lines: Vec<&str> = calls.lines().collect();
    assert!(lines.contains(&"xtask tools check"), "{calls}\n{shown}");
    assert!(lines.contains(&"xtask check no-corpus"), "{calls}\n{shown}");
    assert!(
        shown.contains(&format!("ci: all {} steps passed", lines.len())),
        "every step must reach the invoking cargo:\n{calls}\n{shown}"
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
