// provenance: original
//! Repository guards.
//!
//! `check no-corpus` (CONF-02, CLAUDE.md rule 6) keeps corpus, golden and vector bytes out of git:
//! git holds recipes, manifests and aggregate scores, and the bytes live in the artifact store.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::process::{Command, ExitCode};

use clap::Subcommand;

use crate::Res;
use crate::store::workspace_root;

/// `cargo xtask check ...`
#[derive(Subcommand)]
pub enum CheckCmd {
    /// Fail when a tracked, staged or untracked (non-ignored) file looks like corpus bytes.
    NoCorpus,
}

/// Runs one `check` command.
pub fn run(cmd: CheckCmd) -> Res<ExitCode> {
    match cmd {
        CheckCmd::NoCorpus => no_corpus(),
    }
}

const KIB: u64 = 1024;
/// Largest file allowed anywhere, except [`SIZE_EXEMPT`].
const MAX_FILE: u64 = 256 * KIB;
/// Largest file allowed under a `testdata/` directory.
const MAX_TESTDATA: u64 = 64 * KIB;
/// Files allowed above [`MAX_FILE`].
const SIZE_EXEMPT: [&str; 2] = ["Cargo.lock", "LICENSE"];
/// Extensions that are corpus or binary payloads outside `testdata/`.
const PAYLOAD_EXT: [&str; 10] = [
    "parquet", "arrow", "jsonl", "ndjson", "tar", "tgz", "gz", "zst", "wasm", "whl",
];
/// Extensions allowed under `conformance/golden/` (manifests and notes only).
const GOLDEN_EXT: [&str; 2] = ["json", "md"];

fn no_corpus() -> Res<ExitCode> {
    let root = workspace_root();
    let out = Command::new("git")
        .arg("-C")
        .arg(&root)
        .args([
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
        ])
        .output()
        .map_err(|e| format!("run git ls-files: {e}"))?;
    if !out.status.success() {
        return Err(format!("git ls-files: {}", out.status));
    }
    let paths: BTreeSet<String> = out
        .stdout
        .split(|b| *b == 0)
        .filter(|p| !p.is_empty())
        .map(|p| String::from_utf8_lossy(p).into_owned())
        .collect();
    let mut files = Vec::with_capacity(paths.len());
    for path in paths {
        // A tracked file deleted in the working tree has no bytes to check.
        if let Ok(meta) = fs::metadata(root.join(&path)) {
            files.push((path, meta.len()));
        }
    }
    let found = violations(&files);
    for v in &found {
        println!("{v}");
    }
    if found.is_empty() {
        println!("no-corpus: {} files checked, no violations", files.len());
        Ok(ExitCode::SUCCESS)
    } else {
        eprintln!(
            "error: no-corpus: {} violation(s); corpus bytes belong in the artifact store",
            found.len()
        );
        Ok(ExitCode::FAILURE)
    }
}

/// The violations among `(path, size)` pairs, sorted by path.
pub fn violations(files: &[(String, u64)]) -> Vec<String> {
    let mut sorted: Vec<&(String, u64)> = files.iter().collect();
    sorted.sort();
    let mut out = Vec::new();
    for (path, size) in sorted {
        let p = Path::new(path);
        let ext = p
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let in_testdata = p.components().any(|c| c.as_os_str() == "testdata");
        if *size > MAX_FILE && !SIZE_EXEMPT.contains(&path.as_str()) {
            out.push(format!(
                "{path}: {size} bytes, above the {MAX_FILE}-byte limit"
            ));
        }
        if !in_testdata && PAYLOAD_EXT.contains(&ext.as_str()) {
            out.push(format!("{path}: .{ext} payload outside testdata/"));
        }
        if in_testdata && *size > MAX_TESTDATA {
            out.push(format!(
                "{path}: {size} bytes under testdata/, above the {MAX_TESTDATA}-byte limit"
            ));
        }
        if p.starts_with("conformance/golden") && !GOLDEN_EXT.contains(&ext.as_str()) {
            out.push(format!(
                "{path}: conformance/golden/ holds only .json and .md manifests"
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files(list: &[(&str, u64)]) -> Vec<(String, u64)> {
        list.iter().map(|(p, s)| ((*p).to_owned(), *s)).collect()
    }

    #[test]
    fn no_corpus_clean_tree() {
        let tree = files(&[
            ("Cargo.lock", 900 * KIB),
            ("LICENSE", 300 * KIB),
            ("conformance/corpora/rfc8785.toml", 6 * KIB),
            ("conformance/golden/manifest.json", 2 * KIB),
            ("conformance/golden/README.md", KIB),
            ("crates/x/testdata/small.jsonl", 64 * KIB),
            ("xtask/src/main.rs", 2 * KIB),
        ]);
        assert_eq!(violations(&tree), Vec::<String>::new());
    }

    #[test]
    fn no_corpus_size_limit() {
        let tree = files(&[
            ("src/big.rs", MAX_FILE + 1),
            ("sub/Cargo.lock", MAX_FILE + 1),
            ("ok.rs", MAX_FILE),
        ]);
        assert_eq!(
            violations(&tree),
            [
                "src/big.rs: 262145 bytes, above the 262144-byte limit",
                "sub/Cargo.lock: 262145 bytes, above the 262144-byte limit",
            ]
        );
    }

    #[test]
    fn no_corpus_payload_extensions() {
        let tree: Vec<(String, u64)> = PAYLOAD_EXT
            .iter()
            .map(|e| (format!("data/x.{}", e.to_uppercase()), 1))
            .collect();
        assert_eq!(violations(&tree).len(), PAYLOAD_EXT.len());
        let nested = files(&[("a/testdata/x.jsonl", 1), ("a/testdata/y.wasm", 1)]);
        assert_eq!(violations(&nested), Vec::<String>::new());
    }

    #[test]
    fn no_corpus_testdata_limit() {
        let tree = files(&[
            ("crates/x/testdata/in.json", MAX_TESTDATA + 1),
            ("crates/x/testdata/ok.json", MAX_TESTDATA),
        ]);
        assert_eq!(
            violations(&tree),
            ["crates/x/testdata/in.json: 65537 bytes under testdata/, above the 65536-byte limit"]
        );
    }

    #[test]
    fn no_corpus_golden_extensions() {
        let tree = files(&[
            ("conformance/golden/witness.jsonl", 0),
            ("conformance/golden/cases.txt", 10),
            ("conformance/golden/m.json", 10),
        ]);
        assert_eq!(
            violations(&tree),
            [
                "conformance/golden/cases.txt: conformance/golden/ holds only .json and .md manifests",
                "conformance/golden/witness.jsonl: .jsonl payload outside testdata/",
                "conformance/golden/witness.jsonl: conformance/golden/ holds only .json and .md manifests",
            ]
        );
    }
}
