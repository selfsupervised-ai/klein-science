---
phase: 01-contracts-oracles-and-risk-probes
plan: 03
subsystem: tooling
tags: [rust, clippy, cargo-deny, licences, provenance, notice, determinism, xtask]

requires:
  - phase: 01-01
    provides: "[workspace.lints.clippy] disallowed_types/disallowed_methods = deny; ksci-wire opts in with [lints] workspace = true"
  - phase: 01-02
    provides: "cargo xtask spine: ci::STEPS, check no-corpus, host::git_output, workspace::root, pinned cargo-deny 0.20.2 and cargo-nextest"
provides:
  - "clippy.toml: disallowed-types (std and hashbrown HashMap/HashSet, std RandomState, foldhash RandomState) and disallowed-methods (SystemTime::now, Instant::now, rand thread_rng/random/rng) (DET-08)"
  - "deny.toml: permissive licence allow-list, bans (rug, gmp-mpfr-sys, malachite*, flint-sys, algebraeon, symbolica, internment, getrandom, rand), yanked denied, unknown registries and git denied; dev-deps and xtask/ksci-conform/ksci-metal-probe excluded (LIC-01)"
  - "cargo xtask check provenance with the PORT_POLICY licence table and the NOTICE cross-check (LIC-02)"
  - "NOTICE with the klein-science notice and the 'Ported code notices' section (no Project blocks yet)"
  - "cargo xtask ci: 8 steps, now including cargo deny check (after clippy) and check provenance (after no-corpus)"
affects: [01-06 ksci-conform (deny exclude already lists it), 01-11 ksci-metal-probe (same), 01-12 stand-in name table port (adds Project: sympy), every plan adding a crate or a ported file]

actuals:
  tokens: 7200
  tasks: 2
  commits: 3
plan_head_before: 46c5bc6e6e4a686f4fa0ee7726e84e465f0112d7
plan_head_after: 5b6a7a5c810f64d66d3dbd4d60116dcd2e81df9c

tech-stack:
  added: []
  patterns:
    - "Every guard is seen RED on a deliberate violation in the real tree before it is trusted; the perturbation is reverted and git status --porcelain crates/ is checked empty"
    - "Guard scope is decided in Rust over git ls-files --cached --others --exclude-standard, matching directory and extension on the ASCII-lowercased path, so a case variant is judged, never exempt"
    - "Policy tables are one const (PORT_POLICY) cited from CLAUDE.md; NOTICE membership is an exact-line match"

key-files:
  created:
    - clippy.toml
    - deny.toml
    - NOTICE
  modified:
    - xtask/src/check.rs
    - xtask/src/ci.rs
    - .planning/REQUIREMENTS.md

key-decisions:
  - "allow-invalid only on the three rand entries: clippy 1.99 is silent about paths whose crate is absent (hashbrown, foldhash), but reports rand paths as unreachable in test targets, where rand arrives through proptest without the thread_rng/std features (and rand::rng resolves to rand's private rng module)"
  - "deny.toml keeps all three graph excludes (xtask, ksci-conform, ksci-metal-probe): cargo-deny 0.20.2 accepts a spec that is not in the graph yet, so the fallback to xtask-only was not needed"
  - "cargo-deny rejects unknown config keys (probed with a misspelt exclude-devv: error[unexpected-keys], exit 1), so exclude-dev and yanked are confirmed as real 0.20.2 keys"
  - "The ci deny step names all four checks explicitly (advisories bans licenses sources); cargo finds cargo-deny in $CARGO_HOME/bin like cargo nextest"
  - "check provenance judges the working copy (what cargo compiles); a listed file it cannot read, or a missing NOTICE, is an error, never a skip"
  - "Header grammar is exact: one space after the comment marker and after 'provenance:', no trailing whitespace or CR; a Rust file with a '#' header has no header; DOI must be 10.<digits.dots>/<non-empty suffix>"
  - "A port whose project is outside PORT_POLICY reports only that reason; an allowed port reports the SPDX mismatch and the missing NOTICE line separately, in that order"

patterns-established:
  - "RED witness record: command, perturbation diff, failing-output excerpt, revert confirmation"
  - "TDD on xtask: RED via nextest JUnit report classified by gsd-tools check tdd-red-evidence (RED_EVIDENCE_OK) before GREEN"

