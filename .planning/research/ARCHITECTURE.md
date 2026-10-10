# Architecture Research

**Domain:** a licence-clean Rust computer-algebra system shipped as signed Klein plugins (`science.cas`,
`science.numerics`, `science.prover`). It has one typed tool API with three implementations (R0 WASM component, R1
kernel-run parallel map, R2 native rayon+CubeCL worker), a pure-Python `sympy` stand-in for the Monty guest, and a Lean
pairing.
**Researched:** 2026-10-11
**Confidence:** MEDIUM overall.
- The layering lessons from SymPy and SymEngine are HIGH; they come from official docs and are consistent with long-known
  design.
- The constraints from the Monty, RFC 8785, WIT and Metal sources are HIGH; they are official docs or the RFC, fetched
  2026-10-11.
- The crate boundaries and the determinism mechanisms are this document's recommendations. They are reasoned from those
  facts and have not yet been measured.
- CubeCL per-backend integer and float support is LOW. It is undocumented and must be probed in M0.

The research seam tags every web-sourced digest LOW by policy. Where this document says HIGH, the claim was cross-checked
against an official primary source (an RFC, a project's own docs) or is restated from the single source.

Single source: `.planning/research/SCIENCE-PLUGIN-CAS.md`. Where this document refines or questions it, the point is
listed in "Disagreements and clarifications for the operator" near the end. That file is not modified.

---

## Key recommendations (read this first)

1. **One semantic core, three thin shells.**
   - All mathematics lives in plain `no_std`-friendly Rust crates with no I/O, no threads and no clocks.
   - An **operation registry** (`ks-ops`) is the single entry point. It is wrapped three ways: the R0 component (and R1,
     which runs the same component), and the R2 stdio worker.
   - The shells only decode bytes, call `ks-ops`, and encode bytes. Byte identity across R0/R1/R2 then reduces to "the
     same pure function compiled for two targets".
2. **Copy SymPy's most important layering decision: the Expr/Domain boundary.**
   - Expression trees (`ks-expr`) are not used for algebra. Algebra runs on typed domain elements (`ks-poly` domains: ZZ,
     QQ, GF(p), K[x], K(x), algebraic fields, and EX as the fallback).
   - Explicit conversion happens at the boundary (`construct_domain`, `from_expr`/`to_expr`). This is how SymPy, Diofant
     and SymEngine all keep polys fast, and FriCAS goes further: `Expression` *is* a fraction field over kernels.
3. **Use a closed node-kind enum plus a data-driven function table, not an open class hierarchy.**
   - A closed set of kinds is what makes canonical ordering, JCS encoding and cross-target identity tractable.
   - It is also the only shape the Monty stand-in can mirror, because Monty has no inheritance.
4. **Canonical order is a structural total order. It never depends on interning IDs, pointer values or hash-map iteration.**
   - This is enforced at compile time: `ExprId` does not implement `Ord`, and `HashMap`/`HashSet` are listed in
     `clippy::disallowed_types` for semantic crates.
5. **The wire format is a canonical DAG node table, serialised with JCS.**
   - Identity is the SHA-256 of that JCS. Numbers are JSON *strings*, because JCS numbers are IEEE doubles.
   - Provenance (implementation, device, driver, timings) lives in a separate envelope outside the identity bytes.
6. **Exactness is a property of the algorithm, not only of the arithmetic.**
   - Fuel is a CAS-level **step budget** counted identically on all targets. Wasmtime fuel is only a safety backstop.
   - Modular algorithms use one fixed prime sequence on every target.
   - Randomised algorithms are seeded from the input digest.
   - Every non-unique output (factor order, unit normalisation, root order) is canonicalised before encoding.
7. **R1 is plan → map → reduce, and R0 and R2 execute the *same* plan.**
   - R0 runs the pieces sequentially in-process.
   - R1 is Klein's map (a klein-rs dependency).
   - R2 runs the pieces with rayon and collects them in index order.
   - Because all three share the decomposition, byte identity follows from purity of `map`.
8. **The GPU boundary is a narrow, batched, typed kernel trait (`ks-accel`).**
   - It has a CPU reference implementation that also compiles into WASM, and a CubeCL implementation that is native-only.
   - Exact kernels use primes below 2^31 with 32-bit Montgomery/Barrett arithmetic, so they run on Metal, WGSL, CUDA and
     HIP alike.
   - `lambdify` grids use a single **tape-interpreter kernel**, not per-expression generated kernels. The kernel set
     therefore stays closed and the CubeCL cache can be pinned by digest.
   - f64 on a Mac runs on the CPU, through one pure-Rust libm so that CPU float results are reproducible across R0 and R2.
9. **Move two things earlier than the draft roadmap:**
   - a domain-level matrix core (`ks-dmat`) into M2, because Gröbner/F4, resultants, linsolve and partial fractions need it;
   - the core three-valued `is_*` assumption queries into M1, because SymPy's automatic evaluation (`Pow`, `Abs`,
     `sqrt(x**2)`) consults them.
10. **The Monty stand-in is the hardest external dependency, and it is broader than "operator overloading".**
    - Monty's current docs say arithmetic and comparison dunders, `__call__`, `__getitem__`, `__bool__` and `__len__` are
      not dispatched.
    - Monty also has no inheritance, no `@property`, no generators, and no imports beyond its bundled stdlib.
    - The klein-rs request (dependency 4) must be widened accordingly; see "Integration Points".

---

## Standard Architecture

### System Overview

```
┌──────────────────────────────────────────────────────────────────────────────────────┐
│ GUEST (inside Klein; klein-rs owns the runtime)                                      │
│  LLM-written SymPy program ──► `sympy` stand-in (pure Python, Monty subset)          │
│     local: Symbol/Integer/Rational construction, + - * / **, ==, hash, printing      │
│     heavy op ─► ONE typed tool call through the RLM door  (science.cas.<op>)         │
│  Lean goal ──► kleinCall "science.cas.<op>"  (via klein-guest-lean, klein-rs)        │
└───────────────────────────────┬──────────────────────────────────────────────────────┘
                                │ JCS request envelope (bytes)  ▲ JCS response + certificate
                                ▼                                │
┌──────────────────────────────────────────────────────────────────────────────────────┐
│ KLEIN KERNEL (klein-rs; external to this repo)                                       │
│  plugin admission/provisioning by digest (PluginArchive, lock v3)                    │
│  implementation set sealed in lock: {R0} ⊆ set ⊆ {R0,R1,R2}                          │
│   R0: wasmtime, 1 instance, fuel-bounded       R1: N pure instances, index-order merge│
│   R2: ProcessSandbox + ComputeDevice → stdio worker                                  │
│  lean.certify gate (Lean TCB)                                                        │
└──────┬───────────────────────────┬──────────────────────────────┬────────────────────┘
       ▼                           ▼                              ▼
┌──────────────┐   ┌───────────────────────────┐   ┌──────────────────────────────────┐
│ ks-component │   │ ks-component ×N           │   │ ks-worker (native, per triple)    │
│ (wasm32-     │   │ exports plan/map/reduce   │   │ length-prefixed stdio frames      │
│  wasip2)     │   │ (same artifact as R0)     │   │ rayon pool + ks-accel-cubecl      │
└──────┬───────┘   └────────────┬──────────────┘   └────────────────┬─────────────────┘
       └────────────────────────┼───────────────────────────────────┘
                                ▼
┌──────────────────────────────────────────────────────────────────────────────────────┐
│ ks-ops  operation registry · step budget · plan/map/reduce · result normalisation    │
│ ks-wire canonical DAG node table ⇄ JCS bytes · SHA-256 identity · envelopes          │
├──────────────────────────────────────────────────────────────────────────────────────┤
│ DOMAIN-SPECIFIC LAYERS (pure, deterministic)                                         │
│  ks-solve  ks-calculus  ks-simplify(+egglog, strategy = bundle data)  ks-sets/logic  │
│  ks-matrix (Expr-level)   ks-cert (certificates)   ks-print / ks-parse               │
├──────────────────────────────────────────────────────────────────────────────────────┤
│ ALGEBRA LAYER (no Expr inside the hot loops)                                         │
│  ks-poly (domains, dense DUP, sparse MPoly, gcd, factor, resultants, roots)          │
│  ks-groebner (Buchberger → F4)   ks-dmat (DomainMatrix: dense/sparse over a domain)  │
│  ks-ntheory (primes, factorint, residues, Pratt)                                     │
├──────────────────────────────────────────────────────────────────────────────────────┤
│ CORE                                                                                 │
│  ks-expr (hash-consed DAG, closed kinds, canonical order, auto-eval, subs, diff,     │
│           core assumptions)   ks-functions (function table, eval rules)               │
│  ks-mp (mpmath libmp port: binary MP floats on big-int mantissas, evalf)             │
│  ks-num (Integer small/big, Rational, Zp Montgomery, prime table)                    │
├──────────────────────────────────────────────────────────────────────────────────────┤
│ ACCELERATION BOUNDARY  ks-accel (trait + CPU reference impl, compiles to WASM)        │
│                        ks-accel-cubecl (native only: CUDA / HIP / Metal(MSL) / Vulkan)│
└──────────────────────────────────────────────────────────────────────────────────────┘
```

