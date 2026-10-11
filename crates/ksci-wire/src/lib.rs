// provenance: original
//! `ksci-wire`: the `klein.science.expr.v1` wire contract.
//!
//! An expression travels as a node table `{"nodes":[...],"root":n,"v":1}`. Each node is a JSON array whose first
//! element names its kind; children are referenced by index into `nodes`. This crate owns:
//!
//! - the strict decoder ([`Table::from_slice`], [`Table::from_value`]);
//! - the canonical form ([`Table::canonicalize`]): post-order first completion from the root, structural duplicates
//!   merged, unreachable nodes dropped, argument order preserved;
//! - the only Rust JCS encoder ([`to_jcs`], a wrapper over `serde_json_canonicalizer`);
//! - the only digest helper ([`sha256_hex`]);
//! - expression identity ([`Table::identity`]) = lowercase-hex SHA-256 of the JCS bytes of the canonical table.
//!
//! The contract text is `schema/expr.v1.md`.

use std::collections::BTreeMap;
use std::ops::ControlFlow;

use serde::de::DeserializeSeed;
use serde::ser::{SerializeSeq, SerializeStruct};
use serde::{Serialize, Serializer};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

pub mod envelope;
pub mod numeral;

pub use envelope::{Budget, Provenance, Record, Request, Response, Status};

/// The `v` every document carries.
pub const SCHEMA_VERSION: u32 = 1;

/// Documents longer than this are rejected before parsing.
pub const MAX_DOCUMENT_BYTES: usize = 64 * 1024 * 1024;

/// Arrays and objects may nest at most this deep. The deepest valid document, a request whose argument table holds a
/// symbol with assumptions, nests 6 deep.
pub const MAX_DEPTH: usize = 32;

/// An index into a table's `nodes`.
///
/// It deliberately has no ordering: node indices are positions in one particular table, never a structural order.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct NodeId(pub u32);

static_assertions::assert_not_impl_any!(NodeId: PartialOrd, Ord);

impl NodeId {
    /// The index as a `usize`.
    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

/// A special value. On the wire it is a named atom, never a JSON number or a float.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Special {
    /// `oo`
    Oo,
    /// `-oo`
    NegOo,
    /// `zoo` (complex infinity)
    Zoo,
    /// `nan`
    Nan,
    /// `pi`
    Pi,
    /// `E`
    E,
    /// `I`
    I,
}

impl Special {
    const ALL: [Special; 7] = [
        Special::Oo,
        Special::NegOo,
        Special::Zoo,
        Special::Nan,
        Special::Pi,
        Special::E,
        Special::I,
    ];

    /// The wire name.
    pub const fn name(self) -> &'static str {
        match self {
            Special::Oo => "oo",
            Special::NegOo => "-oo",
            Special::Zoo => "zoo",
            Special::Nan => "nan",
            Special::Pi => "pi",
            Special::E => "E",
            Special::I => "I",
        }
    }

    /// The atom with this exact wire name, if any.
    pub fn from_name(name: &str) -> Option<Special> {
        Special::ALL.into_iter().find(|s| s.name() == name)
    }
}

/// One node of the table. Numerals are decimal strings; children are [`NodeId`]s.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Node {
    /// `["Int", digits]`
    Int(String),
    /// `["Rat", p, q]`
    Rat(String, String),
    /// `["Float", man, exp, prec]`
    Float {
        /// Mantissa, an Int numeral.
        man: String,
        /// Binary exponent, an Int numeral.
        exp: String,
        /// Precision in bits, a positive numeral.
        prec: String,
    },
    /// `["Sym", name, {assumption: bool, ...}]`
    Sym {
        /// The symbol name, compared as exact UTF-8 bytes.
        name: String,
        /// Assumptions are part of the symbol's identity.
        assumptions: BTreeMap<String, bool>,
    },
    /// `["Atom", name]`
    Atom(Special),
    /// `["Add", i, j, ...]`
    Add(Vec<NodeId>),
    /// `["Mul", i, j, ...]`
    Mul(Vec<NodeId>),
    /// `["Pow", base, exp]`
    Pow(NodeId, NodeId),
    /// `["Fn", name, i, ...]`
    Fn {
        /// The function name, compared as exact UTF-8 bytes.
        name: String,
        /// The arguments, in the order given.
        args: Vec<NodeId>,
    },
}

