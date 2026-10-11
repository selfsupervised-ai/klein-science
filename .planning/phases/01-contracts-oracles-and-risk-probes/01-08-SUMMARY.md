---
phase: 01-contracts-oracles-and-risk-probes
plan: 08
subsystem: wire
tags: [python, jcs, rfc8785, standin, host-shim, ksci-worker, uv, differential]

requires:
  - phase: 01-04
    provides: "ksci_wire::frame codec, ksci-worker (R2 shell), testdata/wire/requests.jsonl (18 canonical requests)"
  - phase: 01-01
    provides: "ksci-wire canonical form and identity (schema/expr.v1.md)"
provides:
  - "standin/sympy/_wire.py: the one Python JCS encoder (dumps) and canonicalize, mirroring ksci-wire"
  - "standin/sympy/_tool.py: the single tool-call entry point call(op, args, steps) with a pluggable _transport"
  - "standin/host_shim: CPython-only frame transport to ksci-worker (KSCI_WORKER), with a call counter; never shipped"
  - "conformance/python/dev.in + dev.lock: hash-locked dev test oracles (rfc8785 0.1.4, pydantic-monty 1.1.0 + client + runtime, typing-extensions 4.16.0), operator-approved 2026-10-12"
  - "cargo xtask tools venv: .venv on uv-managed CPython 3.13.16, synced to dev.lock with --require-hashes --no-build"
  - "Three-way differential: Rust worker == Python mirror == rfc8785 over the whole request corpus"
affects: [01-12 stand-in name table and Monty-subset lint, 01-17 Monty census, 01-16 bundle contents, DET-01, GUEST-02]

actuals:
  tokens: 10940
  tasks: 3
  commits: 5
plan_head_before: 798d3e18dbedc94153158048c7946c5916d71a34
plan_head_after: 0e7080003b84e558def6930735fff841580dd224

tech-stack:
  added: ["rfc8785 0.1.4 (dev test oracle only)", "pydantic-monty 1.1.0 (+ -client, -runtime; dev only)", "typing-extensions 4.16.0 (transitive)", "CPython 3.13.16 (uv-managed, dev venv)"]
  patterns:
    - "One encoder per language: Rust serde_json_canonicalizer, Python sympy._wire; rfc8785 appears only in tests"
    - "Guest code under standin/sympy keeps the Monty subset: no classes, decorators, generators, match or del; imports json only"
    - "CPython-only host code lives in standin/host_shim, outside the sympy package"
    - "Dev Python environments come only from `cargo xtask tools venv` (pinned uv, exact CPython patch, hash-locked)"
    - "Mutation runs set PYTHONDONTWRITEBYTECODE=1: a same-size edit restored within one second leaves a stale .pyc"

key-files:
  created:
    - standin/sympy/__init__.py
    - standin/sympy/_wire.py
    - standin/sympy/_tool.py
    - standin/host_shim/__init__.py
    - standin/tests/__init__.py
    - standin/tests/test_wire.py
    - standin/tests/test_wire_rfc8785.py
    - conformance/python/dev.in
    - conformance/python/dev.lock
  modified:
    - xtask/src/host.rs
    - .gitignore

key-decisions:
  - "The Python mirror refuses integers outside +-(2**53 - 1) (ValueError): serde_json_canonicalizer 0.3.2 formats every i64/u64 through f64, so larger values would be rounded; rfc8785 0.1.4 rejects the same range (IntegerDomainError), so 2**200 is a 'both reject' case"
  - "The dev venv pins the exact CPython patch (3.13.16, uv-managed, --managed-python) rather than '3.13', so a system or older managed Python never stands in"
  - "Lock generation and sync use --no-build: only wheels with approved hashes are installed and no package code runs at resolve or install time"
  - "Package legitimacy for the dev oracles: approved by operator, 2026-10-12, for exactly the five-package set (requirement+hash lines sha256 79a648b06d9274e8c09a3fd1694d607091ec592b8d5a2aa14baf3450512ce5c4); the approval was relayed to this executor by the execute-phase orchestrator"

patterns-established:
  - "Three-way codec differential: corpus bytes (accepted by Rust) == Python mirror == independent oracle, plus seeded random values"
  - "Tool-call tests record the transport's request and response bytes, so envelope encoding and response re-encoding are checked byte for byte"

requirements-completed: [DET-01, GUEST-02]

