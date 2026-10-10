# Project Research Summary

**Project:** klein-science (signed Klein plugins `science.cas`, `science.numerics`, `science.prover`)
**Domain:** a licence-clean Rust computer-algebra system with a SymPy-parity target. It runs LLM-written SymPy programs
through a pure-Python stand-in in the Monty guest, executes on WASM (R0), a kernel parallel map (R1) and native CPU/GPU
(R2, CubeCL on Metal first), and pairs with Lean for certificates.
**Researched:** 2026-10-11
**Confidence:** MEDIUM-HIGH. Versions, licences, corpus measurements and CAS-algorithm lessons are HIGH. GPU/Metal
behaviour, egglog determinism and Monty's future feature set are MEDIUM to LOW and must be measured in M0.

Single source: `.planning/research/SCIENCE-PLUGIN-CAS.md` ("SS"). Nothing below overrides it. Every place where the
research disagrees with or refines SS is listed under **Operator decisions needed**, with a recommendation.
Symbolica-clean: no Symbolica material was read, searched or used by any researcher or by this synthesis.

## Executive Summary

klein-science is a large porting project in a well-understood domain. Every successful permissive CAS (SymPy, SymEngine,
FriCAS, Diofant) uses three layers: a canonicalising expression core, a typed domain layer where all the fast algebra
happens, and user-facing algorithms (solve, integrate, simplify) that convert into the domain layer and back. The
research converges on building exactly that in Rust, as about 25 small pure crates (`ks-*`) behind one operation
registry (`ks-ops`). The R0 component, the R1 map and the R2 native worker are thin shells around it. Byte identity
across R0/R1/R2 then reduces to "the same pure function compiled for two targets", provided every ordering decision is
structural, every budget is logical, every random choice is seeded from the input digest, and every number on the wire
is a decimal string. SymPy (BSD-3) is the specification for semantics, and its `core` auto-evaluation, printers and
`simplify` pipeline are ported line by line rather than reinvented. The printed `str()` form is what the LLM reads in
99% of programs, so printer parity is a core requirement, not polish.

The corpus re-measurement (full NuminaMath-TIR, 72,441 rows) confirms SS's headline numbers within method noise: 64%
SymPy share, top-50 names cover about 91%, top-100 about 97%. It also exposes the one structural problem in the
milestone plan: **`solve` (51% of SymPy programs) gates everything.** M1 as scoped reaches only 36.6% of SymPy programs,
so the "≥60% of TIR" M1 gate cannot pass unless an algebraic solve core moves forward (giving about 75.5%) or the gate is
restated. A second finding is that newer reasoning-model corpora (OpenMathReasoning) use SymPy far less (19%) and
numpy/scipy far more, which is a scope question for the operator.

