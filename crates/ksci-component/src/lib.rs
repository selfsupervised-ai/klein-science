// provenance: original
//! `ksci-component`: the R0 shell. It exports the `cas` interface of the `science-cas` world
//! (`wit/science-cas.wit`) and holds no logic: each export calls the `ksci_ops` function of the same
//! name, and [`OpError`] maps one to one onto the WIT `error` variant.
//!
//! The release build is std Rust for `wasm32-unknown-unknown`, wrapped by `wasm-tools component new`
//! (`cargo xtask release dist`), so the component imports nothing (D-16).

use ksci_ops::OpError;

wit_bindgen::generate!({ world: "science-cas", path: "../../wit" });

use exports::klein::science::cas::{Error, Guest};

/// The `cas` exports.
struct Cas;

impl Guest for Cas {
    fn invoke(request_jcs: Vec<u8>) -> Result<Vec<u8>, Error> {
        ksci_ops::invoke(&request_jcs).map_err(to_wit)
    }

    fn plan(request_jcs: Vec<u8>) -> Result<Vec<Vec<u8>>, Error> {
        ksci_ops::plan(&request_jcs).map_err(to_wit)
    }

    fn map(piece: Vec<u8>) -> Result<Vec<u8>, Error> {
        ksci_ops::map(&piece).map_err(to_wit)
    }

    fn reduce(request_jcs: Vec<u8>, outputs: Vec<Vec<u8>>) -> Result<Vec<u8>, Error> {
        ksci_ops::reduce(&request_jcs, &outputs).map_err(to_wit)
    }
}

/// The WIT variant of an [`OpError`]: same kind, same message.
fn to_wit(error: OpError) -> Error {
    let message = error.message().to_owned();
    match error {
        OpError::BadRequest(_) => Error::BadRequest(message),
        OpError::UnsupportedOp(_) => Error::UnsupportedOp(message),
        OpError::Internal(_) => Error::Internal(message),
    }
}

// The exported symbols are the component's core-module ABI, so only a wasm32 build defines them.
// Their component-model names (`klein:science/cas@0.1.0#invoke`) also break a native link: the
// macOS linker's exported-symbols list cuts them at the `#`. The bindings and the `Guest` impl
// above still compile and lint on every target, and the wasm32 output is unchanged.
#[cfg(target_arch = "wasm32")]
export!(Cas);
