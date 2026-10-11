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
