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
//! `Project:` block in NOTICE), or a paper by DOI. On the Rust side it judges what rustc actually
//! read: the inputs cargo dep-info records for two compiles, the host compile of the whole
//! workspace (all targets, all features) and the wasm32-wasip2 compile of the declared R0 crates'
//! libs, resolved to real paths ([`depinfo`], review W3'). On the Python side it walks
//! `standin/` on disk, ignored files included, and refuses symbolic links there.
//!
//! # Threat model of `check provenance`
//!
//! **Guaranteed.** For the tree as compiled during the check, every file rustc reads in those two
//! compiles is, by its content at the time it was judged (right after the compile that read it):
//! - a regular `.rs` file in the repository whose first line is a valid provenance header
//!   (`original`, `paper doi:…`, or a `port` of a project in [`PORT_POLICY`] with its SPDX and a
//!   `Project:` block in NOTICE); or
//! - non-Rust data under a `testdata/` directory of the repository; or
//! - a gated dependency's file: registry sources, git checkouts, the pinned toolchain's sysroot,
//!   or the `OUT_DIR` output of a registry or git package's build script, exactly as cargo
//!   reported it, with no symbolic link from the repository down to it. Those dependencies' licences
//!   are gated by cargo-deny, not by this check.
//!
//! Anything else (a headerless or malformed source, a file outside those places, workspace
//! build-script output, an unresolvable input, an incomplete or unrecognised dep-info file, a
//! failed compile) fails the check. Every `.py` file under `standin/` has a valid header, and
//! `standin/` holds no symbolic link.
//!
//! **Out of scope.**
//! - Doctests: `cargo check --all-targets` does not compile them, and they are never shipped.
//! - `include!` of a file under `testdata/`: dep-info does not record which macro read a file,
//!   and testdata inputs are data, not judged as source.
//! - Python outside `standin/`: no compiler records what Python imports.
//! - Targets and cfgs other than the host compile and the wasm32-wasip2 R0 compile.
//! - A local actor changing files after a compile's inputs are judged, or between this check and
//!   a later build: the check judges the tree as compiled during the check, nothing more.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

use clap::Subcommand;

use crate::Res;
use crate::{depinfo, host, workspace};

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

/// The dedicated target directory of the provenance compile, relative to the workspace root:
/// separate from the developer's `target/debug`, so the compile it judges is its own.
const PROVENANCE_TARGET: &str = "target/provenance";

fn provenance() -> Res<ExitCode> {
    let root = workspace::root()?;
    let root = fs::canonicalize(&root).map_err(|e| format!("{}: {e}", root.display()))?;
    let notice = fs::read_to_string(root.join(NOTICE_FILE))
        .map_err(|e| format!("provenance: {NOTICE_FILE}: {e}"))?;
    let (rust, mut found) = rust_compiles(&root, &notice)?;
    let standin = standin_files(&root)?;
    found.extend(violation_lines(&standin, &notice));
    found.sort();
    found.dedup();
    for (_, line) in &found {
        println!("{line}");
    }
    if found.is_empty() {
        let blocks = notice
            .split('\n')
            .filter(|l| l.starts_with("Project: "))
            .count();
        println!(
            "provenance ok, {rust} Rust sources rustc read (host, all targets; wasm32-wasip2, R0 \
             libs) and {} standin files judged in {}, {NOTICE_FILE} has {blocks} Project blocks",
            standin.len(),
            root.display()
        );
        Ok(ExitCode::SUCCESS)
    } else {
        eprintln!(
            "error: provenance: {} violation(s); every file rustc reads inside the repository is a \
             .rs file with a provenance header (or data under testdata/), every other input comes \
             from the cargo registry, git checkouts or the toolchain, and every .py file under \
             standin/ has a header, with no symbolic link in standin/",
            found.len()
        );
        Ok(ExitCode::FAILURE)
    }
}

/// Each judged path's violations as `(path, line)` pairs, so lines from several compiles merge
/// into one list sorted by path.
fn violation_lines(judged: &[Judged], notice: &str) -> Vec<(String, String)> {
    judged
        .iter()
        .flat_map(|j| {
            provenance_violations(std::slice::from_ref(j), notice)
                .into_iter()
                .map(|line| (j.path.clone(), line))
        })
        .collect()
}

/// Whether `rel` lies under a directory named exactly `testdata` (review W5): only ancestor
/// directories count, never the file name itself.
fn under_testdata(rel: &Path) -> bool {
    rel.parent()
        .is_some_and(|dir| dir.components().any(|c| c.as_os_str() == "testdata"))
}