Data flows **downward** for computation and **upward** only as values. No lower crate depends on a higher one, and no
crate below `ks-ops` knows which implementation (R0/R1/R2) it is running in.

### Component Responsibilities

| Component | Owns | Must NOT | Typical implementation |
|---|---|---|---|
| `ks-num` | Integer (i64 fast path, `dashu-int` big path), Rational, `Zp` (u32 Montgomery, u64 where available), the fixed prime table | expose limb size in any output; use floats | `enum Int { Small(i64), Big(Arc<IBig>) }`, canonical decimal-string encoding |
| `ks-expr` | closed `Kind` enum; per-request arena and interner; canonical structural order; auto-evaluation (Add/Mul/Pow flatten, collect, sort); `subs`, `xreplace`, traversal, `diff`; core assumptions (`is_positive` etc., three-valued) | order by `ExprId`; iterate a `HashMap`; call I/O | arena `Vec<Node>` + interner map; `ExprId(u32)` newtype without `Ord` |
| `ks-functions` | function table (name → arity, eval rule, derivative, numeric hook, Lean name); elementary and special functions | hard-code functions into `Kind` | data table + Rust eval fns; generated from one spec file |
| `ks-mp` | mpmath `libmp` port: (sign, man, exp, bc) floats on `IBig`, correct rounding, elementary/special numerics, `evalf` | use hardware floats on the exact path | direct port of mpmath (BSD-3, notice kept) |
| `ks-poly` | Domain trait and domains (ZZ, QQ, GF(p), K[x..], K(x..), QQ<α>, EX); dense univariate; sparse multivariate (Monagan–Pearce heap); gcd (heuristic, Brown, Zippel); factor (Zassenhaus/van Hoeij; Wang/EEZ multivariate); resultants; real-root isolation; `construct_domain` | depend on `ks-solve`/`ks-calculus` | ports of SymPy `densetools`/`rings`/`factortools`, SymEngine poly ideas |
| `ks-dmat` | DomainMatrix: dense (DDM-like) and sparse (SDM-like) over any domain; Bareiss, Berkowitz, rref_den, modular det/rank via CRT | know about `Expr` | port of SymPy `polys.matrices` design |
| `ks-groebner` | Buchberger, then F4 (symbolic preprocessing on CPU, Macaulay reduction through `ks-accel`) | port msolve/FGb/Singular (GPL or closed) | own implementation from papers |
| `ks-ntheory` | sieve, BPSW/Miller–Rabin, Pollard rho/ECM/SIQS, residues, CRT, discrete log, Pratt certificates | randomness without a digest-derived seed | port of SymPy `ntheory` |
| `ks-simplify` | rule-set engine; bounded egglog saturation; trigsimp/radsimp/powsimp; loads **strategy data** from the bundle | run egglog multi-threaded; extract with ties broken by e-class ID | egglog (MIT), single-threaded, canonical tie-break |
| `ks-solve` | solve/solveset/linsolve/nonlinsolve, inequalities, Piecewise, `SolveResult` flags | claim `complete` without a proof path | port of SymPy solvers on top of `ks-poly`/`ks-groebner` |
| `ks-calculus` | series, Gruntz limits, integrate (table → manualintegrate → heurisch → Risch), Gosper/Zeilberger sums, ODEs | — | Risch on a kernel-field representation (FriCAS-style) |
| `ks-matrix` | Expr-level `Matrix` API (SymPy surface) over `ks-dmat` | duplicate the algorithms in `ks-dmat` | thin wrapper, like SymPy's `Matrix` → `DomainMatrix` |
| `ks-cert` | certificate data types + emitters (ideal membership cofactors, factor product, Pratt, root witness, SOS, WZ) | decide validity (Lean decides) | JCS records with a `lean_template` hint |
| `ks-print` / `ks-parse` | `str`, `srepr`, `latex` printers matching SymPy's output order; `sympify` string parser | define storage order | printing order is separate from storage order (as in SymPy) |
| `ks-lambdify` | compile Expr into a straight-line **tape** IR (register bytecode) | generate kernel source per expression | shared by the CPU interpreter and the GPU tape kernel |
| `ks-accel` | kernel trait (batched, typed); CPU reference implementation (scalar in WASM, rayon natively) | carry state between calls; time-based dispatch | trait objects behind `Accel` |
| `ks-accel-cubecl` | CubeCL kernels: NTT, modular GEMM/elimination, F4 reduce, CRT batch, tape interpreter (f32/f64) | float atomics; autotune on float kernels | `#[cube]` kernels, fixed-tree reductions |
| `ks-wire` | canonical node table; RFC 8785 JCS encoder; SHA-256 identity; request/response/provenance envelopes; stdio framing | depend on any math crate except `ks-expr`/`ks-num` | hand-written JCS encoder with the RFC test vectors |
| `ks-ops` | op registry (`science.cas.factor` → fn); step budget; `plan`/`map`/`reduce`; result normalisation; `SolveResult` status | know the target (R0/R1/R2) | `match op_id`, generated from the tool schema |
| `ks-component` | `wasm32-wasip2` component exporting `invoke` + `plan/map/reduce` | spawn threads, read clocks, use a network | `wit-bindgen` |
| `ks-worker` | R2 native stdio worker; rayon pool; device discovery inside the sandbox grant | open files outside its grant; network | the same `ks-ops` + `ks-accel-cubecl` |
| `standin/sympy` | pure-Python stand-in (Monty subset): local construction, canonical auto-eval mirror, tool-call marshalling, printing | inherit, use `@property` or generators, import non-stdlib modules | one `Expr` class with a kind tag |
| `prover/` | lean-prover `.harness`, skills, premise-index recipes, Lean package (polyrith backend over `kleinCall`), Lean env manifest content | contain the Lean TCB or `lean.certify` (those stay in klein-rs) | data + Lean sources |
| `conformance/` | oracle container recipes, corpus fetch recipes + digests, golden manifests, `ks-conform` runner (R0/R1-emulator/R2 byte compare; golden semantic compare; adjudication list) | commit third-party corpus bytes; ship any oracle | Rust runner + Containerfiles |
| `xtask` / `plugin/` | reproducible builds per triple; `klein.plugin.v1` manifests; signing; digest publication | hold signing keys in git | `cargo xtask dist` |