The top risks are external and determinism-related, not algorithmic. (1) **Monty cannot run the stand-in today**: it
does not dispatch arithmetic, comparison, `__bool__`, `__call__`, `__getitem__` or conversion dunders, and has no
inheritance, `@property` or package imports. That gap is much wider than "operator overloading", it is a klein-rs
deliverable (dependency #4), and it blocks every in-Klein measurement. Mitigation: a CPython `host_shim` so the stand-in
and conformance run now, labelled "proxy". (2) **macOS Seatbelt plus Metal** is the top R2 risk; probe it first in M0
and file the measured rules as the `ComputeDevice` request. (3) **Silent nondeterminism** from hash-map iteration,
interning IDs, thread-count-dependent splits, Wasmtime fuel, egg/egglog time limits and float autotune. It is prevented
mechanically with lints, logical fuel and a multi-thread R0/R1/R2 byte-compare CI job seen RED before it is trusted. (4)
**Oracle drift** from unpinned SymPy, mpmath, ground types and hash seeds. Pin by image digest and generate goldens
under several hash seeds.

## Key Findings

### Recommended Stack

Rust 1.99.0, pinned exactly, building `wasm32-wasip2` components with `wit-bindgen` (not `cargo-component`, which is
stale) and native R2 workers. All core math is our own code ported from SymPy, mpmath and SymEngine, on top of a small
set of permissive building blocks. Shared-bytes crates use the same exact pins as klein-rs, so identity and signatures
agree with Klein. The stack layer is mechanically guarded by a `cargo-deny` allowlist that bans `rug`, `gmp-mpfr-sys`,
`malachite*`, `flint-sys`, `algebraeon`, `internment` and `symbolica`.

**Core technologies:**
- `dashu-int` 0.6.2 / `dashu-ratio` 0.6.1: Z and Q. The only maintained permissive pure-Rust bignum with NTT
  multiplication. It builds unchanged for wasm32. Encode as decimal strings on the wire, never via serde limbs.
- **Own port of mpmath 1.3.0 `libmp`** on `IBig`: `evalf` and numerics. It is the only way to match SymPy's digits
  exactly. Pin to 1.3.0 because SymPy 1.14.0 requires `mpmath<1.4`. One float implementation; `astro-float` is a dev-only
  cross-check.
- **Own arena and interner** (`hashbrown::HashTable` + fixed-seed `foldhash`): a per-request hash-consed DAG with
  `ExprId(u32)` and no `Ord`. Do not use `hashconsing` (global tables) or `internment` (malicious-dependency advisory).
- **Own `ks-poly`, `ks-dmat`, `ks-ntt`, `ks-groebner`**: no permissive crate is close to SymPy `polys`. `faer` 0.24.4
  only for float dense LA. `nalgebra` is not adopted (it would be a second mechanism).
- `egglog` =3.0.0 (`default-features = false`, one thread, canonical-key cost model, step-based limits): bounded
  simplify. It pulls `im-rc` (MPL-2.0, archived), which needs an operator ruling; `egg` 0.11.0 is the fallback.
- `cubecl` =0.11.0, feature `metal` (wgpu-msl): R2 GPU kernels. Verified from crate sources: u8–u64 and f16/bf16/f32 on
  Metal, **no f64**, no u64 atomics. The Metal path is LLVM-free; CUDA/HIP pull a bundled LLVM 23 (budget separately).
  `persistence` off; ship a kernel cache pinned by digest.
- `serde_json_canonicalizer` =0.3.2, `sha2` =0.11.0, `ed25519-dalek` =3.0.0: identical to klein-rs. Sign raw detached
  Ed25519 (no minisign/sigstore envelopes) and test against `verify_strict`.
- `rand_chacha` 0.10.0, seeded from the request digest, with our own range mapping. Deny `getrandom` in CAS crates.
- Offline oracle: Docker `python:3.13-slim-trixie@sha256`, `uv --require-hashes`, SymPy 1.14.0 + mpmath 1.3.0 +
  `rfc8785` 0.1.4, `PYTHONHASHSEED=0`, `SYMPY_GROUND_TYPES=python`, no gmpy2/python-flint, `--network=none`.
  SymEngine and Giac/FLINT live in separate images.

Build flags: `+simd128`, **never `+relaxed-simd`**, `codegen-units=1`, `lto="fat"`, `panic="abort"`,
`--remap-path-prefix`. CI must run the R0 artifact under the exact Wasmtime version Klein pins.

### Expected Features

The "user" is an LLM writing ordinary SymPy and reading printed output. Feature priority follows measured name frequency
(`symbols` 70%, `solve` 51%, `Eq` 27%, `sqrt` 15%, `.subs` 26.5%, `.evalf` 7.9%) and how programs fail (11.7% show a
traceback the LLM repairs from; 2.6% time out).

**Must have (table stakes, v1 = M0–M3):**
- GUEST: imports (cheap, never fail; SymPy-exact `ImportError` for hallucinated names), the full operator and
  conversion protocol, Python interop (`1/2*x` gives `0.5*x`), the common methods, SymPy-exact errors, and fuel
  exhaustion as a deterministic Python exception.
- PRINT: byte parity with SymPy's `str()` (term order via ported `sort_key`), Float formatting at 15 significant digits
  via mpmath `to_str` (already in M1), and a grammar-based `sympify` with no `eval`.
- CORE: symbols with assumptions, Z/Q/Float numbers and constants, elementary and combinatorial functions with
  SymPy-faithful auto-evaluation, relationals, `subs`/`expand`/`diff`/`evalf`.
- SOLVE-algebraic with return-shape parity (12.9% index `solve(...)[0]`) and assumption filtering; NT-primes;
  POLY-basic; bounded `simplify`, `collect`, `Piecewise`, `Interval`, `And`/`Or`.
- CERT 1–3 (solution substitution, `ring` identities, `norm_num` numeric facts): nearly free and high-frequency.
- CONF (oracle pipeline, corpora as recipes, three scoring levels) and DET (JCS identity, R0 replay).

**Should have (differentiators):**
- Deterministic replay: byte-identical exact results across R0/R1/R2 and deterministic fuel instead of wall-clock
  timeouts.
- `SolveResult{complete, certifiable, status}`: SymPy cannot say whether it found all solutions.
- CAS-to-Lean certificates. Ranks 4–6 come next: Pocklington/Pratt primality, univariate solve completeness, and ideal
  membership via `linear_combination`. Together with the last of these, **restore Mathlib's dead `polyrith`** with Klein's
  CAS as backend.
- An adjudication list of cases where SymPy is wrong, each decided by independent oracles.
- Exact GPU acceleration (NTT, modular LA, CRT, F4) admitted per operation class. Honest scope: it does not move TIR
  pass rates; it matters for large problems.
- Simplification strategy as bundle data.

**Defer (v2+):**
- CERT-9 SOS (needs an SDP solver) and CERT-10 WZ (research-grade).
- Algebraic-case Risch, Meijer G (unless measured otherwise), PDEs, geometry (0.9%), stats (0.18%),
  physics/tensor/codegen.
- A numpy/scipy facade (operator decision).
- Anti-features, never built early: `eval`-based parsing, user subclassing of SymPy classes, plotting,
  pretty/LaTeX printing before M6, reproducing SymPy's hash-seed-dependent orders, unbounded saturation, runtime
  backend choice, bit-identical floats across GPU/CPU, `native_decide` in proofs, and certifying limits, definite
  integrals, branch-cut rewrites or float `evalf`.

### Architecture Approach

One semantic core and three thin shells. Pure, `no_std`-friendly crates with no I/O, threads or clocks sit under a
single operation registry, `ks-ops`, which owns the step budget, plan/map/reduce and result normalisation. The wire
format is a canonical DAG node table, serialised as JCS, with all numbers as strings. Identity is the SHA-256 of that
table. Provenance (implementation, device, driver, timings) sits in a separate envelope outside the hashed bytes. The WIT
surface is minimal (`invoke`, `plan`, `map` and `reduce` over JCS bytes), because WIT has no recursive types and two
codecs would be two places for bytes to diverge. R0, R1 and R2 execute the same `plan → map → reduce` decomposition, so
byte identity follows from purity of `map` plus index-ordered merge. The GPU sits behind a narrow, batched kernel trait
(`ks-accel`) whose CPU reference implementation also compiles to WASM. GPU work is therefore off the critical path, and
a late `ComputeDevice` delays budgets, not features.

**Major components:**
1. `ks-num`, `ks-expr`, `ks-functions`, `ks-mp`: numbers; closed node-kind enum with structural canonical order;
   auto-evaluation as a written spec mirrored by the stand-in; core three-valued assumptions; function table generated
   from `schema/functions.toml`; mpmath port.
2. `ks-poly`, `ks-dmat`, `ks-groebner`, `ks-ntheory`: the algebra layer, which never touches `Expr` except through
   `construct_domain`/`to_expr`. Gröbner tracks cofactors from day one, because certificates and polyrith need them.
3. `ks-simplify`, `ks-solve`, `ks-calculus`, `ks-matrix`, `ks-cert`, `ks-print`/`ks-parse`, `ks-lambdify` (one tape IR
   shared by the CPU interpreter and one GPU tape kernel, never per-expression kernel generation).
4. `ks-wire` (node table, JCS, identity, envelopes, stdio framing), `ks-ops` (the only crate that knows the operation
   set), `ks-component` (R0/R1), `ks-worker` (R2), `ks-accel` + `ks-accel-cubecl`.
5. `standin/sympy`: one `Expr` class with a kind tag, valid in both the Monty subset and CPython, plus a Monty-subset
   lint and a CPython `host_shim` that is never shipped. Plus `prover/` (harness, skills, Lean package with the
   polyrith backend), `conformance/` (oracle Containerfiles, fetch recipes, golden manifests, adjudication,
   `ks-conform` with an R1 emulator), and `xtask` (dist, sign, digest).

Critical path: `ks-wire` → `ks-expr` → `ks-poly` → `ks-dmat`/`ks-groebner` → `ks-solve` → `ks-calculus`.

### Critical Pitfalls

1. **Monty does not dispatch the protocols the stand-in needs.** In M0, take a census of the Python protocols TIR uses
   on SymPy objects and file one precise klein-rs request: arithmetic, reflected and unary dunders, rich comparisons,
   `__bool__`, the conversion dunders including `__format__`, `__getitem__`/`__len__`, `__call__`, `@property` or
   `__getattr__`, plugin module import, and bigints. Design without inheritance (a kind tag plus an `isinstance` shim).
   Meanwhile, run everything under CPython via `host_shim`.
2. **Auto-evaluation that differs from SymPy's.** Port `Add/Mul/Pow.flatten` and the `Function.eval` rules as the
   specification, with the old assumptions system in M1 (`sqrt(x**2)`, `Abs`, `log(exp(x))` depend on it). Keep identity,
   internal canonical order and SymPy print order separate. Never use SymEngine as a printed-form oracle. Recovery is
   HIGH cost after M2, because every golden shifts.
3. **Order nondeterminism and non-logical budgets.** Use `ExprId` without `Ord` and `clippy::disallowed_types` for
   `HashMap`/`HashSet`. Partition work by fixed chunk size, independent of core count. Use `find_first`. `fuel_exhausted`
   comes only from logical fuel; Wasmtime fuel, memory caps and GPU timeouts are host errors. Use iterative traversals
   (the R0 stack is about 512 KiB). Land a multi-thread-count R0/R1/R2 byte-compare CI job, seen RED, before any R1/R2
   admission.
4. **JCS cannot carry CAS numbers as JSON numbers.** Every numeric leaf is a tagged string. Special values are named
   atoms. Symbol assumptions are part of the symbol node. Prefer arrays over symbol-keyed objects (UTF-16 key order).
   Fix this in the M0 schema: changing it after records exist changes every identity digest.
5. **`simplify` and `integrate` versus string parity.** egglog will find different "simpler" forms, and improving the
   strategy reduces string parity. Default to a ported SymPy `simplify` pipeline (`sympy-compatible`) with egglog as an
   opt-in strategy. Never write raw AC rewrites; never use time limits. Score by semantic equivalence (derivative check,
   numeric zero-test) with string match as a secondary metric. Risch is SymPy's transcendental case only; the algebraic
   case is a non-goal.

Also important: GPU integer assumptions (use 31-bit primes with u32 Montgomery; one prime table on every target); GPU
determinism leaks (autotune only for exact kernels, precise math mode, chunked dispatch under the macOS watchdog);
Seatbelt grants (measure the minimum; never wildcards); oracle drift (multi-seed goldens, normalised `Dummy` names,
`oracle_timeout` status); SymPy's 80 open "Wrong Result" issues (adjudication needs two independent oracles or one Lean
proof); licence contamination through LLM recall (a provenance header per algorithm module; agents never read GPL oracle
sources); and Lean junk values (`x/0 = 0`, `Real.sqrt` of a negative is 0), which need explicit domain hypotheses and a
faithfulness check before `certifiable`.

