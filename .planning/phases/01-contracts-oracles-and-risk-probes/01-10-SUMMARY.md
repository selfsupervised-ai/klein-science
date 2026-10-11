---
phase: 01-contracts-oracles-and-risk-probes
plan: 10
subsystem: conformance
tags: [goldens, sympy, pythonhashseed, docker, jcs, sha256, adjudication, reproducibility]

requires:
  - phase: 01-05
    provides: "the hardened oracle runner (run_script), pinned sympy images for linux/arm64 and linux/amd64 (tars in the store), run_programs.py"
  - phase: 01-07
    provides: "the 1,000-program TIR slice, slice.jsonl stored by digest 791aab3f…b766 (conformance/golden/tir-slice-v1.slice.json)"
  - phase: 01-02
    provides: "the artifact store (Cache, put, fetch_blob) and check no-corpus"
provides:
  - "cargo xtask oracle golden --name --input [--seeds --runs --platforms --jobs --cpu-seconds]: the D-10 matrix through the one run_script"
  - "cargo xtask oracle golden --verify <manifest>: re-derives a manifest byte for byte from its stored run blobs"
  - "golden manifest format ksci.golden-manifest.v1 (GoldenManifest in xtask/src/oracle.rs)"
  - "conformance/golden/smoke-v1.golden.json: 7 golden, the set-printing case quarantined (reason seed)"
  - "conformance/golden/tir-slice-v1.golden.json: 996 golden, 4 adjudicated (reason seed); golden_sha256 de46d3b2…ddc1"
  - "run_programs.py takes producer metadata keys (the slice's stratum) and ignores them"
affects: [01-13 stand-in baseline scored against tir-slice-v1 and the goldens, 01-17 census (shares run_programs.py), Phase 2 full-corpus goldens]

actuals:
  tokens: 12541
  tasks: 2
  commits: 3
plan_head_before: 229fc4c86235b54a2c41b5e004faa51bdf6383c5
plan_head_after: 4c660048aacdafae11cbb9e7ab33ab2d546cc4c8

tech-stack:
  added: []
  patterns:
    - "Finalisation is one function (finalise) shared by golden and --verify, so the manifest is a pure function of its stored runs"
    - "A run's run.json is evidence: its seed, machine, limits, worker request, sole input and SymPy backends must match the run the manifest names"
    - "Adjudication reason by precedence, each factor judged with the others held fixed: status > platform (same seed and run) > seed (same platform and run) > run"

key-files:
  created:
    - conformance/golden/smoke-v1.golden.json
    - conformance/golden/tir-slice-v1.golden.json
  modified:
    - xtask/src/oracle.rs
    - conformance/oracles/run_programs.py

key-decisions:
  - "The manifest adds schema (ksci.golden-manifest.v1), run_sha256 per run, and counts.adjudication_by_reason to the plan's keys. run.json carries the versions and seed (research Pattern 6), so linking it by digest lets --verify check every run's label"
  - "mem_bytes is passed explicitly (--mem-bytes 2147483648) so the manifest records what xtask set, and --verify checks it against each run.json"
  - "Seeds and platforms are sorted and deduplicated; runs are listed by (platform, seed, run) but executed repetition-major, so the two runs of one (platform, seed) are far apart in time"
  - "--verify takes the given fields (name, input, images, run_programs_blob, params, runs) from the file and re-derives counts, golden_sha256 and adjudication; it also requires the goldens to be fetchable (CONF-02)"
  - "run_programs.py: case_id and code stay required and sha-checked; other keys are producer metadata, ignored (Rule 3 fix; the slice's stratum broke the first TIR run)"

patterns-established:
  - "Golden manifests in git hold digests, params, counts and case_ids only; record bytes stay in the store"
  - "A golden is admitted only on byte-identical canonical records across 4 seeds × 2 runs × 2 platforms"

requirements-completed: [CONF-01, CONF-02]

