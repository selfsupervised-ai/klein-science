# Science plugin: Klein CAS on CPUs and GPUs (single source)

Status: operator decisions 2026-10-11. This document is the single source for the klein-science project. It was moved out of
klein-rs, where it was briefly planned as Phase 04.3, so that this project does not mix with building Klein's core. It rests on two research passes on
2026-10-11:
1. SymPy parity and Symbolica.
2. Performance (Bend/HVM, Burn/CubeCL) and repo placement.

[U] marks a target or a claim that is not yet measured.

## 1. Goal

A Klein-owned, **licence-clean Rust computer-algebra system** that works toward **SymPy parity**. The operator accepts that it
is large and judges it important for Klein.
- **Who does what.** LLMs only *write* mathematical problems, as ordinary SymPy code. The CAS does the symbolic
  manipulation and then the calculation.
- **Lean and the CAS work as a pair.** The Lean prover handles the theory. The CAS hands Lean certificates that Lean can
  check, and Lean goals call the CAS to find witnesses.
- **It runs on CPUs and GPUs.** That includes the local GPU on a Mac, through Metal.

## 2. Licence rules (binding)

- **Symbolica-clean rule (a D-R now):**
  - No lane, reviewer, agent or tool reads Symbolica's source or internal docs, runs it, benchmarks against it or compares
    output with it.
  - Its Source-Available License 1.0 forbids using it as a "reference implementation, behavioral oracle, development
    blueprint… benchmark… output comparison" for a competing CAS, including by clean-room teams and AI systems (§4(4)–(6),
    §5; verified in License.md 2026-10-11).
- **May be ported, with their notices kept:** SymPy (BSD-3), mpmath, Diofant and FriCAS (BSD-3), SymEngine (MIT).
- **Never linked or shipped:** GPL/LGPL code such as FLINT, GMP/`rug`, `malachite`, Maxima, Giac, Singular, PARI or Sage.
- **Building blocks:** `dashu`, `num-*`, `astro-float`, `egglog`/`egg`, `faer`, `nalgebra`, rayon, CubeCL (all permissive).
- **Benchmark oracles, offline only, never linked or shipped:**
  - SymEngine (MIT);
  - GPL/LGPL binaries such as Giac and FLINT (operator 2026-10-11);
  - CPython plus real SymPy, in a container, producing frozen golden outputs pinned by digest. This is a D-R71 ruling:
    build and test time only, never at Klein runtime.

## 3. Shape

- **Plugins.** In `klein-science`, a **public** repo (operator 2026-10-11), as the signed plugins `science.cas`,
  `science.numerics` (an mpmath port, quadrature, ODE solvers) and `science.prover`.
- **Guest surface.** A pure-Python `sympy` stand-in for the Monty guest; a TypeScript one comes later.
  - It builds expressions locally and sends each heavy operation (`solve`, `factor`, `integrate`, `simplify`, `evalf`, …)
    as one typed tool call through the one RLM door.
  - Results are typed: `klein.science.expr.v1`, and a `SolveResult` with `complete` and `certifiable` flags and a status of
    exact | unevaluated | partial | fuel_exhausted.
- **Simplification strategy** (the rule sets and their order) is bundle data, so autoresearch can improve it.
- **Measured basis.** In a 2,000-row NuminaMath-TIR sample, 63% of programs use SymPy and only 122 distinct names appear.
  The 50 most-used names fully cover 92% of those programs, and the top 80 cover 97%.

## 4. Performance architecture: one typed API, three implementations, the set sealed in the lock

| | Implementation | Hardware | Home |
|---|---|---|---|
| R0 | Sandboxed WASM component, single-threaded with SIMD128, fuel-bounded | one CPU core | plugin; the always-present fallback and the replay reference |
| R1 | The kernel runs N instances of the same pure component on independent pieces (CRT primes, matrix blocks, rule sets) and merges them in index order | all CPU cores | **kernel D-R (approved)**: domain-neutral, more than one consumer |
| R2 | The same crates built natively with rayon and CubeCL (CUDA, ROCm, Metal/Vulkan/WebGPU, CPU), run as a signed, locked stdio worker in `ProcessSandbox` like `agent_lean_repl` | all cores plus GPUs, **including the Mac's local GPU via Metal** | plugin; **`ComputeDevice` authority kernel D-R (approved)** |

- **The guest sees one tool.** Which implementations a run may use is sealed in the lock, never chosen at run time
  (EXTENSION-PLACEMENT §5).
- **`ComputeDevice` authority.**
  - Linux: `/dev/nvidia*`, `/dev/kfd`, `/dev/dri`, plus read-only driver libraries.
  - macOS Seatbelt: the IOKit/Metal service lookups. This has to be measured first; it is the top risk.
  - Always: no network, a read-only plugin root, and a private tmp. The CubeCL kernel cache is pinned by digest.
