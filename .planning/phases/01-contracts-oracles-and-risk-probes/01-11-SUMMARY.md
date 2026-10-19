---
phase: 01-contracts-oracles-and-risk-probes
plan: 11
subsystem: infra
tags: [cubecl, metal, wgpu-msl, metal-native, montgomery, fixed-box, jcs, xtask, benchmark]

requires:
  - phase: 01-02
    provides: "xtask host module: heavy_job, klein_rs_build_running, pinned cargo/rustc/git"
  - phase: 01-06
    provides: "ksci-wire to_jcs and sha256_hex used for box.json and the probe report"
provides:
  - "cargo xtask bench box-manifest --box <label>: ksci.box.v1 fixed-box manifest with admission (CONF-06)"
  - "cargo xtask bench probe metal --box <label> [--quick | --watchdog [--max-loops N] | --full]"
  - "ksci-metal-probe (unshipped, macOS only): mulhi_u32, mulhi_u32_limb16, mul_u64_wrapping, mont_mul_u32, fast-math probes, CPU Montgomery baselines, watchdog harness (ACCEL-01)"
  - "bench::admit, the one fixed-box admission rule later benchmarks reuse"
affects: [01-15, ratification of per-box budgets, R2 GPU kernels, ks-ntt]

actuals:
  tokens: 23300   # chars/4 over the realized diff excluding Cargo.lock (93,169 chars); Cargo.lock adds 409 locked packages
  tasks: 2        # of 3; Task 3 is the operator checkpoint
  commits: 2      # MEASURED: git rev-list --count plan_head_before..HEAD at SUMMARY write
plan_head_before: 229fc4c86235b54a2c41b5e004faa51bdf6383c5
plan_head_after: 858151f1d70a14b964b8937282e9956df3c9bf75

tech-stack:
  added: ["cubecl =0.11.0 (default features off; probe enables metal and metal-native)"]
  patterns:
    - "Every benchmark admits a fixed-box manifest first and embeds the SHA-256 of box.json in its report"
    - "Exactness before timing: a kernel whose output differs from its CPU reference exits 1 and no time is reported"
    - "CubeCL runtime config pinned before any device exists (no caches, no records, scratch environment root checked empty)"
    - "Risky GPU trials run in fresh subprocesses with a wall timeout"

key-files:
  created:
    - xtask/src/bench.rs
    - probes/metal/Cargo.toml
    - probes/metal/src/main.rs
    - probes/metal/src/kernels.rs
  modified:
    - Cargo.toml
    - Cargo.lock
    - xtask/src/main.rs
    - xtask/src/host.rs

key-decisions:
  - "CubeCL persistence is compiled in transitively (cubecl-wgpu -> default-featured cubecl-cpp -> cubecl-runtime/default); the probe pins CubeCL's runtime config and fails on any write under its scratch environment root instead"
  - "One probe binary links both Metal paths; --path wgpu-msl|metal-native picks the CubeCL Device at run time"
  - "Probe built with a [profile.probe] (release, no fat LTO, codegen-units 16; probe crate itself 1 unit)"
  - "Load admission compares the decimal text exactly (no float): 2.0 admitted, 2.01 refused"
  - "pmset 'No CPU power status has been recorded' reads as CPU speed limit 100"
  - "Times are integer nanoseconds (cold_ns, warm_ns median/min/max/p10/p90) so the JCS report holds no floats"

requirements-completed: []  # ACCEL-01 and CONF-06 complete only after the Task 3 operator session

