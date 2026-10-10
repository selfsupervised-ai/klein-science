# Requirements: klein-science

**Defined:** 2026-10-11
**Core Value:** an LLM writes a mathematical problem as ordinary SymPy code; Klein computes it exactly and quickly through
this plugin, replays it byte-for-byte, and Lean certifies the parts that can be certified.

Sources: `research/SCIENCE-PLUGIN-CAS.md` (single source, "SS"), `research/SUMMARY.md` and the operator rulings of
2026-10-11 recorded in `PROJECT.md` Key Decisions. "TIR" = the NuminaMath-TIR corpus, pinned by digest. A percentage gate
is measured by executing programs against frozen SymPy goldens, not by name coverage. Until klein-rs ships the Monty
protocol (D4), pass rates are measured under the CPython `host_shim` and labelled **proxy**.

## v1 Requirements

v1 = milestones M0–M5 (roadmap phases 1–6). M6, the parity long tail, is v2.

### Determinism and identity (DET)

- [ ] **DET-01**: An expression's identity is the SHA-256 of the RFC-8785 JCS encoding of its canonical DAG node table,
      and the same expression built in any order has the same identity.
- [ ] **DET-02**: Every numeric leaf on the wire is a tagged string (Integer, Rational, Float with precision) and special
      values are named atoms, so no CAS number passes through a JSON number.
- [ ] **DET-03**: Provenance (implementation, device, driver, timings) travels in an envelope outside the hashed bytes.
- [ ] **DET-04**: `fuel_exhausted` is raised only by a logical step budget counted identically on R0, R1 and R2;
      Wasmtime fuel, memory caps and GPU timeouts surface as host errors, not as results.
- [ ] **DET-05**: A CI job runs the same exact operation on R0, R1 (emulated) and R2 at several thread counts and fails on
      any byte difference; it has been seen RED before it is trusted.
- [ ] **DET-06**: Float results from hardware f32/f64 are tagged `numeric_class: approximate` with backend, device and
      driver, and audit replay serves the recorded value.
- [ ] **DET-07**: Multiprecision `evalf` (the mpmath port) is deterministic-approximate: tagged approximate but
      conformance-gated byte-identical across R0/R1/R2 like exact results.
- [ ] **DET-08**: Maths crates contain no randomly seeded hash map iteration, no ordering by interning id, and no
      randomness not seeded from the request digest (enforced by lint and `cargo-deny`).

### Conformance and oracles (CONF)

- [ ] **CONF-01**: A container image pinned by digest (CPython 3.13, SymPy 1.14.0, mpmath 1.3.0, pinned ground types and
      hash seed, no network) produces golden outputs for a corpus slice, reproducibly on two runs.
- [ ] **CONF-02**: Golden bytes live in an artifact store by digest; git holds only fetch recipes, golden manifests and
      aggregate scores (rule 6).
- [ ] **CONF-03**: The conformance runner scores each program on three levels: stdout byte-equal, final answer equal, and
      error class equal.
- [ ] **CONF-04**: An adjudication list records each case where SymPy is wrong, with our answer and evidence from an
      independent oracle (Giac/FLINT/SymEngine offline, or a Lean proof).
- [ ] **CONF-05**: Scoring for simplify/integrate-shaped results includes semantic equivalence (derivative check, numeric
      zero test) with string match as a secondary metric.
- [ ] **CONF-06**: Every performance claim is measured on a ratified fixed box and carries a fixed-box manifest; budgets
      are ratified per box (Mac: all P-cores vs one P-core, Metal vs that).

### Licence and provenance (LIC)

- [ ] **LIC-01**: `cargo-deny` rejects GPL/LGPL and banned crates (`rug`, `gmp-mpfr-sys`, `malachite*`, `flint-sys`,
      `algebraeon`, `symbolica`) and has been seen RED on a deliberate violation.
