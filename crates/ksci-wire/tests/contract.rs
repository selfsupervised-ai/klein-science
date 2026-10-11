// provenance: original
//! Contract tests for `klein.science.expr.v1` (see `schema/expr.v1.md`).

use ksci_wire::{
    Budget, Node, NodeId, Provenance, Record, Request, Response, Special, Status, Table, WireError,
    sha256_hex, to_jcs,
};
use proptest::prelude::*;
use proptest::test_runner::{RngAlgorithm, RngSeed};

fn table(json: &str) -> Table {
    Table::from_slice(json.as_bytes()).unwrap_or_else(|e| panic!("{json} should decode: {e}"))
}

fn rejected(json: &str) -> WireError {
    match Table::from_slice(json.as_bytes()) {
        Ok(_) => panic!("{json} should be rejected"),
        Err(e) => e,
    }
}

/// x**2 + 1, canonical: post-order first completion from the root.
const X2_PLUS_1_CANONICAL: &str =
    r#"{"nodes":[["Sym","x",{}],["Int","2"],["Pow",0,1],["Int","1"],["Add",2,3]],"root":4,"v":1}"#;

/// SHA-256 of `X2_PLUS_1_CANONICAL`, computed outside Rust with `shasum -a 256`.
const X2_PLUS_1_IDENTITY: &str = "37f7939bd191aa48173b1b6a45838b6b246aeb8a001d276f9b1c2798342cb964";

#[test]
fn identity_order_independent() {
    let a = table(X2_PLUS_1_CANONICAL);
    let b = table(
        r#"{"nodes":[["Add",3,1],["Int","1"],["Int","2"],["Pow",4,2],["Sym","x",{}]],"root":0,"v":1}"#,
    );
    assert_eq!(a.canonicalize().to_jcs(), b.canonicalize().to_jcs());
    assert_eq!(a.identity(), b.identity());
}

#[test]
fn identity_matches_external_sha256() {
    let a = table(X2_PLUS_1_CANONICAL);
    assert_eq!(a.canonicalize().to_jcs(), X2_PLUS_1_CANONICAL.as_bytes());
    assert_eq!(a.identity(), X2_PLUS_1_IDENTITY);
}

