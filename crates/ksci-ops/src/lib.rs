// provenance: original
//! `ksci-ops`: the op layer, and the only crate that knows the operation set.
//!
//! Both shells call it with the same bytes: the R0 WASM component and the R2 native worker (`ksci-worker`). A shell
//! decodes a frame, calls [`invoke`] and encodes the result. It holds no logic of its own, so R0 and R2 return the
//! same response bytes for the same request bytes.
//!
//! The logical step budget ([`StepBudget`]) is the only source of a `fuel_exhausted` response. Host limits (Wasmtime
//! fuel, memory, wall time) are host errors and never decide a result status.

use std::fmt::{self, Write as _};
use std::ops::ControlFlow;

use ksci_wire::{MAX_DOCUMENT_BYTES, Request, Response};

/// The longest error message, in bytes (schema/expr.v1.md, Errors).
pub const MAX_ERROR_MESSAGE_BYTES: usize = 1024;

/// What a cut message ends with.
const TRUNCATED: &str = " [truncated]";

/// An error message of at most [`MAX_ERROR_MESSAGE_BYTES`] bytes (review W1).
///
/// Error text can quote the request (a JSON pointer names a key, an unknown op is named), and a request may be
/// 64 MiB. So a text longer than the bound keeps its longest prefix that ends on a character boundary and leaves room
/// for `" [truncated]"`, which is appended. The cut depends only on the text, so one request always gives one error
/// byte string. The field is private: every message, and so every error a shell returns, is bounded by construction.
///
/// What is bounded is this message. The cut happens while `Message::new` formats its argument, so formatting never
/// builds an oversized copy here. But the value being formatted can already hold one: a `WireError` built upstream
/// in ksci-wire carries its JSON pointer as a `String`, which may be as long as the request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Message(String);

impl Message {
    /// The bounded message for `text`.
    pub fn new(text: impl fmt::Display) -> Message {
        let mut out = Bounded {
            text: String::new(),
            cut: false,
        };
        write!(out, "{text}").expect("Bounded never fails");
        if out.cut {
            let keep = out
                .text
                .floor_char_boundary(MAX_ERROR_MESSAGE_BYTES - TRUNCATED.len());
            out.text.truncate(keep);
            out.text.push_str(TRUNCATED);
        }
        Message(out.text)
    }

    /// The message text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Message {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for Message {
    fn from(text: &str) -> Message {
        Message::new(text)
    }
}

impl From<String> for Message {
    fn from(text: String) -> Message {
        Message::new(text)
    }
}

/// A formatter sink that keeps at most [`MAX_ERROR_MESSAGE_BYTES`] bytes, cut on a character boundary, and records
/// whether anything was dropped.
struct Bounded {
    text: String,
    cut: bool,
}

impl fmt::Write for Bounded {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        if self.cut {
            // Nothing after a cut reaches the message: Message::new keeps at most MAX - " [truncated]" bytes, and a
            // cut leaves at least MAX - 3. Stop early so the kept text is plainly a prefix.
            return Ok(());
        }
        let room = MAX_ERROR_MESSAGE_BYTES - self.text.len();
        if s.len() <= room {
            self.text.push_str(s);
        } else {
            self.text.push_str(&s[..s.floor_char_boundary(room)]);
            self.cut = true;
        }
        Ok(())
    }
}

/// Why an operation produced no response.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum OpError {
    /// The request is malformed, non-canonical, or its arguments do not fit the operation.
    #[error("bad request: {0}")]
    BadRequest(Message),
    /// The request names an operation that is not in the registry.
    #[error("unsupported op: {0}")]
    UnsupportedOp(Message),
    /// The operation failed on a valid request.
    #[error("internal error: {0}")]
    Internal(Message),
}

impl OpError {
    /// The message without its kind, for a shell that carries the kind separately (a frame tag, a WIT variant). At
    /// most [`MAX_ERROR_MESSAGE_BYTES`] bytes.
    pub fn message(&self) -> &str {
        match self {
            OpError::BadRequest(m) | OpError::UnsupportedOp(m) | OpError::Internal(m) => m.as_str(),
        }
    }
}

/// The logical step budget of one request.
///
/// An operation charges one step per unit of logical work; `science.cas.canonicalize` charges one step each time
/// it first enters a reachable input node. When a charge finds no step left the operation stops and the response is
/// `fuel_exhausted`. The count depends only on the request bytes, so every implementation stops at the same point.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StepBudget {
    remaining: u64,
}

impl StepBudget {
    /// A budget of `steps` steps.
    pub const fn new(steps: u64) -> StepBudget {
        StepBudget { remaining: steps }
    }

    /// Spend one step. Breaks, and spends nothing, when no step remains.
    pub fn charge(&mut self) -> ControlFlow<()> {
        match self.remaining.checked_sub(1) {
            Some(rest) => {
                self.remaining = rest;
                ControlFlow::Continue(())
            }
            None => ControlFlow::Break(()),
        }
    }

