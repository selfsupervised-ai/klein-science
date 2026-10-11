// provenance: original
//! Repository guards.
//!
//! `check no-corpus` (CONF-02, CLAUDE.md rule 6) keeps corpus, golden and vector bytes out of git:
//! git holds recipes, manifests and aggregate scores, and the bytes live in the artifact store.
//!
//! It judges the git repository of the current directory (not the tree xtask was compiled
//! from), and the bytes git would commit: each index entry by its staged blob, never by the
//! working copy, plus each untracked, non-ignored file (what `git add` stages next). An entry it
//! cannot size fails the check rather than being skipped.

use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

use clap::Subcommand;

use crate::Res;

/// `cargo xtask check ...`
#[derive(Subcommand)]
pub enum CheckCmd {
    /// Fail when a staged (index) blob or an untracked, non-ignored file looks like corpus bytes.
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
/// Extensions allowed under `conformance/corpora/` (fetch recipes and notes only, rule 6).
const CORPORA_EXT: [&str; 2] = ["toml", "md"];

fn no_corpus() -> Res<ExitCode> {
    let root = repo_root()?;
    let mut files = index_files(&root)?;
    let staged = files.len();
    files.extend(untracked_files(&root)?);
    let found = violations(&files);
    for v in &found {
        println!("{v}");
    }
    if found.is_empty() {
        println!(
            "no-corpus: {} files checked ({staged} index entries, {} untracked) in {}, no violations",
            files.len(),
            files.len() - staged,
            root.display()
        );
        Ok(ExitCode::SUCCESS)
    } else {
        eprintln!(
            "error: no-corpus: {} violation(s); corpus bytes belong in the artifact store",
            found.len()
        );
        Ok(ExitCode::FAILURE)
    }
}

/// Runs `git -C <dir> <args>` and returns its stdout; a non-zero exit is an error.
fn git_out(dir: &Path, args: &[&str]) -> Res<Vec<u8>> {
    let out = Command::new("git")
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

/// The top level of the git work tree of the current directory: the guard judges the
/// repository it is asked about, not the tree xtask was compiled from.
fn repo_root() -> Res<PathBuf> {
    let cwd = env::current_dir().map_err(|e| format!("current directory: {e}"))?;
    let out = git_out(&cwd, &["rev-parse", "--show-toplevel"])?;
    let top = String::from_utf8(out).map_err(|e| format!("git rev-parse: {e}"))?;
    let top = top.trim_end_matches(['\n', '\r']);
    if top.is_empty() {
        return Err(format!("{} is not inside a git work tree", cwd.display()));
    }
    Ok(PathBuf::from(top))
}

/// Every index entry as `(path, staged blob size)`: the bytes the next commit holds, whatever
/// the working copy says. An entry that is not a readable blob (a submodule, a missing object)
/// is an error: what cannot be sized cannot be judged.
fn index_files(root: &Path) -> Res<Vec<(String, u64)>> {
    let listing = git_out(root, &["ls-files", "-z", "--stage"])?;
    let mut entries = Vec::new();
    for record in listing.split(|b| *b == 0).filter(|r| !r.is_empty()) {
        // `<mode> SP <object> SP <stage> TAB <path>`
        let record = String::from_utf8_lossy(record);
        let malformed = || format!("git ls-files --stage: malformed record {record:?}");
        let (meta, path) = record.split_once('\t').ok_or_else(malformed)?;
        let mut fields = meta.split(' ');
        let (Some(mode), Some(oid)) = (fields.next(), fields.next()) else {
            return Err(malformed());
        };
        if mode == "160000" {
            return Err(format!(
                "no-corpus: {path}: index entry is a submodule (mode 160000) with no blob to \
                 judge; corpora are fetch recipes, not submodules"
            ));
        }
        entries.push((path.to_owned(), oid.to_owned()));
    }
    let sizes = blob_sizes(root, &entries)?;
    entries
        .into_iter()
        .zip(sizes)
        .map(|((path, oid), size)| {
            size.map_err(|why| format!("no-corpus: {path}: staged object {oid} {why}"))
                .map(|s| (path, s))
        })
        .collect()
}

/// The size of each entry's object, in order, from one `git cat-file --batch-check`. An object
/// that is missing or not a blob yields `Err(reason)` in its slot.
fn blob_sizes(root: &Path, entries: &[(String, String)]) -> Res<Vec<Result<u64, String>>> {
    let input: String = entries.iter().map(|(_, oid)| format!("{oid}\n")).collect();
    let mut child = Command::new("git")
        .arg("-C")
        .arg(root)
        .args([
            "cat-file",
            "--batch-check=%(objectname) %(objecttype) %(objectsize)",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("run git cat-file: {e}"))?;
    let mut stdin = child.stdin.take().ok_or("git cat-file: no stdin")?;
    // Write from a thread: cat-file answers line by line, so a large index would otherwise fill
    // the stdout pipe while this side is still writing.
    let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()));
    let out = child
        .wait_with_output()
        .map_err(|e| format!("git cat-file: {e}"))?;
    writer
        .join()
        .map_err(|_| "git cat-file: writer thread panicked".to_owned())?
        .map_err(|e| format!("git cat-file: write: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "git cat-file: {}: {}",
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    let sizes: Vec<Result<u64, String>> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|line| {
            let mut f = line.split(' ');
            match (f.next(), f.next(), f.next()) {
                (Some(_), Some("blob"), Some(size)) => size
                    .parse::<u64>()
                    .map_err(|e| format!("has an unreadable size {size:?}: {e}")),
                (Some(_), Some("missing"), None) => {
                    Err("is missing from the object store".to_owned())
                }
                (Some(_), Some(kind), _) => Err(format!("is a {kind}, not a blob")),
                _ => Err(format!("is unreadable ({line:?})")),
            }
        })
        .collect();
    if sizes.len() != entries.len() {
        return Err(format!(
            "git cat-file: {} answers for {} index entries",
            sizes.len(),
            entries.len()
        ));
    }
    Ok(sizes)
}

/// Untracked, non-ignored files as `(path, size)`: what `git add` stages next. They have no
/// index bytes, so the working copy is judged, and one that cannot be read is an error. An
/// embedded repository (listed with a trailing `/`) holds none of this repository's bytes; it is
/// named, not judged.
fn untracked_files(root: &Path) -> Res<Vec<(String, u64)>> {
    let listing = git_out(root, &["ls-files", "-z", "--others", "--exclude-standard"])?;
    let mut files = Vec::new();
    for raw in listing.split(|b| *b == 0).filter(|p| !p.is_empty()) {
        let path = String::from_utf8_lossy(raw).into_owned();
        if path.ends_with('/') {
            println!("no-corpus: note: {path} is an embedded repository, not judged");
            continue;
        }
        // `symlink_metadata`: git stores a symlink as its target string, not the bytes it names.
        let meta = fs::symlink_metadata(root.join(&path))
            .map_err(|e| format!("no-corpus: untracked {path}: {e}"))?;
        if !(meta.is_file() || meta.is_symlink()) {
            return Err(format!(
                "no-corpus: untracked {path} is neither a file nor a symlink"
            ));
        }
        files.push((path, meta.len()));
    }
    Ok(files)
}

/// The violations among `(path, size)` pairs, sorted by path, each reported once (a path can
/// appear more than once, e.g. in several merge stages).
///
/// Letter case: on a case-insensitive file system (the macOS default) `Conformance/Corpora/` is
/// the directory `conformance/corpora/`, so the policy directories and extensions are matched on
/// the ASCII-lowercased path. A case variant gets a directory's restrictions, never its
/// exemptions: `TestData/` has the testdata size limit but not the payload exemption, and only
/// the exact `Cargo.lock` and `LICENSE` are size-exempt. Every policy name is pure ASCII, and
/// Unicode normalisation (NFC/NFD) never adds or removes an ASCII letter, so normalisation
/// variants cannot spell one; non-ASCII case-folding aliases are not handled (see the plan
/// SUMMARY, review fix R1').
pub fn violations(files: &[(String, u64)]) -> Vec<String> {
    let mut sorted: Vec<&(String, u64)> = files.iter().collect();
    sorted.sort();
    sorted.dedup();
    let mut out: Vec<String> = Vec::new();
    let mut push = |v: String| {
        if !out.contains(&v) {
            out.push(v);
        }
    };
    for (path, size) in sorted {
        let p = Path::new(path);
        let folded = path.to_ascii_lowercase();
        let fp = Path::new(&folded);
        let ext = fp.extension().and_then(|e| e.to_str()).unwrap_or("");
        // Exemption: the exact name only. Restriction: any case variant.
        let testdata_exempt = p.components().any(|c| c.as_os_str() == "testdata");
        let testdata_limited = fp.components().any(|c| c.as_os_str() == "testdata");
        if *size > MAX_FILE && !SIZE_EXEMPT.contains(&path.as_str()) {
            push(format!(
                "{path}: {size} bytes, above the {MAX_FILE}-byte limit"
            ));
        }
        if !testdata_exempt && PAYLOAD_EXT.contains(&ext) {
            push(format!("{path}: .{ext} payload outside testdata/"));
        }
        if testdata_limited && *size > MAX_TESTDATA {
            push(format!(
                "{path}: {size} bytes under testdata/, above the {MAX_TESTDATA}-byte limit"
            ));
        }
        if fp.starts_with("conformance/golden") && !GOLDEN_EXT.contains(&ext) {
            push(format!(
                "{path}: conformance/golden/ holds only .json and .md manifests"
            ));
        }
        if fp.starts_with("conformance/corpora") && !CORPORA_EXT.contains(&ext) {
            push(format!(
                "{path}: conformance/corpora/ holds only .toml recipes and .md notes"
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

    #[test]
    fn no_corpus_corpora_extensions() {
        let tree = files(&[
            ("conformance/corpora/rfc8785.toml", 10),
            ("conformance/corpora/README.md", 10),
            ("conformance/corpora/leak.bin", 1),
        ]);
        assert_eq!(
            violations(&tree),
            [
                "conformance/corpora/leak.bin: conformance/corpora/ holds only .toml recipes and .md notes"
            ]
        );
    }

    #[test]
    fn no_corpus_policy_case_variants() {
        // A case variant of a policy directory gets the policy's restrictions, never its
        // exemptions: `TestData/` has the testdata size limit but no payload exemption.
        let tree = files(&[
            ("Conformance/Corpora/leak.bin", 1),
            ("CONFORMANCE/golden/cases.txt", 1),
            ("conformance/Golden/m.JSON", 1),
            ("crates/x/TestData/big.json", MAX_TESTDATA + 1),
            ("crates/x/TestData/x.jsonl", 1),
        ]);
        assert_eq!(
            violations(&tree),
            [
                "CONFORMANCE/golden/cases.txt: conformance/golden/ holds only .json and .md manifests",
                "Conformance/Corpora/leak.bin: conformance/corpora/ holds only .toml recipes and .md notes",
                "crates/x/TestData/big.json: 65537 bytes under testdata/, above the 65536-byte limit",
                "crates/x/TestData/x.jsonl: .jsonl payload outside testdata/",
            ]
        );
    }

    #[test]
    fn no_corpus_reports_each_violation_once() {
        // One path in two merge stages: the size differs, the extension rule is shared.
        let tree = files(&[
            ("data/x.jsonl", 1),
            ("data/x.jsonl", MAX_FILE + 1),
            ("data/x.jsonl", 1),
        ]);
        assert_eq!(
            violations(&tree),
            [
                "data/x.jsonl: .jsonl payload outside testdata/",
                "data/x.jsonl: 262145 bytes, above the 262144-byte limit",
            ]
        );
    }
}
