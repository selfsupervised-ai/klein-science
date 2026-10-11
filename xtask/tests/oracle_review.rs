// provenance: original
//! Review witnesses W2 and W3 for the oracle runner (plan 01-05), run through the real hardened
//! container on `testdata/programs/review.jsonl`:
//!
//! - W2: a case killed by a signal that has no `signal.Signals` member must be recorded, and must
//!   not discard the batch.
//! - W3: a child that escapes the case's process group (`os.setsid`) must not outlive the case,
//!   and must not hide the case's exception by holding the exception channel open.
//! - R1: a case must not be able to signal the harness or a sibling case
//!   (`review-r1-parent.jsonl`, `review-r1-sibling.jsonl`).
//! - R2: `ok` needs positive completion evidence; an exception whose reporting runs user code
//!   that exits, raises or loops is still an error (`review-r2.jsonl`).
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

/// The `(status, exception)` of one record, for compact assertions.
fn outcome(records: &BTreeMap<String, Value>, case_id: &str) -> (String, Value) {
    let r = &records[case_id];
    (
        r["status"].as_str().unwrap().to_owned(),
        r["exception"].clone(),
    )
}

fn exception(kind: &str, message: &str) -> Value {
    serde_json::json!({"type": kind, "message": message})
}

/// R1 (a): a case that SIGKILLs its parent (the harness) must fail itself, not the batch.
#[test]
#[ignore = "needs docker and the pinned base image; run with -- --ignored"]
fn oracle_review_case_cannot_kill_the_harness() {
    let records = run_records("testdata/programs/review-r1-parent.jsonl", "1");
    let killer = "91b6193b2c7d31e80bcf12b872b158c830cb1e401f4d5cac0d83d4fab843d9e0";
    let control = "0a6569409ba2ed1338dc180c9123cb6cbfdd97db7f1b5ae9e0e518f0149ca0ac";
    assert_eq!(
        outcome(&records, killer),
        (
            "error".to_owned(),
            exception("PermissionError", "[Errno 1] Operation not permitted")
        )
    );
    assert_eq!(records[control]["stdout"], "after the parent kill\n");
}

/// R1 (b): a case that SIGSTOPs and SIGKILLs every sibling wrapper it finds in /proc must not
/// change their records. Deterministic (review round 3): each victim announces itself in /tmp
/// and waits for the attacker's done-file, and the attacker acts only once it has seen all three,
/// so the victims are alive when it signals. On the pre-fix runner (c61dc69) this gives
/// `ready 3 signalled 6` and three Signal SIGKILL records, run after run.
#[test]
#[ignore = "needs docker and the pinned base image; run with -- --ignored"]
fn oracle_review_case_cannot_signal_siblings() {
    let records = run_records("testdata/programs/review-r1-sibling.jsonl", "4");
    let attacker = "72e2939e8a99cbf8cbe9ed1a6ce2e4de256ae158d93eb6fee0edbaf19d310acf";
    assert_eq!(
        records[attacker]["stdout"], "ready 3 signalled 0\n",
        "{}",
        records[attacker]
    );
    for (case_id, n) in [
        (
            "24d86ea394aee16937bad408c3fd8cd5ab03485c76c7ae6634d2c95075a3c943",
            0,
        ),
        (
            "42918454b9f0fe24f2a31ddab6da5d965a513241a867333613488977b47cbed5",
            1,
        ),
        (
            "b44ebbb6cebe3db7f08d798b28b07ebf6c06144b89d6b8e7cee9e048f15926a4",
            2,
        ),
    ] {
        let r = &records[case_id];
        assert_eq!(r["status"], "ok", "{r}");
        assert_eq!(
            r["stdout"],
            format!("victim {n} saw the attack end\n"),
            "{r}"
        );
    }
}

/// Review round 3: the container's pid budget is an invariant. With `--jobs 32`, 31 cases
/// that each hold 15 sleeping children plus a control that later starts a subprocess
/// exhausted the 512-task limit before the fix: the control failed with EAGAIN, and some hogs
/// got fewer children, depending on timing. Now every hog gets all 15 and the control is ok.
#[test]
#[ignore = "needs docker and the pinned base image; run with -- --ignored"]
fn oracle_review_pid_budget_holds() {
    let records = run_records("testdata/programs/review-pids.jsonl", "32");
    assert_eq!(records.len(), 32);
    let control = "64d83a3e4f562948898f4a31448221a8302bbafafb2775ff0014e61160f6048a";
    assert_eq!(
        (
            records[control]["status"].as_str(),
            records[control]["stdout"].as_str()
        ),
        (Some("ok"), Some("control 42 0\n")),
        "{}",
        records[control]
    );
    for (case_id, r) in &records {
        if case_id != control {
            assert_eq!(r["status"], "ok", "{r}");
            let out = r["stdout"].as_str().unwrap();
            assert!(out.ends_with(" forked 15\n"), "{r}");
        }
    }
}

/// R2: `ok` needs the completion marker, and the reporter never runs user code to name the
/// exception type.
#[test]
#[ignore = "needs docker and the pinned base image; run with -- --ignored"]
fn oracle_review_exception_reporting_cannot_hide_failure() {
    let records = run_records("testdata/programs/review-r2.jsonl", "4");
    let error = |kind: &str, message: &str| ("error".to_owned(), exception(kind, message));
    // __str__ calls os._exit(0) while the exception is reported.
    assert_eq!(
        outcome(
            &records,
            "7928b0453cd075f8e0f9ce094a6606ed913df7bb98c6174e51aae27aaea7e11d"
        ),
        error("Hidden", "<message unavailable>")
    );
    // __str__ raises.
    assert_eq!(
        outcome(
            &records,
            "073edf412e377e30a18c30ba80c215df5f54603f4998e7ef4cf6989cde7eb894"
        ),
        error("Raising", "<unprintable>")
    );
    // __str__ never returns (the CPU limit ends it).
    assert_eq!(
        outcome(
            &records,
            "5c5550ac8ccc4fbbdd9636451b4a51963caeca270a64c2e1c257b1eb2c29ecfe"
        ),
        error("Looping", "<message unavailable>")
    );
    // Reading the class name through the metaclass would run os._exit(0).
    assert_eq!(
        outcome(
            &records,
            "2406d85eb1cfbe89690ca4c0ef5f03ae2dc411be8c47301b039dc73bdd2d53a0"
        ),
        error("Sneaky", "x")
    );
    // A clean exit before the program's end is not a completion.
    let early = "90d0272c325244d942adfb52a6b78b38db031913b35dfa6db48a7ac11eb98079";
    assert_eq!(
        outcome(&records, early),
        error(
            "IncompleteExit",
            "exit status 0 without the completion marker"
        )
    );
    assert_eq!(records[early]["stdout"], "a\n");
    assert_eq!(
        outcome(
            &records,
            "b894d728effef470a6e088ea8b26d2627954c2c42c22d1c4b314813303aa942c"
        ),
        ("ok".to_owned(), Value::Null)
    );
}