coverage:
  - id: D1
    description: "Fixed-box admission (CONF-06): battery, klein-rs build, load 2.0/2.01, thermal limit, missing/empty field refused; identical readings give identical JCS bytes; no serial/UUID"
    requirement: CONF-06
    verification:
      - kind: unit
        ref: "xtask/src/bench.rs#box_manifest_* (10 tests)"
        status: pass
    human_judgment: false
  - id: D2
    description: "Four integer kernels bit-exact on both Metal paths before timing; fast-math default measured per path; CubeCL wrote nothing"
    requirement: ACCEL-01
    verification:
      - kind: other
        ref: "target/probe/ksci-metal-probe --path wgpu-msl and --path metal-native (smoke, unadmitted)"
        status: pass
    human_judgment: true
    rationale: "The admitted report comes from the Task 3 operator session; Tasks 1-2 ran under parallel-executor load where admission refuses"
  - id: D3
    description: "Watchdog limit, bracketed by a passing and a failing trial"
    requirement: ACCEL-01
    verification: []
    human_judgment: true
    rationale: "Search to failure needs the operator at the Mac (Task 3, gate blocking-human)"

duration: 62min (Tasks 1-2)
completed: 2026-10-12
status: halted
---

# Phase 1 Plan 11: Metal probe and fixed-box manifest Summary

**`cargo xtask bench` with a JCS `ksci.box.v1` fixed-box manifest and exact-decimal admission, plus `ksci-metal-probe` on CubeCL 0.11.0: four integer kernels proven bit-exact on both Metal paths, measured fast-math defaults, CPU Montgomery baselines and a subprocess watchdog harness. HALTED at Task 3, the operator-present watchdog search that also produces the first admitted report.**

## Status: halted at Task 3 (checkpoint:human-action, gate blocking-human)

Tasks 1 and 2 are committed. Task 3 is the operator session: `cargo xtask bench probe metal --box mac-m3max --full`,
run on AC power with the 1-minute load at or below 2.0. That run writes the first admitted `box.json` and
`probe-metal.json`; Claude then verifies and commits them and completes this SUMMARY with the measured numbers.

## Performance

- **Duration:** about 62 min (Tasks 1-2)
- **Started:** about 2026-10-11T22:05Z
- **Paused at checkpoint:** 2026-10-11T23:07Z
- **Tasks:** 2 of 3
- **Files modified:** 8 (4 created)

## Accomplishments

- **Fixed-box manifest (CONF-06).** `bench box-manifest` reads `sysctl` (hw.model, brand string, perflevel0/1 core
  counts, memsize, vm.loadavg), `system_profiler SPDisplaysDataType -json` (GPU name and core count only; that report
  also carries the display serial number, which is never read), `sw_vers`, `pmset -g batt` and `-g therm`, the pinned
  `rustc -vV`, `git rev-parse HEAD` and `host::klein_rs_build_running()`. Fixed system tools run by absolute path.
  `admit()` is pure and refuses: not on AC power, a klein-rs build, 1-minute load above 2.0, CPU speed limit below 100,
  or any missing or empty reading. The load compares the decimal text exactly, so `2.0`/`2.00` are admitted and `2.01`
  is refused. The manifest is JCS with `qos_class: "user-interactive"` and no lists.
- **The probe (ACCEL-01).** `mulhi_u32` (CubeCL `mul_hi`, which Metal lowers through a 64-bit multiply),
  `mulhi_u32_limb16` (16-bit limbs, 32-bit operations only), `mul_u64_wrapping` and `mont_mul_u32` (mod 2013265921,
  R = 2^32, `-p^-1` by Newton iteration) over 2^24 xorshift64* inputs. Each output is compared word for word with a
  CPU reference before any timing; the Montgomery reference is u128 arithmetic, independent of the word algorithm.
  One cold dispatch (compile included) and 30 warm dispatches give median, min, max, p10 and p90.
- **Fast-math detection per path.** `a*b+c` against the CPU's unfused IEEE result and `f32::mul_add` over 4,098
  discriminating lanes; `(a+b)+c` against `a+(b+c)` and `(a+b)-a` against `b`. Verdicts: `fma_contraction:
  fused|unfused|mixed`, `reassociation: observed|not-observed`, with lane counts.
- **CPU baselines.** The kernel's Montgomery algorithm on 1 thread and on the manifest's P-core count (10), each
  thread at QoS user-interactive via `pthread_set_qos_class_self_np`; the report records that macOS cannot hard-pin
  a P-core (assumption A9).
