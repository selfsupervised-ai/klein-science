// provenance: original
//! `ksci-conform`: the unshipped conformance host. It runs the shipped artifacts of
//! `cargo xtask release dist` and compares them byte for byte:
//!
//! - R0: `science_cas.component.wasm` under Wasmtime, at each pin in [`Pin`] (Klein's current pin,
//!   and the D6 floor, D-17). One host definition, [`r0_host!`], is instantiated once per pin (D-01).
//! - R2: `<host-triple>/ksci-worker`, driven over stdio frames ([`R2Worker`]).
//!
//! The shipped artifacts are found under `$KSCI_DIST` (default `<workspace>/target/dist`) and
//! checked against the SHA-256 digests `digests.json` records ([`Dist::open`]), so the bytes under
//! test are exactly the bytes that ship.
//!
//! A response is an [`Outcome`]: the WIT `result<_, error>` of the call. A host limit is never an
//! outcome: Wasmtime fuel exhaustion, a denied memory growth, any other trap and a broken worker
//! are [`HostError`]s (DET-04). Only the logical step budget inside `ksci-ops` produces a
//! `fuel_exhausted` response, and that response is an `Outcome::Ok` like any other.
//!
//! Every R0 call runs in a fresh Wasmtime `Store` with a fresh instance, so no call sees state left
//! by an earlier one, nor by the other pin.

use std::fs;
use std::io::{BufReader, BufWriter};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use ksci_wire::frame::{Tag, read_response_frame, write_request_frame};

/// The WIT `result<T, error>` of one call: the payload, or the error variant and its message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome<T = Vec<u8>> {
    /// The call returned a value (response JCS bytes, for `invoke`, `map` and `reduce`).
    Ok(T),
    /// `error::bad-request`.
    BadRequest(String),
    /// `error::unsupported-op`.
    UnsupportedOp(String),
    /// `error::internal`.
    Internal(String),
}

/// Why a host produced no outcome. Never a response status (DET-04).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HostError {
    /// Wasmtime fuel ran out.
    OutOfFuel,
    /// The guest trapped after the host denied a memory growth.
    MemoryCap,
    /// Any other trap or Wasmtime error, with its text.
    Trap(String),
    /// The artifact could not be loaded or the worker could not be started.
    Spawn(String),
    /// The worker broke the frame protocol (an unreadable frame, an early exit).
    Protocol(String),
}

/// The host limits of one R0 call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Wasmtime fuel for the call (instantiation included).
    pub fuel: u64,
    /// The cap on the guest's linear memory, in bytes.
    pub memory_bytes: usize,
}

impl Limits {
    /// Limits no corpus request reaches: the logical step budget decides every result.
    pub const GENEROUS: Limits = Limits {
        fuel: 1 << 40,
        memory_bytes: 1 << 30,
    };
}

/// The Wasmtime pins the R0 host runs under.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pin {
    /// Klein's current pin, wasmtime =46.0.1.
    W46,
    /// The D6 floor (D-17), wasmtime =49.0.2.
    W49,
}

impl Pin {
    /// Every pin, oldest first.
    pub const ALL: [Pin; 2] = [Pin::W46, Pin::W49];

    /// The pinned Wasmtime version.
    pub const fn version(self) -> &'static str {
        match self {
            Pin::W46 => "46.0.1",
            Pin::W49 => "49.0.2",
        }
    }
}

