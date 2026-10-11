// provenance: original
//! The op layer's contract: the step budget, the error kinds, plan/map/reduce and the shared request corpus.

use ksci_ops::{OpError, invoke, map, plan, reduce};
use ksci_wire::{Request, Response, Status};

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
