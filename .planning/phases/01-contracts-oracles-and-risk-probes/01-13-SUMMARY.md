---
phase: 01-contracts-oracles-and-risk-probes
plan: 13
subsystem: conformance
tags: [scorer, three-level, standin, host_shim, proxy, baseline, importerror, jcs, tir]

requires:
  - phase: 01-10
    provides: "oracle golden and its manifests: tir-slice-v1 (996 golden, 4 seed-adjudicated, golden_sha256 de46d3b2…ddc1); the one runner run_programs.py"
  - phase: 01-12
    provides: "the stand-in import surface (921 names, static sympy.abc) and the Monty-subset lint"
  - phase: 01-08
    provides: "host_shim, the Python JCS mirror standin/sympy/_wire.py, the hash-locked dev venv"
provides:
  - "conformance/runner/score.py: three-level scorer (CONF-03), ImportError path normalisation, aggregate via sympy._wire.dumps"
  - "cargo xtask oracle standin --golden-manifest <m> --name <n> [--platform] [--jobs] [--cpu-seconds]"
  - "testdata/programs/imports.jsonl and conformance/golden/imports-v1.golden.json (16 runs, 9 golden, 0 adjudicated)"
  - "conformance/golden/scores/imports-v1.m0-standin.json: 9/9 on all three levels, error text 3/3"
  - "conformance/golden/scores/tir-slice-v1.m0-standin.json: the M0 baseline, 13.5 / 13.5 / 13.6 %, proxy"
  - "score file format ksci.standin-score.v1 (scorer aggregate + digests + label)"
affects: [phase 02 TIR gates (60/90/97%), 01-17 census, D4 Monty request, verify-work for phase 01]

actuals:
  tokens: 11364
  tasks: 2
  commits: 2
plan_head_before: 5f97524d9e83258c236181a3775069cba1fbf5d3
plan_head_after: 3405ec13bbfa89de3b1320a0ee62f476aed62ed1

tech-stack:
  added: []
  patterns:
    - "Nested levels: level 1 and level 2 each require level 3, so an output match never counts when the outcome differs"
    - "The scorer owns the aggregate's keys; xtask adds digests and the label beside them and refuses a collision (merge_score)"
    - "A score names the stand-in by `git rev-parse HEAD:standin`, and xtask refuses to run (or to write) when standin/ differs from HEAD"

key-files:
  created:
    - conformance/runner/__init__.py
    - conformance/runner/score.py
    - conformance/runner/tests/__init__.py
    - conformance/runner/tests/test_score.py
    - testdata/programs/imports.jsonl
    - conformance/golden/imports-v1.golden.json
    - conformance/golden/scores/imports-v1.m0-standin.json
    - conformance/golden/scores/tir-slice-v1.m0-standin.json
  modified:
    - xtask/src/oracle.rs

key-decisions:
  - "Levels are nested: level 3 = same status and (for errors) same exception type; level 2 = level 3 and same final answer; level 1 = level 3 and same stdout and stdout_truncated. An ok-vs-error pair with equal stdout scores nothing"
  - "ImportError path normalisation applies only to ImportError messages; every other type is compared verbatim, so 'bad (1)' and 'bad (2)' stay different"
  - "excluded counts the manifest's adjudication list; a candidate for a case that is neither golden nor excluded counts in `extra` (0 in both runs)"
  - "oracle standin first runs `oracle golden --verify` on the manifest, defaults --jobs and --cpu-seconds to the manifest's params, uses its mem_bytes, and checks the stand-in run.json (seed 0, machine, limits, prelude host_shim, jobs, sole input, sympy null)"
  - "Percentages are rounded half up with integer arithmetic and written as strings, because the JCS mirror refuses floats"

patterns-established:
  - "Every stand-in score file carries label 'proxy: CPython host_shim until klein-rs D4' plus golden_manifest_sha256, golden_sha256, candidate_sha256, candidate_run_sha256, percase_sha256, standin_tree, run_programs_blob, scorer_blob, image and params"

requirements-completed: [CONF-03, GUEST-01, GUEST-02]

