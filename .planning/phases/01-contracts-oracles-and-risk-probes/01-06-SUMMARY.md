---
phase: 01-contracts-oracles-and-risk-probes
plan: 06
subsystem: plugin-r0
tags: [wit, wit-bindgen, wasm32-unknown-unknown, wasm-tools, wasmtime, conformance, det-04, d6]

requires:
  - phase: 01-04
    provides: ksci-ops (invoke/plan/map/reduce, StepBudget, OpError), ksci-worker (R2 stdio frames), testdata/wire/requests.jsonl
  - phase: 01-05
    provides: xtask heavy-job guard (`tools guard`), pinned tools table (wasm-tools 1.261.0)
provides:
  - "wit/science-cas.wit: package klein:science@0.1.0, interface cas (invoke, plan, map, reduce; variant error), world science-cas"
  - "crates/ksci-component: zero-import R0 shell over ksci_ops (wit-bindgen 0.62.0)"
  - "cargo xtask release dist: target/dist/{science_cas.component.wasm, <host-triple>/ksci-worker, digests.json}"
  - "conformance/ksci-conform: one r0_host! per Wasmtime pin (46.0.1, 49.0.2), R2Worker, Dist (digest-checked artifacts), Outcome/HostError"
  - "Six conformance tests: r0_r2_canonicalize_identical, r0_r2_errors_identical, r0_plan_map_reduce, fuel_step_budget_identical, fuel_wasmtime_is_host_error, memory_cap_is_host_error"
  - "check provenance judges .wit sources by their provenance header"
affects: [01-09 dist gates and reproducibility, 01-18 D6 request draft, Klein D2/D3 WIT naming]

actuals:
  tokens: 28500
  tasks: 2
  commits: 3
plan_head_before: 798d3e18dbedc94153158048c7946c5916d71a34
plan_head_after: eeef3c4f2f51fcb91bed3f4aec5c5ce3d43f1669

tech-stack:
  added: ["wit-bindgen =0.62.0 (macros, realloc)", "wasmtime =46.0.1 as wasmtime46 (test host)", "wasmtime =49.0.2 as wasmtime49 (test host)"]
  patterns:
    - "One host definition per Wasmtime pin via macro_rules! + bindgen!(wasmtime_crate: $wt) (D-01)"
    - "Fresh Store and fresh instance per R0 call; limiter wrapper records a denied memory growth"
    - "Host limits are HostError, never Outcome (DET-04)"
    - "Conformance runs the shipped artifacts, checked against digests.json, never the op layer in-process"

key-files:
  created:
    - wit/science-cas.wit
    - crates/ksci-component/Cargo.toml
    - crates/ksci-component/src/lib.rs
    - xtask/src/release.rs
    - conformance/ksci-conform/Cargo.toml
    - conformance/ksci-conform/src/lib.rs
    - conformance/ksci-conform/tests/r0_r2.rs
    - .planning/phases/01-contracts-oracles-and-risk-probes/deferred-items.md
  modified:
    - Cargo.toml
    - Cargo.lock
    - xtask/src/main.rs
    - xtask/src/check.rs

key-decisions:
  - "`map` is a WIT keyword (the map type), so the export is written `%map`; the exported function name stays `map`"
  - "check provenance judges .wit like .rs: wit_bindgen::generate! records the WIT in rustc dep-info, and the WIT is compiled into the shipped component"
  - "The conformance host uses Unix signals for guest faults on macOS (macos_use_mach_ports(false)): two Wasmtime copies in one process each claim the Mach exception port, and a fault in one copy's code killed the process with SIGILL"
  - "ksci-conform does not depend on ksci-ops: it judges the shipped artifacts only"
  - "ksci-component is listed in [workspace.metadata.ksci] r0-crates"

patterns-established:
  - "release dist sets CARGO_ENCODED_RUSTFLAGS explicitly for every build (R0: -Ctarget-feature=+simd128; host: empty)"
  - "Tests name the Wasmtime versions they need (46.0.1, 49.0.2) independently of Pin::ALL, so a missing host is a test failure"

requirements-completed: [PLUG-02, DET-04, DEP-03]