impl Node {
    /// The kind tag, the first element of the wire array.
    pub const fn kind(&self) -> &'static str {
        match self {
            Node::Int(_) => "Int",
            Node::Rat(..) => "Rat",
            Node::Float { .. } => "Float",
            Node::Sym { .. } => "Sym",
            Node::Atom(_) => "Atom",
            Node::Add(_) => "Add",
            Node::Mul(_) => "Mul",
            Node::Pow(..) => "Pow",
            Node::Fn { .. } => "Fn",
        }
    }

    /// The child at argument position `pos`, if there is one.
    pub fn child(&self, pos: usize) -> Option<NodeId> {
        match self {
            Node::Add(c) | Node::Mul(c) | Node::Fn { args: c, .. } => c.get(pos).copied(),
            Node::Pow(base, exp) => match pos {
                0 => Some(*base),
                1 => Some(*exp),
                _ => None,
            },
            Node::Int(_)
            | Node::Rat(..)
            | Node::Float { .. }
            | Node::Sym { .. }
            | Node::Atom(_) => None,
        }
    }

    /// This node with every child replaced by `f(child)`, argument order kept.
    fn map_children(&self, mut f: impl FnMut(NodeId) -> NodeId) -> Node {
        match self {
            Node::Add(c) => Node::Add(c.iter().map(|&i| f(i)).collect()),
            Node::Mul(c) => Node::Mul(c.iter().map(|&i| f(i)).collect()),
            Node::Pow(base, exp) => Node::Pow(f(*base), f(*exp)),
            Node::Fn { name, args } => Node::Fn {
                name: name.clone(),
                args: args.iter().map(|&i| f(i)).collect(),
            },
            leaf => leaf.clone(),
        }
    }
}

impl Serialize for Node {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut seq = serializer.serialize_seq(None)?;
        seq.serialize_element(self.kind())?;
        match self {
            Node::Int(digits) => seq.serialize_element(digits)?,
            Node::Rat(p, q) => {
                seq.serialize_element(p)?;
                seq.serialize_element(q)?;
            }
            Node::Float { man, exp, prec } => {
                seq.serialize_element(man)?;
                seq.serialize_element(exp)?;
                seq.serialize_element(prec)?;
            }
            Node::Sym { name, assumptions } => {
                seq.serialize_element(name)?;
                seq.serialize_element(assumptions)?;
            }
            Node::Atom(special) => seq.serialize_element(special.name())?,
            Node::Add(children) | Node::Mul(children) => {
                for child in children {
                    seq.serialize_element(&child.0)?;
                }
            }
            Node::Pow(base, exp) => {
                seq.serialize_element(&base.0)?;
                seq.serialize_element(&exp.0)?;
            }
            Node::Fn { name, args } => {
                seq.serialize_element(name)?;
                for child in args {
                    seq.serialize_element(&child.0)?;
                }
            }
        }
        seq.end()
    }
}

