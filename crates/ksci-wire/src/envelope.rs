// provenance: original
//! Request, response and record envelopes (DET-03).
//!
//! Only request and response bytes are hashed. Provenance (implementation, backend, device, driver, wall time)
//! lives only in [`Record`], next to the two digests and never inside the hashed bytes: [`Request`] and
//! [`Response`] deny unknown fields, so a `provenance` key cannot enter them.
//!
//! Every envelope keeps its rules by construction (review F3). Fields are private, and the constructors build only
//! valid values. Every decode path reads a private wire form and turns it into a value with one check per type,
//! [`Wire::check`]. That covers the byte decoders and serde's `Deserialize` alike. So no public path builds,
//! deserialises or records an envelope that breaks `schema/expr.v1.md`.

use std::collections::BTreeMap;

use serde::de::{DeserializeOwned, Error as _};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

use crate::{
    SCHEMA_VERSION, Seg, Table, WireError, check_json_numbers, numeral, parse_document, sha256_hex,
    shape, strict_document, table_number_allowed, to_jcs,
};

/// A wire form: an envelope exactly as the schema spells it, read by serde, and the one check that turns it into a
/// value. Wire forms are private, so a value exists only after its check passed.
trait Wire: DeserializeOwned {
    /// The checked value.
    type Checked;
    /// Apply every rule the type system does not already enforce.
    fn check(self) -> Result<Self::Checked, WireError>;
}

/// JSON-number positions, then the wire shape, then its [`Wire::check`]. Every decode path goes through here.
fn decode<W: Wire>(doc: Value) -> Result<W::Checked, WireError> {
    check_json_numbers(&doc, &|path| envelope_number_allowed(&doc, path))?;
    let wire: W = serde_json::from_value(doc).map_err(|e| WireError::Envelope(e.to_string()))?;
    wire.check()
}

/// The byte decoder: size cap and strict parse, then [`decode`], then the canonical-form check. The bytes must be the
/// value's own JCS encoding.
fn decode_canonical<W: Wire>(bytes: &[u8]) -> Result<W::Checked, WireError>
where
    W::Checked: Serialize,
{
    let value = decode::<W>(parse_document(bytes)?)?;
    if to_jcs(&value)? != bytes {
        return Err(WireError::NonCanonical);
    }
    Ok(value)
}

/// serde's entry point: the strict parse (repeated keys, depth), then [`decode`]. Only the canonical-form check is
/// left out, because serde does not expose the original bytes. A value read this way still encodes to its one
/// canonical byte string.
fn deserialize_checked<'de, D: Deserializer<'de>, W: Wire>(
    deserializer: D,
) -> Result<W::Checked, D::Error> {
    decode::<W>(strict_document(deserializer)?).map_err(D::Error::custom)
}

/// A `u64` on the wire: its decimal numeral, as a string.
fn serialize_decimal<S: Serializer>(n: &u64, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.collect_str(n)
}

/// A request: `{"args":{name: table},"budget":{"steps":"<decimal>"},"op":"...","v":1}`.
///
/// The fields are private, so a request cannot be edited out of its rules (review F3):
///
/// ```compile_fail,E0616
/// fn edit(request: &mut ksci_wire::Request) {
///     request.v = 2;
/// }
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Request {
    v: u32,
    op: String,
    args: BTreeMap<String, Table>,
    budget: Budget,
}

/// The logical step budget of a request. On the wire, `{"steps":"<decimal>"}`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Budget {
    #[serde(serialize_with = "serialize_decimal")]
    steps: u64,
}

impl Budget {
    /// A budget of `steps` logical steps.
    pub const fn new(steps: u64) -> Budget {
        Budget { steps }
    }

    /// The step count.
    pub const fn steps(self) -> u64 {
        self.steps
    }
}

/// A request as the schema spells it.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RequestWire {
    v: u32,
    op: String,
    args: BTreeMap<String, Table>,
    budget: BudgetWire,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BudgetWire {
    steps: String,
}

impl Wire for RequestWire {
    type Checked = Request;