coverage:
  - id: D1
    description: "Three-level scorer: levels, missing, excluded, order independence, empty and adjacency edges, ImportError normalisation, JCS aggregate"
    requirement: CONF-03
    verification:
      - kind: unit
        ref: "conformance/runner/tests/test_score.py (12 tests; seen RED, 12 errors against a stub)"
        status: pass
      - kind: integration
        ref: "env PYTHONPATH=standin .venv/bin/python -m unittest discover -s conformance -t conformance (34 tests)"
        status: pass
    human_judgment: false
  - id: D2
    description: "oracle standin: verified manifest, hardened run with standin/ read-only and host_shim, scoring, lint, proxy-labelled score file"
    requirement: GUEST-02
    verification:
      - kind: unit
        ref: "xtask/src/oracle.rs#standin_score_merge_never_overwrites"
        status: pass
      - kind: unit
        ref: "xtask/src/oracle.rs#standin_run_report_must_match_the_run"
        status: pass
      - kind: integration
        ref: "cargo xtask ci (13 steps, all passed, on the committed tree 3405ec1)"
        status: pass
    human_judgment: false
  - id: D3
    description: "GUEST-01 differential: the imports mini-suite scores 9/9 on all three levels against real SymPy 1.14.0, with equal ImportError/AttributeError text"
    requirement: GUEST-01
    verification:
      - kind: integration
        ref: "python3 -c assert level1==level2==level3==scored==9, error_message_equal==3, missing==0 on conformance/golden/scores/imports-v1.m0-standin.json"
        status: pass
    human_judgment: false
  - id: D4
    description: "M0 baseline on the TIR slice, labelled proxy, lint pass, no stdout or code in git"
    requirement: GUEST-02
    verification:
      - kind: integration
        ref: "python3 -c assert missing==0, scored+excluded==1000, lint=='pass', 'proxy' in label on tir-slice-v1.m0-standin.json"
        status: pass
      - kind: other
        ref: "cargo xtask check no-corpus; grep -c '\"stdout\"\\|\"code\"' prints 0"
        status: pass
    human_judgment: false

duration: 11min
completed: 2026-10-12
status: complete
---

# Phase 1 Plan 13: Three-level runner and the M0 stand-in baseline Summary

**`conformance/runner/score.py` scores run_programs.py records on three nested levels (error class, final answer, stdout bytes), never drops a case, and normalises only the path CPython appends to ImportError text. `cargo xtask oracle standin` runs the stand-in through the one hardened `run_script` in the base image with `standin/` read-only and the `host_shim` prelude, then scores it against a verified golden manifest. On the own-authored imports mini-suite the stand-in matches real SymPy 1.14.0 on 9 of 9 cases at every level, with equal error text on all 3 error cases. The M0 baseline on the TIR slice, labelled proxy, is 13.5 % / 13.5 % / 13.6 %.**

## Performance

- **Duration:** about 11 min (most of it container time: the 16-run imports matrix and the 1,000-program stand-in run)
- **Started:** 2026-10-11T23:57:27Z
- **Completed:** 2026-10-12T00:08:32Z
- **Tasks:** 2 of 2
- **Files:** 8 created, 1 modified (857 insertions, 25 deletions)

## M0 baseline (proxy: CPython host_shim until klein-rs D4)

`conformance/golden/scores/tir-slice-v1.m0-standin.json`, from `cargo xtask oracle standin --golden-manifest conformance/golden/tir-slice-v1.golden.json --name tir-slice-v1 --jobs 8`. These are proxy numbers: the stand-in ran under CPython with `host_shim`, not inside Klein under Monty.

| Field | Value |
|---|---|
| scored / excluded / missing / extra | 996 / 4 (seed-adjudicated) / 0 / 0 |
| **level 1 (stdout byte-equal)** | **134, 13.5 %** |
| **level 2 (final answer equal)** | **134, 13.5 %** |
| **level 3 (error class equal)** | **135, 13.6 %** |
| error_message_equal | 14 of 128 golden error cases |
| lint | pass (`lint: 4 files, 0 violations`) |
| golden_manifest_sha256 | `d304aeec…fe858` |
| golden_sha256 | `de46d3b2…ddc1` |
| candidate_sha256 (records) | `880e0b0a8bf3d35e7d59ed9d2d76940771631403e32c9ed67efd3a79dc3e9eae` |
| candidate_run_sha256 | `eebc5543…4c28` |
| percase_sha256 | `39e7de94423da6da782ed282f39d8f6c7366ee23361e82673838e0cc522cede6` |
| standin_tree | `4a50ee1b4d0a507f2006fd969ba5b6e49f4764cb` |
| params | seed 0, cpu_seconds 60, mem_bytes 2147483648, jobs 8, prelude host_shim, linux/arm64, base image `sha256:70729b46…c2f` |