/// Every way a document can be rejected.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum WireError {
    /// The document is longer than [`MAX_DOCUMENT_BYTES`]; it was not parsed.
    #[error("document is {len} bytes, above the {MAX_DOCUMENT_BYTES}-byte cap")]
    TooLarge {
        /// The document length in bytes.
        len: usize,
    },
    /// The bytes are not JSON (including invalid UTF-8 and lone surrogate escapes).
    #[error("not JSON: {0}")]
    NotJson(String),
    /// An object repeats a key (compared after unescaping). JSON parsers disagree on which copy wins, so none does.
    #[error("duplicate object key at {path}")]
    DuplicateKey {
        /// JSON pointer to the repeated key.
        path: String,
    },
    /// Arrays and objects nest deeper than [`MAX_DEPTH`].
    #[error("nesting deeper than {MAX_DEPTH} at {path}")]
    TooDeep {
        /// JSON pointer to the first array or object past the limit.
        path: String,
    },
    /// A JSON number where only a numeral string, name or object may appear.
    #[error("JSON number not allowed at {path}")]
    JsonNumberNotAllowed {
        /// JSON pointer to the number.
        path: String,
    },
    /// `v` is missing or not [`SCHEMA_VERSION`].
    #[error("unsupported schema version at {path}")]
    Version {
        /// JSON pointer to `v`.
        path: String,
    },
    /// The document does not have the shape the contract requires.
    #[error("bad shape at {path}: {reason}")]
    Shape {
        /// JSON pointer to the offending value.
        path: String,
        /// What was expected.
        reason: &'static str,
    },
    /// A node kind outside the closed set.
    #[error("unknown node kind at {path}")]
    UnknownKind {
        /// JSON pointer to the kind tag.
        path: String,
    },
    /// An atom name outside the seven named atoms.
    #[error("unknown atom at {path}")]
    UnknownAtom {
        /// JSON pointer to the atom name.
        path: String,
    },
    /// A numeral string that does not match its grammar.
    #[error("bad numeral at {path}")]
    Numeral {
        /// JSON pointer to the numeral.
        path: String,
    },
    /// A child index or `root` that is not below `nodes.len()`.
    #[error("index out of range at {path}")]
    IndexOutOfRange {
        /// JSON pointer to the index.
        path: String,
    },
    /// The table has a cycle through this node.
    #[error("cycle through node {node}")]
    Cycle {
        /// A node on the cycle.
        node: u32,
    },
    /// An envelope that does not match its schema (missing or unknown field, wrong type, bad argument table).
    #[error("bad envelope: {0}")]
    Envelope(String),
    /// A request whose bytes differ from its own JCS re-encoding.
    #[error("document is not in canonical (JCS) form")]
    NonCanonical,
    /// A response whose fields contradict each other or the contract (status, numeric class, result presence).
    #[error("invalid response at {path}: {reason}")]
    InvalidResponse {
        /// JSON pointer to the offending field.
        path: String,
        /// The rule that was broken.
        reason: &'static str,
    },
    /// A response whose `identity` is not the identity of its `result`.
    #[error("response identity {claimed} is not the result's identity {actual}")]
    IdentityMismatch {
        /// The identity the response carries.
        claimed: String,
        /// The identity of the response's result table.
        actual: String,
    },
    /// The JCS encoder failed.
    #[error("encode: {0}")]
    Encode(String),
}

fn shape(path: impl Into<String>, reason: &'static str) -> WireError {
    WireError::Shape {
        path: path.into(),
        reason,
    }
}

/// RFC 8785 (JCS) bytes of `value`. This is the only JSON encoder in klein-science's Rust code.
pub fn to_jcs<T: Serialize>(value: &T) -> Result<Vec<u8>, WireError> {
    serde_json_canonicalizer::to_vec(value).map_err(|e| WireError::Encode(e.to_string()))
}

/// Lowercase-hex SHA-256 of `bytes`. This is the only digest helper.
pub fn sha256_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for byte in digest.iter() {
        out.push(char::from(HEX[usize::from(byte >> 4)]));
        out.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    out
}

/// Parse `bytes` as JSON after the size check, rejecting repeated keys and nesting deeper than [`MAX_DEPTH`].
pub(crate) fn parse_document(bytes: &[u8]) -> Result<Value, WireError> {
    if bytes.len() > MAX_DOCUMENT_BYTES {
        return Err(WireError::TooLarge { len: bytes.len() });
    }
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    let value = strict_value(&mut deserializer)
        .map_err(|(e, strict)| strict.unwrap_or_else(|| WireError::NotJson(e.to_string())))?;
    deserializer
        .end()
        .map_err(|e| WireError::NotJson(e.to_string()))?;
    Ok(value)
}

/// Build a [`Value`] from `deserializer`, failing on a repeated object key or nesting deeper than [`MAX_DEPTH`].
///
/// On failure it returns the deserializer's error and, when the failure is one of those two, the typed
/// [`WireError`] that names it.
fn strict_value<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Value, (D::Error, Option<WireError>)> {
    let failure = std::cell::RefCell::new(None);
    match (Strict {
        depth: 0,
        failure: &failure,
    })
    .deserialize(deserializer)
    {
        Ok(value) => Ok(value),
        Err(e) => Err((
            e,
            failure.into_inner().map(|(kind, mut path)| {
                path.reverse();
                let path = pointer_owned(&path);
                match kind {
                    StrictFailure::DuplicateKey => WireError::DuplicateKey { path },
                    StrictFailure::TooDeep => WireError::TooDeep { path },
                }
            }),
        )),
    }
}