- **Watchdog harness.** `--watchdog-trial <loops>` in a fresh subprocess per trial, 120 s wall timeout, 2^18
  elements, a warm-up dispatch, then one timed dispatch of `loops` Montgomery iterations per element, its first 64
  outputs checked against the CPU loop. The search doubles from 2^10 until a trial fails, then binary-searches to
  within 5%; the report holds `limit_loops`, `limit_kernel_ns`, the passing and failing trials, `failure_mode` and
  every trial. `--max-loops` caps the doubling.
- **Report.** `probe-metal.json` (JCS, schema `ksci.probe.metal.v1`): `box_manifest_sha256`, `cubecl_version` (read
  from Cargo.lock), `mode`, `path-wgpu-msl`, `path-metal-native`, `cpu_baselines`, `watchdog`.

## Findings (smoke runs, not measurements)

These ran with the 1-minute load at 4.5 to 11.9 (three parallel executors and a Docker VM at about 700% CPU).
They are correctness and plumbing results. No throughput figure from them is a baseline (CLAUDE.md rule 5).

| Check | path-wgpu-msl | path-metal-native |
|---|---|---|
| mulhi_u32, mulhi_u32_limb16, mul_u64_wrapping, mont_mul_u32 bit-exact | all 4 | all 4 |
| FMA contraction (4,098 discriminating lanes) | fused (4,098/4,098) | fused (4,098/4,098) |
| `(a+b)+c` kept in written order (4 lanes) | yes, 4/4 | yes, 4/4 |
| `(a+b)-a` simplified to `b` (2,331 lanes) | yes, 2,331/2,331, so reassociation observed | no, 0/2,331, so not observed |
| CubeCL writes under its environment root | none | none |

- **FMA is contracted on both paths**, including metal-native, which sets `MTLMathMode::Safe`. f32 results on
  either Metal path are not IEEE-unfused, which bears on any float kernel and on rule 4's "floats tagged approximate".
- **Watchdog harness self-test (plumbing check, plan Task 2 step 4):** `--watchdog --max-loops 65536` on wgpu-msl ran
  7 trials (1,024 to 65,536 loops). All passed, so `limit_loops: null` and `reached_cap: true`. The 65,536-loop
  trial took about 84 ms of kernel time, so the Task 3 search will double well past the cap before any watchdog.
  The run went through the probe binary directly and through `cargo xtask bench probe metal --watchdog --max-loops 65536`
  under a throwaway label (see deviation 7).

## Admission RED witnesses

- **Live refusals:** `cargo xtask bench box-manifest --box mac-m3max` gave "refused to record: 1-minute load average
  is 4.48, above 2.0". `cargo xtask bench probe metal --box mac-m3max --quick` built, then refused at 11.86, and
  later at 8.39. Each exited 1 and wrote nothing (`conformance/boxes/mac-m3max/` stayed empty).
- **Unit tests seen RED:** with the load rule admitting any `2.x`, the power check removed, the klein-rs check
  disabled and the thermal floor at 1, these four failed: `box_manifest_load_two_admitted_above_refused`,
  `_refuses_battery`, `_refuses_klein_rs_build` and `_refuses_thermal_limit`. With empty text accepted,
  `box_manifest_refuses_missing_or_empty_field` failed. All pass on the committed code (10/10).
- **Exactness gate seen RED:** dropping the `mid >> 16` carry from `mulhi_limb16` gave
  `mulhi_u32_limb16: NOT EXACT at index 0: device 1205772, CPU reference 1205773`, exit 1, and no timing printed.
- **Persistence check seen RED:** with CubeCL's autotune, throughput and compilation caches and its records turned
  back on, both paths exited 1 with "CubeCL wrote under its pinned environment root ...: default.db,
  default.db-tshm, default.db-wal". With the pinned config the list is empty and nothing appears in
  `target/environment` or `~/Library/Caches/cubecl`.

## Task Commits

