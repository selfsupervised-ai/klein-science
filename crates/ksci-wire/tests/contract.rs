// provenance: original
//! Contract tests for `klein.science.expr.v1` (see `schema/expr.v1.md`).

use ksci_wire::{Table, WireError};

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