    /// `v` is 1 and the step budget is a step numeral. Every argument table was checked when it was read.
    fn check(self) -> Result<Request, WireError> {
        if self.v != SCHEMA_VERSION {
            return Err(WireError::Version { path: "/v".into() });
        }
        let steps = numeral::parse_u64(&self.budget.steps).ok_or_else(|| WireError::Numeral {
            path: "/budget/steps".into(),
        })?;
        Ok(Request::new(self.op, self.args, Budget::new(steps)))
    }
}

impl Request {
    /// A request for `op` on `args` within `budget`. Every value of these types makes a valid request.
    pub fn new(op: impl Into<String>, args: BTreeMap<String, Table>, budget: Budget) -> Request {
        Request {
            v: SCHEMA_VERSION,
            op: op.into(),
            args,
            budget,
        }
    }

    /// The operation name.
    pub fn op(&self) -> &str {
        &self.op
    }

    /// The named expression arguments.
    pub fn args(&self) -> &BTreeMap<String, Table> {
        &self.args
    }

    /// The logical step budget.
    pub fn budget(&self) -> Budget {
        self.budget
    }

    /// The request's JCS bytes, the bytes that are hashed.
    pub fn to_jcs(&self) -> Vec<u8> {
        to_jcs(self).expect("a request always encodes")
    }

    /// Decode a request that must already be in canonical form.
    ///
    /// The size cap is checked before parsing. Every argument table goes through the strict decoder, `v` must be 1
    /// and the step budget must parse. Finally the request is re-encoded with [`to_jcs`]; if those bytes differ
    /// from the input (key order, whitespace, escapes), the request is [`WireError::NonCanonical`].
    /// So a request digest is unique per request.
    pub fn from_canonical_slice(bytes: &[u8]) -> Result<Request, WireError> {
        decode_canonical::<RequestWire>(bytes)
    }
}

impl<'de> Deserialize<'de> for Request {
    /// Every rule of [`Request::from_canonical_slice`] except the canonical-form byte check.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Request, D::Error> {
        deserialize_checked::<D, RequestWire>(deserializer)
    }
}

/// How a request ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// An exact result.
    Exact,
    /// The logical step budget ran out.
    FuelExhausted,
}

/// A response: `{"identity":"<hex>"|null,"numeric_class":"exact","result":<table>|null,"status":"...","v":1}`.
///
/// The only constructors are [`Response::exact`], [`Response::fuel_exhausted`] and the decoders, which check every
/// response rule. The fields are private, so a response that breaks the rules cannot be built (review F3):
///
/// ```compile_fail,E0451
/// use ksci_wire::{Response, Status, Table};
/// let result = Table::from_slice(br#"{"nodes":[["Int","1"]],"root":0,"v":1}"#).unwrap();
/// let response = Response {
///     v: 1,
///     status: Status::Exact,
///     numeric_class: "approx".into(),
///     result: Some(result),
///     identity: Some("0".repeat(64)),
/// };
/// ```
///
/// nor can a valid one be edited into one that does:
///
/// ```compile_fail,E0616
/// fn edit(response: &mut ksci_wire::Response) {
///     response.numeric_class = "approx".into();
/// }
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Response {
    v: u32,
    status: Status,
    numeric_class: String,
    result: Option<Table>,
    identity: Option<String>,
}

/// The only `numeric_class` in Phase 1.
const NUMERIC_CLASS_EXACT: &str = "exact";

fn invalid(path: &str, reason: &'static str) -> WireError {
    WireError::InvalidResponse {
        path: path.into(),
        reason,
    }
}

/// A response as the schema spells it. `result` and `identity` are required keys: a missing key is not `null`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ResponseWire {
    v: u32,
    status: Status,
    numeric_class: String,
    #[serde(deserialize_with = "Option::deserialize")]
    result: Option<Table>,
    #[serde(deserialize_with = "Option::deserialize")]
    identity: Option<String>,
}

impl Wire for ResponseWire {
    type Checked = Response;

