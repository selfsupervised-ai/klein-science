// provenance: original
//! R0 (the shipped component, under each Wasmtime pin) against R2 (the shipped worker), byte for
//! byte, on the request corpus `testdata/wire/requests.jsonl`.
//!
//! The artifacts come from `cargo xtask release dist` (`$KSCI_DIST`); a missing or stale artifact
//! fails the test and names that command.

use std::fs;
use std::path::Path;

use ksci_conform::{Dist, Limits, Pin, R0, R2Worker};

/// The shipped artifacts, or a panic that says how to build them.
fn dist() -> Dist {
    Dist::open().unwrap_or_else(|e| panic!("{e}; run: cargo xtask release dist"))
}

/// The request corpus, one canonical request per line.
fn corpus() -> Vec<Vec<u8>> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/wire/requests.jsonl");
    let text = fs::read(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let lines: Vec<Vec<u8>> = text
        .split(|b| *b == b'\n')
        .filter(|l| !l.is_empty())
        .map(<[u8]>::to_vec)
        .collect();
    assert!(!lines.is_empty(), "{} is empty", path.display());
    lines
}

#[test]
fn r0_r2_canonicalize_identical() {
    let dist = dist();
    let corpus = corpus();
    let mut r2 = R2Worker::spawn(&dist.worker).expect("start the R2 worker");
    let expected: Vec<_> = corpus
        .iter()
        .map(|request| r2.invoke(request).expect("R2 answers every request"))
        .collect();
    for pin in Pin::ALL {
        let r0 = R0::load(pin, &dist.component).expect("load the R0 component");
        for (line, (request, r2_outcome)) in corpus.iter().zip(&expected).enumerate() {
            let r0_outcome = r0.invoke(request, Limits::GENEROUS).unwrap_or_else(|e| {
                panic!("R0 {} line {}: host error {e:?}", pin.version(), line + 1)
            });
            assert_eq!(
                &r0_outcome,
                r2_outcome,
                "R0 (wasmtime {}) and R2 differ on testdata/wire/requests.jsonl line {}",
                pin.version(),
                line + 1
            );
        }
    }
}