---

## How comparable permissive CASes layer their core, domains and polys

| System (licence) | Core | Numbers | Domains / polys | Lesson for klein-science |
|---|---|---|---|---|
| **SymPy** (BSD-3) | `Basic`/`Expr` trees; automatic evaluation in constructors (`eval` classmethods); `func(*args) == expr` invariant; `compare`/`sort_key` with a class-rank list (numbers < symbols < Pow < Mul < Add < functions) | Python int, or gmpy2 `mpz`, or python-flint `fmpz` as "ground types" | Separate layer: `Domain` objects (ZZ, QQ, GF(p), ZZ[x], QQ(x), QQ<α>, EX). Domain elements are *not* `Expr`; conversion goes through `construct_domain`/`from_sympy`/`to_sympy`. Dense DUP/DMP lists back `Poly`; sparse `PolyElement` dicts back `ring()`. `DomainMatrix` wraps DDM (dense) or SDM (sparse) or DFM (flint) | Copy the Expr/Domain split and the EX fallback. Copy `DomainMatrix` as the algorithm layer under `Matrix`. Do **not** copy `evaluate=False` (SymPy's own docs call it fragile and bug-prone). Keep printing order separate from storage order |
| **SymEngine** (MIT) | C++ `Basic` with few methods; behaviour via visitors and single dispatch; refcounted `RCP`; `Add`/`Mul` = numeric coefficient + dictionary; constructors accept *already canonical* data and check `is_canonical()` only under assertions; helpers `add()/mul()/pow()` canonicalise; cached hash + `__eq__`; `compare()` for canonical order | pluggable integer backend (GMP, FLINT, boost::mp) | separate univariate/multivariate poly classes; optional FLINT/Piranha backends | Copy the "trusted constructor + canonicalising helper + debug `is_canonical`" split and the "few methods on the node, visitors elsewhere" style. Do **not** copy hash-ordered dictionaries as the observable term order: hash-map order is not a portable canonical order |
| **FriCAS** (BSD-3) | Category/domain tower (SPAD); `Expression R` is a field of fractions of `SparseMultivariatePolynomial(R, Kernel)`, so function applications are kernels that act as generators | built-in big integers | Sparse *recursive* multivariate polys, variable order by list position; generic algorithms over categories (Ring, Field, EuclideanDomain, GcdDomain) | Copy the **kernel-field view for Risch and for `together`/`cancel`**: convert Expr into a rational function over kernels, compute, convert back. Use a Rust `Domain` trait hierarchy (Ring → EuclideanDomain → Field) as the analogue of the category tower |
| **Diofant** (BSD-3) | SymPy-derived Python CAS, still active (0.15 released, 0.16 in development) | gmpy2 ground types (recalled, not verified this pass) | Reported to have consolidated on the sparse `PolyElement` representation and dropped much of SymPy's legacy poly code (LOW: recalled, not verified this pass) | A cleaner port source than SymPy for polys where the code is equivalent; confirm module by module before porting, and keep its notice |

**Synthesis.** Every successful design has three layers:
1. A canonicalising expression core.
2. A domain/coefficient layer where all the fast algebra happens on typed elements.
3. User-facing algorithms (solve, integrate, simplify) that convert into the domain layer, compute, and convert back.

The klein-science crate DAG mirrors exactly this, with one addition the others lack: a hard separation between "pure
math" and "how it runs" (`ks-ops` and the shells), which the R0/R1/R2 identity requirement needs.

---

## Recommended Project Structure

```
klein-science/
├── Cargo.toml                    # workspace; [workspace.lints] with clippy::disallowed_types for semantic crates
├── clippy.toml                   # disallowed-types: std::collections::{HashMap,HashSet}, f64::sin… (use ks-libm)
├── NOTICE                        # SymPy, mpmath, Diofant, FriCAS (BSD-3), SymEngine (MIT) notices
├── wit/
│   └── science-cas.wit           # world: invoke + plan/map/reduce; payloads are JCS bytes (see Pattern 4)
├── schema/
│   ├── tool/science.cas.*.json   # typed tool schemas (the "one typed API"); source of truth for ks-ops codegen
│   ├── expr.v1.json              # klein.science.expr.v1 node-table schema
│   └── functions.toml            # function table spec → ks-functions, ks-print, standin, Lean names
├── crates/
│   ├── ks-num/                   # core numbers + prime table
│   ├── ks-libm/                  # re-export of one pure-Rust libm for every CPU float path (R0 = R2 bitwise)
│   ├── ks-expr/                  # DAG, interner, canonical order, auto-eval, subs, diff, core assumptions
│   ├── ks-functions/             # function table + eval rules
│   ├── ks-mp/                    # mpmath libmp port (evalf, special functions numerics)
│   ├── ks-wire/                  # node table ⇄ JCS, SHA-256 identity, envelopes, stdio framing
│   ├── ks-poly/                  # domains, dense/sparse polys, gcd, factor, resultants, roots
│   ├── ks-dmat/                  # DomainMatrix (dense/sparse) — needed by M2, not just M5
│   ├── ks-groebner/              # Buchberger → F4
│   ├── ks-ntheory/
│   ├── ks-simplify/              # engine only; rules/strategy are data under data/
│   ├── ks-solve/
│   ├── ks-calculus/
│   ├── ks-matrix/                # Expr-level Matrix API
│   ├── ks-sets/ ks-logic/ ks-combinat/      # M5
│   ├── ks-print/ ks-parse/
│   ├── ks-lambdify/              # Expr → tape IR
│   ├── ks-cert/                  # certificate records
│   ├── ks-accel/                 # kernel trait + CPU reference implementation
│   ├── ks-accel-cubecl/          # native-only CubeCL kernels (feature-gated per backend)
│   ├── ks-ops/                   # op registry, step budget, plan/map/reduce, normalisation
│   ├── ks-numerics/              # quadrature, ODE, root finding tool ops (science.numerics surface)
│   ├── ks-component/             # wasm32-wasip2 shell (R0, R1)
│   └── ks-worker/                # native stdio shell (R2)
├── data/
│   ├── simplify/                 # rule sets + strategy order (bundle data; digest in manifest)
│   └── integrate/                # integral tables as data
├── standin/
│   ├── sympy/                    # pure-Python stand-in (Monty subset), one package tree
│   ├── host_shim/                # CPython-only dev shim: routes tool calls to R0 (wasmtime) or R2 for tests; never shipped
│   └── lint/                     # Monty-subset AST lint (no inheritance, no @property, no yield, stdlib-only imports)
├── prover/
│   ├── harness/                  # lean-prover .harness + skills (moved from klein-rs)
│   ├── premise-index/            # recipes
│   ├── lean/                     # Lake package: polyrith backend via kleinCall, certificate templates
│   └── env-manifest/             # Lean environment manifest content (once the gate accepts envs by digest)
├── conformance/
│   ├── oracles/                  # Containerfiles: CPython+SymPy, SymEngine, Giac/FLINT binaries (offline only)
│   ├── corpora/                  # fetch recipes + digests ONLY (TIR, SymPy tests, LLM corpora)
│   ├── golden/                   # golden *manifests* (digests); golden bytes in an artifact store
│   ├── adjudication/             # cases where SymPy is wrong, with reasoning
│   └── ks-conform/               # runner: R0 / R1-emulator / R2 byte compare; golden semantic compare
├── plugin/
│   ├── science.cas/manifest.toml
│   ├── science.numerics/manifest.toml
│   └── science.prover/manifest.toml
├── testdata/                     # neutral fixtures
└── xtask/                        # dist, sign, digest, reproducibility check
```

### Structure Rationale

- **`crates/` is flat, with one concern per crate.** Each crate maps to one SymPy package or layer, so porting can be
  tracked module by module against SymPy's test suite. Cargo enforces the layer DAG: a cycle is a compile error.
- **`schema/` is the single source for the tool API and the function table.** `ks-ops` dispatch, the stand-in's Python
  wrappers, the printers and the Lean name map are generated from it. This is one mechanism per concern, and it stops the
  Python and Rust sides drifting apart.
- **`data/` is separate from code.** Simplification strategy is bundle data (single source §3), so autoresearch can change
  it without touching crates. Its digest goes into the manifest and therefore into the lock.
- **`standin/host_shim` exists because Monty cannot yet run the stand-in.** It lets SymPy's own tests and the TIR corpus
  run under CPython against the real R0/R2 *now*. Results produced this way are labelled "proxy", never "in-Klein".
- **The shells (`ks-component`, `ks-worker`) are tiny.** Anything that grows in them is a determinism risk; it belongs in
  `ks-ops`.

### Crate dependency DAG (edges point to dependencies)

```
ks-num ◄── ks-expr ◄── ks-functions ◄── ks-print, ks-parse, ks-lambdify
  ▲          ▲   ▲            ▲
  │          │   └── ks-wire  │
  │          │                │
ks-libm ◄── ks-mp ◄───────────┘ (numeric hooks)
  │
ks-accel ◄── ks-accel-cubecl (native only)
  ▲
  ├── ks-ntheory ◄── ks-poly ◄── ks-dmat ◄── ks-groebner
  │                    ▲            ▲             ▲
  │                    └────────────┴─────────────┤
  │                                     ks-simplify ◄── ks-solve ◄── ks-calculus
  │                                                       ▲             ▲
  │                                     ks-matrix ────────┘             │
  │                                     ks-cert ◄── (poly, groebner, ntheory, solve, calculus)
  └── ks-numerics (quadrature/ODE ensembles)
ks-ops ──► every math crate + ks-wire + ks-accel   (the only crate that knows the op set)
ks-component ──► ks-ops (+ ks-accel CPU only)      ks-worker ──► ks-ops + ks-accel-cubecl
```

Rule: `ks-poly`, `ks-dmat`, `ks-groebner` and `ks-ntheory` must not depend on `ks-expr` *except* through a thin
`convert` module (`from_expr`/`to_expr`) at the top of `ks-poly`. That keeps the algebra layer free of Expr allocation, as
in SymPy.

---

## Architectural Patterns

### Pattern 1: Closed kinds, canonical structural order, auto-evaluation spec

**What.** `Kind` is a closed enum:
- numbers: `Int`, `Rat`, `MpFloat`, plus constants `Pi`, `E`, `I`, `oo`, `zoo`, `nan`;
- `Sym` (name + assumption set, both part of identity, as in SymPy);
- `Add`, `Mul`, `Pow`;
- `Fn(fid)` for named functions from the table, and `UFn(name)` for undefined functions;
- `Rel`, `Piecewise`, `Tuple`, `Matrix`, `Set*`, `Bool*`, `Derivative`, `Integral`, `Sum`, `Limit`, `Order`, `Dummy`.

Constructors are of two kinds, as in SymEngine:
- `Add::canonical(args)` is a helper that flattens, folds numeric coefficients, collects like terms and sorts;
- `Add::from_canonical(args)` is trusted, with `debug_assert!(is_canonical)`.

The order is a **structural total order**:
1. kind rank;
2. for numbers, value;
3. for symbols, name bytes then assumption bytes;
4. otherwise argument count, then children compared lexicographically.

Each node caches a small "order prefix" (kind rank + first-child summary) to make most comparisons O(1).

**Why structural rather than hash-based.** The Python stand-in must reproduce the same order in pure Python for local
`==` and `hash` (Monty's `__eq__`/`__hash__` cannot call the host). A structural order is easy to mirror exactly. A 128-bit
platform hash would also be deterministic, but it would force a pure-Python xxh3/SipHash implementation and would make
storage order inscrutable when debugging.

**Auto-evaluation is a written spec** (`schema/autoeval.md`) that both `ks-expr` and the stand-in implement. Its scope is
SymPy's constructor-time evaluation only: flatten, numeric folding, `x + x → 2*x`, `x*x → x**2`, `Pow` integer folding,
and the assumption-dependent `Pow`/`Abs` rules. Everything else is `doit()`/`simplify` and runs only in Rust. A
differential property test (Rust versus stand-in on generated expressions) gates the spec, and its witness must be seen
RED first.

**Trade-offs.**
- Structural compare is O(size) in the worst case. Mitigated by the cached prefix and by the fact that most Add/Mul
  operands differ at depth 1.
- Printing order differs from storage order; `ks-print` sorts for display, as SymPy does.

```rust
#[derive(Copy, Clone, PartialEq, Eq, Hash)]   // deliberately NO PartialOrd/Ord
pub struct ExprId(u32);

pub fn cmp(ctx: &Ctx, a: ExprId, b: ExprId) -> Ordering { /* structural; never a.0.cmp(&b.0) */ }
```

### Pattern 2: Request-scoped arena + interner (hash-consing without global state)

**What.**
- Each tool invocation creates a `Ctx` with an arena (`Vec<Node>`, children in a side `Vec<ExprId>`) and an interner
  keyed by `(kind, payload, children)`. The interner's hash is internal and never observable.
- The arena is dropped at the end of the request, so no GC is needed.
- R0 and R1 instances are single-threaded, so no atomics are needed in WASM.
- R2 uses either per-worker `Ctx` plus wire merges (plan/map/reduce), or a sharded concurrent interner for intra-op
  parallelism (parallel Monagan–Pearce). That is the "lock-free concurrent interning keyed by content" idea kept from the
  HVM review.

**When.** Always. A process-global interner is the wrong default: it makes results depend on what ran before, and it
leaks memory across requests.

**Trade-offs.** There is no cross-request sharing. If memoisation across calls is wanted later (for example, `factor` by
input digest), it is a cache in `ks-ops` keyed by the SHA-256 identity. It lives outside the arena and replays bytes, so it
stays deterministic.

### Pattern 3: Canonical DAG node table as wire format; identity = SHA-256(JCS(table))

**What.** `klein.science.expr.v1`:

```json
{"nodes":[["Sym","x",{}],["Int","2"],["Pow",0,1],["Int","1"],["Add",3,2]],"root":4,"v":1}
```

- Nodes are emitted in deterministic post-order DFS from the root, visiting children in canonical order. Each node is
  emitted once, at its first occurrence, and children are referenced by index.
- Given a canonical DAG, this table is **unique**.
- **All numbers are strings.** That covers integers, numerators and denominators, MP-float mantissas, exponents and
  precision. RFC 8785 serialises JSON numbers as IEEE-754 doubles and says values outside that range must be wrapped as
  strings.
- The only JSON numbers are node indices (< 2^32, so exact in a double).
- The JCS encoder is hand-written, around 200 lines. It must pass the RFC's sorting and escaping vectors and must reject
  lone surrogates. A Python mirror of the encoder lives in the stand-in for local identity.

**Envelopes.**
```
request  = {op, v, args:{name: expr-table | scalar-string | list}, budget:{steps}, opts}
response = {status: exact|unevaluated|partial|fuel_exhausted, complete, certifiable,
            numeric_class: exact|approximate, result, certificate?}
record   = {request_digest, response_digest, provenance:{impl, backend, device, driver, wall_ms}}
```

Identity covers `request` and `response` only. `provenance` is **outside** the hashed bytes, otherwise R0 and R2 could
never be byte-identical.

**Why a table, not a tree.** A literal JCS tree expands shared subterms exponentially (for example, `a = a*a + a`
iterated 60 times). The table is linear in DAG size and is still a canonical serialisation of the expression tree. See
"Disagreements and clarifications".

### Pattern 4: One typed API, bytes at the component boundary

**What.** WIT has no recursive types (official component-model docs), so an expression cannot be a WIT variant tree.
Rather than maintaining a WIT-typed node table *and* a JSON codec for R2's stdio, use **one codec**:

```wit
package klein:science@0.1.0;

interface cas {
  record budget { steps: u64 }
  variant error { bad-request(string), unsupported-op(string), internal(string) }
  invoke: func(request-jcs: list<u8>) -> result<list<u8>, error>;

  // R1 support: same artifact, pure functions
  plan:   func(request-jcs: list<u8>) -> result<list<list<u8>>, error>;     // pieces, index-ordered
  map:    func(piece: list<u8>) -> result<list<u8>, error>;                  // pure
  reduce: func(request-jcs: list<u8>, outputs: list<list<u8>>) -> result<list<u8>, error>;
}
world science-cas { export cas; }
```

R2's stdio frames carry the same JCS bytes (`u32` length prefix + bytes). The "typed" part of the API is the tool JSON
Schema in `schema/tool/`, which Klein validates at the door. The plan/map/reduce shape is a **proposal for the klein-rs R1
D-R contract**, not something this repo can decide.

**Trade-offs.** WIT-level typing of arguments is lost. In exchange the bytes compared in conformance are exactly the bytes
on the wire on all three targets, and the R0-versus-R2 codec divergence class disappears.

### Pattern 5: plan → map → reduce shared by all three implementations

**What.** For operation classes with natural decomposition, `ks-ops` exposes `plan(request) → [piece]`, a pure
`map(piece) → output`, and `reduce(request, [output]) → response`:

| Operation class | Pieces | Reduce |
|---|---|---|
| multi-modular integer det/rank/solve | one or more primes from the fixed prime table | CRT + rational reconstruction |
| modular GCD (Brown/Zippel) | evaluation points × primes | interpolation + CRT |
| F4 over Q | primes | CRT/rational reconstruction + a verification step |
| block matrix multiply | blocks | concatenation |
| simplify | rule sets | canonical min by (cost, structural order) |

- **R0** executes `plan`, then `map` sequentially, then `reduce`.
- **R1** is Klein's map: N instances, merged in index order.
- **R2** runs `par_iter().map()` and collects in index order. It may also substitute a GPU batch for a group of pieces,
  through `ks-accel`.

Byte identity follows from `map`'s purity plus index-ordered collection.

**Termination rules must be deterministic and identical.** "Stop when the CRT image is stable for 2 consecutive primes"
is acceptable only if the prime sequence and batch boundaries are fixed by `plan`. A rule like "until the time budget
expires" is never acceptable. Prefer a-priori bounds (Hadamard, Mignotte) where they are affordable.

### Pattern 6: Step budget, not wasm fuel, decides `fuel_exhausted`

**What.** Every loop in heuristic or unbounded algorithms charges a `Budget` in abstract steps: e-graph iterations, Risch
recursion depth, Gruntz rewrite steps, factor-combination subsets, Gröbner S-pairs. When the budget runs out, the
algorithm returns `fuel_exhausted` with the partial result.
- These counts are identical on R0, R1 and R2 because they are part of the algorithm.
- Wasmtime fuel is set strictly higher as a safety backstop. If wasm fuel trips, the result is an *error*, never a
  `fuel_exhausted` result, because native R2 has no equivalent.

### Pattern 7: Normalise every non-unique output before encoding

Determinism of exact results also requires normalising outputs that the mathematics leaves ambiguous:
- factor lists sorted by (degree, canonical order) with unit and content normalised;
- polynomial roots in canonical order (real roots ascending via isolation intervals, then complex ones);
- `solve` dicts sorted;
- Gröbner bases reduced and monic, sorted by leading monomial;
- RREF and HNF unique forms;
- `Piecewise` conditions canonicalised;
- egglog extraction ties broken by structural order, never by e-class ID.

Randomised algorithms (Cantor–Zassenhaus, Pollard rho, ECM, probabilistic primality preprocessing) use a counter-based
PRNG (for example ChaCha8) seeded from the SHA-256 of the request. With that, R0 sequential and R2 parallel draw the same
streams per piece.

---

## GPU offload boundary

### Kernel set (closed, batched, typed)

| Kernel | Domain / types | Used by | GPU backends | CPU fallback (always present) | Notes |
|---|---|---|---|---|---|
| `ntt_mul_batch` | Z/p, p < 2^31 NTT-friendly (p = c·2^k+1), u32 Montgomery | big-int multiply (3-prime CRT), dense poly multiply over Z and Z/p | Metal, WGSL, CUDA, HIP | `ks-accel` CPU NTT (same primes, same transform order) | Results are exact; identical by construction. Thresholds by size only |
| `modmat_elim_batch` | dense Z/p matrices, u32 | det, rank, nullspace, rref per prime; multi-modular integer LA | all | CPU elimination (rayon natively, scalar in WASM) | Pivot rule fixed (first non-zero in column order) so the per-prime output is unique |
| `modmat_mul_batch` | Z/p GEMM, u32 | block LA, Wiedemann (later) | all | CPU | Do not use the float-BLAS modular trick: it needs f64 (absent on Metal) and its best-known implementations are LGPL |
| `f4_reduce` | sparse-dense Macaulay matrix mod p | `ks-groebner` F4 | all | CPU | Symbolic preprocessing (monomial selection, column order) stays on the CPU; only the reduction goes to the GPU |
| `crt_batch` | vectors of residues → Z via a fixed prime list | all multi-modular reduces | all | CPU | Garner's algorithm with a fixed prime order |
| `tape_eval_f32` / `tape_eval_f64` | straight-line tape over a point grid | `lambdify` grids, batch `evalf` at machine precision, quadrature node evaluation | f32: all; f64: CUDA/HIP only | CPU tape interpreter using `ks-libm` | **One interpreter kernel**, so no kernel generation per expression and the CubeCL cache is pinnable by digest |
| `ode_ensemble_f64` | fixed-step or embedded RK, many initial values | `science.numerics` | CUDA/HIP | CPU rayon | float: tagged approximate, fixed-tree reductions |

**Stays on the CPU** (single source §4, confirmed by the structure of the algorithms): rewriting, auto-evaluation, pattern
matching, egglog, Risch, Gruntz, sparse polynomial arithmetic (Monagan–Pearce), Hensel lifting and factor recombination,
and symbolic preprocessing for F4.

### Backend constraints that shape the kernels

- **Metal has no FP64.** This is confirmed by Apple forum statements, by PyTorch MPS and by MLX maintainers (MEDIUM: no
  Apple spec page was fetched). So `tape_eval_f64` and `ode_ensemble_f64` dispatch to the CPU on a Mac.
- **On a Mac, `evalf` never silently uses f32 on the GPU.** The f32 GPU path is used only when the caller explicitly asks
  for f32 (`lambdify(..., dtype=float32)`-style).
- **64-bit integers.** WGSL has no u64. MSL has 64-bit integers, but whether CubeCL's wgpu/MSL path exposes them is
  undocumented (LOW). Hence the design rule: **all exact GPU kernels use 32-bit primes and 32-bit limbs with mulhi**, so
  one kernel source serves Metal, Vulkan, CUDA and HIP. u64 variants for CUDA/HIP are an optimisation, admitted separately
  by conformance.
- **CubeCL is alpha**, with API changes between minor versions. Pin the exact version. Ship the warm kernel cache and the
  autotune bundle by digest; CubeCL supports shipping a pre-warmed environment bundle.
- **Autotune.** Autotune is allowed for exact kernels, because any configuration gives the same result. It is **disabled
  or pinned** for float kernels, because a different tile shape changes the reduction tree.
- **Unified memory (36 GB on the M3 Max).** GPU batches must have a size cap from the sealed config, so that a large
  Macaulay matrix cannot push the machine into paging.

### Dispatch rule

`ks-ops` asks `ks-accel` for an `Accel` handle:
- R0 and R1 get `CpuAccel` (scalar).
- R2 gets `CubeclAccel` if the device probe succeeds inside the `ComputeDevice` grant, otherwise `CpuAccel` (rayon).

Size thresholds come from sealed configuration, never from timing. For exact kernels the choice cannot change output; for
float kernels the choice is recorded in provenance and replay serves the recorded value.

---

## Data Flow

### Request flow (forward computation)

```
LLM SymPy code (Monty guest)
  │ x = Symbol('x'); p = x**2 - 2          ← local construction, stand-in auto-eval, local identity
  ▼
stand-in: factor(p) ─► encode node table (Python JCS) ─► tool call science.cas.factor
  ▼
Klein door: schema validation ─► lock says {R0,R2} ─► chooses per sealed policy
  ▼
shell (component | worker): JCS bytes ─► ks-wire decode ─► Ctx arena
  ▼
ks-ops: budget → plan → map (ks-poly via ks-accel) → reduce → normalise → SolveResult/Expr
  ▼
ks-wire encode ─► response JCS (+certificate) ─► Klein records {request_digest, response_digest, provenance}
  ▼
stand-in: decode table ─► rebuild via stand-in constructors (idempotent) ─► Python Expr
```

### R1 flow

```
Klein ─► component.plan(req) ─► [p0..pN-1]
      ─► N fresh instances: map(p_i) (parallel, pure) ─► [o0..oN-1] merged in index order
      ─► component.reduce(req, [o_i]) ─► response
Conformance: bytes(response_R1) == bytes(response_R0) == bytes(response_R2), witness seen RED.
```

### Lean pairing (both directions)

```
CAS finds, Lean checks:
  science.cas.<op> ─► response{certifiable:true, certificate:{kind: ideal_membership|factor_product|pratt|
                       root_witness|sos|wz|linear_combination, data, lean_template}}
  ─► prover harness (Lean-mode child role, prover/harness) builds sealed statement + proof from template
  ─► lean.certify (klein-rs TCB) decides ─► certified | rejected      (the CAS never claims "certified")

Lean asks, CAS answers:
  Lean goal ─► tactic in prover/lean (e.g. polyrith backend) ─► kleinCall "science.cas.ideal_membership"
  ─► same tool API ─► certificate (cofactors) ─► `linear_combination` closes the goal ─► Lean kernel checks
```

### Conformance flow (offline only)

```
corpus fetch recipe + digest ─► fetch into an artifact store (never git)
  ─► oracle container (CPython+SymPy | SymEngine | Giac/FLINT) ─► golden outputs, pinned by digest
  ─► ks-conform: run R0 (wasmtime embed), R1-emulator, R2 worker on the same requests
       (1) byte compare across implementations  (2) semantic compare vs golden  (3) property tests
       (4) adjudication list overrides
  ─► admission report per operation class (an op class is admitted to R1/R2 only after (1) passes, witness seen RED)
```

The R1-emulator in `ks-conform` is a test tool (wasmtime, N instances, index-order merge). It lets map purity be tested
before Klein's R1 lands. **Admission still requires re-running against Klein's real R1.**

---

## Build Order (dependencies and what klein-rs blocks)

### External dependencies (klein-rs requests)

| # | Dependency | Blocks in klein-science | Does NOT block |
|---|---|---|---|
| D1 | `ComputeDevice` authority (Linux devices; macOS Seatbelt IOKit/Metal, still to be measured) | admitting any R2 GPU operation class inside Klein; R2 conformance *in the sandbox* | writing CubeCL kernels; running the R2 worker unsandboxed on the Mac for development and measurement |
| D2 | R1 deterministic parallel WASM map | admitting operation classes to R1 | the plan/map/reduce design, `ks-ops`, testing with the R1-emulator |
| D3 | `PluginArchive` provisioning by digest; lock v3 | shipping and installing signed bundles | reproducible builds, manifests, signatures, digests in CI |
| D4 | Monty: plugin components (Skill/Harness/Data) **and** Python guest features (see "Integration Points") | running the stand-in inside Klein; in-Klein TIR pass rates | stand-in development and the TIR proxy under CPython via `host_shim` |
| D5 | `lean.certify` + Lean guest engine (Klein 04.1) | `science.prover`; the "Lean loop working" pass criterion of M4 | `ks-cert`; offline certificate checking with a pinned Lean+Mathlib in the oracle container |

### Suggested order (waves; each wave is buildable once the previous one lands)

**Wave 0 — M0 spike (no klein-rs needed, except measuring D4 if a Monty build is available).**
1. `ks-wire`: JCS encoder plus RFC vectors, node table, identity, envelopes. This is the contract every other piece tests
   against.
2. `wit/science-cas.wit` + `ks-component` + `ks-worker`, with one trivial operation (`expand` on integer polynomials)
   end to end, and a byte compare between R0 and R2 (the first RED witness).
3. Oracle pipeline: CPython + SymPy container, golden output for a TIR slice, digests.
4. CubeCL Metal probe (unsandboxed): `ntt_mul_batch` u32, plus a check of which integer widths compile on MSL. Write the
   D1 Seatbelt requirements down from what the probe touches.
5. Stand-in skeleton plus the Monty-subset lint, plus `host_shim`. Write the precise D4 feature request (dunders,
   properties, inheritance alternative, module provisioning, big ints).

**Wave 1 — M1 core.**
- Order: `ks-num` → `ks-expr` (auto-eval spec, core assumptions) → `ks-functions` → `ks-mp` → `ks-print`/`ks-parse` →
  `ks-lambdify` + `tape_eval` CPU → `diff`/`subs`/`evalf`.
- The stand-in mirrors the auto-eval spec in parallel, with a differential test gate.

**Wave 2 — M2 algebra.**
- `ks-ntheory` (primes first; modular algorithms need the prime table) → `ks-poly` domains + dense + sparse → gcd →
  factor.
- In parallel: `ks-dmat` (needs only domains), then `ks-groebner` (needs `ks-poly` + `ks-dmat`).
- `ks-accel` exact kernels (NTT, modmat, CRT, F4 reduce): CPU reference first, CubeCL second. The first R1/R2 admissions
  are multi-modular determinant and big-int multiply.
- `ks-cert` v1 (factor product, Pratt, ideal-membership cofactors), checked offline in Lean.

**Wave 3 — M3.** `ks-simplify` (egglog bounded, strategy data), `ks-solve` (needs poly, groebner and dmat), Piecewise,
new-style assumptions (`ask`/`refine`).

**Wave 4 — M4.** `ks-calculus` (series → Gruntz → integrate ladder → Gosper → ODE), `ks-numerics` (quadrature, ODE
ensembles), and `prover/` with the polyrith backend. **The M4 pass criterion is hard-blocked on D5.**

**Wave 5 — M5.** `ks-matrix` Expr API over `ks-dmat`, sets, logic, combinatorics; GPU dense LA budgets.

**Critical path:** `ks-wire` → `ks-expr` → `ks-poly` → `ks-dmat`/`ks-groebner` → `ks-solve` → `ks-calculus`.

Everything with a GPU sits off the critical path behind `ks-accel`'s CPU reference implementation. A late D1 delays
budgets, not features.

---

## Scaling Considerations

Here, scale means problem size and corpus size, not users.

| Scale | Architecture adjustments |
|---|---|
| TIR-sized problems (most LLM programs: small expressions, low degree) | R0 alone; per-request arena; latency dominated by the door round-trip, so local stand-in construction matters more than kernel speed |
| Medium (degree 10^3–10^4 polys, 100×100 exact matrices, Gröbner of a few dozen polys) | R1 or R2 rayon via plan/map/reduce; Monagan–Pearce heaps; small-int fast paths |
| Large (10^6-degree NTT, 1000×1000 integer determinant, 10^8-point grids) | R2 GPU via `ks-accel`; batch caps from sealed config; f64 on the Mac goes to CPU rayon |

### Scaling priorities

1. **First bottleneck: expression swell and allocation in `ks-expr`**, not arithmetic. Fix with hash-consing, request
   arenas, cached order prefixes, and converting to the domain layer early (`expand` of a polynomial-shaped Expr should go
   through `ks-poly` sparse, as SymEngine and SymPy do).
2. **Second bottleneck: the door round-trip for chatty programs.** Fix it in the stand-in: local construction, and only
   heavy operations cross the door. If needed, a `batch` operation that evaluates several independent calls in one
   request.
3. **Third: unified-memory pressure on the Mac** during GPU batches. Fix with sealed batch caps and streaming Macaulay
   blocks.

---

## Anti-Patterns

### Anti-Pattern 1: Algebra on Expr trees
- **What people do:** implement gcd, factor or Gröbner directly on `Add`/`Mul` trees.
- **Why it's wrong:** it is orders of magnitude slower and makes normal forms ambiguous. SymPy itself moved its algebra
  into the domain layer.
- **Do this instead:** `construct_domain`, compute on typed elements, `to_expr` once.

### Anti-Pattern 2: Ordering or iterating by anything platform-dependent
- **What people do:** sort args by pointer, interning ID or `HashMap` iteration; use `usize` in encoded output; call the
  platform `libm`.
- **Why it's wrong:** WASM is 32-bit and single-threaded and native is 64-bit and parallel, so outputs diverge silently.
  The bug shows up only in cross-implementation conformance, late.
- **Do this instead:** structural order; `ExprId` without `Ord`; `clippy::disallowed_types`; a single pure-Rust libm via
  `ks-libm`; all encoded integers are decimal strings.

### Anti-Pattern 3: Using wasm fuel or wall time as the algorithmic budget
- **Why it's wrong:** R2 has no wasm fuel, and time differs per machine. `fuel_exhausted` results would then differ
  across implementations.
- **Do this instead:** CAS-level step budgets (Pattern 6).

### Anti-Pattern 4: Per-expression GPU kernel generation for `lambdify`
- **Why it's wrong:** it breaks "CubeCL kernel cache pinned by digest", opens a code-generation surface inside the
  sandbox, and causes JIT latency per call.
- **Do this instead:** one tape-interpreter kernel. Expressions are data.

### Anti-Pattern 5: Two codecs (WIT-typed for R0, JSON for R2)
- **Why it's wrong:** two encoders means two places for bytes to diverge, and conformance then compares decoded values
  instead of bytes.
- **Do this instead:** one JCS codec in `ks-wire`, used by all shells and mirrored once in Python.

### Anti-Pattern 6: `evaluate=False` and unevaluated-construction flags
- **Why it's wrong:** SymPy's own guide calls it fragile and bug-prone, because rebuilding from args loses the flag.
- **Do this instead:** minimal auto-evaluation; everything else is `doit()`/`simplify` operations.

### Anti-Pattern 7: Provenance inside the hashed result
- **Why it's wrong:** R0 and R2 can never be byte-identical if the bytes say which one ran.
- **Do this instead:** a separate provenance envelope (Pattern 3).

### Anti-Pattern 8: Porting GPL algorithm implementations "for reference"
- **What people do:** port msolve, FFLAS-FFPACK, Singular, FLINT or PARI code because they are the best implementations
  of F4 and modular LA.
- **Why it's wrong:** it breaks licence rule 2. GPL/LGPL binaries may only be offline oracles.
- **Do this instead:** implement from papers (Faugère's F4, Monagan–Pearce, van Hoeij), and port SymPy, Diofant,
  SymEngine and FriCAS where they cover the algorithm. (Symbolica is excluded from everything, including being read.)

---

## Integration Points

### External services (klein-rs; requests only, never edits)

| Service | Integration pattern | Notes / gotchas |
|---|---|---|
| **Monty guest (D4)** | stand-in loaded as plugin-provided Python; host function = the tool call | Monty's docs (fetched 2026-10-11) list as **not supported**: arithmetic and reflected dunders, ordering comparisons, `__call__`, `__getitem__`, `__bool__`, `__len__`, inheritance (rejected at parse time; `isinstance` matches the exact class only), `@property`/`@staticmethod`/`@classmethod`, generators, and imports beyond the bundled stdlib (no package system). `__eq__`/`__hash__`/`__repr__` cannot suspend for host calls. **The request must cover:** (a) arithmetic + reflected + unary dunders + `__pow__`/`__rpow__`/`__truediv__`; (b) ordering comparisons, returning a non-bool `Rel` object; (c) `__call__` for `Function('f')(x)` and `Lambda`; (d) `__getitem__` for `Matrix[i, j]`; (e) `__bool__` raising, as SymPy relationals do; (f) `@property` or `__getattr__` for `.args`, `.free_symbols`, `.is_*`; (g) plugin-provided module import (`import sympy`, `from sympy import *`); (h) confirming that Python ints are arbitrary-precision. The stand-in design (one `Expr` class with a kind tag, local auto-eval) works around missing inheritance either way |
| **R1 map (D2)** | component exports plan/map/reduce; kernel merges in index order | propose the WIT shape in Pattern 4 as the D-R contract |
| **ComputeDevice + ProcessSandbox (D1)** | R2 worker as a signed, locked stdio binary per target triple | the M0 probe produces the exact list of IOKit/Metal service lookups for Seatbelt; the kernel cache dir is read-only and digest-pinned; private tmp |
| **PluginArchive + lock v3 (D3)** | `xtask dist` emits component + native workers + manifest + Ed25519 signature + digest | one plugin index modelled on a marketplace index |
| **lean.certify + Lean guest (D5)** | prover harness turns certificates into sealed statements; Lean tactics call `kleinCall` | the CAS never asserts "certified"; `certifiable` only means "a template exists" |

### Internal boundaries

| Boundary | Communication | Notes |
|---|---|---|
| stand-in ↔ CAS | JCS node tables over one tool call per heavy operation | the stand-in re-canonicalises decoded results through its own constructors; differential test against `ks-expr` |
| `ks-expr` ↔ `ks-poly` | `construct_domain` / `to_expr` | the only place Expr enters the algebra layer |
| `ks-poly`/`ks-dmat`/`ks-groebner` ↔ `ks-accel` | batched typed calls (`&[u32]` residues, prime ids) | no Expr and no allocation policy leaks across |
| `ks-ops` ↔ shells | `fn invoke(&[u8]) -> Result<Vec<u8>>` + plan/map/reduce | shells contain no logic |
| `ks-simplify` ↔ `data/` | strategy file (digest-pinned) loaded at instance start | changing strategy changes the plugin digest, so the lock catches it |
| `ks-cert` ↔ `prover/` | certificate JCS records with `lean_template` | templates are data in `prover/lean` |
| `ks-numerics` ↔ `science.cas` | shared `ks-mp`, `ks-lambdify`, `ks-accel` | see the clarification on plugin packaging below |

---

## Disagreements and clarifications for the operator (single source is not modified)

1. **"SHA-256 over the RFC-8785 JCS canonical tree" (CLAUDE.md rule 4, single source §4).**
   - Read literally, a tree encoding is exponential in size for shared subterms.
   - **Recommendation:** identity = SHA-256 of the JCS of the *canonical DAG node table* (Pattern 3). That is a
     canonical, linear-size serialisation of the same tree.
   - Optional later: a Merkle per-node digest, for subterm caching.
   - This needs an operator ruling, because the rule text is binding.
2. **`science.numerics` as a separate signed plugin (§3) versus "numerics live in the same science plugin" (§8.6).**
   - **Recommendation:** one repo and one crate graph, with `ks-mp` shared, and two plugin manifests (`science.cas`,
     `science.numerics`) built from the same workspace.
   - `evalf` stays in `science.cas` because SymPy users expect it there.
   - Please confirm whether §8.6 means "same repo" or "same bundle".
3. **Matrices in M5.** Domain-level matrices (`ks-dmat`) are a prerequisite of M2 (F4, resultants, multi-modular LA) and
   M3 (linsolve). Recommend moving `ks-dmat` into M2 and keeping only the Expr-level `Matrix` API and the GPU LA budgets
   in M5.
4. **Assumptions in M3.** SymPy's constructor-time evaluation consults `is_*` assumptions. Recommend the core
   three-valued assumption system in M1 and `ask`/`refine` in M3.
5. **"Monty operator overloading" as M0 scope.** It is a klein-rs deliverable (D4), and Monty's current gap is wider
   than operator overloading (see "Integration Points"). Recommend that M0 *specifies and tests against* the request, and
   measures the TIR proxy under CPython in the meantime.
6. **Floats.** MP floats from `ks-mp` are computed with integer mantissas, so they are byte-reproducible across R0, R1
   and R2. The rule "floats tagged approximate and replayed" still holds and costs nothing. Hardware f32/f64 (GPU grids,
   ODE ensembles) is where replay is actually needed. No change requested; this note just clarifies where the risk is.
7. **Golden outputs and the no-corpus-bytes rule (rule 6).** Golden outputs embed corpus inputs. Recommend keeping
   golden bytes in the artifact store by digest and committing only manifests.

---

## Open questions / M0 probes

- Which integer widths CubeCL's wgpu/MSL path compiles on the M3 Max (u32 mulhi, u64), and the measured cost of a
  `ntt_mul_batch` at 2^20.
- The exact Seatbelt services the CubeCL Metal runtime touches (the D1 input; the single source's top risk).
- Whether egglog 2.x builds for `wasm32-wasip2`, and whether its extraction is deterministic single-threaded with custom
  tie-breaking (to verify before M3).
- Whether Monty integers are arbitrary-precision, and the latency of a host call from the guest (this sets how chatty the
  stand-in may be).
- Whether Klein instantiates a fresh component per call (this determines whether the per-request arena is free or needs
  explicit reset).
- Crate prefix: `ks-` is a placeholder; the naming is the operator's call.

---

## Sources

Official docs and specifications (fetched 2026-10-11; the seam's tier for web sources is LOW, so these are cross-checked
primary sources):
- [SymPy: Introducing the domains of the poly module](https://docs.sympy.org/latest/modules/polys/domainsintro.html): the
  Expr/Domain split, DUP/DMP, PolyElement, `construct_domain`, EX.
- [SymPy: DomainMatrix](https://docs.sympy.org/latest/modules/polys/domainmatrix.html): DDM/SDM, Bareiss `fflu`,
  Berkowitz, dense/sparse selection.
- [SymPy: Writing custom functions](https://docs.sympy.org/latest/guides/custom-functions.html): `eval`
  auto-evaluation; `evaluate=False` discouraged.
- [SymEngine design](https://symengine.org/design/design.html), [SymEngine Basic/Add API](https://symengine.org/symengine/classSymEngine_1_1Add.html):
  RCP, `is_canonical`, add/mul/pow helpers, coefficient + dictionary, compare.
- [RFC 8785 JSON Canonicalization Scheme](https://www.rfc-editor.org/rfc/rfc8785): number serialisation, big numbers as
  strings, UTF-16 key sort, I-JSON.
- [Component Model: An Overview of WIT](https://component-model.bytecodealliance.org/design/wit.html) and
  [packr README](https://docs.rs/crate/packr/0.5.1): no recursive types in WIT.
- [Wasmtime: Deterministic Wasm Execution](https://docs.wasmtime.dev/examples-deterministic-wasm-execution.html): NaN
  canonicalisation, deterministic relaxed SIMD.
- [Monty limitations: classes](https://pydantic.dev/docs/monty/limitations/classes/) and
  [language](https://pydantic.dev/docs/monty/limitations/language/): unsupported dunders, inheritance, properties,
  imports.
- [CubeCL README](https://github.com/tracel-ai/cubecl): runtimes (CUDA, HIP, wgpu/MSL, SPIR-V, WGSL, CPU), alpha status,
  kernel cache and autotune bundle.

Secondary (community, MEDIUM/LOW):
- [Apple Developer Forums: no FP64 in Metal](https://developer.apple.com/forums/thread/709663),
  [MLX FP64 emulation issue](https://upd.dev/ml-explore/mlx/issues/1905),
  [Dawn: 64-bit atomics on MSL](https://dawn.googlesource.com/dawn/+/b620b2fe514a5ee2e6a2d924fd998c514019da87).
- [FriCAS Expression sandbox](https://wiki.fricas.org/SandBoxExpr),
  [Expression in SPAD](https://www.euclideanspace.com/prog/scratchpad/spad/topics/expression/index.htm): Expression as
  SMP over kernels.
- [Diofant docs index](https://diofant.readthedocs.io/en/latest/), [Diofant repo](https://github.com/diofant/diofant):
  BSD-3, module list; poly consolidation is recalled, not verified.
- [Monagan & Pearce, parallel sparse multiplication](https://www.cecm.sfu.ca/~pborwein/MITACS/papers/ParPolyMul.pdf).
- [arXiv 2601.06765, GPU-targeted F4/F5 design](https://arxiv.org/abs/2601.06765) (also cited by the single source);
  [LIP6 work on msolve GPU linear algebra](https://hal.sorbonne-universite.fr/hal-04117304v1/document) (msolve is GPL:
  ideas only, no code).
- [cuPQC-NTT docs](https://docs.nvidia.com/cuda/cupqc/libraries/cupqc_ntt/cupqc_ntt_feature.html): 32-bit Montgomery
  NTT as industry practice.
- [egglog on docs.rs](https://docs.rs/crate/egglog/latest): MIT, single-threaded by default.
- [dashu](https://github.com/cmpute/dashu): pure Rust, `no_std`, MIT/Apache.

Symbolica was not read, searched for or consulted (binding rule 1).

---
*Architecture research for: klein-science (Rust CAS as signed Klein plugins)*
*Researched: 2026-10-11*