    /// Check every response rule of `schema/expr.v1.md`:
    ///
    /// - `v` is 1 and `numeric_class` is `exact`;
    /// - status `exact`: `result` is present and in canonical form, and `identity` is its identity;
    /// - status `fuel_exhausted`: `result` and `identity` are both `null`.
    fn check(self) -> Result<Response, WireError> {
        if self.v != SCHEMA_VERSION {
            return Err(WireError::Version { path: "/v".into() });
        }
        if self.numeric_class != NUMERIC_CLASS_EXACT {
            return Err(invalid(
                "/numeric_class",
                "numeric_class is \"exact\" in Phase 1",
            ));
        }
        match (self.status, &self.result, &self.identity) {
            (Status::Exact, None, _) => {
                return Err(invalid("/result", "status exact carries a result"));
            }
            (Status::Exact, Some(_), None) => {
                return Err(invalid(
                    "/identity",
                    "status exact carries the result's identity",
                ));
            }
            (Status::Exact, Some(result), Some(claimed)) => {
                let canonical = result.canonicalize();
                if &canonical != result {
                    return Err(invalid("/result", "the result is in canonical form"));
                }
                let actual = sha256_hex(&canonical.to_jcs());
                if claimed != &actual {
                    return Err(WireError::IdentityMismatch {
                        claimed: claimed.clone(),
                        actual,
                    });
                }
            }
            (Status::FuelExhausted, Some(_), _) => {
                return Err(invalid(
                    "/result",
                    "status fuel_exhausted carries no result",
                ));
            }
            (Status::FuelExhausted, None, Some(_)) => {
                return Err(invalid(
                    "/identity",
                    "status fuel_exhausted carries no identity",
                ));
            }
            (Status::FuelExhausted, None, None) => {}
        }
        Ok(Response {
            v: self.v,
            status: self.status,
            numeric_class: self.numeric_class,
            result: self.result,
            identity: self.identity,
        })
    }
}

impl Response {
    /// An exact response: `result` in canonical form and its identity.
    pub fn exact(result: &Table) -> Response {
        let result = result.canonicalize();
        Response {
            v: SCHEMA_VERSION,
            status: Status::Exact,
            numeric_class: NUMERIC_CLASS_EXACT.into(),
            identity: Some(result.identity()),
            result: Some(result),
        }
    }

    /// The response for a request whose logical step budget ran out: no result and no identity.
    pub fn fuel_exhausted() -> Response {
        Response {
            v: SCHEMA_VERSION,
            status: Status::FuelExhausted,
            numeric_class: NUMERIC_CLASS_EXACT.into(),
            result: None,
            identity: None,
        }
    }

    /// How the request ended.
    pub fn status(&self) -> Status {
        self.status
    }

    /// The numeric class, `exact` in Phase 1.
    pub fn numeric_class(&self) -> &str {
        &self.numeric_class
    }

    /// The result table in canonical form: present exactly when the status is [`Status::Exact`].
    pub fn result(&self) -> Option<&Table> {
        self.result.as_ref()
    }

    /// The result's identity: present exactly when the status is [`Status::Exact`].
    pub fn identity(&self) -> Option<&str> {
        self.identity.as_deref()
    }

    /// The response's JCS bytes, the bytes that are hashed.
    pub fn to_jcs(&self) -> Vec<u8> {
        to_jcs(self).expect("a response always encodes")
    }

    /// Decode a response, which must satisfy every response rule and be in canonical form (as
    /// [`Response::to_jcs`] writes it).
    pub fn from_slice(bytes: &[u8]) -> Result<Response, WireError> {
        decode_canonical::<ResponseWire>(bytes)
    }
}

impl<'de> Deserialize<'de> for Response {
    /// Every rule of [`Response::from_slice`] except the canonical-form byte check.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Response, D::Error> {
        deserialize_checked::<D, ResponseWire>(deserializer)
    }
}

/// Where and how a response was produced. Every field is a string on the wire: no numbers go on the wire.
///
/// Every value of every field is valid, so the fields are public.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Provenance {
    /// The implementation (for example R0 or R2).
    #[serde(rename = "impl")]
    pub impl_: String,
    /// The backend.
    pub backend: String,
    /// The device.
    pub device: String,
    /// The driver.
    pub driver: String,
    /// Wall time in milliseconds. On the wire it is a decimal numeral string, `0|[1-9][0-9]*`.
    #[serde(
        serialize_with = "serialize_decimal",
        deserialize_with = "deserialize_wall_ms"
    )]
    pub wall_ms: u64,
}