coverage:
  - id: D1
    description: "WIT tool world klein:science@0.1.0 with invoke, plan, map, reduce over JCS bytes and variant error"
    requirement: PLUG-02
    verification:
      - kind: other
        ref: "grep -c 'package klein:science@0.1.0' wit/science-cas.wit == 1; grep -c 'invoke:\\|plan:\\|map:\\|reduce:' == 4"
        status: pass
    human_judgment: false
  - id: D2
    description: "Zero-import R0 component built by cargo xtask release dist (wasm32-unknown-unknown +simd128, wasm-tools component new)"
    requirement: PLUG-02
    verification:
      - kind: other
        ref: "cargo xtask release dist; wasm-tools component wit target/dist/science_cas.component.wasm | grep -c '^ *import ' == 0"
        status: pass
    human_judgment: false
  - id: D3
    description: "R0 under wasmtime 46.0.1 and 49.0.2 (both orders) and R2 return identical outcomes on every corpus request, on errors, and on plan/map/reduce"
    requirement: PLUG-02
    verification:
      - kind: integration
        ref: "conformance/ksci-conform/tests/r0_r2.rs#r0_r2_canonicalize_identical"
        status: pass
      - kind: integration
        ref: "conformance/ksci-conform/tests/r0_r2.rs#r0_r2_errors_identical"
        status: pass
      - kind: integration
        ref: "conformance/ksci-conform/tests/r0_r2.rs#r0_plan_map_reduce"
        status: pass
    human_judgment: false
  - id: D4
    description: "Logical step budget gives identical fuel_exhausted bytes on R0 (both pins) and R2; Wasmtime fuel and memory cap are HostError::OutOfFuel / HostError::MemoryCap"
    requirement: DET-04
    verification:
      - kind: integration
        ref: "conformance/ksci-conform/tests/r0_r2.rs#fuel_step_budget_identical"
        status: pass
      - kind: integration
        ref: "conformance/ksci-conform/tests/r0_r2.rs#fuel_wasmtime_is_host_error"
        status: pass
      - kind: integration
        ref: "conformance/ksci-conform/tests/r0_r2.rs#memory_cap_is_host_error"
        status: pass
    human_judgment: false
  - id: D5
    description: "D6 evidence: R0 bytes identical under Klein's pin 46.0.1 and the D6 floor 49.0.2"
    requirement: DEP-03
    verification:
      - kind: integration
        ref: "cargo xtask tools guard -- cargo nextest run -p ksci-conform (6 passed)"
        status: pass
    human_judgment: false

duration: 19min
completed: 2026-10-12
status: complete
---

# Phase 1 Plan 06: R0 Tool World, Dual-Pin Wasmtime Host and DET-04 Summary

**A zero-import `klein:science@0.1.0` component (wasm32-unknown-unknown + `wasm-tools component new`), shipped by `cargo xtask release dist`, returns the same bytes as the R2 worker under Wasmtime 46.0.1 and 49.0.2; Wasmtime fuel and memory caps surface as `HostError`, never as responses.**

## Performance

- **Duration:** about 19 min
- **Started:** 2026-10-11T21:42:32Z
- **Completed:** 2026-10-11T22:01Z
- **Tasks:** 2 (Task 1 tracer, Task 2 TDD)
- **Files modified:** 11 (plus deferred-items.md)

## Accomplishments

- `wit/science-cas.wit`: package `klein:science@0.1.0`, interface `cas` with `invoke`, `plan`, `%map`, `reduce` over
  `list<u8>` JCS bytes and `variant error { bad-request, unsupported-op, internal }`, world `science-cas { export cas; }`.
- `crates/ksci-component`: the R0 shell. Each export calls the matching `ksci_ops` function and maps `OpError` one to one.
  It holds no other logic.
- `cargo xtask release dist` builds the shipped artifacts under the heavy-job guard:
  - `ksci-component` for `wasm32-unknown-unknown` with `CARGO_ENCODED_RUSTFLAGS=-Ctarget-feature=+simd128` (never
    relaxed SIMD), wrapped with the version-checked `wasm-tools component new`;
  - `ksci-worker` for the host triple (taken from `rustc -vV`);
  - `target/dist/digests.json` in JCS (artifact digests, `rustc -vV`, `wasm-tools --version`). Each digest is printed.