#[test]
fn identity_duplicates_and_unreachable_nodes() {
    // x**2 + 2 with the Int "2" listed twice (one per parent) and an unreachable Sym y.
    let clean =
        table(r#"{"nodes":[["Sym","x",{}],["Int","2"],["Pow",0,1],["Add",2,1]],"root":3,"v":1}"#);
    let dirty = table(
        r#"{"nodes":[["Int","2"],["Sym","x",{}],["Pow",1,0],["Int","2"],["Sym","y",{}],["Add",2,3]],"root":5,"v":1}"#,
    );
    assert_eq!(dirty.canonicalize().nodes().len(), 4);
    assert_eq!(clean.canonicalize().to_jcs(), dirty.canonicalize().to_jcs());
    assert_eq!(clean.identity(), dirty.identity());
}

#[test]
fn identity_single_node_is_its_own_canonical_form() {
    let single = r#"{"nodes":[["Int","7"]],"root":0,"v":1}"#;
    let t = table(single);
    assert_eq!(t.canonicalize(), t);
    assert_eq!(t.canonicalize().to_jcs(), single.as_bytes());
}

#[test]
fn identity_rejects_cycle() {
    let err = rejected(r#"{"nodes":[["Add",1,2],["Sym","x",{}],["Mul",1,0]],"root":0,"v":1}"#);
    assert!(matches!(err, WireError::Cycle { .. }), "{err:?}");
}

#[test]
fn identity_rejects_unreachable_cycle() {
    let err = rejected(r#"{"nodes":[["Int","1"],["Pow",2,0],["Pow",1,0]],"root":0,"v":1}"#);
    assert!(matches!(err, WireError::Cycle { .. }), "{err:?}");
}

#[test]
fn identity_rejects_out_of_range_index() {
    let err = rejected(r#"{"nodes":[["Sym","x",{}],["Pow",0,2]],"root":1,"v":1}"#);
    assert_eq!(
        err,
        WireError::IndexOutOfRange {
            path: "/nodes/1/2".into()
        }
    );
    let err = rejected(r#"{"nodes":[["Sym","x",{}]],"root":1,"v":1}"#);
    assert_eq!(
        err,
        WireError::IndexOutOfRange {
            path: "/root".into()
        }
    );
}

// ---------------------------------------------------------------------------------------------------------------
// DET-02: numerals, special atoms and the strict decoder.

/// A table document with the given node list and root.
fn doc(nodes: &str, root: u32) -> String {
    format!(r#"{{"nodes":[{nodes}],"root":{root},"v":1}}"#)
}

fn numeral_error(path: &str) -> WireError {
    WireError::Numeral { path: path.into() }
}

fn number_error(path: &str) -> WireError {
    WireError::JsonNumberNotAllowed { path: path.into() }
}

#[test]
fn numerals_int() {
    for ok in ["0", "-7", "123"] {
        table(&doc(&format!(r#"["Int","{ok}"]"#), 0));
    }
    for bad in [
        "", "-0", "01", "+1", "1.0", "1e3", " 1", "-", "1 ", "0x1", "١",
    ] {
        assert_eq!(
            rejected(&doc(&format!(r#"["Int","{bad}"]"#), 0)),
            numeral_error("/nodes/0/1"),
            "Int {bad:?}"
        );
    }
}

#[test]
fn numerals_rat() {
    for (p, q) in [("1", "2"), ("-3", "7"), ("2", "4")] {
        // ["Rat","2","4"] is accepted: lowest terms are not checked at the wire layer (D-20).
        table(&doc(&format!(r#"["Rat","{p}","{q}"]"#), 0));
    }
    for (p, q, path) in [
        ("1", "1", "/nodes/0/2"),
        ("1", "0", "/nodes/0/2"),
        ("1", "-2", "/nodes/0/2"),
        ("1", "02", "/nodes/0/2"),
        ("1", "", "/nodes/0/2"),
        ("0", "2", "/nodes/0/1"),
        ("-0", "2", "/nodes/0/1"),
        ("01", "2", "/nodes/0/1"),
    ] {
        assert_eq!(
            rejected(&doc(&format!(r#"["Rat","{p}","{q}"]"#), 0)),
            numeral_error(path),
            "Rat {p:?}/{q:?}"
        );
    }
}

#[test]
fn numerals_float() {
    table(&doc(r#"["Float","5","-3","53"]"#, 0));
    table(&doc(r#"["Float","0","0","1"]"#, 0));
    for (man, exp, prec, path) in [
        ("5", "-3", "0", "/nodes/0/3"),
        ("5", "-3", "-1", "/nodes/0/3"),
        ("5", "-3", "053", "/nodes/0/3"),
        ("05", "-3", "53", "/nodes/0/1"),
        ("5", "-03", "53", "/nodes/0/2"),
        ("5.0", "0", "53", "/nodes/0/1"),
    ] {
        assert_eq!(
            rejected(&doc(&format!(r#"["Float","{man}","{exp}","{prec}"]"#), 0)),
            numeral_error(path),
            "Float {man:?} {exp:?} {prec:?}"
        );
    }
}

#[test]
fn numerals_json_number() {
    // A JSON number where a numeral string belongs.
    assert_eq!(
        rejected(&doc(r#"["Int",5]"#, 0)),
        number_error("/nodes/0/1")
    );
    assert_eq!(
        rejected(&doc(r#"["Rat","1",2]"#, 0)),
        number_error("/nodes/0/2")
    );
    assert_eq!(
        rejected(&doc(r#"["Float","5","-3",53]"#, 0)),
        number_error("/nodes/0/3")
    );
    // A float, negative or too-large child index.
    let leaves = r#"["Sym","x",{}],["Int","1"]"#;
    assert_eq!(
        rejected(&doc(&format!(r#"{leaves},["Add",0,1.0]"#), 2)),
        number_error("/nodes/2/2")
    );
    assert_eq!(
        rejected(&doc(&format!(r#"{leaves},["Add",0,-1]"#), 2)),
        number_error("/nodes/2/2")
    );
    assert_eq!(
        rejected(&doc(&format!(r#"{leaves},["Pow",0,4294967296]"#), 2)),
        number_error("/nodes/2/2")
    );
    // Numbers in name, kind and assumption positions, and a bare number as a node.
    assert_eq!(
        rejected(&doc(r#"["Fn",7,0]"#, 0)),
        number_error("/nodes/0/1")
    );
    assert_eq!(rejected(&doc(r#"[3,"x"]"#, 0)), number_error("/nodes/0/0"));
    assert_eq!(
        rejected(&doc(r#"["Sym","x",{"positive":1}]"#, 0)),
        number_error("/nodes/0/2/positive")
    );
    assert_eq!(
        rejected(&doc(r#"["Atom",1]"#, 0)),
        number_error("/nodes/0/1")
    );
    assert_eq!(rejected(&doc(r#"5"#, 0)), number_error("/nodes/0"));
    // A non-integer root.
    assert_eq!(
        rejected(r#"{"nodes":[["Int","1"]],"root":0.0,"v":1}"#),
        number_error("/root")
    );
}

#[test]
fn numerals_atom() {
    for name in ["oo", "-oo", "zoo", "nan", "pi", "E", "I"] {
        let t = table(&doc(&format!(r#"["Atom","{name}"]"#), 0));
        assert_eq!(
            t.to_jcs(),
            doc(&format!(r#"["Atom","{name}"]"#), 0).as_bytes()
        );
    }
    for name in ["Oo", "inf", "infinity", "", "e", "i", "Pi"] {
        assert_eq!(
            rejected(&doc(&format!(r#"["Atom","{name}"]"#), 0)),
            WireError::UnknownAtom {
                path: "/nodes/0/1".into()
            },
            "Atom {name:?}"
        );
    }
}

#[test]
fn numerals_bigint() {
    let p: String = "9876543210".repeat(30);
    let q: String = "1234567891".repeat(30);
    assert_eq!(p.len(), 300);
    for json in [
        doc(&format!(r#"["Int","-{p}"]"#), 0),
        doc(&format!(r#"["Rat","{p}","{q}"]"#), 0),
        doc(&format!(r#"["Float","{p}","-{q}","1000"]"#), 0),
    ] {
        assert_eq!(table(&json).to_jcs(), json.as_bytes());
        assert_eq!(table(&json).canonicalize().to_jcs(), json.as_bytes());
    }
}

#[test]
fn numerals_boundary_document_size() {
    let mut exact = doc(r#"["Int","1"]"#, 0).into_bytes();
    exact.resize(ksci_wire::MAX_DOCUMENT_BYTES, b' ');
    assert!(Table::from_slice(&exact).is_ok());
    // One byte more is rejected before parsing: these bytes are not JSON, yet the error is the size.
    let over = vec![b'x'; ksci_wire::MAX_DOCUMENT_BYTES + 1];
    assert_eq!(
        Table::from_slice(&over),
        Err(WireError::TooLarge {
            len: ksci_wire::MAX_DOCUMENT_BYTES + 1
        })
    );
}

#[test]
fn numerals_boundary_arity() {
    let leaves = r#"["Sym","x",{}],["Int","1"]"#;
    for kind in ["Add", "Mul"] {
        table(&doc(&format!(r#"{leaves},["{kind}",0,1]"#), 2));
        for bad in [format!(r#"["{kind}",0]"#), format!(r#"["{kind}"]"#)] {
            assert!(
                matches!(
                    rejected(&doc(&format!("{leaves},{bad}"), 2)),
                    WireError::Shape { .. }
                ),
                "{bad}"
            );
        }
    }
    table(&doc(&format!(r#"{leaves},["Pow",0,1]"#), 2));
    table(&doc(r#"["Fn","f"]"#, 0));
    table(&doc(&format!(r#"{leaves},["Fn","f",0,1,0]"#), 2));
    for bad in [r#"["Pow",0,1,1]"#, r#"["Pow",0]"#, r#"["Int","1","2"]"#] {
        assert!(
            matches!(
                rejected(&doc(&format!("{leaves},{bad}"), 2)),
                WireError::Shape { .. }
            ),
            "{bad}"
        );
    }
}

#[test]
fn numerals_boundary_index() {
    // Three nodes: index 2 (len - 1) is accepted, index 3 (len) is rejected.
    table(r#"{"nodes":[["Add",1,2],["Sym","x",{}],["Int","1"]],"root":0,"v":1}"#);
    assert_eq!(
        rejected(r#"{"nodes":[["Add",1,3],["Sym","x",{}],["Int","1"]],"root":0,"v":1}"#),
        WireError::IndexOutOfRange {
            path: "/nodes/0/2".into()
        }
    );
    table(r#"{"nodes":[["Sym","x",{}],["Int","1"],["Add",0,1]],"root":2,"v":1}"#);
    assert_eq!(
        rejected(r#"{"nodes":[["Sym","x",{}],["Int","1"],["Add",0,1]],"root":3,"v":1}"#),
        WireError::IndexOutOfRange {
            path: "/root".into()
        }
    );
}

#[test]
fn numerals_empty() {
    for bad in [
        r#"{"nodes":[],"root":0,"v":1}"#.to_owned(),
        doc(r#"["Sym","",{}]"#, 0),
        doc(r#"["Fn",""]"#, 0),
        doc(r#"[]"#, 0),
        r#"{"nodes":[["Int","1"]],"v":1}"#.to_owned(),
        r#"{"nodes":[["Int","1"]],"root":0}"#.to_owned(),
        r#"{"nodes":[["Int","1"]],"root":0,"v":1,"x":true}"#.to_owned(),
    ] {
        assert!(matches!(rejected(&bad), WireError::Shape { .. }), "{bad}");
    }
    table(&doc(r#"["Sym","x",{}]"#, 0));
    assert_eq!(
        rejected(r#"{"nodes":[["Int","1"]],"root":0,"v":2}"#),
        WireError::Version { path: "/v".into() }
    );
}

#[test]
fn numerals_kind() {
    for kind in ["Foo", "int", "Integer", "Symbol", ""] {
        assert_eq!(
            rejected(&doc(&format!(r#"["{kind}","1"]"#), 0)),
            WireError::UnknownKind {
                path: "/nodes/0/0".into()
            },
            "kind {kind:?}"
        );
    }
}

#[test]
fn numerals_assumption_keys() {
    table(&doc(
        r#"["Sym","x",{"positive":true,"extended_real":false}]"#,
        0,
    ));
    for key in ["Positive", "_x", "", "a1", "a-b", "é"] {
        assert!(
            matches!(
                rejected(&doc(&format!(r#"["Sym","x",{{"{key}":true}}]"#), 0)),
                WireError::Shape { .. }
            ),
            "assumption key {key:?}"
        );
    }
}

#[test]
fn numerals_encoding() {
    // A lone surrogate escape is not JSON text.
    assert!(matches!(
        rejected(&doc(r#"["Sym","\ud800",{}]"#, 0)),
        WireError::NotJson(_)
    ));
    // No Unicode normalisation: "x" and "x" + U+0301 are different names.
    let pair = table(&doc(r#"["Sym","x",{}],["Sym","x́",{}],["Add",0,1]"#, 2));
    assert_eq!(pair.canonicalize().nodes().len(), 3);
    assert_ne!(
        table(&doc(r#"["Sym","x",{}]"#, 0)).identity(),
        table(&doc(r#"["Sym","x́",{}]"#, 0)).identity()
    );
    // Object keys are written in JCS order.
    let sym = table(&doc(r#"["Sym","x",{"real":true,"positive":false}]"#, 0));
    assert_eq!(
        sym.to_jcs(),
        doc(r#"["Sym","x",{"positive":false,"real":true}]"#, 0).as_bytes()
    );
}

#[test]
fn numerals_adjacency() {
    // Structurally identical nodes merge; nodes that differ only in kind stay distinct.
    let t = table(&doc(
        r#"["Int","2"],["Float","1","1","53"],["Int","2"],["Add",0,1,2]"#,
        3,
    ));
    assert_eq!(
        t.canonicalize().to_jcs(),
        doc(r#"["Int","2"],["Float","1","1","53"],["Add",0,1,0]"#, 2).as_bytes()
    );
}

#[test]
fn numerals_argument_order_preserved() {
    let yx = table(&doc(r#"["Sym","y",{}],["Sym","x",{}],["Add",0,1]"#, 2));
    let xy = table(&doc(r#"["Sym","x",{}],["Sym","y",{}],["Add",0,1]"#, 2));
    assert_eq!(
        yx.canonicalize().to_jcs(),
        doc(r#"["Sym","y",{}],["Sym","x",{}],["Add",0,1]"#, 2).as_bytes()
    );
    assert_ne!(yx.identity(), xy.identity());
}

// ---------------------------------------------------------------------------------------------------------------
// DET-03: envelopes, the canonical-input check and provenance outside the hashed bytes.

/// A canonical request for `science.cas.canonicalize` on x**2 + 1 with the given `steps` JSON value.
fn request_json(steps: &str) -> String {
    format!(
        r#"{{"args":{{"expr":{X2_PLUS_1_CANONICAL}}},"budget":{{"steps":{steps}}},"op":"science.cas.canonicalize","v":1}}"#
    )
}

fn request_rejected(json: &str) -> WireError {
    match Request::from_canonical_slice(json.as_bytes()) {
        Ok(_) => panic!("{json} should be rejected"),
        Err(e) => e,
    }
}

#[test]
fn envelope_request_roundtrip() {
    let json = request_json(r#""100""#);
    let request =
        Request::from_canonical_slice(json.as_bytes()).expect("canonical request decodes");
    assert_eq!(request.op, "science.cas.canonicalize");
    assert_eq!(request.budget.steps_u64(), Some(100));
    assert_eq!(request.args["expr"].identity(), X2_PLUS_1_IDENTITY);
    assert_eq!(to_jcs(&request).expect("encodes"), json.as_bytes());
}

#[test]
fn envelope_noncanonical() {
    let canonical = request_json(r#""100""#);
    let variants = [
        // A space after the opening brace.
        canonical.replacen('{', "{ ", 1),
        // Keys reordered: v first.
        format!(
            r#"{{"v":1,"args":{{"expr":{X2_PLUS_1_CANONICAL}}},"budget":{{"steps":"100"}},"op":"science.cas.canonicalize"}}"#
        ),
        // A duplicated key.
        canonical.replacen(
            r#""op":"science.cas.canonicalize""#,
            r#""op":"science.cas.canonicalize","op":"science.cas.canonicalize""#,
            1,
        ),
        // An unnecessary escape.
        canonical.replacen("science.cas", r"science.cas", 1),
        // Assumption keys out of order inside the table.
        canonical.replacen(
            r#"["Sym","x",{}]"#,
            r#"["Sym","x",{"real":true,"positive":true}]"#,
            1,
        ),
        // A trailing newline.
        format!("{canonical}\n"),
    ];
    for json in &variants {
        assert_ne!(json, &canonical);
        assert_eq!(request_rejected(json), WireError::NonCanonical, "{json}");
    }
}

#[test]
fn envelope_steps() {
    for (steps, expected) in [("0", 0), ("18446744073709551615", u64::MAX)] {
        let request =
            Request::from_canonical_slice(request_json(&format!(r#""{steps}""#)).as_bytes())
                .expect("steps decode");
        assert_eq!(request.budget.steps_u64(), Some(expected));
    }
    for steps in ["18446744073709551616", "-1", "01", "", "+1", "1.5"] {
        assert_eq!(
            request_rejected(&request_json(&format!(r#""{steps}""#))),
            WireError::Numeral {
                path: "/budget/steps".into()
            },
            "steps {steps:?}"
        );
    }
    assert_eq!(
        request_rejected(&request_json("5")),
        WireError::JsonNumberNotAllowed {
            path: "/budget/steps".into()
        }
    );
    assert_eq!(
        Budget {
            steps: "007".into()
        }
        .steps_u64(),
        None
    );
}

#[test]
fn envelope_request_rejects_bad_contents() {
    // A version other than 1.
    let v2 = request_json(r#""1""#).replacen(r#","v":1}"#, r#","v":2}"#, 1);
    assert!(v2.ends_with(r#""v":2}"#));
    assert_eq!(
        request_rejected(&v2),
        WireError::Version { path: "/v".into() }
    );
    // A JSON number inside the argument table is reported with its full path.
    let number = request_json(r#""1""#).replacen(r#"["Int","2"]"#, r#"["Int",2]"#, 1);
    assert_eq!(
        request_rejected(&number),
        WireError::JsonNumberNotAllowed {
            path: "/args/expr/nodes/1/1".into()
        }
    );
    // A cyclic argument table, an unknown key and a missing budget.
    let cyclic = request_json(r#""1""#).replacen(r#"["Pow",0,1]"#, r#"["Pow",4,1]"#, 1);
    let extra = request_json(r#""1""#).replacen(r#","v":1}"#, r#","v":1,"x":true}"#, 1);
    let missing = format!(
        r#"{{"args":{{"expr":{X2_PLUS_1_CANONICAL}}},"op":"science.cas.canonicalize","v":1}}"#
    );
    for json in [cyclic, extra, missing] {
        assert!(
            !matches!(request_rejected(&json), WireError::NonCanonical),
            "{json}"
        );
    }
}

fn exact_response() -> Response {
    Response {
        v: 1,
        status: Status::Exact,
        numeric_class: "exact".into(),
        result: Some(table(X2_PLUS_1_CANONICAL)),
        identity: Some(X2_PLUS_1_IDENTITY.into()),
    }
}

fn exhausted_response() -> Response {
    Response {
        v: 1,
        status: Status::FuelExhausted,
        numeric_class: "exact".into(),
        result: None,
        identity: None,
    }
}

#[test]
fn envelope_response_shape() {
    let exact = format!(
        r#"{{"identity":"{X2_PLUS_1_IDENTITY}","numeric_class":"exact","result":{X2_PLUS_1_CANONICAL},"status":"exact","v":1}}"#
    );
    let exhausted = r#"{"identity":null,"numeric_class":"exact","result":null,"status":"fuel_exhausted","v":1}"#;
    assert_eq!(exact_response().to_jcs(), exact.as_bytes());
    assert_eq!(exhausted_response().to_jcs(), exhausted.as_bytes());
    assert_eq!(Response::from_slice(exact.as_bytes()), Ok(exact_response()));
    assert_eq!(
        Response::from_slice(exhausted.as_bytes()),
        Ok(exhausted_response())
    );
}

fn provenance(tag: &str) -> Provenance {
    Provenance {
        impl_: format!("ksci-{tag}"),
        backend: format!("backend-{tag}"),
        device: format!("device-{tag}"),
        driver: format!("driver-{tag}"),
        wall_ms: format!("{}", tag.len()),
    }
}

#[test]
fn envelope_provenance_outside() {
    let request = request_json(r#""100""#);
    let response = exact_response().to_jcs();
    let r0 = Record::new(request.as_bytes(), &response, provenance("r0"));
    let r2 = Record::new(request.as_bytes(), &response, provenance("r2-metal"));
    assert_ne!(r0.provenance, r2.provenance);
    assert_eq!(r0.request_digest, r2.request_digest);
    assert_eq!(r0.response_digest, r2.response_digest);
    assert_eq!(r0.request_digest, sha256_hex(request.as_bytes()));
    assert_eq!(r0.response_digest, sha256_hex(&response));
    // The record itself carries provenance under "impl".
    let record = String::from_utf8(to_jcs(&r0).expect("encodes")).expect("utf-8");
    assert!(
        record.contains(r#""provenance":{"backend":"backend-r0""#),
        "{record}"
    );
    assert!(record.contains(r#""impl":"ksci-r0""#), "{record}");
    // A response or request carrying provenance is rejected.
    let with_provenance = String::from_utf8(response).expect("utf-8").replacen(
        r#""numeric_class""#,
        r#""provenance":{},"numeric_class""#,
        1,
    );
    assert!(Response::from_slice(with_provenance.as_bytes()).is_err());
    let request_with_provenance = request.replacen(r#""op""#, r#""provenance":{},"op""#, 1);
    assert!(Request::from_canonical_slice(request_with_provenance.as_bytes()).is_err());
}

// ---------------------------------------------------------------------------------------------------------------
// DET-01: identity does not depend on construction order (property test, fixed seed so failures replay).

/// One generated node: a leaf, or an internal node whose child selectors are reduced modulo its own index.
#[derive(Clone, Debug)]
enum Spec {
    Int(i64),
    Sym(u8),
    Atom(u8),
    Add(Vec<usize>),
    Mul(Vec<usize>),
    Pow(usize, usize),
    Fn(u8, Vec<usize>),
}

fn spec() -> impl Strategy<Value = Spec> {
    prop_oneof![
        any::<i64>().prop_map(Spec::Int),
        (0u8..5).prop_map(Spec::Sym),
        (0u8..7).prop_map(Spec::Atom),
        prop::collection::vec(any::<usize>(), 2..5).prop_map(Spec::Add),
        prop::collection::vec(any::<usize>(), 2..5).prop_map(Spec::Mul),
        (any::<usize>(), any::<usize>()).prop_map(|(b, e)| Spec::Pow(b, e)),
        ((0u8..3), prop::collection::vec(any::<usize>(), 0..4)).prop_map(|(f, a)| Spec::Fn(f, a)),
    ]
}

const ATOMS: [&str; 7] = ["oo", "-oo", "zoo", "nan", "pi", "E", "I"];

fn leaf(spec: &Spec) -> Node {
    match spec {
        Spec::Int(n) => Node::Int(n.to_string()),
        Spec::Sym(s) | Spec::Fn(s, _) => Node::Sym {
            name: format!("s{s}"),
            assumptions: Default::default(),
        },
        Spec::Atom(a) => {
            Node::Atom(Special::from_name(ATOMS[usize::from(*a)]).expect("named atom"))
        }
        Spec::Add(_) | Spec::Mul(_) | Spec::Pow(..) => Node::Int("0".into()),
    }
}

/// A DAG where node i refers only to nodes below i; the root is the last node.
fn build(specs: &[Spec]) -> Vec<Node> {
    specs
        .iter()
        .enumerate()
        .map(|(i, spec)| {
            let pick = |sel: &usize| NodeId(u32::try_from(sel % i).expect("small"));
            match spec {
                _ if i == 0 => leaf(spec),
                Spec::Add(c) => Node::Add(c.iter().map(pick).collect()),
                Spec::Mul(c) => Node::Mul(c.iter().map(pick).collect()),
                Spec::Pow(b, e) => Node::Pow(pick(b), pick(e)),
                Spec::Fn(f, a) => Node::Fn {
                    name: format!("f{f}"),
                    args: a.iter().map(pick).collect(),
                },
                leaf_spec => leaf(leaf_spec),
            }
        })
        .collect()
}

fn remap(node: &Node, mut f: impl FnMut(NodeId) -> NodeId) -> Node {
    match node {
        Node::Add(c) => Node::Add(c.iter().map(|&i| f(i)).collect()),
        Node::Mul(c) => Node::Mul(c.iter().map(|&i| f(i)).collect()),
        Node::Pow(b, e) => Node::Pow(f(*b), f(*e)),
        Node::Fn { name, args } => Node::Fn {
            name: name.clone(),
            args: args.iter().map(|&i| f(i)).collect(),
        },
        leaf => leaf.clone(),
    }
}

/// The same expression rebuilt with duplicated nodes (some edges redirected to the copies), unreachable nodes
/// appended, and every node moved to a new position.
fn scramble(base: &[Node], dups: &[usize], redirect: &[bool], extra: &[u8], keys: &[u64]) -> Table {
    let n = base.len();
    let mut nodes = base.to_vec();
    let mut copy_of: Vec<Option<usize>> = vec![None; n];
    for d in dups {
        let src = d % n;
        if copy_of[src].is_none() {
            copy_of[src] = Some(nodes.len());
            nodes.push(base[src].clone());
        }
    }
    let mut edge = 0usize;
    let mut nodes: Vec<Node> = nodes
        .iter()
        .map(|node| {
            remap(node, |c| {
                edge += 1;
                match copy_of[c.index()] {
                    Some(copy) if redirect[edge % redirect.len()] => {
                        NodeId(u32::try_from(copy).expect("small"))
                    }
                    _ => c,
                }
            })
        })
        .collect();
    for (k, e) in extra.iter().enumerate() {
        nodes.push(Node::Sym {
            name: format!("unreachable{e}"),
            assumptions: Default::default(),
        });
        if k == 0 {
            // An unreachable internal node pointing into the live part.
            nodes.push(Node::Add(vec![
                NodeId(0),
                NodeId(u32::try_from(n - 1).expect("small")),
            ]));
        }
    }
    // Move node i to the position of its rank under (key, i).
    let mut order: Vec<usize> = (0..nodes.len()).collect();
    order.sort_by_key(|&i| (keys[i % keys.len()], i));
    let mut position = vec![0u32; nodes.len()];
    for (new, &old) in order.iter().enumerate() {
        position[old] = u32::try_from(new).expect("small");
    }
    let moved: Vec<Node> = order
        .iter()
        .map(|&old| remap(&nodes[old], |c| NodeId(position[c.index()])))
        .collect();
    Table::new(moved, NodeId(position[n - 1])).expect("scrambled table is valid")
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 512,
        rng_algorithm: RngAlgorithm::ChaCha,
        rng_seed: RngSeed::Fixed(0x6b73_6369_7769_7265),
        failure_persistence: None,
        ..ProptestConfig::default()
    })]

    #[test]
    fn identity_property(
        specs in prop::collection::vec(spec(), 1..=48),
        dups in prop::collection::vec(any::<usize>(), 0..=8),
        redirect in prop::collection::vec(any::<bool>(), 1..=8),
        extra in prop::collection::vec(any::<u8>(), 0..=7),
        keys in prop::collection::vec(any::<u64>(), 1..=64),
    ) {
        let base = build(&specs);
        let root = NodeId(u32::try_from(base.len() - 1).expect("small"));
        let clean = Table::new(base.clone(), root).expect("generated table is valid");
        let scrambled = scramble(&base, &dups, &redirect, &extra, &keys);
        prop_assert!(scrambled.nodes().len() <= 64);
        prop_assert_eq!(clean.canonicalize().to_jcs(), scrambled.canonicalize().to_jcs());
        prop_assert_eq!(clean.identity(), scrambled.identity());
    }
}
