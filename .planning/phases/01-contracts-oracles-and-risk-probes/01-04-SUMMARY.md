---
phase: 01-contracts-oracles-and-risk-probes
plan: 04
subsystem: ops
tags: [rust, ksci-ops, ksci-worker, stdio-frames, step-budget, plan-map-reduce, jcs]

requires:
  - phase: 01-01
    provides: "ksci-wire: Request::from_canonical_slice, Table::canonicalize_with, Response::exact/fuel_exhausted, MAX_DOCUMENT_BYTES"
  - phase: 01-02
    provides: "xtask spine: cargo xtask check no-corpus, store fetch (RFC 8785 vectors)"
provides:
  - "ksci_wire::frame: the one stdio frame codec (request = u32 BE length + JCS; response = u8 tag + u32 BE length + payload), cap checked before any body byte"
  - "ksci-ops: OPS registry with science.cas.canonicalize, StepBudget (the only source of fuel_exhausted), OpError, invoke/plan/map/reduce"
  - "ksci-worker: the R2 native shell (51 lines): frames in, ksci_ops::invoke, frames out; exit 0/1/2"
  - "testdata/wire/requests.jsonl: 18 own-authored canonical requests, shared by R0, R2 and the Python differential"
  - "schema/expr.v1.md: step counting rules for science.cas.canonicalize"
affects: [01-06 R0 component and conformance host, 01-08 Python JCS mirror, PLUG-02, DET-04]

actuals:
  tokens: 7950
  tasks: 2
  commits: 3
plan_head_before: 46c5bc6e6e4a686f4fa0ee7726e84e465f0112d7
plan_head_after: 205b855b4afa9febb87a2c85f4db2efc0d163c39

tech-stack:
  added: []
  patterns:
    - "Shells hold no logic: decode a frame, call ksci_ops, encode a frame"
    - "Op registry is a const slice of (name, fn); an op exists exactly when listed"
    - "Frame payloads are read with Read::take, so a length prefix never allocates the declared size"
    - "Stdio tests read frames on a helper thread with a 30 s recv_timeout, so a worker that waits fails instead of hanging"

key-files:
  created:
    - crates/ksci-wire/src/frame.rs
    - crates/ksci-ops/Cargo.toml
    - crates/ksci-ops/src/lib.rs
    - crates/ksci-ops/tests/ops.rs
    - crates/ksci-worker/Cargo.toml
    - crates/ksci-worker/src/main.rs
    - crates/ksci-worker/tests/stdio.rs
    - testdata/wire/requests.jsonl
  modified:
    - crates/ksci-wire/src/lib.rs
    - schema/expr.v1.md
    - Cargo.lock

key-decisions:
  - "The frame codec has both halves (read/write request, read/write response) in ksci_wire::frame, so the worker, its tests and the conformance host share one codec"
  - "write_response_frame refuses payloads above MAX_DOCUMENT_BYTES, and invoke maps an oversized response to Internal, so every frame written can be read back"
  - "plan rejects a non-canonical request (BadRequest) and an unknown op (UnsupportedOp) up front; reduce checks the request and that its single output is a valid response"
  - "Error frames carry OpError::message() (the text without its kind); the tag carries the kind"
  - "Worker exit codes: 0 clean EOF, 2 any frame error (oversized after a bad-request frame, truncated, read failure), 1 stdout write failure"

patterns-established:
  - "Mutation witnesses for tests whose behaviour already existed: mutate, run, restore with git checkout of that file, record the failing test"

requirements-completed: [PLUG-02, DET-04]

