# Roadmap: klein-science

## Overview

Single source for design and decisions: [research/SCIENCE-PLUGIN-CAS.md](research/SCIENCE-PLUGIN-CAS.md) (operator
decisions 2026-10-11). The operator rulings of 2026-10-11 in [PROJECT.md](PROJECT.md) Key Decisions refine it. The
research behind them is in [research/SUMMARY.md](research/SUMMARY.md).

Scheduling (operator 2026-10-11): this project is kept apart from Klein's core build. It starts once the operator schedules
it, after Klein's 04.05/04.06 closes (deadline 2026-10-21).

klein-science ships a licence-clean, Symbolica-clean Rust CAS with a SymPy-parity target as signed Klein plugins
(`science.cas`, `science.numerics`, `science.prover`). The roadmap follows the operator's milestones in horizontal layers,
because each upper CAS layer depends on the one below it. Phase 1 fixes the contracts and oracles and measures the
largest risks. Phase 2 builds the core expression system. Phase 3 adds the exact algebra layer and the algebraic `solve`
core. Phases 4 to 6 complete solve and simplify, then calculus, numerics and the Lean loop, then matrices and discrete
maths. v1 is phases 1–6 (M0–M5). M6, the parity long tail, is v2 and has no phase yet (see "Deferred: the M6 parity
long tail" below).

Every success criterion below is checked by an oracle: a test seen RED before it is trusted, a conformance number on
digest-pinned goldens, or a measurement on the ratified fixed box. Nothing is estimated. Until a klein-rs dependency
lands, its proxy is used and labelled: TIR pass rates run under the CPython `host_shim` (proxy until D4), R1 is the
`ks-conform` emulator (until D2), and Lean certificates are checked by a pinned offline Lean + Mathlib toolchain (until
D5). Every phase also has performance budgets (§4 of the single source), ratified per fixed box and measured, never
estimated. The `science.prover` plugin, with the Lean prover harness and skills, moves here from Klein once Klein's 04.1
Lean guest engine lands (Phase 5).

| Phase | Operator milestone | Passes when | Size [U] (SS §5) |
|---|---|---|---|
| 1 | M0 | baseline measured; klein-rs requests filed | 18 plans in 8 waves (widened by the research) |
| 2 | M1 | SymPy `core` tests | ~20k LOC |
| 3 | M2 | ≥ 60% of TIR; SymPy `polys`/`ntheory` tests | ~35k LOC |
| 4 | M3 | ≥ 90% of TIR | ~30k LOC |
| 5 | M4 | Lean loop working | ~45k LOC |
| 6 | M5 | ≥ 97% of TIR | ~30k LOC |

## Phases

**Phase Numbering:**
- Integer phases (1, 2, 3): Planned milestone work
- Decimal phases (2.1, 2.2): Urgent insertions (marked with INSERTED)

Decimal phases appear between their surrounding integers in numeric order.

- [ ] **Phase 1: Contracts, Oracles and Risk Probes** - M0: wire identity, signed reproducible build, pinned SymPy oracle, determinism and licence guards, Metal probe, klein-rs requests
- [ ] **Phase 2: Core Expressions** - M1: hash-consed DAG, numbers, functions, `is_*` assumptions, printing and parsing, `evalf`; SymPy `core` tests
- [ ] **Phase 3: Polynomials, Number Theory and Algebraic Solve** - M2: domains, factor, Gröbner, `ks-dmat`, primes, algebraic `solve`, first R2 admissions; ≥ 60% of TIR
- [ ] **Phase 4: Solve, Simplify, Assumptions and Piecewise** - M3: full `solve`, ported `simplify`, `ask`/`refine`, sets as results, solve certificates; ≥ 90% of TIR
- [ ] **Phase 5: Calculus, Numerics and the Lean Loop** - M4: limits, series, integrate, sums, ODEs, `science.numerics` with `lambdify`, `science.prover`; Lean loop working
- [ ] **Phase 6: Matrices, Sets, Logic, Combinatorics and GPU Budgets** - M5: `Matrix`, discrete maths, Metal LA and NTT budgets; ≥ 97% of TIR

## Phase Details

### Phase 1: Contracts, Oracles and Risk Probes

**Goal**: Every later layer has a fixed wire contract, a pinned SymPy oracle, and mechanical determinism and licence
guards to test against. The largest unknowns (Monty protocols, Metal under Seatbelt, CubeCL integer widths, the
Wasmtime pin) are measured and filed as klein-rs requests.
**Depends on**: Nothing (first phase)
**Requirements**: DET-01, DET-02, DET-03, DET-04, DET-08, CONF-01, CONF-02, CONF-03, CONF-06, LIC-01, LIC-02, DEP-01, DEP-02, DEP-03, PLUG-01, PLUG-02, GUEST-01, GUEST-02, ACCEL-01
**Success Criteria** (what must be TRUE):
  1. The same expression built in two different orders gets the same identity (SHA-256 over the JCS of the canonical
     DAG node table). Every numeric leaf on the wire is a tagged string, special values are named atoms, provenance
     travels in an envelope outside the hashed bytes, and the RFC 8785 test vectors pass.
  2. CI builds the zero-import WASM component (D-16) and the native worker reproducibly (two clean builds give the same
     digests). It emits the `klein.plugin.v1` manifest and a raw detached Ed25519 signature that passes `verify_strict`,
     and one trivial operation through the WIT tool returns byte-identical JCS from R0 and R2. The signature check and
     the R0-versus-R2 compare were each seen RED on a deliberate perturbation.
  3. Each guard was seen RED on a deliberate violation before it is trusted. A too-small logical step budget yields
     `fuel_exhausted` with the same bytes on R0 and R2, while Wasmtime fuel or memory-cap exhaustion yields a host
     error. `cargo-deny` rejects `rug` and the other banned crates. Lints reject unseeded hash-map iteration, ordering
     by interning id and unseeded randomness in maths crates. A ported module without a provenance header fails CI, and
     `NOTICE` carries the upstream notices.
  4. The digest-pinned SymPy oracle image (CPython 3.13, SymPy 1.14.0, mpmath 1.3.0, no network) produces the same
     golden bytes for a TIR slice on two runs and under several hash seeds. Golden bytes sit in the artifact store by
     digest, and git holds only recipes, manifests and scores. The three-level runner records the M0 baseline for the
     stand-in skeleton under the CPython `host_shim` (proxy). There, all five import forms succeed with no tool call, a
     missing name raises SymPy's exact `ImportError` text, and the Monty-subset lint passes.
  5. The CubeCL probe reports the M3 Max Metal GPU's measured u32 multiply-high and u64 throughput, fast-math default
     and watchdog limit, each under a fixed-box manifest. The operator has ratified per-box budgets (Mac: all P-cores
     against one P-core, Metal against that). Three requests are filed against klein-rs with the operator's approval:
     the TIR protocol census (widened Monty, D4), the measured minimum Seatbelt rules for Metal (input to D1), and the
     Wasmtime ≥ 49.0.2 request (D6).

**Klein-core dependencies**: this phase files D1, D4 and D6. Nothing here is blocked.
- D4 (Monty): the stand-in runs under the CPython `host_shim`, and the baseline is labelled proxy.
- D3 (`PluginArchive`): CI verifies the signature and digest itself. Installing the bundle into Klein waits for D3.
- D1 (`ComputeDevice`): the probe runs natively outside `ProcessSandbox`. Its Seatbelt measurement is the D1 input.
- D6 (Wasmtime): CI runs R0 under Klein's current pin and under ≥ 49.0.2. Builds use `+simd128`, never `+relaxed-simd`.

**Research flag**: needed.
- Seatbelt plus Metal minimum rules (no authoritative Apple source).
- CubeCL integer widths and u64 throughput on the M3 Max.
- egglog 3.0 on `wasm32-wasip2` and its single-thread determinism.
- Monty host-call latency and bigints.
- Also measure, on the oracle, the share of TIR definite integrals that SymPy routes through `meijerint`. It decides
  whether Meijer G leaves the v2 tail.

**Plans**: 4/18 plans executed in 8 waves

Plans:
**Wave 1**
- [x] 01-01-PLAN.md — ksci-wire: canonical node table, string numerals, JCS, SHA-256 identity, envelopes (wave 1)

**Wave 2** *(blocked on Wave 1 completion)*
- [x] 01-02-PLAN.md — xtask spine: recipe fetch, artifact-store client, pinned host tools, heavy-job guard, RFC 8785 vectors, no-corpus (wave 2)

**Wave 3** *(blocked on Wave 2 completion)*
- [x] 01-03-PLAN.md — determinism and licence guards: clippy, cargo-deny, provenance headers and NOTICE, each seen RED (wave 3)
- [x] 01-04-PLAN.md — ksci-ops and the R2 worker: `science.cas.canonicalize`, logical step budget, stdio frames (wave 3)
- [ ] 01-05-PLAN.md — digest-pinned SymPy oracle image, hardened program runner, introspection table (wave 3)

**Wave 4** *(blocked on Wave 3 completion)*
- [ ] 01-06-PLAN.md — WIT tool and zero-import R0 component; R0 (Wasmtime 46.0.1 and 49.0.2) == R2; fuel and memory host errors (wave 4)
- [ ] 01-07-PLAN.md — TIR corpus by recipe, extraction, stratified hash-ranked ~1k golden slice (wave 4)
- [ ] 01-08-PLAN.md — Python JCS mirror, `_tool` and `host_shim`; three-way differential with rfc8785 (wave 4)

**Wave 5** *(blocked on Wave 4 completion)*
- [ ] 01-09-PLAN.md — reproducible dist (two clean builds), zero-import and relaxed-SIMD gates, VM CI workflow (wave 5)
- [ ] 01-10-PLAN.md — reproducible goldens: 4 hash seeds x 2 runs x 2 platforms, adjudication queue (wave 5)
- [ ] 01-11-PLAN.md — fixed-box manifest and CubeCL Metal probe (mulhi, u64, Montgomery, fast-math, watchdog) (wave 5)
- [ ] 01-12-PLAN.md — stand-in import surface (902 names, sympy.abc) and Monty-subset lint (wave 5)

**Wave 6** *(blocked on Wave 5 completion)*
- [ ] 01-13-PLAN.md — three-level runner and the M0 baseline (proxy) (wave 6)
- [ ] 01-14-PLAN.md — go-live: VM runner, green CI, private GHCR publish and round trip (wave 6)
- [ ] 01-15-PLAN.md — Seatbelt minimum rules by ablation, D1 draft, per-box budget ratification (wave 6)

**Wave 7** *(blocked on Wave 6 completion)*
- [ ] 01-16-PLAN.md — signed klein.plugin.v1 bundle: capability manifests, Mac Keychain signing, verify_strict (wave 7)
- [ ] 01-17-PLAN.md — TIR and OMR protocol census (dynamic and static), meijerint share, D4 draft (wave 7)

**Wave 8** *(blocked on Wave 7 completion)*
- [ ] 01-18-PLAN.md — D6 draft; operator files D1, D4 and D6; ratification recorded (wave 8)

### Phase 2: Core Expressions

**Goal**: An LLM's SymPy program that builds expressions, evaluates numbers and functions, substitutes, expands,
differentiates and prints gets SymPy's exact output from the stand-in. The same exact core results are byte-identical
on R0, R1 and R2.
**Depends on**: Phase 1
**Requirements**: CORE-01, CORE-02, CORE-03, CORE-04, CORE-05, CORE-06, CORE-07, CORE-08, CORE-09, PRINT-01, PRINT-02, PRINT-03, GUEST-03, GUEST-04, GUEST-05, GUEST-06, GUEST-07, DET-05, DET-07, GATE-01
**Success Criteria** (what must be TRUE):
  1. SymPy 1.14.0's `core` test suite passes against the stand-in under CPython. The stand-in mirrors the written
     auto-evaluation spec (`Add`/`Mul`/`Pow` flatten rules, function `eval` rules, per-symbol three-valued `is_*`
     assumptions), and a differential test against the Rust core was seen RED on an injected rule difference.
  2. On digest-pinned goldens, these programs print byte-identically to SymPy's `str()`/`repr`, term order included:
     symbols with assumptions, `Function`/`Dummy`/`Wild`, exact numbers and constants, elementary and combinatorial
     functions, relationals, `subs`, `expand`, `diff`/`Derivative` and `Lambda`. Floats print 15 significant digits (or
     `n` on request), `N`/`evalf` match SymPy's digits at arbitrary precision, and `sympify`/`S("…")`/`parse_expr`
     build SymPy's trees without `eval`.
  3. Python interop matches SymPy on goldens:
     - operators and reflected forms against Python numbers (`1/2*x` prints `0.5*x`);
     - comparisons build Relationals, and truth-testing one raises SymPy's `TypeError` text;
     - `int()`/`float()`/f-strings convert as SymPy does;
     - the common methods return SymPy's results;
     - failures surface SymPy's exception class and message.
  4. Fuel exhaustion surfaces as the same Python exception on every replay. A CI job runs the same exact core
     operations, multiprecision `evalf` included, on R0, R1 (emulated) and R2 at several thread counts, with different
     interning orders and hash seeds. It fails on any byte difference and was seen RED on an injected order dependence
     before it is trusted.

**Klein-core dependencies**:
- D4 (Monty): SymPy's `core` tests run under CPython by design, because they subclass SymPy types. Every TIR pass rate
  is proxy until D4.
- D2 (R1 map): R1 is the `ks-conform` emulator (N R0 instances, merged in index order) until the kernel map lands.
- D6 (Wasmtime): the determinism job runs R0 under Klein's pinned Wasmtime, and under both pins until D6 lands.
- D1 is not needed. R2 here is CPU (rayon).

**Research flag**: standard patterns: a direct port of SymPy `core`, its printers and mpmath 1.3.0. Skip
research-phase.
**Note**: the `.simplify` method is wired here. Its output parity is gated by the ported pipeline in Phase 4.

**Plans**: TBD

### Phase 3: Polynomials, Number Theory and Algebraic Solve

**Goal**: LLM programs get SymPy's answers through the exact algebra layer when they factor, divide, take gcds, find
roots, solve linear systems and univariate polynomial equations, or ask number-theory questions. At least 60% of TIR
programs pass.
**Depends on**: Phase 2
**Requirements**: POLY-01, POLY-02, POLY-03, POLY-04, POLY-05, POLY-06, NT-01, NT-02, NT-03, SOLVE-01, SOLVE-02, SOLVE-04, CERT-03, CERT-04, ACCEL-02, ACCEL-03, CONF-04, GATE-02
**Success Criteria** (what must be TRUE):
  1. At least 60% of the digest-pinned TIR programs pass on the three-level scorer (proxy until D4), and SymPy's `polys`
     and `ntheory` test suites pass against the stand-in. Every case where SymPy is judged wrong is in the adjudication
     list, with our answer and evidence from an offline independent oracle (Giac/FLINT/SymEngine) or a Lean proof.
  2. Domains ZZ, QQ, GF(p) and algebraic extensions round-trip through one conversion boundary. These match SymPy's
     goldens:
     - `Poly`, `degree`, `.all_coeffs`, `div`, `gcd`/`lcm` and `discriminant`;
     - `factor` (univariate and multivariate over ZZ/QQ) and `roots`;
     - deterministic `isprime`, `factorint` (dict order included), `primefactors`, `divisors`, `divisor_count` and
       `totient`;
     - `primerange`, `nextprime`, `prime`, `mod_inverse` and `crt`/`solve_congruence`.
  3. `solve` returns SymPy's answers and return shapes (list, dict, list of dicts, `dict=True`) for linear systems and
     univariate polynomials through degree 4 with radicals, and it filters by symbol assumptions. Every call returns a
     `SolveResult` with `complete`, `certifiable` and status, and `complete` is true only for decision procedures.
  4. Gröbner bases (Buchberger, then F4) and resultants carry cofactors that exact multiplication confirms. `ks-dmat`
     gives the same solutions and reduced forms over ZZ, QQ and GF(p) through Bareiss and through multi-modular CRT. Lean
     checks the gcd, primality, factorisation and Pocklington/Pratt certificates, with `norm_num` and without
     `native_decide`.
  5. The `ks-accel` NTT, modular-matrix, CRT and F4-reduction kernels have a CPU reference that also compiles to WASM
     and uses one 31-bit prime table on every target. An operation class is admitted to R1/R2 only after R0, R1 and R2
     give byte-identical output on the conformance corpus, and each admission's witness was seen RED.

**Klein-core dependencies**:
- D4 (Monty): the ≥ 60% gate is measured under the `host_shim` and labelled proxy.
- D2 (R1 map): R1 admissions use the emulator. They are re-run on the kernel map when D2 lands.
- D1 (`ComputeDevice`): GPU R2 runs natively on the Mac outside `ProcessSandbox`. Inside Klein, R2 stays CPU-only
  (rayon) until D1 lands.
- D5 (`lean.certify`): certificates are checked by the pinned Lean + Mathlib toolchain run offline at test time
  (proxy).
- Certificates here are integer facts over ℕ/ℤ with no division or roots, so they cannot rely on junk values. The
  faithfulness check lands in Phase 4, before any `certifiable=true` solve result.

**Research flag**: standard patterns: Monagan–Pearce, Brown/Zippel, Zassenhaus, Bareiss, Buchberger and SymPy
`polys`. Research only if F4 goes beyond the papers.

**Plans**: TBD

### Phase 4: Solve, Simplify, Assumptions and Piecewise

**Goal**: LLM programs that solve transcendental equations, systems and inequalities, simplify, query assumptions and
use Piecewise and sets get SymPy's results, and at least 90% of TIR programs pass. Solve and identity results carry
Lean-checked certificates where they are certifiable.
**Depends on**: Phase 3
**Requirements**: SOLVE-03, SOLVE-05, SOLVE-06, SOLVE-07, SIMP-01, SIMP-02, SIMP-03, SIMP-04, SIMP-05, CONF-05, CERT-01, CERT-02, CERT-05, CERT-06, CERT-09, GATE-03
**Success Criteria** (what must be TRUE):
  1. At least 90% of the digest-pinned TIR programs pass (proxy until D4). Simplify- and integrate-shaped results are
     scored by semantic equivalence (derivative check, numeric zero test), and string match is reported as a secondary
     metric.
  2. `solve` handles rational-function equations, polynomial systems (through Gröbner) and transcendental equations
     (through the `_tsolve`/`_invert` heuristics, reporting `complete=false`). These match SymPy's goldens: `solveset`,
     `linsolve`, `nonlinsolve`, univariate and rational inequalities, and `nsolve` (the `findroot` port). Their results
     come back as `Piecewise`, `Interval`, `FiniteSet`, `Union`/`Intersection` and `And`/`Or`/`Not`.
  3. The default `simplify` (the ported SymPy pipeline), `collect`, `together`, `apart`, `nsimplify` and `trigsimp` give
     SymPy's output on the conformance corpus, and `ask`/`refine`/`Q` answer three-valued as SymPy does. The bounded
     egglog (or `egg`) strategy loads only as opt-in bundle data, step-limited and never time-limited.
  4. Lean checks these certificates:
     - solution substitution (`norm_num`/`ring_nf`);
     - identity (`ring`, or `field_simp; ring`);
     - completeness (full factorisation);
     - ideal membership (`linear_combination (exp := n)`).
     `certifiable` is set only after a faithfulness check. That check was seen RED rejecting statements that rely on
     `x/0 = 0` or on `Real.sqrt` of a negative.

**Klein-core dependencies**:
- D4 (Monty): the ≥ 90% gate is proxy until D4.
- D4 component kinds (Data, Klein 04.05 plan 49): loading the simplify strategy as a Data component needs them. Until
  then the strategy ships as digest-pinned files inside `science.cas`.
- D5 (`lean.certify`): certificates are checked by the pinned offline Lean toolchain (proxy).
- Operator ruling needed before the egglog strategy lands: egglog's `im-rc` (MPL-2.0) question. The fallback is
  `egg` 0.11.0.

**Research flag**: needed.
- The egglog strategy design, and the unmeasured share of rows where it diverges from SymPy's form.
- Transcendental solve heuristics.
- The Lean faithfulness check. It moves here from Phase 5, because the first `certifiable=true` results land in this
  phase.

**Plans**: TBD

### Phase 5: Calculus, Numerics and the Lean Loop

**Goal**: LLM programs that take limits, series, integrals, sums and simple ODEs and recurrences get SymPy's results.
`science.numerics` ships quadrature, ODE solvers and `lambdify`, and the CAS-to-Lean loop runs end to end through
`lean.certify`.
**Depends on**: Phase 4
**Requirements**: CALC-01, CALC-02, CALC-03, CALC-04, CALC-05, NUM-01, NUM-02, DET-06, PLUG-03, PLUG-04, CERT-08, PROVER-01, PROVER-02, PROVER-03
**Success Criteria** (what must be TRUE):
  1. These match SymPy on the conformance corpus, scored semantically with string match as secondary:
     - `limit` (the Gruntz port);
     - `series`/`.removeO`;
     - `integrate` (table, then manualintegrate, then heurisch, then transcendental Risch), with definite integrals
       marked heuristic;
     - `summation`/`Sum` with Gosper;
     - `dsolve`/`rsolve` on SymPy's simple classes.
  2. Every indefinite integral returns an antiderivative certificate (F′ = f with domain side conditions) that Lean
     checks.
  3. `science.numerics` quadrature and ODE solvers (mpmath ports) match mpmath 1.3.0's digits on digest-pinned goldens.
     `science.cas`, `science.numerics` and `science.prover` (Lean harness and skills) are built from one workspace as
     three signed bundles that pass `verify_strict`. The bundles share the mpmath port, and `evalf` lives in
     `science.cas`.
  4. `lambdify` compiles to one tape IR, run by a CPU interpreter (f64) and by one Metal GPU tape kernel (f32). Its
     results are tagged `numeric_class: approximate` with backend, device and driver, and audit replay serves the
     recorded value byte for byte.
  5. Inside Klein, a Lean goal obtains a witness through `kleinCall "science.cas.*"`, Mathlib's `polyrith` closes goals
     with Klein's CAS as backend, and a CAS certificate passes `lean.certify` end to end (the "Lean loop working"
     gate).