/// One host definition, instantiated once per Wasmtime pin: `$wt` is the renamed `wasmtime` crate.
macro_rules! r0_host {
    ($module:ident, $wt:ident) => {
        /// The R0 host under one Wasmtime pin.
        pub mod $module {
            use std::path::Path;

            use $wt::component::{Component, Linker};
            use $wt::{
                Config, Engine, ResourceLimiter, Store, StoreLimits, StoreLimitsBuilder, Trap,
            };

            use crate::{HostError, Limits, Outcome};

            $wt::component::bindgen!({
                world: "science-cas",
                path: "../../wit",
                wasmtime_crate: $wt,
            });

            use exports::klein::science::cas::Error;

            /// The store data: the memory limiter, and whether it ever denied a growth.
            struct State {
                limits: StoreLimits,
                denied: bool,
            }

            impl ResourceLimiter for State {
                fn memory_growing(
                    &mut self,
                    current: usize,
                    desired: usize,
                    maximum: Option<usize>,
                ) -> $wt::Result<bool> {
                    let allowed = self.limits.memory_growing(current, desired, maximum)?;
                    if !allowed {
                        self.denied = true;
                    }
                    Ok(allowed)
                }

                fn table_growing(
                    &mut self,
                    current: usize,
                    desired: usize,
                    maximum: Option<usize>,
                ) -> $wt::Result<bool> {
                    self.limits.table_growing(current, desired, maximum)
                }

                fn instances(&self) -> usize {
                    self.limits.instances()
                }

                fn tables(&self) -> usize {
                    self.limits.tables()
                }

                fn memories(&self) -> usize {
                    self.limits.memories()
                }
            }

            /// The R0 component compiled once for this pin; every call gets a fresh store.
            pub struct R0 {
                engine: Engine,
                component: Component,
            }

            impl R0 {
                /// Compile the component at `path` with fuel metering on.
                ///
                /// Guest faults (an `unreachable` after a failed allocation, for one) are
                /// handled with Unix signals on every platform. On macOS Wasmtime defaults to a
                /// Mach exception port per process, and two Wasmtime copies in one process (both
                /// pins) each claim it, so a fault in the first copy's code reaches the second
                /// copy's handler and kills the process with SIGILL. Signal handlers chain: each
                /// copy forwards a fault outside its own code to the handler it replaced.
                pub fn load(path: &Path) -> Result<R0, HostError> {
                    let mut config = Config::new();
                    config.consume_fuel(true);
                    config.macos_use_mach_ports(false);
                    let engine = Engine::new(&config)
                        .map_err(|e| HostError::Spawn(format!("engine: {e:?}")))?;
                    let component = Component::from_file(&engine, path).map_err(|e| {
                        HostError::Spawn(format!("load {}: {e:?}", path.display()))
                    })?;
                    Ok(R0 { engine, component })
                }

                /// Run `f` against a fresh instance in a fresh store under `limits`. The
                /// component imports nothing, so the linker is empty.
                fn call<T>(
                    &self,
                    limits: Limits,
                    f: impl FnOnce(
                        &ScienceCas,
                        &mut Store<State>,
                    ) -> $wt::Result<Result<T, Error>>,
                ) -> Result<Outcome<T>, HostError> {
                    let state = State {
                        limits: StoreLimitsBuilder::new()
                            .memory_size(limits.memory_bytes)
                            .build(),
                        denied: false,
                    };
                    let mut store = Store::new(&self.engine, state);
                    store.limiter(|s| s);
                    store
                        .set_fuel(limits.fuel)
                        .map_err(|e| HostError::Trap(format!("set_fuel: {e:?}")))?;
                    let linker = Linker::<State>::new(&self.engine);
                    let result = ScienceCas::instantiate(&mut store, &self.component, &linker)
                        .and_then(|bindings| f(&bindings, &mut store));
                    match result {
                        Ok(Ok(value)) => Ok(Outcome::Ok(value)),
                        Ok(Err(Error::BadRequest(m))) => Ok(Outcome::BadRequest(m)),
                        Ok(Err(Error::UnsupportedOp(m))) => Ok(Outcome::UnsupportedOp(m)),
                        Ok(Err(Error::Internal(m))) => Ok(Outcome::Internal(m)),
                        Err(e) => Err(classify(&e, store.data().denied)),
                    }
                }

                /// `cas.invoke`.
                pub fn invoke(&self, request: &[u8], limits: Limits) -> Result<Outcome, HostError> {
                    self.call(limits, |b, s| b.klein_science_cas().call_invoke(s, request))
                }

                /// `cas.plan`.
                pub fn plan(
                    &self,
                    request: &[u8],
                    limits: Limits,
                ) -> Result<Outcome<Vec<Vec<u8>>>, HostError> {
                    self.call(limits, |b, s| b.klein_science_cas().call_plan(s, request))
                }

                /// `cas.map`.
                pub fn map(&self, piece: &[u8], limits: Limits) -> Result<Outcome, HostError> {
                    self.call(limits, |b, s| b.klein_science_cas().call_map(s, piece))
                }

                /// `cas.reduce`.
                pub fn reduce(
                    &self,
                    request: &[u8],
                    outputs: &[Vec<u8>],
                    limits: Limits,
                ) -> Result<Outcome, HostError> {
                    self.call(limits, |b, s| {
                        b.klein_science_cas().call_reduce(s, request, outputs)
                    })
                }
            }

            /// A Wasmtime error as a host error: out of fuel, a trap after a denied memory
            /// growth, or any other failure. Never an outcome.
            fn classify(error: &$wt::Error, denied: bool) -> HostError {
                match error.downcast_ref::<Trap>() {
                    Some(Trap::OutOfFuel) => HostError::OutOfFuel,
                    _ if denied => HostError::MemoryCap,
                    _ => HostError::Trap(format!("{error:?}")),
                }
            }
        }
    };
}

