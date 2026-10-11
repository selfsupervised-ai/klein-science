---
phase: 01-contracts-oracles-and-risk-probes
plan: 01
subsystem: wire
tags: [rust, jcs, rfc8785, sha256, serde_json_canonicalizer, sha2, proptest, static_assertions]

requires: []
provides:
  - "Workspace root: toolchain 1.99.0, exact pins, release profile, workspace clippy lints (disallowed_types/methods = deny)"
  - "ksci-wire: klein.science.expr.v1 node table, strict decoder, canonical form, to_jcs (the only Rust JCS encoder), sha256_hex (the only digest helper), Table::identity"
  - "ksci-wire numeral grammar (is_int, is_rat, is_float, parse_steps) and seven named atoms"
  - "ksci-wire envelopes: Request::from_canonical_slice, Budget::steps_u64, Response::to_jcs/from_slice, Status, Provenance, Record::new"
  - "schema/expr.v1.md contract text"
affects: [01-02 RFC 8785 vectors, 01-03 guards and provenance headers, 01-05 ksci-ops, 01-06 conformance, 01-08 Python JCS mirror, CORE-01]

actuals:
  tokens: 22200
  tasks: 3
  commits: 5
plan_head_before: d9eb74890980d6fb826ce365dec5d1b5337f40b9
plan_head_after: 36032aa65d3c9ce77d1a5fdece1ef987a029f11c

tech-stack:
  added: [serde =1.0.228, serde_json =1.0.150, serde_json_canonicalizer =0.3.2, sha2 =0.11.0, thiserror =2.0.21, static_assertions =1.1.0, proptest =1.11.0 (dev)]
  patterns:
    - "Iterative explicit-stack traversals only (decode, cycle check, canonicalise, JSON-number walk); no recursion"
    - "One validator: Table::new checks every non-JSON rule; from_value handles JSON positions and calls it"
    - "Envelopes decode through one helper: size cap, JSON-number positions, serde deny_unknown_fields, byte-equal JCS re-encoding"
    - "Every .rs file starts with `// provenance: original`"

key-files:
  created:
    - Cargo.toml
    - Cargo.lock
    - rust-toolchain.toml
    - crates/ksci-wire/Cargo.toml
    - crates/ksci-wire/src/lib.rs
    - crates/ksci-wire/src/numeral.rs
    - crates/ksci-wire/src/envelope.rs
    - crates/ksci-wire/tests/contract.rs
    - schema/expr.v1.md
  modified:
    - .gitignore

key-decisions:
  - "Table fields are private; Table::new is the validated constructor, so a Table value always satisfies the contract and canonicalize cannot loop"
  - "JSON numbers are checked by one iterative walker with a position predicate; tables, requests and responses share it"
  - "Response::from_slice also requires canonical bytes (same helper as requests), since response bytes are hashed"
  - "WireError::Envelope(String) carries serde's message for envelope schema errors; table errors keep their own variants"

patterns-established:
  - "RED witnesses for guards: perturb, observe the failure, revert, record command + diff + excerpt in SUMMARY"
  - "Identity oracle outside Rust: expected digests computed with shasum -a 256"

requirements-completed: [DET-01, DET-02, DET-03, DET-08]

coverage:
  - id: D1
    description: "Order-independent identity: permuted, duplicated and unreachable nodes give one canonical table and one identity"
    requirement: DET-01
    verification:
      - kind: unit
        ref: "crates/ksci-wire/tests/contract.rs#identity_order_independent, identity_duplicates_and_unreachable_nodes, identity_matches_external_sha256"
        status: pass
      - kind: unit
        ref: "crates/ksci-wire/tests/contract.rs#identity_property (proptest, 512 cases, ChaCha fixed seed)"
        status: pass
    human_judgment: false
  - id: D2
    description: "Numerals only as grammar-checked strings, seven named atoms, JSON numbers only at v/root/child indices, size cap, arity and encoding boundaries"
    requirement: DET-02
    verification:
      - kind: unit
        ref: "cargo nextest run -p ksci-wire numerals (15 tests)"
        status: pass
    human_judgment: false
  - id: D3
    description: "Provenance outside the hashed bytes; requests accepted only in canonical form"
    requirement: DET-03
    verification:
      - kind: unit
        ref: "cargo nextest run -p ksci-wire envelope (6 tests)"
        status: pass
    human_judgment: false
  - id: D4
    description: "NodeId has no ordering; adding PartialOrd or Ord fails to compile"
    requirement: DET-08
    verification:
      - kind: other
        ref: "RED witness: cargo build -p ksci-wire with #[derive(..., PartialOrd, Ord)] -> E0283 at assert_not_impl_any (see RED witnesses)"
        status: pass
    human_judgment: false
  - id: D5
    description: "schema/expr.v1.md reads correctly as the contract text that later plans and klein-rs reviewers cite"
    verification: []
    human_judgment: true
    rationale: "Prose accuracy and clarity of a contract document is a reviewer judgement; tests check the behaviour it describes, not the text"

