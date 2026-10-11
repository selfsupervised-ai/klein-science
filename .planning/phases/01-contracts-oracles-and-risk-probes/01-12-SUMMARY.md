---
phase: 01-contracts-oracles-and-risk-probes
plan: 12
subsystem: standin
tags: [sympy-standin, monty, lint, ast, import-surface, notice, bsd-3, python]

requires:
  - phase: 01-05
    provides: "SymPy 1.14.0 introspection table, blob 011f497f...1da9 (921 runtime __all__ names, 76 abc Symbols), and the pinned sympy oracle image"
  - phase: 01-08
    provides: "standin/sympy/_tool.py, _wire.py, host_shim, and the hash-locked dev venv with pydantic-monty 1.1.0"
provides:
  - "conformance/oracles/sympy/gen_standin.py: deterministic generator with a --check mode"
  - "standin/sympy/__init__.py: SymPy 1.14.0's runtime __all__ (921 names) as plain unavailable-name functions, plus __version__"
  - "standin/sympy/abc.py: the 76 sympy.abc Symbol names, generated statically"
  - "standin/lint: Monty-subset AST lint, rules R1..R9, `python -m lint <paths>`"
  - "NOTICE: the Project: sympy block (SymPy 1.14.0 LICENSE verbatim)"
affects: [01-13 stand-in M0 baseline, 01-17 census, phase 02 expression objects, D4 Monty request]

actuals:
  tokens: 48700
  tasks: 2
  commits: 4
plan_head_before: 229fc4c86235b54a2c41b5e004faa51bdf6383c5
plan_head_after: 9c40905260fa172274ccbc339fa040acace9026f

tech-stack:
  added: []
  patterns:
    - "Generated guest code: names validated as plain identifiers before interpolation; --check regenerates in memory"
    - "Monty as a parse oracle: feed_run the snippet; MontySyntaxError or NotImplementedError from 'the monty syntax parser' is a parse-time rejection (nothing runs)"

key-files:
  created:
    - conformance/oracles/sympy/gen_standin.py
    - standin/sympy/abc.py
    - standin/lint/__init__.py
    - standin/lint/__main__.py
    - standin/tests/test_imports.py
    - standin/tests/test_lint.py
  modified:
    - standin/sympy/__init__.py
    - NOTICE

key-decisions:
  - "The stand-in binds all 921 runtime __all__ names (902 literal + 19 subpackages), not the plan's 902, as 01-05 measured and required"
  - "__version__ (the one dunder in __all__) is bound to \"1.14.0\", SymPy's value, instead of a function"
  - "R9 rejects any reference to eval/exec/compile/__import__/open/__builtins__, not only a call; R2 rejects every decorator. Both are stricter than Monty, which accepts module-level decorators and runs eval"
  - "The SymPy licence is read from licenses/LICENSE in the dist metadata (PEP 639 layout); read_text(\"LICENSE\") returns None in 1.14.0"

patterns-established:
  - "Stand-in module bodies hold only a docstring, plain defs and constant assignments, so importing makes no tool call (asserted on the AST)"
  - "Every lint rule has a RED fixture; rules that mirror Monty parser limits are cross-checked against pydantic-monty"

requirements-completed: [GUEST-01, GUEST-02, LIC-02]

