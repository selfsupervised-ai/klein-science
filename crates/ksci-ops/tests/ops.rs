// provenance: original
//! The op layer's contract: the step budget, the error kinds, plan/map/reduce and the shared request corpus.

use ksci_ops::{MAX_ERROR_MESSAGE_BYTES, OpError, invoke, map, plan, reduce};
use ksci_wire::{MAX_DOCUMENT_BYTES, Request, Response, Status};

/// `x**2 + 1` in canonical order: 5 reachable nodes (Sym x, Int 2, Pow, Int 1, Add).
const X2_PLUS_1: &str =
    r#"{"nodes":[["Sym","x",{}],["Int","2"],["Pow",0,1],["Int","1"],["Add",2,3]],"root":4,"v":1}"#;

/// The one byte string of a budget-exhausted response (schema/expr.v1.md, Response).
const FUEL_EXHAUSTED: &[u8] =
    br#"{"identity":null,"numeric_class":"exact","result":null,"status":"fuel_exhausted","v":1}"#;

/// The canonical request for `op` with `args` (an already-canonical JSON object) and `steps`.
fn request_with(op: &str, args: &str, steps: &str) -> Vec<u8> {
    format!(r#"{{"args":{args},"budget":{{"steps":"{steps}"}},"op":"{op}","v":1}}"#).into_bytes()
}

fn canonicalize_request(steps: &str) -> Vec<u8> {
    request_with(
        "science.cas.canonicalize",
        &format!(r#"{{"expr":{X2_PLUS_1}}}"#),
        steps,
    )
}

fn status(response: &[u8]) -> Status {
    Response::from_slice(response)
        .expect("a valid response")
        .status()
}

fn is_bad_request(result: &Result<Vec<u8>, OpError>) -> bool {
    matches!(result, Err(OpError::BadRequest(_)))
}

/// The lines of the shared request corpus, without their newline.
fn corpus() -> Vec<Vec<u8>> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../testdata/wire/requests.jsonl"
    );
    let bytes = std::fs::read(path).expect("testdata/wire/requests.jsonl is readable");
    assert_eq!(bytes.last(), Some(&b'\n'), "the corpus ends with a newline");
    bytes[..bytes.len() - 1]
        .split(|b| *b == b'\n')
        .map(<[u8]>::to_vec)
        .collect()
}

#[test]
fn ops_budget_exhausted() {
    let four = invoke(&canonicalize_request("4")).expect("a valid request");
    assert_eq!(status(&four), Status::FuelExhausted);
    assert_eq!(four, FUEL_EXHAUSTED, "fuel_exhausted has fixed bytes");

    let five = invoke(&canonicalize_request("5")).expect("a valid request");
    assert_eq!(
        status(&five),
        Status::Exact,
        "5 steps cover 5 reachable nodes"
    );

    let zero = invoke(&canonicalize_request("0")).expect("a valid request");
    assert_eq!(zero, FUEL_EXHAUSTED);
}

