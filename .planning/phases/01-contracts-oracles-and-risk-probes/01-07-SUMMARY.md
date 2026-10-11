---
phase: 01-contracts-oracles-and-risk-probes
plan: 07
subsystem: conformance
tags: [numinamath-tir, parquet, pyarrow, docker, ast, census, golden-slice, sha256, jcs]

requires:
  - phase: 01-05
    provides: "cargo xtask oracle run/build-image (hardened runner, pins, store), pinned base image, introspection table 011f497f…1da9, operator clearance of pyarrow 26.0.0"
  - phase: 01-02
    provides: "cargo xtask store fetch/get/put, ksci.recipe.v1, check no-corpus"
provides:
  - "conformance/corpora/tir.toml: NuminaMath-TIR rev 77a91d7b, parquet sha256 66163f9f…61d8"
  - "ksci-oracle-extract image (base digest + pyarrow 26.0.0), pinned for linux/arm64, tar in the store"
  - "extract.py --format tir: programs.jsonl (72,388 distinct programs) stored by digest c79bb79d…d8fa"
  - "census.scan: parse, sympy_imports, sympy_names: the one AST name scanner (reused by plan 01-17)"
  - "slice.py: stratified, hash-ranked golden slice, no RNG; slice.jsonl stored by digest 791aab3f…b766"
  - "conformance/golden/tir-slice-v1.slice.json: the slice manifest (digests and strata counts, no code)"
affects: [01-10 goldens over the slice, 01-13 stand-in baseline, 01-17 static census]

actuals:
  tokens: 10890
  tasks: 2
  commits: 4
plan_head_before: 798d3e18dbedc94153158048c7946c5916d71a34
plan_head_after: f420f388ca94c6799389fc001195b7e7d083a665

tech-stack:
  added: ["pyarrow 26.0.0 (extraction image only, hash-locked)"]
  patterns:
    - "Corpus parquet is parsed only in the extraction image, by a process that has dropped every capability"
    - "Selection without an RNG: seed from the corpus digest, rank by sha256(seed || case_id) within a stratum"
    - "Manifests in git are the exact JCS bytes xtask stored, written back with cargo xtask store get"

key-files:
  created:
    - conformance/corpora/tir.toml
    - conformance/oracles/extract/Containerfile
    - conformance/oracles/extract/requirements.in
    - conformance/oracles/extract/requirements.lock
    - conformance/oracles/extract/extract.py
    - conformance/census/__init__.py
    - conformance/census/scan.py
    - conformance/census/tests/__init__.py
    - conformance/census/tests/test_scan.py
    - conformance/census/tests/test_slice.py
    - conformance/oracles/sympy/slice.py
    - conformance/golden/tir-slice-v1.slice.json
  modified:
    - conformance/oracles/images.json
    - .gitignore

key-decisions:
  - "pyarrow 26.0.0, not the plan's 25.0.1: the operator's decision recorded in plan 01-05"
  - "A ```python fence is a line whose stripped text is exactly ```python; the block runs to the next line that is exactly ``` (or to the end); blocks are joined with newlines"
  - "Allocation: a stratum whose proportional quota is below min(population, 10) is fixed at that floor and the rest re-apportioned until none is below; then largest remainder (ties: larger population, then name)"
  - "slice.py takes its two inputs by position (programs, then names.json): store inputs are mounted under their digests, so names carry no meaning"
  - "Unparseable programs are counted (syntax_errors=251) and never selected; SymPy-using means census.scan finds a sympy import"

patterns-established:
  - "One AST scanner (census.scan) for every static count of SymPy use"
  - "Corpus-derived data lives in the store; git holds the recipe, the image pin and a code-free manifest"

requirements-completed: [CONF-01, CONF-02]

coverage:
  - id: D1
    description: "TIR pinned by recipe; fetch re-hashes the parquet; extraction in the network-less extraction image is reproducible and stored by digest"
    requirement: CONF-02
    verification:
      - kind: integration
        ref: "cargo xtask store fetch conformance/corpora/tir.toml (upstream, then cache, sha256:66163f9f…61d8)"
        status: pass
      - kind: integration
        ref: "cargo xtask oracle run --image extract --platform linux/arm64 --script conformance/oracles/extract/extract.py --input 66163f9f…61d8 -- --format tir (twice: programs.jsonl sha256=c79bb79d…d8fa, rows=72441)"
        status: pass
      - kind: other
        ref: "cargo xtask check no-corpus (no violations)"
        status: pass
    human_judgment: false
  - id: D2
    description: "census.scan: SymPy imports and names per program; unparseable code returns None"
    requirement: CONF-01
    verification:
      - kind: unit
        ref: "conformance/census/tests/test_scan.py (seen RED against the stub interface)"
        status: pass
    human_judgment: false
  - id: D3
    description: "1,000-program golden slice, stratified by SymPy subpackage, chosen without an RNG; manifest without code"
    requirement: CONF-01
    verification:
      - kind: unit
        ref: "conformance/census/tests/test_slice.py (seen RED against the stub interface)"
        status: pass
      - kind: integration
        ref: "cargo xtask oracle run --image base --platform linux/arm64 --script conformance/oracles/sympy/slice.py ... (three runs: slice.jsonl sha256=791aab3f…b766 each time)"
        status: pass
      - kind: other
        ref: "grep -c '\"code\"' conformance/golden/tir-slice-v1.slice.json prints 0; selected sum 1000"
        status: pass
    human_judgment: false