coverage:
  - id: D1
    description: "The five import forms work with no tool call, and missing names raise CPython's ImportError/AttributeError text for 'sympy'"
    requirement: GUEST-01
    verification:
      - kind: unit
        ref: "standin/tests/test_imports.py (13 tests)"
        status: pass
    human_judgment: false
  - id: D2
    description: "The stand-in name table equals SymPy 1.14.0's runtime __all__ (921 names, in order) and the 76 sympy.abc names, regenerable byte-for-byte"
    requirement: GUEST-01
    verification:
      - kind: unit
        ref: "standin/tests/test_imports.py#test_all_is_sympy_runtime_all_in_order"
        status: pass
      - kind: other
        ref: "python3 conformance/oracles/sympy/gen_standin.py --names target/oracle/names.json --check"
        status: pass
    human_judgment: false
  - id: D3
    description: "Monty-subset lint R1..R9, seen RED, agreeing with pydantic-monty 1.1.0's parser"
    requirement: GUEST-02
    verification:
      - kind: unit
        ref: "standin/tests/test_lint.py (16 tests, including test_monty_agrees)"
        status: pass
      - kind: other
        ref: "env PYTHONPATH=standin .venv/bin/python -m lint standin/sympy"
        status: pass
    human_judgment: false
  - id: D4
    description: "Project: sympy NOTICE block with SymPy 1.14.0's LICENSE verbatim; port headers accepted"
    requirement: LIC-02
    verification:
      - kind: other
        ref: "cargo xtask check provenance"
        status: pass
    human_judgment: false

duration: 8min
completed: 2026-10-11
status: complete
---

# Phase 1 Plan 12: Stand-in import surface and Monty-subset lint Summary

**The stand-in `sympy` now binds SymPy 1.14.0's 921 runtime `__all__` names and 76 `sympy.abc` names. Both files are generated from the oracle's introspection table as plain functions. All five import forms make no tool call, and a missing name raises CPython's exact ImportError text. An AST lint (R1 to R9) holds the stand-in to the Monty subset and agrees with pydantic-monty 1.1.0's parser. NOTICE carries SymPy's licence verbatim.**

## Performance

- **Duration:** about 8 min
- **Started:** 2026-10-11T22:16:51Z
- **Completed:** 2026-10-11T22:25Z
- **Tasks:** 2 (a tracer, then a TDD task)
- **Files:** 8 (6 created, 2 modified)

## Accomplishments

- **Generator.** `gen_standin.py --names <names.json> [--check]` uses only the standard library and runs on the host.
  - It refuses a table whose `sympy_version` is not 1.14.0.
  - Every name must be a plain ASCII identifier that is not a keyword or a dunder before it is written into source. `__version__` is the single allowed exception.
  - Output is deterministic: SymPy's order, LF line endings, and the input SHA-256 on line 2.
  - Measured on blob `011f497f…1da9`: `__init__.py` is 138,399 bytes (below the 256 KiB cap) and `abc.py` is 10,479 bytes.
- **Import surface (GUEST-01).**
  - These forms work with zero tool calls: `import sympy`, `import sympy as sp`, `from sympy import symbols`, `from sympy import *` and `from sympy.abc import x`. Zero calls means both `host_shim.calls == 0` and a spy on `_tool._transport` that recorded nothing.
  - `from sympy import *` into an empty namespace binds exactly the 921 `__all__` names.
  - `from sympy import binom` gives `cannot import name 'binom' from 'sympy'` once only the trailing ` (<path>)` is removed.
  - `sympy.not_a_name` raises `module 'sympy' has no attribute 'not_a_name'`.
- **Lint (GUEST-02).** `standin/lint` uses only the standard library `ast`. `python -m lint` prints `path:line:col: Rn message` sorted by path, line and column. It exits 1 on any violation and 2 on a usage error.
- **LIC-02.** NOTICE has a `Project: sympy` block. `cargo xtask check provenance` passes: 26 Rust sources, 12 standin files, 1 Project block.

## Task Commits

1. **Task 1 (tracer): introspection → name table → five import forms.** `bd86226` (feat)
   - Tracer gate: interactive run, `end-of-phase`, automated-only verify. I re-ran the verify after the commit: 13/13 tests pass and `--check` exits 0. Then I went on to the expansion task.
2. **Task 2 (TDD): Monty-subset lint, Monty cross-check, NOTICE.**
   - RED: `c0f8af5` (test). Failing tests plus a lint stub with no rules.
   - GREEN: `dd841d8` (feat). Rules R1 to R9.
   - NOTICE block: `9c40905` (chore).

**Plan metadata:** the docs commit below, with this SUMMARY and deferred-items.md.

## RED witnesses