fn deserialize_wall_ms<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u64, D::Error> {
    let digits = String::deserialize(deserializer)?;
    numeral::parse_u64(&digits)
        .ok_or_else(|| D::Error::custom("bad wall_ms: a decimal numeral 0|[1-9][0-9]* within u64"))
}

/// A record of one request: the two digests, and provenance outside them.
///
/// A record is built from a decoded request and response, never from raw bytes, so it cannot hash bytes that break
/// the envelope rules (review F3):
///
/// ```compile_fail,E0308
/// fn record(provenance: ksci_wire::Provenance) -> ksci_wire::Record {
///     ksci_wire::Record::new(b"{}", b"not a response", provenance)
/// }
/// ```
///
/// and its digests cannot be overwritten:
///
/// ```compile_fail,E0616
/// fn edit(record: &mut ksci_wire::Record) {
///     record.response_digest = "not a digest".into();
/// }
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Record {
    request_digest: String,
    response_digest: String,
    provenance: Provenance,
}

/// A record as the schema spells it.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RecordWire {
    request_digest: String,
    response_digest: String,
    provenance: Provenance,
}

/// 64 lowercase hex digits, as [`sha256_hex`] writes them.
fn is_digest(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

impl Wire for RecordWire {
    type Checked = Record;

    /// Both digests are 64 lowercase hex digits. A record read back cannot be checked against bytes it does not
    /// carry, so only the digest shape is checked.
    fn check(self) -> Result<Record, WireError> {
        for (path, digest) in [
            ("/request_digest", &self.request_digest),
            ("/response_digest", &self.response_digest),
        ] {
            if !is_digest(digest) {
                return Err(shape(path, "a digest is 64 lowercase hex digits"));
            }
        }
        Ok(Record {
            request_digest: self.request_digest,
            response_digest: self.response_digest,
            provenance: self.provenance,
        })
    }
}

impl Record {
    /// Build a record from a request and its response: the digests of their JCS bytes, the bytes that were sent.
    ///
    /// Both are checked values, so a record never hashes bytes that break the envelope rules. For a request read by
    /// [`Request::from_canonical_slice`] and a response read by [`Response::from_slice`], the JCS bytes are exactly
    /// the bytes that were read.
    pub fn new(request: &Request, response: &Response, provenance: Provenance) -> Record {
        Record {
            request_digest: sha256_hex(&request.to_jcs()),
            response_digest: sha256_hex(&response.to_jcs()),
            provenance,
        }
    }

    /// [`sha256_hex`] of the request bytes.
    pub fn request_digest(&self) -> &str {
        &self.request_digest
    }

    /// [`sha256_hex`] of the response bytes.
    pub fn response_digest(&self) -> &str {
        &self.response_digest
    }

    /// Provenance, which never changes either digest.
    pub fn provenance(&self) -> &Provenance {
        &self.provenance
    }
}

impl<'de> Deserialize<'de> for Record {
    /// The strict parse, then both digests are checked for shape.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Record, D::Error> {
        deserialize_checked::<D, RecordWire>(deserializer)
    }
}

/// Where an envelope may carry a JSON number: `v`, and the table positions inside `args/<name>` (requests) or
/// `result` (responses). A stray `args` or `result` key is rejected afterwards by `deny_unknown_fields`. Records carry
/// no numbers.
fn envelope_number_allowed(doc: &Value, path: &[Seg<'_>]) -> bool {
    match path {
        [Seg::Key("v")] => true,
        [Seg::Key("args"), Seg::Key(name), rest @ ..] => doc
            .get("args")
            .and_then(|args| args.get(*name))
            .is_some_and(|table| table_number_allowed(table, rest)),
        [Seg::Key("result"), rest @ ..] => doc
            .get("result")
            .is_some_and(|table| table_number_allowed(table, rest)),
        _ => false,
    }
}