    /// The steps not yet spent.
    pub const fn remaining(&self) -> u64 {
        self.remaining
    }
}

/// An operation: a decoded request and its budget in, a response out.
type Op = fn(&Request, &mut StepBudget) -> Result<Response, OpError>;

/// The operation registry. An operation exists exactly when it is listed here.
const OPS: &[(&str, Op)] = &[("science.cas.canonicalize", canonicalize)];

fn lookup(name: &str) -> Result<Op, OpError> {
    OPS.iter()
        .find(|(n, _)| *n == name)
        .map(|(_, op)| *op)
        .ok_or_else(|| OpError::UnsupportedOp(Message::new(name)))
}

/// Decode a request, which must be in canonical form, so that one request has one byte string and one digest.
fn decode(request_jcs: &[u8]) -> Result<Request, OpError> {
    Request::from_canonical_slice(request_jcs).map_err(|e| OpError::BadRequest(Message::new(e)))
}

/// Run a request: decode it, look up its operation, run it within the request's step budget and return the
/// response's JCS bytes.
pub fn invoke(request_jcs: &[u8]) -> Result<Vec<u8>, OpError> {
    let request = decode(request_jcs)?;
    let op = lookup(request.op())?;
    let mut budget = StepBudget::new(request.budget().steps());
    let response = op(&request, &mut budget)?.to_jcs();
    if response.len() > MAX_DOCUMENT_BYTES {
        return Err(OpError::Internal(Message::new(format_args!(
            "the response is {} bytes, above the {MAX_DOCUMENT_BYTES}-byte cap",
            response.len()
        ))));
    }
    Ok(response)
}

/// Split a request into pieces for `map`. Phase 1 operations do not split: the one piece is the request itself.
///
/// The request is checked as [`invoke`] checks it (canonical form, a known operation), so a bad request fails here
/// rather than in every piece.
pub fn plan(request_jcs: &[u8]) -> Result<Vec<Vec<u8>>, OpError> {
    lookup(decode(request_jcs)?.op())?;
    Ok(vec![request_jcs.to_vec()])
}

/// Run one piece. A piece is a request, so this is [`invoke`].
pub fn map(piece: &[u8]) -> Result<Vec<u8>, OpError> {
    invoke(piece)
}

/// Combine the outputs of the pieces of `request_jcs`. [`plan`] makes exactly one piece, so there must be exactly
/// one output, a valid response, and it is the result.
pub fn reduce(request_jcs: &[u8], outputs: &[Vec<u8>]) -> Result<Vec<u8>, OpError> {
    decode(request_jcs)?;
    let [output] = outputs else {
        return Err(OpError::BadRequest(Message::new(format_args!(
            "reduce takes exactly one output, got {}",
            outputs.len()
        ))));
    };
    Response::from_slice(output).map_err(|e| {
        OpError::BadRequest(Message::new(format_args!(
            "the output is not a response: {e}"
        )))
    })?;
    Ok(output.clone())
}

/// `science.cas.canonicalize`: the canonical table of `args.expr` and its identity.
fn canonicalize(request: &Request, budget: &mut StepBudget) -> Result<Response, OpError> {
    let args = request.args();
    let expr = match args.get("expr") {
        Some(expr) if args.len() == 1 => expr,
        _ => {
            return Err(OpError::BadRequest(
                "science.cas.canonicalize takes exactly one argument, expr".into(),
            ));
        }
    };
    Ok(match expr.canonicalize_with(&mut || budget.charge()) {
        Some(table) => Response::exact(&table),
        None => Response::fuel_exhausted(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ksci_wire::{Status, Table};

    /// `x**2 + 1` with its nodes out of canonical order: the root is first and `x` is last.
    const SHUFFLED: &str = r#"{"nodes":[["Add",3,1],["Int","1"],["Int","2"],["Pow",4,2],["Sym","x",{}]],"root":0,"v":1}"#;

    #[test]
    fn ops_canonicalize_tracer() {
        let request = format!(
            r#"{{"args":{{"expr":{SHUFFLED}}},"budget":{{"steps":"100"}},"op":"science.cas.canonicalize","v":1}}"#
        );
        let bytes =
            invoke(request.as_bytes()).expect("a canonical request for a known op succeeds");
        let response =
            Response::from_slice(&bytes).expect("invoke returns a valid canonical response");
        let input = Table::from_slice(SHUFFLED.as_bytes()).unwrap();

        assert_eq!(response.status(), Status::Exact);
        assert_eq!(response.result(), Some(&input.canonicalize()));
        assert_eq!(response.identity(), Some(input.identity().as_str()));
        // The identity schema/expr.v1.md gives for x**2 + 1, computed outside Rust with shasum -a 256.
        assert_eq!(
            response.identity(),
            Some("37f7939bd191aa48173b1b6a45838b6b246aeb8a001d276f9b1c2798342cb964")
        );
    }
}
