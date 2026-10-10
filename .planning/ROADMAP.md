# klein-science roadmap

Single source for design and decisions: [research/SCIENCE-PLUGIN-CAS.md](research/SCIENCE-PLUGIN-CAS.md) (operator
decisions 2026-10-11).

Scheduling (operator 2026-10-11): this project is kept apart from Klein's core build. It starts once the operator schedules
it, after Klein's 04.05/04.06 closes (deadline 2026-10-21).

## Phases

| Phase | Scope | Passes when | Size [U] |
|---|---|---|---|
| 1 (M0) Spike | Monty operator overloading (`__add__`, …); `sympy` stand-in skeleton; offline SymPy oracle pipeline; WIT tool interface; first CubeCL probe on the Mac's Metal GPU (CUDA deferred) | baseline measured | 2–3 plans |
| 2 (M1) Core | expression DAG (hash-consed, canonical order), numbers, elementary functions, `subs`, `expand`, `diff`, `evalf`, printing and parsing, `lambdify` | ≥60% of TIR corpus; SymPy `core` tests | ~20k LOC |
| 3 (M2) Polynomials and number theory | domains, gcd, factor, Gröbner (Buchberger → F4), resultants, primes | SymPy `polys`/`ntheory` tests | ~35k LOC |
| 4 (M3) Solve, simplify, assumptions | `solve`/`solveset`/`nsolve`, bounded egglog simplify, Piecewise, assumptions | ≥90% of TIR | ~30k LOC |
| 5 (M4) Calculus | Gruntz limits, series, integrate (table → manualintegrate → Risch), Gosper sums, simple ODEs | Lean certification loop working | ~45k LOC |
| 6 (M5) Matrices, sets, logic, combinatorics | exact and GPU linear algebra | ≥97% of TIR | ~30k LOC |
| 7 (M6) Parity long tail | PDEs, Meijer G, stats, physics, tensors, codegen | SymPy test suite, module by module | multi-year |

Every phase also has measured performance budgets (§4 of the single source). The `science.prover` plugin, with the Lean
prover harness and skills, moves here from Klein once Klein's 04.1 Lean guest engine lands.

## Dependencies on Klein core (requests to klein-rs; approved in principle by the operator 2026-10-11)

1. **`ComputeDevice` authority** (kernel D-R): `ProcessSandbox` may grant GPU device and driver access to a signed, locked
   native accelerator. This covers Linux CUDA/ROCm and macOS Metal (Seatbelt rules to be measured).
2. **Deterministic parallel WASM map** (kernel D-R): Klein runs N instances of a pure component on independent pieces and
   merges them in index order.
3. **Plugin provisioning by digest:** a `PluginArchive` artifact kind; lock v3 pins the plugin.
4. **Plugin component kinds** Skill/Harness/Data (Klein 04.05 plan 49) and the Python guest (Monty) with operator
   overloading.
5. **`lean.certify`** and the Lean guest engine (Klein 04.1).

## Infrastructure (operator 2026-10-11: local GPU first, remote GPU later)

- **Local first:** the operator's Mac is an Apple M3 Max with a 30-core GPU (Metal 4), 14 CPU cores and 36 GB of unified
  memory. Every GPU path (R2 via CubeCL's Metal/wgpu backend) is developed and measured there first.
  - **Consequence:** Metal has no f64. Exact work (Z, Q, Z/p, NTT, CRT) and f32 grids run on the GPU; f64 work falls back to
    the CPU (rayon).
  - **Consequence:** GPU benchmarks and conformance run on the Mac, unlike klein-rs, whose builds run only on the VM. Keep
    local builds to this workspace and watch memory pressure (the machine pages when klein-rs builds run alongside).
- **Remote GPU later:** a Nebius L40S (~$70–200/month) or H100 by the hour for CUDA and f64. Deferred; the estimate is in the
  single source §9. Nothing is created until the operator asks.