/// One step of a path, owned, collected while a strict parse unwinds.
enum OwnedSeg {
    Key(String),
    Index(usize),
}

fn pointer_owned(path: &[OwnedSeg]) -> String {
    let segs: Vec<Seg<'_>> = path
        .iter()
        .map(|seg| match seg {
            OwnedSeg::Key(key) => Seg::Key(key),
            OwnedSeg::Index(i) => Seg::Index(*i),
        })
        .collect();
    pointer(&segs)
}

#[derive(Clone, Copy)]
enum StrictFailure {
    DuplicateKey,
    TooDeep,
}

/// The failure and its path, innermost segment first: each enclosing container appends its own segment as the
/// error unwinds, so the success path builds no paths at all.
type FailureCell = std::cell::RefCell<Option<(StrictFailure, Vec<OwnedSeg>)>>;

/// A seed that builds a [`Value`] like serde_json's own, but fails on a repeated key and counts nesting.
///
/// The depth bound is ours and stricter than serde_json's recursion limit (128), so recursion here is bounded at
/// [`MAX_DEPTH`] frames.
#[derive(Clone, Copy)]
struct Strict<'a> {
    depth: usize,
    failure: &'a FailureCell,
}

impl Strict<'_> {
    /// Enter one more array or object, or fail as too deep.
    fn enter<E: serde::de::Error>(self) -> Result<Self, E> {
        if self.depth >= MAX_DEPTH {
            *self.failure.borrow_mut() = Some((StrictFailure::TooDeep, Vec::new()));
            return Err(E::custom(format_args!("nesting deeper than {MAX_DEPTH}")));
        }
        Ok(Strict {
            depth: self.depth + 1,
            failure: self.failure,
        })
    }

    /// Add `seg` to the path of a failure that is unwinding through this container.
    fn unwind(self, seg: impl FnOnce() -> OwnedSeg) {
        if let Some((_, path)) = self.failure.borrow_mut().as_mut() {
            path.push(seg());
        }
    }
}

impl<'de> serde::de::DeserializeSeed<'de> for Strict<'_> {
    type Value = Value;

    fn deserialize<D: serde::Deserializer<'de>>(self, deserializer: D) -> Result<Value, D::Error> {
        deserializer.deserialize_any(self)
    }
}

impl<'de> serde::de::Visitor<'de> for Strict<'_> {
    type Value = Value;

    fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("a JSON value")
    }

    fn visit_bool<E>(self, b: bool) -> Result<Value, E> {
        Ok(Value::Bool(b))
    }

    fn visit_i64<E>(self, n: i64) -> Result<Value, E> {
        Ok(Value::Number(n.into()))
    }

    fn visit_u64<E>(self, n: u64) -> Result<Value, E> {
        Ok(Value::Number(n.into()))
    }

    fn visit_f64<E>(self, n: f64) -> Result<Value, E> {
        // As serde_json's own Value: JSON text has no NaN or infinity, so this is always a number.
        Ok(serde_json::Number::from_f64(n).map_or(Value::Null, Value::Number))
    }

    fn visit_str<E>(self, s: &str) -> Result<Value, E> {
        Ok(Value::String(s.to_owned()))
    }

    fn visit_string<E>(self, s: String) -> Result<Value, E> {
        Ok(Value::String(s))
    }

    fn visit_unit<E>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_none<E>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_some<D: serde::Deserializer<'de>>(self, deserializer: D) -> Result<Value, D::Error> {
        self.deserialize(deserializer)
    }

    fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut seq: A) -> Result<Value, A::Error> {
        let inner = self.enter()?;
        let mut items = Vec::new();
        loop {
            match seq.next_element_seed(inner) {
                Ok(Some(item)) => items.push(item),
                Ok(None) => return Ok(Value::Array(items)),
                Err(e) => {
                    let i = items.len();
                    self.unwind(|| OwnedSeg::Index(i));
                    return Err(e);
                }
            }
        }
    }

    fn visit_map<A: serde::de::MapAccess<'de>>(self, mut access: A) -> Result<Value, A::Error> {
        use serde::de::Error;
        let inner = self.enter()?;
        let mut map = Map::new();
        while let Some(key) = access.next_key::<String>()? {
            if map.contains_key(&key) {
                *self.failure.borrow_mut() =
                    Some((StrictFailure::DuplicateKey, vec![OwnedSeg::Key(key)]));
                return Err(A::Error::custom("duplicate object key"));
            }
            match access.next_value_seed(inner) {
                Ok(value) => {
                    map.insert(key, value);
                }
                Err(e) => {
                    self.unwind(|| OwnedSeg::Key(key));
                    return Err(e);
                }
            }
        }
        Ok(Value::Object(map))
    }
}

