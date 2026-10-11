// provenance: original
//! Request, response and record envelopes (DET-03).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{Table, WireError};

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
    /// Steps, a decimal numeral within `u64`.
    pub steps: String,
}

impl Budget {
    /// The step count.
    pub fn steps_u64(&self) -> Option<u64> {
        None
    }
}

impl Request {
    /// Decode a request that must already be in canonical (JCS) form.
    pub fn from_canonical_slice(_bytes: &[u8]) -> Result<Request, WireError> {
        Err(WireError::Encode("unimplemented".into()))
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

/// A response.
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
    /// The response's JCS bytes.
    pub fn to_jcs(&self) -> Vec<u8> {
        Vec::new()
    }

    /// Decode a response.
    pub fn from_slice(_bytes: &[u8]) -> Result<Response, WireError> {
        Err(WireError::Encode("unimplemented".into()))
    }
}

/// Where and how a response was produced.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Provenance {
    /// The implementation.
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

/// A record of one request.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    /// Digest of the request bytes.
    pub request_digest: String,
    /// Digest of the response bytes.
    pub response_digest: String,
    /// Provenance.
    pub provenance: Provenance,
}

impl Record {
    /// Build a record.
    pub fn new(_request_jcs: &[u8], _response_jcs: &[u8], provenance: Provenance) -> Record {
        Record {
            request_digest: String::new(),
            response_digest: String::new(),
            provenance,
        }
    }
}
