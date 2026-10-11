// provenance: original
//! Request, response and record envelopes (DET-03).
//!
//! Only request and response bytes are hashed. Provenance (implementation, backend, device, driver, wall time)
//! lives only in [`Record`], next to the two digests and never inside the hashed bytes: [`Request`] and
//! [`Response`] deny unknown fields, so a `provenance` key cannot enter them.

use std::collections::BTreeMap;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    SCHEMA_VERSION, Seg, Table, WireError, check_json_numbers, numeral, parse_document, sha256_hex,
    table_number_allowed, to_jcs,
};

/// A request: `{"args":{name: table},"budget":{"steps":"<decimal>"},"op":"...","v":1}`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    /// Schema version, exactly 1.
    pub v: u32,
    /// The operation name.
    pub op: String,
    /// Named expression arguments.
    pub args: BTreeMap<String, Table>,
    /// The logical step budget.
    pub budget: Budget,
}

/// The logical step budget of a request.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Budget {
    /// Steps, a decimal numeral `0|[1-9][0-9]*` within `u64`.
    pub steps: String,
}

impl Budget {
    /// The step count, or `None` if `steps` is not a valid step numeral.
    pub fn steps_u64(&self) -> Option<u64> {
        numeral::parse_steps(&self.steps)
    }
}

impl Request {
    /// Decode a request that must already be in canonical form.
    ///
    /// The size cap is checked before parsing. Every argument table goes through the strict decoder, `v` must be 1
    /// and the step budget must parse. Finally the request is re-encoded with [`to_jcs`]; if those bytes differ
    /// from the input (key order, whitespace, duplicate keys, escapes), the request is [`WireError::NonCanonical`].
    /// So a request digest is unique per request.
    pub fn from_canonical_slice(bytes: &[u8]) -> Result<Request, WireError> {
        let request: Request = decode_canonical(bytes)?;
        if request.v != SCHEMA_VERSION {
            return Err(WireError::Version { path: "/v".into() });
        }
        if request.budget.steps_u64().is_none() {
            return Err(WireError::Numeral {
                path: "/budget/steps".into(),
            });
        }
        Ok(request)
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
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Response {
    /// Schema version, exactly 1.
    pub v: u32,
    /// How the request ended.
    pub status: Status,
    /// Always "exact" in Phase 1.
    pub numeric_class: String,
    /// The result table, if any.
    pub result: Option<Table>,
    /// The result's identity, if any.
    pub identity: Option<String>,
}

impl Response {
    /// An exact response carrying `result` in canonical form and its identity.
    pub fn exact(_result: &Table) -> Response {
        unimplemented!("review F1 skeleton")
    }

    /// A response for a request whose step budget ran out.
    pub fn fuel_exhausted() -> Response {
        unimplemented!("review F1 skeleton")
    }

    /// The response's JCS bytes, the bytes that are hashed.
    pub fn to_jcs(&self) -> Vec<u8> {
        to_jcs(self).expect("a response always encodes")
    }

    /// Decode a response, which must be in canonical form (as [`Response::to_jcs`] writes it).
    pub fn from_slice(bytes: &[u8]) -> Result<Response, WireError> {
        let response: Response = decode_canonical(bytes)?;
        if response.v != SCHEMA_VERSION {
            return Err(WireError::Version { path: "/v".into() });
        }
        Ok(response)
    }
}

/// Where and how a response was produced. Every field is a string: no numbers go on the wire.
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
    /// Wall time in milliseconds, as a decimal string.
    pub wall_ms: String,
}

/// A record of one request: the two digests, and provenance outside them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    /// [`sha256_hex`] of the request bytes.
    pub request_digest: String,
    /// [`sha256_hex`] of the response bytes.
    pub response_digest: String,
    /// Provenance, which never changes either digest.
    pub provenance: Provenance,
}

impl Record {
    /// Build a record from the request and response bytes as they were sent.
    pub fn new(request_jcs: &[u8], response_jcs: &[u8], provenance: Provenance) -> Record {
        Record {
            request_digest: sha256_hex(request_jcs),
            response_digest: sha256_hex(response_jcs),
            provenance,
        }
    }
}

/// Where an envelope may carry a JSON number: `v`, and the table positions inside `args/<name>` (requests) or
/// `result` (responses). A stray `args` or `result` key is rejected afterwards by `deny_unknown_fields`.
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

/// Size cap, parse, JSON-number positions, schema, then the byte-for-byte canonical-form check.
fn decode_canonical<T: Serialize + DeserializeOwned>(bytes: &[u8]) -> Result<T, WireError> {
    let doc = parse_document(bytes)?;
    check_json_numbers(&doc, &|path| envelope_number_allowed(&doc, path))?;
    let decoded: T = serde_json::from_value(doc).map_err(|e| WireError::Envelope(e.to_string()))?;
    if to_jcs(&decoded)? != bytes {
        return Err(WireError::NonCanonical);
    }
    Ok(decoded)
}