coverage:
  - id: D1
    description: "oracle golden: the D-10 matrix (16 runs), byte-identical admission, adjudication with reasons, goldens stored by digest, manifest written only after every run"
    requirement: CONF-01
    verification:
      - kind: unit
        ref: "xtask/src/oracle.rs#golden_judge_admits_only_agreeing_cases (seen RED against the stub judge)"
        status: pass
      - kind: unit
        ref: "xtask/src/oracle.rs#golden_matrix_is_exact (seen RED against the stub)"
        status: pass
      - kind: unit
        ref: "xtask/src/oracle.rs#golden_run_report_must_match_the_run (seen RED against the stub)"
        status: pass
      - kind: unit
        ref: "xtask/src/oracle.rs#golden_judge_requires_the_same_cases_in_every_run"
        status: pass
      - kind: integration
        ref: "cargo xtask oracle golden --name smoke-v1 --input testdata/programs/smoke.jsonl --cpu-seconds 2 (run twice; golden_sha256 6f6a90ed… both times)"
        status: pass
    human_judgment: false
  - id: D2
    description: "The seed-sensitive smoke program is quarantined with reason seed (the RED witness of the reproducibility check)"
    requirement: CONF-01
    verification:
      - kind: integration
        ref: "python3 -c \"...assert any(a['reason']=='seed' for a in d['adjudication'])\" on conformance/golden/smoke-v1.golden.json"
        status: pass
    human_judgment: false
  - id: D3
    description: "--verify re-derives a manifest byte for byte from stored runs and rejects a changed one"
    requirement: CONF-02
    verification:
      - kind: integration
        ref: "cargo xtask oracle golden --verify conformance/golden/smoke-v1.golden.json"
        status: pass
      - kind: integration
        ref: "cargo xtask oracle golden --verify conformance/golden/tir-slice-v1.golden.json"
        status: pass
      - kind: integration
        ref: "--verify on two tampered copies (reason seed→run; one run's records digest swapped) exits 1"
        status: pass
      - kind: unit
        ref: "xtask/src/oracle.rs#golden_manifest_roundtrips_as_jcs"
        status: pass
    human_judgment: false
  - id: D4
    description: "TIR slice goldens: 1000 cases, 996 golden, 4 adjudicated (seed), manifest without stdout or code"
    requirement: CONF-02
    verification:
      - kind: integration
        ref: "python3 acceptance assert (golden+adjudication==cases, cases>=950, 16 runs) on tir-slice-v1.golden.json"
        status: pass
      - kind: other
        ref: "grep -c '\"stdout\"\\|\"code\"' conformance/golden/tir-slice-v1.golden.json prints 0; cargo xtask check no-corpus passes"
        status: pass
      - kind: other
        ref: "cargo xtask check provenance passes"
        status: pass
    human_judgment: false

duration: 1h 36m
completed: 2026-10-11
status: complete
---

# Phase 1 Plan 10: Reproducible goldens (D-10 protocol) Summary

**`cargo xtask oracle golden` runs a program set in the pinned SymPy image under PYTHONHASHSEED 0, 1, 2 and 4294967295, twice each, on linux/arm64 and emulated linux/amd64. Each of the 16 runs goes through the one hardened `run_script`. A case is admitted only when its canonical record is byte-identical in all 16 runs. The 1,000-program TIR slice yields 996 goldens (sha256 `de46d3b2…ddc1`, in the store only) and 4 seed-dependent cases quarantined for adjudication. The seed check was seen RED on the set-printing smoke program, and `--verify` re-derives each manifest byte for byte.**

## Performance

- **Duration:** 1h 36m (most of it container time: about 25 min for the two smoke matrices and about 65 min for the TIR matrix)
- **Started:** 2026-10-11T22:16:12Z
- **Completed:** 2026-10-11T23:52:35Z
- **Tasks:** 2 of 2
- **Files:** 2 created, 2 modified (868 insertions, 25 deletions)

## Accomplishments