/// The Rust side: two compiles, the host compile of the whole workspace ([`depinfo::HOST`]) and
/// the R0 compile for wasm32-wasip2 ([`depinfo::r0_selection`]), so code behind a wasm-only cfg
/// is read and judged too. Each compile's inputs are classified, read and judged immediately
/// after that compile and before the next one starts (review W-A): a build script that rewrites
/// a source between the two compiles cannot have one compile's bytes judged in place of the
/// other's. Returns the number of distinct Rust sources judged and the violation lines.
fn rust_compiles(root: &Path, notice: &str) -> Res<(usize, Vec<(String, String)>)> {
    let target = root.join(PROVENANCE_TARGET);
    let external = external_roots()?;
    let host: Vec<String> = depinfo::HOST.iter().map(|s| (*s).to_owned()).collect();
    let r0 = depinfo::r0_selection(root)?;
    let mut sources = BTreeSet::new();
    let mut found = Vec::new();
    for selection in [host, r0] {
        let compiled = depinfo::compile_inputs(root, &target, &selection)?;
        let exempt = dependency_roots(root, &target, &compiled.dependency_out_dirs)?;
        let judged = classify_inputs(root, &compiled.inputs, &exempt, &external)?;
        found.extend(violation_lines(&judged, notice));
        sources.extend(judged.into_iter().map(|j| j.path));
    }
    Ok((sources.len(), found))
}

/// Classifies one compile's inputs, each resolved to its real path (links followed: rustc read
/// the target), reading the bytes of every judged source now.
///
/// - Inside the repository, a regular `.rs` file is judged by its header. A non-`.rs` input
///   (`include_str!`/`include_bytes!` data) is allowed, unjudged, only under a `testdata/`
///   directory; anywhere else it is a violation. Nothing is exempt for lying in the provenance
///   target directory (review C1): a file there, including the `OUT_DIR` output of a workspace
///   build script, is judged like any other input; a headed build script does not vouch for the
///   bytes it generates.
/// - The `OUT_DIR` output of a registry or git dependency's build script (`exempt`, from
///   [`dependency_roots`]) is that dependency's own output and is allowed, as its source is
///   (operator decision 2026-10-11).
/// - Outside the repository, an input is allowed only under the cargo registry sources, the cargo
///   git checkouts or the pinned toolchain's sysroot (`external`); anything else is a violation.
///
/// One real file read under several names is judged once, its other names noted.
fn classify_inputs(
    root: &Path,
    inputs: &[PathBuf],
    exempt: &[PathBuf],
    external: &[PathBuf],
) -> Res<Vec<Judged>> {
    let mut by_path: BTreeMap<String, Judged> = BTreeMap::new();
    for input in inputs {
        let written = input.display().to_string();
        let abs = if input.is_absolute() {
            input.clone()
        } else {
            root.join(input)
        };
        let (path, entry) = match fs::canonicalize(&abs) {
            Err(e) => (
                written.clone(),
                Some(Entry::Refused(format!(
                    "compiler input cannot be resolved: {e}"
                ))),
            ),
            Ok(real) if exempt.iter().any(|d| real.starts_with(d)) => continue,
            Ok(real) => match real.strip_prefix(root) {
                Ok(rel) => {
                    let rel = rel.display().to_string();
                    let entry = if !real.is_file() {
                        Some(Entry::Refused(
                            "compiler input is not a regular file".to_owned(),
                        ))
                    } else if rel.to_ascii_lowercase().ends_with(".rs") {
                        let bytes =
                            fs::read(&real).map_err(|e| format!("provenance: {rel}: {e}"))?;
                        Some(Entry::Bytes(bytes))
                    } else if under_testdata(Path::new(&rel)) {
                        None
                    } else {
                        Some(Entry::Refused(
                            "non-Rust compiler input outside testdata/; include_str! and \
                             include_bytes! data lives under a testdata/ directory"
                                .to_owned(),
                        ))
                    };
                    (rel, entry)
                }
                Err(_) if external.iter().any(|e| real.starts_with(e)) => continue,
                Err(_) => (
                    real.display().to_string(),
                    Some(Entry::Refused(
                        "compiler input outside the repository, the cargo registry and git \
                         checkouts, and the toolchain sysroot"
                            .to_owned(),
                    )),
                ),
            },
        };
        let Some(entry) = entry else { continue };
        let judged = by_path.entry(path.clone()).or_insert(Judged {
            path: path.clone(),
            entry,
            read_as: Vec::new(),
        });
        if written != path && !judged.read_as.contains(&written) {
            judged.read_as.push(written);
        }
    }
    Ok(by_path.into_values().collect())
}