The stand-in run reported ok 94, error 904, timeout 2. The golden reported ok 857, error 128, timeout 11.

**Why cases fail.** I classified the 861 level-3 failures by the stand-in's outcome. The per-case blob and the candidate records were joined in a scratch script; nothing from it is committed.

| Stand-in outcome | Cases |
|---|---|
| `NotImplementedError: sympy.<name> is not available in the klein-science stand-in` | **835 (97 %)** |
| ModuleNotFoundError (`import sympy.<submodule>`, which the stand-in does not provide) | 12 |
| TypeError (arithmetic on an instance placeholder such as `sympy.pi * 2`) | 9 |
| AttributeError | 3 |
| ImportError | 2 |

The first unavailable name each failing program reaches is mostly `symbols` (624), then `sqrt` (42), `Rational` (40), `Symbol` (21), `binomial` (20) and `Matrix` (11).

**The expectation holds.** Using the census scanner (`census.scan`, the one AST scanner) with the introspection table:
- 97 of the 996 scored programs import SymPy but use no SymPy name, and every scored program imports SymPy.
- 94 of those 97 pass level 1, so they make up 94 of the 134 level-1 passes.
- So the pass share is roughly the share of programs that use no SymPy name, as expected.

**The other 40 level-1 passes, and a caveat.**
- 2 are timeouts on both sides.
- 14 are golden error cases where the stand-in raises the same type with the same text: AttributeError 8, ModuleNotFoundError 3 and ImportError 3.
- **24 match by coincidence.**
  - In 22 of them, SymPy itself raises `NotImplementedError` (for example "solving Abs(x) when the argument is not real or imaginary"), and the stand-in's placeholder raises the same type for an unavailable name.
  - In the other 2, both sides raise TypeError, with different messages.
  - Level 3 compares the type name only, as the plan specifies, and stdout is empty on both sides. So these 24 pass all three levels while `error_message_equal` is false.
  - A stricter reading of the baseline (level 1 plus equal error text) is 110 of 996 (11.0 %). This is a property of the placeholder's exception type, not of the scorer; see Issues.

This is the measured M0 baseline, not a target.

## GUEST-01 differential (imports mini-suite)

`testdata/programs/imports.jsonl` is own-authored, with case_id = sha256 of the code. It has 9 programs:
- the five import forms, each followed by `print("ok")`;
- `from sympy import binom`;
- `from sympy.abc import nonexistent_name`;
- `sympy.not_a_name`;
- `print(len(sympy.__all__))`.

- **Goldens.** I ran `cargo xtask oracle golden --name imports-v1 --input testdata/programs/imports.jsonl --cpu-seconds 10`.
  - 16 runs (4 seeds × 2 runs × 2 platforms): 9 golden, 0 adjudicated (ok 6, error 3).
  - golden_sha256 is `d31ab43d08e287c3e6332d30315ff05bcfb1b928ecdf848e71f1825a7ff28471`. SymPy prints `921` for `len(sympy.__all__)`.
- **Stand-in.** `cargo xtask oracle standin --golden-manifest conformance/golden/imports-v1.golden.json --name imports-v1` scored 9 of 9 on levels 1, 2 and 3, with error_message_equal 3 of 3, missing 0 and lint pass.
- **The normalisation is load-bearing (RED witness).** The golden and stand-in record blobs (`d31ab43d…` and `d4a56d8b…`) differ on exactly two lines, and only in the ImportError paths:
  - `(/usr/local/lib/python3.13/site-packages/sympy/__init__.py)` against `(/opt/ksci/standin/sympy/__init__.py)`;
  - the same for `sympy/abc.py`.
  - A verbatim comparison would give 1 of 3 equal messages; after removing the trailing parenthesis, 3 of 3. The AttributeError text is equal without normalisation.