- **R1, unit (the target).**
  - I ran `NO_COLOR=1 PYTHONDONTWRITEBYTECODE=1 .venv/bin/python -m unittest discover -s standin -t standin -p 'test_lint.py' -v` against the stub, whose `check` returns `[]`.
  - Result: exit 1, 16 tests, 32 failures. `tests.test_lint.Rules.test_lint_R1_inheritance` failed with `AssertionError: Lists differ: [] != ['R1']`.
  - The classifier (`gsd-tools check tdd-red-evidence`) returned `RED_EVIDENCE_OK`, reason `target_test_failed`.
  - Semantic check: the target test ran and failed on the planned assertion: no R1 was reported for `class A(B): pass`. There was no import, collection or fixture fault.
- **R1, CLI.**
  - The file holds `class A(B): pass`. The command is `PYTHONPATH=standin .venv/bin/python -m lint target/tdd/witness/witness.py`.
  - Against the stub: `lint: 1 files, 0 violations`, exit 0. That is the wrongly clean result.
  - After GREEN: `target/tdd/witness/witness.py:1:1: R1 class A has bases or keywords: Monty has no inheritance or metaclasses`, exit 1.
- **Import tests.** Against the pre-plan `standin/sympy/__init__.py`, with no `abc.py`, 9 of the 13 tests fail (8 errors, 1 failure). The 4 that pass are true of any bare package: `import sympy`, `import sympy as sp`, the `binom` ImportError and the AttributeError.
- **Generator `--check`.** With one byte appended to `abc.py`, it prints `standin/sympy/abc.py differs from the generator output` and exits 1. After regeneration it exits 0.
- **Provenance.** Without the NOTICE block it reports 2 violations, `port sympy has no "Project: sympy" line in NOTICE` for `__init__.py` and for `abc.py`. With the block it passes.

## Monty cross-check (pydantic-monty 1.1.0, D4 evidence)

Monty has no parse-only API. `feed_run` parses the whole snippet before it runs any of it. I checked this with `print("ran")` followed by `class A(B): pass`: no output appeared. A `yield` inside a function that is never defined (`if False:`) is rejected the same way. A parse-time rejection arrives as `MontySyntaxError`, or as a `MontyRuntimeError` that wraps `NotImplementedError: The monty syntax parser does not yet support …`.

| Input | Monty 1.1.0 outcome | Lint |
|---|---|---|
| `standin/sympy/__init__.py` | runs, ok | clean |
| `standin/sympy/abc.py` | runs, ok | clean |
| `standin/sympy/_wire.py` | runs, ok | clean |
| `standin/sympy/_tool.py` | parses; runtime `ImportError: attempted relative import with no known parent package` | clean |
| `from sympy import _wire` | parses; runtime `ModuleNotFoundError: No module named 'sympy'` | clean |
| `class A(B): pass` | parse-rejected: "class inheritance and metaclasses" | R1 |
| `yield` / `match` / `del` / `async for` / `async with` / `except*` | parse-rejected, each with its own "does not yet support" message | R3 / R4 / R5 / R6 / R6 / R7 |
| `@f` on a module-level `def` | runs, ok | R2 (stricter) |
| `eval("1")` | runs, returns 1 | R9 (stricter) |
| `import subprocess` | parses; runtime `ModuleNotFoundError` | R8 |

**D4 evidence (plugin module import).** Monty 1.1.0 cannot import the stand-in's own submodules. Inside the stand-in, `from . import _wire` and `from sympy import …` both fail at run time, though they parse. Under Monty today the stand-in is one module per feed, until Klein's D4 adds plugin module import.

## SymPy licence text

- **Command:** `docker run --rm --network=none --read-only --entrypoint python sha256:b468cfd6ec79408995fe9b8506e47952296a5fe78f9285d57517dff5b1d7e140 -c 'import importlib.metadata as m, sys; sys.stdout.write(m.distribution("sympy").read_text("licenses/LICENSE"))'`
  - It runs the pinned linux/arm64 image identity from `conformance/oracles/images.json`.
  - It reads distribution metadata only, so no SymPy code runs.
