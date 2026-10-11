// provenance: original
//! The files rustc read, from cargo dep-info (review W3', operator decision 2026-10-11).
//!
//! [`compile_inputs`] runs one `cargo check --workspace --all-targets --all-features --locked`
//! of the checkout with the pinned cargo, into a dedicated target directory, and reads the
//! rustc dep-info (`.d`) file of every compiler artifact that compile reports, fresh or rebuilt,
//! including dependencies, proc macros and build scripts. The union of their inputs is what
//! rustc read: modules, `#[path]` modules, `include!`, `include_str!` and `include_bytes!` files,
//! wherever they live and however they were reached.
//!
//! Only the artifacts of this compile are read, never a directory listing, so a stale `.d` file
//! left in the target directory by an earlier compile is never judged.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use serde_json::Value;

use crate::Res;
use crate::host;

/// Compiles the workspace at `root` into `target_dir` and returns every input path rustc
/// recorded, as written in the dep-info (relative paths are relative to `root`, where cargo runs
/// rustc for workspace members), sorted and deduplicated. A failed compile is an error.
pub fn compile_inputs(root: &Path, target_dir: &Path) -> Res<Vec<PathBuf>> {
    let cargo = host::cargo()?;
    let out = cargo
        .command()
        .args([
            "check",
            "--workspace",
            "--all-targets",
            "--all-features",
            "--locked",
            "--message-format=json-render-diagnostics",
            "--target-dir",
        ])
        .arg(target_dir)
        .current_dir(root)
        .stdin(Stdio::null())
        .stderr(Stdio::inherit())
        .output()
        .map_err(|e| format!("run {} check: {e}", cargo.path.display()))?;
    if !out.status.success() {
        return Err(format!(
            "provenance: cargo check failed ({}); the inputs of a compile that fails cannot be judged",
            out.status
        ));
    }
    let mut dep_files = BTreeSet::new();
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let msg: Value = serde_json::from_str(line)
            .map_err(|e| format!("provenance: cargo message {line:?}: {e}"))?;
        if msg.get("reason").and_then(Value::as_str) != Some("compiler-artifact") {
            continue;
        }
        let files = msg
            .get("filenames")
            .and_then(Value::as_array)
            .ok_or_else(|| format!("provenance: compiler-artifact without filenames: {line}"))?;
        for file in files {
            let file = file
                .as_str()
                .ok_or_else(|| format!("provenance: non-string artifact filename in {line}"))?;
            dep_files.insert(dep_info_path(Path::new(file))?);
        }
    }
    if dep_files.is_empty() {
        return Err("provenance: cargo check reported no compiler artifacts".to_owned());
    }
    let mut inputs = BTreeSet::new();
    for dep in &dep_files {
        let text = fs::read_to_string(dep)
            .map_err(|e| format!("provenance: dep-info {}: {e}", dep.display()))?;
        inputs.extend(
            parse_dep_info(&text).map_err(|e| format!("provenance: {}: {e}", dep.display()))?,
        );
    }
    Ok(inputs.into_iter().collect())
}

/// The rustc dep-info file of one artifact cargo reported. rustc writes `<crate>-<hash>.d` next
/// to `lib<crate>-<hash>.rmeta` (or `.rlib`, `.dylib`, a binary); cargo uplifts a build script
/// as `build/<pkg>-<hash>/build-script-<name>`, next to `build_script_<name>-<hash>.d`. A
/// missing file is an error: an artifact whose inputs cannot be read cannot be judged.
pub fn dep_info_path(artifact: &Path) -> Res<PathBuf> {
    let shown = artifact.display();
    let name = artifact
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| format!("provenance: artifact {shown} has no UTF-8 file name"))?;
    let dir = artifact
        .parent()
        .ok_or_else(|| format!("provenance: artifact {shown} has no directory"))?;
    let dep = if let Some(script) = name.strip_prefix("build-script-") {
        let unit = dir
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| format!("provenance: build script {shown} has no unit directory"))?;
        let (_, hash) = unit
            .rsplit_once('-')
            .ok_or_else(|| format!("provenance: build script directory {unit:?} has no hash"))?;
        dir.join(format!(
            "build_script_{}-{hash}.d",
            script.replace('-', "_")
        ))
    } else {
        let stem = name.split_once('.').map_or(name, |(s, _)| s);
        let stem = stem.strip_prefix("lib").unwrap_or(stem);
        dir.join(format!("{stem}.d"))
    };
    if dep.is_file() {
        Ok(dep)
    } else {
        Err(format!(
            "provenance: no dep-info {} for artifact {shown}",
            dep.display()
        ))
    }
}

/// Every prerequisite path of a rustc dep-info file.
///
/// rustc writes a small, fixed subset of Makefile syntax, and only that subset is accepted (review
/// W2): empty lines, `#` comment lines (`# env-dep:`, `# checksum:`), and single-target rules
/// `<target>: <input> <input>...` whose words are separated by single spaces, with a space inside
/// a path written `\ ` (the only escape rustc emits). Anything else is an error rather than a
/// guess: a second target or any other `:` in a word, a continuation backslash, any other `\`
/// escape, a tab (recipe lines, tab separators) or a carriage return.
pub fn parse_dep_info(text: &str) -> Res<Vec<PathBuf>> {
    let mut inputs = Vec::new();
    for line in text.split('\n') {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let bad = |why: &str| Err(format!("unrecognised dep-info line {line:?}: {why}"));
        if line.contains(['\t', '\r']) {
            return bad("tab or carriage return");
        }
        if line.ends_with('\\') {
            return bad("continuation backslash");
        }
        let mut words = split_words(line)
            .map_err(|why| format!("unrecognised dep-info line {line:?}: {why}"))?
            .into_iter();
        let target = words.next().unwrap_or_default();
        match target.strip_suffix(':') {
            Some(name) if !name.is_empty() && !name.contains(':') => {}
            _ => return bad("not a single-target rule `<target>: <inputs>`"),
        }
        for word in words {
            if word.contains(':') {
                return bad("a second `:` (multi-target rule or a path with a colon)");
            }
            inputs.push(PathBuf::from(word));
        }
    }
    Ok(inputs)
}