duration: 13min
completed: 2026-10-11
status: complete
---

# Phase 1 Plan 01: ksci-wire wire contract Summary

**`ksci-wire` decodes `klein.science.expr.v1` node tables strictly. It reduces any table to one canonical form (iterative post-order, merging duplicates on their JCS bytes). Identity is SHA-256 over the `serde_json_canonicalizer` bytes. Requests are accepted only in canonical form, and provenance stays outside the hashed bytes.**

## Performance

- **Duration:** about 13 min
- **Started:** 2026-10-11T11:51:49Z
- **Completed:** 2026-10-11T12:05:07Z
- **Tasks:** 3 of 3
- **Files created or modified:** 10

## Accomplishments

- Workspace root. `rust-toolchain.toml` pins 1.99.0 with the wasm32-unknown-unknown and wasm32-wasip2 targets. Every
  dependency has an `=` pin (serde and serde_json match klein-rs). Release profile: fat LTO, one codegen unit,
  `panic = "abort"`. The workspace denies `disallowed_types` and `disallowed_methods` (plan 01-03 adds the lists).
- `ksci-wire` expression identity:
  - `NodeId` has no ordering, enforced by `assert_not_impl_any`.
  - The strict decoder rejects cycles anywhere in the table, out-of-range indices, unknown kinds and unknown atoms.
  - Canonicalisation is an iterative post-order DFS with `BTreeMap<Vec<u8>, NodeId>` dedupe.
  - `to_jcs` is the only Rust JCS encoder and `sha256_hex` the only digest helper.
  - The `x**2 + 1` identity matches a digest computed outside Rust with `shasum -a 256`.
- DET-02 grammar:
  - Hand-written byte scans for Int, Rat (grammar only, D-20), Float and step numerals.
  - JSON numbers are allowed only at `v`, `root` and child indices, each an integer below 2^32. Anywhere else the
    error is `JsonNumberNotAllowed` with a JSON pointer.
  - Non-empty names; assumption keys must match `[a-z][a-z_]*`; Add and Mul need at least 2 children.
  - Documents are capped at 64 MiB, checked before parsing.
- DET-03 envelopes:
  - `Request::from_canonical_slice` rejects any request whose bytes differ from its JCS re-encoding.
  - `Response` encodes to fixed bytes, asserted literally in the tests.
  - `Record::new` hashes only the request and response bytes. `Provenance` uses strings only.
  - Every envelope struct sets `deny_unknown_fields`, so a `provenance` key cannot get into hashed bytes.
- `schema/expr.v1.md`: the contract text, with 10 `##` sections including Envelopes.

## Task Commits

1. **Task 1: tracer (workspace root, table to canonical table to JCS to identity)**: `afb93f4` (feat)
2. **Task 2: numeral grammar, special atoms, strict decoder (TDD)**: RED `50f8bb1` (test), GREEN `e337e7c` (feat)
3. **Task 3: envelopes, canonical-input check, order-independence property (TDD)**: RED `908b09d` (test), GREEN `36032aa` (feat)

## RED witnesses

Each witness: perturb the code, run, see it fail, revert. The revert is confirmed with `diff` against a saved copy
and a green re-run.

