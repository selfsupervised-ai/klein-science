// provenance: original
//! The R2 worker over real stdio: frames in, frames out, exit codes.

use std::io::{Read, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use ksci_wire::MAX_DOCUMENT_BYTES;
use ksci_wire::frame::{Tag, read_response_frame, write_request_frame};

/// Longest wait for the worker before a test fails instead of hanging.
const PATIENCE: Duration = Duration::from_secs(30);

const X2_PLUS_1: &str =
    r#"{"nodes":[["Sym","x",{}],["Int","2"],["Pow",0,1],["Int","1"],["Add",2,3]],"root":4,"v":1}"#;

fn spawn() -> (Child, ChildStdin, ChildStdout) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_ksci-worker"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the worker starts");
    let stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    (child, stdin, stdout)
}

/// Read one response frame, or kill the worker and fail if none arrives within [`PATIENCE`].
fn next_frame(child: &mut Child, stdout: ChildStdout) -> (ChildStdout, Option<(Tag, Vec<u8>)>) {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut stdout = stdout;
        let frame = read_response_frame(&mut stdout).expect("a well-formed response frame");
        let _ = tx.send((stdout, frame));
    });
    match rx.recv_timeout(PATIENCE) {
        Ok(got) => got,
        Err(_) => {
            let _ = child.kill();
            panic!("the worker sent no frame within {PATIENCE:?}");
        }
    }
}

/// Close stdin, then the worker's exit code, its remaining stdout and its stderr.
///
/// Every wait has the [`PATIENCE`] deadline (review N2): a worker that keeps its output open, or does not exit, after
/// end of input is killed and the test fails instead of hanging.
fn finish(mut child: Child, stdin: ChildStdin, stdout: ChildStdout) -> (i32, Vec<u8>, String) {
    drop(stdin);
    let mut stderr_pipe = child.stderr.take().unwrap();
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut stdout = stdout;
        let mut rest = Vec::new();
        let mut stderr = String::new();
        let read = stdout
            .read_to_end(&mut rest)
            .and_then(|_| stderr_pipe.read_to_string(&mut stderr));
        let _ = tx.send(read.map(|_| (rest, stderr)));
    });
    let (rest, stderr) = match rx.recv_timeout(PATIENCE) {
        Ok(read) => read.expect("the worker's output is readable"),
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            panic!("the worker kept its output open {PATIENCE:?} after end of input");
        }
    };
    let code = exit_within(&mut child)
        .code()
        .expect("the worker exits, not killed");
    assert!(
        !stderr.contains("panicked"),
        "the worker panicked: {stderr}"
    );
    (code, rest, stderr)
}

/// Polling step while waiting for the worker to exit. Waits count steps, so the tests read no clock.
const TICK: Duration = Duration::from_millis(10);

/// The worker's exit status, or kill it and fail if it has not exited within [`PATIENCE`].
fn exit_within(child: &mut Child) -> ExitStatus {
    for _ in 0..PATIENCE.as_millis() / TICK.as_millis() {
        if let Some(status) = child.try_wait().expect("the worker's status is readable") {
            return status;
        }
        thread::sleep(TICK);
    }
    let _ = child.kill();
    let _ = child.wait();
    panic!("the worker did not exit within {PATIENCE:?} of end of input");
}

fn corpus() -> Vec<Vec<u8>> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../testdata/wire/requests.jsonl"
    );
    let bytes = std::fs::read(path).expect("testdata/wire/requests.jsonl is readable");
    bytes[..bytes.len() - 1]
        .split(|b| *b == b'\n')
        .map(<[u8]>::to_vec)
        .collect()
}

#[test]
fn worker_roundtrip_matches_invoke() {
    let (mut child, mut stdin, mut stdout) = spawn();
    let lines = corpus();
    assert!(lines.len() >= 12);
    for (i, line) in lines.iter().enumerate() {
        write_request_frame(&mut stdin, line).unwrap();
        let (out, frame) = next_frame(&mut child, stdout);
        stdout = out;
        let expected = ksci_ops::invoke(line).expect("every corpus line is a valid request");
        assert_eq!(frame, Some((Tag::Ok, expected)), "line {}", i + 1);
    }
    let (code, rest, _) = finish(child, stdin, stdout);
    assert_eq!(
        (code, rest),
        (0, Vec::new()),
        "a clean end of input exits 0"
    );
}