- **Determinism.**
  - Exact arithmetic (Z, Q, Z/p) is independent of parallel order. An operation class is admitted for R1 or R2 only after
    R0, R1 and R2 produce byte-identical JCS output on the conformance corpus, with a witness seen RED.
  - Floating-point results are tagged `numeric_class: approximate`, with backend, device and driver recorded. Audit replay
    serves the recorded value. GPU kernels use fixed-tree reductions, with no float atomics.
  - Metal has no f64, so f64 work on a Mac falls back to the CPU.
- **Where the GPU pays off:**
  - dense modular linear algebra;
  - F4 Macaulay-matrix reduction mod p;
  - NTT multiplication of large polynomials and big integers;
  - batched multi-modular/CRT work;
  - `lambdify` grids and batch `evalf`;
  - quadrature and ODE ensembles.

  Rewriting, simplify, pattern matching, Risch and sparse polynomials run on CPU cores (R1/R2 rayon).
- **Not adopted:** Bend 2 and HVM. They break Bend 1/HVM compatibility, support only Nat/U32/F32, use affine
  (non-shareable) values, and the HVM4 repo shows no licence. Interaction nets also conflict with the hash-consed sharing a
  CAS depends on. One idea is kept: lock-free concurrent interning, with each node identified by its content hash.
- **Performance levers:**
  - a hash-consed DAG and arenas;
  - small-integer fast paths;
  - Monagan–Pearce heap-based sparse arithmetic (including its parallel form);
  - Brown/Zippel multi-modular GCD;
  - Hensel/Zassenhaus/van Hoeij factoring;
  - NTT;
  - F4/F5;
  - **bounded** e-graph saturation, because AC rules blow up.
- **Budgets (targets to ratify [U]):**
  - native ≤ 2× SymEngine on `expand2`/`lwbench`, WASM ≤ 3× native, and ≥ 20× faster than SymPy;
  - 16 cores ≥ 8× faster than one on Fateman sparse multiply;
  - GPU ≥ 5× faster than a 16-core CPU on a 10^6-degree NTT;
  - GPU multi-modular ≥ 10× faster than R1 on a 1000×1000 integer determinant;
  - CUDA ≥ 20× faster than R0 on a 10^8-point `lambdify` grid.

## 5. Milestones

| Milestone | Scope | Passes when | Size [U] |
|---|---|---|---|
| M0 | spike: Monty operator overloading (`__add__`, …); stand-in skeleton; oracle pipeline; WIT tool; first R2 Metal and CUDA probe | baseline measured | 2–3 plans |
| M1 | core: expression DAG, numbers, elementary functions, `subs`, `expand`, `diff`, `evalf`, printing and parsing, `lambdify` | ≥60% of TIR; SymPy `core` tests | ~20k LOC |
| M2 | polynomials and number theory: domains, gcd, factor, Gröbner, resultants, primes | SymPy `polys`/`ntheory` tests | ~35k LOC |
| M3 | solve, simplify (egglog), assumptions, Piecewise | ≥90% of TIR | ~30k LOC |
| M4 | calculus: Gruntz limits, series, integrate (table → manualintegrate → Risch), Gosper sums, simple ODEs | Lean loop working | ~45k LOC |
| M5 | matrices (exact plus GPU), sets, logic, combinatorics | ≥97% of TIR | ~30k LOC |
| M6 | parity long tail: PDEs, Meijer G, stats, physics, tensors, codegen | SymPy test suite, module by module | multi-year |

**Conformance** has four parts:
1. SymPy's own tests, run against the stand-in.
2. LLM-program corpora, scored against real-SymPy golden outputs.
3. Property tests, such as `expand(factor(p)) == p`.
4. An adjudication list for cases where SymPy itself is wrong.

## 6. Lean and the CAS

- **The CAS finds; Lean checks.** The CAS returns an answer plus a certificate. A Lean-mode child role turns it into a sealed
  statement plus a proof template: `linear_combination` (ideal membership), `ring` (factorisation product), `norm_num` /
  `decide` (roots, primality via Pratt certificates), `nlinarith` with SOS hints, or WZ certificates. `lean.certify`
  decides.
- **Lean asks; the CAS answers.** A Lean goal calls `kleinCall "science.cas.*"` for witnesses. Mathlib's `polyrith`
  (removed after its Sage server shut down) can be restored with Klein's CAS as its backend.
- **Heuristic only:** simplification across branch cuts, limits, definite integrals, completeness of a solve, and float
  `evalf`.

## 7. Repo split (operator 2026-10-11)

- **Stays in klein-rs** (trusted, domain-neutral):
  - the `lean/` verifier TCB;
  - the `lean.certify` gate;
  - the guest-engine seam and `klein-guest-lean`;
  - the `agent-lean/` worker source, because D-R87 gate packets bind those bytes;
  - `ProcessSandbox` and the `ComputeDevice` authority;
  - the R1 parallel map;
  - plugin admission and provisioning, including a `PluginArchive` artifact kind (D-R).