## Implications for Roadmap

The research keeps SS's M0–M6 shape. It changes what sits in each milestone in four places: assumptions and the
algebraic solve core move earlier, `ks-dmat` moves to M2, and simplify defaults to a SymPy port. All four are subject to
the operator decisions below. Klein-core dependencies D1–D5 are external; each phase lists what they block.

### Phase 1 (M0): Contracts, oracles and risk probes
**Rationale:** every later piece tests against the wire contract and the oracle pipeline. The four largest unknowns
(Monty protocols, Metal under Seatbelt, CubeCL integer widths, egglog on wasm32) must be measured before M1 is planned.
**Delivers:**
- `ks-wire` (JCS node table, string numbers, identity, envelopes, RFC vectors);
- `wit/science-cas.wit` with `ks-component` and `ks-worker` running one trivial operation, plus the first R0-versus-R2
  RED witness;
- the pinned SymPy oracle container, multi-seed goldens for a TIR slice, and golden manifests by digest (bytes kept
  outside git);
- the TIR protocol census and the widened D4 request;
- stand-in skeleton, Monty-subset lint and `host_shim`;
- the CubeCL Metal probe (u32 mulhi and u64 throughput, fast-math, watchdog) and the Seatbelt rule list, filed as D1;
- `cargo-deny` seen RED on `rug`, the provenance-header template, the fixed-box manifest, and the adjudication schema;
- the logical-fuel decision.
**Addresses:** GUEST-imports skeleton, CONF, DET.
**Avoids:** Pitfalls 1, 4, 5, 13, 15, 16, 17, 18, 19, 24, 25.