- **Text:** 7,885 bytes, 153 lines, SHA-256 `07a5e9819f727b4986ad2829c7a29a6320d42575f720eb24d71b7fef573a0286`.
  - Contents: the SymPy BSD-3 notice, then the Diofant, multipledispatch and PyDy BSD notices, then the latex2sympy MIT notice.
  - It is appended to NOTICE byte for byte; `NOTICE.endswith(text)` is True.

## Files Created/Modified

- `conformance/oracles/sympy/gen_standin.py`: the generator and its `--check` mode.
- `standin/sympy/__init__.py`: generated. 920 `def`s plus `__version__ = "1.14.0"`, then `__all__` with 921 names.
- `standin/sympy/abc.py`: generated. 76 `def`s and `__all__`.
- `standin/lint/__init__.py`: `Violation`, `check(source, path)`, and rules R1 to R9 (R0 is a syntax error).
- `standin/lint/__main__.py`: the `python -m lint <paths...>` command.
- `standin/tests/test_imports.py`: 13 GUEST-01 tests.
- `standin/tests/test_lint.py`: 16 GUEST-02 tests, including `test_monty_agrees`.
- `NOTICE`: the `Project: sympy` block.

## Decisions Made

See `key-decisions` in the frontmatter. Two R8 details:
- The allowed set is Monty's module list minus `time`, `random`, `os`, `os.path`, `pathlib`, `posixpath` and `datetime`, plus `sympy`, `sympy.*` and relative imports.
- Dotted names must match exactly, so `import json.decoder` and `from __future__ import …` are rejected.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug in the plan's constant] 921 names, not 902**
- **Found during:** Task 1. Plan 01-05 had already measured this and recorded the correction for this plan.
- **Issue:** the plan says `__all__` has 902 names and asks for `grep -c '^def '` to print 902. SymPy's runtime `__all__`, which `from sympy import *` binds, has 921 names: the 902-name literal plus 19 appended subpackages. `introspect.py` asserts this count.
- **Fix:** the stand-in binds all 921 in SymPy's order. `grep -c '^def ' standin/sympy/__init__.py` prints 920, because the 921st name is `__version__`; see item 2. The tests pin the name list by digest (`fa0874d8…f8f5`), computed from names.json and not from the generated file.
- **Commit:** `bd86226`.

**2. [Rule 1 - Bug] `__version__` is a value, not a function**
- **Found during:** Task 1. The identifier validation refused the dunder.
- **Issue:** `__version__` is in `sympy.__all__` and is the string `"1.14.0"`. A function placeholder would break `sympy.__version__`. Pitfall K allows "module-level values".
- **Fix:** `VALUES = {"__version__": "1.14.0"}` in the generator. Every other dunder is still refused. The AST test allows only `__all__` and `__version__` assignments.
- **Commit:** `bd86226`.

**3. [Rule 2 - Missing validation] names.json is validated before interpolation**
- **Issue:** the generator writes names into Python source, so a malformed table could inject code into guest-side files (T-01-32).
- **Fix:** names must be ASCII identifiers that are not keywords and not dunders (`__version__` excepted). Names must be unique, and the version must be 1.14.0.
- **Commit:** `bd86226`.

**4. [Rule 3 - Blocking] The licence path inside the dist metadata**
- **Found during:** Task 2.
- **Issue:** the plan's `read_text("LICENSE")` returns None for sympy 1.14.0, whose metadata uses the PEP 639 path `sympy-1.14.0.dist-info/licenses/LICENSE`.
- **Fix:** read `licenses/LICENSE`. The command and digest are above.
- **Commit:** `9c40905`.

**5. [Rule 2 - Hardening] R9 covers references; R6 covers async comprehensions; one extra import test**
- R9 matches any `Name` reference to the dynamic names, plus `__builtins__`, so `run = eval` cannot get round it.
- R6 also covers `async for` inside a comprehension.
- `test_module_bodies_are_static` proves "import makes no tool call" from the AST of both generated files.
- **Commits:** `c0f8af5`, `dd841d8`.