/// Splits a dep-info line at spaces; `\ ` is a space inside a path, and any other backslash is
/// an error.
fn split_words(line: &str) -> Result<Vec<String>, String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next() {
                Some(' ') => word.push(' '),
                other => return Err(format!("unrecognised escape `\\{}`", other.unwrap_or(' '))),
            },
            ' ' => {
                if !word.is_empty() {
                    words.push(std::mem::take(&mut word));
                }
            }
            _ => word.push(c),
        }
    }
    if !word.is_empty() {
        words.push(word);
    }
    Ok(words)
}

/// Checks that one artifact's dep-info inputs (as written, relative to `root`) include the
/// artifact's own target source `src` (cargo's `src_path`). (RED stub: accepts anything.)
pub fn require_source(_dep: &Path, _inputs: &[PathBuf], _root: &Path, _src: &Path) -> Res<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rejects(text: &str) {
        assert!(
            parse_dep_info(text).is_err(),
            "accepted {text:?}: {:?}",
            parse_dep_info(text)
        );
    }

    #[test]
    fn depinfo_rejects_multi_target_rule() {
        rejects("a b: c\n");
    }

    #[test]
    fn depinfo_rejects_second_colon_word() {
        rejects("a: b: c\n");
    }

    #[test]
    fn depinfo_rejects_continuation_backslash() {
        rejects("a: b \\\nc:\n");
    }

    #[test]
    fn depinfo_rejects_unknown_escape() {
        rejects("a: b\\#c\n");
    }

    #[test]
    fn depinfo_rejects_escaped_backslash() {
        rejects("a: b\\\\c\n");
    }

    #[test]
    fn depinfo_rejects_recipe_line() {
        rejects("a: b\n\tcmd\n");
    }

    #[test]
    fn depinfo_rejects_tab_separator() {
        rejects("a: b\tc\n");
    }

    #[test]
    fn depinfo_rejects_line_without_rule() {
        rejects("no rule here\n");
    }

    #[test]
    fn depinfo_rejects_empty_target() {
        rejects(": b\n");
    }

    #[test]
    fn depinfo_requires_the_artifact_source() {
        let dir = std::env::temp_dir().join(format!("ksci-xtask-depsrc-{}", std::process::id()));
        fs::create_dir_all(dir.join("src")).unwrap();
        fs::write(dir.join("src/lib.rs"), "").unwrap();
        fs::write(dir.join("src/other.rs"), "").unwrap();
        let src = dir.join("src/lib.rs");
        let dep = Path::new("k-1.d");
        assert!(require_source(dep, &[PathBuf::from("src/lib.rs")], &dir, &src).is_ok());
        assert!(require_source(dep, &[src.clone()], &dir, &src).is_ok());
        // An empty (or comment-only, or phony-only) dep-info lists nothing.
        assert!(require_source(dep, &[], &dir, &src).is_err());
        assert!(require_source(dep, &[PathBuf::from("src/other.rs")], &dir, &src).is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn depinfo_parses_rules_comments_and_escaped_spaces() {
        let text = "/t/deps/k-1.d: crates/k/src/lib.rs crates/k/src/a\\ b.rs /reg/x.rs\n\n\
                    /t/deps/libk-1.rmeta: crates/k/src/lib.rs\n\n\
                    crates/k/src/lib.rs:\n\
                    # env-dep:CARGO_PKG_NAME=k\n";
        let got = parse_dep_info(text).unwrap();
        let want: Vec<PathBuf> = [
            "crates/k/src/lib.rs",
            "crates/k/src/a b.rs",
            "/reg/x.rs",
            "crates/k/src/lib.rs",
        ]
        .iter()
        .map(PathBuf::from)
        .collect();
        assert_eq!(got, want);
        assert!(parse_dep_info("not a rule\n").is_err());
    }

    #[test]
    fn depinfo_path_of_each_artifact_kind() {
        let dir = std::env::temp_dir().join(format!("ksci-xtask-depinfo-{}", std::process::id()));
        let unit = dir.join("build/libc-0123abcd");
        fs::create_dir_all(&unit).unwrap();
        for f in [
            "ksci_wire-11.d",
            "libc-22.d",
            "thiserror_impl-33.d",
            "contract-44.d",
        ] {
            fs::write(dir.join(f), "").unwrap();
        }
        fs::write(unit.join("build_script_build-0123abcd.d"), "").unwrap();
        for (artifact, dep) in [
            ("libksci_wire-11.rmeta", "ksci_wire-11.d"),
            ("liblibc-22.rmeta", "libc-22.d"),
            ("libthiserror_impl-33.dylib", "thiserror_impl-33.d"),
            ("libcontract-44.rmeta", "contract-44.d"),
        ] {
            assert_eq!(dep_info_path(&dir.join(artifact)).unwrap(), dir.join(dep));
        }
        assert_eq!(
            dep_info_path(&unit.join("build-script-build")).unwrap(),
            unit.join("build_script_build-0123abcd.d")
        );
        assert!(dep_info_path(&dir.join("libmissing-55.rmeta")).is_err());
        let _ = fs::remove_dir_all(&dir);
    }
}