requirements-completed: [DET-08, LIC-01, LIC-02]

coverage:
  - id: D1
    description: "clippy rejects HashMap/HashSet/RandomState types and clock calls in every crate with workspace lints, seen RED on ksci-wire"
    requirement: DET-08
    verification:
      - kind: other
        ref: "RED witness a: HashMap + Instant::now test fn in crates/ksci-wire/tests/contract.rs -> cargo clippy -p ksci-wire --all-targets -- -D warnings exit 101 (2 x disallowed type, 1 x disallowed method); reverted"
        status: pass
      - kind: other
        ref: "cargo clippy --workspace --all-targets --locked -- -D warnings (clean tree, run twice: exit 0, exit 0)"
        status: pass
    human_judgment: false
  - id: D2
    description: "cargo-deny rejects rug (banned + LGPL) and getrandom (banned) in the shipped graph and passes the clean tree"
    requirement: LIC-01
    verification:
      - kind: other
        ref: "RED witness b: rug =1.30.0 in ksci-wire -> cargo deny check licenses bans exit 6 (rug and gmp-mpfr-sys banned, both LGPL-3.0+ rejected); reverted"
        status: pass
      - kind: other
        ref: "RED witness c: getrandom 0.3 in ksci-wire -> cargo deny check bans exit 2 (getrandom 0.3.4 banned); reverted"
        status: pass
      - kind: other
        ref: "cargo deny check (clean tree, run twice: exit 0, exit 0; advisories ok, bans ok, licenses ok, sources ok)"
        status: pass
    human_judgment: false
  - id: D3
    description: "check provenance enforces the header grammar, licence policy and NOTICE cross-check, sorted output, seen RED"
    requirement: LIC-02
    verification:
      - kind: unit
        ref: "cargo nextest run -p xtask provenance (11 tests: 11 passed)"
        status: pass
      - kind: other
        ref: "RED witness d: untracked headerless crates/ksci-wire/src/witness.rs -> cargo xtask check provenance exit 1 naming the file; deleted"
        status: pass
      - kind: other
        ref: "cargo xtask check provenance on the clean tree: exit 0, 5 files checked"
        status: pass
    human_judgment: false
  - id: D4
    description: "NOTICE carries the project notice and the Ported code notices section explaining Project: {name} blocks"
    requirement: LIC-02
    verification:
      - kind: other
        ref: "grep -c 'Project: {name}\\|Ported code notices' NOTICE -> 2"
        status: pass
    human_judgment: true
    rationale: "Whether the NOTICE wording is adequate for the Apache-2.0 and BSD-3 notice obligations is a legal-text judgement no test asserts"
  - id: D5
    description: "cargo xtask ci runs 8 steps including cargo deny check and check provenance, all passing"
    verification:
      - kind: integration
        ref: "cargo xtask ci -> 'ci: all 8 steps passed', exit 0 (nextest 94 tests passed)"
        status: pass
    human_judgment: false

duration: 9min
completed: 2026-10-11
status: complete
---

# Phase 1 Plan 03: Determinism and licence guards Summary

**clippy now rejects hash-ordered maps, random hash seeds and clock calls in every crate that opts into the workspace
lints. cargo-deny rejects the GPL/LGPL crates, Symbolica, internment and the randomness crates (getrandom, rand),
plus any licence outside a permissive allow-list, in the shipped graph. `cargo xtask check provenance` requires a
provenance header on every shipped `.rs` and stand-in `.py` file and ties port headers to a const licence policy and
to `NOTICE`. All three run in `cargo xtask ci`, and each was seen RED on a deliberate violation before it was
trusted.**

## Performance

- **Duration:** about 9 min (2026-10-11T19:26:37Z to 2026-10-11T19:35Z)
- **Tasks:** 2 of 2
- **Files created or modified:** 6 (clippy.toml, deny.toml, NOTICE, xtask/src/check.rs, xtask/src/ci.rs, .planning/REQUIREMENTS.md)

## Accomplishments