- **`oracle golden`** (`xtask/src/oracle.rs`).
  - It obtains and verifies the sympy pins for each platform (`obtain_pinned`, loading the stored tar if needed), puts a path input into the store, and records `git hash-object` of run_programs.py.
  - It calls `run_script` once per (platform, seed, run) with `--require-sympy-env` and an explicit `--mem-bytes`, and collects the records.jsonl and run.json digests.
  - `finalise` is the one path for both `golden` and `--verify`. It:
    - checks that the runs are exactly the matrix;
    - checks each run.json against the run it claims to be (seed, `machine`, CPU and memory limits, requested jobs, the sole input's sha256, pure-Python ground types and mpmath backend);
    - judges the records and derives counts, golden_sha256 and the adjudication list.
  - Golden JSONL (the agreeing canonical lines, by case_id) is put into the store. The manifest is JCS, written to a `.partial.<pid>` file and renamed only after every run completed and the goldens were stored. A failed run (for example the first TIR attempt, below) wrote no manifest.
  - The pins are re-read after the runs, and a change aborts the write.
- **`oracle golden --verify <manifest>`** reloads the 32 run blobs, re-derives everything except the container runs, requires the goldens to be fetchable, and fails unless the JCS bytes are identical. On two tampered copies it exits 1, and the regenerated bytes are left in `target/oracle/` for inspection:
  - a reason changed from seed to run;
  - one run's records digest swapped for another's.
- **Goldens**: `smoke-v1` and `tir-slice-v1` (tables below).

## Seed RED witness (the reproducibility check catches a seed-dependent program)

The smoke case `e9bc520dceaaa5d5dc48801421e5f5c20473c49ceda6aca289c3e674dbf46b2c` prints a three-element set of strings. In the 16 smoke runs it printed four different orders, one per seed. Each order was identical on both platforms and in both repetitions:

| Seed | Runs with this output |
|---|---|
| 0 | amd64 ×2, arm64 ×2 |
| 1 | amd64 ×2, arm64 ×2 |
| 2 | amd64 ×2, arm64 ×2 |
| 4294967295 | amd64 ×2, arm64 ×2 |

`golden` put it in `adjudication` with reason `seed`, and it is not in the goldens. The plan's assert (`any(a['reason']=='seed' ...)`) passes. The 7 deterministic smoke cases are golden.

## smoke-v1 (`conformance/golden/smoke-v1.golden.json`)

| Field | Value |
|---|---|
| input_sha256 | `1816ae53734f37cf6df33888d55fe9078ac9ba7be1cff930eeb6f4e4acc7ce28` (testdata/programs/smoke.jsonl) |
| run_programs_blob | `a9ee19b53a640454e22a0ac0ea5f3ff16f966afc` |
| params | seeds 0,1,2,4294967295; runs 2; platforms linux/amd64, linux/arm64; cpu_seconds 2; mem_bytes 2147483648; jobs 4 |
| runs | 16 |
| cases / golden / adjudication | 8 / 7 / 1 |
| golden by status | ok 3, error 2, timeout 2 |
| adjudication by reason | seed 1 (status 0, platform 0, run 0) |
| golden_sha256 | `6f6a90ed13761510dfbf22e83fc4e591a8d2a0dfcedafd648e8f2e6513bf43bb` |

**Timeout cases.** Both the CPU-loop case and the sleep (wall-clock) case are golden, with status `timeout` in all 16 runs. Neither was adjudicated for status.

**Reproduced.** The smoke matrix was run twice: once with the original runner and once after the runner fix. All 16 records digests, all 16 run.json digests and the golden_sha256 were identical. Only `run_programs_blob` moved (`03ff51c9…` to `a9ee19b5…`).

## tir-slice-v1 (`conformance/golden/tir-slice-v1.golden.json`)

| Field | Value |
|---|---|
| input_sha256 | `791aab3fbaf3def0518b416f05f66947f23d8cddc5808dbca66eae38cd48b766` = `slice_sha256` of `tir-slice-v1.slice.json` |
| run_programs_blob | `a9ee19b53a640454e22a0ac0ea5f3ff16f966afc` |
| images | arm64 `sha256:b468cfd6…e140` (native); amd64 `sha256:4f33ed06…bfa9` (emulated) |
| params | seeds 0,1,2,4294967295; runs 2; platforms linux/amd64, linux/arm64; cpu_seconds 60; mem_bytes 2147483648; jobs 8 |
| runs | 16. Every run reported ok=859 error=130 timeout=11 |
| cases / golden / adjudication | **1000 / 996 / 4** |
| golden by status | **ok 857, error 128, timeout 11** |
| adjudication by reason | **seed 4**; status 0, platform 0, run 0 |
| golden_sha256 | **`de46d3b284d104a8460a63a3a9f0278b17d79b02bb7485578630837a7c98ddc1`** |
| manifest size | 4,777 bytes. `grep -c '"stdout"\|"code"'` prints 0 |

**The four adjudicated cases.** All four have reason `seed`. In each case, every variant is identical on both platforms and in both repetitions for its seed.

| case_id (prefix) | Stratum | Status | What varies with the seed |
|---|---|---|---|
| `33a2ba85…` | logic | error | the iteration order of a printed dict of `Q.*` assumptions (3 variants) |
| `c3e4fbba…` | solvers | error | which `Abs(...)` argument a `NotImplementedError` message names (2 variants) |
| `f7acd77f…` | solvers | ok | the order of a printed set of tuples (4 variants) |
| `f8b84155…` | solvers | ok | the order of a returned solution list (2 variants) |

**Pitfall J.** No disagreement comes from printed `Dummy` names (`_x`), so nothing is listed for Phase 2 normalisation. Near-limit timeouts did not flip: all 11 timeouts are golden in every run. Emulated amd64 ran about 4 minutes per run, so it was far from the 60 s per-case limit's flip zone for these programs.

## Task Commits

1. **Task 1 (tracer): golden protocol and the smoke goldens.** `a21b2c3` (feat).
2. **Rule 3 fix: run_programs.py accepts the slice's metadata key; smoke regenerated.** `10c707e` (fix).
3. **Task 2: TIR slice goldens.** `4c66004` (feat).

**Tracer gate.** The mode was interactive with `end-of-phase` and an automated-only verify. `--verify` was re-run on the committed tree and printed "re-derives byte for byte from 16 stored runs". Then the plan expanded to Task 2.

**Unit tests seen RED.** I wrote the five golden tests first against stubs of `judge`, `check_matrix` and `check_run_report`, which returned "not implemented". `cargo test -p xtask golden` gave 3 failed and 3 passed:
- The three target tests failed on their first `unwrap` of the stub's error.
- `golden_judge_requires_the_same_cases_in_every_run` passed only because it asserts errors.
- The roundtrip test and the pre-existing no-corpus test do not touch the stubs.

After the implementation, all 68 xtask tests pass, and clippy `-D warnings` and `cargo fmt --check` are clean.

## Files Created/Modified

- `xtask/src/oracle.rs`:
  - adds `OracleCmd::Golden` and `GoldenArgs`, and the manifest types `GoldenManifest`, `GoldenImage`, `GoldenParams`, `GoldenRun`, `GoldenCounts` and `Adjudication`;
  - adds `judge`, `check_matrix`, `check_run_report`, `finalise`, `golden` and `verify_golden`, plus 5 tests;
  - `record_counts` now sits on a shared `parse_records`, and `check_name` takes the kind of name it checks.
- `conformance/oracles/run_programs.py`: the input contract changed from exactly `{case_id, code}` to requiring `case_id` and `code`, with other keys ignored.
- `conformance/golden/smoke-v1.golden.json`: the smoke manifest.
- `conformance/golden/tir-slice-v1.golden.json`: the TIR slice manifest.

## Decisions Made

See `key-decisions` in the frontmatter. In short:
- The manifest adds a schema tag, a run.json digest for each run, and reason counts, so the labels of the runs can be verified.
- Memory is passed explicitly.
- The run order is canonical in the manifest, but runs are executed repetition-major.
- `--verify` re-derives the derived fields from the stored runs and treats the rest as the recorded description of the runs.

**Reason precedence, made precise.** "platform" means the platforms disagree for some (seed, run). "seed" means the seeds disagree for some (platform, run). "run" is what is left: only the two repetitions of one (platform, seed) disagree. A case that is flaky between repetitions and also happens to differ across platforms is therefore labelled `platform`, as the plan's precedence prescribes. Neither manifest has such a case.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] run_programs.py rejected the golden slice's input lines**
- **Found during:** Task 2, first `oracle golden --name tir-slice-v1` run.
- **Issue:** The slice from plan 01-07 writes `{case_id, code, stratum}` per line. run_programs.py (plan 01-05) required exactly `{case_id, code}`, so the first container run exited 2 (`...:1: a case is exactly {case_id, code}`). `golden` then aborted with "stored no records.jsonl (the run failed); no manifest written", which is the designed interrupt behaviour.
- **Fix:** `case_id` and `code` stay required, and case_id is still checked against sha256(code). Other keys are producer metadata and are ignored, and the docstring says so. The alternatives were rejected:
  - Rewriting the slice would move the pinned `slice_sha256`.
  - Feeding a projected copy would break `input_sha256 = slice_sha256`, which plan 01-13 relies on to run the same input.
- **Files modified:** conformance/oracles/run_programs.py; smoke-v1 regenerated with the fixed runner (identical records, run.json and golden digests).
- **Verification:** The failing run is the RED. The re-run TIR matrix completed all 16 runs.
- **Committed in:** `10c707e`.

**2. [Rule 2 - Missing critical] Run evidence and pin stability checks**
- **Found during:** Task 1.
- **Issue:** The plan's manifest lists a run's `{platform, seed, run, records_sha256}`. Nothing tied those labels to what actually ran, so a mislabelled or swapped run would pass `--verify` (threat T-01-28).
- **Fix:**
  - Each run's run.json digest goes in the manifest, and `finalise` checks the seed, machine, limits, jobs, input and SymPy backends against it.
  - `golden` re-reads the sympy pins after the runs and refuses to write if they moved.
- **Files modified:** xtask/src/oracle.rs.
- **Verification:** `golden_run_report_must_match_the_run`, and both manifests verify.
- **Committed in:** `a21b2c3`.

---

**Total deviations:** 2 auto-fixed (1 blocking, 1 missing critical).
**Impact on plan:** Both are needed for correct, verifiable goldens. The manifest format is a superset of the plan's keys.

## Issues Encountered

- **LSP.** rust-analyzer reported "No LSP server available for file type: .rs". Per CLAUDE.md I used the fallback: `ast-grep` structural scans for the call sites of `run_script`, `check_name` and `record_counts` before editing, plus `cargo check --workspace --all-targets` (clean).
- **Plan ledger.** The worktree harness refused writes into the shared `.git/worktrees/...` directory, so the #3968 ledger file was not written. The base `229fc4c` is the known spawn base, and `commits: 3` is `git rev-list --count 229fc4c..HEAD`, measured before this SUMMARY commit.
- **heavy-job contention.** Other agents held the heavy-job lock at times, and runs waited for it as designed. The record digests show this did not affect any result.
- **Remote store.** Nothing was pushed (D-05). The goldens, the 32 run blobs of each manifest and the inputs are in the local cache (`~/.cache/klein-science`) only. Pushing them to GHCR needs operator approval. Until then, `--verify` works only on this Mac or with the remote populated.

## User Setup Required

None.

## Next Phase Readiness

- Plan 01-13 can score the stand-in against `tir-slice-v1.golden.json`. That manifest gives:
  - `input_sha256` (the slice, now runnable by run_programs.py);
  - `golden_sha256`;
  - the adjudication list to exclude.
- **Open for the operator:** push the golden and run blobs to the remote store (D-05) so the manifests verify off this machine.

---
*Phase: 01-contracts-oracles-and-risk-probes*
*Completed: 2026-10-11*