- `conformance/ksci-conform`:
  - `r0_host!` is written once and instantiated for `wasmtime46` (=46.0.1) and `wasmtime49` (=49.0.2) via
    `bindgen!(wasmtime_crate: $wt)` (D-01);
  - every call uses a fresh `Store` with `consume_fuel(true)` + `set_fuel`, and a `StoreLimitsBuilder::memory_size`
    limiter wrapped to record a denied growth;
  - `R2Worker` drives `ksci-worker` over stdio frames;
  - `Dist::open` checks each artifact against `digests.json`.
- Zero imports, from `wasm-tools component wit target/dist/science_cas.component.wasm`:
  ```
  package root:component;

  world root {
    export klein:science/cas@0.1.0;
  }
  ```
  There are no `import` lines (the count is 0). Plan 01-09 turns this into a gate.

## Tracer feedback gate (Task 1)

Interactive run, `human_verify_mode = end-of-phase`, and the tracer's `<verify>` is automated only. The verify was
re-run and passed, so the run went on to Task 2:
- `cargo xtask release dist` printed 3 `sha256:` lines (component, worker, digests.json);
- `cargo xtask tools guard -- cargo nextest run -p ksci-conform r0_r2` gave 1 passed.

## Task Commits

1. **Task 1: Tracer: WIT, zero-import R0 via release dist, R0(46.0.1) == R2**: `23e6f69` (feat)
2. **Task 2 RED: dual-pin and DET-04 tests**: `e266ad6` (test)
3. **Task 2 GREEN: wasmtime 49.0.2 host, macOS signal traps**: `eeef3c4` (feat)

No REFACTOR commit was needed.

## TDD Gate Compliance (Task 2)

- **RED** `e266ad6`. All 6 tests failed on the planned assertion: each test looks up the hosts for the versions it
  requires, `["46.0.1", "49.0.2"]`, and panicked `no R0 host for wasmtime 49.0.2`.
  - Command: `cargo xtask tools guard -- cargo nextest run -p ksci-conform --config-file <scratch>/nextest.toml --profile red`
    (a JUnit profile). Exit 100.
  - JUnit report `target/nextest/red/junit.xml`, run on 2026-10-11 from 21:52:08Z: tests=6, failures=6.
  - Classifier: `gsd-tools check tdd-red-evidence` returned **RED_EVIDENCE_OK** (`target_test_failed`, format
    junit, matched `ksci-conform::r0_r2#memory_cap_is_host_error`).
  - Semantic assessment: the target ran and failed on its planned precondition, that the D6-floor host exists. It was
    not a compile, fixture or load fault. The tests compile against the Task 1 API, and the missing behaviour is the
    49.0.2 host.
  - Caveat: this RED shows the missing pin, not the fuel or memory classification. Those contracts were seen RED
    through the perturbation witnesses below.
- **GREEN** `eeef3c4`. 6/6 pass, on both pins in both orders.

## RED Witnesses (rule 5)

### (a) Fuel mapping perturbation: `fuel_wasmtime_is_host_error` and an R0-versus-R2 mismatch

Perturbation, a temporary edit to `r0_host!`'s `invoke` in `conformance/ksci-conform/src/lib.rs`:
```diff
-                    self.call(limits, |b, s| b.klein_science_cas().call_invoke(s, request))
+                    match self.call(limits, |b, s| b.klein_science_cas().call_invoke(s, request)) {
+                        Err(HostError::OutOfFuel) => Ok(Outcome::Ok(ksci_wire::Response::fuel_exhausted().to_jcs())),
+                        other => other,
+                    }
```
A temporary test, `red_fuel_mapping_mismatch`, also ran R0 at Wasmtime fuel 1000 next to R2 on corpus line 1.

Command: `cargo xtask tools guard -- cargo nextest run -p ksci-conform --no-fail-fast fuel_wasmtime_is_host_error red_fuel_mapping_mismatch`

```
FAIL fuel_wasmtime_is_host_error
assertion `left == right` failed: R0 46.0.1: Wasmtime fuel exhaustion is a host error, never a response
  left: Ok(Ok([123, 34, 105, 100, ... 102,117,101,108,95,101,120,104,97,117,115,116,101,100 ... 125]))   # {"identity":null,...,"status":"fuel_exhausted","v":1}
 right: Err(OutOfFuel)
FAIL red_fuel_mapping_mismatch
assertion `left == right` failed: R0 46.0.1 at Wasmtime fuel 1000 vs R2
  left:  ... "status":"fuel_exhausted" ...                    (R0, perturbed)
 right:  ... "identity":"37f7939b...","status":"exact" ...    (R2)
Summary 2 tests run: 0 passed, 2 failed
```