#[test]
fn worker_frame_cap() {
    // One byte over the cap: a bad-request frame naming the limit, before any body byte arrives, then exit 2.
    let (mut child, mut stdin, stdout) = spawn();
    let over = u32::try_from(MAX_DOCUMENT_BYTES + 1).unwrap();
    stdin.write_all(&over.to_be_bytes()).unwrap();
    stdin.flush().unwrap();
    let (stdout, frame) = next_frame(&mut child, stdout);
    let (tag, message) = frame.expect("a response frame");
    assert_eq!(tag, Tag::BadRequest);
    let message = String::from_utf8(message).unwrap();
    assert!(
        message.contains(&MAX_DOCUMENT_BYTES.to_string()),
        "{message}"
    );
    let (code, rest, _) = finish(child, stdin, stdout);
    assert_eq!((code, rest), (2, Vec::new()));

    // Exactly the cap, then end of input: truncated, exit 2, no frame.
    let (child, mut stdin, stdout) = spawn();
    let at = u32::try_from(MAX_DOCUMENT_BYTES).unwrap();
    stdin.write_all(&at.to_be_bytes()).unwrap();
    let (code, rest, _) = finish(child, stdin, stdout);
    assert_eq!((code, rest), (2, Vec::new()));

    // End of input inside the length prefix: truncated, exit 2, no frame.
    let (child, mut stdin, stdout) = spawn();
    stdin.write_all(&[0, 0]).unwrap();
    let (code, rest, _) = finish(child, stdin, stdout);
    assert_eq!((code, rest), (2, Vec::new()));
}

#[test]
fn worker_error_tags() {
    let (mut child, mut stdin, stdout) = spawn();
    let good = format!(
        r#"{{"args":{{"expr":{X2_PLUS_1}}},"budget":{{"steps":"100"}},"op":"science.cas.canonicalize","v":1}}"#
    );
    let spaced = good.replacen(':', ": ", 1);
    let nope = good.replace("science.cas.canonicalize", "science.cas.nope");

    write_request_frame(&mut stdin, spaced.as_bytes()).unwrap();
    let (stdout, frame) = next_frame(&mut child, stdout);
    assert_eq!(frame.map(|(tag, _)| tag), Some(Tag::BadRequest));

    write_request_frame(&mut stdin, nope.as_bytes()).unwrap();
    let (stdout, frame) = next_frame(&mut child, stdout);
    let (tag, message) = frame.expect("a response frame");
    assert_eq!(tag, Tag::UnsupportedOp);
    assert_eq!(message, b"science.cas.nope");

    // The worker keeps serving after error frames.
    write_request_frame(&mut stdin, good.as_bytes()).unwrap();
    let (stdout, frame) = next_frame(&mut child, stdout);
    let expected = ksci_ops::invoke(good.as_bytes()).unwrap();
    assert_eq!(frame, Some((Tag::Ok, expected)));

    let (code, rest, _) = finish(child, stdin, stdout);
    assert_eq!((code, rest), (0, Vec::new()));
}

/// Review W1: a request whose error text would echo 64 MiB of input still gets a bounded bad-request frame, and the
/// worker keeps serving until the input ends.
#[test]
fn worker_error_frame_bounded() {
    let (mut child, mut stdin, stdout) = spawn();
    let request = format!(r#"{{"{}":1}}"#, "~".repeat(MAX_DOCUMENT_BYTES - 6));
    write_request_frame(&mut stdin, request.as_bytes()).unwrap();
    let (stdout, frame) = next_frame(&mut child, stdout);
    let (tag, message) = frame.expect("a response frame");
    assert_eq!(tag, Tag::BadRequest);
    let bound = ksci_ops::MAX_ERROR_MESSAGE_BYTES;
    assert!(message.len() <= bound, "{} bytes", message.len());
    let (code, rest, _) = finish(child, stdin, stdout);
    assert_eq!((code, rest), (0, Vec::new()));
}