- **No stand-in defect.** No diff needed a fix in plan 01-12's generator.

## Accomplishments

- **Scorer (CONF-03).** `conformance/runner/score.py` uses only the standard library plus the stand-in mirror.
  - It has the functions `score_case`, `score`, `final_answer`, `normalise_import_message`, `percent` and `encode`, and a CLI: `--golden --candidate [--exclude] --out --cases-out`.
  - It validates the record shape and refuses duplicate case_ids (exit 2).
  - The aggregate is `sympy._wire.dumps` bytes. Per-case output is JCS JSON Lines in case_id order and holds no stdout, messages or code.
- **`oracle standin`** (`xtask/src/oracle.rs`) does the following, in order:
  1. It requires `standin/` to match HEAD and records `HEAD:standin`.
  2. It runs `oracle golden --verify` on the manifest.
  3. It obtains the pinned base image.
  4. It calls `run_script` with `mount_standin: true`, `prelude: host_shim`, seed 0 and the manifest's input and mem_bytes. No KSCI_WORKER reaches the container.
  5. It re-checks that `standin/` and the base pin did not move during the run.
  6. It checks the stand-in run.json, scores the records with the venv Python, stores the per-case file and runs the lint.
  7. It writes the merged score file atomically (partial file, then rename).
- **`check_report`.** `check_run_report` (goldens) and the new `check_standin_report` now share one run.json checker, `check_report`.

## Task Commits

1. **Task 1 (tracer): scorer, `oracle standin`, imports mini-suite at 100 %.** `cd19e7c` (feat).
   - Tracer gate: interactive mode, `end-of-phase`, automated-only verify. After the commit I re-ran the conformance unit suite (34 tests, OK) and the score assert, then expanded to Task 2.
2. **Task 2: M0 baseline on the TIR slice (proxy).** `3405ec1` (feat).

**RED.** I wrote `test_score.py` first against a stub `score.py` whose functions raise NotImplementedError: 12 tests, 12 errors. After the implementation, 12 of 12 pass. The two new xtask tests pass, and all 13 oracle tests pass.

## Verification (plan level)

- `env PYTHONPATH=standin .venv/bin/python -m unittest discover -s conformance -t conformance -v`: 34 tests, OK. That is 12 new and 22 census tests.
- Both score-file asserts from the plan pass. `grep -c proxy` prints 1, and `grep -c '"stdout"'` prints 0 on both score files (and the same for `"code"` on the TIR file).
- `imports-v1.golden.json`: 16 runs, 0 adjudication entries.
- `cargo xtask check no-corpus`: no violations. `cargo xtask check provenance`: ok.
- **`cargo xtask ci` on the committed tree `3405ec1`: all 13 steps passed.** The new tests run in the existing "conformance python" step (discovery root `conformance`, `PYTHONPATH=standin`), so no CI step was added.

## Files Created/Modified

- `conformance/runner/__init__.py`, `conformance/runner/tests/__init__.py`: package markers.
- `conformance/runner/score.py`: the three-level scorer.
- `conformance/runner/tests/test_score.py`: 12 tests. They cover levels, truncation, error class, empty, missing, excluded, order, malformed input, percent, ImportError normalisation, and the CLI.
- `testdata/programs/imports.jsonl`: the 9 own-authored import programs.
- `conformance/golden/imports-v1.golden.json`: the imports golden manifest.
- `conformance/golden/scores/imports-v1.m0-standin.json`, `conformance/golden/scores/tir-slice-v1.m0-standin.json`: aggregate scores and digests, labelled proxy.
- `xtask/src/oracle.rs`: `OracleCmd::Standin`, `StandinArgs`, `standin`, `check_standin_report`, `merge_score`, `venv_python`, `git_text`, `standin_changes` and the shared `check_report`, plus 2 tests.

## Decisions Made

