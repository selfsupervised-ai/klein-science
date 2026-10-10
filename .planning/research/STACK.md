# Stack Research

**Project:** klein-science (`science.cas`, `science.numerics`, `science.prover`, all signed `klein.plugin.v1` bundles)
**Domain:** a licence-clean Rust computer-algebra system with a SymPy-parity target. It runs as R0 (a `wasm32-wasip2`
component), R1 (a kernel parallel map of R0) and R2 (a native rayon and CubeCL worker).
**Researched:** 2026-10-11
**Overall confidence:** MEDIUM-HIGH. Versions and licences are HIGH-grade facts. Accelerator and e-graph determinism
behaviour is MEDIUM and must be measured in M0.

## How confidence is reported here

- **Version and licence facts** come from the crates.io and PyPI registry APIs, queried on 2026-10-11. Some
  type-support facts come from reading the published crate sources (CubeCL 0.11.0, egglog 3.0.0). These are primary
  sources. The GSD `classify-confidence` seam rates them **MEDIUM**, its ceiling for verified fetches (`legitimacyVerdict:
  OK` for every recommended crate). Treat them as authoritative for what they state.
- **Recommendation confidence** (the "Rec." column) is a judgement:
  - **HIGH** means the choice is standard, or forced by a binding rule.
  - **MEDIUM** means the choice is right on current evidence but has to be confirmed by an M0 measurement.
  - **LOW** means a hypothesis.
- **Symbolica-clean.** No Symbolica source, docs, benchmarks or output were read, searched or used for this file. The
  name appears below only as a `cargo-deny` ban entry.

---

## TL;DR: the stack in one table

| Layer | Use | Version | Licence | Rec. |
|---|---|---|---|---|
| Toolchain | Rust stable, pinned in `rust-toolchain.toml` | **1.99.0** (2026-09-28) | MIT/Apache-2.0 | HIGH |
| Big integers | `dashu-int` | **0.6.2** | MIT OR Apache-2.0 | HIGH |
| Rationals | `dashu-ratio` | **0.6.1** | MIT OR Apache-2.0 | HIGH |
| Arbitrary-precision floats | **Own port of mpmath 1.3.0 `libmp`** on `dashu-int` mantissas | (port) | BSD-3 notice kept | HIGH |
| Float cross-check (dev only) | `astro-float` | 0.9.6 | MIT | MEDIUM |
| Expression DAG and interning | **Own arena and interner** on `hashbrown::HashTable` + `foldhash` | 0.17.1 / 0.2.0 | MIT OR Apache-2.0 / Zlib | HIGH |
| Small vectors | `smallvec` | 1.16.3 | MIT OR Apache-2.0 | HIGH |
| Sparse multivariate polynomials | **Own** (port of SymPy `polys`, with SymEngine ideas) | (port) | BSD-3 / MIT notices | HIGH |
| E-graph simplification | `egglog` (`default-features = false`, single thread) | **3.0.0** | MIT (but pulls `im-rc`, MPL-2.0+, see flag F1) | MEDIUM |
| Exact dense linear algebra | **Own** `ks-dmat` (Bareiss, multi-modular CRT, Wiedemann) | (own) | none | HIGH |
| Float dense linear algebra | `faer` | **0.24.4** | MIT | HIGH |
| CPU parallelism (R2) | `rayon` | 1.12.0 | MIT OR Apache-2.0 | HIGH |
| GPU compute (R2) | `cubecl`, feature `metal` (wgpu runtime + MSL); later `cuda` and `hip` | **=0.11.0** | MIT OR Apache-2.0 | MEDIUM |
| GPU portability layer | `wgpu` (pulled in by `cubecl-wgpu`) | 30.0.1 | MIT OR Apache-2.0 | HIGH |
| NTT | **Own** `ks-ntt`: 31-bit primes, u32 Montgomery, one algorithm for WASM SIMD128, CPU and `#[cube]` | (own) | none | MEDIUM |
| WASM guest bindings | `wit-bindgen` + `cargo build --target wasm32-wasip2` | **0.62.0** | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | HIGH |
| WASM tooling | `wasm-tools` (validate, `component wit`, strip), `wkg` (WIT packages) | 1.261.0 / 0.16.1 | Apache-2.0 WITH LLVM-exception (+ OR Apache/MIT for wasm-tools) | HIGH |
| Canonical JSON | `serde_json_canonicalizer` (**the same crate and pin as klein-rs**) | **=0.3.2** | MIT | HIGH |
| SHA-256 | `sha2` (same pin as klein-rs) | **=0.11.0** | MIT OR Apache-2.0 | HIGH |
| Ed25519 signing | `ed25519-dalek` (same pin as klein-rs) | **=3.0.0** | BSD-3-Clause | HIGH |
| Deterministic randomness | `rand_chacha` (ChaCha20, seed derived from the input digest) | 0.10.0 | MIT OR Apache-2.0 | HIGH |
| Tests | `proptest`, `insta`, `cargo-nextest` | 1.12.0 / 1.49.0 / 0.9.148 | MIT/Apache, Apache-2.0 | HIGH |
| Benchmarks | `criterion` | 0.8.2 | Apache-2.0 OR MIT | HIGH |
| Licence and supply chain | `cargo-deny`, `cargo-about`, `cargo-auditable` | 0.20.2 / 0.9.2 / 0.7.7 | MIT OR Apache-2.0 | HIGH |
| Offline oracle | Docker + `python:3.13-slim-trixie@sha256:…` + `uv` 0.13.0 + **SymPy 1.14.0 + mpmath 1.3.0** + `rfc8785` 0.1.4 | — | BSD / Apache-2.0 (never shipped) | HIGH |

---

## Recommended Stack

### Core Technologies