/// Check that `value` is the number [`SCHEMA_VERSION`].
pub(crate) fn check_version(value: Option<&Value>, path: &str) -> Result<(), WireError> {
    match value.and_then(Value::as_u64) {
        Some(v) if v == u64::from(SCHEMA_VERSION) => Ok(()),
        _ => Err(WireError::Version { path: path.into() }),
    }
}

/// One step of a path into a JSON document.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Seg<'a> {
    Key(&'a str),
    Index(usize),
}

/// The JSON pointer for `path`.
fn pointer(path: &[Seg<'_>]) -> String {
    let mut out = String::new();
    for seg in path {
        out.push('/');
        match seg {
            Seg::Key(key) => out.push_str(key),
            Seg::Index(i) => out.push_str(&i.to_string()),
        }
    }
    out
}

/// Reject every JSON number in `value` whose position `allowed` refuses, and every number at an allowed position
/// that is not an integer in `0..2^32`. The walk is iterative and in document order.
pub(crate) fn check_json_numbers(
    value: &Value,
    allowed: &dyn Fn(&[Seg<'_>]) -> bool,
) -> Result<(), WireError> {
    let mut stack: Vec<(&Value, Vec<Seg<'_>>)> = vec![(value, Vec::new())];
    while let Some((value, path)) = stack.pop() {
        match value {
            Value::Number(n) => {
                let is_index = n.as_u64().is_some_and(|i| u32::try_from(i).is_ok());
                if !(is_index && allowed(&path)) {
                    return Err(WireError::JsonNumberNotAllowed {
                        path: pointer(&path),
                    });
                }
            }
            Value::Array(items) => {
                for (i, item) in items.iter().enumerate().rev() {
                    let mut child = path.clone();
                    child.push(Seg::Index(i));
                    stack.push((item, child));
                }
            }
            Value::Object(map) => {
                for (key, item) in map.iter().rev() {
                    let mut child = path.clone();
                    child.push(Seg::Key(key));
                    stack.push((item, child));
                }
            }
            Value::Null | Value::Bool(_) | Value::String(_) => {}
        }
    }
    Ok(())
}

/// Where a node table document may carry a JSON number: `v`, `root`, and the child indices of Add, Mul, Pow and Fn.
pub(crate) fn table_number_allowed(table: &Value, path: &[Seg<'_>]) -> bool {
    match path {
        [Seg::Key("v")] | [Seg::Key("root")] => true,
        [Seg::Key("nodes"), Seg::Index(i), Seg::Index(j)] => {
            let kind = table
                .get("nodes")
                .and_then(|nodes| nodes.get(*i))
                .and_then(|node| node.get(0))
                .and_then(Value::as_str);
            match kind {
                Some("Add" | "Mul" | "Pow") => *j >= 1,
                Some("Fn") => *j >= 2,
                _ => false,
            }
        }
        _ => false,
    }
}

/// An assumption key: `[a-z][a-z_]*`.
fn is_assumption_key(key: &str) -> bool {
    match key.as_bytes() {
        [first, rest @ ..] => {
            first.is_ascii_lowercase() && rest.iter().all(|b| b.is_ascii_lowercase() || *b == b'_')
        }
        [] => false,
    }
}

/// Check one node's numerals, names and arity.
fn validate_node(i: usize, node: &Node) -> Result<(), WireError> {
    let numeral = |j: usize| WireError::Numeral {
        path: format!("/nodes/{i}/{j}"),
    };
    match node {
        Node::Int(digits) if !numeral::is_int(digits) => Err(numeral(1)),
        Node::Rat(p, _) if !numeral::is_rat_numerator(p) => Err(numeral(1)),
        Node::Rat(_, q) if !numeral::is_rat_denominator(q) => Err(numeral(2)),
        Node::Float { man, exp, prec } if !numeral::is_float(man, exp, prec) => {
            let j = if !numeral::is_int(man) {
                1
            } else if !numeral::is_int(exp) {
                2
            } else {
                3
            };
            Err(numeral(j))
        }
        Node::Sym { name, .. } | Node::Fn { name, .. } if name.is_empty() => Err(shape(
            format!("/nodes/{i}/1"),
            "a name is a non-empty string",
        )),
        Node::Sym { assumptions, .. } => match assumptions.keys().find(|k| !is_assumption_key(k)) {
            Some(key) => Err(shape(
                format!("/nodes/{i}/2/{key}"),
                "an assumption key matches [a-z][a-z_]*",
            )),
            None => Ok(()),
        },
        Node::Add(children) | Node::Mul(children) if children.len() < 2 => Err(shape(
            format!("/nodes/{i}"),
            "Add and Mul take at least two children",
        )),
        _ => Ok(()),
    }
}

/// An expression: a validated node table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Table {
    nodes: Vec<Node>,
    root: NodeId,
}

impl Table {
    /// Build a table, checking every rule of the contract that does not depend on JSON: a non-empty node list, the
    /// numeral grammar, non-empty names, assumption keys, arities, indices in range and no cycles.
    pub fn new(nodes: Vec<Node>, root: NodeId) -> Result<Table, WireError> {
        if nodes.is_empty() {
            return Err(shape("/nodes", "the node list is empty"));
        }
        for (i, node) in nodes.iter().enumerate() {
            validate_node(i, node)?;
        }
        let len = nodes.len();
        if root.index() >= len {
            return Err(WireError::IndexOutOfRange {
                path: "/root".into(),
            });
        }
        for (i, node) in nodes.iter().enumerate() {
            let mut pos = 0;
            while let Some(child) = node.child(pos) {
                if child.index() >= len {
                    let offset = if matches!(node, Node::Fn { .. }) {
                        2
                    } else {
                        1
                    };
                    return Err(WireError::IndexOutOfRange {
                        path: format!("/nodes/{i}/{}", pos + offset),
                    });
                }
                pos += 1;
            }
        }
        check_acyclic(&nodes)?;
        Ok(Table { nodes, root })
    }

    /// The nodes, in table order.
    pub fn nodes(&self) -> &[Node] {
        &self.nodes
    }

    /// The root node.
    pub fn root(&self) -> NodeId {
        self.root
    }

    /// Decode a node table document. Documents above [`MAX_DOCUMENT_BYTES`] are rejected before parsing.
    pub fn from_slice(bytes: &[u8]) -> Result<Table, WireError> {
        Table::from_value(&parse_document(bytes)?)
    }

    /// Decode a parsed node table document.
    pub fn from_value(value: &Value) -> Result<Table, WireError> {
        check_json_numbers(value, &|path| table_number_allowed(value, path))?;
        let obj = value
            .as_object()
            .ok_or_else(|| shape("", "a table is a JSON object"))?;
        if obj.len() != 3 || !["nodes", "root", "v"].iter().all(|k| obj.contains_key(*k)) {
            return Err(shape("", "a table has exactly the keys nodes, root and v"));
        }
        check_version(obj.get("v"), "/v")?;
        let root = decode_index(&obj["root"], "/root")?;
        let list = obj["nodes"]
            .as_array()
            .ok_or_else(|| shape("/nodes", "nodes is a JSON array"))?;
        let nodes = list
            .iter()
            .enumerate()
            .map(|(i, node)| decode_node(node, i))
            .collect::<Result<Vec<_>, _>>()?;
        Table::new(nodes, root)
    }

    /// The canonical table, or `None` if `step` breaks.
    ///
    /// An iterative post-order DFS from the root visits children in argument order. Each node is re-encoded under
    /// canonical child indices and merged with any structurally equal node already emitted, keyed by its encoded
    /// bytes. A node is emitted once, at first completion; unreachable nodes are dropped; the root is last.
    /// `step` is called once each time a reachable input node is first entered.
    pub fn canonicalize_with(&self, step: &mut dyn FnMut() -> ControlFlow<()>) -> Option<Table> {
        let len = self.nodes.len();
        let mut canonical: Vec<Option<NodeId>> = vec![None; len];
        let mut entered = vec![false; len];
        let mut out: Vec<Node> = Vec::new();
        let mut seen: BTreeMap<Vec<u8>, NodeId> = BTreeMap::new();
        let mut stack: Vec<(usize, usize)> = Vec::new();

        let root = self.root.index();
        if step().is_break() {
            return None;
        }
        entered[root] = true;
        stack.push((root, 0));
        while let Some(frame) = stack.last_mut() {
            let (node, pos) = *frame;
            if let Some(child) = self.nodes[node].child(pos) {
                frame.1 += 1;
                let child = child.index();
                // An entered child is already complete: Table::new ruled out cycles.
                if !entered[child] {
                    if step().is_break() {
                        return None;
                    }
                    entered[child] = true;
                    stack.push((child, 0));
                }
            } else {
                stack.pop();
                let mapped = self.nodes[node].map_children(|c| {
                    canonical[c.index()].expect("children complete before their parent")
                });
                let key = to_jcs(&mapped).expect("a node always encodes");
                let id = *seen.entry(key).or_insert_with(|| {
                    out.push(mapped);
                    NodeId((out.len() - 1) as u32)
                });
                canonical[node] = Some(id);
            }
        }
        let root = canonical[root].expect("the root completes last");
        Some(Table { nodes: out, root })
    }

    /// The canonical table (see [`Table::canonicalize_with`]).
    pub fn canonicalize(&self) -> Table {
        self.canonicalize_with(&mut || ControlFlow::Continue(()))
            .expect("an unbounded step budget never breaks")
    }

    /// JCS bytes of `{"nodes":[...],"root":n,"v":1}` for this table as it stands (not canonicalised).
    pub fn to_jcs(&self) -> Vec<u8> {
        to_jcs(self).expect("a table always encodes")
    }

    /// The expression identity: lowercase-hex SHA-256 of the JCS bytes of the canonical table.
    pub fn identity(&self) -> String {
        sha256_hex(&self.canonicalize().to_jcs())
    }
}

impl Serialize for Table {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut doc = serializer.serialize_struct("Table", 3)?;
        doc.serialize_field("nodes", &self.nodes)?;
        doc.serialize_field("root", &self.root.0)?;
        doc.serialize_field("v", &SCHEMA_VERSION)?;
        doc.end()
    }
}

impl<'de> serde::Deserialize<'de> for Table {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Table, D::Error> {
        use serde::de::Error;
        // The same strict parse as Table::from_slice, so no serde path lets a repeated key through.
        let value = strict_document(deserializer)?;
        Table::from_value(&value).map_err(D::Error::custom)
    }
}

/// The strict parse for a serde `Deserialize` impl: [`strict_value`], with a repeated key or excess depth reported
/// as its typed [`WireError`].
pub(crate) fn strict_document<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Value, D::Error> {
    use serde::de::Error;
    strict_value(deserializer).map_err(|(e, strict)| match strict {
        Some(strict) => D::Error::custom(strict),
        None => e,
    })
}

/// Reject any cycle, reachable or not, with an iterative three-colour DFS.
fn check_acyclic(nodes: &[Node]) -> Result<(), WireError> {
    const WHITE: u8 = 0;
    const GREY: u8 = 1;
    const BLACK: u8 = 2;
    let mut colour = vec![WHITE; nodes.len()];
    let mut stack: Vec<(usize, usize)> = Vec::new();
    for start in 0..nodes.len() {
        if colour[start] != WHITE {
            continue;
        }
        colour[start] = GREY;
        stack.push((start, 0));
        while let Some(frame) = stack.last_mut() {
            let (node, pos) = *frame;
            match nodes[node].child(pos) {
                Some(child) => {
                    frame.1 += 1;
                    match colour[child.index()] {
                        WHITE => {
                            colour[child.index()] = GREY;
                            stack.push((child.index(), 0));
                        }
                        GREY => return Err(WireError::Cycle { node: child.0 }),
                        _ => {}
                    }
                }
                None => {
                    colour[node] = BLACK;
                    stack.pop();
                }
            }
        }
    }
    Ok(())
}

fn decode_index(value: &Value, path: &str) -> Result<NodeId, WireError> {
    value
        .as_u64()
        .and_then(|i| u32::try_from(i).ok())
        .map(NodeId)
        .ok_or_else(|| shape(path, "an index is a non-negative integer below 2^32"))
}

fn decode_string<'a>(value: &'a Value, path: &str) -> Result<&'a str, WireError> {
    value
        .as_str()
        .ok_or_else(|| shape(path, "expected a string"))
}

fn decode_children(items: &[Value], first: usize, node: usize) -> Result<Vec<NodeId>, WireError> {
    items
        .iter()
        .enumerate()
        .skip(first)
        .map(|(j, item)| decode_index(item, &format!("/nodes/{node}/{j}")))
        .collect()
}

fn decode_assumptions(value: &Value, path: &str) -> Result<BTreeMap<String, bool>, WireError> {
    let obj: &Map<String, Value> = value
        .as_object()
        .ok_or_else(|| shape(path, "assumptions are a JSON object"))?;
    obj.iter()
        .map(|(key, flag)| match flag {
            Value::Bool(b) => Ok((key.clone(), *b)),
            _ => Err(shape(
                format!("{path}/{key}"),
                "an assumption is a JSON boolean",
            )),
        })
        .collect()
}

fn decode_node(value: &Value, i: usize) -> Result<Node, WireError> {
    let path = format!("/nodes/{i}");
    let items = value
        .as_array()
        .ok_or_else(|| shape(path.as_str(), "a node is a JSON array"))?;
    let kind = items
        .first()
        .ok_or_else(|| shape(path.as_str(), "a node starts with its kind"))?;
    let kind = decode_string(kind, &format!("{path}/0"))?;
    let arity = |n: usize| {
        if items.len() == n {
            Ok(())
        } else {
            Err(shape(
                path.as_str(),
                "wrong number of elements for this kind",
            ))
        }
    };
    let string = |j: usize| decode_string(&items[j], &format!("{path}/{j}")).map(str::to_owned);
    match kind {
        "Int" => {
            arity(2)?;
            Ok(Node::Int(string(1)?))
        }
        "Rat" => {
            arity(3)?;
            Ok(Node::Rat(string(1)?, string(2)?))
        }
        "Float" => {
            arity(4)?;
            Ok(Node::Float {
                man: string(1)?,
                exp: string(2)?,
                prec: string(3)?,
            })
        }
        "Sym" => {
            arity(3)?;
            Ok(Node::Sym {
                name: string(1)?,
                assumptions: decode_assumptions(&items[2], &format!("{path}/2"))?,
            })
        }
        "Atom" => {
            arity(2)?;
            let name = string(1)?;
            Special::from_name(&name)
                .map(Node::Atom)
                .ok_or(WireError::UnknownAtom {
                    path: format!("{path}/1"),
                })
        }
        "Add" => Ok(Node::Add(decode_children(items, 1, i)?)),
        "Mul" => Ok(Node::Mul(decode_children(items, 1, i)?)),
        "Pow" => {
            arity(3)?;
            let children = decode_children(items, 1, i)?;
            Ok(Node::Pow(children[0], children[1]))
        }
        "Fn" => {
            if items.len() < 2 {
                return Err(shape(path.as_str(), "Fn has a name"));
            }
            Ok(Node::Fn {
                name: string(1)?,
                args: decode_children(items, 2, i)?,
            })
        }
        _ => Err(WireError::UnknownKind {
            path: format!("{path}/0"),
        }),
    }
}