coverage:
  - id: D1
    description: "A canonical science.cas.canonicalize request framed on the R2 worker's stdin returns an ok frame byte-identical to ksci_ops::invoke, carrying the canonical table and identity"
    requirement: PLUG-02
    verification:
      - kind: unit
        ref: "crates/ksci-ops/src/lib.rs#tests::ops_canonicalize_tracer"
        status: pass
      - kind: integration
        ref: "crates/ksci-worker/tests/stdio.rs#worker_roundtrip_matches_invoke (18 corpus lines, one long-lived child)"
        status: pass
    human_judgment: false
  - id: D2
    description: "fuel_exhausted comes only from StepBudget, with fixed bytes; steps 4/5/0 on x**2 + 1 and the step counting rules"
    requirement: DET-04
    verification:
      - kind: unit
        ref: "crates/ksci-ops/tests/ops.rs#ops_budget_exhausted"
        status: pass
      - kind: unit
        ref: "crates/ksci-ops/tests/ops.rs#ops_budget_counts_entered_nodes"
        status: pass
    human_judgment: false
  - id: D3
    description: "plan returns one piece, map is invoke, reduce returns its single output and rejects any other count"
    requirement: PLUG-02
    verification:
      - kind: unit
        ref: "crates/ksci-ops/tests/ops.rs#ops_plan_map_reduce"
        status: pass
    human_judgment: false
  - id: D4
    description: "Malformed, non-canonical and unsupported requests give bad-request/unsupported-op frames, never a panic; an oversized length prefix gives a bad-request frame and exit 2 without reading the body"
    verification:
      - kind: unit
        ref: "crates/ksci-ops/tests/ops.rs#ops_errors"
        status: pass
      - kind: integration
        ref: "crates/ksci-worker/tests/stdio.rs#worker_error_tags, worker_frame_cap"
        status: pass
    human_judgment: false
  - id: D5
    description: "Shared own-authored request corpus: every line canonical, both statuses covered, under the testdata limit"
    verification:
      - kind: unit
        ref: "crates/ksci-ops/tests/ops.rs#requests_corpus_canonical"
        status: pass
      - kind: other
        ref: "cargo xtask check no-corpus (82 files, no violations)"
        status: pass
    human_judgment: false

duration: 9min
completed: 2026-10-11
status: complete
---

# Phase 1 Plan 04: ksci-ops and the R2 stdio worker Summary

**`ksci-ops` owns the op set: `science.cas.canonicalize`, a logical `StepBudget` that alone yields `fuel_exhausted`,
and invoke/plan/map/reduce. `ksci-worker` is a 51-line R2 shell over u32-length stdio frames from `ksci_wire::frame`.
Its ok frames are byte-identical to `ksci_ops::invoke` across an 18-request own-authored corpus.**

## Performance

- **Duration:** 9 min
- **Started:** 2026-10-11T19:27:36Z
- **Completed:** 2026-10-11T19:36:03Z
- **Tasks:** 2
- **Files modified:** 11

## Accomplishments

- The R2 path works end to end. The release `ksci-worker` binary, fed one frame holding the shuffled `x**2 + 1`,
  returned the exact response from `schema/expr.v1.md`, byte for byte. That includes identity `37f7939b…cb964`.
- `ksci_wire::frame` is one codec for both sides:
  - it checks the length against `MAX_DOCUMENT_BYTES` before reading any body byte;
  - it reads the body incrementally, so a 64 MiB length prefix allocates nothing up front;
  - it distinguishes truncated frames and unknown tags.
- DET-04, logical side. `StepBudget` charges one step each time a reachable input node is first entered. Its `Break`
  is the only route to `Response::fuel_exhausted()` outside tests (ast-grep below). Those bytes are pinned to the
  schema literal.
- PLUG-02, R2 side:
  - `plan` returns `[request]`;
  - `map` is `invoke`;
  - `reduce` accepts exactly one output, and that output must be a valid response.
- `testdata/wire/requests.jsonl` holds 18 canonical requests in 3467 bytes. It includes every case the plan lists,
  plus a negative 300-digit Int, an `Fn` with arguments, steps 5 and 4 on the shuffled table, and the maximum
  step budget.

## Task Commits

1. **Task 1 (tracer): frame → ksci-worker → ksci-ops canonicalize** – `f028a33` (feat)
2. **Task 2 (TDD): budget, error frames, plan/map/reduce, corpus**
   - RED: `95fd7ef` (test)
   - GREEN: `205b855` (feat)
   - REFACTOR: none needed

**Plan metadata:** this SUMMARY commit (docs)

## TDD record (Task 2)