| Technology | Version | Purpose | Why recommended |
|---|---|---|---|
| **Rust stable** | 1.99.0, pinned exactly in `rust-toolchain.toml` | every crate, all three implementations | It clears every MSRV in this stack: wasmtime and cranelift need 1.97, nalgebra and `wide` 1.89, `cubecl` uses edition 2024. klein-rs follows a "latest stable" directive (D-01, currently 1.96). Pin 1.99.0 here and bump it on the same rhythm. Reproducible `wasm32-wasip2` output needs an exact toolchain pin, not a channel. |
| **`wasm32-wasip2`** (Tier 2) | rustc built-in target | R0 component, the replay reference | Fixed by the operator (single source §7). rustc links components itself through `wasm-component-ld` 0.5.31, so no extra packager is needed. **Do not move to `wasm32-wasip3` yet.** WASI 0.3.0 is out, but the target reaches Tier 2 only in Rust 1.100 and is untested in rustc CI; revisit after klein-rs adopts it. |
| **`dashu-int` / `dashu-ratio`** | 0.6.2 / 0.6.1 (dashu-base 0.6.1) | Z and Q | The only actively maintained, **permissive**, pure-Rust bignum with asymptotically fast algorithms. Its changelog lists Karatsuba, Toom-3 and **NTT multiplication** (since 0.4.3), Burnikel–Ziegler division, Hensel exact division (0.6.0), and Montgomery REDC with Jacobi/Kronecker symbols (0.6.2). Small values are stored inline (0.6.1 made inline clones about 4–5× faster). It is pure Rust, so it builds unchanged for wasm32 and native, and needs no C toolchain. |
| **mpmath 1.3.0 `libmp` port** | port of mpmath 1.3.0 (BSD-3) | `evalf`, `science.numerics`, arbitrary-precision floats | SymPy's `evalf` digits come from mpmath. Porting its algorithms (sign, mantissa, exponent, bit count over `IBig`, with the same rounding) is the only way to match the golden outputs **digit for digit**. **Pin the port to mpmath 1.3.0**: SymPy 1.14.0 requires `mpmath<1.4,>=1.1.0`, so the oracle runs 1.3.0, even though mpmath 1.4.1 exists. Use one float representation; `dashu-float` and `astro-float` stay out of the runtime. |
| **Own expression arena and interner** | uses `hashbrown` 0.17.1 `HashTable` + `foldhash` 0.2.0 `fast::FixedState` | hash-consed DAG (`ks-expr`) | No crate provides what a CAS needs: a per-request arena, an `ExprId(u32)` without `Ord`, structural canonical ordering, and an interning hash that never escapes. `HashTable` gives raw-entry interning without double hashing. A fixed-seed `foldhash` keeps within-target runs reproducible. The external identity is SHA-256 over JCS, computed at the boundary (rule 4), never the interning hash. |
| **`egglog`** | 3.0.0, `default-features = false` | bounded e-graph simplify (M3) | Named building block (single source §2). 3.0 adds **per-e-graph thread configuration** (`set_num_threads`, which replaced the global rayon pool), so R0, R1 and R2 can all run it with 1 thread. Extraction is pluggable (`TreeCostModel`/`DagCostModel` on main) for a canonical tie-break. Its repo carries a `wasm-example` built with `default-features = false`. See flag F1 for the `im-rc` dependency. |
| **`faer`** | 0.24.4 (now on Codeberg) | f64/f32/complex dense LA: numeric matrices, eigen, LU and QR for `science.numerics`, `lambdify` post-processing | The fastest pure-Rust dense LA. It has explicit SIMD via `pulp`, optional rayon, and is MIT. Floats are tagged approximate and replayed (rule 4), so its parallel reductions are acceptable. |
| **`rayon`** | 1.12.0 | R2 data parallelism | Standard. Use only order-preserving combinators (`par_iter().map().collect()`, `par_chunks`) on any path that reaches output. |
| **`cubecl`** | **=0.11.0** (2026-10-06); features `metal` now, `cuda` and `hip` later | R2 GPU kernels: NTT, modular GEMM/elimination, F4 Macaulay reduce, CRT batch, tape interpreter | The operator's choice. It is the only maintained Rust-native kernel language that targets Metal (MSL), CUDA, HIP, Vulkan and WGSL from one `#[cube]` source. Details in "GPU layer" below. **Pin with `=`**: the README says "alpha … expect breaking changes between minor versions". |
| **`wit-bindgen`** | 0.62.0 | guest bindings for the WIT tool interface | The README's recommended Rust flow is `rustup target add wasm32-wasip2`, `crate-type = ["cdylib"]`, `wit_bindgen::generate!`, then `cargo build --target wasm32-wasip2`. That yields a component directly. |
| **`serde_json_canonicalizer`** | **=0.3.2** | RFC 8785 JCS for `klein.science.expr.v1`, envelopes, golden records | It is **exactly what klein-rs uses** (design/16-dependencies.md, with a differential test against the RFC vectors and Cyberphone's reference). Identity bytes must agree with Klein's own canonicalizer, so one implementation across Klein is the principled choice (see flag F4). |
| **`sha2`** | **=0.11.0** | expression identity, golden manifests, bundle digests | Same pin as klein-rs. It is pure Rust and portable; it uses ARMv8 SHA instructions when they are present, and the output is identical either way. |
| **`ed25519-dalek`** | **=3.0.0** (pulls `curve25519-dalek` 5.0.0) | signing `klein.plugin.v1` bundles in release CI | Same crate and pin as klein-rs, whose `klein-compose::trust::verify_detached` checks **raw detached Ed25519 over the message bytes, hex-encoded, with `verify_strict` only**. Sign with `SigningKey::sign` (pure Ed25519, no prehash) and test against `verify_strict`. |

### Supporting Libraries

| Library | Version | Licence | Purpose | When to use |
|---|---|---|---|---|
| `smallvec` | 1.16.3 | MIT OR Apache-2.0 | inline argument lists for `Add`/`Mul` nodes, exponent vectors | M1 onward |
| `foldhash` | 0.2.0 | Zlib | fixed-seed internal hashing (`foldhash::fast::FixedState`) | every internal hash table |
| `hashbrown` | 0.17.1 | MIT OR Apache-2.0 | `HashTable` for the interner | `ks-expr` |
| `indexmap` | 2.14.2 | Apache-2.0 OR MIT | insertion-ordered maps where order must be the algorithm's own order (e.g. collected terms before canonical sort) | sparingly; never as a substitute for canonical sorting |
| `rand_chacha` | 0.10.0 | MIT OR Apache-2.0 | ChaCha20 stream for randomized algorithms (Zippel evaluation points, Cantor–Zassenhaus, Miller–Rabin extra bases, Pollard rho). **Seed is derived from the request digest.** | M2 onward. Draw raw `u32`/`u64` words and map them to ranges with **own** code, not `rand`'s distributions, which have "value-breaking" changes across versions (num-bigint 0.5.0 notes this). |
| `thiserror` | 2.0.21 | MIT OR Apache-2.0 | typed errors | all crates |
| `serde` / `serde_json` | 1.0.229 / 1.0.152 | MIT OR Apache-2.0 | wire structs, fed to the canonicalizer | `ks-wire` |
| `papaya` | 0.2.5 | MIT | lock-free concurrent hash map | **Only if** an R2 measurement shows a shared interner beats per-worker arenas with index-ordered merge (the operator's "lock-free concurrent interning" idea). Not in M1. |
| `astro-float` | 0.9.6 | MIT | correctly rounded MPFR-style functions | **dev-dependency only**: an independent in-process cross-check of the mpmath port at high precision. Not an oracle for SymPy parity. |
| `num-bigint` / `num-rational` | 0.5.1 / 0.4.2 | MIT OR Apache-2.0 | arrive transitively via `egglog` (`num`) | Allowed in the tree. Never part of `ks-*` public APIs (one integer type). |
| `cranelift-jit` | 0.136.2 | Apache-2.0 WITH LLVM-exception | native JIT for `lambdify` tapes | **Later, R2 only, if measured.** WASM cannot JIT, so the tape interpreter remains the reference. MSRV 1.97. |
| `wide` | 1.7.1 | Zlib OR Apache-2.0 OR MIT | portable SIMD lanes, including wasm `simd128` | Optional. The few hot kernels (NTT butterfly, coefficient vectors) can use `core::arch::wasm32` directly instead. Pick one per kernel family, not both. |

### Development Tools

| Tool | Version | Purpose | Notes |
|---|---|---|---|
| `wasm-tools` | 1.261.0 | `validate`, `component wit` (check the exported world), `strip`, `metadata` | Run `wasm-tools validate --features all` and `component wit` as CI gates on the R0 artifact. |
| `wkg` (wasm-pkg-tools) | 0.16.1 | fetch and lock WIT packages (Klein's tool world) by version | Only if Klein publishes its WIT as a package; otherwise vendor the WIT with a digest. |
| `cargo-deny` | 0.20.2 | **licence gate** (deny GPL/LGPL/AGPL; MPL only by explicit exception), bans, advisories | The mechanical enforcement of CLAUDE.md rule 2. Config below. |
| `cargo-about` | 0.9.2 | generates `NOTICE` and third-party licence text for every shipped bundle | Ports (SymPy, mpmath, SymEngine, Diofant, FriCAS) are added to `NOTICE` by hand; cargo-about covers linked crates. |
| `cargo-auditable` | 0.7.7 | embeds the dependency list in native R2 binaries | Lets Klein-side audits see what a signed worker contains. |
| `cargo-nextest` | 0.9.148 | test runner | Per-test process isolation helps GPU tests. |
| `proptest` | 1.12.0 | property tests (`expand(factor(p)) == p`, CRT round-trips, NTT == schoolbook) | Fix `PROPTEST_RNG_SEED` in CI so failures replay. |
| `insta` | 1.49.0 | snapshot tests for printers (`str`, `srepr`, `latex`) | Snapshots of **our own** output only. Third-party corpus bytes never go in git (rule 6). |
| `criterion` | 0.8.2 | budget benchmarks on the fixed box | Performance claims are measured (rule 5). Record box, toolchain, commit and device in each result. |
| Docker | 28.3.3 (installed locally) | offline oracle containers | `--network=none` at run time; images pinned by `@sha256` digest. |
| `uv` | 0.13.0 | hash-locked Python dependencies inside the oracle image | `uv pip install --require-hashes -r requirements.lock` |

---

## Layer-by-layer detail

### 1. Numbers (Z, Q, Z/p, arbitrary-precision R)

- **Z.** Use `ks-num::Int`, a thin wrapper that starts as a newtype over `dashu_int::IBig`. Add an `enum { Small(i64),
  Big(Arc<IBig>) }` fast path only if a benchmark shows it beats dashu's own inline representation. ARCHITECTURE.md
  already sketches the enum; the measurement decides (rule 5).
- **Wire encoding is decimal strings, never limbs.** dashu's serde output for `IBig` is "two's complement little-endian
  bytes" (0.5.0 changelog), and limb width differs between wasm32 and 64-bit hosts. Never serialize dashu through serde
  onto the wire.
- **Q.** `dashu_ratio::RBig`, always normalized.
- **Z/p.** Own `Zp32` (u32 Montgomery, primes below 2^31) and `Zp64` (u64, CPU only). Results are exact, so any correct
  implementation is byte-identical. Writing the u32 variant in the same shape as the `#[cube]` kernels keeps the R0 and R2
  code reviewable side by side.
  - `num-modular` 0.6.6 (MIT/Apache, already in the tree via dashu) is acceptable for the CPU, but not required.
- **Primes and ntheory.** Port SymPy `ntheory` (BPSW `isprime`, `factorint`), because parity with SymPy's behaviour
  matters more than raw speed.
  - Do not add `num-prime` (Apache-2.0) or `machine-prime` (CC0) as runtime dependencies. At most they serve as dev-only
    cross-checks.
- **Arbitrary-precision floats.** Port mpmath **1.3.0** `libmp` over `IBig` (see Core). Every float result is tagged
  `numeric_class: approximate` (rule 4). The port is still deterministic across R0, R1 and R2, because it is integer
  arithmetic underneath, and that makes conformance easy.

### 2. Expression DAG and interning

- Use a per-request arena: `Vec<Node>` plus an interner over `hashbrown::HashTable<ExprId>` keyed by a `foldhash`
  structural hash, with full equality on collision.
- **Canonical argument order is structural** (a port of SymPy's `sort_key` semantics). It is never `ExprId` order, which
  is allocation order and differs between R0 and an R1 merge, and never hash order.
- **Pitfall.** `rustc-hash`'s FxHasher, and anything hashing `usize`, gives different values on wasm32 (32-bit `usize`)
  and on aarch64/x86_64. Any `HashMap` iteration order that reaches output therefore breaks byte identity *between R0 and
  R2* even with fixed seeds. Rule: hash-map iteration never reaches output; sort canonically first.
- **Do not adopt:**
  - `hashconsing` (1.8.0): process-global tables and an old Send/Sync advisory (RUSTSEC-2020-0107).
  - `internment` (0.8.6): 0.8.7 was **pulled from crates.io for a malicious dependency** (RUSTSEC-2026-0266), with
    earlier UAF and data-race advisories. It is also process-global.

### 3. Sparse multivariate polynomials

- **Build our own.** No permissive Rust crate is close to SymPy `polys` parity. The plan is:
  - port SymPy's `rings` (sparse, dict-based `PolyElement`) and `densetools`/`densearith` (dense recursive DMP);
  - use packed exponent vectors (several exponents per `u64`, with an overflow check) in sorted term arrays for
    Monagan–Pearce heap multiply and divide;
  - take SymEngine (MIT) as a design reference for packed monomials.
- `feanor-math` 3.6.1 (MIT) is a serious generic algebra library, but its generic-ring architecture would fight the
  domain design. The seam also scores it `SUS`/LOW on legitimacy because of low download counts. **Do not depend on it.**
  Using it as a *reference* would extend the CLAUDE.md port list, which is an operator decision (flag F5).
- `algebraeon` 0.0.17 is **GPL-3.0-only**. It must never be linked or read as a reference implementation.

### 4. E-graph simplification

- **`egglog` 3.0.0** is the named building block. Use it as follows:
  - `EGraph::set_num_threads(1)` in all three implementations;
  - a custom `TreeCostModel` whose cost is `(size, canonical structural key)`, so ties never fall to table order or
    e-class ID;
  - bounded schedules with node, iteration and time caps expressed as *steps*, not wall time;
  - never `:unsafe-seminaive` (the changelog calls it "evaluation-order dependent").
- Pass numbers into egglog as opaque leaves (our `ExprId`, or custom primitives), not as egglog's `BigRat`. That keeps
  exactly one rational type in the system.
- `egg` 0.11.0 (MIT) is the lighter alternative. It is not adopted, so the project has one e-graph mechanism.
- klein-rs currently locks `egglog` 2.0.0. klein-science is a separate workspace, so the versions need not match.

### 5. Dense linear algebra

- **Exact (Z, Q, Z/p, K[x]):** own `ks-dmat`, a port of SymPy's `polys.matrices` (DomainMatrix: DDM and SDM, Bareiss,
  `rref_den`, Berkowitz), plus multi-modular determinant and rank with CRT and rational reconstruction. Neither `faer` nor
  `nalgebra` supports exact domains.
- **GPU:** dense mod-p elimination and GEMM in `#[cube]` kernels with 31-bit primes (see §6).
- **Floating point:** `faer` 0.24.4.
- **Not adopted: `nalgebra` 0.35.0** (Apache-2.0, sound). It overlaps with `faer` on dynamic matrices and adds nothing
  for exact work, so it would be a second mechanism.

### 6. GPU layer: CubeCL 0.11.0 (verified from crate sources)

| Fact | Evidence | Consequence |
|---|---|---|
| Two Metal paths exist. Feature `metal` = `wgpu-msl` (wgpu runtime + MSL emitter); feature `metal-native` = new `cubecl-metal` crate (objc2-metal, 5,260 downloads, first released in 0.11.0) | `cubecl-0.11.0/Cargo.toml` features | **Start on `metal` (wgpu-msl)**, the path Burn users exercise. Probe `metal-native` in M0 as a second candidate, mainly because it may need fewer Seatbelt services. |
| The wgpu-msl path needs the **Metal 3 GPU family and an MSL 3.2 compiler**. Otherwise it falls back to WGSL, which has no u64 unless `SHADER_INT64` is available | `cubecl-wgpu/src/backend/metal.rs` (canary compile) | The M3 Max qualifies. Record the macOS and MSL version in every R2 provenance record. |
| On Metal, CubeCL registers **u8–u64, i8–i64, f16, bf16, f32, bool**, and **no f64** | `register_types` in both `cubecl-wgpu/backend/metal.rs` and `cubecl-metal/runtime.rs` | This settles the open question in ARCHITECTURE.md: **u64/i64 *are* exposed on the MSL path.** f64 on a Mac stays on the CPU. |
| Atomics on Metal: **i32/u32 all ops; f32 add/load/store only; u64 atomics deliberately not registered** ("registering it made add, load and store return wrong values") | same file, comment at the atomics block | Exact kernels need no 64-bit atomics. Use fixed-tree reductions and no float atomics. |
| `cuda`, `hip` and `cpu` backends depend on `cubecl-llvm`, which pulls `llvm-sys` 231 and the **prerelease `tracel-llvm-bundler` 23.1.0-3** (a bundled LLVM). The Metal paths do not | crates.io dependency lists | The Metal-first build is LLVM-free and simpler to make reproducible. Budget the CUDA/ROCm lane separately: LLVM bundle provenance and pinning. |
| `persistence` feature: kernel and autotune cache persisted (turso-backed); "Disable persistent tune cache option" exists | release notes and features | **Leave `persistence` off** inside the sandbox. Ship a pinned kernel cache by digest instead, as the single source §4 requires. |
| wgpu `SHADER_INT64` is supported on Metal (MSL 2.3+); u64 atomic min/max needs MSL 2.4+; **`SHADER_F64` is Vulkan-only** | docs.rs `wgpu::Features` | This confirms no f64 on Metal through any wgpu path. |

**Kernel design rule** (agrees with ARCHITECTURE.md): exact kernels use **31-bit primes and u32 Montgomery with 64-bit
products**, so one algorithm runs on WASM SIMD128, CPU, Metal, Vulkan, CUDA and HIP. u64 is exposed on Metal, but Apple
GPUs very likely emulate 64-bit integer multiplies (MEDIUM, from general knowledge; **measure in M0**). u64-prime variants
for CUDA/HIP are an optimisation admitted separately by conformance.

**Autotune:** allowed for exact kernels (any configuration gives the same bytes). Disabled or pinned for float kernels.

### 7. NTT

- **Own `ks-ntt`**, roughly 1–2k LOC.
  - Use a fixed table of NTT-friendly primes below 2^31 (`p = c·2^k + 1`) for the GPU and WASM. The CPU may add 62-bit
    primes.
  - Use radix-2/4 Cooley–Tukey with precomputed twiddles.
  - Use Garner CRT to reconstruct big-integer and polynomial coefficients.
- WASM SIMD128 has `i64x2.extmul_low_i32x4_u` and `extmul_high`, so u32×u32→u64 Montgomery vectorizes in R0.
- For R0 big-integer multiplication, `dashu-int`'s internal NTT is enough. `ks-ntt` serves polynomials, batched
  multi-modular work and the GPU.
- **Do not link:**
  - `tfhe-ntt` 0.7.1 and `concrete-ntt` 0.2.0 (Zama). Their licence is **BSD-3-Clause-Clear**, which explicitly grants
    no patent rights and is not on the CLAUDE.md permissive list. They are also native-only.
  - `icicle-core`: stale on crates.io (1.3.0, 2024), and its later releases are distributed outside crates.io.
- Optional later: Plonky3's `p3-dft` and `p3-monty-31` 0.8.0 (MIT OR Apache-2.0) are high-quality 31-bit-field NTTs. They
  are an R2 CPU acceleration candidate **only if measured faster** than `ks-ntt`. They are tied to Plonky3's field traits.

### 8. WASM component model (R0)

- **Build:** `cargo build --release --locked --target wasm32-wasip2`. Put this in `.cargo/config.toml`:
  - `target-feature=+simd128`, which is **not** on by default for wasm targets;
  - `--remap-path-prefix`;
  - `codegen-units=1`, `lto="fat"`, `panic="abort"`.
- **Never enable `+relaxed-simd`.** Relaxed SIMD results are implementation-defined, and Wasmtime lowers them to "the
  fastest lowering for the platform" unless `relaxed_simd_deterministic` is set.
- **Bindings:** `wit-bindgen` 0.62.0 `generate!` against Klein's tool WIT. It exports `invoke` plus `plan`/`map`/`reduce`
  for R1, per ARCHITECTURE.md.
- **Not `cargo-component`.** It was last released 0.21.1 on 2025-03-18, and its README calls it "experimental and … not
  currently stable". The wit-bindgen README's flow no longer needs it.
- **Not `wasm-opt`/Binaryen** on the component. Its component support is limited, and it adds a non-Rust step to the
  reproducibility chain.
- **The host is klein-rs's Wasmtime** (pinned at 46.0.1; latest is 49.0.2). wit-bindgen output is component-model ABI,
  so the guest does not need the host's version. **CI must run the R0 artifact under the exact Wasmtime version Klein
  pins.** Use `wasmtime =46.0.x` as a dev-dependency test host, or the Klein binary.

### 9. Canonical JSON and identity

- `serde_json_canonicalizer` =0.3.2 + `sha2` =0.11.0, as in klein-rs.
- **Encode every number in `klein.science.expr.v1` as a JSON string**: integers in decimal, rationals as `"p/q"` or a
  pair, mpf as mpmath's exact `(sign, man, exp, bc)` tuple of strings. JCS serializes JSON numbers as IEEE doubles
  (ECMAScript `Number.toString`, via `ryu-js`), so a bare integer above 2^53 silently loses precision. With no JSON
  numbers, JCS reduces to key sorting plus string escaping, and the hardest part of RFC 8785 (number canonicalization)
  disappears.
- Reuse klein-rs's differential test against the RFC 8785 vectors.

### 10. Signing and release

- Release CI produces:
  - a reproducible `.wasm`;
  - native R2 binaries per triple;
  - the `klein.plugin.v1` manifest (JCS);
  - a detached Ed25519 signature (hex) over the bytes Klein verifies;
  - a SHA-256 digest.
- Use `ed25519-dalek` =3.0.0 in an `xtask sign` step. The private key lives outside the repo (operator-held, or a CI
  secret). The repo never contains it, and agents never read it.
- **Do not use `minisign` 0.10.0 or `sigstore` 0.14.0 formats.** Klein's verifier expects raw detached Ed25519 with
  `verify_strict`. A different envelope would not verify.

### 11. Offline oracle harness (build and test time only, never shipped)

| Item | Pin | Why |
|---|---|---|
| Base image | `python:3.13-slim-trixie@sha256:<digest>` (CPython 3.13.14) | SymPy 1.14.0's classifiers stop at 3.13, and 3.15.0 shipped 2026-10-09. Pinning **by digest**, not tag, is what makes the golden outputs "frozen". |
| SymPy | `sympy==1.14.0` (latest stable, 2025-04-27) | The parity target. Bump deliberately, regenerating goldens and diffing. |
| mpmath | `mpmath==1.3.0` | SymPy 1.14.0 requires `<1.4`. This version is also the port source for `ks-mp`. |
| Python JCS | `rfc8785==0.1.4` (Trail of Bits, Apache-2.0) | Golden records are JCS-canonicalized inside the container. The Rust conformance runner re-canonicalizes with `serde_json_canonicalizer`, and the two must agree. That disagreement check is a free extra oracle. |
| Dependency lock | `uv` 0.13.0, `--require-hashes` | Every wheel is pinned by hash. |
| Environment | `PYTHONHASHSEED=0`, `SYMPY_GROUND_TYPES=python`, `MPMATH_NOGMPY=1`, `LC_ALL=C.UTF-8`, `TZ=UTC`; no gmpy2 or python-flint in this image | SymPy's set/dict-dependent output order (e.g. `solve` results, `free_symbols`) varies with hash randomization. Ground types and the mpmath backend must not drift with what happens to be installed. |
| Run | `docker run --network=none --read-only`, per-case subprocess with a CPU-time limit; record a `timeout`/`error` status rather than dropping cases | SymPy can hang on hard inputs. A missing row would bias the score. |
| Output | JSONL of JCS records (`case_digest`, `sympy_version`, `srepr`, `str`, our `expr.v1` translation, status), plus a manifest of SHA-256 digests | The **manifest digest** goes in git. Golden and corpus bytes live outside git (an OCI artifact or release asset), fetched by recipe (rule 6). |
| Arch check | Generate once on `linux/arm64` (the Mac) and once on `linux/amd64`, then compare | Any byte difference shows that a case depends on platform behaviour. Adjudicate it before it becomes a golden. |
| Other oracles | Separate images: SymEngine (`symengine==0.14.1`, MIT; cp313 wheels exist), Giac and FLINT binaries (GPL/LGPL, operator-approved offline use) | Keeps the SymPy golden image's digest independent of benchmark tooling. Nothing from these images is linked or shipped. |

---

## Installation

```toml
# rust-toolchain.toml
[toolchain]
channel = "1.99.0"
components = ["rustfmt", "clippy", "rust-analyzer"]
targets = ["wasm32-wasip2", "aarch64-apple-darwin"]
```

```toml
# Cargo.toml (workspace) — exact pins where Klein shares bytes or the API is unstable
[workspace.dependencies]
dashu-int   = "0.6.2"
dashu-ratio = "0.6.1"
hashbrown   = "0.17.1"
foldhash    = "0.2.0"
smallvec    = "1.16.3"
egglog      = { version = "=3.0.0", default-features = false }
faer        = { version = "0.24.4", default-features = false, features = ["std"] }  # enable "rayon" only in R2
rayon       = "1.12.0"
cubecl      = { version = "=0.11.0", default-features = false, features = ["std", "stdlib", "metal"] }  # R2 only; add "cuda"/"hip" later
wit-bindgen = "0.62.0"
serde       = { version = "1.0.229", features = ["derive"] }
serde_json  = "1.0.152"
serde_json_canonicalizer = "=0.3.2"
sha2        = "=0.11.0"
ed25519-dalek = "=3.0.0"           # xtask/sign and verification tests only
rand_chacha = "0.10.0"
thiserror   = "2.0.21"
# dev-only (referenced from each crate's [dev-dependencies] with `workspace = true`)
proptest = "1.12.0"
insta    = "1.49.0"
criterion = "0.8.2"
astro-float = "0.9.6"
```

```toml
# .cargo/config.toml
[target.wasm32-wasip2]
rustflags = ["-C", "target-feature=+simd128", "--remap-path-prefix=$CARGO_HOME=/cargo", "--remap-path-prefix=$PWD=/src"]

[profile.release]
codegen-units = 1
lto = "fat"
panic = "abort"
```

```bash
rustup target add wasm32-wasip2
cargo install --locked wasm-tools@1.261.0 cargo-deny@0.20.2 cargo-about@0.9.2 cargo-nextest@0.9.148 cargo-auditable@0.7.7
cargo build --release --locked --target wasm32-wasip2 -p ks-component
wasm-tools validate target/wasm32-wasip2/release/ks_component.wasm
wasm-tools component wit target/wasm32-wasip2/release/ks_component.wasm
```

```toml
# deny.toml (excerpt) — mechanical enforcement of CLAUDE.md rule 2
[licenses]
allow = ["MIT", "Apache-2.0", "Apache-2.0 WITH LLVM-exception", "BSD-2-Clause", "BSD-3-Clause",
         "ISC", "Zlib", "Unicode-3.0", "BSL-1.0", "CC0-1.0"]
confidence-threshold = 0.93
# MPL-2.0 only by explicit, operator-approved exception (flag F1):
# exceptions = [{ name = "im-rc", allow = ["MPL-2.0"] }]

[bans]
deny = [
  { name = "rug" }, { name = "gmp-mpfr-sys" }, { name = "malachite" }, { name = "malachite-base" },
  { name = "malachite-nz" }, { name = "malachite-q" }, { name = "algebraeon" },
  { name = "flint-sys" }, { name = "symbolica" }, { name = "internment" },
]
```

---

## Alternatives Considered

| Recommended | Alternative | When to use the alternative |
|---|---|---|
| `dashu-int` 0.6.2 | `num-bigint` 0.5.1 (MIT/Apache) | Only if dashu became unmaintained. num-bigint has no FFT/NTT multiply (its best is Toom-3 plus Burnikel–Ziegler division since 0.4.7), so it loses on large operands. It is already in the tree via egglog. |
| mpmath port on `dashu-int` | `astro-float` 0.9.6 / `dashu-float` 0.6.2 as the runtime float | Only if digit-exact parity with SymPy's `evalf` were dropped as a goal. Both use different algorithms and rounding paths from mpmath, so last digits would disagree with the goldens. |
| Own arena and interner | `hashconsing` 1.8.0 | Never for the core (global tables). It is fine for a throwaway spike. |
| `egglog` 3.0.0 | `egg` 0.11.0 (MIT, no MPL dependency) | If flag F1 is resolved "no MPL", **`egg` is the fallback**. It has a simpler API with explicit `Runner` limits, and a custom `CostFunction` gives the same canonical tie-break. |
| `faer` 0.24.4 | `nalgebra` 0.35.0 (Apache-2.0) | Small fixed-size geometry, which a CAS does not need. |
| CubeCL `metal` (wgpu-msl) | CubeCL `metal-native` (cubecl-metal) | If the M0 Seatbelt probe shows the native path needs fewer sandbox services, or if it is measurably faster. It has 5k downloads and is days old, so it is not the first choice. |
| CubeCL | `cudarc` 0.19.10 / `objc2-metal` 0.3.2 directly | Only for a single backend with a hand-written kernel. That would break "one kernel source, all GPUs". |
| Own `ks-ntt` | `p3-dft` 0.8.0 (Plonky3) | As an R2 CPU accelerator, if measured faster. |
| `serde_json_canonicalizer` =0.3.2 | hand-written streaming JCS encoder (as ARCHITECTURE.md sketches for `ks-wire`) | Only if JCS encoding measures as hot. It would then have to be differential-tested against `serde_json_canonicalizer` and the RFC vectors (flag F4). |
| `wit-bindgen` + rustc's `wasm32-wasip2` | `cargo-component` 0.21.1 | Never; it is stale. |
| `wasm32-wasip2` | `wasm32-wasip3` | After WASI 0.3 is Tier 2 on stable (Rust 1.100) **and** klein-rs's host supports it. |

## What NOT to Use

| Avoid | Why | Use instead |
|---|---|---|
| `malachite` 0.13.0 | **LGPL-3.0-only** (registry-verified). Forbidden by CLAUDE.md rule 2, even though it is pure Rust. | `dashu-int` |
| `rug` 1.30.0 / `gmp-mpfr-sys` | **LGPL-3.0+**, and links GMP/MPFR | `dashu-int` + mpmath port |
| Any FLINT binding (`flint-sys`, python-flint at runtime) | LGPL. Offline oracle only. | own implementations; FLINT binary in an oracle container |
| `algebraeon` 0.0.17 | **GPL-3.0-only** | own `ks-poly` |
| `internment` | 0.8.7 pulled for a malicious dependency (RUSTSEC-2026-0266); earlier UAF and data race | own interner |
| `tfhe-ntt`, `concrete-ntt` | BSD-3-Clause-**Clear** grants no patent rights; not on the permissive list | own `ks-ntt` |
| `cargo-component` | stale (2025-03), self-described experimental | `cargo build --target wasm32-wasip2` + `wit-bindgen` |
| `+relaxed-simd` target feature | implementation-defined results, which break replay | `+simd128` only |
| `bincode` 3.x | RUSTSEC-2025-0141, unmaintained | JCS for the wire; nothing binary is needed |
| `rkyv` | six 2026 advisories (UAF, OOB on crafted archives); zero-copy is not needed | JCS |
| `rand::StdRng` / `thread_rng` / `rand` distributions on result paths | StdRng's algorithm is not stable across versions; distributions are value-breaking across releases; thread RNG is non-deterministic | `rand_chacha::ChaCha20Rng` seeded from the request digest, with own range mapping |
| `std::collections::HashMap` with `RandomState`, or any hash-ordered iteration that reaches output | non-deterministic, and pointer-width-dependent between wasm32 and 64-bit | canonical structural sort before output |
| f64 kernels on Metal | Metal has no f64 (CubeCL registers none; wgpu `SHADER_F64` is Vulkan-only) | CPU (rayon + `faer`) for f64 on a Mac |
| `serde` derive of `IBig`/`RBig` onto the wire | the format is two's-complement LE limbs | decimal strings |
| CubeCL `persistence` feature in the sandbox | writes a cache outside the pinned location | a pinned kernel cache shipped by digest |
| Bend / HVM | already rejected by the operator (single source §4) | — |
| Symbolica, in any form | binding D-R (Symbolica-clean) | — |

## Stack Patterns by Variant

**If the build target is R0 (`wasm32-wasip2`):**
- No rayon, no CubeCL, no `faer` `rayon` feature, no threads. Use SIMD128 in the NTT and coefficient kernels.
- The `ks-accel` CPU reference implementation is the R0 path. It is the replay reference for everything.

**If the build target is R2 native on macOS:**
- `cubecl` feature `metal`, plus `rayon`, plus `faer` with `rayon`.
- f64 goes to the CPU. Exact kernels use u32 and 31-bit primes.
- `persistence` off; kernel cache pinned by digest.

**If the build target is R2 native on Linux with CUDA or ROCm (deferred):**
- Add `cubecl` features `cuda` or `hip`. This pulls the `cubecl-llvm` and LLVM-23 bundle, so pin and digest it.
- u64-prime kernel variants are admitted only via conformance with a witness seen RED.

**If egglog's `im-rc` MPL-2.0 dependency is not approved (F1):**
- Use `egg` 0.11.0 with a custom bounded `Runner` and a canonical `CostFunction`. The rule-set data format stays ours, so
  the strategy-as-bundle-data design does not change.

## Version Compatibility

| Package | Compatible with | Notes |
|---|---|---|
| `cubecl` =0.11.0 | `wgpu` ^30 (30.0.1), edition 2024 | Every `cubecl-*` crate must be the same exact version. |
| `egglog` 3.0.0 | `hashbrown` 0.16, `num` 0.4, `im-rc` 15.1, `ordered-float` 5 | Two `hashbrown` versions in the tree (0.16 via egglog, 0.17 ours) are harmless. |
| `faer` 0.24.4 | `pulp` 0.22, `gemm` 0.19, MSRV 1.84 | Check that the `wasm32-wasip2` build compiles in M0. pulp's wasm SIMD path is unverified (LOW). |
| `wit-bindgen` 0.62.0 guest | klein-rs `wasmtime` 46.0.1 host (WASI 0.2.x) | Test the artifact under the host version Klein pins. |
| `sympy` 1.14.0 | `mpmath` >=1.1.0,<1.4 → 1.3.0; CPython 3.9–3.13 | The `ks-mp` port source is mpmath 1.3.0. |
| `symengine` (python) 0.14.1 | cp39–cp314 wheels | SymEngine oracle image only. |
| `ed25519-dalek` =3.0.0 | `curve25519-dalek` 5.0.0, `sha2` 0.11 | Same as klein-rs, so one `sha2` in a joint tree. |
| Rust 1.99.0 | maximum MSRV in the stack is 1.97 (wasmtime / cranelift) | `burn` (1.95) is not used. |

## Flags for the operator (recommendations; nothing in SCIENCE-PLUGIN-CAS.md is contradicted)

- **F1. egglog pulls `im-rc` 15.1.0.** That crate is MPL-2.0+ (weak copyleft, not GPL/LGPL) and **unmaintained**: the
  repo was archived 2026-05-03 (RUSTSEC-2026-0250). The single source lists egglog as permissive, and egglog itself is MIT,
  but the shipped R0/R2 bundles would contain MPL-2.0 code.
  - Options:
    - (a) approve an MPL-2.0 exception for `im-rc` only, in `deny.toml`, with notice text;
    - (b) use `egg` 0.11.0 instead;
    - (c) ask egglog upstream to move to `imbl`.
  - **Recommendation: (a) for M3**, with (b) as the documented fallback.
- **F2. klein-rs pins `wasmtime` 46.0.1.**
  - It is affected by RUSTSEC-2026-0222 and -0223 (patched in 46.0.2), and by RUSTSEC-2026-0316 ("dynamic record lifting
    can allocate beyond the hostcall fuel limit", patched ≥48.0.3 / ≥49.0.1). R0 relies on fuel bounds.
  - **Request to klein-rs** (add to ROADMAP "Dependencies on Klein core"):
    - move to ≥49.0.1, or at least 46.0.2;
    - for science components, set `relaxed_simd_deterministic(true)` and `cranelift_nan_canonicalization(true)`;
    - keep `consume_fuel` on.
- **F3. ARCHITECTURE.md open questions answered by this research.**
  - (1) CubeCL's wgpu/MSL path *does* register u64/i64 on Metal. It needs the Metal 3 family and an MSL 3.2 compiler.
  - (2) egglog builds for wasm with `default-features = false` (upstream `wasm-example`), and 3.0 makes the thread count
    per e-graph. The `wasm32-wasip2` build itself is still to be confirmed in M0.
- **F4. JCS implementation.** ARCHITECTURE.md sketches a hand-written JCS encoder for `ks-wire`. Recommendation: use the
  same `serde_json_canonicalizer` =0.3.2 that klein-rs uses, so the plugin and Klein share one canonicalizer. Add a
  hand-written encoder only if measured hot, and then differential-test it.
- **F5. Port list.** CLAUDE.md allows porting SymPy, mpmath, Diofant, FriCAS and SymEngine. Permissive Rust crates such
  as `feanor-math` (MIT) and Plonky3 are not on the list as references. Recommendation: keep the list as is; add a crate
  only when a phase needs it, by operator decision.

## Sources

Registry and source facts (queried 2026-10-11; seam tier MEDIUM, verified, legitimacy OK):
- crates.io API `https://crates.io/api/v1/crates/<name>`: versions, licences, dates and dependency lists for every crate
  named above.
- PyPI JSON API: sympy 1.14.0 (`requires_dist: mpmath<1.4,>=1.1.0`, classifiers 3.9–3.13), mpmath 1.3.0 / 1.4.1,
  symengine 0.14.1, rfc8785 0.1.4, uv 0.13.0.
- `static.rust-lang.org/dist/channel-rust-stable.toml`: Rust 1.99.0 (2026-09-28).
- CubeCL 0.11.0 crate sources: `cubecl-wgpu/src/backend/metal.rs`, `cubecl-metal/src/runtime.rs`, `cubecl/Cargo.toml`
  features. [CubeCL README](https://github.com/tracel-ai/cubecl), [CubeCL releases](https://github.com/tracel-ai/cubecl/releases).
- [wgpu `Features` docs](https://docs.rs/wgpu/latest/wgpu/struct.Features.html): SHADER_INT64 (Metal MSL 2.3+),
  SHADER_F64 (Vulkan only).
- [egglog CHANGELOG](https://raw.githubusercontent.com/egraphs-good/egglog/main/CHANGELOG.md) (3.0.0, 2026-08-18), the
  egglog `wasm-example/Cargo.toml`, and egglog-core-relations 3.0.0 sources (rand only in tests).
- [dashu-int CHANGELOG](https://github.com/cmpute/dashu/blob/master/integer/CHANGELOG.md) and
  [num-bigint RELEASES](https://raw.githubusercontent.com/rust-num/num-bigint/master/RELEASES.md).
- [wit-bindgen README](https://github.com/bytecodealliance/wit-bindgen) and
  [cargo-component README](https://github.com/bytecodealliance/cargo-component).
- [wasmtime `Config` docs](https://docs.rs/wasmtime/latest/wasmtime/struct.Config.html) (49.0.2):
  `relaxed_simd_deterministic`, `cranelift_nan_canonicalization`, `consume_fuel`.
- [Rust wasm32-wasip3 platform page](https://doc.rust-lang.org/nightly/rustc/platform-support/wasm32-wasip3.html) and
  [compiler-team MCP #1001](https://github.com/rust-lang/compiler-team/issues/1001).
- RustSec advisory-db (main, fetched 2026-10-11): wasmtime 2026-0222/0223/0315/0316, im-rc 2026-0250, internment
  2026-0266, bincode 2025-0141, rkyv 2026-*.
- klein-rs (read-only): `Cargo.lock`, `rust-toolchain.toml`, `design/16-dependencies.md` (JCS, sha2, ed25519-dalek, and
  wasmtime pins and policies), `crates/klein-compose/src/trust.rs` (`verify_detached`, `verify_strict`).
- [Dawn MSL 64-bit atomics note](https://dawn.googlesource.com/dawn/+/b620b2fe514a5ee2e6a2d924fd998c514019da87): u64
  atomic min/max only on Apple8/9 GPUs (LOW, corroborating).

---
*Stack research for: klein-science (licence-clean Rust CAS as signed Klein plugins)*
*Researched: 2026-10-11*