- **Moves to klein-science:**
  - the **lean-prover `.harness` and its skills.** The operator places the prover in the science plugin, not core, which
    amends the 04.1 note "installed by `klein init`": `klein init` *offers* to install the plugin.
  - premise-index recipes;
  - CAS crates, the accelerator, the `sympy` stand-in and numerics;
  - conformance corpora, as fetch recipes plus digests;
  - the Lean environment manifest content, once the gate accepts environments by digest.
- **Test fixture.** `examples/skills/lean-prover/` is replaced by a neutral fixture skill under `testdata/`. It is
  referenced from about ten places (grep lower bound), so that is a rename with both oracles.
- **Build and provisioning.** klein-science CI produces a reproducible `wasm32-wasip2` component plus native builds per
  target triple, a `klein.plugin.v1` manifest, an Ed25519 signature and an artifact published by digest. Klein provisions
  it by digest, and lock v3 pins it.
- **Index.** One plugin index, modelled on Claude Code's marketplace (an index that points at source repos).

## 8. Operator decisions (2026-10-11)

1. Use the repo `klein-science`, public.
2. The Lean prover agent is part of the science plugin, not core.
3. **Kernel D-Rs approved:** the `ComputeDevice` authority and the R1 deterministic parallel WASM map.
4. **Offline oracles:**
   - SymEngine (MIT);
   - GPL/LGPL binaries (Giac, FLINT), never linked or shipped;
   - CPython plus SymPy in a container (a D-R71 ruling).
5. **Hardware:** a GPU lane box for budget CI, with a cost estimate before it is created. The Mac's local GPU is also used
   (Metal through CubeCL).
6. Numerics live in the same science plugin.
7. The Symbolica-clean rule becomes a D-R now.

## 9. GPU lane box: cost estimate (2026-10-11)

**Prices.** Nebius list prices (nebius.com/prices, fetched 2026-10-11; USD excluding tax; on-demand prices effective
2026-10-01). The platforms are available in the project's region, eu-north1. The GPU quota there has not been checked [U].

| Option | Preset | On-demand | Preemptible | Notes |
|---|---|---|---|---|
| **L40S (Intel)** | `gpu-l40s-a` 1gpu-8vcpu-32gb (up to 32vcpu-128gb) | from $1.55/h | $0.74/h flat | 48 GB; strong int32/FP32; FP64 is weak (consumer-class rate). Fine for exact modular/NTT/CRT work and f32 grids |
| L40S (AMD) | `gpu-l40s-d` 1gpu-16vcpu-96gb | from $1.82/h | ~$0.90/h | more CPU per GPU |
| **H100 SXM** | `gpu-h100-sxm` 1gpu-16vcpu-200gb | $4.50/h | from $0.79/h (spot) | strong FP64. Needed only for the f64 budgets (dense LU, quadrature, ODE ensembles) |
| H200 SXM | 1gpu-16vcpu-200gb | $5.40/h | from $0.79/h | no advantage for a CAS over H100 |

**Monthly cost by usage pattern.** The box is stopped by the idle-reaper between runs. Rust builds happen on the CPU lane
box and only binaries and tests run on the GPU box, so GPU hours are test hours only.

| Usage | L40S on-demand | H100 on-demand |
|---|---|---|
| CI bursts, ~2 h per working day (44 h) | ~$68 | ~$198 |
| Active development, ~6 h per working day (132 h) | ~$205 | ~$594 |
| Always on (730 h) | ~$1,130 | ~$3,285 |
| Stopped (boot disk only, ~200 GiB) | ~$14–24 | ~$14–24 |

**Recommendation.**
- One **L40S Intel 1gpu-8vcpu-32gb** box: on-demand for gates and preemptible ($0.74/h) for long benchmark sweeps. Expect
  roughly **$70–200 a month** at the planned usage.
- Rent an H100 by the hour only for the f64 budget runs in phases 5–6 (M4/M5), a few hours each (~$5–20 per run).
- Nothing is created until the operator approves the box and its monthly cap. The stopped `nova-miner-instance` H100 belongs to
  another project and is not reused.

## 10. Sources

- github.com/sympy/sympy
- symbolica-dev/symbolica License.md and symbolica.io/license
- NuminaMath-TIR
- arXiv 2607.06820, 2604.13514, 2608.00326, 2510.25975, 2607.27705, 2605.25863, 2601.06765, 2610.01198
- github.com/bendlang/bend
- github.com/tracel-ai/burn and tracel-ai/cubecl
- wasmtime `Config` documentation
- Mathlib Polyrith docs
- anthropics/claude-plugins-official, openai/plugins