See `key-decisions` in the frontmatter. The most consequential is the nesting of the levels. The plan's behaviour block requires `{ok, "a\n5\n"}` against `{error, ValueError}` to score (False, False, False). Making levels 1 and 2 conditional on level 3 is the one definition that satisfies that and keeps the levels ordered, so level 1 implies level 2, which implies level 3.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 2 - Missing critical] Score honesty checks (threat T-01-36)**
- **Found during:** Task 1.
- **Issue:** As planned, a score could name a stand-in tree other than the one mounted (the working tree), or score against a manifest whose goldens or adjudication list had been edited.
- **Fix:**
  - `oracle standin` refuses to run when `standin/` differs from HEAD, and refuses to write if it changed during the run. It also runs `oracle golden --verify` first, re-checks the base pin after the run, and checks the stand-in run.json, including `sympy: null`, which shows no real SymPy environment was found.
  - The score also records `run_programs_blob` and `scorer_blob` (git hash-object), `image` and `params`.
  - The scorer reports `extra` (candidates that are neither golden nor excluded) and `error_cases` (the denominator of error_message_equal).
- **Files modified:** xtask/src/oracle.rs, conformance/runner/score.py.
- **Verification:** `standin_run_report_must_match_the_run`, `standin_score_merge_never_overwrites`, `test_score_excluded`.
- **Committed in:** `cd19e7c`.

**2. [Rule 1 - Refactor for one mechanism] Shared run.json checker**
- **Issue:** The stand-in run needs the same run.json checks as a golden run, except the SymPy-backend pointers.
- **Fix:** I extracted `check_report(what, report, expected)`. `check_run_report` now builds its list and calls it. Its behaviour is unchanged, and `golden_run_report_must_match_the_run` still passes.
- **Committed in:** `cd19e7c`.

---

**Total deviations:** 2 (1 missing critical, 1 refactor). **Impact:** no scope change. The score file is a superset of the plan's keys.

## Issues Encountered

- **The placeholder's exception type collides with SymPy's own (for the operator).** The stand-in's unavailable-name functions raise `NotImplementedError`, the type SymPy raises for unsupported solver cases. Because of this, 22 TIR cases score on all three levels by coincidence, and 2 more do the same through TypeError. That inflates the M0 proxy by about 2.4 points (13.5 % against 11.0 % with equal error text). Options, for a later plan rather than this one (01-12 owns the generator):
  - (a) Keep it, and report the level-1-plus-message figure next to the levels from now on. This is my recommendation for M0, since every unavailable-name case will be replaced in Phase 2.
  - (b) Give the placeholders a type of their own, a subclass of NotImplementedError, so level 3 separates them.
  - (c) Make level 3 compare the normalised message as well. That changes CONF-03's definition.
- **LSP.** rust-analyzer answered "No LSP server available for file type: .rs". Per CLAUDE.md rule 7 I used the fallback:
  - `ast-grep` scans for the call sites of `OracleCmd::*`, `run_script` and `host::git_output` before editing;
  - `cargo check` through clippy `--all-targets -D warnings`, which is clean;
  - `cargo xtask ci`, which is green.
  - The only signature change is internal: `check_run_report` now delegates to `check_report`, and it has a single call site (`finalise`).
- **Plan ledger.** As in 01-10, I did not write the #3968 ledger into the shared git dir. The base `5f97524` is the spawn base checked at start. `commits: 2` is `git rev-list --count 5f97524..HEAD`, measured before this SUMMARY commit.
- **Python venv path in three places.** `ci.rs` and `host::venv` build `.venv/bin/python` themselves, and the new `oracle::venv_python` builds it a third time. One shared `host` helper would be cleaner. I left ci.rs and host.rs alone, because sibling wave agents may edit them; this is a small DRY follow-up.
- **Remote store.** Nothing was pushed. The imports goldens, the 16 run blobs, both stand-in record blobs and both per-case blobs are in `~/.cache/klein-science` only, as for 01-10.
- `release::tests::gate_*_accepts_dist` fail under a bare `cargo test -p xtask` until `target/dist` exists. They pass inside `cargo xtask ci`, which builds dist first. This was already the case and has nothing to do with this plan.

## User Setup Required

None.

## Next Phase Readiness

- Every later TIR gate (60 %, 90 %, 97 %) can be measured with `cargo xtask oracle standin` against `tir-slice-v1.golden.json`, compared with this proxy baseline.
- Open for the operator:
  - the placeholder exception-type question above (resolved in the Follow-up below);
  - pushing the golden, run, record and per-case blobs to the remote store (D-05).

