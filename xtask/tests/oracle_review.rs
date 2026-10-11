// provenance: original
//! Review witnesses W2 and W3 for the oracle runner (plan 01-05), run through the real hardened
//! container on `testdata/programs/review.jsonl`:
//!
//! - W2: a case killed by a signal that has no `signal.Signals` member must be recorded, and must
//!   not discard the batch.
//! - W3: a child that escapes the case's process group (`os.setsid`) must not outlive the case,
//!   and must not hide the case's exception by holding the exception channel open.
//!
//! Ignored by default: it needs a running docker daemon and the pinned base image, and it takes
//! the heavy-job lock, which `cargo xtask ci` already holds while it runs the test suite. Run it
//! with `cargo test -p xtask --test oracle_review -- --ignored`.

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

const SIGNAL: &str = "abe29b00d8d415bf35ff136c0fc14329a4b888e0015264fc7a9331dc50c31aaa";
const ESCAPE_RAISE: &str = "5231a540d48679f0b2b07ba104e70c841a758c48c950a0b4b4ab6a7031603be3";
const STRAY_SPAWN: &str = "42099a760be656bf40c0ff9d1e2b038d5a598e69da305aa84a0801aa24880c3c";
const STRAY_CHECK: &str = "34fc6f017b2511fb7c4854b40805437b0406d1ea89a5af4f8fc32387b39dcc7a";
const CONTROL: &str = "d2320929e926a2795564b45ce474d1379a36b32367d056bc565db30a6ba9a382";

fn cache_dir() -> PathBuf {
    match env::var_os("KSCI_CACHE") {
        Some(dir) => PathBuf::from(dir),
        None => PathBuf::from(env::var_os("HOME").expect("HOME"))
            .join(".cache")
            .join("klein-science"),
    }
}

/// Runs `input` through `cargo xtask oracle run` in the pinned base image and returns the
/// stored records by case_id. The run itself must succeed (W2: one case never discards it).
fn run_records(input: &str, jobs: &str) -> BTreeMap<String, Value> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .current_dir(root)
        .args([
            "oracle",
            "run",
            "--image",
            "base",
            "--platform",
            "linux/arm64",
            "--script",
            "conformance/oracles/run_programs.py",
            "--input",
            input,
            "--cpu-seconds",
            "5",
            "--jobs",
            jobs,
        ])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "oracle run failed ({}):\n{stdout}\n{stderr}",
        out.status
    );
    let hex = stdout
        .lines()
        .find_map(|l| l.strip_prefix("records.jsonl sha256="))
        .unwrap_or_else(|| panic!("no records digest in:\n{stdout}"));
    let text = fs::read_to_string(cache_dir().join("blobs").join("sha256").join(hex)).unwrap();
    text.lines()
        .map(|l| {
            let v: Value = serde_json::from_str(l).unwrap();
            (v["case_id"].as_str().unwrap().to_owned(), v)
        })
        .collect()
}

/// The sweep that ends a finished case must never reach into a case that is still running:
/// its child, and its orphaned grandchild (adopted by the case's own wrapper), both survive
/// while 24 other cases finish around them. Seen RED with the wrapper's subreaper disabled:
/// the orphan was killed by another case's sweep and the case printed an empty line.
#[test]
#[ignore = "needs docker and the pinned base image; run with -- --ignored"]
fn oracle_review_sweep_spares_running_cases() {
    let records = run_records("testdata/programs/review-concurrency.jsonl", "4");
    assert_eq!(records.len(), 26);
    assert!(records.values().all(|r| r["status"] == "ok"));
    let child = "d979b41a8fab9a83e5426777c3dedfedc37d0fc7e515846f685327092878e8d4";
    let orphan = "1d69cd63b82b2b067ad869844a3ce4be2d933e514a0fa649736664aaff2c972c";
    assert_eq!(
        records[child]["stdout"], "child done 0\n",
        "{}",
        records[child]
    );
    assert_eq!(
        records[orphan]["stdout"], "orphan done\n",
        "{}",
        records[orphan]
    );
}

#[test]
#[ignore = "needs docker and the pinned base image; run with -- --ignored"]
fn oracle_review_cases_are_recorded_and_contained() {
    // Sequential, so the stray check runs after the stray spawn.
    let records = run_records("testdata/programs/review.jsonl", "1");
    let text = format!("{records:?}");
    assert_eq!(records.len(), 5, "{text}");

    // W2: the real-time signal is named by its number.
    let signal = &records[SIGNAL];
    assert_eq!(signal["status"], "error", "{signal}");
    assert_eq!(signal["exception"]["type"], "Signal", "{signal}");
    let number = signal["exception"]["message"].as_str().unwrap();
    assert!(
        !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit()),
        "{signal}"
    );

    // W3: the exception survives an escaped child holding the channel open.
    let raised = &records[ESCAPE_RAISE];
    assert_eq!(raised["status"], "error", "{raised}");
    assert_eq!(raised["exception"]["type"], "ValueError", "{raised}");
    assert_eq!(raised["exception"]["message"], "escaped", "{raised}");

    // W3: the escaped child of a successful case dies with the case.
    assert_eq!(records[STRAY_SPAWN]["status"], "ok");
    assert_eq!(records[STRAY_SPAWN]["stdout"], "spawned\n");
    assert_eq!(records[STRAY_CHECK]["status"], "ok");
    assert_eq!(
        records[STRAY_CHECK]["stdout"], "stray gone\n",
        "{}",
        records[STRAY_CHECK]
    );

    assert_eq!(records[CONTROL]["stdout"], "control\n");
}