duration: 9 min
completed: 2026-10-11
status: complete
---

# Phase 1 Plan 07: TIR corpus recipe, extraction and the golden slice Summary

**NuminaMath-TIR is pinned by recipe and its 72,388 distinct programs are extracted inside a network-less pyarrow image, with the same digest on every run. A stdlib AST scanner then classifies them, and 1,000 of the 46,189 SymPy-using programs are chosen across 15 SymPy-subpackage strata. The choice uses a seed derived from the parquet digest and hash ranking, never an RNG. Git holds only the recipe, the image pin and a manifest with no code.**

## Performance

- **Duration:** about 9 min
- **Started:** 2026-10-11T21:42:52Z
- **Completed:** 2026-10-11T21:51:35Z
- **Tasks:** 2 of 2
- **Files:** 12 created, 2 modified (833 insertions)

## Accomplishments

- **TIR recipe** (`conformance/corpora/tir.toml`).
  - It pins revision `77a91d7b7a1a98ac4b1beb7d86c09d156b935dcd`, the file `data/train-00000-of-00001.parquet` and sha256 `66163f9fdf2220fed2116142781fe7019a117191e7f08a3339a70a98401061d8`.
  - The first `store fetch` missed the remote (GHCR denied), downloaded from upstream and re-hashed the file. The second fetch was a cache hit that named the same sha256.
- **Extraction image** `ksci-oracle-extract`.
  - It is built `FROM` the pinned base digest and installs from the hash lock with `--no-deps --require-hashes --only-binary=:all:`.
  - The build fails unless the installed distributions are exactly `pip` and `pyarrow`, so there is no SymPy, mpmath or numpy. It runs as `USER 65534:65534`.
- **`extract.py --format tir`**
  - It drops every capability before it opens the untrusted parquet, then reads the `solution` column.
  - It takes each row's ```python fenced blocks, joins them with newlines, sets `case_id = sha256(program)`, deduplicates keeping the lowest row, and writes `programs.jsonl` sorted by `case_id` plus `extract.json`.
- **`census.scan`** is the one AST scanner, for plan 01-17 as well. It provides:
  - `parse`, which returns None on bad code and silences compiler warnings;
  - `sympy_imports`;
  - `sympy_names`, which covers from-imports, star imports resolved by reference against `__all__`, attributes of sympy-module aliases, and `abc:` names.
- **`slice.py`**
  - seed = sha256(ascii(parquet hex) ‖ `golden-slice-v1`) = `98a3f54041e4e7bee6181c774593cb6322a3847b656ecd893ed199e10297fccb`;
  - the strata are taken from the introspection table, using a fixed priority list;
  - each stratum gets at least min(population, 10), and the rest is apportioned by largest remainder;
  - within a stratum, programs are ranked by sha256(seed ‖ case_id).

## Digests