**Klein-core dependencies**:
- D5 (`lean.certify` and the Lean guest engine, Klein 04.1) is a **hard blocker**. Criterion 5 and the Lean-loop gate
  cannot pass on a proxy. Until Klein 04.1 lands, work proceeds against the pinned offline Lean toolchain calling the
  native worker directly, labelled proxy.
- D4 component kinds (Skill/Harness): `science.prover` packages the harness and skills as these kinds. The prover moves
  here from Klein once Klein's 04.1 Lean guest engine lands.
- D3 (`PluginArchive`): installing the three bundles into Klein by digest needs `PluginArchive` and lock v3. Until then
  CI verifies their signatures and digests.
- D1 (`ComputeDevice`): the Metal tape kernel runs natively outside `ProcessSandbox`. Inside Klein, `lambdify` uses the
  CPU interpreter until D1 lands.
- D6 (Wasmtime): R0 float paths want NaN canonicalisation. Floats are tagged approximate and replayed from the record,
  so a replay never depends on it.

**Research flag**: needed.
- The share of TIR definite integrals routed through `meijerint` (from the Phase 1 measurement).
- The Risch port.
- Lean certificate templates against pinned Mathlib.
- `kleinCall`/`polyrith` integration.

**Plans**: TBD

### Phase 6: Matrices, Sets, Logic, Combinatorics and GPU Budgets

