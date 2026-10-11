---
phase: 01-contracts-oracles-and-risk-probes
plan: 09
subsystem: infra
tags: [reproducible-builds, wasm-tools, remap-path-prefix, ld64, github-actions, self-hosted-runner, ci]

requires:
  - phase: 01-contracts-oracles-and-risk-probes (01-03)
    provides: cargo xtask ci step runner, check provenance, check no-corpus
  - phase: 01-contracts-oracles-and-risk-probes (01-06)
    provides: release dist (R0 component via wasm-tools component new, R2 worker), ksci-conform
  - phase: 01-contracts-oracles-and-risk-probes (01-08)
    provides: cargo xtask tools venv, hash-locked dev.lock, standin and conformance unittest suites
provides:
  - "release::build_artifacts: the one build path of dist and dist --twice, with path remapping"
  - "cargo xtask release dist --twice: two builds of the committed tree from two absolute paths, digests compared, builder-path search, digests.json with reproducible:true"
  - "cargo xtask release gates <artifact>: zero-import gate (D-16) and relaxed-SIMD gate, run on every build"
  - "Permanent RED fixtures xtask/tests/fixtures/gates/imports.wat and relaxed-simd.wat"
  - "The full Phase 1 cargo xtask ci (13 steps), the same command locally and on the VM"
  - ".github/workflows/ci.yml: push-to-main and workflow_dispatch only; ci and publish-dist jobs on [self-hosted, linux, x64, klein-science]"