Revert: `lib.rs` and `r0_r2.rs` were restored from the saved green copies, and `red_fuel_mapping_mismatch` is gone
(`grep -c red_fuel` gives 0). `fuel_wasmtime_is_host_error` then passed again (1 passed).

### (b) Worker perturbation: `r0_r2_canonicalize_identical`

Perturbation, a temporary edit to `crates/ksci-worker/src/main.rs`:
```diff
-            Ok(response) => write_response_frame(output, Tag::Ok, &response)?,
+            Ok(mut response) => {
+                response.push(b' ');
+                write_response_frame(output, Tag::Ok, &response)?
+            }
```
`cargo xtask release dist` was re-run. The worker digest became `b3ff6b5f…` and the component stayed `700d1968…`.

Command: `cargo xtask tools guard -- cargo nextest run -p ksci-conform r0_r2_canonicalize_identical`
```
FAIL r0_r2_canonicalize_identical
panicked at conformance/ksci-conform/tests/r0_r2.rs:102:17:
assertion `left == right` failed: R0 (wasmtime 46.0.1) and R2 differ on testdata/wire/requests.jsonl line 1
Summary 1 test run: 0 passed, 1 failed
```

Revert: `git checkout -- crates/ksci-worker/src/main.rs`, after which `git status --porcelain crates/ksci-worker` was
empty. Dist was re-run and the worker digest returned to `63b81a50…`, byte-identical to the Task 1 build. The full
suite was green again: 6 passed.

### (c) Deviation witness: the macOS Mach-port SIGILL

Without `macos_use_mach_ports(false)`, `memory_cap_is_host_error` died with `SIGILL` (signal 4). Both pins run in one
process in that test. A one-pin-per-process probe gave `Err(MemoryCap)` on each pin (request of 4,577,905 bytes, 2 MiB
cap). With the flag set, the test passes on both pins in both orders.

## D6 Evidence (for plan 01-18)