- **clippy.toml (DET-08).**
  - `disallowed-types`: std `HashMap`, `HashSet`, `std::hash::RandomState`, `std::collections::hash_map::RandomState`, `hashbrown::{HashMap,HashSet}` and `foldhash::fast::RandomState`. Each entry has a `reason` naming DET-08.
  - `disallowed-methods`: `SystemTime::now`, `Instant::now`, `rand::thread_rng`, `rand::random` and `rand::rng`.
  - The lint levels come from `[workspace.lints.clippy]` (plan 01-01). The witness failed even without `-D warnings`, so the `deny` level alone is enough.
- **deny.toml (LIC-01, and DET-08 for the randomness crates).**
  - `[graph]`: `exclude-dev = true` and `exclude = ["xtask", "ksci-conform", "ksci-metal-probe"]`. Since the review fixes there is no target filtering (`targets = []`, W1), and every optional feature is audited (`all-features = true`, W2).
  - Licences: the eight-entry permissive allow-list with confidence 0.93. MPL-2.0 is rejected, and a comment says it needs an operator-approved exception (flag F1).
  - Bans: 13 crates, each with a reason. `wildcards = "deny"`, `multiple-versions = "warn"`.
  - Yanked crates are denied, and so are unknown registries and git sources.
  - getrandom and rand are already in `Cargo.lock` through proptest (a dev-dependency). The clean tree still passes, which shows that `exclude-dev` works.
- **`cargo xtask check provenance` (LIC-02).**
  - As first built, it listed files with `git ls-files`. Since review fix W3' it judges the Rust files rustc actually read, taken from cargo dep-info for an all-targets, all-features workspace compile, and walks `standin/` on disk for `.py` files. See "Review fixes".
  - The header is line 1, or line 2 of a `.py` file whose first line is a shebang or a PEP 263 coding cookie. It has one of three forms: `original`, `port <project>@<tag>:<path> (<SPDX>)` or `paper doi:<doi>`.
  - `PORT_POLICY` sets sympy, mpmath, diofant and fricas to BSD-3-Clause, and symengine to MIT.
  - A port also needs an exact `Project: <project>` line in NOTICE.
  - Violations are printed as `<path>: <reason>`, sorted by path, and the check exits 1 if there are any.
- **NOTICE.** It holds the klein-science copyright, the Apache-2.0 statement and the "Ported code notices" section, which explains the `Project: {name}` blocks. It has no blocks yet; plan 01-12 adds `Project: sympy`.
- **`cargo xtask ci`** now runs 8 steps: tools check, fmt, clippy, **deny**, store fetch rfc8785, nextest, check no-corpus, **check provenance**. Every step passed.

## RED witnesses

Each perturbation was made in the real tree, observed failing, and reverted. After each one,
`git status --porcelain crates/` was empty. After witnesses b and c, `Cargo.lock` was restored with `git checkout`
as well.

**a. clippy, HashMap and Instant::now (DET-08)**

Perturbation (crates/ksci-wire/tests/contract.rs):
```diff
+#[test]
+fn det08_red_witness() {
+    let mut m: std::collections::HashMap<u32, u32> = std::collections::HashMap::new();
+    m.insert(1, 2);
+    let _t = std::time::Instant::now();
+    assert_eq!(m.len(), 1);
+}
```
Command: `cargo clippy -p ksci-wire --all-targets -- -D warnings`. Exit 101:
```
error: use of a disallowed type `std::collections::HashMap`
    --> crates/ksci-wire/tests/contract.rs:1151:54
     = note: DET-08: hash-ordered iteration is nondeterministic; sort canonically or use BTreeMap
     = note: requested on the command line with `-D clippy::disallowed-types`
error: use of a disallowed type `std::collections::HashMap`
    --> crates/ksci-wire/tests/contract.rs:1151:16
error: use of a disallowed method `std::time::Instant::now`
    --> crates/ksci-wire/tests/contract.rs:1153:14
     = note: DET-08: monotonic-clock time makes a result depend on the run; budgets are logical step counts
     = note: requested on the command line with `-D clippy::disallowed-methods`
error: could not compile `ksci-wire` (test "contract") due to 3 previous errors
```
Without `-D warnings`, the run also exits 101 with the same 3 errors: the workspace `deny` level does the work.
Reverted with `git checkout -- crates/ksci-wire/tests/contract.rs`; `git status --porcelain crates/` was empty.