/// The exemption roots for dependency build-script output: the `OUT_DIR`s cargo reported for
/// registry or git packages (operator decision 2026-10-11), exactly as cargo reported them,
/// never resolved through a link (review C1').
///
/// Each must be a plain path (no `.` or `..`) strictly below the provenance `target` directory,
/// and every component from the canonical repository `root` down to the `OUT_DIR` itself must be
/// a real directory, not a symbolic link; otherwise it is an error naming the offending path.
/// Under those conditions the reported path is its own real path, so comparing it with the real
/// path of each compiler input cannot exempt anything outside that one directory.
pub fn dependency_roots(root: &Path, target: &Path, reported: &[PathBuf]) -> Res<Vec<PathBuf>> {
    let mut roots = Vec::new();
    for dir in reported {
        let shown = dir.display();
        let below = dir
            .strip_prefix(target)
            .ok()
            .filter(|rel| {
                !rel.as_os_str().is_empty()
                    && rel
                        .components()
                        .all(|c| matches!(c, std::path::Component::Normal(_)))
            })
            .ok_or_else(|| {
                format!(
                    "provenance: dependency OUT_DIR {shown} is not a plain path below the \
                     provenance target directory {}",
                    target.display()
                )
            })?;
        let from_root = target
            .strip_prefix(root)
            .map_err(|_| {
                format!(
                    "provenance: {} is not below {}",
                    target.display(),
                    root.display()
                )
            })?
            .join(below);
        let mut at = root.to_path_buf();
        for part in from_root.components() {
            at.push(part);
            let meta = fs::symlink_metadata(&at)
                .map_err(|e| format!("provenance: {}: {e}", at.display()))?;
            if meta.file_type().is_symlink() {
                return Err(format!(
                    "provenance: {} is a symbolic link; a dependency OUT_DIR and every directory \
                     above it up to the repository must be real directories (review C1')",
                    at.display()
                ));
            }
            if !meta.is_dir() {
                return Err(format!("provenance: {} is not a directory", at.display()));
            }
        }
        roots.push(dir.clone());
    }
    Ok(roots)
}

/// Where a compiler input outside the repository may come from: the cargo registry sources and
/// git checkouts under `$CARGO_HOME`, and the pinned toolchain's sysroot. Each is canonicalised;
/// one that does not exist admits nothing.
fn external_roots() -> Res<Vec<PathBuf>> {
    let bin = host::bin_dir()?;
    let home = bin
        .parent()
        .ok_or_else(|| format!("{} has no parent", bin.display()))?;
    let rustc = host::toolchain_bin("rustc")?;
    let out = Command::new(&rustc)
        .args(["--print", "sysroot"])
        .output()
        .map_err(|e| format!("run {}: {e}", rustc.display()))?;
    if !out.status.success() {
        return Err(format!(
            "{} --print sysroot: {}",
            rustc.display(),
            out.status
        ));
    }
    let sysroot = PathBuf::from(String::from_utf8_lossy(&out.stdout).trim());
    Ok([
        home.join("registry").join("src"),
        home.join("git").join("checkouts"),
        sysroot,
    ]
    .iter()
    .filter_map(|p| fs::canonicalize(p).ok())
    .collect())
}

/// The Python side: every `.py` file under `standin/`, found by walking the directory itself
/// (ignored files included; no git listing), in name order. A symbolic link anywhere in the
/// tree, `standin` itself included, is refused and never followed.
fn standin_files(root: &Path) -> Res<Vec<Judged>> {
    let mut out = Vec::new();
    walk_standin(root, "standin", &mut out)?;
    Ok(out)
}

fn walk_standin(root: &Path, rel: &str, out: &mut Vec<Judged>) -> Res<()> {
    let path = root.join(rel);
    let meta = match fs::symlink_metadata(&path) {
        Err(e) if rel == "standin" && e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(format!("provenance: {rel}: {e}")),
        Ok(meta) => meta,
    };
    let refuse = |reason: &str| Judged {
        path: rel.to_owned(),
        entry: Entry::Refused(reason.to_owned()),
        read_as: Vec::new(),
    };
    if meta.is_symlink() {
        out.push(refuse(
            "symbolic link inside standin/; provenance is judged on regular files only, so \
             replace the link with the files it names",
        ));
    } else if meta.is_dir() {
        let mut names = Vec::new();
        for item in fs::read_dir(&path).map_err(|e| format!("provenance: {rel}: {e}"))? {
            let item = item.map_err(|e| format!("provenance: {rel}: {e}"))?;
            let name = item.file_name().into_string().map_err(|n| {
                format!(
                    "provenance: {rel}/{}: name is not UTF-8",
                    n.to_string_lossy()
                )
            })?;
            names.push(name);
        }
        names.sort();
        for name in names {
            walk_standin(root, &format!("{rel}/{name}"), out)?;
        }
    } else if rel.to_ascii_lowercase().ends_with(".py") {
        let bytes = fs::read(&path).map_err(|e| format!("provenance: {rel}: {e}"))?;
        out.push(Judged {
            path: rel.to_owned(),
            entry: Entry::Bytes(bytes),
            read_as: Vec::new(),
        });
    }
    Ok(())
}