r0_host!(w46, wasmtime46);
r0_host!(w49, wasmtime49);

/// The R0 component under one Wasmtime pin.
pub enum R0 {
    /// Under wasmtime =46.0.1.
    W46(w46::R0),
    /// Under wasmtime =49.0.2.
    W49(w49::R0),
}

/// Runs `$call` on the host of whichever pin `$r0` holds.
macro_rules! on_pin {
    ($r0:expr, $host:ident => $call:expr) => {
        match $r0 {
            R0::W46($host) => $call,
            R0::W49($host) => $call,
        }
    };
}

impl R0 {
    /// Compile the component at `path` under `pin`.
    pub fn load(pin: Pin, path: &Path) -> Result<R0, HostError> {
        Ok(match pin {
            Pin::W46 => R0::W46(w46::R0::load(path)?),
            Pin::W49 => R0::W49(w49::R0::load(path)?),
        })
    }

    /// `cas.invoke` in a fresh store.
    pub fn invoke(&self, request: &[u8], limits: Limits) -> Result<Outcome, HostError> {
        on_pin!(self, h => h.invoke(request, limits))
    }

    /// `cas.plan` in a fresh store.
    pub fn plan(&self, request: &[u8], limits: Limits) -> Result<Outcome<Vec<Vec<u8>>>, HostError> {
        on_pin!(self, h => h.plan(request, limits))
    }

    /// `cas.map` in a fresh store.
    pub fn map(&self, piece: &[u8], limits: Limits) -> Result<Outcome, HostError> {
        on_pin!(self, h => h.map(piece, limits))
    }

    /// `cas.reduce` in a fresh store.
    pub fn reduce(
        &self,
        request: &[u8],
        outputs: &[Vec<u8>],
        limits: Limits,
    ) -> Result<Outcome, HostError> {
        on_pin!(self, h => h.reduce(request, outputs, limits))
    }
}

/// The R2 worker process: one process, one request frame and one response frame per call.
pub struct R2Worker {
    child: Child,
    input: Option<BufWriter<ChildStdin>>,
    output: BufReader<ChildStdout>,
}

impl R2Worker {
    /// Start the worker binary at `path`.
    pub fn spawn(path: &Path) -> Result<R2Worker, HostError> {
        let mut child = Command::new(path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| HostError::Spawn(format!("spawn {}: {e}", path.display())))?;
        let input = child.stdin.take().map(BufWriter::new);
        let output = child
            .stdout
            .take()
            .map(BufReader::new)
            .ok_or_else(|| HostError::Spawn("worker has no stdout".to_owned()))?;
        Ok(R2Worker {
            child,
            input,
            output,
        })
    }