coverage:
  - id: D1
    description: "The Python JCS mirror encodes every corpus request to exactly its line bytes and agrees byte for byte with rfc8785 (corpus, strings, UTF-16 key order, containers, 2000 seeded random values); both reject large ints and lone surrogates"
    requirement: DET-01
    verification:
      - kind: unit
        ref: "standin/tests/test_wire.py#test_dumps_roundtrip_corpus"
        status: pass
      - kind: unit
        ref: "standin/tests/test_wire_rfc8785.py#test_rfc8785_corpus, test_rfc8785_strings, test_rfc8785_containers, test_rfc8785_large_ints_both_reject, test_rfc8785_surrogates_both_reject, test_rfc8785_random_values"
        status: pass
    human_judgment: false
  - id: D2
    description: "The mirror's canonicalize and identity equal the Rust R2 worker's result and identity for every exact corpus request; fuel_exhausted where the step budget is below the reachable node count (steps \"1\" included)"
    requirement: DET-01
    verification:
      - kind: integration
        ref: "standin/tests/test_wire.py#test_worker_differential (18 lines through host_shim to target/release/ksci-worker)"
        status: pass
      - kind: unit
        ref: "standin/tests/test_wire.py#test_canonicalize_schema_example"
        status: pass
    human_judgment: false
  - id: D3
    description: "The stand-in calls tools only through sympy._tool.call; under CPython host_shim frames requests to the worker, counts calls, refuses without a worker, and importing sympy makes no call"
    requirement: GUEST-02
    verification:
      - kind: integration
        ref: "standin/tests/test_wire.py#test_worker_differential, test_worker_error_tag, test_no_transport, test_no_call_on_import"
        status: pass
    human_judgment: false
  - id: D4
    description: "Hash-locked dev oracles and `cargo xtask tools venv` (CPython 3.13.16, --require-hashes --no-build, refuses a .venv on another interpreter)"
    verification:
      - kind: other
        ref: "cargo xtask tools venv (twice, idempotent); wrong-interpreter .venv (3.13.2) refused"
        status: pass
    human_judgment: false

duration: 27min
completed: 2026-10-12
status: complete
---

# Phase 1 Plan 08: Python wire layer and three-way JCS differential Summary

**Python RFC 8785 mirror and canonicaliser in `standin/sympy/_wire.py`, a single `_tool.call` entry point, and a CPython-only `host_shim` that frames calls to `ksci-worker`. The Rust worker, the Python mirror and rfc8785 0.1.4 now agree byte for byte across the request corpus. The dev oracles are hash-locked and installed by `cargo xtask tools venv` on CPython 3.13.16.**

## Performance

- **Duration:** about 27 min of active work, not counting the wait at the Task 2 checkpoint
- **Started:** 2026-10-11T21:43:44Z
- **Completed:** 2026-10-11T22:12Z (2026-10-12 local)
- **Tasks:** 3 of 3
- **Files modified:** 11

## Accomplishments

- `sympy._wire.dumps` implements RFC 8785 over the wire's value domain:
  - object keys are sorted by UTF-16 code units;
  - strings use the ES6 escape set;
  - floats, lone surrogates (in values and in keys) and integers outside ±(2**53−1) are rejected;
  - strings are never Unicode-normalised.
- `_wire.canonicalize` mirrors `ksci_wire::Table::canonicalize`: an iterative post-order walk that removes duplicates by their JCS bytes, drops unreachable nodes and emits the root last.
- `sympy._tool.call(op, args, steps)` builds the canonical request with the mirror and maps tags 1 to 3 to `RuntimeError("klein tool error <kind>: <message>")`.
- `host_shim` lives outside the sympy package and runs only under CPython:
  - it accepts only an absolute path to an existing worker file;
  - it starts the worker with `Popen([path])` and no shell, lazily, as one long-lived child;
  - it uses the u32 big-endian frames and checks the 64 MiB cap;
  - it provides `calls`, `reset()`, and a default transport that refuses every call with "set KSCI_WORKER".
- Differential over all 18 corpus lines:
  - the request bytes `call` builds equal the corpus line;
  - re-encoding the worker's response with the mirror gives the exact bytes the worker sent;
  - every exact result equals the mirror's canonical table, and its identity equals sha256 of that table's JCS;
  - the expected status comes from an independent count of reachable nodes.
- Three-way check against rfc8785 covers the corpus, the plan's string set, UTF-16 key order, containers, the ±(2**53−1) boundary, "both reject" cases, and 2000 seeded random values.
- The full standin suite under `.venv` passes 14 tests: 8 in `test_wire.py` and 6 in `test_wire_rfc8785.py`.

## Task Commits

1. **Task 1, tracer: `_tool.call` through the mirror and `host_shim` to the R2 worker.** `149dab1` (feat), then `b1a4139` (fix: surrogates in keys are now rejected before the UTF-16 sort).
   - Tracer gate: end-of-phase mode with an automated-only check, so the check was re-run end to end. It passed, and work continued.