**Goal**: LLM programs using `Matrix`, sets, logic and combinatorics get SymPy's results, and at least 97% of TIR
programs pass. GPU linear-algebra and NTT budgets are measured on the Mac's Metal GPU.
**Depends on**: Phase 5 (the ≥ 97% gate counts every earlier layer; `Matrix` builds on Phase 3's `ks-dmat`)
**Requirements**: MAT-01, DISC-01, CERT-07, ACCEL-04, GATE-04
**Success Criteria** (what must be TRUE):
  1. At least 97% of the digest-pinned TIR programs pass on the three-level scorer (proxy until D4).
  2. `Matrix` with `det`, `inv`, `rref`, `eigenvals`, `dot`, `cross`, `norm` and `.T` matches SymPy's goldens on top of
     `ks-dmat`. Inverse, solution and rank-witness results carry certificates that Lean checks.
  3. Sets, logic (`satisfiable`, `Implies`) and combinatorics (`Permutation`, `multiset_permutations`, `subsets`,
     `stirling`) match SymPy's goldens.
  4. GPU dense modular linear-algebra and NTT budgets are measured on the M3 Max's Metal GPU under the fixed-box
     manifest and compared against the ratified per-box budgets. Every GPU result is byte-identical to R0.

**Klein-core dependencies**:
- D1 (`ComputeDevice`): budgets are measured natively, and the manifest records whether `ProcessSandbox` was in force.
  They are re-measured inside the sandbox once D1 lands.
- D2 (R1 map): R1 admissions of matrix operation classes use the emulator until the kernel map lands.
- D4 (Monty): the ≥ 97% gate is proxy until D4.
- D5 (`lean.certify`): linear-algebra certificates go through `lean.certify`, which lands in Phase 5. If D5 slips, the
  offline proxy is used.

**Research flag**: the `Matrix`, sets, logic and combinatorics work is a standard port. The GPU part needs research:
F4 Macaulay reduction on the GPU (arXiv 2601.06765) and dense modular linear algebra under the Metal watchdog. CUDA and
ROCm conformance stays in v2, because the remote box is deferred.

**Plans**: TBD

## Deferred: the M6 parity long tail (no phase yet)

The work goes module by module against SymPy's test suite. It is multi-year and is planned one module at a time when the
operator opens the next cycle (REQUIREMENTS.md v2, TAIL-01 to TAIL-08):
- Meijer G integration (earlier if the Phase 1 `meijerint` measurement shows TIR needs it);
- PDEs, `sympy.stats`, `sympy.geometry`, physics, tensors and codegen;
- the LaTeX and pretty printers;
- SOS certificates (needs an operator-approved SDP solver) and WZ-pair certificates;
- CUDA/ROCm R2 lanes and f64 GPU budgets on the deferred remote GPU box;
- a narrow numpy/scipy facade over `science.numerics` (decided after Phase 4);
- a TypeScript `sympy` stand-in.

## Dependencies on Klein core (requests to klein-rs; approved in principle by the operator 2026-10-11)

1. **D1. `ComputeDevice` authority** (kernel D-R): `ProcessSandbox` may grant GPU device and driver access to a signed,
   locked native accelerator. This covers Linux CUDA/ROCm and macOS Metal. The Seatbelt rules are measured in Phase 1
   (DEP-02) and filed as this request's input.
2. **D2. Deterministic parallel WASM map** (kernel D-R): Klein runs N instances of a pure component on independent pieces
   and merges them in index order.
3. **D3. Plugin provisioning by digest:** a `PluginArchive` artifact kind; lock v3 pins the plugin.
4. **D4. Plugin component kinds and the Python guest (Monty), widened** (operator 2026-10-11):
   - the Skill/Harness/Data component kinds (Klein 04.05 plan 49);
   - Monty's protocol set, widened from "operator overloading" to the full set from the Phase 1 TIR census (DEP-01):
     arithmetic, reflected and unary operators; rich comparisons; `__bool__`; conversions including `__format__`;
     `__getitem__`/`__len__`; `__call__`; `@property`/`__getattr__`; an `isinstance` hook in place of inheritance;
     plugin module import; and bigints.

   Until it lands, the stand-in and every TIR pass rate run under the CPython `host_shim` and are labelled proxy.
5. **D5. `lean.certify`** and the Lean guest engine (Klein 04.1).
6. **D6. Wasmtime ≥ 49.0.2** (operator 2026-10-11, floor raised from 49.0.1 by D-17; filed in Phase 1 as DEP-03): move Klein's pin from 46.0.1 to ≥ 49.0.2.
   The current pin is affected by RUSTSEC-2026-0222/0223 and by the 0316 fuel bypass. The request also sets
   `relaxed_simd_deterministic(true)` and `cranelift_nan_canonicalization(true)` for science components and keeps
   `consume_fuel` on.

   Until it lands:
   - science components build with `+simd128` and never `+relaxed-simd`;
   - results never depend on Wasmtime fuel, because `fuel_exhausted` comes only from the logical step budget;
   - CI runs R0 under Klein's current pin as well.

What each request blocks, and the labelled proxy used until it lands:

| Request | Blocks | Proxy until it lands |
|---|---|---|
| D1 `ComputeDevice` | in-Klein GPU R2: Phase 3 admissions, Phase 5 Metal `lambdify`, Phase 6 budgets | native runs outside `ProcessSandbox`, labelled in the fixed-box manifest; inside Klein R2 is CPU-only (rayon) |
| D2 R1 map | real R1 lanes: the Phase 2 determinism job, Phase 3 and Phase 6 admissions | the `ks-conform` R1 emulator; admissions are re-run when D2 lands |
| D3 `PluginArchive` | installing the bundles into Klein by digest (Phase 1 onward; all three bundles in Phase 5) | CI verifies the signature (`verify_strict`) and digest itself |
| D4 component kinds + Monty | in-Klein stand-in execution and every TIR number (Phases 2–6); the Data kind for the simplify strategy (Phase 4); the Skill/Harness kinds for `science.prover` (Phase 5) | CPython `host_shim`, numbers labelled proxy; the strategy as digest-pinned files in `science.cas` |
| D5 `lean.certify` | **hard block** on the Phase 5 Lean-loop gate (`kleinCall`, `polyrith` in Klein); in-Klein certificate checks from Phase 3 | pinned offline Lean + Mathlib toolchain at test time (not accepted for the Phase 5 gate) |
| D6 Wasmtime ≥ 49.0.2 | trusting R0 under Klein's pin (advisories incl. the fuel bypass); R0 float NaN behaviour (Phase 5) | `+simd128` only; logical step budget; CI runs both pins |

## Infrastructure (operator 2026-10-11: local GPU first, remote GPU later)

- **Local first:** the operator's Mac is an Apple M3 Max with a 30-core GPU (Metal 4), 14 CPU cores and 36 GB of unified
  memory. Every GPU path (R2 via CubeCL's Metal/wgpu backend) is developed and measured there first.
  - **Consequence:** Metal has no f64. Exact work (Z, Q, Z/p, NTT, CRT) and f32 grids run on the GPU; f64 work falls back to
    the CPU (rayon).
  - **Consequence:** GPU benchmarks and conformance run on the Mac, unlike klein-rs, whose builds run only on the VM. Keep
    local builds to this workspace and watch memory pressure (the machine pages when klein-rs builds run alongside).
- **Remote GPU later:** a Nebius L40S (~$70–200/month) or H100 by the hour for CUDA and f64. Deferred; the estimate is in the
  single source §9. Nothing is created until the operator asks.

## Progress

**Execution Order:**
Phases execute in numeric order: 1 → 2 → 3 → 4 → 5 → 6

| Phase | Plans Complete | Status | Completed |
|-------|----------------|--------|-----------|
| 1. Contracts, Oracles and Risk Probes | 4/18 | In Progress | - |
| 2. Core Expressions | 0/TBD | Not started | - |
| 3. Polynomials, Number Theory and Algebraic Solve | 0/TBD | Not started | - |
| 4. Solve, Simplify, Assumptions and Piecewise | 0/TBD | Not started | - |
| 5. Calculus, Numerics and the Lean Loop | 0/TBD | Not started | - |
| 6. Matrices, Sets, Logic, Combinatorics and GPU Budgets | 0/TBD | Not started | - |

---
*Roadmap created: 2026-10-11*