**b. cargo-deny, rug (LIC-01)**

Perturbation (crates/ksci-wire/Cargo.toml):
```diff
 thiserror = { workspace = true }
+rug = "=1.30.0"
```
Command: `cargo deny check licenses bans`. Exit 6:
```
error[rejected]: failed to satisfy license requirements
   ┌─ .../gmp-mpfr-sys-1.7.1/Cargo.toml:39:12
39 │ license = "LGPL-3.0+"
   │            rejected: license is not explicitly allowed
error[rejected]: failed to satisfy license requirements
   ┌─ .../rug-1.30.0/Cargo.toml:38:12
38 │ license = "LGPL-3.0+"
   │            rejected: license is not explicitly allowed
error[banned]: crate 'rug = 1.30.0' is explicitly banned
   ├ rug v1.30.0
     └── ksci-wire v0.1.0
error[banned]: crate 'gmp-mpfr-sys = 1.7.1' is explicitly banned
bans FAILED, licenses FAILED
```
Reverted with `git checkout -- crates/ksci-wire/Cargo.toml Cargo.lock`; `git status --porcelain crates/ Cargo.lock` was empty.

**c. cargo-deny, getrandom (DET-08)**

Perturbation (crates/ksci-wire/Cargo.toml):
```diff
 thiserror = { workspace = true }
+getrandom = "0.3"
```
Command: `cargo deny check bans`. Exit 2:
```
error[banned]: crate 'getrandom = 0.3.4' is explicitly banned
74 │     { crate = "getrandom", reason = "DET-08: OS randomness makes results differ per run; ..." },
   │                banned here
   ├ getrandom v0.3.4
     └── ksci-wire v0.1.0
bans FAILED
```
Reverted with `git checkout -- crates/ksci-wire/Cargo.toml Cargo.lock`. The lockfile had gained one line, the new
dependency edge. `git status --porcelain crates/ Cargo.lock` was empty.

**d. check provenance, untracked headerless file (LIC-02)**

Perturbation: an untracked file `crates/ksci-wire/src/witness.rs` containing `pub fn witness() {}` and no header.
Command: `cargo xtask check provenance`. Exit 1:
```
crates/ksci-wire/src/witness.rs: no provenance header
error: provenance: 1 violation(s); every .rs file under crates/ and .py file under standin/ starts with a provenance header
```
I deleted the file. `git status --porcelain crates/` was empty, and the re-run exited 0.

A second real-tree witness, d', checked a port header whose project has no NOTICE block. The same file, with
`// provenance: port sympy@sympy-1.14.0:sympy/core/add.py (BSD-3-Clause)`, gave
`crates/ksci-wire/src/witness.rs: port sympy has no "Project: sympy" line in NOTICE` and exit 1. I deleted the file.

## TDD (Task 2)

- **RED** (`542fe34`, `test(01-03)`).
  - 11 `provenance_*` unit tests over an injected file list and NOTICE text: the 9 behaviours in the plan, plus `provenance_malformed` and `provenance_scope`.
  - The stub returned `<path>: unimplemented` for every file, and `provenance_in_scope` returned false.
  - `cargo nextest run -p xtask provenance --no-fail-fast` with a JUnit profile exited 100: 11 tests, 0 passed, 11 failed.
  - `gsd-tools check tdd-red-evidence` returned `RED_EVIDENCE_OK` for `provenance_missing`, `provenance_port_not_in_notice` and `provenance_sorted`.
  - Semantic assessment: every failure is an `assertion left == right failed` on the planned output. Examples are `left: ["standin/names.py: unimplemented"]` against `right: ["standin/names.py: port sympy has no \"Project: sympy\" line in NOTICE"]`, and input order against the expected path order for `provenance_sorted`. There was no compile, fixture or collection fault.
- **GREEN** (`5b6a7a5`, `feat(01-03)`): the real check, `CheckCmd::Provenance`, NOTICE and the ci step. All 11 tests pass.
  - After RED I added two case-variant assertions to `provenance_scope` (`Crates/x/src/a.rs` and `STANDIN/a.Py`). The RED stub returned false for every path, so they would also have failed then.
- **REFACTOR:** none was needed.

## Task Commits