**RED.** `ops_plan_map_reduce` ran against stub `plan`/`map`/`reduce` that return
`Err(Internal("… not implemented"))`. It failed on its first planned assertion,
`plan(r) == Ok(vec![r])` (left `Err(Internal("plan is not implemented"))`). Run:
`cargo nextest run --profile red -p ksci-ops -p ksci-worker`, with a scratch config that emits JUnit. That gave
8 tests: 7 passed, 1 failed, exit 100.
- The JUnit record went through `gsd-tools check tdd-red-evidence`, which returned **RED_EVIDENCE_OK**
  (`target_test_failed`, matched `ksci-ops::ops#ops_plan_map_reduce`).
- **Semantic assessment:** the target ran, and failed on the planned assertion for the intended reason, the
  missing behaviour. Compilation, discovery and fixtures were sound: the other 7 tests in the same run passed. Valid
  RED.

**Tests over behaviour Task 1 already built** (`ops_budget_exhausted`, `ops_errors`, `requests_corpus_canonical`,
`worker_*`) passed at RED, which was expected. Each one was then shown to fail against a mutation of the code it
covers (CLAUDE.md rule 5). Every file was restored with `git checkout -- <file>` after its mutation, and
`git diff --stat` was clean.

| # | Mutation | Failing test(s) |
|---|---|---|
| M1 | `StepBudget::charge` never breaks | `ops_budget_exhausted`, `requests_corpus_canonical` |
| M2 | `charge` off by one (`checked_sub(1).filter(r > 0)`) | `ops_budget_exhausted` ("5 steps cover 5 reachable nodes") |
| M3 | `canonicalize` drops the exactly-one-argument check | `ops_errors` (the `extra` argument case) |
| M4 | corpus line 7 gets a space | `requests_corpus_canonical` ("line 7 is not a canonical request"), `worker_roundtrip_matches_invoke` |
| M5 | worker writes no bad-request frame on `TooLarge` | `worker_frame_cap` |
| M6 | worker exits after the first error frame | `worker_error_tags` |
| M7 | `UnsupportedOp` mapped to tag 1 | `worker_error_tags` (left 1, right 2) |
| M8 | worker sends `response[1..]` | `worker_roundtrip_matches_invoke`, `worker_error_tags` |
| M9 | frame reader skips the cap check | `worker_frame_cap` ("the worker sent no frame within 30s"): the worker waited for the body |
| M10 | `canonicalize_with` charges every edge rather than each first entry | `ops_budget_counts_entered_nodes` (left 3, right 2) |

`ops_canonicalize_tracer` (Task 1) failed under a mutation where an exact result was turned into `fuel_exhausted`
(left `FuelExhausted`, right `Exact`).

**GREEN.** `plan`/`map`/`reduce` were implemented. Results:
- ksci-ops, ksci-worker and ksci-wire: 51/51 pass;
- workspace: 91/91 pass;
- clippy `-D warnings` and `cargo fmt --check` are clean;
- `cargo check -p ksci-ops -p ksci-wire --target wasm32-wasip2` is clean;
- the ksci-wire doctests pass (5/5).

## Code oracles (CLAUDE.md rule 7)

- **Semantic.** rust-analyzer was not available: the `LSP` tool returned "No LSP server available for file type:
  .rs". The fallback was compiler enumeration. `#[deprecated]` was put on `ksci_ops::invoke` temporarily, and
  `cargo check --workspace --all-targets` listed 17 use sites. The only production ones are `map`
  (`ksci-ops/src/lib.rs:117`) and the worker shell (`ksci-worker/src/main.rs:38`). The other 15 are tests and the
  import line. A first attempt renamed the function instead. That stopped the downstream crates from compiling, so
  it gave an incomplete list and was discarded.
- **Structural (ast-grep, all `.rs`):**
  - `Response::fuel_exhausted()` appears once outside tests, at `ksci-ops/src/lib.rs:147`. That is the `None` arm of
    the `canonicalize_with(&mut || budget.charge())` call. The other 3 hits are in `ksci-wire/tests/contract.rs`.
  - `ksci_ops::$F(...)` has one hit in non-test code: `ksci-worker/src/main.rs:38`, which calls `invoke`.
  - `StepBudget::new` is called once, in `invoke`.
- **Every-site claims:** none. No renames, removals or signature changes. `ksci_wire` gained a module and nothing
  else changed.

## Decisions Made