- [ ] **LIC-02**: Every ported algorithm module carries a provenance header naming its permissive source, and `NOTICE`
      carries the upstream notices.

### Klein core dependencies (DEP)

- [ ] **DEP-01**: A census of the Python protocols TIR programs use on SymPy objects is filed as one precise klein-rs
      request for Monty (operators and reflected/unary forms, rich comparisons, `__bool__`, conversions incl.
      `__format__`, `__getitem__`/`__len__`, `__call__`, `@property`/`__getattr__`, an `isinstance` hook, plugin module
      import, bigints).
- [ ] **DEP-02**: The measured minimum macOS Seatbelt rules for CubeCL on Metal are filed as the input to the
      `ComputeDevice` request.
- [ ] **DEP-03**: A klein-rs request to move Wasmtime to ≥ 49.0.1 with deterministic relaxed-SIMD and NaN
      canonicalisation is filed and listed in ROADMAP "Dependencies on Klein core".

### Plugins and packaging (PLUG)

- [ ] **PLUG-01**: CI builds a reproducible `wasm32-wasip2` component and a native worker, a `klein.plugin.v1` manifest,
      a raw detached Ed25519 signature that passes `verify_strict`, and an artifact addressed by digest.
- [ ] **PLUG-02**: A WIT interface exposes one typed tool (`invoke`, `plan`, `map`, `reduce` over JCS bytes), and one
      trivial operation returns byte-identical results from R0 and R2.
- [ ] **PLUG-03**: `science.cas` and `science.numerics` are built from one workspace as two signed bundles sharing the
      mpmath port; `evalf` lives in `science.cas`.
- [ ] **PLUG-04**: `science.prover` ships as a signed bundle containing the Lean prover harness and its skills.

### Guest surface (GUEST)

- [ ] **GUEST-01**: An LLM program can `import sympy`, `import sympy as sp`, `from sympy import …`, `from sympy import *`
      and `from sympy.abc import x` without any tool call, and a missing name raises SymPy's exact error text.
- [ ] **GUEST-02**: The stand-in passes a Monty-subset lint and also runs under CPython through a `host_shim` that is
      never shipped.
- [ ] **GUEST-03**: An LLM program can build expressions with `+ - * / ** -x`, reflected forms against Python numbers,
      and comparisons that build Relationals, with SymPy's results (including `1/2*x` → `0.5*x`).
- [ ] **GUEST-04**: Truth-testing a Relational raises SymPy's `TypeError` text, and `int()`/`float()`/f-strings convert
      numbers as SymPy does.
- [ ] **GUEST-05**: The common methods (`.subs`, `.evalf`, `.is_*`, `.coeff`, `.simplify`, `.lhs`/`.rhs`, `.doit`,
      `.free_symbols`, `.args`, `.as_numer_denom`, `.as_coefficients_dict`) behave as in SymPy.
- [ ] **GUEST-06**: Common failures raise SymPy's exception class and message (`NotImplementedError` from solve or
      integrate, `TypeError`, `ValueError`).
- [ ] **GUEST-07**: Fuel exhaustion surfaces as a deterministic Python exception, identical on every replay.

### Printing and parsing (PRINT)

- [ ] **PRINT-01**: `str()` and `repr` of expressions and containers are byte-identical to SymPy's default printer,
      including term order (ported `sort_key`).
- [ ] **PRINT-02**: Floats print at 15 significant digits by default and `n` digits on request, with mpmath rounding
      (ported `to_str`).
- [ ] **PRINT-03**: `sympify`, `S("…")` and `parse_expr` use a grammar parser with no `eval` and produce SymPy's trees.

### Core expressions (CORE)

- [ ] **CORE-01**: The expression store is a per-request hash-consed DAG whose canonical order is structural and
      independent of interning ids and hash seeds.
- [ ] **CORE-02**: `symbols`/`Symbol`, `Function`, `Dummy` and `Wild` work, with assumptions (`integer`, `positive`,
      `real`, `nonnegative`, …).
