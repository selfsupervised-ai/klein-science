// provenance: original
//! RFC 8785 published test vectors (Cyberphone json-canonicalization at commit
//! 19d51d7fe467d4706a3ff08adf8a748f29fc21e0) against `ksci_wire::to_jcs` (DET-01).
//!
//! The vector bytes are never in git (rule 6). `cargo xtask store fetch
//! conformance/corpora/rfc8785.toml` fetches them by recipe, re-hashes each against its pinned
//! digest, and materialises them under `$KSCI_CORPORA/rfc8785` (default `<target>/corpora/rfc8785`).
//!
//! Upstream layout, checked when the recipe was written:
//! - `testdata/output/*.json` carry no trailing newline; the test still strips at most one.
//! - `testdata/outhex/*.txt` hold lowercase hex bytes separated by single spaces, 32 bytes per
//!   line, every line ending in `\n`.

use std::path::PathBuf;

const FETCH: &str = "run: cargo xtask store fetch conformance/corpora/rfc8785.toml";

/// `$KSCI_CORPORA`, else `$CARGO_TARGET_DIR/corpora`, else `<workspace>/target/corpora`: the same
/// rule `cargo xtask store fetch` uses.
fn corpus_dir() -> PathBuf {
    let corpora = if let Some(dir) = std::env::var_os("KSCI_CORPORA") {
        PathBuf::from(dir)
    } else if let Some(dir) = std::env::var_os("CARGO_TARGET_DIR") {
        PathBuf::from(dir).join("corpora")
    } else {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/corpora")
    };
    corpora.join("rfc8785").join("testdata")
}

fn read(kind: &str, file: &str) -> Vec<u8> {
    let path = corpus_dir().join(kind).join(file);
    match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(e) => panic!("missing RFC 8785 vector {} ({e}); {FETCH}", path.display()),
    }
}

/// The outhex rendering: lowercase hex, single spaces, 32 bytes per line, each line ending in `\n`.
fn outhex(bytes: &[u8]) -> String {
    let mut out = String::new();
    for line in bytes.chunks(32) {
        let hex: Vec<String> = line.iter().map(|b| format!("{b:02x}")).collect();
        out.push_str(&hex.join(" "));
        out.push('\n');
    }
    out
}

/// Checks one vector: JCS of the input equals the output bytes and the outhex rendering.
fn check(name: &str) {
    let input = read("input", &format!("{name}.json"));
    let value: serde_json::Value =
        serde_json::from_slice(&input).unwrap_or_else(|e| panic!("{name}: input is not JSON: {e}"));
    let ours = ksci_wire::to_jcs(&value).unwrap_or_else(|e| panic!("{name}: {e}"));

    let output = read("output", &format!("{name}.json"));
    let expected = output.strip_suffix(b"\n").unwrap_or(&output);
    assert_eq!(
        String::from_utf8_lossy(&ours),
        String::from_utf8_lossy(expected),
        "{name}: JCS output differs from testdata/output"
    );
    assert_eq!(
        ours.as_slice(),
        expected,
        "{name}: JCS bytes differ from testdata/output"
    );

    let hex = read("outhex", &format!("{name}.txt"));
    let hex = String::from_utf8(hex).unwrap_or_else(|e| panic!("{name}: outhex: {e}"));
    assert_eq!(
        outhex(&ours),
        hex,
        "{name}: JCS bytes differ from testdata/outhex"
    );
}

#[test]
fn rfc8785_arrays() {
    check("arrays");
}

#[test]
fn rfc8785_french() {
    check("french");
}

#[test]
fn rfc8785_structures() {
    check("structures");
}

#[test]
fn rfc8785_unicode() {
    check("unicode");
}

#[test]
fn rfc8785_values() {
    check("values");
}

#[test]
fn rfc8785_weird() {
    check("weird");
}