1. **Task 1: tracer (box manifest, admission, mulhi_u32 exact then timed, report with manifest digest)**: `26d99ad` (feat)
2. **Task 2: four kernels, fast-math detection, CPU baselines, watchdog harness**: `858151f` (feat)
3. **Task 3: operator-present watchdog search and full session**: pending (checkpoint)

## Files Created/Modified

- `xtask/src/bench.rs`: `BenchCmd` (box-manifest, probe metal), `Readings`, `admit`, `Refusal`, `manifest_bytes`,
  the parsers, report assembly, and 11 tests (10 `box_manifest_*` plus `probe_scope_from_flags`).
- `probes/metal/src/kernels.rs`: six `#[cube]` kernels plus the `mulhi_limb16` and `mont_mul` helpers, `P`, `NEG_P_INV`.
- `probes/metal/src/main.rs`: probe modes, exactness checks, timing, fast math, CPU baselines, watchdog trial and
  search, the CubeCL config pin and empty-root check, 5 tests. Off macOS it is a stub that exits 2.
- `probes/metal/Cargo.toml`: `ksci-metal-probe`, features `path-wgpu-msl` (default) and `path-metal-native`; cubecl
  only under `cfg(target_os = "macos")`.
- `Cargo.toml`: member `probes/metal`, `cubecl = { version = "=0.11.0", default-features = false }`, `[profile.probe]`.
- `xtask/src/main.rs`: `Bench` subcommand. `xtask/src/host.rs`: `klein_rs_build_running` drops its dead-code `expect`.
- `Cargo.lock`: 409 packages added, none removed, no existing version changed. Optional CubeCL backends (cuda,
  hip, cpu/llvm, `tracel-llvm-bundler`) are locked but never built: `cargo tree --target all --all-features -i
  cubecl-cuda` prints nothing.

## Decisions Made

See `key-decisions` above. The probe crate is excluded from the shipped deny graph (deny.toml already listed it), and
`cargo deny check advisories bans licenses sources` passes.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] CubeCL `persistence` cannot be switched off through features**
- **Found during:** Task 1
- **Issue:** `cubecl-wgpu` depends on `cubecl-cpp` with default features, which enables `cubecl-core/default`, then
  `cubecl-runtime/default`, then `persistence` (turso-backed). Acceptance check `cargo tree -e features -p
  ksci-metal-probe | grep -c persistence` prints **2**, not 0, whatever the probe's own features are.
- **Fix:** before any device exists, the probe pins `CubeClRuntimeConfig`: autotune, throughput and compilation
  caches off, records `Off`, and the environment root set to a fresh scratch directory. That also shuts out any
  `cubecl.toml` or `CUBECL_*` variable. When the probe ends it lists that root and exits 1 if anything is there. The
  report carries `cubecl_persistence_writes: []` per path. RED witness above.
- **Files modified:** probes/metal/src/main.rs
- **Committed in:** 26d99ad

**2. [Acceptance check wording] `grep -c '"=0.11.0"' Cargo.toml` prints 2.** The existing pin `sha2 = "=0.11.0"`
matches the same pattern. Exactly one cubecl line exists (`cubecl = { version = "=0.11.0", ... }`).

**3. [Rule 3 - Blocking] Build profile for the probe.** The shipped release profile (fat LTO, one codegen unit)
relinks CubeCL, wgpu and naga for minutes on every probe change. Root `[profile.probe]` inherits release with
`lto = false` and `codegen-units = 16`. The probe crate keeps one codegen unit, since it holds the timed CPU loop.
Commit 26d99ad.

**4. [Design] One binary, both paths.** The plan has xtask build the probe per feature path. Instead xtask builds once
with `path-wgpu-msl,path-metal-native`, and `--path` selects `Device::metal_msl(..)` or `Device::metal(..)` at run time
through CubeCL 0.11's runtime-erased `Client`. Both features stay as named, and each path still runs in its own
process.

**5. [Rule 3] `clippy::disallowed_methods` on `Instant::now`.** clippy.toml's DET-08 list applies to every crate. The
timing probe allows it on the macOS module only, with a reason. Its times are measurements, never results.