### Phase 2 (M1): Core expression system
**Rationale:** the critical path starts at `ks-expr`. Auto-evaluation and printing define what every later golden
looks like, so they must be faithful before algebra lands.
**Delivers:**
- `ks-num`, `ks-expr` (with the old three-valued assumptions), `ks-functions` from the spec table, `ks-mp` (mpmath
  1.3.0 port, `to_str`), `ks-print`/`ks-parse`, `subs`/`expand`/`diff`/`evalf`;
- the auto-eval spec mirrored in the stand-in, with a differential test;
- the multi-thread R0/R1/R2 determinism CI job;
- lazy SHA-256 at the boundary only.
**Addresses:** CORE, PRINT, GUEST-methods/errors/fuel.
**Avoids:** Pitfalls 2, 3, 9, 21, 22, 26, 28, 30.

### Phase 3 (M2): Algebra and number theory, plus the algebraic solve core
**Rationale:** `solve` is the single gate on TIR coverage, and its algebraic core depends on POLY factoring and
root-finding. Domain matrices are a prerequisite for F4, resultants, multi-modular LA and linsolve.
**Delivers:**
- `ks-ntheory` (BPSW, `factorint`, fixed prime table, digest-seeded PRNG);
- `ks-poly` (domains, Monagan–Pearce, Brown/Zippel gcd, Zassenhaus factoring);
- `ks-dmat` (Bareiss, multi-modular CRT) and `ks-groebner` (Buchberger, then F4, with cofactors);
- `ks-accel` CPU reference kernels (NTT, modmat, CRT), with the first R2 admissions;
- `ks-cert` v1 (CERT 2–4);
- if the operator accepts it, the algebraic `solve` core with return-shape parity.
**Uses:** dashu, `ks-ntt`, `rand_chacha`, CubeCL Metal (off the critical path).
**Avoids:** Pitfalls 6, 7, 13, 27.