/// What the provenance check found at one path.
pub enum Entry {
    /// The bytes of a source file, judged by its header.
    Bytes(Vec<u8>),
    /// Refused outright, for this reason.
    Refused(String),
}

/// One judged path: repository-relative for a file in the repository, absolute otherwise.
pub struct Judged {
    pub path: String,
    pub entry: Entry,
    /// The other names rustc read this file under (through links or `..`), noted in the report.
    pub read_as: Vec<String>,
}

/// The provenance violations among judged paths, against the NOTICE text, as `<path>: <reason>`
/// lines sorted by path (each path judged once). An [`Entry::Refused`] is always a violation.
///
/// The header is the first line, or for a `.py` file the second line when the first is a `#!`
/// shebang or a PEP 263 coding cookie. It is one of, with `//` in Rust and `#` in Python:
/// - `// provenance: original`
/// - `// provenance: port <project>@<tag>:<upstream-path> (<SPDX>)`
/// - `// provenance: paper doi:<doi>`
///
/// A port passes only when the project is in [`PORT_POLICY`] (exact name), the SPDX equals the
/// policy's, and the NOTICE text has a line that is exactly `Project: <project>`.
pub fn provenance_violations(judged: &[Judged], notice: &str) -> Vec<String> {
    let mut sorted: Vec<&Judged> = judged.iter().collect();
    sorted.sort_by(|a, b| a.path.cmp(&b.path));
    sorted.dedup_by(|a, b| a.path == b.path);
    let mut out = Vec::new();
    for j in sorted {
        let reasons = match &j.entry {
            Entry::Refused(reason) => vec![reason.clone()],
            Entry::Bytes(bytes) => header_violations(&j.path, bytes, notice),
        };
        let note = if j.read_as.is_empty() {
            String::new()
        } else {
            format!(" (rustc read it as {})", j.read_as.join(", "))
        };
        for reason in reasons {
            out.push(format!("{}: {reason}{note}", j.path));
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
        let judged: Vec<Judged> = files
            .iter()
            .map(|(p, c)| Judged {
                path: (*p).to_owned(),
                entry: Entry::Bytes(c.as_bytes().to_vec()),
                read_as: Vec::new(),
            })
            .collect();
        provenance_violations(&judged, notice)
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

    /// A scratch repository root with `target/provenance/debug/build/dep-1/out` (real dirs).
    fn exemption_tree(name: &str) -> (PathBuf, PathBuf, PathBuf) {
        let base =
            std::env::temp_dir().join(format!("ksci-xtask-exempt-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        let root = fs::canonicalize(&base).unwrap();
        let target = root.join("target/provenance");
        let out = target.join("debug/build/dep-1/out");
        fs::create_dir_all(&out).unwrap();
        (root, target, out)
    }

    #[test]
    fn provenance_exemption_root_is_cargo_out_dir() {
        let (root, target, out) = exemption_tree("plain");
        assert_eq!(
            dependency_roots(&root, &target, std::slice::from_ref(&out)).unwrap(),
            std::slice::from_ref(&out)
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn provenance_exemption_refuses_linked_out_dir() {
        // Review C1': an OUT_DIR swapped for a link to the repository root must not make the
        // whole repository exempt.
        let (root, target, out) = exemption_tree("linked-out");
        fs::remove_dir(&out).unwrap();
        std::os::unix::fs::symlink(&root, &out).unwrap();
        let err = dependency_roots(&root, &target, std::slice::from_ref(&out)).unwrap_err();
        assert!(err.contains("symbolic link"), "{err}");
        assert!(err.contains("debug/build/dep-1/out"), "{err}");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn provenance_exemption_refuses_linked_ancestor() {
        let (root, target, out) = exemption_tree("linked-ancestor");
        let build = target.join("debug/build");
        fs::rename(&build, root.join("elsewhere")).unwrap();
        std::os::unix::fs::symlink(root.join("elsewhere"), &build).unwrap();
        let err = dependency_roots(&root, &target, std::slice::from_ref(&out)).unwrap_err();
        assert!(err.contains("symbolic link"), "{err}");
        assert!(err.contains("debug/build"), "{err}");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn provenance_exemption_refuses_out_dir_outside_target() {
        let (root, target, _) = exemption_tree("outside");
        let stray = root.join("crates/k/out");
        fs::create_dir_all(&stray).unwrap();
        assert!(dependency_roots(&root, &target, &[stray]).is_err());
        let dotted = target.join("debug/build/dep-1/out/../../../../..");
        assert!(dependency_roots(&root, &target, &[dotted]).is_err());
        let _ = fs::remove_dir_all(&root);
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
