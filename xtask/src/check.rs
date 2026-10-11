// provenance: original
//! Repository guards.
//!
//! `check no-corpus` (CONF-02, CLAUDE.md rule 6) keeps corpus, golden and vector bytes out of git:
//! git holds recipes, manifests and aggregate scores, and the bytes live in the artifact store.
//!
//! It judges the klein-science checkout of the current directory ([`workspace::root`], never
//! the tree xtask was compiled from), and the bytes git would commit: each index entry by its staged blob, never by the
//! working copy, plus each untracked, non-ignored file (what `git add` stages next). An entry it
//! cannot size fails the check rather than being skipped.
//!
//! `check provenance` (LIC-02, CLAUDE.md rule 2) makes every shipped source file name where it
//! came from: original work, a port of an allowed project (tied to [`PORT_POLICY`] and to a
//! `Project:` block in NOTICE), or a paper by DOI. It judges the working copy of every tracked or
//! untracked, non-ignored `.rs` file under `crates/` and `.py` file under `standin/`.

use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{ExitCode, Stdio};

use clap::Subcommand;

use crate::Res;
use crate::{host, workspace};

/// `cargo xtask check ...`
#[derive(Subcommand)]
pub enum CheckCmd {
    /// Fail when a staged (index) blob or an untracked, non-ignored file looks like corpus bytes.
    NoCorpus,
    /// Fail when a `.rs` file under `crates/` or a `.py` file under `standin/` lacks a valid
    /// provenance header, or a port header is outside the licence policy or missing from NOTICE.
    Provenance,
}