See `key-decisions` in the frontmatter. The step-counting rule (one step per reachable input node, charged on first
entry) is now written in `schema/expr.v1.md`. The plan required that only if it was missing, and it was.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] The plan's response construction does not match the ksci-wire API**
- **Found during:** Task 1
- **Issue:** The plan builds `Response { v, status, … }` by struct literal and calls `budget.steps_u64()`. Plan
  01-01 (review F3) made the Response fields private and renamed the accessor to `Budget::steps()`.
- **Fix:** Used `Response::exact(&t)`, `Response::fuel_exhausted()` and `request.budget().steps()`.
  `Response::exact` canonicalises again, without charging the budget. That pass is idempotent on an
  already-canonical table and gives the same identity as `sha256_hex(t.to_jcs())`.
- **Files modified:** crates/ksci-ops/src/lib.rs
- **Committed in:** f028a33

**2. [Rule 2 - Missing critical] Both halves of the frame codec, and a cap on writes**
- **Found during:** Task 1
- **Issue:** The plan lists only the worker's half: read request, write response. The stdio tests and the
  conformance host (01-06) need the host's half. Without it they would reimplement the codec.
- **Fix:** Added:
  - `write_request_frame`;
  - `read_response_frame`;
  - `FrameError::UnknownTag` and `FrameError::Io`.

  Writers refuse payloads above `MAX_DOCUMENT_BYTES`. `invoke` returns `Internal` if a response would exceed the
  cap, which can happen because canonical renumbering can lengthen child indices. So every frame written can be
  read back.
- **Files modified:** crates/ksci-wire/src/frame.rs, crates/ksci-ops/src/lib.rs
- **Committed in:** f028a33

**3. [Rule 2 - Missing critical] plan and reduce validate their inputs**
- **Found during:** Task 2
- **Fix:**
  - `plan` rejects a non-canonical request or an unknown op before returning a piece.
  - `reduce` decodes the request and requires its single output to pass `Response::from_slice`.
  - Both are tested in `ops_plan_map_reduce`.
- **Committed in:** 205b855

**4. [Rule 2 - Missing critical] A witness for the documented step-counting rules**
- **Found during:** Task 2
- **Issue:** The schema text now makes three counting claims (equal nodes at different indices, an index referenced
  twice, unreachable nodes), and no test covered them.
- **Fix:** Added `ops_budget_counts_entered_nodes`, shown to fail under mutation M10.
- **Committed in:** 205b855

---

**Total deviations:** 4 auto-fixed (1 blocking, 3 missing critical). **Impact:** no scope creep. No new external
crates: Cargo.lock only gained the two workspace crates (T-01-SC).

## Issues Encountered

- **Stub restore.** A mutation restore (`git checkout -- crates/ksci-ops/src/lib.rs`) also discarded the RED stubs,
  which were not yet committed. They were re-added, the RED state was confirmed identical (8 tests: 7 passed, 1
  failed), and RED was committed before any further mutation. No effect on the result.
- **RFC 8785 vectors.** The `ksci-wire::rfc8785` tests failed in this fresh worktree because the vectors had not
  been fetched yet (rule 6 keeps them out of git). `cargo xtask store fetch conformance/corpora/rfc8785.toml`
  fetched them with digests checked, and all 91 workspace tests then passed. This is environmental and unrelated to
  this plan.
- **No LSP.** rust-analyzer was not available in this session; see Code oracles. Per CLAUDE.md the session needs a
  restart to get it.

## Out of scope / orchestrator notes

- REQUIREMENTS.md was not touched. PLUG-02 and DET-04 are shared with sibling plans (01-06 covers R0 and the
  Wasmtime host-error side), so marking them is left to the orchestrator's shared-ID gate. STATE.md and ROADMAP.md
  were not touched either.

## User Setup Required

None.

## Next Phase Readiness

- Plan 01-06 can build the R0 component on `ksci_ops::{invoke, plan, map, reduce}`, and its conformance host on
  `ksci_wire::frame` plus `testdata/wire/requests.jsonl`.
- Plan 01-08 can run the same corpus through the Python JCS mirror.
- The provenance check (01-03) will find `// provenance: original` on line 1 of every new `.rs` file.

---
*Phase: 01-contracts-oracles-and-risk-probes*
*Completed: 2026-10-11*