1. **Task 1: Tracer, clippy and cargo-deny guards wired into xtask ci, each seen RED:** `003c1ed` (feat)
2. **Task 2: provenance-header check with licence policy and NOTICE:** `542fe34` (test, RED) and `5b6a7a5` (feat, GREEN)

**Tracer feedback gate.** The run was interactive (`auto_advance` false), the mode was `end-of-phase`, and the verify was automated only. I re-ran the tracer verify, `cargo clippy --workspace --all-targets --locked -- -D warnings` and `cargo deny check`, twice. Both runs exited 0, so the plan moved on to Task 2.

## Files Created/Modified

- `clippy.toml`: the disallowed-types and disallowed-methods lists (DET-08).
- `deny.toml`: graph scope, licence allow-list, bans, advisories and sources (LIC-01).
- `NOTICE`: the project notice and the "Ported code notices" section (LIC-02).
- `xtask/src/check.rs`: `CheckCmd::Provenance`, `PORT_POLICY`, `provenance_in_scope`, `provenance_violations` and 11 tests.
- `xtask/src/ci.rs`: the `deny` step after clippy, and the `check provenance` step after no-corpus.
- `.planning/REQUIREMENTS.md`: DET-08 and LIC-01 marked complete. LIC-02 was held back by the shared-ID gate, because a sibling plan still declares it.

## Decisions Made

See `key-decisions` in the frontmatter. The two with the most effect:
- **`allow-invalid`** went only on the rand entries, because clippy flagged only those. It is silent about hashbrown and foldhash while they are absent from the graph.
- **The graph excludes** list all three crates now, since cargo-deny 0.20.2 accepts a spec that is not in the graph yet. The fallback the plan allowed was not needed.

## Deviations from Plan

None that changed the plan's scope. Notes:
- **Scope selection.** The plan scoped the check with the git pathspec `'crates/**/*.rs' 'standin/**/*.py'`. I list all non-ignored files instead and filter them in Rust, with case-insensitive directory and extension matching. That is a superset of the pathspec: it also catches `crates/x.rs` at depth 1 and case variants such as `Crates/…`, which follows the plan 01-02 R1' precedent. Behaviour on the current tree is identical: the same 5 files.
- **Extra tests.** I wrote 11 tests rather than the minimum of 9.
- **`cargo deny check` step.** It names the four checks explicitly. That is equivalent to the bare command.

**Total deviations:** 0 auto-fixed. **Impact:** none.

## Issues Encountered

- **rust-analyzer unavailable.** The `LSP` tool returned "No LSP server available for file type: .rs" in this session, so the semantic oracle was not available (CLAUDE.md rule 7).
  - Fallbacks: `cargo clippy --workspace --all-targets` (a full compile of every target) and the ast-grep structural scans.
  - ast-grep found `STEPS` used only in ci.rs, at the definition, the loop and `len`, plus the test. It found `CheckCmd::*` matched only in `check::run`.
  - It found `provenance_in_scope`, which is called only from `provenance()` and the tests, and `provenance_violations`, which is called only from `provenance()` and the test helper.
  - This plan makes no every-site claim (no rename, removal or wire change).
- **rand entries in clippy.toml.** `rand::rng` resolves to rand's private `rng` module, not the `rng()` function. Those entries are therefore only a backstop. The binding guard for rand is the cargo-deny ban, which was seen RED via getrandom and which covers rand by the same mechanism.

## Known Stubs

None.

## User Setup Required

None. No external service configuration is required.

## Next Phase Readiness

- Plans 01-06 (ksci-conform) and 01-11 (ksci-metal-probe) are already excluded from the deny graph.
- New crates must opt into `[lints] workspace = true` and start every `.rs` file with a provenance header.
- Plan 01-12 must add a `Project: sympy` block with SymPy's BSD-3 notice verbatim before its first port header passes.
- LIC-02 stays unchecked in REQUIREMENTS.md until the sibling plan that also declares it finishes.

---
*Phase: 01-contracts-oracles-and-risk-probes*
*Completed: 2026-10-11*

## Self-Check: PASSED