### Phase 4 (M3): Solve, simplify, assumptions queries, Piecewise
**Rationale:** this completes the ≥90% TIR gate.
**Delivers:**
- full `ks-solve` (transcendental `_tsolve`, inequalities, `nsolve`, `SolveResult` honesty);
- the `sympy-compatible` simplify pipeline, plus bounded `klein-egglog` as strategy data;
- `ask`/`refine`; `Piecewise` and sets;
- CERT 1, 5 and 6;
- equivalence scoring in CONF.
**Avoids:** Pitfalls 8, 10, 11 (assumption-gated rewrites with side-conditions), 29.

### Phase 5 (M4): Calculus, numerics and the Lean loop
**Rationale:** the "Lean loop working" gate is hard-blocked on D5 (`lean.certify`).
**Delivers:**
- series, Gruntz (with Diofant fixes taken selectively, each with a witness), and the integrate ladder (table →
  manualintegrate → heurisch → transcendental Risch);
- Gosper sums and simple ODEs;
- `ks-numerics` (quadrature, ODE);
- the `prover/` harness, the polyrith backend, CERT-8, and a Lean faithfulness check;
- `lambdify` grids, if moved here.
**Avoids:** Pitfalls 11, 12, 23, 27, 31.

### Phase 6 (M5): Matrices, sets, logic, combinatorics, GPU budgets
**Delivers:**
- the Expr-level `Matrix` over `ks-dmat`, sets, logic and combinatorics, and CERT-7;
- the GPU dense LA budgets measured under the fixed-box manifest;
- the ≥97% TIR gate.
**Avoids:** Pitfalls 6, 14, 19.

### Phase 7 (M6): Parity long tail
Module by module against SymPy's test suite: Meijer G, PDEs, stats, geometry, physics, LaTeX printer, CERT-9/10 if
approved. It is multi-year and should be planned one module at a time.

### Phase Ordering Rationale

- The wire contract and oracles come first because every RED witness and every golden depends on them, and the JCS
  number encoding cannot change cheaply later.
- Auto-evaluation and assumptions come before algebra because a canonical-form change after M2 shifts every golden.
- POLY comes before or alongside solve because `solve` needs factoring and root-finding; `ks-dmat` comes before
  Gröbner because F4 needs it.
