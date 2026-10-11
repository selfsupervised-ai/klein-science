// provenance: original
//! R0 (the shipped component, under each Wasmtime pin) against R2 (the shipped worker), byte for
//! byte, on the request corpus `testdata/wire/requests.jsonl`; and the DET-04 contract: the logical
//! step budget gives the same `fuel_exhausted` bytes everywhere, while Wasmtime fuel and the memory
//! cap are host errors, never responses.
//!
//! The artifacts come from `cargo xtask release dist` (`$KSCI_DIST`); a missing or stale artifact
//! fails the test and names that command.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use ksci_conform::{Dist, HostError, Limits, Outcome, Pin, R0, R2Worker};
use ksci_wire::{Budget, Node, NodeId, Request, Response, Table};

/// The Wasmtime versions R0 must run under: Klein's current pin and the D6 floor (D-17).
const PIN_VERSIONS: [&str; 2] = ["46.0.1", "49.0.2"];

/// The hosted pin of each required version, or a panic naming the version with no host.
fn pins() -> [Pin; 2] {
    PIN_VERSIONS.map(|version| {
        Pin::ALL
            .into_iter()
            .find(|pin| pin.version() == version)
            .unwrap_or_else(|| panic!("no R0 host for wasmtime {version}"))
    })
}

/// The required pins in both orders, so no result depends on which pin ran first.
fn pin_orders() -> [[Pin; 2]; 2] {
    let [a, b] = pins();
    [[a, b], [b, a]]
}

/// The shipped artifacts, or a panic that says how to build them.
fn dist() -> Dist {
    Dist::open().unwrap_or_else(|e| panic!("{e}; run: cargo xtask release dist"))
}