1. **DET-08 NodeId ordering guard** (`crates/ksci-wire/src/lib.rs`)
   - Command: `cargo build -p ksci-wire`
   - Perturbation:
     ```diff
     -#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
     +#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
      pub struct NodeId(pub u32);
     ```
   - Output:
     ```
     error[E0283]: type annotations needed
       --> crates/ksci-wire/src/lib.rs:36:41
     36 | static_assertions::assert_not_impl_any!(NodeId: PartialOrd, Ord);
        |                                         ^^^^^^ cannot infer type
     note: multiple `impl`s satisfying `NodeId: AmbiguousIfImpl<_>` found
     error: could not compile `ksci-wire` (lib) due to 1 previous error
     ```
   - Deriving `PartialOrd` alone fails with the same E0283 at `assert_not_impl_any`.
   - Reverted (`diff` clean), and the build was green again.
2. **Identity dedupe** (DET-01)
   - Perturbation: the dedupe key also carries the input node index, so nothing merges:
     `key.extend_from_slice(&node.to_le_bytes())`.
   - `cargo nextest run -p ksci-wire identity`: `identity_duplicates_and_unreachable_nodes` FAIL (`left: 5, right: 4`).
   - `identity_property` FAIL; proptest shrank it to a minimal failing input.
   - Reverted.
3. **Identity over the canonical table** (DET-01)
   - Perturbation: `sha256_hex(&self.canonicalize().to_jcs())` became `sha256_hex(&self.to_jcs())`.
   - `identity_order_independent` and `identity_duplicates_and_unreachable_nodes` FAIL (5 passed, 2 failed).
   - Reverted.
4. **Canonical-input check** (DET-03)
   - Perturbation: `if false && to_jcs(&decoded)? != bytes`.
   - `cargo nextest run -p ksci-wire envelope`: `envelope_noncanonical` FAIL, the other 5 pass. Reverted.
5. **Provenance outside the digests** (DET-03)
   - Perturbation: `request_digest = sha256_hex(request ++ provenance.impl_)`.
   - `envelope_provenance_outside` FAIL (digests `845d6c13…` and `ccea6bd3…` differ). Reverted.
6. **TDD RED gates.** Both were run through `gsd-tools check tdd-red-evidence` on nextest JUnit XML, and both
   returned `RED_EVIDENCE_OK`.
   - Task 2: target `numerals_int`. 7 of the 15 numerals tests failed on their planned assertions; for example,
     `{"nodes":[["Int",""]],...}` was accepted ("should be rejected").
   - Task 3: target `envelope_noncanonical`, which failed with `left: Encode("unimplemented"), right: NonCanonical`
     against the type-only skeleton. All 6 envelope tests failed.
   - Semantic assessment: every target ran and failed on its planned assertion. There was no compile, load or
     fixture fault.
   - Tests that passed at RED cover behaviour that already shipped in Task 1 (atoms, ranges, kinds, encoding,
     identity). They are witnessed by perturbations 2 and 3, not by the RED runs.

## Files Created/Modified

- `Cargo.toml`: the workspace, with exact pins, workspace lints and the release profile.
- `Cargo.lock`: committed, so builds use `--locked`.
- `rust-toolchain.toml`: pins 1.99.0, rustfmt and clippy, and the wasm32-unknown-unknown and wasm32-wasip2 targets.
- `.gitignore`: adds `/target` and `/.venv`.
- `crates/ksci-wire/Cargo.toml`: the crate manifest, using the workspace lints.
- `crates/ksci-wire/src/lib.rs`: node table types, the decoder, the JSON-number walker, canonicalisation, `to_jcs`,
  `sha256_hex` and identity.
- `crates/ksci-wire/src/numeral.rs`: the numeral grammar.
- `crates/ksci-wire/src/envelope.rs`: Request, Budget, Response, Status, Provenance and Record.
- `crates/ksci-wire/tests/contract.rs`: 29 tests (`identity_*`, `numerals_*`, `envelope_*` and `identity_property`).
- `schema/expr.v1.md`: the contract text.

## Decisions Made

- **`Table` fields are private, and `Table::new` is the one validator.** It checks numerals, names, assumption keys,
  arity, ranges, a non-empty node list and acyclicity. JSON-specific checks (key set, `v`, number positions, kind
  and atom names) stay in `from_value`. So any `Table` value can be canonicalised safely.
- **One iterative walker checks JSON-number positions.** Tables, requests and responses each pass it a position
  predicate. Inside an envelope, an error carries the full pointer (for example `/args/expr/nodes/1/1`).