- GPU work is a parallel track behind the `ks-accel` CPU reference. D1 delays only budgets and R2 admissions.
- Every klein-rs dependency is filed from M0 measurements so that it lands before it blocks: D4 before in-Klein M1
  numbers, D2 before R1 admissions, D5 before the M4 gate.

### Research Flags

Phases likely to need `/gsd-plan-phase --research-phase`:
- **Phase 1 (M0):** Seatbelt and Metal (no authoritative Apple source), CubeCL integer widths and throughput on the M3
  Max, egglog 3.0 on `wasm32-wasip2` and its single-thread determinism, Monty host-call latency and bigints.
- **Phase 4 (M3):** the egglog strategy design and the share of rows where it diverges from SymPy's form (unmeasured);
  transcendental solve heuristics.
- **Phase 5 (M4):** the share of TIR definite integrals that SymPy routes through `meijerint` (decides whether a
  minimal `meijerint` moves forward), the Risch port, Lean certificate templates against pinned Mathlib, and faithfulness
  checking.
- **Phase 6 (M5) GPU part:** F4 Macaulay reduction on the GPU (arXiv 2601.06765), and per-backend conformance once CUDA
  is in scope.

Phases with standard patterns (skip research-phase):
- **Phase 2 (M1):** a direct port of SymPy `core`, the printers and mpmath. The patterns are well documented.
- **Phase 3 (M2):** standard literature (Monagan–Pearce, Brown/Zippel, Zassenhaus, Bareiss, Buchberger) plus SymPy's
  `polys`. F4 only needs research if it goes beyond the papers.

## Operator decisions needed

Each decision is deduplicated across the four research files. None is applied; SS stands until the operator rules.

1. **Identity encoding vs rule 4's wording.** CLAUDE.md rule 4 says "SHA-256 over the RFC-8785 JCS canonical tree". A
   literal tree is exponential in size for shared subterms. **Recommend:** identity = SHA-256 of the JCS of the
   canonical DAG node table, which is a linear-size canonical serialisation of the same tree, and an amendment to the
   rule's wording. (ARCHITECTURE)
2. **`science.numerics`: same bundle or same repo?** SS §3 lists it as a separate signed plugin; §8.6 says "numerics live
   in the same science plugin". **Recommend:** one repo and one workspace with shared `ks-mp`, two manifests, and `evalf`
   in `science.cas`. Please confirm whether §8.6 means repo or bundle. (ARCHITECTURE)
3. **The M1 "≥60% of TIR" gate.** M1 as scoped covers 36.6% of SymPy programs. **Recommend (a):** pull an algebraic solve
   core (linear systems, univariate polynomials with radicals up to degree 4, assumption filtering, return-shape parity)
   and the POLY root-finding it needs into M1/M2. That gives about 75.5% of SymPy programs. Alternatives: (b) restate the
   gate as "≥60% of all TIR rows", or (c) accept that the gate fails. (FEATURES)
4. **Core assumptions into M1.** The old per-symbol `is_*` system goes in M1, because construction-time evaluation
   depends on it; `ask`/`refine`/`Q` stay in M3. **Recommend:** yes. (ARCHITECTURE, PITFALLS)
5. **Domain matrices into M2.** **Recommend:** move `ks-dmat` into M2 (F4, resultants, multi-modular LA, linsolve). Keep
   only the Expr-level `Matrix` API and the GPU LA budgets in M5. (ARCHITECTURE)
6. **`lambdify` placement.** It is in M1 but appears in 0.3% of TIR programs and is mainly a GPU and numerics
   differentiator. **Recommend:** move it to M4 with `science.numerics`. No gate is affected. (FEATURES)
7. **Widen klein-rs dependency #4 (Monty).** The gap covers far more than operator overloading: conversion dunders,
   `__format__`, `__getitem__`/`__len__`, `__call__`, `__bool__`, `@property`/`__getattr__`, inheritance (or an
   `isinstance` hook), plugin module import, bigints, and `fractions`. **Recommend:** file one precise request from the
   M0 census; M0 specifies and tests against it instead of "spiking" it; SymPy's own tests run under CPython against the
   stand-in, because they subclass SymPy types. (FEATURES, ARCHITECTURE, PITFALLS)