| Artifact | sha256 | Where |
|---|---|---|
| TIR parquet | `66163f9fdf2220fed2116142781fe7019a117191e7f08a3339a70a98401061d8` | store (upstream HF) |
| `tir.toml` (recipe_sha256) | `28267eb97ae2065dfd6d955f88f2ccfb156a0716af933b52161ac86ed38ef946` | git |
| extract image, linux/arm64 (identity = manifest) | `sha256:a3de5b2ea00bb0d91cb1dbe0b77c6e2cf7125335dbabc2fa23c5a90e9e7bf81a` | images.json |
| extract image tar | `60e46bc302fe011684577776d31905170fbe8346ba147559e6fb1361eefcf6fb` | store |
| programs.jsonl, extraction run 1 | `c79bb79d8c075476d53f88fe88ac76495eff968be383a5b66e75076b6143d8fa` | store |
| programs.jsonl, extraction run 2 | `c79bb79d8c075476d53f88fe88ac76495eff968be383a5b66e75076b6143d8fa` (equal) | store |
| extract.json (both runs) | `f4d5de310ab5abeb959077373138d8616409d714a12fcdaf695d623d18c3c0ec` | store |
| introspection table (input) | `011f497f2f95d8ffdcd59528df1caf992f2cf3a65a2cdfa43c90c7c717a81da9` | store (plan 01-05) |
| slice.jsonl, run 1 (before the warnings fix) | `791aab3fbaf3def0518b416f05f66947f23d8cddc5808dbca66eae38cd48b766` | store |
| slice.jsonl, run 1 (committed code) | `791aab3fbaf3def0518b416f05f66947f23d8cddc5808dbca66eae38cd48b766` (equal) | store |
| slice.jsonl, run 2 (with `--meta slice_sha256`) | `791aab3fbaf3def0518b416f05f66947f23d8cddc5808dbca66eae38cd48b766` (equal) | store |
| slice.json manifest = `conformance/golden/tir-slice-v1.slice.json` | `1f7b1c3f93f96791703c64fddd3a5b16d84aebb61dabe8537524ae485eaac3ef` | git and store |

**extract.json:** `{"distinct_programs":72388,"duplicates":30,"format":"tir","input_sha256":"66163f9f…61d8","pyarrow_version":"26.0.0","rows":72441,"rows_with_code":72418}`.

So rows = 72,441, matching the recipe. 23 rows have no python block, and 30 rows repeat an earlier program.

## Strata (n = 1000; 72,388 programs; sympy_using 46,189; syntax_errors 251)

| Stratum | Population | Selected |
|---|---:|---:|
| solvers | 23,774 | 497 |
| functions | 4,925 | 103 |
| simplify | 4,811 | 101 |
| none (imports sympy, uses no name) | 4,652 | 97 |
| core | 3,914 | 82 |
| ntheory | 1,119 | 23 |
| polys | 834 | 17 |
| integrals | 675 | 14 |
| matrices | 595 | 13 |
| series | 593 | 12 |
| other | 116 | 10 |
| sets | 88 | 10 |
| logic | 77 | 10 |
| calculus | 15 | 10 |
| printing | 1 | 1 |
| **total** | **46,189** | **1,000** |

## Task Commits

1. **Task 1 (tracer): TIR recipe, extraction image and extract.py.** `d8d3721` (feat).
2. **Task 2, RED: failing scanner and slice tests against stub interfaces.** `3de7ae9` (test).
3. **Task 2, GREEN: census.scan and slice.py.** `e479ce1` (feat).
4. **Task 2: the slice manifest.** `f420f38` (feat).

**Tracer gate.** The mode was interactive and `end-of-phase`, with an automated-only verify. On the committed tree, `store fetch` named sha256 `66163f9f…`, and the extraction run printed `programs.jsonl sha256=c79bb79d…`. Then the plan expanded.

## TDD Gate Compliance

- **RED** (`3de7ae9`).
  - Ran `NO_COLOR=1 python3 -m unittest discover -s conformance -t conformance -v`: exit 1, 21 tests, 18 failing.
  - The classifier returned `RED_EVIDENCE_OK` (`target_test_failed`) for two targets:
    - `test_slice_allocation_floor_per_stratum`: expected `{'a': 80, 'b': 10, 'c': 10}`, got `{}`;
    - `test_scan_names_star_import_resolved_by_reference`: expected `{'symbols'}`, got `set()`.
  - **Semantic assessment:** the stub modules exist, so nothing failed to import or collect. Each target ran and failed on its planned assertion. The syntax-error tests raised SyntaxError, which is the planned behaviour violated. The three tests that passed (numpy-only, other-module alias, valid parse) assert absences or a trivial parse that the stub already satisfied.
- **GREEN** (`e479ce1`): 22 tests pass. That includes a test added in this step, `test_scan_parse_ignores_compiler_warnings`, which was seen RED when the warnings filter was removed and GREEN once it was restored.
- **REFACTOR:** none was needed.

## Decisions Made

- **What a fence is.** The opening fence is a line whose stripped text is exactly ```` ```python ````. The closing fence is the next line that is exactly ```` ``` ````; a block left open runs to the end of the solution. A block's lines are joined with `\n`, and a row's blocks are joined with `\n`. This definition fixes `case_id`, so it is written in `extract.py`'s docstring.
- **How the floor works.** A floor fixes a stratum only when its proportional quota is below min(population, 10). The others keep proportional shares, recomputed until none falls below its floor. With {900, 90, 10} and n = 100, this gives {80, 10, 10}, as unit-tested.
- **Manifest fields.** Besides the plan's keys, the manifest records `schema` (`ksci.golden-slice.v1`), `floor`, `stratum_priority`, `programs` and `selected`. The rules that produced the slice are therefore in the manifest itself.
- **Inputs by position.** `slice.py` reads its inputs by position. Store inputs are mounted under their digests, so a file name says nothing. Each input is validated: every case_id must be the sha256 of its code with none repeated, and the table must have `all` and `abc`.