- **`Response::from_slice` enforces canonical bytes** through the same helper as requests. The plan asked for this
  only on requests, but response bytes are hashed too, and the shared helper adds no second mechanism.
- **A Float with an Int numeral's value is a distinct node** (`numerals_adjacency`). Kinds are never unified at the
  wire layer.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] The editor rewrote JSON escapes in test and schema text**
- **Found during:** Task 3 (GREEN run).
- **Issue:** When the files were written, `́` became a literal U+0301 character and `s` became `s`.
  - The `s` variant then equalled the canonical request, so `envelope_noncanonical` caught it with its own
    `assert_ne`.
  - The `́` tests stayed semantically valid (the same decoded name), but did not exercise the JSON escape the
    plan names.
- **Fix:** `perl -CSD` restored the ASCII escapes in `contract.rs` and `schema/expr.v1.md`. A non-ASCII scan now
  finds only the two intended literals: the Arabic-Indic digit in `numerals_int` and `é` in
  `numerals_assumption_keys`.
- **Files modified:** `crates/ksci-wire/tests/contract.rs`, `schema/expr.v1.md`.
- **Committed in:** `36032aa`. Commits `50f8bb1` and `e337e7c` carry the literal U+0301.

**2. [Rule 1 - Bug] A test edit hit the table's `v` instead of the envelope's**
- **Found during:** Task 3 (GREEN run).
- **Issue:** `replacen(",\"v\":1}", ...)` matched the nested table's `"v":1}` first. So the "v = 2" case and the
  "unknown key" case changed the argument table, not the envelope.
- **Fix:** both cases now edit the envelope's trailing `"v":1}` through `strip_suffix`. They assert `Version` and
  `Envelope` exactly.
- **Committed in:** `36032aa`.

**3. [Rule 2 - Missing critical] Added `WireError::Envelope(String)`**
- **Found during:** Task 3.
- **Issue:** serde's errors for envelope shape (unknown or missing field, wrong type) needed a variant. Folding
  them into `Shape` would lose serde's message.
- **Fix:** a new variant.
- **Committed in:** `36032aa`.

**4. [Test strengthening] Witnesses and checks beyond the plan**
- Added perturbation witnesses for identity dedupe, identity-over-canonical, the canonical-input check and the
  provenance digests. The plan required only the NodeId witness.
- Added an identity checked against an external `shasum` digest.
- Added tests: `numerals_kind`, `numerals_assumption_keys`, `numerals_adjacency`,
  `numerals_argument_order_preserved` and `envelope_request_rejects_bad_contents`.

---

**Total deviations:** 3 auto-fixed (2 bugs, 1 missing critical), plus added witnesses.
**Impact on plan:** none on scope. Every artifact and signature in the plan's table exists.

## Issues Encountered

- `Table::to_jcs` needs `to_jcs<T: Serialize>` without `?Sized`, because `serde_json_canonicalizer::to_vec`
  requires `Sized`. Fixed before the first commit.
- nextest once marked `numerals_boundary_arity` as LEAK (a timing artifact of the leak detector). It passed on every
  run.

## TDD Gate Compliance

- Task 2: RED `50f8bb1` `test(01-01)`, then GREEN `e337e7c` `feat(01-01)`. The RED evidence was
  `RED_EVIDENCE_OK`, target `numerals_int`.
- Task 3: RED `908b09d` `test(01-01)`, then GREEN `36032aa` `feat(01-01)`. The RED evidence was
  `RED_EVIDENCE_OK`, target `envelope_noncanonical`.
- The RED commit for Task 3 includes a type-only skeleton of `envelope.rs`, so that the tests compile and fail on
  assertions rather than on compilation.
- No refactor commits.

## Verification

- `cargo nextest run -p ksci-wire`: 29 tests run, 29 passed.
- `cargo nextest run -p ksci-wire identity`: 8 passed (including `identity_property`). `numerals`: 15 passed.
  `envelope`: 6 passed.
- `cargo clippy -p ksci-wire --all-targets --locked -- -D warnings`: clean. `cargo fmt --check`: clean.
- `cargo build --locked` on rustc 1.99.0 (b940084d7 2026-09-28): OK.
- Acceptance checks all pass. The pins are present, every file starts with `// provenance: original`, the
  `assert_not_impl_any` line appears once, `## ` headings: 10, `MAX_DOCUMENT_BYTES` in `lib.rs`: 5,
  `deny_unknown_fields` in `envelope.rs`: 6, `from_canonical_slice`: 1, `wall_ms: String`: 1, `Envelopes`: 1.