---

**Total deviations:** 5 auto-fixed: 2 bugs (one of them the plan's constant), 2 missing-critical or hardening, 1 blocking.
**Impact on plan:** the scope did not change, and every acceptance criterion holds, with 921 names as the corrected fact.

## Issues Encountered

- **CI gating (deferred).** `cargo xtask ci` runs no host-side Python suite: its steps are cargo commands only.
  - T-01-32's clause "the lint runs in `cargo xtask ci`" is therefore not yet true. Plan 01-17 also assumes a host-side `unittest discover` in ci, and no phase-01 plan adds one.
  - The fix needs a new `cargo xtask` command in `xtask/`, which is outside this plan's files, and sibling wave-5 worktrees may be editing `ci.rs`.
  - It is recorded in `deferred-items.md` with the exact commands. The lint, R8 and R9, the generated-file checks and the tests all exist; only the CI wiring is missing.
- **Between commits.** `cargo xtask check provenance` fails at `bd86226`, `c0f8af5` and `dd841d8`, because the port headers landed in Task 1 and the NOTICE block in Task 2, as the plan ordered. It passes from `9c40905` on.
- **Code oracles.** No Rust was changed, so the rust-analyzer and tree-sitter rule (CLAUDE.md rule 7) had nothing to check. The provenance check compiled the workspace cleanly.

## Known Stubs

These are intentional, by design of the GUEST-01 skeleton (D-01):
- `standin/sympy/__init__.py` has 920 functions that raise `NotImplementedError("sympy.<name> is not available in the klein-science stand-in")`. They include the 51 non-`__version__` instances such as `pi`, `oo`, `S`, `I` and `Q`, and the 20 module-kind names such as `polys` and `evalf`.
  - Arithmetic on an instance placeholder (`sympy.pi * 2`) raises TypeError, not NotImplementedError.
  - `import sympy.polys` raises ModuleNotFoundError.
  - Phase 2 replaces these with expression objects and Symbols.
- `standin/sympy/abc.py`: 76 such functions. SymPy's `_clash`, `_clash1` and `_clash2` dicts are not reproduced.

These are the plan's goal and do not block it.

## Threat Flags

None. No new network, auth, file-access or trust-boundary surface. The Docker read used `--network=none --read-only` on the pinned image identity.

## TDD Gate Compliance

- RED `c0f8af5`: `test(01-12)`, `RED_EVIDENCE_OK`.
- GREEN `dd841d8`: `feat(01-12)`, 16/16 pass.
- No REFACTOR commit: none was needed.

## Verification (plan level)

- Full standin suite: `NO_COLOR=1 PYTHONDONTWRITEBYTECODE=1 .venv/bin/python -m unittest discover -s standin -t standin -v` ran 43 tests, OK. This includes test_wire against a release `ksci-worker` built with `cargo build --release --locked -p ksci-worker`.
- `env PYTHONPATH=standin .venv/bin/python -m lint standin/sympy`: `lint: 4 files, 0 violations`, exit 0.
- `python3 conformance/oracles/sympy/gen_standin.py --names target/oracle/names.json --check`: exit 0.
- `cargo xtask check provenance`: ok, 1 Project block.
- `cargo xtask check no-corpus`: 144 files, no violations.

## User Setup Required

None.

## Next Phase Readiness

- Plan 01-13 (the M0 stand-in baseline) can run these exact files. Under CPython, use `host_shim`. Under Monty, feed one module at a time; see the D4 evidence above.
- The deferred CI wiring should land before the phase gate, so that the lint and the stand-in tests are CI-enforced.

## Self-Check: PASSED

- All 8 key files exist on disk.
- Commits `bd86226`, `c0f8af5`, `dd841d8` and `9c40905` are on this branch: `git log 229fc4c..HEAD` lists exactly these 4.
- `plan_head_after` is HEAD before the docs commit.

---
*Phase: 01-contracts-oracles-and-risk-probes*
*Completed: 2026-10-11*