## Follow-up (2026-10-12, after the plan)

The placeholder exception-type question was resolved under the operator's standing instruction (the recommended option after root-cause analysis).

- **The finding.** 24 of the 134 level-1 passes are coincidences, and so are 25 of the 135 level-3 passes:
  - 23 golden cases raise SymPy's own `NotImplementedError` and meet a placeholder's `NotImplementedError`. 22 of them pass all three levels; 1 passes level 3 only, because SymPy printed 8 bytes before raising. That case is the gap between level 3 (135) and level 1 (134).
  - 2 golden cases raise TypeError, and the stand-in raises TypeError too, with other text.
- **Why the subclass option (b) is ruled out.** A distinct `NotImplementedError` subclass cannot be written in guest code. Lint R1 mirrors a real Monty limit. Checked with pydantic-monty 1.1.0 from the dev venv:
  - `class StandinNotImplementedError(NotImplementedError): pass` is rejected at parse time: "The monty syntax parser does not yet support class inheritance and metaclasses".
  - `type('StandinNotImplementedError', (NotImplementedError,), {})` passes the lint, but at run time Monty gives "TypeError: type() bases are not supported". It would also be a way around R1.
  - A plain class with no bases cannot be raised: Monty turns `raise C()` into a TypeError.
  - CPython 3.13 has no built-in subclass of `NotImplementedError` (`__subclasses__()` is `[]`), so no built-in type is both distinct by name and caught by `except NotImplementedError`.
- **How the 2 TypeError cases arise.** They are caused by the placeholders, but not by the placeholder's own raise. Values such as `sp.pi`, `sp.true` and `sp.false` are bound as plain functions, so Python's operator dispatch raises before any placeholder runs:
  - `2 * sp.pi` gives "unsupported operand type(s) for *: 'int' and 'function'", where SymPy raises "cannot determine truth value of Relational: …";
  - `sp.false | sp.true` gives "… for |: 'function' and 'function'", where SymPy raises "BooleanAtom not allowed in this context."
  - So even a subclass would have left these two.
- **What was done instead (option (a)).** `conformance/runner/score.py` adds one stricter count, `level1_message_equal` (with `level1_message_equal_percent`).
  - It is level 1 and, for a golden error case, equal error text, compared through the existing ImportError path normaliser. For every other case it equals level 1.
  - The three CONF-03 levels and their definitions are unchanged.
  - The tests in `test_score.py` were seen RED: 4 errors (KeyError) before the scorer change.
  - Commit `99f1124`.
- **The new figures.** The M0 scores were regenerated with the same candidate records: run.json `eebc5543…` and records `880e0b0a…` are unchanged, and only the scorer blob and the per-case blob are new.

  | Score file | level 1 | level 2 | level 3 | level1_message_equal |
  |---|---|---|---|---|
  | `tir-slice-v1.m0-standin.json` | 134 (13.5 %) | 134 (13.5 %) | 135 (13.6 %) | **110 (11.0 %)** |
  | `imports-v1.m0-standin.json` | 9 (100.0 %) | 9 (100.0 %) | 9 (100.0 %) | **9 (100.0 %)** |

  The 24 cases the strict count drops are exactly the 22 NotImplementedError and the 2 TypeError coincidences. No case passes the strict count without passing level 1.
- **Open item.** Monty: subclassing built-in exceptions. This is a request candidate for 01-18's Klein-core request drafts, and it is not filed yet. If Monty gains it, the placeholders can get their own type and R1 can be relaxed explicitly for that case.
- **Also done after the plan.** `host::venv_python_path` is now the one definition of the dev venv's interpreter path (commit `e41ef31`). That closes the "Python venv path in three places" issue above.

---
*Phase: 01-contracts-oracles-and-risk-probes*
*Completed: 2026-10-12*

## Self-Check: PASSED

- All 9 key files exist on disk.
- `git log 5f97524..HEAD` lists exactly `cd19e7c` and `3405ec1`, and the working tree was clean before this SUMMARY.
- STATE.md and ROADMAP.md were not modified; the orchestrator owns them.