**6. [Schema] Integer nanoseconds.** Times are `cold_ns` and `warm_ns{median,min,max,p10,p90}`, plus an integer
`ops_per_s`, instead of `cold_ms` floats, so the JCS report has no floating-point formatting.

**7. [Environment] Admitted runs blocked by parallel load; plumbing checked under a throwaway label.**
- Throughout Tasks 1-2 the 1-minute load was 4.5 to 11.9: three parallel executors plus the Docker VM. Admission
  correctly refused every `mac-m3max` run.
- Plumbing check: `admit`'s load arm was disabled locally, never committed, and
  `cargo xtask bench probe metal --box plumbing-selftest --watchdog --max-loops 65536` was run once.
- The full xtask path worked end to end:
  - build, readings, `box.json`, both paths, CPU baselines on 10 threads, and the 7-trial capped watchdog;
  - JCS report with `box_manifest_sha256` equal to `shasum -a 256 box.json`
    (`234a3082...95`, recorded at load 7.22);
  - all keys present; all kernels `exact: true` on both paths.
- The edit was reverted and `conformance/boxes/plumbing-selftest/` deleted. Neither the files nor their numbers are
  measurements.
- The committed admission then refused again (load 8.39).
- Consequence: `conformance/boxes/mac-m3max/box.json` and `probe-metal.json` are not committed yet. The plan's
  Task 2 report commit, and Task 1's admitted quick run, move into the Task 3 operator session. That session
  writes both files under admission (`--full` covers the standard run).

**8. [Interpretation] Thermal reading.** On this Mac `pmset -g therm` prints "No CPU power status has been recorded"
and no `CPU_Speed_Limit` line. That text is read as 100. Any other text without the line is a missing reading, which is refused.

**9. [Unspecified in plan] Watchdog trial size.** 2^18 elements: enough threads to fill 30 GPU cores, so the loop
count sets the dispatch time. 64 elements per trial are checked against the CPU loop.

---

**Total deviations:** 9. Three were Rule 3 auto-fixes (persistence pin, build profile, clippy allow). Two were
acceptance-check wording (persistence count, the `=0.11.0` grep). One was environment-blocked (admitted runs moved to
Task 3), and three were design or schema choices.
**Impact on plan:** no scope creep. The plan's stated persistence check cannot be met through features; the runtime
pin is checked by a test that was seen RED. Moving the admitted report to Task 3 changes when it is produced, not
what it contains.

## Code oracles

rust-analyzer still answered "No LSP server available for file type: .rs", so the CLAUDE.md fallback was used.
- `cargo clippy` (default features, and with `path-metal-native`) on the probe and xtask passes with `-D warnings`.
- `cargo check -p ksci-metal-probe --target wasm32-unknown-unknown` compiles the non-macOS stub with no dependencies.
- `cargo xtask check provenance` reads 29 sources, the probe's two files among them, and finds 0 violations.
- `cargo deny check` passes.
- ast-grep finds one call site of `host::klein_rs_build_running()`, at `xtask/src/bench.rs`, which is why its
  dead-code `expect` was removed.
- `cargo nextest run -p xtask -p ksci-metal-probe`: 112 passed.

## Issues Encountered

- Several commands failed under the worktree-isolation guard (multi-line Python heredocs, `sed` with a variable path).
  Each was rerun as a plain command or done with the Edit tool.

## User Setup Required

Task 3 needs the operator at the Mac on AC power. See the checkpoint.

## Next Phase Readiness

- Ready for Task 3: `cargo xtask bench probe metal --box mac-m3max --full`, run in this worktree's checkout so its
  `conformance/boxes/mac-m3max/` outputs land where they get committed.
- Plan 01-15 needs this plan's admitted `cpu_baselines` and watchdog numbers. Until Task 3 completes, this SUMMARY
  stays `status: halted`.

---
*Phase: 01-contracts-oracles-and-risk-probes*
*Paused at Task 3: 2026-10-12*

## Self-Check: PASSED