/// The step counting rules of schema/expr.v1.md (Request): one step per input node first entered.
#[test]
fn ops_budget_counts_entered_nodes() {
    let fewest_steps = |table: &str| {
        let args = format!(r#"{{"expr":{table}}}"#);
        (0..=10u32)
            .find(|steps| {
                let request = request_with("science.cas.canonicalize", &args, &steps.to_string());
                status(&invoke(&request).unwrap()) == Status::Exact
            })
            .expect("at most 10 steps")
    };
    // Structurally equal nodes at different indices are each entered.
    assert_eq!(
        fewest_steps(r#"{"nodes":[["Sym","x",{}],["Sym","x",{}],["Mul",0,1]],"root":2,"v":1}"#),
        3
    );
    // An index referenced twice is entered once.
    assert_eq!(
        fewest_steps(r#"{"nodes":[["Sym","x",{}],["Mul",0,0]],"root":1,"v":1}"#),
        2
    );
    // Unreachable nodes are never entered.
    assert_eq!(
        fewest_steps(
            r#"{"nodes":[["Int","7"],["Sym","y",{}],["Int","3"],["Add",1,2]],"root":3,"v":1}"#
        ),
        3
    );
}

#[test]
fn ops_errors() {
    // Non-canonical: a space after the first colon, and the keys out of order.
    let mut spaced = canonicalize_request("100");
    spaced.insert(8, b' ');
    assert!(
        is_bad_request(&invoke(&spaced)),
        "whitespace is non-canonical"
    );
    let reordered = format!(
        r#"{{"budget":{{"steps":"1"}},"args":{{"expr":{X2_PLUS_1}}},"op":"science.cas.canonicalize","v":1}}"#
    );
    assert!(is_bad_request(&invoke(reordered.as_bytes())));
    assert!(is_bad_request(&invoke(b"not json")));

    let nope = request_with(
        "science.cas.nope",
        &format!(r#"{{"expr":{X2_PLUS_1}}}"#),
        "1",
    );
    assert_eq!(
        invoke(&nope),
        Err(OpError::UnsupportedOp("science.cas.nope".into()))
    );

    let op = "science.cas.canonicalize";
    for args in [
        "{}".to_owned(),
        format!(r#"{{"e":{X2_PLUS_1}}}"#),
        format!(r#"{{"expr":{X2_PLUS_1},"extra":{X2_PLUS_1}}}"#),
    ] {
        let request = request_with(op, &args, "100");
        Request::from_canonical_slice(&request).expect("the envelope itself is valid");
        assert!(is_bad_request(&invoke(&request)), "args {args}");
    }
}

#[test]
fn ops_plan_map_reduce() {
    let r = canonicalize_request("100");
    assert_eq!(plan(&r), Ok(vec![r.clone()]), "one piece: the request");
    let piece = &plan(&r).unwrap()[0];
    let o = invoke(&r).unwrap();
    assert_eq!(map(piece), Ok(o.clone()), "map is invoke");
    assert_eq!(reduce(&r, std::slice::from_ref(&o)), Ok(o.clone()));

    assert!(is_bad_request(&reduce(&r, &[])), "no output");
    assert!(
        is_bad_request(&reduce(&r, &[o.clone(), o.clone()])),
        "two outputs"
    );
    assert!(
        is_bad_request(&reduce(&r, &[b"junk".to_vec()])),
        "not a response"
    );

    let mut spaced = r.clone();
    spaced.insert(8, b' ');
    assert!(matches!(plan(&spaced), Err(OpError::BadRequest(_))));
    let nope = request_with(
        "science.cas.nope",
        &format!(r#"{{"expr":{X2_PLUS_1}}}"#),
        "1",
    );
    assert_eq!(
        plan(&nope),
        Err(OpError::UnsupportedOp("science.cas.nope".into()))
    );
}

#[test]
fn requests_corpus_canonical() {
    let lines = corpus();
    assert!(lines.len() >= 12, "{} corpus lines", lines.len());
    let (mut exact, mut exhausted) = (0, 0);
    for (i, line) in lines.iter().enumerate() {
        let n = i + 1;
        if let Err(e) = Request::from_canonical_slice(line) {
            panic!("line {n} is not a canonical request: {e}");
        }
        match invoke(line).map(|r| status(&r)) {
            Ok(Status::Exact) => exact += 1,
            Ok(Status::FuelExhausted) => exhausted += 1,
            Err(e) => panic!("line {n}: {e}"),
        }
    }
    assert!(
        exact > 0 && exhausted > 0,
        "the corpus covers both statuses"
    );
}

/// The message of a request that fails.
fn error_message(request: &[u8]) -> String {
    invoke(request)
        .expect_err("the request fails")
        .message()
        .to_owned()
}

/// Review W1: an error message never echoes unbounded input. It is at most MAX_ERROR_MESSAGE_BYTES, the same for the
/// same request, and a cut message ends with " [truncated]".
#[test]
fn ops_error_message_bounded() {
    // A key echoed in a JSON pointer: a 1 MiB request, and one exactly at the document cap.
    for size in [1 << 20, MAX_DOCUMENT_BYTES] {
        let request = format!(r#"{{"{}":1}}"#, "~".repeat(size - 6));
        assert_eq!(request.len(), size);
        let message = error_message(request.as_bytes());
        assert_eq!(
            message.len(),
            MAX_ERROR_MESSAGE_BYTES,
            "a {size}-byte request"
        );
        assert!(message.ends_with(" [truncated]"));
        assert_eq!(message, error_message(request.as_bytes()), "deterministic");
    }
    // Four-byte characters at every alignment against the bound: the cut lands on a character boundary.
    for lead in 0..4 {
        let key = format!("{}{}", "~".repeat(lead), "\u{1D465}".repeat(1000));
        let message = error_message(format!(r#"{{"{key}":1}}"#).as_bytes());
        assert!(message.len() <= MAX_ERROR_MESSAGE_BYTES, "lead {lead}");
        assert!(message.len() > MAX_ERROR_MESSAGE_BYTES - 4, "lead {lead}");
        assert!(message.ends_with(" [truncated]"), "lead {lead}");
    }
    // An unbounded operation name.
    let op = "x".repeat(1 << 20);
    let request = request_with(&op, &format!(r#"{{"expr":{X2_PLUS_1}}}"#), "1");
    assert!(matches!(invoke(&request), Err(OpError::UnsupportedOp(_))));
    assert_eq!(error_message(&request).len(), MAX_ERROR_MESSAGE_BYTES);
    // A short message is kept whole.
    let nope = request_with(
        "science.cas.nope",
        &format!(r#"{{"expr":{X2_PLUS_1}}}"#),
        "1",
    );
    assert_eq!(error_message(&nope), "science.cas.nope");
}