affects: [01-14 VM runner registration, 01-16 signing and cross-host digest check, every later phase's CI]

actuals:
  tokens: 14400
  tasks: 3
  commits: 8
plan_head_before: 229fc4c86235b54a2c41b5e004faa51bdf6383c5
plan_head_after: 825b32abdd1d94651ba832420bca32376ab5193f

tech-stack:
  added: []
  patterns:
    - "Release rustflags only through CARGO_ENCODED_RUSTFLAGS: --remap-path-prefix <tree>=/src and <CARGO_HOME>=/cargo; +simd128 for wasm only"
    - "Every release build uses the same relative target dir (target/dist-build) inside the tree it builds"
    - "Apple workers link with -Wl,-oso_prefix,<tree>/ so LC_UUID does not depend on the tree's path"
    - "A heavy-job holder marks its children (KSCI_HEAVY_JOB_HELD) so a nested guard runs under the parent's hold"

key-files:
  created:
    - xtask/tests/fixtures/gates/imports.wat
    - xtask/tests/fixtures/gates/relaxed-simd.wat
    - .github/workflows/ci.yml
  modified:
    - xtask/src/release.rs
    - xtask/src/ci.rs
    - xtask/src/host.rs
    - xtask/src/depinfo.rs
    - xtask/src/check.rs
    - xtask/src/workspace.rs
    - Cargo.toml
    - .planning/phases/01-contracts-oracles-and-risk-probes/deferred-items.md

key-decisions:
  - "macOS worker reproducibility (A6): ld64 hashes N_OSO debug-map paths of the LTO object into LC_UUID; fixed with -Clink-arg=-Wl,-oso_prefix,<src_root>/ on apple triples"
  - "One relative build dir (target/dist-build) for dist and dist --twice, so a plain dist ships the same bytes the check compared"
  - "publish-dist runs dist --twice (not plain dist), so the published digests.json itself records reproducible:true for the bytes it lists"
  - "check provenance now judges the shipped R0 compile: depinfo::R0_TARGET = wasm32-unknown-unknown and depinfo::R0_RUSTFLAGS (+simd128) are the single definitions release.rs uses"
  - "cargo xtask ci runs the Python suites against the shipped worker (KSCI_WORKER = target/dist/<host>/ksci-worker)"

patterns-established:
  - "Gate fixtures are own-authored WAT, assembled by the pinned wasm-tools in the test, never committed as binaries"
  - "A shell-free CI: the workflow only installs tools and calls cargo xtask; every guard lives in xtask/src/ci.rs"

requirements-completed: [PLUG-01]

coverage:
  - id: D1
    description: "dist --twice builds the committed tree from two absolute paths and gets identical digests for the component and the aarch64-apple-darwin worker; digests.json records reproducible:true"
    requirement: PLUG-01
    verification:
      - kind: integration
        ref: "cargo xtask release dist --twice"
        status: pass
      - kind: other
        ref: "grep -c '\"reproducible\":true' target/dist/digests.json (prints 1)"
        status: pass
    human_judgment: false
  - id: D2
    description: "Release builds remap the source tree and CARGO_HOME; no artifact contains $HOME, either source tree or CARGO_HOME"
    requirement: PLUG-01
    verification:
      - kind: integration
        ref: "cargo xtask release dist --twice (builder-path search over artifact bytes)"
        status: pass
      - kind: unit
        ref: "xtask/src/release.rs#release_remap_flags_cover_src_and_cargo_home"
        status: pass
    human_judgment: false
  - id: D3
    description: "Zero-import gate and relaxed-SIMD gate, each with a permanent RED fixture, run on every build and alone"
    verification:
      - kind: unit
        ref: "cargo nextest run -p xtask gate_ (4 tests)"
        status: pass
      - kind: integration
        ref: "cargo xtask release gates target/dist/science_cas.component.wasm"
        status: pass
      - kind: integration
        ref: "cargo xtask release gates target/wasm32-wasip2/release/ksci_component.wasm (RED witness, exit 1, 14 imports)"
        status: pass
    human_judgment: false
  - id: D4
    description: "cargo xtask ci runs every Phase 1 guard and exits 0 on the Mac"
    requirement: PLUG-01
    verification:
      - kind: integration
        ref: "cargo xtask ci (13 steps passed)"
        status: pass
    human_judgment: false
  - id: D5
    description: "CI workflow: push to main and workflow_dispatch only, self-hosted klein-science labels, contents: read, packages: write only on publish-dist, checkout pinned by SHA"
    verification:
      - kind: other
        ref: "acceptance greps on .github/workflows/ci.yml; Ruby YAML parse of the file"
        status: pass
    human_judgment: true
    rationale: "GitHub's own parse of the pushed workflow and a real run on the VM runner happen in plan 01-14; the x86_64-unknown-linux-gnu reproducibility (A6 on the VM) is only shown by that run"

duration: 18min
completed: 2026-10-11
status: complete
---

# Phase 01 Plan 09: Reproducible Gated Dist and VM CI Summary

**Two builds of the committed tree from different absolute paths give byte-identical R0 component and macOS worker (remap flags plus ld64 `-oso_prefix`), every build passes a zero-import and a relaxed-SIMD gate, and one `cargo xtask ci` of 13 guards runs locally and in a push/dispatch-only self-hosted workflow.**

## Performance

- **Duration:** about 18 min
- **Started:** 2026-10-11T22:15:56Z
- **Completed:** 2026-10-11T22:33:45Z
- **Tasks:** 3 (all done)
- **Files modified:** 11

## Accomplishments

- `release::build_artifacts` is the one build path of `dist` and `dist --twice` (D-01). Each cargo build gets exactly
  `CARGO_ENCODED_RUSTFLAGS`: `--remap-path-prefix=<tree>=/src` and `--remap-path-prefix=<CARGO_HOME>=/cargo` (each
  under its given and canonical spelling), plus `-Ctarget-feature=+simd128` for the wasm build only and
  `-Wl,-oso_prefix,<tree>/` for an Apple worker.
- `dist --twice` refuses uncommitted tracked changes, exports `git ls-files -z` into
  `$TMPDIR/ksci-repro-a/src` and `$TMPDIR/ksci-repro-b/nested/src`, builds each into its own `target/dist-build`,
  compares every artifact's SHA-256, searches the artifact bytes for `$HOME`, both trees and `$CARGO_HOME`, and only
  then ships to `target/dist/` with `"reproducible":true`.
- The import gate (`wasm-tools component wit`, no `^\s*import\s` line) and the relaxed-SIMD gate
  (`wasm-tools validate --features=-relaxed-simd`, on the component and on the core module before `component new`)
  run inside every build, and alone as `cargo xtask release gates <artifact>`.
- `cargo xtask ci` runs 13 steps and exits 0 on the Mac. `.github/workflows/ci.yml` gives the VM a `ci` job and a
  `publish-dist` job.

### Both builds' digests (Mac, aarch64-apple-darwin, rustc 1.99.0, wasm-tools 1.261.0)

| Artifact | Build a (`ksci-repro-a/src`) | Build b (`ksci-repro-b/nested/src`) |
|---|---|---|
| `science_cas.component.wasm` | `9661c2918052ff0b9e6d5c9198088d3519fc2c853b027f0719434c9401f75c68` | `9661c2918052ff0b9e6d5c9198088d3519fc2c853b027f0719434c9401f75c68` |
| `aarch64-apple-darwin/ksci-worker` | `d704bf7bbf4cf938853d73d872f717e6e08c3e6680b360a1842e369c49e78558` | `d704bf7bbf4cf938853d73d872f717e6e08c3e6680b360a1842e369c49e78558` |

A plain `cargo xtask release dist` of the checkout itself gives the same two digests. The same digests appeared in
the `release dist` step of the final `cargo xtask ci` run (HEAD 825b32a). digests.json (from `--twice`):
`526fbd045431a8266df7af695172283890d0fc9587159e48c707933ab7e6c6f2`.

### A6 on the Mac: cause and fix

The first `dist --twice` gave equal components but different workers
(`f9995f0a…` vs `2558bd9d…`). `cmp -l` showed 48 differing bytes: the 16-byte `LC_UUID` and the ad-hoc code
signature's hash of the page holding it. ld64 derives `LC_UUID` from the whole link, including the debug-map
(`N_OSO`) entries that name the LTO object by its absolute path in the target directory (the object carries std's
line tables, so the entry exists). Stripping later removes the entries but not the UUID. A hand rebuild of both trees
with `-Clink-arg=-Wl,-oso_prefix,<tree>/` gave the same worker and UUID (`32BAF6C9-…`), so `build_artifacts` now
passes it for Apple triples. Because the remaining OSO path is relative to the tree, the target directory must also
be the same relative path in every build: a plain `dist` into `target/dist-build` against `--twice` into `target`
gave a different worker until both used `BUILD_DIR = target/dist-build`. The x86_64-unknown-linux-gnu worker is
checked by the same command on the VM (plan 01-14).

### wasip2 RED witness of the import gate (D-16)

```
cargo build --locked --release -p ksci-component --target wasm32-wasip2
cargo xtask release gates target/wasm32-wasip2/release/ksci_component.wasm
error: import gate: target/wasm32-wasip2/release/ksci_component.wasm has 14 import(s); the R0 component must import nothing (D-16):
  import wasi:io/poll@0.2.12;
  import wasi:clocks/monotonic-clock@0.2.12;
  import wasi:io/error@0.2.12;
  import wasi:io/streams@0.2.12;
  import wasi:cli/stdout@0.2.12;
  import wasi:cli/stderr@0.2.12;
  import wasi:cli/stdin@0.2.12;
  import wasi:cli/environment@0.2.12;
  import wasi:cli/exit@0.2.12;
  import wasi:cli/terminal-input@0.2.12;
  import wasi:cli/terminal-output@0.2.12;
  import wasi:cli/terminal-stdin@0.2.12;
  import wasi:cli/terminal-stdout@0.2.12;
  import wasi:cli/terminal-stderr@0.2.12;
```

Exit 1, exactly the 14 WASI 0.2.x imports research Pitfall A predicts. That artifact lives in
`target/wasm32-wasip2/` and is never shipped; only `target/dist/` ships.

## Task Commits

1. **Task 1: Tracer, release dist --twice** - `b26741a` (feat), `6e8f06b` (fix: `-oso_prefix`), `198953a` (fix: one
   relative build dir)
2. **Task 2: gates (TDD)** - `a85e700` (test, RED), `99dbcf3` (feat, GREEN)
3. **Task 3: full ci and the workflow** - `5080514` (feat), `825b32a` (fix: Python suites against the shipped worker)
4. Deferred items closed - `0265579` (docs)

**Plan metadata:** recorded in the SUMMARY commit below.

## TDD Gate Compliance (Task 2)

- **RED** `a85e700`: `gate_imports_rejects_fixture` and `gate_relaxed_rejects_fixture` were written against gates
  stubbed to return `Ok(())`. A nextest run with a JUnit report (scratch profile, `target/nextest/red/junit.xml`)
  exited 100 with 2 of 4 tests failing. Both records returned `RED_EVIDENCE_OK` (`target_test_failed`, format
  `junit`) from `gsd-tools check tdd-red-evidence`. Semantic assessment: each target ran and failed on its
  `expect_err` because the stub accepted the fixture; that is the planned assertion, not a load, parse or fixture
  fault (both fixtures assembled through the pinned `wasm-tools parse`). The two `*_accepts_dist` tests passed
  vacuously against the stubs, as expected for the regression side.
- **GREEN** `99dbcf3`: the real gates. `cargo nextest run -p xtask gate_` runs 4 tests, 4 pass.
- **REFACTOR:** none needed.

## Files Created/Modified

- `xtask/src/release.rs`: `build_artifacts`, `dist`, `dist --twice`, remap and link flags, the two gates,
  `release gates`, `shipped_worker`, unit tests (`release_*`, `gate_*`).
- `xtask/tests/fixtures/gates/imports.wat`, `relaxed-simd.wat`: own-authored permanent RED fixtures.
- `xtask/src/ci.rs`: typed steps (cargo or the dev venv's Python), the 13-step Phase 1 sequence, heavy-job marker,
  `KSCI_WORKER` for the Python suites.
- `xtask/src/host.rs`: `cargo_home()`, `HEAVY_JOB_HELD` and `HeavyGuard { _lock: Option<File> }`.
- `xtask/src/depinfo.rs`, `xtask/src/check.rs`, `xtask/src/workspace.rs`, `Cargo.toml`: the R0 compile of
  `check provenance` is now the shipped one (`wasm32-unknown-unknown`, `+simd128`); `compile_inputs` takes explicit
  rustflags.
- `.github/workflows/ci.yml`: the VM workflow.
- `deferred-items.md`: 01-06's two dist items marked resolved.

## CI step list (`cargo xtask ci`)

tools check, fmt, clippy, build (kept from fda1983), deny, check provenance, check no-corpus, store fetch rfc8785,
release dist --twice (with gates), nextest (165 passed, 6 skipped), tools venv, standin python (14 OK),
conformance python (22 OK). Final line: `ci: all 13 steps passed`.

## Decisions Made

See `key-decisions` above. In short: `-oso_prefix` and one relative build dir for macOS reproducibility;
`publish-dist` builds with `--twice` so the published record proves itself; `check provenance` judges the shipped
R0 compile; the Python suites in CI test the shipped worker.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Native worker not reproducible across source paths (assumption A6)**
- **Found during:** Task 1 (tracer verify)
- **Issue:** ld64 `LC_UUID` depends on absolute `N_OSO` object paths; 48 bytes differed between the two trees.
- **Fix:** `-Clink-arg=-Wl,-oso_prefix,<src_root>/` for Apple triples (`native_link_flags`); the plan anticipated
  a build-flag fix here.
- **Files modified:** xtask/src/release.rs
- **Verification:** both trees give worker `d704bf7b…`; `release_native_link_flags_apple_only`.
- **Committed in:** 6e8f06b

**2. [Rule 1 - Bug] Plain dist and dist --twice shipped different workers**
- **Found during:** Task 1
- **Issue:** with `-oso_prefix` the relative target dir enters the UUID; `dist` used `target/dist-build` and the twice
  builds used `target`.
- **Fix:** one `BUILD_DIR = target/dist-build` for every release build; `build_artifacts` refuses any other target dir.
  Its signature keeps the plan's `(src_root, target_dir, out_dir)` but enforces this one value.
- **Files modified:** xtask/src/release.rs
- **Verification:** plain dist and dist --twice both give `d704bf7b…` / `9661c291…`.
- **Committed in:** 198953a

**3. [Rule 3 - Blocking] `cargo xtask ci` would deadlock on the heavy-job lock**
- **Found during:** Task 3
- **Issue:** `ci` holds `$KSCI_CACHE/heavy.lock` and the new `release dist --twice` step takes it too, in a child
  process, so it would wait on its own parent forever.
- **Fix:** `ci` sets `KSCI_HEAVY_JOB_HELD=1` on every step; `heavy_job()` that finds the lock taken with the marker
  set runs under the parent's hold (a child that finds the lock free still takes it).
- **Files modified:** xtask/src/host.rs, xtask/src/ci.rs
- **Verification:** the ci log shows `heavy-job: … is held by the parent job (KSCI_HEAVY_JOB_HELD)` and the step
  passes.
- **Committed in:** 5080514

**4. [Rule 3 - Blocking] The plan's Python steps are not cargo commands**
- **Found during:** Task 3
- **Issue:** `ci.rs` allowed only cargo argv steps; steps 11 and 12 run `.venv/bin/python`.
- **Fix:** typed `Step { name, program: Cargo | VenvPython, args, env }`; the venv's Python is reached by its absolute
  path in the checkout.
- **Files modified:** xtask/src/ci.rs
- **Committed in:** 5080514

**5. [Rule 1 - Bug] Stand-in suite could not find the worker under ci**
- **Found during:** Task 3 (first full `cargo xtask ci`: 3 failures in standin python)
- **Issue:** the stand-in tests default to `target/release/ksci-worker`, which release builds no longer produce
  (they go to `target/dist-build`).
- **Fix:** `ci` sets `KSCI_WORKER` to `release::shipped_worker` (`target/dist/<host>/ksci-worker`) for the Python
  steps, so they test the shipped bytes. The developer default is unchanged.
- **Files modified:** xtask/src/ci.rs, xtask/src/release.rs
- **Verification:** second full `cargo xtask ci` exits 0.
- **Committed in:** 825b32a

**6. [Plan wording] publish-dist runs `release dist --twice`, not plain `dist`**
- **Issue:** plain `dist` records `"reproducible":false` (nothing was compared), so the published digests.json
  would have said false for bytes the ci job had checked.
- **Fix:** the publish job builds with `--twice`, so the published record proves itself. It costs one more pair of
  release builds on the VM. Plain `dist` is proven to give the same bytes (D1).
- **Committed in:** 5080514

**7. [Deferred items from 01-06, assigned to this plan by the orchestrator]**
- `check provenance` now compiles the R0 crates for `wasm32-unknown-unknown` with `+simd128` (was wasip2);
  `compile_inputs` takes explicit rustflags. Every-site check: rust-analyzer reported "No LSP server available",
  so the fallback was used: ast-grep (`$P::compile_inputs($$$)`, `R0_TARGET`, `heavy_job()`, `bin_dir()`) plus
  `cargo clippy --workspace --all-targets` (one caller of `compile_inputs`, in check.rs).
- Release builds are path-remapped (closed by Task 1).
- **Committed in:** b26741a, 0265579

**Hardening beyond the plan text (Rule 2):** the builder-path search also looks for both source trees and
`$CARGO_HOME`, not only `$HOME`; the workflow checks out with `persist-credentials: false` and runs
`oras logout ghcr.io` in an `if: always()` step so the short-lived token does not stay in the runner user's
credential store.

---

**Total deviations:** 5 auto-fixed (3 Rule 1, 2 Rule 3), 1 plan-wording change, 1 pair of assigned deferred items.
**Impact on plan:** each fix was needed for reproducibility or for `cargo xtask ci` to run at all. No scope creep
beyond the two deferred items the orchestrator assigned.

## Issues Encountered

- rust-analyzer: "No LSP server available for file type: .rs" for the whole session; the CLAUDE.md fallback was used.
- The `dist --twice` clean-tree check means a tracked edit made while `cargo xtask ci` runs fails the run; the
  deferred-items edit was committed before the dist step reached it.

## User Setup Required

None for this plan. Plan 01-14 registers the VM runner (labels `self-hosted, linux, x64, klein-science`), restricts
its runner group to this repository, enables "Require approval for all outside collaborators", and checks
`gh workflow view ci`.

## Next Phase Readiness

- Ready for plan 01-14 (VM runner) and 01-16 (signing, cross-host digest check D-07). The VM's first run tests A6
  for x86_64-unknown-linux-gnu; if its two workers differ, the cause belongs in `native_link_flags`.
- The published record prints `DIGESTS_JSON_SHA256=<hex>` for 01-16.

---
*Phase: 01-contracts-oracles-and-risk-probes*
*Completed: 2026-10-11*

## Self-Check: PASSED

All 6 claimed files exist; all 8 task commits (b26741a, 6e8f06b, 198953a, a85e700, 99dbcf3, 5080514, 0265579, 825b32a) are on this branch above 229fc4c.