2. **Task 2, package-legitimacy checkpoint (blocking-human).**
   - The operator approved it on 2026-10-12 for exactly the resolved set.
   - The lock was regenerated in place and its requirement and hash lines hash to `79a648b0…ce5c4`, the same digest as before. It was produced identically by the host uv 0.12.15 and by the pinned uv 0.13.0.
   - `0e43b07` (chore).
3. **Task 3, hash-locked dev environment and the rfc8785 differential (TDD).** `70752b2` (test, RED shown first), then `0e70800` (feat: `tools venv`).

**Plan metadata:** recorded in the docs commit that adds this SUMMARY.

## Approved lock

The lock is `conformance/python/dev.lock`. It was generated with `uv pip compile conformance/python/dev.in --generate-hashes --python-version 3.13 --no-build`. Approval: by the operator, 2026-10-12.

| Package | Version | sha256 hashes | Via |
|---|---|---|---|
| pydantic-monty | 1.1.0 | 2 | dev.in |
| pydantic-monty-client | 1.1.0 | 97 | pydantic-monty |
| pydantic-monty-runtime | 1.1.0 | 13 | pydantic-monty |
| rfc8785 | 0.1.4 | 2 | dev.in |
| typing-extensions | 4.16.0 | 2 | pydantic-monty-client |

- There are 116 hashes in total.
- PyPI `monty`, an unrelated project, is absent.
- The SHA-256 of the requirement and hash lines is `79a648b06d9274e8c09a3fd1694d607091ec592b8d5a2aa14baf3450512ce5c4`.

**CPython of the dev venv:** 3.13.16, uv-managed (`cpython-3.13.16-macos-aarch64-none`). Later plans run `cargo xtask tools venv` first, because worktrees do not share `.venv`.

## TDD Gate Compliance

- **RED (`70752b2`):**
  - The mirror predates these tests, so RED was shown against a mirror with one deliberate defect: key sort in code point order.
  - Command: `NO_COLOR=1 PYTHONDONTWRITEBYTECODE=1 .venv/bin/python -m unittest discover -s standin -t standin -p test_wire_rfc8785.py -k test_rfc8785_strings`.
  - It exited 1. The failure was on the three-way equality assertion: the mirror wrote U+FFFF before U+1D465, and rfc8785 wrote U+1D465 first.
  - `gsd-tools check tdd-red-evidence` returned `RED_EVIDENCE_OK` (target_test_failed).
  - Semantic check: the target test ran and failed on the planned UTF-16 key-order assertion. There was no load, fixture or unrelated failure.
- **GREEN (`0e70800`):** the real mirror plus `tools venv` give 14 of 14 tests passing under `.venv`.
- **REFACTOR:** none needed.

## Witnesses (rule 5: each test seen RED)

I ran each check against deliberately broken copies of the code, using `PYTHONDONTWRITEBYTECODE=1` and restoring each file with `git checkout`.

**test_wire.py.** All 11 broken versions made a test fail:
- key sort in code point order;
- no duplicate removal in canonicalize;
- steps sent as a JSON number;
- a tool call at import time;
- floats accepted;
- the call counter not incremented;
- uppercase hex escapes;
- surrogates accepted in values;
- surrogates in keys not checked;
- the integer range not checked;
- children walked in reverse.

**test_wire_rfc8785.py.** 6 of 8 broken versions made a test fail:
- key order;
- uppercase hex;
- the integer range;
- escaping `/`;
- escaping DEL;
- escaping all non-ASCII.

The two that survived are equivalent mutants for these tests:
- **Removing the mirror's explicit surrogate checks.** The final UTF-8 or UTF-16 encode still raises a ValueError subclass, so the "both reject" tests still see a ValueError. `test_wire.py` asserts the mirror's own error message, and that test catches both removals.

**`tools venv`:** a `.venv` on CPython 3.13.2 was refused with the instruction to remove it. A second run on the correct venv is idempotent and reports "Checked 5 packages".

## Files Created/Modified

- `standin/sympy/__init__.py`: marks the package as the stand-in; plan 01-12 generates the name table into it.
- `standin/sympy/_wire.py`: the JCS mirror (`dumps`) and `canonicalize`.
- `standin/sympy/_tool.py`: `call` and `_transport`.
- `standin/host_shim/__init__.py`: the CPython-only worker transport (`install`, `reset`, `calls`).
- `standin/tests/__init__.py`, `standin/tests/test_wire.py`: corpus round trip, worker differential, error and transport tests.
- `standin/tests/test_wire_rfc8785.py`: the three-way rfc8785 differential.
- `conformance/python/dev.in`, `conformance/python/dev.lock`: the approved, hash-locked dev oracles.
- `xtask/src/host.rs`: adds `ToolsCmd::Venv`, `VENV_PYTHON` and `DEV_LOCK`.
- `.gitignore`: adds `__pycache__/`.