## User Setup Required

None.

## Next Phase Readiness

- Plan 01-02 (RFC 8785 vectors) can call `ksci_wire::to_jcs` and `sha256_hex` directly.
- Plan 01-03 adds the `clippy.toml` disallowed lists. The workspace lints are already set to deny them.
- The `ksci-ops` step budget can use `Table::canonicalize_with`: `step` is called once per reachable input node
  entered.

---
*Phase: 01-contracts-oracles-and-risk-probes*
*Completed: 2026-10-11*

## Self-Check: PASSED

- All 10 key files exist on disk.
- All 5 task commits (afb93f4, 50f8bb1, e337e7c, 908b09d, 36032aa) are on this branch.
- REQUIREMENTS.md: DET-02 and DET-03 marked complete. DET-01 and DET-08 are held by the shared-ID gate until sibling plans that also declare them finish.

## Review fixes

Two review findings, both confirmed: on the merged code each counter-example decoded as `Ok`. Each fix went test-first:
the RED run came first, then the fix, then a mutation witness. The suite now has 33 tests (29 + 4 new), and all pass.
`cargo clippy -p ksci-wire --all-targets --locked -- -D warnings` and `cargo fmt --check` are clean, and
`cargo build -p ksci-wire --target wasm32-wasip2` builds.

### F1: `Response::from_slice` checked only `v`

- **Counter-example (was `Ok`):**
  `{"identity":"000…000","numeric_class":"approx","result":{"nodes":[["Int","1"]],"root":0,"v":1},"status":"exact","v":1}`
- **Fix:** `Response::from_slice` now runs the new `Response::check`, which applies these rules:
  - `v` is 1, and `numeric_class` is `"exact"`.
  - Status `exact` needs a result in canonical form and exactly that result's identity.
  - Status `fuel_exhausted` carries neither a result nor an identity.
  - Errors: `WireError::InvalidResponse { path, reason }` for the field checks, and
    `WireError::IdentityMismatch { claimed, actual }` for a wrong identity.
- **Constructors:** `Response::exact(&Table)`, which canonicalises its input, and `Response::fuel_exhausted()` build
  the two valid shapes.
- **Strict reading:** the schema did not say that `result` must be canonical. It now does, so each outcome has one
  response byte string.
- **Record:** it embeds only the two digests, not a response, and it has no decode path. So nothing changed there.

### F2: duplicate object keys, last one won

- **Counter-example (was `Ok`):** `{"nodes":[["Int","999"]],"nodes":[["Int","1"]],"root":0,"v":1}`
- **Fix:** one `DeserializeSeed`/`Visitor` (`Strict` in `lib.rs`) builds the `Value`. It adds no new dependencies.
  - `parse_document` uses it, so tables, requests and responses all pass through it. `Table`'s serde `Deserialize`
    impl uses it too.
  - A repeated key, compared after unescaping, is `WireError::DuplicateKey { path }`.
  - Nesting deeper than `MAX_DEPTH = 32` is `WireError::TooDeep { path }`. That is stricter than serde_json's
    recursion limit of 128, so recursion is bounded at 32 frames. The deepest valid document nests 6 deep.
  - Paths are assembled only while an error unwinds.
  - The 64 MiB pre-parse cap is unchanged.
- **Existing test changed:** `envelope_noncanonical` no longer lists the duplicated-`op` variant, because that
  variant now fails earlier as `DuplicateKey`. The case moved to `duplicate_key_envelope`, which asserts
  `DuplicateKey { path: "/op" }`.