- Files present: clippy.toml, deny.toml, NOTICE, xtask/src/check.rs, xtask/src/ci.rs, this SUMMARY.
- Commits on this branch since 46c5bc6: 003c1ed, 542fe34, 5b6a7a5 (3).
- Re-run on the final tree: `cargo xtask ci` exit 0 (8 steps); `cargo nextest run -p xtask provenance` 11/11 passed;
  `cargo xtask check provenance` exit 0; `git status --porcelain crates/` empty.

## Review fixes

A cross-model review (Codex gpt-6-astra) returned FAIL with three must-fix warnings. The orchestrator reproduced all
three. Each fix below was checked with the reviewer's own counter-example (the oracle): it passed before the fix, it
fails after, and every mutation was reverted afterwards. `git status --porcelain` was clean after each revert.

**W1: the deny graph checked the wrong wasm target** (`86c5c25`, fix).
- The target list named `wasm32-unknown-unknown`, but the shipped R0 target is `wasm32-wasip2`. Dependencies declared only for wasip2 were never audited.
- Fix: `deny.toml` `targets = []`, so there is no target filtering and every target-specific dependency is audited. There is no target list to keep in step with the shipped targets.
- Oracle: add `[target.wasm32-wasip2.dependencies] getrandom = "=0.3.4"` to `crates/ksci-wire/Cargo.toml`.
  - Before: `cargo deny check bans` returned **rc=0**. `cargo deny --target wasm32-wasip2 check bans` returned rc=2, so the miss came from the target filter.
  - After: `cargo deny check bans` returns **rc=2**, with `error[banned]: crate 'getrandom = 0.3.4' is explicitly banned`.
  - Reverted with `git checkout -- crates/ksci-wire/Cargo.toml Cargo.lock`. The clean tree's `cargo deny check` returned rc=0.

**W2: optional features were never audited** (`018966a`, fix).
- With `all-features = false`, a banned crate behind an optional feature was invisible to the check.
- Fix: `deny.toml` `all-features = true`. Every `cargo deny check`, including the ci step, now audits the all-features graph.
- Oracle: add `getrandom = { version = "=0.3.4", optional = true }` and `[features] runtime-entropy = ["dep:getrandom"]` to ksci-wire.
  - Before: plain `cargo deny check bans` returned **rc=0**, while `--all-features` returned rc=2.
  - After: plain `cargo deny check bans` returns **rc=2**, with `getrandom = 0.3.4` banned (`getrandom v0.3.4 └── ksci-wire v0.1.0`).
  - Reverted. The clean tree's `cargo deny check` returned rc=0.

**W3: directory symlinks hid compiled source from `check provenance`** (RED `347f068`, test; GREEN `68b231f`, fix).
- The bug: git lists a symlinked directory as one path with no extension, and the scope filter dropped it. A `#[path]` module could then compile headerless code from behind the link.
- Fix: one mechanism, which is refusal.
  - Every non-ignored path under `crates/` or `standin/` is examined with `symlink_metadata`, which does not follow links. This includes the tree roots themselves, in any letter case.
  - Any symbolic link is a violation, `<path>: symbolic link inside a checked source tree; ...`. The check never follows a link.
  - `provenance_violations` now judges `Entry::Bytes` (headers) and `Entry::Symlink` (refusal) in one list sorted by path. I named the new enum `Entry`, not `Source`, after ast-grep showed that `store::Source` already exists.
- RED: three integration tests in `xtask/tests/review_bypass.rs` run the built binary in a scratch klein repository.
  - `provenance_rejects_directory_symlink_in_source_tree` (linked dir plus `git add -N`).
  - `provenance_rejects_file_symlink_in_source_tree` (an untracked `.rs` link to a *headed* file outside the trees).
  - `provenance_rejects_symlinked_standin_tree` (the `standin` root itself is a link).
  - Against the unfixed code, nextest exited 100 with 3 of 3 failing on the planned assertion: `check provenance passed: status: exit status: 0 ... provenance ok`. There was no fixture fault.