## Decisions Made

See key-decisions in the frontmatter. The main one is the integer domain. The mirror and rfc8785 both accept exactly ±(2**53−1) and reject anything beyond it, including 2**200. The Rust codec would round such values, and the wire never carries them, because every numeral is a string.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] The integer domain is limited to ±(2**53−1)**
- **Found during:** Task 1
- **Issue:** The plan says "ints in decimal". But `serde_json_canonicalizer` 0.3.2 sends i64 and u64 through f64 (`write_i64`/`write_u64` call `write_f64`), so for a larger integer the Rust and Python bytes would disagree.
- **Fix:** `dumps` raises ValueError beyond ±(2**53−1). The Task 3 case "2**200 encodes identically" became "both reject", because rfc8785 0.1.4 raises `IntegerDomainError` (a ValueError subclass) for the same range. The coordinator directed this outcome.
- **Files modified:** standin/sympy/_wire.py, standin/tests/test_wire.py, standin/tests/test_wire_rfc8785.py
- **Commits:** 149dab1, 70752b2

**2. [Rule 1 - Bug] Surrogates in keys produced a codec error instead of the mirror's own error**
- **Found during:** Task 1, while checking the tests against broken code
- **Issue:** Keys were encoded to UTF-16 for sorting before any surrogate check.
- **Fix:** `_check_scalars(key)` now runs before the sort. The test asserts the mirror's message, and the U+ notation in that message is uppercase.
- **Files modified:** standin/sympy/_wire.py, standin/tests/test_wire.py
- **Commit:** b1a4139

**3. [Rule 3 - Blocking] `__pycache__/` added to .gitignore**
- **Found during:** Task 1. Bytecode would otherwise have been left as untracked files.
- **Commit:** 149dab1. Main has since merged the same line through plans 01-06 and 01-07; the orchestrator resolves that at merge time.

**4. [Rule 2 - Missing critical] `--no-build` on compile and sync, and an exact CPython patch**
- **Found during:** Tasks 2 and 3
- **Issue:** An sdist build during resolution or install would run package code before or outside the legitimacy check. A `3.13` request alone would pick up whatever 3.13 is already on the machine (here 3.13.2).
- **Fix:**
  - `--no-build` on both compile and sync;
  - `--managed-python --python 3.13.16`;
  - the venv's version is verified after creation.
- **Commit:** 0e70800

**5. Extra tests beyond the plan's minimum**
- `test_wire.py`: schema identity example, unsupported-op error tag, no-transport refusal, strings and key order (8 tests in all).
- `test_wire_rfc8785.py`: surrogates rejected by both encoders, and the seeded random values (6 tests in all).
- Both files exceed the plan's minimum counts (at least 4 and at least 7).

---

**Total deviations:** 4 auto-fixed (2 Rule 1, 1 Rule 2, 1 Rule 3), plus extra tests.
**Impact on plan:** each fix keeps the Rust and Python codecs in byte agreement or tightens the supply-chain gate. Nothing was added outside the plan's scope.

## Issues Encountered

- **Stale bytecode.** A broken edit that kept the file the same size, restored within the same second, left a stale `.pyc`, and the uppercase-hex break first looked as if no test caught it. All later runs use `PYTHONDONTWRITEBYTECODE=1`.
- **rust-analyzer unavailable.** It reported "No LSP server available" this session. The every-site check for the new `ToolsCmd` variant used the fallbacks instead: an ast-grep scan, which found one match site in `xtask/src/host.rs::run` and a use in `main.rs`, plus clippy (`-D warnings`) and 60 xtask unit tests.
- **Commit-ledger file.** The sandbox refused writes under `.git/worktrees/`, so the plan base (`798d3e1`) is recorded only in this SUMMARY's frontmatter.

## User Setup Required

None. `cargo xtask tools venv` creates the dev environment. It needs the pinned uv (`cargo xtask tools install`) and network access the first time.

## Next Phase Readiness

- **Plan 01-12:** can generate the stand-in name table into `standin/sympy/__init__.py` and add the Monty-subset lint over `standin/sympy`. The `host_shim` stays outside its scope.
- **Plans 01-12 and 01-17:** reuse the approved dev lock through `cargo xtask tools venv`, which includes pydantic-monty for the Monty census.
- **Proxy status:** the TIR numbers through `host_shim` stay labelled as proxy until klein-rs D4 lands.

---
*Phase: 01-contracts-oracles-and-risk-probes*
*Completed: 2026-10-12*

## Self-Check: PASSED