- [ ] **CORE-03**: Per-symbol three-valued `is_*` assumption queries match SymPy and feed construction-time evaluation.
- [ ] **CORE-04**: `Integer`, `Rational`, `Float` (arbitrary precision), `S`, `I`, `pi`, `E`, `oo`, `zoo` and `nan`
      behave as in SymPy, with exact Z/Q arithmetic on `dashu`.
- [ ] **CORE-05**: `Add`, `Mul` and `Pow` auto-evaluate exactly as SymPy's `flatten` rules do, with a written
      specification mirrored by the stand-in and a differential test.
- [ ] **CORE-06**: Elementary and combinatorial functions (`sqrt`, trig and inverses, `atan2`, `log`, `exp`, `Abs`,
      `floor`/`ceiling`, `Max`/`Min`, `re`/`im`/`arg`/`sign`, `Mod`, `root`, `binomial`, `factorial`) auto-evaluate as
      SymPy's `eval` rules do.
- [ ] **CORE-07**: `Eq`, `Ne`, `Lt`, `Le`, `Gt`, `Ge` with `.lhs`/`.rhs` behave as in SymPy.
- [ ] **CORE-08**: `subs`, `expand`, `diff`/`Derivative` and `Lambda` give SymPy's results.
- [ ] **CORE-09**: `N`/`evalf` evaluate elementary functions and constants to arbitrary precision through the mpmath
      1.3.0 port, matching SymPy's digits.

### Polynomials (POLY)

- [ ] **POLY-01**: Domains ZZ, QQ, GF(p) and algebraic extensions convert to and from expressions at one boundary.
- [ ] **POLY-02**: `Poly`, `degree`, `.all_coeffs`, `div`, `gcd`/`lcm` and `discriminant` give SymPy's results.
- [ ] **POLY-03**: `factor` factors univariate and multivariate polynomials over ZZ/QQ as SymPy does.
- [ ] **POLY-04**: Univariate root finding (rational roots, radicals through degree 4, `roots`) matches SymPy.
- [ ] **POLY-05**: Resultants and Gröbner bases (Buchberger, then F4) are computed with cofactor tracking.
- [ ] **POLY-06**: The exact domain matrix layer (`ks-dmat`: Bareiss, multi-modular CRT) solves and reduces matrices over
      ZZ, QQ and GF(p).

### Number theory (NT)