## Deviations from Plan

### Auto-fixed Issues

**1. [Operator decision] pyarrow 26.0.0 instead of 25.0.1**
- **Found during:** Task 1.
- **Issue:** the plan pins `pyarrow==25.0.1`, but plan 01-05's operator checkpoint cleared 26.0.0 and waived the 7-day freshness rule.
- **Fix:** `requirements.in` pins `pyarrow==26.0.0`. The lock is the output of the pinned uv 0.13.0 (`uv pip compile --no-config --index-url https://pypi.org/simple … --generate-hashes --python-version 3.13 --python-platform linux`).
  - It holds the two approved cp313 wheel hashes (`e3b190ba…` aarch64, `240bd18a…` x86_64).
  - It also holds the other 48 files of pyarrow 26.0.0. All 50 hashes were checked against PyPI's JSON API: every one is published, and none is yanked.
  - pip installs only the wheel that matches the platform.
- **Acceptance:** `grep -c 'pyarrow==25.0.1'` prints 0. The equivalent check, `grep -c 'pyarrow==26.0.0'`, prints 1, and the lock has 50 `--hash=sha256:` lines.
- **Commit:** `d8d3721`.

**2. [Rule 2 - Security] The extractor drops every capability before parsing the parquet**
- **Found during:** Task 1 (T-01-19).
- **Issue:** the launcher leaves the harness with CAP_KILL, CAP_SETGID and CAP_SETUID, and extraction needs none of them.
- **Fix:** `extract.py` clears all capability sets with `capset` and checks `/proc/self/status` before it imports pyarrow. If that fails, it exits 4. Both runs passed the check.
- **Commit:** `d8d3721`.

**3. [Rule 3 - Blocking] `__pycache__/` in .gitignore**
- **Issue:** the plan's verify command (`python3 -m unittest discover …` on the host) writes bytecode into the checkout. Those files would be left untracked.
- **Commit:** `3de7ae9`.

**4. [Rule 1 - Bug] `census.scan.parse` silences compiler warnings**
- **Found during:** Task 2, the first slice run.
- **Issue:** the first run printed SyntaxWarnings for invalid escapes in corpus programs. Under warnings-as-errors, a valid program would have been counted as a syntax error.
- **Fix:** `ast.parse` now runs under `warnings.catch_warnings()` with `simplefilter("ignore")`.
- **Witness:** `test_scan_parse_ignores_compiler_warnings` fails without the filter and passes with it. The slice digest is unchanged.
- **Commit:** `e479ce1`.

---

**Total deviations:** 4: 1 operator-decision carry-over, 1 security hardening, 1 blocking, 1 bug. Every other acceptance criterion holds as written.
**Impact on plan:** no scope change.

## Issues Encountered

- **sympy_using differs from FEATURES.md.** This plan counts 46,189 SymPy-using programs; FEATURES.md reports 46,380 (64.0%). The methods differ:
  - this count is over distinct programs (72,388), not rows (72,441);
  - it uses this plan's fence definition;
  - the 251 unparseable programs are excluded.

  The proportion, 46,189 / 72,388 = 63.8%, agrees within method noise.
- **The printing stratum has a population of 1.** Most programs that use printing names also use a higher-priority subpackage. This one program is selected, because the floor is min(population, 10).
- **No Rust changed, so rust-analyzer and tree-sitter were not needed.** The only Rust tooling used was the xtask CLI.
- **GHCR remote miss on fetch** ("denied"). This is expected until the operator approves pushes (D-05). The fetch fell back to upstream and re-hashed the file.

## User Setup Required

None. No external service configuration is required.

## Next Phase Readiness

- Plan 01-10 can produce goldens over the slice: `cargo xtask store get 791aab3f…b766` gives `slice.jsonl`, in the `{case_id, code, stratum}` form that `run_programs.py` accepts.
- Plan 01-17 can reuse `census.scan` directly.
- The blobs live only in the local store until a push is approved (D-05): the parquet, `programs.jsonl`, `slice.jsonl` and the extract image tar.

---
*Phase: 01-contracts-oracles-and-risk-probes*
*Completed: 2026-10-11*