    /// Send one request frame and read its response frame.
    pub fn invoke(&mut self, request: &[u8]) -> Result<Outcome, HostError> {
        let input = self
            .input
            .as_mut()
            .ok_or_else(|| HostError::Protocol("worker stdin is closed".to_owned()))?;
        write_request_frame(input, request)
            .map_err(|e| HostError::Protocol(format!("write request frame: {e}")))?;
        let (tag, payload) = read_response_frame(&mut self.output)
            .map_err(|e| HostError::Protocol(format!("read response frame: {e}")))?
            .ok_or_else(|| HostError::Protocol("worker closed stdout".to_owned()))?;
        let text = || {
            String::from_utf8(payload.clone())
                .map_err(|e| HostError::Protocol(format!("error message is not UTF-8: {e}")))
        };
        Ok(match tag {
            Tag::Ok => Outcome::Ok(payload.clone()),
            Tag::BadRequest => Outcome::BadRequest(text()?),
            Tag::UnsupportedOp => Outcome::UnsupportedOp(text()?),
            Tag::Internal => Outcome::Internal(text()?),
        })
    }
}

impl Drop for R2Worker {
    fn drop(&mut self) {
        // Closing stdin is a clean end of input; the worker exits 0.
        drop(self.input.take());
        let _ = self.child.wait();
    }
}

/// The shipped artifacts of `cargo xtask release dist`, checked against `digests.json`.
pub struct Dist {
    /// `science_cas.component.wasm`.
    pub component: PathBuf,
    /// `<host-triple>/ksci-worker`.
    pub worker: PathBuf,
}

impl Dist {
    /// The dist directory: `$KSCI_DIST`, or `<workspace>/target/dist`.
    pub fn dir() -> PathBuf {
        match std::env::var_os("KSCI_DIST") {
            Some(dir) => PathBuf::from(dir),
            None => Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/dist")
                .components()
                .collect(),
        }
    }

    /// The artifacts under [`Dist::dir`]. The worker's directory is the host triple of the
    /// `rustc -vV` that `digests.json` records, and every artifact must hash to its recorded
    /// digest.
    pub fn open() -> Result<Dist, String> {
        let dir = Dist::dir();
        let record_path = dir.join("digests.json");
        let record =
            fs::read(&record_path).map_err(|e| format!("read {}: {e}", record_path.display()))?;
        let record: serde_json::Value = serde_json::from_slice(&record)
            .map_err(|e| format!("{}: {e}", record_path.display()))?;
        let triple = record
            .get("rustc -vV")
            .and_then(|v| v.as_str())
            .and_then(|vv| vv.lines().find_map(|l| l.strip_prefix("host: ")))
            .ok_or_else(|| format!("{}: no rustc host triple", record_path.display()))?;
        let artifacts = record
            .get("artifacts")
            .and_then(|a| a.as_object())
            .ok_or_else(|| format!("{}: no artifacts", record_path.display()))?;
        let worker_rel = format!("{triple}/ksci-worker{}", std::env::consts::EXE_SUFFIX);
        for rel in ["science_cas.component.wasm", worker_rel.as_str()] {
            let recorded = artifacts
                .get(rel)
                .and_then(|d| d.as_str())
                .ok_or_else(|| format!("{}: no digest for {rel}", record_path.display()))?;
            let path = dir.join(rel);
            let bytes = fs::read(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
            let actual = ksci_wire::sha256_hex(&bytes);
            if actual != recorded {
                return Err(format!(
                    "{} hashes to {actual}, digests.json records {recorded}",
                    path.display()
                ));
            }
        }
        Ok(Dist {
            component: dir.join("science_cas.component.wasm"),
            worker: dir.join(worker_rel),
        })
    }
}