- GREEN: all 3 pass. A new unit test, `provenance_checked_trees`, covers the tree-membership predicate. `cargo nextest run -p xtask provenance` now runs 15 tests, all passing.
- Real-tree oracle: `review-fixture/witness.rs` with no header, a link `crates/ksci-wire/src/imported -> ../../../review-fixture`, `#[path = "imported/witness.rs"] pub mod provenance_bypass;` appended to ksci-wire's `lib.rs`, then `git add -N .`.
  - Before: `cargo xtask check provenance` returned **rc=0** ("5 files checked"), while `cargo check -p ksci-wire` compiled the file.
  - After: **rc=1**, with `crates/ksci-wire/src/imported: symbolic link inside a checked source tree; ...`.
  - Reverted with `git reset` on the intent-to-add entries, removal of the link and fixture, and `git checkout -- crates/ksci-wire/src/lib.rs`.

**Tree after W1 to W3.** `cargo xtask ci` passed: `ci: all 8 steps passed`, exit 0, and nextest ran 98 tests with 98 passing. Clippy and fmt are clean. rust-analyzer was still unavailable in this session, so structural checks used ast-grep (`provenance_violations` and `Entry::*` call sites, and the `store::Source` clash) together with the full clippy compile.

### Second review: W3' (supersedes the W3 mechanism)