- **Wasmtime versions:**
  - `wasmtime =46.0.1` (Klein's current pin, renamed dependency `wasmtime46`);
  - `wasmtime =49.0.2` (the D6 floor per D-17, renamed dependency `wasmtime49`).
  - Both use features `runtime`, `cranelift`, `component-model` and `std`, with no WASI and no parallel compilation.
  - Engine: `Config::consume_fuel(true)` and `macos_use_mach_ports(false)`.
- **Instantiation (assumption A5):** both pins instantiate the zero-import component with an empty `Linker`.
- **Tests and pass counts.** `cargo xtask tools guard -- cargo nextest run -p ksci-conform` gives 6 run, 6 passed. Every
  test exercises both pins:

  | Test | Per pin | Result |
  |---|---|---|
  | `r0_r2_canonicalize_identical` | 18 corpus requests × 2 orders = 36 R0 invocations, each equal to R2 | pass on 46.0.1 and 49.0.2 |
  | `r0_r2_errors_identical` | non-canonical request → `BadRequest`; op `science.cas.nope` → `UnsupportedOp("science.cas.nope")`; 2 orders each | pass on both |
  | `r0_plan_map_reduce` | plan = [request], map = invoke, reduce(request, [invoke]) = invoke; 2 orders | pass on both |
  | `fuel_step_budget_identical` | steps "1" → `Outcome::Ok(Response::fuel_exhausted().to_jcs())`, equal to R2; 2 orders | pass on both |
  | `fuel_wasmtime_is_host_error` | Wasmtime fuel 1000 → `Err(HostError::OutOfFuel)`; 2 orders | pass on both |
  | `memory_cap_is_host_error` | 2 MiB cap: a corpus request → `Ok`; a 4,577,905-byte Add of 200,000 Ints → `Err(HostError::MemoryCap)`; 2 orders | pass on both |

- **Observed HostError variants:** `HostError::OutOfFuel` (Wasmtime `Trap::OutOfFuel`) and `HostError::MemoryCap`
  (a guest trap after the limiter denied a growth). Neither ever became an `Outcome`.
- **Statement:** the R0 component's response bytes were identical under Wasmtime 46.0.1 and 49.0.2, and identical to
  R2, for every corpus request, both error requests, plan/map/reduce and the `fuel_exhausted` response, with both pins
  run in both orders.
- **Artifacts under test:**
  - `science_cas.component.wasm` sha256 `700d196883b02b1f91ecc76c37d1b93dea89ead6bb61f8d582b27f45e46ac36e`;
  - `aarch64-apple-darwin/ksci-worker` sha256 `63b81a501f79e929a9e41d956a76c5cda424d4a915660674d1d3d62b710fec7f`;
  - built with rustc 1.99.0 (b940084d7 2026-09-28) and wasm-tools 1.261.0.
- **Advisories (T-01-17, accepted):** 46.0.1 is used only as an unshipped test host. `ksci-conform` is in deny.toml
  `[graph] exclude`, and `cargo deny check` reports `advisories ok, bans ok, licenses ok, sources ok`.

## Files Created/Modified

- `wit/science-cas.wit`: the tool world (PLUG-02).
- `crates/ksci-component/{Cargo.toml,src/lib.rs}`: the R0 cdylib shell.
- `xtask/src/release.rs` (new) and `xtask/src/main.rs` (`Release` variant): `cargo xtask release dist`.
- `xtask/src/check.rs`: `SOURCE_EXTENSIONS`/`is_source`. `.wit` inputs are judged by header, and the messages say
  "source" instead of "Rust".
- `conformance/ksci-conform/{Cargo.toml,src/lib.rs,tests/r0_r2.rs}`: the dual-pin host, R2 driver and tests.
- `Cargo.toml`: member `conformance/ksci-conform`; workspace dependencies `wit-bindgen`, `wasmtime46`, `wasmtime49`;
  `ksci-component` added to `r0-crates`.
- `Cargo.lock`: the new dependencies. Existing shipped-crate versions are unchanged; some entries only gained a version
  suffix where two versions now coexist.
- `.planning/phases/01-contracts-oracles-and-risk-probes/deferred-items.md`: two out-of-scope findings for 01-09.

## Decisions Made

See the key-decisions list in the frontmatter. In short: `%map` escaping; `.wit` is judged as source; Unix-signal
traps on macOS for a two-Wasmtime process; the conformance host links no op layer.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] `map` is a WIT keyword**
- **Found during:** Task 1.
- **Issue:** `wit_bindgen::generate!` failed with "expected `type`, `resource` or `func`, found keyword `map`".
- **Fix:** the export is declared `%map: func(...)`, the WIT escape for identifiers. The exported name is still `map`.
  The plan's grep counts 4.
- **Files modified:** wit/science-cas.wit.
- **Committed in:** 23e6f69.

**2. [Rule 3 - Blocking] `check provenance` refused `wit/science-cas.wit`**
- **Found during:** Task 1. `cargo xtask check provenance` was RED on the real tree:
  `wit/science-cas.wit: non-Rust compiler input outside testdata/`. wit-bindgen records the WIT in rustc's dep-info.
- **Fix:** `.wit` joins `.rs` as a source extension judged by its `//` provenance header (`SOURCE_EXTENSIONS`,
  `is_source`). New unit test `provenance_wit_is_source`, plus `.wit` cases in `provenance_original_ok` and
  `provenance_missing`.
- **Witness:** with the WIT's header line removed, the check failed with `wit/science-cas.wit: no provenance header`.
  With it restored, the check passed.
- **Files modified:** xtask/src/check.rs.
- **Committed in:** 23e6f69.

**3. [Rule 1 - Bug] SIGILL with two Wasmtime copies in one process on macOS**
- **Found during:** Task 2 GREEN.
- **Issue:** `memory_cap_is_host_error` aborted with SIGILL. On macOS Wasmtime handles guest faults through a
  per-process Mach exception port, and the second Wasmtime copy took it over from the first.
- **Fix:** `config.macos_use_mach_ports(false)`. Unix signal handlers chain, and Linux uses signals by default anyway.
- **Verification:** see RED witness (c). The test now passes on both pins in both orders.
- **Committed in:** eeef3c4.

**4. [Scope - D-01] ksci-conform does not depend on ksci-ops**
- **Issue:** the plan listed ksci-ops as a dependency, but nothing used it. Linking the op layer would also let a test
  bypass the shipped artifacts.
- **Fix:** removed the dependency, with a comment saying why.
- **Committed in:** eeef3c4.