/// The R0 component compiled under `pin`.
fn r0(pin: Pin, dist: &Dist) -> R0 {
    R0::load(pin, &dist.component)
        .unwrap_or_else(|e| panic!("load R0 under wasmtime {}: {e:?}", pin.version()))
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

/// The first corpus request, decoded.
fn first_request() -> Request {
    Request::from_canonical_slice(&corpus()[0]).expect("the corpus is canonical")
}

/// The outcome of `request` on R2 and on R0 under both pins in both orders, all of which must be
/// equal; returns it.
fn agreed(dist: &Dist, request: &[u8], limits: Limits) -> Outcome {
    let mut r2 = R2Worker::spawn(&dist.worker).expect("start the R2 worker");
    let expected = r2.invoke(request).expect("R2 answers");
    for order in pin_orders() {
        for pin in order {
            let got = r0(pin, dist)
                .invoke(request, limits)
                .unwrap_or_else(|e| panic!("R0 {}: host error {e:?}", pin.version()));
            assert_eq!(
                got,
                expected,
                "R0 (wasmtime {}) and R2 differ",
                pin.version()
            );
        }
    }
    expected
}

#[test]
fn r0_r2_canonicalize_identical() {
    let dist = dist();
    let corpus = corpus();
    let mut r2 = R2Worker::spawn(&dist.worker).expect("start the R2 worker");
    let expected: Vec<Outcome> = corpus
        .iter()
        .map(|request| r2.invoke(request).expect("R2 answers every request"))
        .collect();
    for order in pin_orders() {
        for pin in order {
            let r0 = r0(pin, &dist);
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
}

#[test]
fn r0_r2_errors_identical() {
    let dist = dist();
    let canonical = &corpus()[0];

    // Whitespace after the opening brace: valid JSON, not canonical.
    let mut non_canonical = b"{ ".to_vec();
    non_canonical.extend_from_slice(&canonical[1..]);
    let outcome = agreed(&dist, &non_canonical, Limits::GENEROUS);
    assert!(
        matches!(outcome, Outcome::BadRequest(_)),
        "a non-canonical request is a bad request, got {outcome:?}"
    );

    let first = first_request();
    let nope = Request::new("science.cas.nope", first.args().clone(), first.budget()).to_jcs();
    let outcome = agreed(&dist, &nope, Limits::GENEROUS);
    assert_eq!(
        outcome,
        Outcome::UnsupportedOp("science.cas.nope".to_owned())
    );
}

#[test]
fn r0_plan_map_reduce() {
    let dist = dist();
    let request = &corpus()[0];
    for order in pin_orders() {
        for pin in order {
            let r0 = r0(pin, &dist);
            let v = pin.version();
            let invoked = match r0.invoke(request, Limits::GENEROUS) {
                Ok(Outcome::Ok(bytes)) => bytes,
                other => panic!("R0 {v}: invoke gave {other:?}"),
            };
            assert_eq!(
                r0.plan(request, Limits::GENEROUS),
                Ok(Outcome::Ok(vec![request.clone()])),
                "R0 {v}: plan makes one piece, the request"
            );
            assert_eq!(
                r0.map(request, Limits::GENEROUS),
                Ok(Outcome::Ok(invoked.clone())),
                "R0 {v}: map(request) == invoke(request)"
            );
            assert_eq!(
                r0.reduce(request, std::slice::from_ref(&invoked), Limits::GENEROUS),
                Ok(Outcome::Ok(invoked.clone())),
                "R0 {v}: reduce(request, [invoke(request)]) == invoke(request)"
            );
        }
    }
}

#[test]
fn fuel_step_budget_identical() {
    let dist = dist();
    let first = first_request();
    let request = Request::new(first.op(), first.args().clone(), Budget::new(1)).to_jcs();
    let outcome = agreed(&dist, &request, Limits::GENEROUS);
    assert_eq!(outcome, Outcome::Ok(Response::fuel_exhausted().to_jcs()));
}

#[test]
fn fuel_wasmtime_is_host_error() {
    let dist = dist();
    let request = &corpus()[0];
    let limits = Limits {
        fuel: 1000,
        ..Limits::GENEROUS
    };
    for order in pin_orders() {
        for pin in order {
            assert_eq!(
                r0(pin, &dist).invoke(request, limits),
                Err(HostError::OutOfFuel),
                "R0 {}: Wasmtime fuel exhaustion is a host error, never a response",
                pin.version()
            );
        }
    }
}

/// A canonical request of about 4 MB: `canonicalize` of an `Add` of 200,000 distinct `Int`s.
fn large_request() -> Vec<u8> {
    const TERMS: u32 = 200_000;
    let mut nodes: Vec<Node> = (0..TERMS).map(|i| Node::Int(i.to_string())).collect();
    nodes.push(Node::Add((0..TERMS).map(NodeId).collect()));
    let table = Table::new(nodes, NodeId(TERMS)).expect("a valid table");
    let args = BTreeMap::from([("expr".to_owned(), table)]);
    Request::new("science.cas.canonicalize", args, Budget::new(1_000_000)).to_jcs()
}

#[test]
fn memory_cap_is_host_error() {
    // The component's initial memory is 17 pages (1.06 MiB): 2 MiB lets it instantiate, and is
    // smaller than the request it must copy in.
    const CAP: usize = 2 << 20;
    let dist = dist();
    let request = large_request();
    assert!(
        request.len() > 2 * CAP,
        "the request is {} bytes",
        request.len()
    );
    let capped = Limits {
        memory_bytes: CAP,
        ..Limits::GENEROUS
    };
    let small = &corpus()[0];
    for order in pin_orders() {
        for pin in order {
            let r0 = r0(pin, &dist);
            let v = pin.version();
            // Control: the cap admits instantiation and a small request.
            assert!(
                matches!(r0.invoke(small, capped), Ok(Outcome::Ok(_))),
                "R0 {v}: a corpus request runs under the cap"
            );
            assert_eq!(
                r0.invoke(&request, capped),
                Err(HostError::MemoryCap),
                "R0 {v}: a denied memory growth is a host error, never a response"
            );
        }
    }
}