- [ ] **NT-01**: `isprime` is deterministic (BPSW plus SymPy's Miller–Rabin bases) and matches SymPy.
- [ ] **NT-02**: `factorint`, `primefactors`, `divisors`, `divisor_count`, `totient` match SymPy, including dict order.
- [ ] **NT-03**: `primerange`, `nextprime`, `prime`, `mod_inverse`, `crt`/`solve_congruence` match SymPy.

### Solve (SOLVE)

- [ ] **SOLVE-01**: `solve` handles linear systems and univariate polynomial equations through degree 4 with radicals,
      with SymPy's return shape (list, dict, list of dicts, `dict=True`).
- [ ] **SOLVE-02**: `solve` filters solutions by symbol assumptions as SymPy does.
- [ ] **SOLVE-03**: `solve` handles rational-function equations and polynomial systems (via Gröbner).
- [ ] **SOLVE-04**: Every solve returns a `SolveResult` with `complete`, `certifiable` and status (exact, unevaluated,
      partial, fuel_exhausted); `complete` is true only for decision procedures.
- [ ] **SOLVE-05**: `solve` handles transcendental equations through SymPy's `_tsolve`/`_invert` heuristics, reporting
      `complete=false`.
- [ ] **SOLVE-06**: `solveset`, `linsolve`, `nonlinsolve` and univariate and rational inequalities match SymPy.
- [ ] **SOLVE-07**: `nsolve` finds numeric roots through the mpmath `findroot` port.

### Simplification (SIMP)

- [ ] **SIMP-01**: The default `simplify` is a port of SymPy's pipeline and gives SymPy's output on the conformance
      corpus.
- [ ] **SIMP-02**: `collect`, `together`, `apart`, `nsimplify` and `trigsimp` match SymPy.
- [ ] **SIMP-03**: A bounded egglog (or egg) strategy is available as opt-in bundle data, step-limited and never
      time-limited (the `im-rc` licence question is ruled before this lands).
- [ ] **SIMP-04**: `ask`, `refine` and `Q` answer assumption queries three-valued as SymPy does.
- [ ] **SIMP-05**: `Piecewise`, `Interval`, `FiniteSet`, `Union`/`Intersection` and `And`/`Or`/`Not` work as solve and
      inequality results.

### Calculus (CALC)

- [ ] **CALC-01**: `limit` uses a Gruntz port and matches SymPy.
- [ ] **CALC-02**: `series` and `.removeO` match SymPy.
- [ ] **CALC-03**: `integrate` runs the ladder table → manualintegrate → heurisch → transcendental Risch and matches
      SymPy on indefinite integrals; definite integrals are marked heuristic.
- [ ] **CALC-04**: `summation`/`Sum` with Gosper's algorithm match SymPy.
- [ ] **CALC-05**: `dsolve` and `rsolve` handle the simple ODE and recurrence classes SymPy solves.

### Numerics (NUM)

- [ ] **NUM-01**: `science.numerics` provides quadrature and ODE solvers ported from mpmath.
- [ ] **NUM-02**: `lambdify` compiles to one tape IR run by a CPU interpreter and one GPU tape kernel (f32 on Metal,
      f64 on CPU).

### Matrices and discrete maths (MAT, DISC)

- [ ] **MAT-01**: `Matrix` with `det`, `inv`, `rref`, `eigenvals`, `dot`, `cross`, `norm` and `.T` matches SymPy, built on
      the exact domain matrix layer.
- [ ] **DISC-01**: Sets, logic (`satisfiable`, `Implies`) and combinatorics (`Permutation`, `multiset_permutations`,
      `subsets`, `stirling`) match SymPy.

### Lean certificates and prover (CERT, PROVER)

- [ ] **CERT-01**: A solved equation yields a solution-substitution certificate that Lean checks with `norm_num`/`ring_nf`.
- [ ] **CERT-02**: `expand`/`factor`/`together`/`apart` yield an identity certificate that Lean checks with `ring` or
      `field_simp; ring`.
- [ ] **CERT-03**: gcd, small primality and factorisation results yield certificates that Lean checks with `norm_num`.
- [ ] **CERT-04**: Large primes yield a Pocklington (or Pratt) certificate that Lean checks without `native_decide`.
- [ ] **CERT-05**: A complete univariate polynomial solve yields a completeness certificate (full factorisation).
- [ ] **CERT-06**: Ideal membership yields cofactors that Lean checks with `linear_combination (exp := n)`.
- [ ] **CERT-07**: Linear-algebra results (inverse, solution, rank witness) yield certificates Lean checks.
- [ ] **CERT-08**: Indefinite integrals yield an antiderivative certificate (F′ = f with domain side conditions).
- [ ] **CERT-09**: Before `certifiable` is set, a faithfulness check rejects Lean statements that rely on junk values
      (`x/0 = 0`, `Real.sqrt` of a negative).
- [ ] **PROVER-01**: A Lean goal can call `kleinCall "science.cas.*"` to obtain a witness.
- [ ] **PROVER-02**: Mathlib's `polyrith` works again with Klein's CAS as its backend.
- [ ] **PROVER-03**: The CAS→Lean loop runs end to end through `lean.certify` (klein-rs D5).

### Acceleration (ACCEL)

- [ ] **ACCEL-01**: A CubeCL probe on the M3 Max's Metal GPU measures u32 multiply-high and u64 throughput, the
      fast-math default and the watchdog limit.
- [ ] **ACCEL-02**: `ks-accel` defines batched NTT, modular matrix, CRT and F4-reduction kernels with a CPU reference
      implementation that also compiles to WASM, using 31-bit primes and one prime table on every target.
- [ ] **ACCEL-03**: An exact operation class is admitted to R1/R2 only after R0, R1 and R2 produce byte-identical output
      on the conformance corpus, with a witness seen RED.
- [ ] **ACCEL-04**: GPU dense modular linear algebra and NTT budgets are measured on the Mac's Metal GPU under the
      fixed-box manifest.

### Milestone gates (GATE)

- [ ] **GATE-01**: M1: SymPy's `core` tests pass against the stand-in under CPython.
- [ ] **GATE-02**: M2: ≥ 60% of TIR programs pass (proxy until D4), with SymPy's `polys`/`ntheory` tests passing.
- [ ] **GATE-03**: M3: ≥ 90% of TIR programs pass.
- [ ] **GATE-04**: M5: ≥ 97% of TIR programs pass.

## v2 Requirements

Deferred: tracked but not in the current roadmap.

### Parity long tail (M6)

- **TAIL-01**: Meijer G integration (earlier if the M0/M1 `meijerint` measurement shows TIR needs it).
- **TAIL-02**: PDEs, `sympy.stats`, `sympy.geometry`, physics, tensors, codegen, module by module against SymPy's tests.
- **TAIL-03**: LaTeX and pretty printers.
- **TAIL-04**: SOS certificates (needs an SDP solver approved by the operator; Clarabel.rs is the candidate).
- **TAIL-05**: WZ-pair certificates (needs Zeilberger and Lean-side glue).
- **TAIL-06**: CUDA/ROCm R2 lanes and f64 GPU budgets on the deferred remote GPU box.
- **TAIL-07**: A narrow numpy/scipy facade over `science.numerics` (decided after M3).
- **TAIL-08**: A TypeScript `sympy` stand-in.

## Out of Scope

| Feature | Reason |
|---------|--------|
| Reading, running, benchmarking or comparing with Symbolica | Binding rule 1: its licence forbids use as a reference, oracle or benchmark |
| Linking or shipping GPL/LGPL code (FLINT, GMP/`rug`, `malachite`, Maxima, Giac, Singular, PARI, Sage) | Binding rule 2; GPL binaries are offline oracles only |
| Any edit to klein-rs | This repo ships plugins; Klein core needs are requests |
| `eval`-based `sympify` | It is a sandbox hole; a grammar parser gives the same trees |
| User subclassing of SymPy classes | Monty has no inheritance; 0.0% of TIR programs define a class |
| Plotting | No display, non-deterministic, 0.1% of TIR |
| Reproducing SymPy's hash-seed-dependent orders | Non-deterministic even in SymPy; conflicts with DET |
| Unbounded or time-limited simplify saturation | AC blowup and machine-speed-dependent results |
| Runtime choice of CPU/GPU backend | The implementation set is sealed in the lock |
| Bit-identical hardware floats across GPU/CPU | Impossible across drivers; floats are tagged and replayed |
| `native_decide` in certificate proofs | Outside the Lean kernel TCB |
| Certifying limits, definite integrals, branch-cut rewrites or float `evalf` | Heuristic only (SS §6); `certifiable=false` |
| Algebraic-case Risch | Research-grade; TIR integrals are elementary |
| Creating a remote GPU box | Deferred; nothing is created without operator approval |

## Traceability

Which phases cover which requirements. Updated during roadmap creation.

| Requirement | Phase | Status |
|-------------|-------|--------|

**Coverage:**
- v1 requirements: 92 total
- Mapped to phases: 0 (pending roadmap)
- Unmapped: 92

---
*Requirements defined: 2026-10-11*
*Last updated: 2026-10-11 after initial definition*