**5. [Ordering - TDD] Tests written before the wasmtime49 dependency**
- **Issue:** Task 2 step 1 adds the 49.0.2 host before the tests. That order would have made every new test pass
  immediately, an unexpected GREEN.
- **Fix:** the tests name the versions they require and failed first (RED `e266ad6`). The 49.0.2 host followed (GREEN
  `eeef3c4`).

**6. [Rule 2 - Correctness] Shipped artifacts are digest-checked by the tests**
- `Dist::open` refuses an artifact whose SHA-256 differs from `digests.json`. A test therefore cannot run a stale or
  hand-edited binary, and the error says "run: cargo xtask release dist". The worker's directory comes from the
  `rustc -vV` host line that `digests.json` records.

**7. [Minor] Small interface choices**
- `Outcome<T = Vec<u8>>` is generic, so `plan` returns `Outcome<Vec<Vec<u8>>>`.
- `wit-bindgen` has `default-features = false` with features `macros` and `realloc`, so no async runtime is pulled in.
- `ksci-component` was added to `r0-crates`, so provenance also compiles the shell for the R0 check.

---

**Total deviations:** 3 auto-fixed (2 blocking, 1 bug) and 4 minor or scope adjustments.
**Impact on plan:** every auto-fix was needed for the plan's own verification (provenance, both pins in one test
process). There was no scope creep, and the out-of-scope findings went to deferred-items.md.

## Issues Encountered

- `cargo nextest run --workspace` in a fresh worktree needs `cargo xtask store fetch conformance/corpora/rfc8785.toml`
  first. CI runs that step. After the fetch, all 156 workspace tests passed.

## Deferred Issues

Both are recorded in `deferred-items.md` for plan 01-09:
- `check provenance` still compiles R0 crates for wasm32-wasip2, while D-16 ships wasm32-unknown-unknown.
- `release dist` passes no `--remap-path-prefix`. The component embeds 8 `/Users/…/.cargo/registry` paths (panic
  locations), so its digest depends on `$CARGO_HOME` (research Pitfall I).

## Verification (plan-level)

- `cargo xtask release dist`: exit 0, 3 sha256 lines.
- `cargo xtask tools guard -- cargo nextest run -p ksci-conform`: 6 passed.
- `wasm-tools component wit target/dist/science_cas.component.wasm`: 0 imports.
- `cargo deny check`: advisories ok, bans ok, licenses ok, sources ok. The only warnings are the existing duplicate
  `syn` and unused licence allow-list entries.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: clean. `cargo fmt --all --check`: clean.
- `cargo xtask check provenance`: ok, 26 sources.
- `cargo nextest run --workspace --locked`: 156 passed.
- Code oracles: the `LSP` tool reported "No LSP server available for file type: .rs" in this session, so the fallback
  was `cargo clippy --workspace --all-targets`. An ast-grep scan (`$X.ends_with(".rs")`) found no remaining `.rs`-only
  source classification, and `r0_host!` is instantiated exactly twice.

## User Setup Required

None.

## Next Phase Readiness

- Plan 01-09 can gate the zero-import list and relaxed SIMD on `target/dist/science_cas.component.wasm`, and fold
  `release dist` into CI. Two deferred items are waiting for it.
- Plan 01-18 can cite the D6 Evidence block above.
- The WIT name `klein:science@0.1.0` is still provisional until Klein assigns it (D2/D3).
- Scope of the DET-04 mark: this plan covers R0 (both Wasmtime pins) and R2, Wasmtime fuel and the memory cap.
  DET-04's wording also names R1 and GPU timeouts. No R1 or GPU path exists yet. When those land (DET-05, Phase 2;
  the Phase 3 GPU work), they need the same `fuel_exhausted` parity and host-error tests. The verifier should
  re-check the mark against that wording.
- REQUIREMENTS.md: PLUG-02 and DET-04 are marked complete. DEP-03 is left for the last plan that declares it
  (requirements.ready-ids blocked it).

---
*Phase: 01-contracts-oracles-and-risk-probes*
*Completed: 2026-10-12*

## Self-Check: PASSED

- Files found: wit/science-cas.wit, crates/ksci-component/src/lib.rs, xtask/src/release.rs, conformance/ksci-conform/src/lib.rs, conformance/ksci-conform/tests/r0_r2.rs, deferred-items.md
- Commits found on HEAD: 23e6f69, e266ad6, eeef3c4