8. **Logical fuel for `fuel_exhausted`.** **Recommend:** `fuel_exhausted` comes only from an algorithm-level step budget
   counted the same way on R0/R1/R2. Wasmtime fuel, memory caps and GPU timeouts are host errors, excluded from
   conformance. This refines SS's envelope semantics. (ARCHITECTURE, PITFALLS)
9. **Wasmtime upgrade request to klein-rs.** The 46.0.1 pin is affected by RUSTSEC-2026-0222/0223 and by 0316 (fuel
   bypass). **Recommend:** ask klein-rs to move to ≥49.0.1 (at least 46.0.2), set `relaxed_simd_deterministic(true)` and
   `cranelift_nan_canonicalization(true)` for science components, and keep `consume_fuel` on. Add it to ROADMAP
   "Dependencies on Klein core". (STACK, PITFALLS)
10. **egglog's `im-rc` (MPL-2.0, archived).** SS lists egglog as permissive, but bundles would ship MPL code.
    **Recommend:** a narrow `deny.toml` exception for `im-rc` only, with notice text, for M3. `egg` 0.11.0 is the
    documented fallback, and we also ask upstream to move to `imbl`. (STACK)
11. **SOS certificates need an SDP solver**, which is not on the approved building-block list. **Recommend:** defer
    CERT-9. When it is needed, evaluate Clarabel.rs (Apache-2.0). Never link CSDP (EPL-1.0). (FEATURES)
12. **Default simplify.** SS says "simplify (egglog)". **Recommend:** the default is a ported SymPy `simplify` pipeline
    (`sympy-compatible`); bounded egglog is an opt-in strategy that autoresearch improves; scoring is semantic, with
    string match secondary. (PITFALLS, FEATURES)
13. **Budgets that name 16 cores.** The M3 Max has 14 heterogeneous cores, so "16 cores ≥ 8×" and "GPU ≥ 5× a 16-core
    CPU" cannot be measured. Unified-memory GPU speedups also do not transfer to CUDA. **Recommend:** ratify budgets per
    box (for the Mac, all P-cores against one P-core), and attach a fixed-box manifest to every result. (PITFALLS)
14. **Goldens and rule 6.** Golden outputs embed corpus content. **Recommend:** golden bytes live in an artifact store by
    digest; git holds only recipes, manifests and aggregate scores. (ARCHITECTURE, PITFALLS)
15. **numpy/scipy facade.** In R1-era corpora (OpenMathReasoning), only 19% of programs use SymPy, while numpy is at 24%
    and scipy at 14%. **Recommend:** keep it out of v1. Decide after M3 whether to add a *narrow* facade
    (`scipy.integrate.quad`, `scipy.optimize.fsolve`, `numpy.roots`, `linalg.solve`) over `science.numerics`. Pin
    OpenMathReasoning shards by digest first. (FEATURES)
16. **Deterministic MP `evalf`.** The mpmath port is integer arithmetic, so it can be byte-identical across targets.
    **Recommend:** treat it as "deterministic-approximate", tagged approximate but conformance-gated like the exact
    classes. Reserve replay-from-record for hardware f32/f64. (PITFALLS, ARCHITECTURE)
17. **JCS implementation.** **Recommend:** use klein-rs's `serde_json_canonicalizer` =0.3.2 so Klein has one
    canonicalizer, rather than ARCHITECTURE's hand-written encoder. Add a hand-written encoder only if measurement shows
    JCS is hot, and then differential-test it. A Python mirror is still needed in the stand-in. (STACK vs ARCHITECTURE)
18. **Meijer G timing.** **Recommend:** measure in M0/M1 how many TIR definite integrals SymPy routes through
    `meijerint`. If the share is large, pull a minimal `meijerint` into M4. (PITFALLS)
19. **Port and reference list.** **Recommend:** keep the CLAUDE.md list as is (no `feanor-math` or Plonky3 as
    references), and add entries only per phase, by decision. Optionally, run an offline clone-detection pass against
    GPL corpora, where the tool reports locations only. (STACK, PITFALLS)
20. **Crate prefix.** `ks-` is a placeholder. The naming is the operator's call. (ARCHITECTURE)

## Confidence Assessment