- **Schema:** `schema/expr.v1.md` now covers repeated keys, the depth cap (the section is renamed "Document size and
  depth caps"), the Response rules and the Canonical input wording.

### RED evidence (commit `a402acf`, run against the unfixed code)

`cargo nextest run -p ksci-wire --no-fail-fast`: 33 run, 29 passed, 4 failed. Every failure was on an assertion. A
type-only skeleton (the new `WireError` variants, plus `Response::exact` and `fuel_exhausted` as `unimplemented!`)
let the tests compile.

- `envelope_response_invariants`: the counter-example was "should be rejected, decoded as
  `Response { v: 1, status: Exact, numeric_class: "approx", … identity: Some("000…000") }`".
- `duplicate_key_table`: `{"nodes":[["Int","999"]],"nodes":[["Int","1"]],"root":0,"v":1}` should be rejected.
- `duplicate_key_envelope`: `left: NonCanonical, right: DuplicateKey { path: "/op" }`.
- `duplicate_key_depth_bound`: `left: Shape { path: "/nodes/0/2/a", … }`,
  `right: TooDeep { path: "/nodes/0/2/a/0/…/0" }`.

### Mutation witnesses (on the fixed code)

For each witness the core of the fix was reverted, the tests run, and the file restored from a saved copy. A `diff`
of each restored file was clean, and the full suite was green again (33/33).

| # | Mutation | Result |
|---|---|---|
| M1 | Remove `response.check()?` from `Response::from_slice` | `envelope_response_invariants` FAIL on the counter-example (1 of 8 envelope tests) |
| M2 | `if false && claimed != &actual` (identity check off) | FAIL: the all-zero identity with `numeric_class` `"exact"` is accepted |
| M3 | `if false && &canonical != result` (canonical-result check off) | FAIL: the result with an unreachable `Sym y` is accepted |
| M4 | `if false && map.contains_key(&key)` (duplicate check off) | `duplicate_key_table` and `duplicate_key_envelope` FAIL (31/33 pass) |
| M5 | `if false && self.depth >= MAX_DEPTH` (explicit depth bound off) | `duplicate_key_depth_bound` FAIL: `Shape` instead of `TooDeep` |
| M6 | `Table::deserialize` back to `Value::deserialize` | `duplicate_key_table` FAIL: `serde_json::from_str::<Table>` accepts the duplicate |

### Commits

- `a402acf` `test(01-01)`: RED witnesses for response invariants and duplicate keys (review F1/F2)
- `e465f6a` `fix(01-01)`: enforce response invariants and reject duplicate keys (review F1/F2)

### F3: the F1 invariants held only on `Response::from_slice` (re-review)

- **Counter-example (compiled, and serialised `"numeric_class":"approx"`):** the reviewer's struct literal
  `Response { v: 1, status: Status::Exact, numeric_class: "approx".into(), result: Some(int1), identity: Some("0"×64) }`
  followed by `response.to_jcs()`.
- **Second variant:** `serde_json::from_str::<Response>` skipped `check`, because `Deserialize` was derived. The
  same shape existed on `Request` and `Budget`, `Record` and `Provenance`. Each had public fields and a derived
  `Deserialize`, and `Record::new` hashed any `&[u8]`.
- **Fix: invariants by construction, with one mechanism.**
  - `Request`, `Response` and `Record` have private fields and read accessors.
  - `Response` has `status()`, `numeric_class()`, `result()` and `identity()`. It is built only by
    `Response::exact`, `Response::fuel_exhausted` or a decoder.
  - `Request` has `op()`, `args()`, `budget()` and `to_jcs()`, and is built by `Request::new` or a decoder.
  - `Record` has `request_digest()`, `response_digest()` and `provenance()`.
  - Each envelope has a private wire form (`RequestWire`, `ResponseWire`, `RecordWire`) and one private
    `Wire::check`. Every decode path reads the wire form and checks it with that function:
    - the byte decoders `Request::from_canonical_slice` and `Response::from_slice`;
    - all three serde `Deserialize` impls. They use the same strict parse as tables (`strict_document`, now shared
      with `Table`), the same JSON-number positions and the same `check`.
  - Only the canonical byte comparison is limited to the byte path, because serde does not expose the original
    bytes. A value read through serde still encodes to its one canonical byte string. The test pins this.
  - The public `Response::check` is gone. Its rules are now `ResponseWire::check`, unchanged.
- **Typed `Record::new(&Request, &Response, Provenance)`:** it hashes `to_jcs()` of each argument. For decoded
  inputs, those are the bytes that were read, so the digest semantics are identical. `envelope_provenance_outside`
  still asserts `sha256_hex(bytes as sent)`. This supersedes the F1 note that `Record` needed no change.
- **Parse, don't validate:**
  - `Budget` holds a `u64` (`Budget::new`, `steps()`), and `steps_u64()` is gone.
  - `Provenance.wall_ms` is a `u64`. Its fields stay public, because every value is valid.
  - Both are written as decimal numeral strings. `wall_ms` is read with the step grammar.
  - `numeral::parse_steps` is renamed `numeral::parse_u64`.
- **Other rules:**
  - `Record` digests must be 64 lowercase hex digits (`Shape` at `/request_digest` or `/response_digest`).
  - `Response`'s `result` and `identity` keys are required on every path. Serde had read a missing key as `null`.
- **Error order:** the byte decoders now check the rules before the canonical byte comparison. Before, the
  comparison came first. Every existing test case is canonical, so none changed. A document that is both
  non-canonical and invalid now reports the rule it breaks.
- **`Table`:** its fields were already private and its `Deserialize` already validated (F2). It is unchanged.
- **Schema:** `schema/expr.v1.md` now states:
  - the Record digest shape and the `wall_ms` grammar;
  - that a record is built only from checked envelopes;
  - that other readers, serde among them, apply every rule except the byte comparison;
  - that a missing or unknown key fails before the canonical-form check.

### F3 RED evidence (commit `7a4c916`, run against the unfixed code)

`cargo test -p ksci-wire --no-fail-fast`: contract 33 passed and 3 failed; doctests 0 passed and 5 failed. The tests
collect every bypass before asserting, so each count is complete.

- `serde_response_validates`: **15 of 15** invalid responses were accepted by `serde_json::from_str::<Response>`.
  That covers the 13 F1 cases, including the reviewer's counter-example, and the 2 missing-key documents.
- `serde_request_validates`: **5 of 5** were accepted: `v: 2`, steps `"007"`, `""` and `"18446744073709551616"`,
  and a repeated argument name (last one won).
- `serde_record_validates`: **6 of 7** were accepted: three bad digests (`"xyz"`, uppercase hex, 63 digits) and
  `wall_ms` values `"abc"`, `"05"` and `"-1"`. The seventh, `wall_ms` as the JSON number `5`, was rejected, but by a
  type error that did not name its path.
- Five `compile_fail` doctests on `Response`, `Request` and `Record` each reported "Test compiled successfully, but
  it's marked `compile_fail`":
  - the reviewer's struct literal;
  - `response.numeric_class = …`;
  - `request.v = 2`;
  - `Record::new(b"{}", b"not a response", p)`;
  - `record.response_digest = …`.
- **Compile-fail codes after the fix.** Stable rustdoc does not check the `E0xxx` code on a `compile_fail` block:
  a probe that changed `E0451` to `E0999` still passed. So each snippet was also compiled with `rustc` against the
  fixed rlib. Each failed with exactly its claimed code and nothing else:
  - the struct literal: **E0451** (`v`, `status`, `numeric_class`, `result` and `identity` are private);
  - the three field assignments: **E0616**;
  - `Record::new` over bytes: **E0308**.

### F3 mutation witnesses (on the fixed code)

Each core part was reverted and the suite run. The file was then restored from a saved copy, and `git diff` was clean
each time.

| # | Mutation | Result |
|---|---|---|
| M7 | `Response::deserialize` back to the unchecked wire read (the derived-`Deserialize` equivalent) | `serde_response_validates` FAIL: 13/15 accepted (the 2 missing-key cases are still caught by M12's attribute); 35/36 pass |
| M8 | `Request::deserialize` back to the unchecked wire read | `serde_request_validates` FAIL: 5/5 accepted |
| M9 | `deserialize_checked` reads `Value::deserialize` instead of `strict_document` | `serde_request_validates` FAIL: the repeated argument name is accepted |
| M10 | `if false && !is_digest(digest)` | `serde_record_validates` FAIL: the 3 bad digests are accepted |
| M11 | `wall_ms` parsed with `str::parse::<u64>().unwrap_or(0)` | `serde_record_validates` FAIL: `"abc"`, `"05"` and `"-1"` are accepted |
| M12 | drop `deserialize_with = "Option::deserialize"` on `result`/`identity` | `serde_response_validates` FAIL: both missing-key documents are accepted |
| M13 | `Response` fields made `pub` again | doctests FAIL: both `Response` `compile_fail` blocks compile (3/5 pass) |

### F3 code oracles (CLAUDE.md rule 7)

- **Semantic: compiler enumeration** (no LSP in this session). After the library change,
  `cargo check --workspace --all-targets` listed **18 errors, all in `tests/contract.rs`**. The library and every
  other target were clean.
  - 13 errors, E0616 on private fields, now replaced by the accessors:
    - `request.op`, `request.budget` (×2) and `request.args`;
    - `r.identity`;
    - 8 `Record` field reads (`provenance`, `request_digest`, `response_digest`).
  - 2 errors, E0277 and E0599 on `Budget { steps: "007" }.steps_u64()`. That line is now
    `numeral::parse_u64("007") == None`, plus a `Budget::new(u64::MAX)` encoding check.
  - 3 errors, E0308: one on `wall_ms: String` (now `u64`), and one on each `Record::new(bytes, bytes, _)` call (now
    the decoded `&Request` and `&Response`).
  - The literals in `exact_response()` and `exhausted_response()` were not in that list, because rustc reports
    E0451 in a later pass. They were found by reading the file and replaced with `Response::exact(..)` and
    `Response::fuel_exhausted()`. Their bytes stay pinned by the literal JSON in `envelope_response_shape`, which
    now also checks every accessor. The second check reported none.
  - After the fixes: `cargo check --workspace --all-targets`, `cargo check -p ksci-wire --target wasm32-wasip2` and
    clippy `-D warnings` each report **0** errors and **0** warnings.
- **Structural: tree-sitter (ast-grep 0.45.3)** over all 4 `.rs` files in the worktree (`envelope.rs`, `lib.rs`,
  `numeral.rs`, `contract.rs`):
  - Struct literals:
    - `Response { $$$ }`: **3**, all in `envelope.rs`: `ResponseWire::check` after its checks, `exact` and
      `fuel_exhausted`.
    - `Request { $$$ }`: **1** (`Request::new`).
    - `Record { $$$ }`: **2** (`RecordWire::check`, `Record::new`).
    - `Budget { $$$ }`: **1** (`Budget::new`).
    - `Provenance { $$$ }`: **1** (the test helper; its fields are public by design).
  - Constructors:
    - `Record::new($$$)`: **2**, both typed.
    - `Request::new($$$)`: **2**.
    - `Budget::new($$$)`: **2**.
    - `Response::exact($$$)` and `Response::fuel_exhausted()`: **3** each.
  - Removed API: `$X.steps_u64()` **0**, `parse_steps($$$)` **0**. `$X.check()` gives **1**, the single
    `wire.check()` in `decode`.
  - Field access:
    - `$X.v`: 3. `$X.op`: 2. `$X.args`: 2. `$X.budget`: 2. `$X.steps`: 2. `$X.status`: 3. `$X.numeric_class`: 3.
      `$X.result`: 3. `$X.request_digest`: 3. `$X.response_digest`: 3. `$X.provenance`: 2. `$X.wall_ms`: 0.
      `$X.impl_`: 0.
    - All of these are `self.*` inside `envelope.rs`.
    - `$X.identity`: 5. Four are `self.*` in `envelope.rs`. The fifth is the method `Table::identity()` in
      `contract.rs`.
  - **Gap:** tree-sitter does not parse macro bodies (`assert_eq!`, `format!`). A text scan closed it:
    - non-call field access to any envelope field in `contract.rs`, macro bodies included: **0**;
    - doc comments and doctests: **6** lines, all inside the 5 `compile_fail` blocks, which are the negative
      witnesses;
    - `schema/expr.v1.md`: every mention of `numeric_class`, `identity`, `result`, `budget`, `request_digest`,
      `response_digest` and `wall_ms` agrees with the code.
- **Suite:** 36/36 `cargo nextest run -p ksci-wire` (33 + 3 new) and 5/5 doctests pass.
  `cargo fmt --check` and `cargo build -p ksci-wire --target wasm32-wasip2` are clean.

### F3 commits

- `7a4c916` `test(01-01)`: RED witnesses for serde and struct-literal bypasses of envelope invariants (review F3)
- `3058308` `fix(01-01)`: envelope invariants by construction on every path (review F3)