The re-review confirmed that W1 and W2 are closed: the orchestrator re-ran both oracles and got rc 2, and the clean tree gave rc 0. W3 was only partly closed. Two escapes remained, both because the check used git locations:
- **(a)** A link outside `crates/`, `xtask/review-link -> ../review-fixture`, reached from ksci-wire by `#[path = "../../../xtask/review-link/witness.rs"]`. **(a')** is the same link reached by `include!`.
- **(b)** A link inside `crates/` that is listed in `.gitignore`, so `--exclude-standard` hid it.

The operator ruled on 2026-10-11 that the check must judge what rustc actually read.

**Mechanism** (`9743790`, fix; RED `c38ca11`, test).
- **The compile.** The new `xtask/src/depinfo.rs` runs one `cargo check --workspace --all-targets --all-features --locked --message-format=json-render-diagnostics --target-dir target/provenance` with the pinned cargo, as the rest of xtask does.
  - For every `compiler-artifact` this compile reports, fresh or rebuilt, it reads rustc's dep-info file. That covers workspace crates, dependencies, proc macros and build scripts.
  - The `.d` file sits next to the artifact: `lib<crate>-<hash>.rmeta` maps to `<crate>-<hash>.d`, and `build/<pkg>-<hash>/build-script-build` maps to `build_script_build-<hash>.d`.
  - The `.d` files are never found by listing a directory, so stale ones left by earlier compiles are never read. A missing `.d` file, or a failed compile, is an error.
- **Classification.** Each input path is canonicalised, which follows links, because rustc read the target.
  - Inside the repository, a regular `.rs` file is judged with the same header parser, `PORT_POLICY` and NOTICE cross-check as before.
  - Outside the repository, an input is allowed only under `$CARGO_HOME/registry/src`, `$CARGO_HOME/git/checkouts` or the pinned toolchain's sysroot. Anything else is a violation that names the absolute path.
- **The rule for non-`.rs` inputs in the repository (decided here).** Data read through `include_str!` or `include_bytes!` is allowed, and not judged as source, only under a `testdata/` directory. That matches the existing no-corpus convention. A non-`.rs` input anywhere else in the repository is a violation: `non-Rust compiler input outside testdata/`.
  - Build-script output under the provenance target directory (`OUT_DIR`) is allowed. The build script that wrote it is itself a judged input.
- **Reporting.** A violation names the real file and the names rustc used for it, for example `review-fixture/witness.rs: no provenance header (rustc read it as crates/ksci-wire/src/review-link/witness.rs)`. Output is sorted by path.
- **Python side.** It now walks `standin/` on disk, so ignored files are included and there is no git listing. Symbolic links anywhere in `standin/`, the root included, are still refused. The header rules are unchanged.
- **One mechanism.** The git-listing path for `.rs` files was removed, together with `provenance_in_scope`, `in_checked_tree`, `CHECKED_TREES`, `Entry::Symlink` and their unit tests.
  - Every-site evidence: after the removal, ast-grep finds no occurrence of any of these names, and the workspace clippy compile is clean.
  - The workspace compile also covers xtask. Its 8 sources carry headers, and judging it costs nothing.
- **Changed behaviour on purpose.** A link to a *headed* file inside the repository now passes, because the check judges the content rustc read. The W3 test that expected that case to be refused was rewritten to link a headerless file instead.

**Witnesses.** 12 integration tests in `xtask/tests/review_bypass.rs` each build a real two-member cargo workspace in a scratch repository, about 1.3 s per test.

| Case | Test | Before (code at c38ca11) | After (9743790) |
|---|---|---|---|
| (a) `#[path]` through a link outside crates/ | `provenance_rejects_path_module_through_link_outside_crates` | exit 0, passed (RED) | exit 1, `review-fixture/witness.rs: no provenance header` |
| (a') `include!` through that link | `provenance_rejects_include_through_link_outside_crates` | exit 0 (RED) | exit 1, same |
| (b) gitignored link in crates/ | `provenance_rejects_gitignored_link` | exit 0 (RED) | exit 1, same |
| directory link in crates/ plus `#[path]` | `provenance_rejects_directory_symlink_in_source_tree` | exit 1, but naming the link rather than the file read (RED on the assertion) | exit 1, names `review-fixture/witness.rs` |
| `.rs` file link to a headerless file | `provenance_rejects_file_symlink_to_headerless_source` | exit 1, naming the link (RED on the assertion) | exit 1, names the target |
| link to a headed file (must pass) | `provenance_judges_a_link_by_the_file_rustc_read` | exit 1 (RED) | exit 0 |
| input outside the repository | `provenance_rejects_input_outside_the_repository` | exit 0 (RED) | exit 1, `outside the repository` |
| non-`.rs` input outside testdata/ | `provenance_data_inputs_only_under_testdata` | exit 0 (RED) | exit 1 for `src/data.txt`; `testdata/v.json` not reported |
| ignored `standin/*.py` without a header | `provenance_judges_ignored_standin_files` | exit 0 (RED) | exit 1 |
| headerless ordinary module | `provenance_rejects_headerless_module` | exit 1 (already green) | exit 1 |
| symlinked `standin` root | `provenance_rejects_symlinked_standin_tree` | exit 1 (already green) | exit 1 |
| clean workspace | `provenance_passes_a_clean_workspace` | exit 0 (already green) | exit 0 |

- **RED run** at c38ca11: nextest exited 100, with 9 of 12 failing on their assertions. Six showed `check provenance passed ... exit status: 0`, two `did not report "review-fixture/witness.rs"`, and one, the headed-link case, an unexpected exit 1. There was no setup fault.
- **GREEN run:** 12 of 12 pass. Two new depinfo unit tests (`.d` parsing including escaped spaces and comments, and the artifact-to-`.d` mapping for each artifact kind) also pass.
- **The reviewer's oracles on the real tree, after the fix:**
  - (a) `#[path]` through `xtask/review-link`: rc=1.
  - (a') `include!` through the same link: rc=1.
  - (b) `crates/ksci-wire/src/review-link` listed in `.gitignore`: rc=1.
  - Each reported `review-fixture/witness.rs: no provenance header (rustc read it as …)`.
  - All mutations were reverted and the clean tree's rc was 0.
  - The real-tree "before" numbers come from the RED tests above, which run the same constructions against the old binary. I did not rebuild the old binary separately.

**Residual risks, recorded rather than hidden.**
- **Host cfg only.** The dep-info compile covers only the host target's cfg. A module behind, for example, `#[cfg(target_arch = "wasm32")]` is not read by the host compile, so it is not judged. A second compile for `wasm32-wasip2` (shipped crates only) would close this for R0. It is not done here, because test-only crates such as proptest and rusty-fork are not known to build for wasip2.
- **testdata/ inputs are not judged.** An `include!` (not `include_str!`) of a file under `testdata/` would compile unjudged code. dep-info does not say which macro read a file.
- **The Python side has no dep-info equivalent.** A `.py` file outside `standin/`, imported through `sys.path` tricks, is not seen.

**Final tree.** `cargo xtask ci` passed: `ci: all 8 steps passed`, exit 0. nextest ran 107 tests with 107 passing. `check provenance` reported `13 Rust sources rustc read and 0 standin files judged`. It runs last in ci, with its own compile into `target/provenance`, which took about 4 s on a warm cache. rust-analyzer was unavailable, so every-site claims rest on the clippy and compile enumeration plus ast-grep.