| Area | Confidence | Notes |
|------|------------|-------|
| Stack | HIGH (MEDIUM for GPU/egglog) | Versions and licences come from the crates.io and PyPI APIs; the CubeCL Metal type support was read from the 0.11.0 crate sources. The wasm32 builds of egglog and faer, Apple u64 integer speed, and the Seatbelt needs are unmeasured. |
| Features | HIGH (corpus); MEDIUM (Lean, Monty) | The census was re-measured on the full digest-pinned NuminaMath-TIR. Milestone coverage is a static upper bound (names, not algorithm reach). The OpenMathReasoning sample is not digest-pinned. |
| Architecture | MEDIUM | The layering lessons from SymPy, SymEngine and FriCAS and the RFC 8785, WIT and Monty constraints are HIGH. Crate boundaries, plan/map/reduce and determinism mechanisms are reasoned from those facts but not yet measured. |
| Pitfalls | HIGH (CAS and determinism); MEDIUM-LOW (Metal) | Monty docs, RFC 8785, egg defaults, SymPy tracker counts and the oracle pins were verified on 2026-10-11. The Metal fast-math default, the watchdog behaviour and the AGX service names rest on thin sources. |

**Overall confidence:** MEDIUM-HIGH

### Gaps to Address

- **Monty's future feature set and host-call latency:** this decides how chatty the stand-in may be, and whether the
  in-Klein TIR numbers exist at all before D4. Handle with the M0 census and `host_shim` proxy numbers, clearly labelled.
- **The Metal Seatbelt minimum profile:** measured in the M0 probe. Fallback: R2 on the Mac stays CPU-only (rayon) until
  klein-rs ships the rules.
- **egglog determinism and wasm32 build:** a witness test in M0/M3 before egglog is admitted. ARCHITECTURE's question
  about egglog 2.x is superseded by STACK's 3.0 recommendation.
- **Real `solve`/`integrate` reach versus name coverage:** milestone percentages are upper bounds; execution-based
  scoring under `host_shim` replaces them from M1.
- **Simplify divergence share and `meijerint` share:** both unmeasured [U].
- **Fuel-count stability across Wasmtime versions:** unverified. This is moot if decision 8 is accepted.
- **Whether Klein instantiates a fresh component per call:** this affects arena reset. Ask klein-rs.
- **All performance budgets remain [U]** until measured on a ratified fixed box.

## Sources

### Primary (HIGH confidence)
- crates.io and PyPI registry APIs (2026-10-11): every crate and Python package version, licence and dependency named.
- CubeCL 0.11.0 crate sources (`cubecl-wgpu/src/backend/metal.rs`, `cubecl-metal/src/runtime.rs`); the wgpu `Features`
  docs.
- RFC 8785; Component Model WIT overview; Wasmtime deterministic-execution and `Config` docs; RustSec advisory-db.
- Monty limitations pages (classes, modules, language), pydantic.dev, fetched 2026-10-11.
- NuminaMath-TIR rev `77a91d7b…`, parquet SHA-256 `66163f9f…`: the corpus census (measured).
- SymPy docs (domains, DomainMatrix, custom functions); SymEngine design docs; egg `Runner` docs; SymPy GitHub "Wrong
  Result" label counts.
- klein-rs (read-only): `Cargo.lock`, `rust-toolchain.toml`, `design/16-dependencies.md`, `klein-compose/src/trust.rs`.

### Secondary (MEDIUM confidence)
- The egglog CHANGELOG and its wasm-example; the dashu and num-bigint changelogs; the CubeCL/Burn autotune docs.
- Mathlib `polyrith`, `norm_num` prime, PrimeCert, `leanprover/sos`, and `grind` docs; WZ-LLM (arXiv 2605.04472).
- The Chromium and WebKit Seatbelt GPU profiles; Apple forum threads on FP64, 64-bit integers, math mode and command
  buffer timeouts; the Dawn note on 64-bit atomics on MSL.
- FriCAS Expression references; the AC e-graph blowup papers; WASI 0.3 release notes.

### Tertiary (LOW confidence)
- Diofant's poly consolidation (recalled, not verified); the speed of Apple GPU u64 arithmetic (anecdotal);
  arXiv 2605.15445 and 2604.13514 (abstract only); the OpenMathReasoning sample (not digest-pinned).

---
*Research completed: 2026-10-11*
*Ready for roadmap: yes, after the operator rules on the decisions above (3, 4, 7 and 8 shape M0–M2 directly)*