/// Runs one `check` command.
pub fn run(cmd: CheckCmd) -> Res<ExitCode> {
    match cmd {
        CheckCmd::NoCorpus => no_corpus(),
        CheckCmd::Provenance => provenance(),
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
    let root = workspace::root()?;
    let mut files = index_files(&root)?;
    let staged = files.len();
    files.extend(untracked_files(&root)?);
    let found = violations(&files);
    for v in &found {
        println!("{v}");
    }
    if found.is_empty() {
        println!(
            "no-corpus: {} files checked ({staged} index entries, {} untracked) in {} with {}, \
             no violations",
            files.len(),
            files.len() - staged,
            root.display(),
            host::git_path()?.display()
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

/// Every index entry as `(path, staged blob size)`: the bytes the next commit holds, whatever
/// the working copy says. An entry that is not a readable blob (a submodule, a missing object)
/// is an error: what cannot be sized cannot be judged.
fn index_files(root: &Path) -> Res<Vec<(String, u64)>> {
    let listing = host::git_output(root, &["ls-files", "-z", "--stage"])?;
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
    let mut child = host::git()?
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
    let listing = host::git_output(root, &["ls-files", "-z", "--others", "--exclude-standard"])?;
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

/// The licence policy for ported code (CLAUDE.md rule 2): every upstream project that may be
/// ported, by its exact lower-case name, and the SPDX identifier its port headers must declare.
/// Anything else (Symbolica, FLINT, Giac, any GPL or LGPL project) is not portable.
pub const PORT_POLICY: [(&str, &str); 5] = [
    ("diofant", "BSD-3-Clause"),
    ("fricas", "BSD-3-Clause"),
    ("mpmath", "BSD-3-Clause"),
    ("symengine", "MIT"),
    ("sympy", "BSD-3-Clause"),
];

/// The notice file at the workspace root that every port's upstream notice lives in.
const NOTICE_FILE: &str = "NOTICE";

fn provenance() -> Res<ExitCode> {
    let root = workspace::root()?;
    // Tracked and untracked, non-ignored files: a new file cannot dodge the check by not being
    // added yet. Scope is decided here, not by a git pathspec, so case variants are judged too.
    let listing = host::git_output(
        &root,
        &[
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
        ],
    )?;
    let mut paths = Vec::new();
    for raw in listing.split(|b| *b == 0).filter(|p| !p.is_empty()) {
        let path = String::from_utf8(raw.to_vec())
            .map_err(|_| format!("provenance: path {raw:?} is not UTF-8"))?;
        if in_checked_tree(&path) {
            paths.push(path);
        }
    }
    paths.sort();
    paths.dedup();
    let notice = fs::read_to_string(root.join(NOTICE_FILE))
        .map_err(|e| format!("provenance: {NOTICE_FILE}: {e}"))?;
    // The working copy is judged: it is what cargo compiles. A listed file that cannot be read
    // (a tracked file deleted from the working copy) fails the check rather than being skipped.
    // A symbolic link anywhere in a checked tree is refused, never followed (review W3): git
    // lists a linked directory as one extension-less path, so the source behind it would
    // otherwise go unjudged while `#[path]` or a module file compiles it.
    let mut files = Vec::with_capacity(paths.len());
    for path in paths {
        let meta = fs::symlink_metadata(root.join(&path))
            .map_err(|e| format!("provenance: {path}: {e}"))?;
        if meta.is_symlink() {
            files.push((path, Entry::Symlink));
        } else if provenance_in_scope(&path) {
            let bytes =
                fs::read(root.join(&path)).map_err(|e| format!("provenance: {path}: {e}"))?;
            files.push((path, Entry::Bytes(bytes)));
        }
    }
    let found = provenance_violations(&files, &notice);
    for v in &found {
        println!("{v}");
    }
    if found.is_empty() {
        let blocks = notice
            .split('\n')
            .filter(|l| l.starts_with("Project: "))
            .count();
        println!(
            "provenance ok, {} files checked in {}, {NOTICE_FILE} has {blocks} Project blocks",
            files.len(),
            root.display()
        );
        Ok(ExitCode::SUCCESS)
    } else {
        eprintln!(
            "error: provenance: {} violation(s); every .rs file under crates/ and .py file under \
             standin/ starts with a provenance header, and neither tree holds a symbolic link",
            found.len()
        );
        Ok(ExitCode::FAILURE)
    }
}

/// Whether the provenance check judges `path`: a `.rs` file under `crates/` or a `.py` file
/// under `standin/`. As in [`violations`], directory and extension are matched on the
/// ASCII-lowercased path, so a case variant (`Crates/x.RS`) is judged, never exempt.
pub fn provenance_in_scope(path: &str) -> bool {
    let folded = path.to_ascii_lowercase();
    (folded.starts_with("crates/") && folded.ends_with(".rs"))
        || (folded.starts_with("standin/") && folded.ends_with(".py"))
}

/// The source trees the provenance check guards, as ASCII-lowercase directory names.
const CHECKED_TREES: [&str; 2] = ["crates", "standin"];

/// Whether `path` is a checked tree itself or lies inside one (case-insensitive, as
/// [`provenance_in_scope`]): where a symbolic link is refused.
pub fn in_checked_tree(path: &str) -> bool {
    let folded = path.to_ascii_lowercase();
    CHECKED_TREES.iter().any(|tree| {
        folded == *tree
            || folded
                .strip_prefix(tree)
                .is_some_and(|rest| rest.starts_with('/'))
    })
}

/// One entry of a checked source tree, as the provenance check sees it.
pub enum Entry {
    /// The bytes of a regular in-scope file.
    Bytes(Vec<u8>),
    /// A symbolic link (to a file or a directory): refused, never followed.
    Symlink,
}

/// The provenance violations among `(path, source)` pairs, judged against the NOTICE text,
/// as `<path>: <reason>` lines sorted by path (each path judged once). A [`Entry::Symlink`] is
/// always a violation.
///
/// The header is the first line, or for a `.py` file the second line when the first is a `#!`
/// shebang or a PEP 263 coding cookie. It is one of, with `//` in Rust and `#` in Python:
/// - `// provenance: original`
/// - `// provenance: port <project>@<tag>:<upstream-path> (<SPDX>)`
/// - `// provenance: paper doi:<doi>`
///
/// A port passes only when the project is in [`PORT_POLICY`] (exact name), the SPDX equals the
/// policy's, and the NOTICE text has a line that is exactly `Project: <project>`.
pub fn provenance_violations(files: &[(String, Entry)], notice: &str) -> Vec<String> {
    let mut sorted: Vec<&(String, Entry)> = files.iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    sorted.dedup_by(|a, b| a.0 == b.0);
    let mut out = Vec::new();
    for (path, source) in sorted {
        let reasons = match source {
            Entry::Symlink => vec![
                "symbolic link inside a checked source tree; provenance is judged on regular \
                 files only, so replace the link with the files it names"
                    .to_owned(),
            ],
            Entry::Bytes(bytes) => header_violations(path, bytes, notice),
        };
        for reason in reasons {
            out.push(format!("{path}: {reason}"));
        }
    }
    out
}

/// One parsed provenance header.
enum Header<'a> {
    Original,
    Paper,
    Port { project: &'a str, spdx: &'a str },
}

/// The reasons one file's header fails, in a fixed order; empty when it passes.
fn header_violations(path: &str, bytes: &[u8], notice: &str) -> Vec<String> {
    let python = path.to_ascii_lowercase().ends_with(".py");
    let comment = if python { "#" } else { "//" };
    let mut lines = bytes.split(|b| *b == b'\n');
    let first = lines.next().unwrap_or_default();
    let line = if python && (first.starts_with(b"#!") || is_coding_cookie(first)) {
        lines.next().unwrap_or_default()
    } else {
        first
    };
    let lead = format!("{comment} provenance:");
    if !line.starts_with(lead.as_bytes()) {
        return vec!["no provenance header".to_owned()];
    }
    let parsed = std::str::from_utf8(line)
        .ok()
        .and_then(|l| l.strip_prefix(&format!("{lead} ")))
        .and_then(parse_header);
    let malformed = || {
        format!(
            "malformed provenance header {:?}",
            String::from_utf8_lossy(line)
        )
    };
    match parsed {
        None => vec![malformed()],
        Some(Header::Original | Header::Paper) => Vec::new(),
        Some(Header::Port { project, spdx }) => {
            let Some((_, required)) = PORT_POLICY.iter().find(|(p, _)| *p == project) else {
                return vec![format!(
                    "port project {project:?} is not in the licence policy"
                )];
            };
            let mut reasons = Vec::new();
            if spdx != *required {
                reasons.push(format!(
                    "port {project} declares {spdx}, the licence policy requires {required}"
                ));
            }
            let block = format!("Project: {project}");
            if !notice.split('\n').any(|l| l == block) {
                reasons.push(format!("port {project} has no {block:?} line in NOTICE"));
            }
            reasons
        }
    }
}

/// The header after `provenance: `, or `None` when it matches none of the three forms.
fn parse_header(body: &str) -> Option<Header<'_>> {
    if body == "original" {
        return Some(Header::Original);
    }
    if let Some(doi) = body.strip_prefix("paper doi:") {
        return is_doi(doi).then_some(Header::Paper);
    }
    let port = body.strip_prefix("port ")?;
    let (source, spdx) = port.rsplit_once(" (")?;
    let spdx = spdx.strip_suffix(')')?;
    let (project, rest) = source.split_once('@')?;
    let (tag, upstream) = rest.split_once(':')?;
    let token =
        |s: &str| !s.is_empty() && !s.chars().any(|c| c.is_whitespace() || c == '(' || c == ')');
    let spdx_ok = !spdx.is_empty() && !spdx.contains(['(', ')']) && spdx.trim() == spdx;
    (token(project) && token(tag) && token(upstream) && spdx_ok)
        .then_some(Header::Port { project, spdx })
}

/// `10.<registrant>/<suffix>`: registrant digits and dots, suffix non-empty with no whitespace
/// or control characters.
fn is_doi(doi: &str) -> bool {
    let Some((prefix, suffix)) = doi.split_once('/') else {
        return false;
    };
    let Some(registrant) = prefix.strip_prefix("10.") else {
        return false;
    };
    !registrant.is_empty()
        && registrant.bytes().all(|b| b.is_ascii_digit() || b == b'.')
        && !suffix.is_empty()
        && !suffix.chars().any(|c| c.is_whitespace() || c.is_control())
}

/// Whether a line is a PEP 263 coding cookie: a comment matching `coding[:=]\s*[-\w.]+`.
fn is_coding_cookie(line: &[u8]) -> bool {
    let text = String::from_utf8_lossy(line);
    let Some(comment) = text
        .trim_start_matches([' ', '\t', '\x0c'])
        .strip_prefix('#')
    else {
        return false;
    };
    comment.match_indices("coding").any(|(i, _)| {
        let after = &comment[i + "coding".len()..];
        after
            .strip_prefix([':', '='])
            .map(|v| v.trim_start_matches([' ', '\t']))
            .and_then(|v| v.chars().next())
            .is_some_and(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.')
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A NOTICE with one `Project: sympy` block.
    const NOTICE_SYMPY: &str = "klein-science\n\nPorted code notices\n\nProject: sympy\nCopyright (c) 2006-2023 SymPy Development Team\n";
    /// A NOTICE with no Project blocks.
    const NOTICE_EMPTY: &str = "klein-science\n\nPorted code notices\n";

    fn judge(files: &[(&str, &str)], notice: &str) -> Vec<String> {
        let files: Vec<(String, Entry)> = files
            .iter()
            .map(|(p, c)| ((*p).to_owned(), Entry::Bytes(c.as_bytes().to_vec())))
            .collect();
        provenance_violations(&files, notice)
    }

    #[test]
    fn provenance_checked_trees() {
        for inside in ["crates", "crates/k/src/imported", "Standin", "standin/a"] {
            assert!(in_checked_tree(inside), "{inside}");
        }
        for outside in [
            "cratesx/a.rs",
            "standins",
            "xtask/src/main.rs",
            "review-fixture/a",
        ] {
            assert!(!in_checked_tree(outside), "{outside}");
        }
    }

    const SYMPY_PORT_PY: &str =
        "# provenance: port sympy@sympy-1.14.0:sympy/__init__.py (BSD-3-Clause)\nimport x\n";

    #[test]
    fn provenance_original_ok() {
        let tree = [
            (
                "crates/a/src/lib.rs",
                "// provenance: original\n//! Docs.\n",
            ),
            ("standin/a.py", "# provenance: original\nimport x\n"),
            (
                "standin/run.py",
                "#!/usr/bin/env python3\n# provenance: original\n",
            ),
            (
                "standin/enc.py",
                "# -*- coding: utf-8 -*-\n# provenance: original\n",
            ),
            ("crates/a/src/one.rs", "// provenance: original"),
        ];
        assert_eq!(judge(&tree, NOTICE_EMPTY), Vec::<String>::new());
    }

    #[test]
    fn provenance_missing() {
        let tree = [
            ("crates/a/src/lib.rs", "//! Docs.\npub fn f() {}\n"),
            ("crates/a/src/empty.rs", ""),
            ("standin/empty.py", ""),
            // The header on line 2 counts only after a shebang or coding cookie, and only in Python.
            ("crates/a/src/late.rs", "\n// provenance: original\n"),
            ("standin/late.py", "import x\n# provenance: original\n"),
        ];
        assert_eq!(
            judge(&tree, NOTICE_EMPTY),
            [
                "crates/a/src/empty.rs: no provenance header",
                "crates/a/src/late.rs: no provenance header",
                "crates/a/src/lib.rs: no provenance header",
                "standin/empty.py: no provenance header",
                "standin/late.py: no provenance header",
            ]
        );
    }

    #[test]
    fn provenance_port_ok() {
        let tree = [
            ("standin/names.py", SYMPY_PORT_PY),
            (
                "crates/a/src/poly.rs",
                "// provenance: port sympy@sympy-1.14.0:sympy/polys/densearith.py (BSD-3-Clause)\n",
            ),
        ];
        assert_eq!(judge(&tree, NOTICE_SYMPY), Vec::<String>::new());
    }

    #[test]
    fn provenance_port_not_in_notice() {
        let tree = [("standin/names.py", SYMPY_PORT_PY)];
        let expected = [r#"standin/names.py: port sympy has no "Project: sympy" line in NOTICE"#];
        assert_eq!(judge(&tree, NOTICE_EMPTY), expected);
        // A near-miss in NOTICE is not the line.
        for notice in [
            "Project: sympyy\n",
            "Project: SymPy\n",
            " Project: sympy\n",
            "Project: sympy \n",
            "Project:sympy\n",
        ] {
            assert_eq!(judge(&tree, notice), expected, "NOTICE {notice:?}");
        }
    }

    #[test]
    fn provenance_port_not_allowed() {
        let tree = [
            (
                "crates/a/src/a.rs",
                "// provenance: port symbolica@v1:src/lib.rs (MIT)\n",
            ),
            (
                "crates/a/src/b.rs",
                "// provenance: port flint@v3.0.0:src/fmpz.c (LGPL-3.0-or-later)\n",
            ),
            (
                "crates/a/src/c.rs",
                "// provenance: port giac@1.9:src/giac.cc (GPL-3.0-or-later)\n",
            ),
        ];
        let notice = "Project: symbolica\nProject: flint\nProject: giac\n";
        assert_eq!(
            judge(&tree, notice),
            [
                r#"crates/a/src/a.rs: port project "symbolica" is not in the licence policy"#,
                r#"crates/a/src/b.rs: port project "flint" is not in the licence policy"#,
                r#"crates/a/src/c.rs: port project "giac" is not in the licence policy"#,
            ]
        );
    }

    #[test]
    fn provenance_spdx_mismatch() {
        let tree = [
            (
                "standin/a.py",
                "# provenance: port sympy@sympy-1.14.0:sympy/core/add.py (MIT)\n",
            ),
            (
                "crates/a/src/a.rs",
                "// provenance: port symengine@v0.14.0:symengine/add.cpp (BSD-3-Clause)\n",
            ),
        ];
        let notice = "Project: sympy\nProject: symengine\n";
        assert_eq!(
            judge(&tree, notice),
            [
                "crates/a/src/a.rs: port symengine declares BSD-3-Clause, the licence policy requires MIT",
                "standin/a.py: port sympy declares MIT, the licence policy requires BSD-3-Clause",
            ]
        );
    }

    #[test]
    fn provenance_adjacent_name() {
        let tree = [
            (
                "standin/a.py",
                "# provenance: port sympyy@sympy-1.14.0:sympy/core/add.py (BSD-3-Clause)\n",
            ),
            (
                "standin/b.py",
                "# provenance: port SymPy@sympy-1.14.0:sympy/core/add.py (BSD-3-Clause)\n",
            ),
        ];
        let notice = "Project: sympy\nProject: sympyy\nProject: SymPy\n";
        assert_eq!(
            judge(&tree, notice),
            [
                r#"standin/a.py: port project "sympyy" is not in the licence policy"#,
                r#"standin/b.py: port project "SymPy" is not in the licence policy"#,
            ]
        );
    }

    #[test]
    fn provenance_paper_ok() {
        let tree = [
            (
                "crates/a/src/a.rs",
                "// provenance: paper doi:10.1145/1234\n",
            ),
            (
                "standin/a.py",
                "# provenance: paper doi:10.1006/jsco.1996.0052\n",
            ),
        ];
        assert_eq!(judge(&tree, NOTICE_EMPTY), Vec::<String>::new());
    }

    #[test]
    fn provenance_sorted() {
        let tree = [
            ("crates/a/src/b.rs", "pub fn b() {}\n"),
            ("crates/a/src/a.rs", "pub fn a() {}\n"),
        ];
        assert_eq!(
            judge(&tree, NOTICE_EMPTY),
            [
                "crates/a/src/a.rs: no provenance header",
                "crates/a/src/b.rs: no provenance header",
            ]
        );
    }

    #[test]
    fn provenance_malformed() {
        let tree = [
            ("crates/a/src/a.rs", "// provenance: originall\n"),
            ("crates/a/src/b.rs", "// provenance: original \n"),
            ("crates/a/src/c.rs", "// provenance: paper doi:\n"),
            ("crates/a/src/d.rs", "// provenance: paper 10.1145/1234\n"),
            (
                "crates/a/src/e.rs",
                "// provenance: port sympy@sympy-1.14.0 (BSD-3-Clause)\n",
            ),
            (
                "crates/a/src/f.rs",
                "// provenance: port sympy:sympy/a.py (BSD-3-Clause)\n",
            ),
            (
                "crates/a/src/g.rs",
                "// provenance: port sympy@sympy-1.14.0:sympy/a.py BSD-3-Clause\n",
            ),
            ("crates/a/src/h.rs", "# provenance: original\n"),
            ("standin/a.py", "// provenance: original\n"),
            ("crates/a/src/i.rs", "// provenance: original\r\n"),
        ];
        let found = judge(&tree, NOTICE_SYMPY);
        let paths: Vec<&str> = found
            .iter()
            .map(|v| v.split_once(": ").map_or(v.as_str(), |(p, _)| p))
            .collect();
        assert_eq!(
            paths,
            [
                "crates/a/src/a.rs",
                "crates/a/src/b.rs",
                "crates/a/src/c.rs",
                "crates/a/src/d.rs",
                "crates/a/src/e.rs",
                "crates/a/src/f.rs",
                "crates/a/src/g.rs",
                "crates/a/src/h.rs",
                "crates/a/src/i.rs",
                "standin/a.py",
            ]
        );
        assert_eq!(
            found[0],
            r#"crates/a/src/a.rs: malformed provenance header "// provenance: originall""#
        );
        // A Rust file with a Python-style header has no header at all.
        assert_eq!(found[7], "crates/a/src/h.rs: no provenance header");
    }

    #[test]
    fn provenance_scope() {
        assert!(provenance_in_scope("crates/ksci-wire/src/lib.rs"));
        assert!(provenance_in_scope("crates/ksci-wire/tests/contract.rs"));
        assert!(provenance_in_scope("crates/x/src/Upper.RS"));
        assert!(provenance_in_scope("standin/names.py"));
        assert!(provenance_in_scope("standin/pkg/sub/mod.py"));
        // A case variant of a scope directory is judged, never exempt.
        assert!(provenance_in_scope("Crates/x/src/a.rs"));
        assert!(provenance_in_scope("STANDIN/a.Py"));
        assert!(!provenance_in_scope("xtask/src/main.rs"));
        assert!(!provenance_in_scope("crates/ksci-wire/Cargo.toml"));
        assert!(!provenance_in_scope("crates/x/tool.py"));
        assert!(!provenance_in_scope("standin/x.rs"));
        assert!(!provenance_in_scope("docs/crates/x.rs"));
    }

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
