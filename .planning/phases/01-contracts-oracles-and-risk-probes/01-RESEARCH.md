# Phase 1: Contracts, Oracles and Risk Probes - Research

**Researched:** 2026-10-11
**Domain:** wire contract (RFC 8785 JCS + SHA-256), WASM component and native worker builds, Ed25519 signing, an offline
SymPy oracle, determinism and licence guards, CubeCL on Metal, macOS Seatbelt, and Monty protocol census
**Confidence:** MEDIUM-HIGH. Facts came from tool output in this session: crate sources, klein-rs sources, registry
APIs, and five local build experiments whose output is pasted below. GPU, Seatbelt and Monty behaviour remain
measurement tasks for this phase, by design.

Symbolica-clean: no Symbolica material was searched, read or used.

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions

#### Carried forward (operator rulings 2026-10-11, PROJECT.md Key Decisions, not re-asked)
- Identity is SHA-256 of the JCS of the canonical DAG node table. CLAUDE.md rule 4 was amended to say so on 2026-10-11.
- `fuel_exhausted` comes only from a logical step budget counted identically on R0, R1 and R2. Wasmtime fuel, memory
  caps and GPU timeouts are host errors.
- The Wasmtime ≥ 49.0.1 request (D6). Builds use `+simd128` and never `+relaxed-simd`. CI runs R0 under Klein's current
  pin and under ≥ 49.0.1.
- Golden bytes live in an artifact store by digest. Git holds recipes, manifests and scores.
- Budgets are ratified per fixed box (Mac: all P-cores against one P-core, Metal against that).
- JCS uses `serde_json_canonicalizer` =0.3.2 (the same crate and pin as klein-rs), mirrored once in Python for the
  stand-in.
- The D4 request is widened to the full protocol set from the census.

#### Design principles (operator, binding on every plan and task)
- **D-01:** **DRY, SOLID, no bloat.** There is one mechanism per concern: one JCS codec (`ksci-wire`), one digest
  helper, one manifest writer, one fixed-box manifest schema used by the probe and all benchmarks, and one artifact-store
  client used for goldens, corpora and bundles. Add no speculative abstractions, crates, flags or config beyond what a
  Phase 1 requirement needs. Deleting beats adding. A plan that adds a second path to an existing concern is wrong. See
  memory `principled-design`.

#### Naming and layout
- **D-02:** The crate prefix is **`ksci-`** (for example `ksci-wire`, `ksci-component`, `ksci-worker`). Every `ks-`
  name in the research docs maps to `ksci-`. — **Reversibility:** costly — every crate path, `use` line and manifest
  name would change.
- **D-03:** CLAUDE.md rule 4 was amended now, before planning, and is done. DET-01 is no longer blocked.

#### CI, artifact store and keys
- **D-04:** **CI host is the Klein VM plus the Mac.** The VM (the box klein-rs builds on) runs as a self-hosted runner
  for:
  - the `wasm32-wasip2` component and the Linux native worker;
  - `cargo-deny` and the determinism lints;
  - the two-clean-builds digest check and the R0-versus-R2 compare.

  The Mac runs the Metal probe and the Mac native worker through `cargo xtask`, and attaches the fixed-box manifest.
  Heavy builds must not run concurrently with klein-rs builds, on the VM or on the Mac (the Mac pages).
- **D-05:** The **artifact store is a private GHCR OCI registry** in the `selfsupervised-ai` org, pushed and pulled with
  `oras` and addressed by sha256. It holds golden bytes, corpus snapshots (TIR and the OMR sample) and built bundles.
  Git holds the fetch recipe and the manifest digest only. Pushing to the registry is outward-facing: the first push
  needs the operator's go-ahead. — **Reversibility:** costly — every recipe and manifest names this store.
- **D-06:** The **signing key is the operator release key from day one**. There is no dev key and no rotation later.
- **D-07:** **Signing happens on the Mac.** The VM builds and publishes the unsigned artifacts and their digests. On the
  Mac, `cargo xtask sign` rebuilds (or fetches) them and checks that the digests match the VM's, which doubles as the
  cross-host reproducibility check. It then signs with the key in the macOS Keychain. The private key never leaves the
  Mac, never touches the VM, and is never read into an agent conversation. Signatures are raw detached Ed25519 (no
  prehash), hex-encoded, and are tested against `verify_strict`. — **Reversibility:** one-way — a published public key
  is a trust root that Klein's lock will pin.
- **D-08:** **Native-worker targets** are `aarch64-apple-darwin` (the Mac and Metal R2) and `x86_64-unknown-linux-gnu`
  (the VM and the future remote GPU box), plus the `wasm32-wasip2` component.

#### Oracle slice and census
- **D-09:** The **golden slice** is about 1,000 SymPy-using NuminaMath-TIR programs (rev `77a91d7b…`). They are drawn
  deterministically from a seed derived from the corpus digest, stratified by the SymPy modules each program touches
  (core, solve, polys, calculus and so on). The full-corpus goldens come in Phase 2.
- **D-10:** **Reproducibility proof.** Run `PYTHONHASHSEED` values 0 (the production value), 1, 2 and one large value,
  twice each. Also cross-check `linux/arm64` against `linux/amd64`. Any case whose bytes differ goes to adjudication,
  not into the goldens.
- **D-11:** The **protocol census (D4 evidence) uses two oracles.** One is dynamic: SymPy's dunders are instrumented
  inside the oracle container, and every protocol the programs actually hit is logged. The other is a static AST scan
  of the program sources. Any disagreement between them is reviewed. The same dynamic run measures the share of TIR
  definite integrals that SymPy routes through `meijerint`.
- **D-12:** **The census covers NuminaMath-TIR plus an OpenMathReasoning sample.** The OMR dataset revision is pinned,
  and either a fixed set of shards or about 10k tool-integrated programs is drawn from a digest-derived seed. Each one
  is pinned by SHA-256 and its bytes are stored in the artifact store, never in git. The whole OMR dataset is not
  fetched.

#### klein-rs requests (D1, D4, D6)
- **D-13:** **Drafts live here, and the operator files them.** Each request is drafted at
  `.planning/requests/D1-compute-device.md`, `D4-monty-protocols.md` and `D6-wasmtime.md`. The operator approves each
  one, and it is then filed as a GitHub issue on klein-rs that links back to the draft by commit. Nothing is filed
  without the operator's sign-off. Nothing in klein-rs is edited.
- **D-14:** **Each request carries:**
  - the measured evidence (the census table, the Seatbelt run logs, the advisory IDs);
  - the exact API or config asked for;
  - an acceptance test that klein-rs can run and that is RED today.
- **D-15:** **The D1 Seatbelt minimum is shown by ablation.** Start from a deny-default profile and add rules until the
  CubeCL probe passes. Then remove each rule one at a time and record that the probe goes RED. Every rule in the filed
  set carries its own RED witness.

### Claude's Discretion
- The repo layout for unshipped tooling. Follow the ARCHITECTURE.md tree (`crates/`, `plugin/`, `xtask/`,
  `conformance/golden/` for manifests). Put the probe, the oracle images and the census tooling in a clearly unshipped
  location, outside the shipped bundles and outside the `cargo-deny` scope of the shipped crates where that applies.
- Which trivial operation runs through the WIT tool on R0 and R2, how far the stand-in skeleton goes beyond GUEST-01/02,
  and the fields of the fixed-box manifest. All within the requirements and D-01.

### Deferred Ideas (OUT OF SCOPE)
- Full-corpus TIR goldens: Phase 2.
- The numpy/scipy facade decision: after Phase 4. OMR shards are now pinned early for the census, and they serve that
  decision too.
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description (REQUIREMENTS.md) | Research Support |
|----|-------------|------------------|
| DET-01 | Identity = SHA-256 of RFC-8785 JCS of the canonical DAG node table; same expression built in any order → same identity | Pattern 1 (canonical table: iterative post-order renumbering + dedupe via `BTreeMap`); canonical-input check; RFC vectors by recipe (digests verified); 3-way differential (Rust, Python mirror, `rfc8785`) |
| DET-02 | Numeric leaves are tagged strings; special values are named atoms | Pattern 1 numeral grammar; decoder rejects JSON numbers outside node indices and `v`; Pitfall 4 |
| DET-03 | Provenance in an envelope outside the hashed bytes | Pattern 2 (request, response and record envelopes) |
| DET-04 | `fuel_exhausted` only from the logical step budget; Wasmtime fuel and memory caps are host errors | Pattern 3; Wasmtime `consume_fuel`/`set_fuel`/`StoreLimitsBuilder::memory_size` verified in 46.0.1 and 49.0.2 sources |
| DET-08 | No unseeded hash iteration, no ordering by interning id, no unseeded randomness in maths crates | clippy `disallowed-types`/`disallowed-methods`; `static_assertions::assert_not_impl_any!(NodeId: PartialOrd, Ord)`; cargo-deny bans on `getrandom`/`rand` |
| CONF-01 | Digest-pinned image (CPython 3.13, SymPy 1.14.0, mpmath 1.3.0, pinned ground types and hash seed, no network); reproducible goldens | Image index digest resolved: `python:3.13-slim-trixie@sha256:70729b46…` (CPython 3.13.16); env vars verified in SymPy and mpmath sources; multi-seed and multi-arch runs |
| CONF-02 | Golden bytes in artifact store by digest; git holds recipes, manifests, scores | `oras blob fetch <name>@sha256:<file-sha>`; re-hash after fetch; D-05 |
| CONF-03 | Three-level runner: stdout byte-equal, final answer equal, error class equal | Pattern 6; ImportError path normalisation (verified that the message embeds an absolute path) |
| CONF-06 | Every performance claim carries a fixed-box manifest; budgets ratified per box | Fixed-box manifest fields (probed on the Mac: 10 P + 4 E cores, Mac15,11, macOS 26.5.2) |
| LIC-01 | cargo-deny rejects GPL/LGPL and banned crates; seen RED | deny.toml `[bans] deny = [{ crate = … }]` (cargo-deny book); `[graph] exclude` for unshipped tooling |
| LIC-02 | Provenance header per ported module; NOTICE carries upstream notices | `xtask provenance` check; the stand-in's name table is a SymPy port (902 `__all__` names) → SymPy notice needed now |
| DEP-01 | TIR protocol census filed as one Monty request | Dynamic + static census design; Monty 1.1.0 limits fetched; `pydantic-monty` used for latency and bigint evidence |
| DEP-02 | Measured minimum Seatbelt rules for CubeCL on Metal | Base profile = klein-rs `seatbelt_image_rules` (quoted verbatim); candidate pool from Chromium `gpu.sb`; ablation via kernel-log oracle |
| DEP-03 | Wasmtime ≥ 49.0.1 request with deterministic relaxed-SIMD and NaN canonicalisation | Advisory table: 46.0.1 is affected by 6 advisories; **0325/0326/0327 are patched only at ≥ 49.0.2** |
| PLUG-01 | Reproducible wasm32-wasip2 component + native worker, `klein.plugin.v1` manifest, raw detached Ed25519 passing `verify_strict`, digest-addressed artifact | klein-rs signs `klein.capability_manifest.v1` JCS bytes (quoted); two-path reproducibility verified locally; Keychain signing is a human action |
| PLUG-02 | WIT exposes `invoke`/`plan`/`map`/`reduce` over JCS bytes; one trivial op byte-identical R0 vs R2 | wit-bindgen 0.62.0 component built locally; trivial op = `science.cas.canonicalize`; WASI import finding |
| GUEST-01 | Five import forms work with no tool call; missing name raises SymPy's exact error text | Name table generated from SymPy 1.14.0 `__all__` (902 names, no module `__getattr__`); `sympy.abc` uses `exec`, so do not port its mechanism |
| GUEST-02 | Stand-in passes a Monty-subset lint and runs under CPython `host_shim` | Monty 1.1.0 parse-time rejections fetched (inheritance, method decorators, `yield`, `match`, `del`, async with/for, `except*`) |
| ACCEL-01 | CubeCL probe: u32 mulhi and u64 throughput, fast-math default, watchdog limit | CubeCL 0.11.0 sources: Metal lowers u32 `mul_hi` through a 64-bit multiply; wgpu-msl compiles with default `MTLCompileOptions` (fast math on, per Apple); `metal-native` sets `Safe`/`Precise` |
</phase_requirements>

## Project Constraints (from CLAUDE.md)

Binding, with the same authority as locked decisions:

1. **Symbolica-clean.** No agent, tool or reviewer reads, runs, benchmarks or compares against Symbolica. `symbolica`
   appears only as a `cargo-deny` ban entry.
2. **Licences.**
   - May be ported, with notices kept in `NOTICE`: SymPy, mpmath, Diofant, FriCAS (BSD-3) and SymEngine (MIT).
   - Never linked or shipped: GPL/LGPL (FLINT, GMP/`rug`, `malachite`, Maxima, Giac, Singular, PARI, Sage).
3. **Offline oracles only.** CPython + SymPy runs only in a container at build or test time. Outputs are frozen and
   pinned by digest, and nothing ships them.
4. **Determinism.**
   - Exact results are byte-identical across WASM, parallel CPU and native/GPU. This is shown by conformance with a
     witness seen RED.
   - Floats are tagged approximate and replayed.
   - Identity is SHA-256 over JCS of the canonical DAG node table.
5. **Claims need oracles.** A test is a witness only after it has failed against unfixed code. Performance is measured
   on a fixed box, never estimated.
6. **No benchmark or third-party corpus bytes in git.** Corpora are fetch recipes plus digests.

From `.claude/CLAUDE.md`:
- exact pins (`serde_json_canonicalizer =0.3.2`, `sha2 =0.11.0`, `ed25519-dalek =3.0.0`, `cubecl =0.11.0`,
  `wit-bindgen 0.62.0`, Rust 1.99.0);
- "What NOT to use": `cargo-component`, `+relaxed-simd`, `rand::StdRng`/`thread_rng`, `HashMap` with `RandomState` on
  output paths, serde-derived `IBig` on the wire, CubeCL `persistence` in the sandbox, minisign/sigstore envelopes;
- the GSD workflow: edits only through GSD commands.

From operator memory (`MEMORY.md`):
- decisions are presented as options with a recommendation;
- ask before spending money or publishing;
- never read secrets;
- review every return independently;
- "every site" claims need both the LSP and tree-sitter;
- no remote GPU box.

## Summary

Phase 1 builds a small, fully guarded skeleton: one wire codec, one trivial operation through one WIT tool on two
targets, one signed manifest path, one pinned SymPy oracle, and four guard families (determinism lints, cargo-deny,
provenance headers, import and SIMD gates), each with a RED witness. It also runs three measurement campaigns that feed
the klein-rs requests: the Monty protocol census, the Metal and Seatbelt probe, and the Wasmtime advisory evidence. None
of this is CAS mathematics. The risks are integration contracts with Klein and determinism leaks.

Five findings change what the planner must do, beyond the project-level research:

1. **Klein has no `klein.plugin.v1` schema and no tool WIT in code.** The document Klein actually verifies today is
   `klein.capability_manifest.v1`. The signature covers that manifest's RFC-8785 bytes, through
   `verify_attested → verify_detached → verify_strict`. Emit capability manifests that match klein-rs's struct
   exactly; define `klein.plugin.v1` here as a bundle index (it becomes D3 input).
2. **A std `wasm32-wasip2` build imports 14 WASI 0.2.9 interfaces**, including `monotonic-clock` and `environment`
   (verified locally). Klein's design says a WASM plugin "imports nothing". A zero-import component was built two ways:
   `no_std` (incompatible with `serde_json_canonicalizer`, which uses `std::io`), and std on `wasm32-unknown-unknown`
   wrapped by `wasm-tools component new`. Build path and import policy need an operator ruling early.
3. **D6 floor.** Wasmtime 46.0.1 is affected by six RustSec advisories. Three of them, published 2026-10-02
   (0325/0326/0327), are patched only at **≥ 49.0.2** or 48.0.4. 49.0.1 is still affected. The locked text says
   "≥ 49.0.1"; the evidence supports asking for ≥ 49.0.2.
4. **CubeCL Metal facts from 0.11.0 sources.**
   - u32 `mul_hi` is lowered through a 64-bit multiply on Metal, so the "u32 mulhi" probe partly measures u64.
   - The `metal` (wgpu-msl) path compiles kernels with a default `MTLCompileOptions`, which Apple documents as fast
     math **on**.
   - `metal-native` sets `MTLMathMode::Safe`.
   - The watchdog limit is undocumented anywhere; it must be measured.
5. **klein-science is a public GitHub repo.** GitHub says self-hosted runners "should almost never be used for public
   repositories". The VM runner (which also builds klein-rs) needs strict trigger hygiene.

**Primary recommendation:** Build in this order:
1. `ksci-wire` with its guards.
2. The trivial `canonicalize` operation through `ksci-ops`, then both shells and the conformance host under both
   Wasmtime pins.
3. The signing path, against klein-rs's actual capability-manifest contract.

Run the oracle, census and Metal/Seatbelt work as a parallel track after `ksci-wire` exists. Gate every claim on a
witness seen RED.

## Architectural Responsibility Map

| Capability | Primary Tier | Secondary Tier | Rationale |
|------------|-------------|----------------|-----------|
| Node table, JCS, identity, envelopes, stdio framing | `ksci-wire` (pure lib) | Python mirror in `standin/sympy/_wire.py` | One codec (D-01); the mirror exists only because Monty cannot call Rust |
| Op registry, logical step budget, plan/map/reduce defaults | `ksci-ops` (pure lib) | — | The only crate that knows the op set (ARCHITECTURE) |
| R0 shell (WIT exports) | `ksci-component` (wasm) | — | Decode → `ksci-ops` → encode; no logic |
| R2 shell (stdio frames) | `ksci-worker` (native bin) | — | Same as R0; per D-08 triples |
| R0 hosting under Wasmtime 46.0.1 / 49.0.2, R0-vs-R2 compare, fuel/memory host errors | `conformance/ksci-conform` (unshipped) | — | Test host, excluded from shipped deny scope |
| Build, digest, manifest, sign, verify, provenance check, box manifest | `xtask` (unshipped) | Mac (signing, human-run) | One manifest writer, one digest helper (D-01) |
| Golden generation, census (dynamic), meijerint share | SymPy oracle container (offline) | Artifact store (GHCR) | Rule 3 |
| Static census, Monty-subset lint, three-level scorer | `conformance/` + `standin/lint` Python (CPython) | — | Unshipped tooling |
| Stand-in skeleton (imports, names, `sympy.abc`) | `standin/sympy` (Monty subset) | `standin/host_shim` (CPython-only) | GUEST-01/02 |
| Metal probe, Seatbelt ablation | `probes/metal` (Mac, unshipped) | kernel log (oracle) | ACCEL-01, DEP-02 |
| Requests D1/D4/D6 | `.planning/requests/*.md` | operator files them | D-13 |

## Standard Stack

### Core (shipped crates)
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| `serde_json_canonicalizer` | **=0.3.2** (2026-02-03, MIT) | RFC 8785 encoder | Same crate and pin as klein-rs [VERIFIED: klein-rs/Cargo.toml:31 `serde_json_canonicalizer = "=0.3.2"`]. **Requires std**: `src/util.rs:7 use std::io;` [VERIFIED: crate source] |
| `sha2` | **=0.11.0** (2026-03-25) | SHA-256 | klein-rs pin [VERIFIED: klein-rs/Cargo.toml:32 `sha2 = "=0.11.0"`] |
| `serde` / `serde_json` | 1.0.229 / 1.0.152 | Wire structs | [VERIFIED: crates.io API] |
| `thiserror` | 2.0.21 | Typed errors | [VERIFIED: crates.io API] |
| `wit-bindgen` | **0.62.0** (2026-09-10) | Guest bindings (component only) | Verified locally: `wit_bindgen::generate!({ world: "science-cas", path: "wit" })` builds a component [VERIFIED: local build] |
| `static_assertions` | 1.1.0 | `assert_not_impl_any!(NodeId: PartialOrd, Ord)` | A compile-time guard (one line); RED shows as a compile error |

### Supporting (unshipped tooling; excluded from the shipped deny graph)
| Library / tool | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| `ed25519-dalek` | **=3.0.0** (BSD-3-Clause) | `xtask sign` / `xtask verify` (`verify_strict` only) | klein-rs pin [VERIFIED: klein-rs/Cargo.toml:71 `ed25519-dalek = "=3.0.0"`] |
| `wasmtime` + `wasmtime-wasi` | **=46.0.1** and **=49.0.2** (renamed deps) | R0 test host under Klein's pin and the D6 target | 46.0.1 = Klein pin [VERIFIED: klein-rs/Cargo.toml:148 `wasmtime = "=46.0.1"`]; 49.0.2 = latest stable, MSRV 1.96.0 [VERIFIED: crates.io API] |
| `cubecl` | **=0.11.0**, features `metal` (+ `metal-native` as second candidate) | Metal probe only in Phase 1 | [VERIFIED: crate sources 0.11.0] |
| `proptest` | 1.12.0 (2026-10-10) or klein-rs's 1.11.0 | Wire round-trip and order-independence properties | Fix `PROPTEST_RNG_SEED` in CI |
| `clap` | 4.6.7 | `xtask` CLI | — |
| `wasm-tools` (CLI) | 1.261.0 | `component wit` import gate, `validate --features=-relaxed-simd`, (`component new` if option B) | [VERIFIED: installed and run locally] |
| `cargo-deny` | 0.20.2 | Licence/bans/advisories gate | Local is 0.19.6 → install the pin |
| `cargo-nextest` | 0.9.148 | Test runner | Local is 0.9.143 |
| `cargo-about` | 0.9.2 | Third-party licence text for bundles | Local is 0.9.1 |
| `oras` | 1.3.4 (Apache-2.0) | GHCR push/pull by digest | Not installed locally (`brew install oras`, or the release tarball checked against `oras_1.3.4_checksums.txt`) |
| Docker | 28.3.3, buildx with linux/amd64 + arm64 | Oracle images | Installed; daemon must be running |
| `uv` | 0.13.0 | `--require-hashes` locks | Local is 0.12.15 |
| SymPy / mpmath / rfc8785 (PyPI) | 1.14.0 / 1.3.0 / 0.1.4 | Oracle image; `rfc8785` only as a test oracle | [WARNING: legitimacy seam flagged SUS (unknown-downloads); see audit] |
| `pydantic-monty` (PyPI) | 1.1.0 (2026-10-05, MIT) | Monty-subset lint cross-check, host-call latency, bigint probe | [WARNING: flagged SUS — too-new, unknown-downloads]. **Do not confuse with PyPI `monty`**, an unrelated package |
| `pyarrow` (PyPI) | latest, hash-locked | Parquet → JSONL corpus extraction (outside the golden image) | [WARNING: flagged SUS — too-new] |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| `static_assertions` | `trybuild` compile-fail test | Heavier (a separate test harness); same guarantee |
| Klein-oracle check via our own `verify_strict` only | Plus an out-of-tree VM-only crate path-depending on klein-rs `klein-compose`, running `verify_attested` on our bytes | The strongest oracle ("Klein's own verifier accepts us") without editing klein-rs. It couples to a private sibling checkout, so it must be excluded from the workspace. Recommended as an optional VM job |
| A two-pin Wasmtime host in one crate | Two host crates | One crate with renamed deps (`wasmtime46 = { package = "wasmtime", version = "=46.0.1" }`) keeps one harness (D-01) |

**Installation (tooling):**
```bash
rustup toolchain install 1.99.0 --target wasm32-wasip2   # rust-toolchain.toml drives this
cargo install --locked wasm-tools@1.261.0 cargo-deny@0.20.2 cargo-nextest@0.9.148 cargo-about@0.9.2
brew install oras   # or the pinned release tarball, checked against its checksums file
```

**Version verification (this session):** crates.io API for every crate above; PyPI JSON for the Python packages;
`static.rust-lang.org/dist/channel-rust-1.99.0.toml` (version `1.99.0 (b940084d7 2026-09-28)`, `wasm32-wasip2`
`available = true`).

## Package Legitimacy Audit

Seam run: `gsd-tools query package-legitimacy check`.

| Package | Registry | Age | Downloads | Source Repo | Verdict | Disposition |
|---------|----------|-----|-----------|-------------|---------|-------------|
| serde_json_canonicalizer | crates | 0.3.2: 8 mo | 11.1M total | github.com/evik42/serde-json-canonicalizer | OK | Approved (klein-rs pin) |
| sha2 | crates | 0.11.0: 7 mo | 1.04B total | RustCrypto/hashes | OK | Approved |
| ed25519-dalek | crates | 3.0.0: 3 mo | 234M total | dalek-cryptography | OK | Approved (klein-rs pin; beware near-name squatters `xs_ed25519-dalek`, `frauth-ed25519-dalek` noted in klein-rs Cargo.toml) |
| wit-bindgen | crates | 0.62.0: 1 mo | 281M total | bytecodealliance | OK | Approved |
| wasmtime / wasmtime-wasi | crates | 46.0.1: 3.5 mo; 49.0.2: 9 days | 39M / 12M | bytecodealliance | OK | Approved (test host only) |
| cubecl | crates | 0.11.0: 5 days | 1.45M total | tracel-ai | OK | Approved (probe only; alpha, exact pin) |
| serde, serde_json, thiserror, proptest, clap, static_assertions, trybuild | crates | mature | high | upstream | OK | Approved |
| sympy | PyPI | 1.14.0: 1.5 yr | n/a (seam has no PyPI download signal) | github.com/sympy/sympy | SUS (unknown-downloads) | Flagged: planner adds `checkpoint:human-verify` on the hash lock |
| mpmath | PyPI | 1.3.0 pinned (latest 1.4.1) | n/a | mpmath.org | SUS (unknown-downloads) | Flagged (same checkpoint) |
| rfc8785 | PyPI | 0.1.4: 2 yr | n/a | trailofbits/rfc8785.py | SUS (unknown-downloads) | Flagged (same checkpoint) |
| pydantic-monty | PyPI | 1.1.0: 6 days | n/a | github.com/pydantic/monty | SUS (too-new, unknown-downloads) | Flagged. It is a meta-package depending on `pydantic-monty-client==1.1.0` and `pydantic-monty-runtime==1.1.0` [VERIFIED: wheel METADATA]; lock and inspect both |
| pyarrow | PyPI | latest: 2 days | n/a | arrow.apache.org | SUS (too-new) | Flagged; pin a release older than 7 days |

**Packages removed due to [SLOP] verdict:** none.
**Packages flagged as suspicious [SUS]:** sympy, mpmath, rfc8785, pydantic-monty, pyarrow. The SUS verdicts on
established PyPI packages come from the seam lacking PyPI download data. One `checkpoint:human-verify` on the
`uv --require-hashes` lockfiles covers them; `pydantic-monty` needs its two sub-packages inspected.
**Confusion hazard:** PyPI `monty` (2026.7.16) is a different project; only `pydantic-monty` is Monty.

## Architecture Patterns

### System Architecture Diagram

```
                ┌────────────── shipped ───────────────┐
 request bytes  │                                      │
 (JCS) ────────►│ ksci-component (R0, wasm)            │──┐
                │   WIT invoke/plan/map/reduce          │  │        ┌──────────────────────────────┐
                │        │                              │  │        │ ksci-conform (unshipped)      │
 frame u32+JCS ►│ ksci-worker (R2, native stdio) ──┐    │  ├───────►│  host A: wasmtime =46.0.1     │
                │        │                         │    │  │        │  host B: wasmtime =49.0.2     │
                │        ▼                         ▼    │  │        │  R2: spawn worker, frames     │
                │   ksci-ops: canonical-input check →   │  │        │  compare bytes R0A==R0B==R2   │
                │     budget(steps) → op → envelope     │  │        │  fuel/memory trap ⇒ host error│
                │        │                              │  │        └──────────────┬───────────────┘
                │        ▼                              │  │                       │ RED/GREEN
                │   ksci-wire: decode → validate →      │  │                       ▼
                │     canonical table → JCS → SHA-256   │  │                  CI on VM (push/dispatch only)
                └──────────────────────────────────────┘  │
 xtask dist ─► .wasm + workers + capability manifests ────┘──► digests ──► oras push GHCR (unsigned)
                                                                      │
 Mac: xtask sign ◄── rebuild wasm, compare digest ◄───────────────────┘
      (human runs; key in Keychain) ──► signature hex ──► xtask verify (verify_strict) ──► bundle by digest

 Oracle track (offline):
 recipe(TIR rev, sha256) ─► oras/HF fetch ─► re-hash ─► extract JSONL ─► hash-rank stratified 1k slice
   ─► docker --network=none image@sha256 ─► seeds {0,1,2,large}×2, arm64+amd64 ─► golden JSONL (JCS via mirror)
   ─► agreement? yes → golden manifest digest in git; no → adjudication queue
   ─► (same image, instrumented) census + meijerint share ─► D4 evidence
 Stand-in runner: image (no SymPy) + standin/ + host_shim ─► three-level scorer vs goldens ─► M0 baseline score

 Metal track (Mac): probe(kernels, fixed-box manifest) ─► sandbox-exec(base ∪ rules) ─► kernel log denials
   ─► add rule until pass ─► remove each rule ⇒ RED ─► D1 evidence
```

### Recommended Project Structure
```
Cargo.toml                 # one workspace; [workspace.lints]; members below
rust-toolchain.toml        # channel = "1.99.0"; targets wasm32-wasip2 (+ wasm32-unknown-unknown if option B),
                           #   aarch64-apple-darwin, x86_64-unknown-linux-gnu
clippy.toml  deny.toml  NOTICE  .cargo/config.toml
wit/science-cas.wit        # invoke/plan/map/reduce over list<u8>
schema/expr.v1.md          # node-table grammar (the contract text; tests cite it)
crates/
  ksci-wire/               # codec, NodeId (no Ord), identity, envelopes, frames
  ksci-ops/                # registry (one op), StepBudget, plan/map/reduce defaults
  ksci-component/          # R0 shell (cdylib)
  ksci-worker/             # R2 shell (bin)
xtask/                     # dist, digest, manifest, sign, verify, provenance, box-manifest, fetch (oras)
conformance/
  ksci-conform/            # Rust: dual-Wasmtime R0 host, R2 driver, byte compare, host-error tests
  oracles/sympy/           # Containerfile, requirements.in/.lock, run_golden.py, census_dynamic.py
  corpora/                 # recipes: tir.toml, omr.toml, rfc8785.toml (digests only)
  golden/                  # golden manifests (digests) + aggregate scores
  census/                  # static AST scanner, report → .planning/requests/D4 evidence
  runner/                  # three-level scorer (CPython)
  boxes/                   # fixed-box manifest records + ratified budgets
standin/
  sympy/                   # __init__.py (generated name table), abc.py, _expr.py, _wire.py (JCS mirror)
  host_shim/               # CPython-only; never shipped
  lint/                    # Monty-subset AST lint
probes/metal/              # ksci-metal-probe (bin) + seatbelt/*.sb + ablation script
plugin/science.cas/        # capability-manifest templates
.planning/requests/        # D1, D4, D6 drafts
```
`deny.toml` `[graph] exclude = ["ksci-conform", "xtask", "ksci-metal-probe"]` (exclusion is recursive)
[CITED: embarkstudios.github.io/cargo-deny/checks/cfg.html]. That keeps Wasmtime 46.0.1's advisories and CubeCL's
tree out of the shipped verdict while one `Cargo.lock` remains.

### Pattern 1: Canonical node table (DET-01/02)
**What:**
- Input is any table: nodes in any order, possibly with duplicates and unreachable nodes.
- The canonical form is an **iterative** post-order DFS from `root`, with children visited in the given argument
  order. Each structurally distinct node is emitted once, at first completion. Duplicates are merged through a
  `BTreeMap<Vec<u8>, NodeId>` keyed by the node's encoded bytes under the canonical child indices.
- Identity = `sha256(jcs({"nodes":[…],"root":n,"v":1}))`.

**Rules:**
- The only JSON numbers are node indices (< 2^32) and `v`.
- Numerals are strings with a strict grammar: Int `0|-?[1-9][0-9]*`; Rat `p`,`q` with `q > 1`.
- Specials are named atoms (`oo`, `-oo`, `zoo`, `nan`, `pi`, `E`, `I`).
- Symbol assumptions are part of the symbol node.
- The decoder rejects:
  - cycles;
  - out-of-range indices;
  - unknown kinds;
  - JSON numbers outside those two positions;
  - lone surrogates (in Python).
- **Argument order is preserved, never sorted at the wire layer.** Structural argument order belongs to `ksci-expr`
  (CORE-01, Phase 2). Sorting here would be a second ordering mechanism.

**Canonical-input check:** the shells re-encode the parsed request and compare bytes. Non-canonical input is
`bad-request`. This makes Klein's `request_digest` unique per request.

**When:** every encode and every identity.

### Pattern 2: Envelopes (DET-03)
```
request  = {"v":1,"op":"science.cas.canonicalize","args":{"expr":<table>},"budget":{"steps":"<decimal>"}}
response = {"v":1,"status":"exact"|"fuel_exhausted","numeric_class":"exact","result":<table>|null,"identity":"<hex>"}
record   = {"request_digest","response_digest","provenance":{"impl","backend","device","driver","wall_ms"}}
```
Only `request` and `response` are hashed. `provenance` is never inside them.

### Pattern 3: Logical step budget vs host errors (DET-04)
- `ksci-ops::StepBudget` is charged one step per node visited by `canonicalize`. Exhaustion returns
  `status: "fuel_exhausted"`, with the same bytes on R0 and R2.
- The Wasmtime host sets `Config::consume_fuel(true)` + `Store::set_fuel(n)` and
  `StoreLimitsBuilder::memory_size(bytes)`. A trap (fuel or OOM) maps to a **host error**, never to a response.
  [VERIFIED: wasmtime 46.0.1 `src/config.rs:612 consume_fuel`, `src/runtime/store.rs:1041 set_fuel`,
  `src/runtime/limits.rs:238 memory_size`; identical names in 49.0.2 at `config.rs:643`, `store.rs:1016`,
  `limits.rs:238`]
- RED witness: a perturbed host that maps `OutOfFuel` to `fuel_exhausted` makes R0 ≠ R2 (R2 has no Wasmtime fuel).

### Pattern 4: WIT (PLUG-02)
```wit
package klein:science@0.1.0;
interface cas {
  variant error { bad-request(string), unsupported-op(string), internal(string) }
  invoke: func(request-jcs: list<u8>) -> result<list<u8>, error>;
  plan:   func(request-jcs: list<u8>) -> result<list<list<u8>>, error>;
  map:    func(piece: list<u8>) -> result<list<u8>, error>;
  reduce: func(request-jcs: list<u8>, outputs: list<list<u8>>) -> result<list<u8>, error>;
}
world science-cas { export cas; }
```
Phase 1 default: `plan` → one piece, `map` = `invoke`, `reduce` → the single output. The `invoke` + `variant error`
subset was built and printed by `wasm-tools component wit` in this session [VERIFIED: local build]. Note that the
`klein:` package namespace is Klein's to assign; `klein:plugin` is named in klein-rs design but defined nowhere
[VERIFIED: rg over klein-rs found no `.wit` files].

### Pattern 5: Signed manifests that Klein can verify (PLUG-01)
- Klein verifies `klein.capability_manifest.v1` [VERIFIED: klein-rs `crates/klein-compose/src/manifest.rs:41`
  `pub const CAPABILITY_MANIFEST_SCHEMA_V1: &str = "klein.capability_manifest.v1";`].
- The struct is `#[serde(deny_unknown_fields)]`, with fields
  `schema, id, version, kind, artifact_digest, input_schema_hash, output_schema_hash, config_schema_hash, provides,
  requires, authority, determinism, publisher, secret_refs, host_protocol, trust_class, limits`
  [VERIFIED: manifest.rs:278-314].
- The signed bytes are `manifest.to_canonical()` (JCS) [VERIFIED: trust.rs:669-672 "The signed byte basis, computed
  once, HERE"]. Verification is `verify_detached(public_key_hex, message, signature_hex, …)` with 64-hex keys and
  128-hex signatures, calling `verifying_key.verify_strict(message, &signature)` [VERIFIED: trust.rs:478-498].

Discrete values to emit, quoted verbatim:
- `kind`: `"adapter"`, `"tool"`, `"provider"`, `"store"`, `"surface"`, `"mcp_server"`, `"process"`
  [VERIFIED: klein-compose/src/plan.rs:49-55]. Use `"tool"` for R0 and `"process"` for R2 [ASSUMED mapping].
- `determinism`: `Deterministic`, `Replayable`, `Live`, serialized `snake_case` →
  `"deterministic"` [VERIFIED: manifest.rs:121-130].
- `trust_class`: `"signed"` [VERIFIED: manifest.rs:149-155 + operator_label 201-208].
- `authority`: a subset of `"read_fs","write_fs","spawn_process","network_read","network_write","git_write",
  "secret_read","external_side_effect"` [VERIFIED: manifest.rs:73-82]. Use the empty set.
- `host_protocol`: `{"min":u32,"max":u32}` [VERIFIED: manifest.rs:218-223]. Values [ASSUMED → D3 question].
- `limits`: `{"timeout_ms":u64,"max_output_bytes":u64}` [VERIFIED: manifest.rs:229-234]. Values [ASSUMED].
- `id`: `namespace/name`, each segment `[a-z0-9._-]` [VERIFIED: id.rs:150-156].
- The namespace must not be in `"compose","digest","effect","event","interpreter","ir","kernel","language",
  "policy","proof","reducer","schema","supervisor"` [VERIFIED: failure.rs:126-140]. `science` is allowed.
- Digests are lowercase 64-hex [VERIFIED: klein-ir/src/digest.rs:150-157 `hex_lower` with `b"0123456789abcdef"`].
- Trust roots are `BTreeMap<key_id, 64-hex public key>` [VERIFIED: lock.rs:153, 428-436].

`klein.plugin.v1` = our bundle index: `{schema:"klein.plugin.v1", plugin, version, components:[{manifest_digest,
artifact_digest, key_id, signature_hex}]}`. It is a D3 proposal [ASSUMED shape]. Use one writer (ksci-wire JCS) and
one signature function for every document.

### Pattern 6: Oracle and scoring (CONF-01/02/03)
- **Image.** `python:3.13-slim-trixie@sha256:70729b46c69b4f1e97c4822c1af3df53a1476cf5ddc6c087c0c10bc3a5678c2f`
  (index). Per-arch digests: amd64 `sha256:8fb4cfa1…9024`, arm64 `sha256:cf97c3b7…b14`. It ships
  `PYTHON_VERSION=3.13.16` [VERIFIED: Docker Hub registry API, image config created 2026-10-06].
- **Env.**
  - `SYMPY_GROUND_TYPES=python`, read as `os.environ.get('SYMPY_GROUND_TYPES', 'auto').lower()`
    [VERIFIED: sympy-1.14.0 `sympy/external/gmpy.py:151`].
  - `MPMATH_NOGMPY=1`, read as `if 'MPMATH_NOGMPY' not in os.environ:` [VERIFIED: mpmath 1.3.0
    `libmp/backend.py:67`].
  - `SYMPY_USE_CACHE=yes` (explicit) [VERIFIED: sympy/core/cache.py:128].
  - `PYTHONHASHSEED`, `LC_ALL=C.UTF-8`, `TZ=UTC`.
- **Run flags.** `--network=none --read-only --tmpfs /tmp --cap-drop=ALL --security-opt=no-new-privileges
  --pids-limit --memory --user nobody`. Per-case subprocess with `RLIMIT_CPU`. Record `timeout` and `error`, never
  drop a case.
- **Slice selection without an RNG.**
  - `seed = sha256(parquet_sha256 ‖ "golden-slice-v1")`.
  - For each stratum, rank rows by `sha256(seed ‖ row_key)` and take the lowest k.
  - Python only guarantees `random()` stability for a compatible seeder, not `sample()`
    [CITED: docs.python.org/3/library/random.html "Notes on Reproducibility"]. Hash ranking is language-independent
    (Rust and Python agree).
- **Strata.** SymPy module per name, from the oracle: `type(getattr(sympy, n)).__module__`/`inspect.getmodule`.
  Produce a name→module table as oracle output.
- **Golden record** (JCS via the Python mirror; `rfc8785` only in tests): `case_id` (sha256 of program bytes),
  `stdout`, `exception` `{type, message}` or null, `status` `ok|error|timeout`. The hash seed and versions go in the
  **run manifest**, not the record, so records compare across seeds.
- **Three levels:**
  - stdout byte-equal;
  - final answer equal. Definition [ASSUMED]: the last non-empty stdout line, whitespace-trimmed.
  - error class equal: exception type name.
- **ImportError location normalisation.** CPython's message embeds the module file path:
  `"cannot import name 'binom' from 'fakesym' (/…/fakesym/__init__.py)"` [VERIFIED: local CPython 3.14.7 probe].
  Normalise the trailing `(…)` before comparing.
- **meijerint share.** Patch `sympy.integrals.integrals.meijerint_definite` (it is bound at module level by
  `from .meijerint import meijerint_definite, …` at integrals.py:1639 and called in `try_meijerg` at :573)
  [VERIFIED: sympy-1.14.0 source]. Count definite `Integral` evaluations, calls, and non-`None` returns.

### Pattern 7: Guards (DET-08, LIC-01, LIC-02) with RED witnesses
| Guard | Mechanism | RED witness |
|---|---|---|
| Hash-map types | `clippy.toml` `disallowed-types`: `std::collections::{HashMap,HashSet}`, `std::hash::RandomState`, `hashbrown::{HashMap,HashSet}`, `foldhash::fast::RandomState`; `[workspace.lints.clippy] disallowed_types = "deny"` | A fixture function using `HashMap` → clippy error |
| Clocks/RNG calls | `disallowed-methods`: `std::time::SystemTime::now`, `std::time::Instant::now`, `rand::thread_rng`, `rand::random`, `rand::rng`; non-maths crates `#![allow]` explicitly | A fixture call → clippy error |
| Interning-id order | `assert_not_impl_any!(NodeId: PartialOrd, Ord)` | Add `#[derive(PartialOrd, Ord)]` → compile error |
| Randomness crates | cargo-deny `[bans] deny = [{ crate = "getrandom", … }, { crate = "rand", … }]` | Add `getrandom` to `ksci-wire` → `cargo deny check bans` RED |
| Licence | cargo-deny `[licenses] allow = [MIT, Apache-2.0, Apache-2.0 WITH LLVM-exception, BSD-2/3-Clause, ISC, Zlib, Unicode-3.0, …]`; `[bans] deny` `rug`, `gmp-mpfr-sys`, `malachite*`, `flint-sys`, `algebraeon`, `symbolica`, `internment` | Add `rug` → RED (licences + bans) |
| Provenance | `xtask provenance`: every `.rs` under `crates/` and `.py` under `standin/` carries `provenance: original` \| `port <project>@<tag>:<path> (<SPDX>)` \| `paper doi:…`; each port project must have a NOTICE entry | Fixture file without a header → RED; a port header whose project is missing from NOTICE → RED |
| No relaxed SIMD | `wasm-tools validate --features=-relaxed-simd <artifact>` | **Seen RED in this session**: a build with `+relaxed-simd` and `f32x4_relaxed_madd` failed with `relaxed SIMD support is not enabled (at offset 0x12296)`; the baseline validated [VERIFIED: local] |
| Import surface | `wasm-tools component wit <artifact>`; compare the import list to an allowlist (empty, or the ruled list) | Add a `SystemTime::now()` call → `wasi:clocks/wall-clock` appears → RED |
| No corpus bytes in git | CI check: no file under `conformance/` other than recipes/manifests/scores; size cap | Commit a JSONL → RED |

### Anti-Patterns to Avoid
- **A hand-written JCS encoder in Rust:** violates the locked `serde_json_canonicalizer` decision. The Python mirror is
  the only second encoder, and it is differential-tested.
- **`rfc8785` (Python) in the golden production path:** that would be a third encoder. Use it only as a test oracle.
- **Sorting commutative arguments in `ksci-wire`:** that is a second ordering mechanism. CORE-01 owns it.
- **Recursion in decode or canonicalize:** the R0 stack is about 512 KiB. Use explicit stacks.
- **Wasmtime fuel mapped to `fuel_exhausted`:** see Pattern 3.
- **Python `random.sample` for corpus selection:** use hash ranking.
- **`pull_request` triggers on the self-hosted runner:** see Security.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| RFC 8785 in Rust | own encoder | `serde_json_canonicalizer =0.3.2` | Number and UTF-16 key rules; klein-rs parity |
| Ed25519 | anything custom | `ed25519-dalek =3.0.0` `verify_strict` | Accept-set parity with Klein (D-R58) |
| WIT bindings / component encoding | manual canonical ABI | `wit-bindgen 0.62.0` + rustc's `wasm-component-ld` (or `wasm-tools component new`) | ABI correctness |
| Import/feature inspection | own wasm parser | `wasm-tools component wit`, `wasm-tools validate --features` | Verified RED/GREEN |
| OCI push/pull | HTTP scripts | `oras push` / `oras blob fetch …@sha256:` | Digest addressing; re-hash after fetch |
| Python dependency pinning | `pip install` | `uv pip install --require-hashes -r requirements.lock` | Oracle drift (Pitfall 16) |
| Compile-time negative trait check | macro tricks | `static_assertions::assert_not_impl_any!` | One line |
| Licence/bans | grep over `Cargo.lock` | `cargo-deny 0.20.2` | Transitive graph, SPDX expressions |

**Key insight:** every Phase 1 deliverable is a contract that later phases test against. Using the same crate as Klein
(JCS, sha2, Ed25519) makes Klein itself an oracle. Hand-rolled versions would need their own oracles.

## Common Pitfalls

### Pitfall A: Std `wasm32-wasip2` drags in 14 WASI imports
**What goes wrong:** the R0 artifact imports clocks, environment, stdio and exit, while Klein's plugin world "imports
nothing" [VERIFIED: klein-rs design/EXTENSION-PLACEMENT.md:29]. The host must then link WASI, and nondeterminism
surfaces (clocks, env) exist.
**Evidence** (rustc 1.97.0, wit-bindgen 0.62.0, `panic=abort`, `lto=fat`, a trivial `invoke`):
```
world root {
  import wasi:io/poll@0.2.9;  import wasi:clocks/monotonic-clock@0.2.9;  import wasi:io/error@0.2.9;
  import wasi:io/streams@0.2.9;  import wasi:cli/stdout@0.2.9;  import wasi:cli/stderr@0.2.9;
  import wasi:cli/stdin@0.2.9;  import wasi:cli/environment@0.2.9;  import wasi:cli/exit@0.2.9;
  import wasi:cli/terminal-input@0.2.9; … terminal-output, terminal-stdin, terminal-stdout, terminal-stderr
  export klein:science/cas@0.1.0;
}
```
Zero imports were verified two ways:
- (a) `#![no_std]` + own `#[global_allocator]` + `#[panic_handler]` + an exported `cabi_realloc` →
  `world root { export klein:science/cas@0.1.0; }` (2,693 bytes). Without the explicit export, linking failed:
  `module does not export a function named cabi_realloc`.
- (b) std + `serde_json_canonicalizer` + `sha2` built for `wasm32-unknown-unknown`, then `wasm-tools component new` →
  zero imports, `wasm-tools validate --features all` VALID. The same crate built for wasip2 had 14 imports.

**How to avoid:** take an operator ruling (Open Question 1) before building `ksci-component`. Gate the import list in
CI either way.
**Warning signs:** `wasm-tools component wit` shows any `import`.

### Pitfall B: Asking klein-rs for "≥ 49.0.1"
**What goes wrong:** 49.0.1 is still affected by RUSTSEC-2026-0325 ("Mis-typed WebAssembly tag imports can lead to GC
heap corruption"), 0326 ("Rooting for GC values live across `try_call` may be missing") and 0327 ("component
async-lifted callback result count is unvalidated, causing a native stack buffer overflow"). Each is patched at
`>= 48.0.4, < 49.0.0` and `>= 49.0.2` (dated 2026-10-02).

Evidence for D6. 46.0.1 is affected by:

| Advisory | Patched in |
|---|---|
| 0222 | ≥ 46.0.2 |
| 0223 | ≥ 46.0.2 |
| 0268 | ≥ 46.0.3 |
| 0269 | ≥ 46.0.3 |
| 0316 | no 46.x fix: ≥ 48.0.3 / ≥ 49.0.1 |
| 0327 | ≥ 48.0.4 / ≥ 49.0.2 |

0315, 0325 and 0326 list `unaffected = ["< 47.0.0"]`. [VERIFIED: rustsec/advisory-db `crates/wasmtime/RUSTSEC-*.md`,
fetched this session]

**How to avoid:** draft D6 as "≥ 49.0.2". Flag the change from the locked "≥ 49.0.1" text to the operator. Use
`=49.0.2` as CI's second host. klein-rs MSRV is fine: `wasmtime 49.0.2` declares `rust_version 1.96.0` and klein-rs's
channel is `"1.96"` [VERIFIED: klein-rs rust-toolchain.toml:8]. The acceptance test that is RED today:
`cargo deny check advisories` (or `cargo audit`) on klein-rs's `Cargo.lock`. klein-rs has no deny.toml and creates its
engine with `wasmtime::Engine::default()` (no fuel, no deterministic relaxed SIMD, no NaN canonicalisation)
[VERIFIED: klein-sandbox/src/wasmtime_heap.rs:69].

### Pitfall C: Signing the wrong bytes
**What goes wrong:** signing the `.wasm`, a hand-built manifest or a non-JCS rendering. Klein signs and verifies the
**JCS bytes of the capability manifest**, and the manifest pins `artifact_digest` (trust.rs:669-683).
**How to avoid:**
- emit manifests through ksci-wire JCS and sign exactly those bytes;
- test `verify_strict` with a byte-flip RED;
- optionally run klein-compose's own `verify_attested` on the VM.

### Pitfall D: Measuring "u32 mulhi" and getting a u64 multiply
**What goes wrong:** on Metal, CubeCL lowers 32-bit `mul_hi` with `expand_u_himul_64` (widen, multiply in 64 bits,
shift). 64-bit `mul_hi` uses `himul_sim` (16/32-bit limb split) [VERIFIED: cubecl-cpp-0.11.0
`src/metal/binary.rs:129-139`; cubecl-core `src/frontend/polyfills.rs:137-169`]. WGSL uses the 64-bit path only if
`supports_u64` [VERIFIED: cubecl-wgpu `src/compiler/wgsl/ops/math.rs:287-300`].
**How to avoid:** the probe times four kernels, each first checked for exact output against a CPU reference:
1. CubeCL `mul_hi` on u32 (as lowered);
2. a hand-written 16-bit-limb u32 mulhi;
3. a u64 `wrapping_mul`;
4. a u32 Montgomery multiply (the real kernel shape).

Report each with median and spread, warm and cold separately.

### Pitfall E: Fast math differs by Metal path
**What goes wrong:**
- **`metal` (wgpu-msl):** `wgpu-hal 30.0.1` builds the MSL passthrough library with a bare `MTLCompileOptions::new()`
  (`src/metal/device.rs:1303-1307`, no `mathMode` and no language version) [VERIFIED: crate source]. Apple documents
  `fastMathEnabled` "The default value is `true`" [CITED: developer.apple.com/documentation/metal/mtlcompileoptions/
  fastmathenabled]. So f32 kernels on that path are fast-math.
- **`metal-native`:** `cubecl-metal 0.11.0` sets `setMathMode(MTLMathMode::Safe)` and
  `setMathFloatingPointFunctions(Precise)` [VERIFIED: `src/compute/context.rs:68-74`].

**How to avoid:** the probe measures the default empirically on both paths. Use an f32 kernel sensitive to FMA
contraction or reassociation, compared bit-for-bit with a CPU IEEE evaluation. Record the result per path in the box
manifest. Exact integer kernels are unaffected.

### Pitfall F: Watchdog trials wreck the session
**What goes wrong:** no source documents the Metal command-buffer timeout [web search found none]. A trial that hits it
can lose the device (wgpu `DeviceLost`) or stall the desktop.
**How to avoid:**
- run each trial in a fresh subprocess, with a loop-count parameter doubled until failure, then a binary search;
- record the failure mode string;
- run with the operator present (human checkpoint), on AC power, with nothing else running.

### Pitfall G: Self-hosted runner on a public repo
**What goes wrong:** `selfsupervised-ai/klein-science` is **public** and klein-rs is private [VERIFIED: `gh api`]. A
fork PR can execute code on the Klein VM [CITED: docs.github.com/en/actions/reference/security/secure-use: self-hosted
runners "should almost never be used for public repositories"].
**How to avoid:**
- trigger only on `push` to `main` and `workflow_dispatch`; never `pull_request` or `pull_request_target` on
  self-hosted labels;
- set "Require approval for all outside collaborators";
- use a dedicated runner group restricted to this repo, running as an unprivileged user;
- give the GHCR token `write:packages` scope only. The operator installs it; agents never read it.

### Pitfall H: Concurrency with klein-rs on the VM and the Mac
GitHub `concurrency` groups do not span repos. klein-rs registers its runner at repo scope with labels
`self-hosted, linux, klein-lane-1` [VERIFIED: klein-rs .github/workflows/ci.yml:26-30]. klein-science therefore needs
its own runner process, and two processes can run builds at the same time.

**Mitigation:** a heavy-job wrapper that takes `flock` on a host lock file and waits while klein-rs `cargo`/`rustc`
processes exist. The same check is a field in the fixed-box manifest on the Mac; the probe refuses to record under load.

### Pitfall I: Remap flags in `.cargo/config.toml` do not expand `$CARGO_HOME`/`$PWD`
STACK.md's snippet writes `--remap-path-prefix=$CARGO_HOME=/cargo`. Cargo config does not expand environment
variables in `rustflags` [ASSUMED]. A `RUSTFLAGS` env also **replaces** config rustflags.

**Mitigation:** `xtask dist` computes absolute prefixes and passes `CARGO_ENCODED_RUSTFLAGS`, including
`-C target-feature=+simd128`.

Verified in this session: building the same crate in two directories with
`--remap-path-prefix=<dir>=/src --remap-path-prefix=$HOME/.cargo=/cargo` gave identical SHA-256
`faacd95c…dd545` for both `.wasm` files, with no `/Users/` strings.

### Pitfall J: Golden flakiness (Pitfall 16 refined)
- Cases that time out near the CPU limit flip between runs. Treat any status disagreement across the 8 seed runs or
  2 arches as adjudication.
- `Dummy` prints as `_x`. Normalise only if it is observed.
- Warnings printed by SymPy to stderr are not in stdout scoring.

### Pitfall K: The stand-in name table and `sympy.abc`
- SymPy 1.14.0 `__all__` has **902** names and no module-level `__getattr__` [VERIFIED: AST of sympy-1.14.0
  `sympy/__init__.py`]. Generate the stand-in table from the oracle (header `port sympy@sympy-1.14.0:sympy/__init__.py
  (BSD-3-Clause)`), which makes SymPy's NOTICE required in Phase 1.
- `sympy/abc.py` builds its namespace with `exec('from sympy import *', ns)` and the `string` module (lines 88-96).
  Generate `abc.py` statically instead (Monty has no `string` module in its list).
- Placeholders must be plain `def`s or module-level values. Monty does not dispatch `__call__` on instances.

### Pitfall L: OMR dynamic census undercounts
OMR programs co-import numpy and scipy, which are absent from the oracle image. They fail at import, so the dynamic
census misses them.

**Mitigation:** record `import_failed:<module>` per program, treat those programs as static-only, and report the share.
Do not add numpy to the golden image (it would move its digest).

### Pitfall M: egglog 3.0 on wasm (research flag; Phase 4 consumer)
- `egglog =3.0.0` with `default-features = false` builds for `wasm32-wasip2`.
- Its component imports `wasi:clocks/wall-clock`, `wasi:filesystem/{types,preopens}` and `wasi:random/insecure-seed`
  beyond the std set [VERIFIED: local build + `wasm-tools component wit`].
- `rand 0.9.5` is a **normal** dependency of `egglog-core-relations 3.0.0`, pulling `getrandom 0.3.4`
  [VERIFIED: `cargo tree -i getrandom`]. This contradicts STACK.md's "rand only in tests".
- The `wasm32-unknown-unknown` build fails on `getrandom` without `wasm_js`.

Record this for Phase 4 and the im-rc ruling. It also means cargo-deny's `getrandom` ban will fire when egglog lands.
Single-thread determinism was **not** measured.

## Code Examples

### wit-bindgen guest (built in this session)
```rust
// Source: local probe, wit-bindgen =0.62.0, rustc 1.97.0
wit_bindgen::generate!({ world: "science-cas", path: "wit" });
use exports::klein::science::cas::{Error, Guest};
struct C;
impl Guest for C {
    fn invoke(req: Vec<u8>) -> Result<Vec<u8>, Error> {
        // decode → ksci_ops::invoke → encode; no logic in the shell
        ksci_ops::invoke(&req).map_err(|e| Error::BadRequest(e.to_string()))
    }
}
export!(C);
```

### Verification (mirror of Klein's one call site)
```rust
// Source: klein-rs crates/klein-compose/src/trust.rs:478-498 (pattern, not a dependency)
let vk = ed25519_dalek::VerifyingKey::from_bytes(&key_bytes)?;      // 32 bytes from 64 hex
let sig = ed25519_dalek::Signature::from_bytes(&sig_bytes);         // 64 bytes from 128 hex
vk.verify_strict(manifest_jcs_bytes, &sig)?;                        // never `verify`
```

### CubeCL u32 mul_hi kernel (CubeCL's own test shape)
```rust
// Source: cubecl-core-0.11.0/src/runtime_tests/binary.rs:714-760
#[cube(launch_unchecked)]
fn test_mulhi_kernel<N: Size>(lhs: &[Vector<u32, N>], rhs: &[Vector<u32, N>], output: &mut [Vector<u32, N>]) {
    if ABSOLUTE_POS < rhs.len() {
        output[ABSOLUTE_POS] = lhs[ABSOLUTE_POS].mul_hi(rhs[ABSOLUTE_POS]);
    }
}
// launch: test_mulhi_kernel::launch_unchecked(&client, CubeCount::Static(1,1,1), CubeDim::new_1d(n), vec,
//   BufferArg::from_raw_parts(lhs_handle, len), …); client.read_one_unchecked(output_handle)
```

### Seatbelt base profile (start of the D-15 ablation)
```scheme
; Source: klein-rs crates/klein-tools/src/process_sandbox.rs:1598-1610 and :1647 (verbatim rules)
(version 1)
(deny default)
(deny network*)
(allow process-exec (literal "<probe image>"))
(allow file-read-data (literal "/"))
(allow sysctl-read (sysctl-name "hw.pagesize_compat"))
(allow process-fork)
; candidate pool (add one at a time; Chromium gpu.sb, BSD) [CITED: chromium sandbox/policy/mac/gpu.sb]:
;  (allow iokit-open (iokit-connection "IOAccelerator"))
;  (allow iokit-open (iokit-user-client-class "IOSurfaceRootUserClient"))
;  (allow mach-lookup (xpc-service-name "com.apple.MTLCompilerService"))
;  (allow file-read* (subpath "/System/Library/Extensions"))
;  (allow sysctl-read (sysctl-name "hw.memsize") …)   (allow user-preference-read)
;  plus the Metal shader-cache directory rule if a file-write denial appears
```
Oracle: the unified log's sandbox denials for the probe pid (for example
`log show --last 2m --predicate 'eventMessage CONTAINS "deny"'`) [ASSUMED predicate; klein-rs used a "kernel-log
oracle" for the same purpose, process_sandbox.rs:1626]. Each kept rule: removing it ⇒ the probe fails and the
specific denial reappears.

### Monty probe (D4 evidence; upstream Monty, labelled proxy)
```python
# Source: pydantic/monty README (fetched 2026-10-11)
from pydantic_monty import Monty
with Monty() as pool:
    with pool.checkout() as session:
        session.feed_run(code, inputs={...}, external_lookup={'tool': host_fn})
```
Measure:
- host-call round-trip median over 10k calls;
- `2**200`, `10**400 + 1`, `1/2`, `repr(0.1)` and `f"{x:.4f}"` against CPython.

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `cargo-component` | rustc `wasm32-wasip2` + `wasm-component-ld`, or `wasm-tools component new` | 2025 | No extra packager |
| `fastMathEnabled` | `mathMode` + `mathFloatingPointFunctions` | macOS 15 (deprecated) [CITED: Apple docs] | Only `metal-native` sets them in CubeCL |
| Wasmtime 46.x line | 49.0.2 (latest stable); 50.0.0-rc.1 needs rustc 1.97 | 2026-10-02/05 | D6 floor ≥ 49.0.2 |
| Monty beta | Monty 1.0 (2026-09-25), 1.1.0 (2026-10-05) | 2026-09 | Its limits list still excludes the dunders the stand-in needs |

**Deprecated/outdated in project docs (corrections):**
- STACK.md "wasmtime and cranelift need 1.97": `wasmtime 49.0.2` declares rust_version `1.96.0`. Only 50.0.0-rc.1 needs
  1.97.
- STACK.md "CPython 3.13.14": the current pinned image ships `3.13.16`.
- STACK.md "egglog-core-relations rand only in tests": rand is a normal dependency.
- STACK.md's `.cargo/config.toml` `$CARGO_HOME` remap: see Pitfall I.

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | R0 manifest `kind:"tool"`, R2 `kind:"process"`; `host_protocol` and `limits` values | Pattern 5 | Klein refuses the manifest at D3; edit before release, since the signature covers it |
| A2 | `klein.plugin.v1` bundle-index shape | Pattern 5 | Rework when D3 defines it |
| A3 | "Final answer" = last non-empty stdout line | Pattern 6 | Level-2 scores mis-measured; cheap to change (scores are recomputed) |
| A4 | Cargo config does not expand `$VAR` in rustflags | Pitfall I | Low; the xtask approach works either way |
| A5 | Wasmtime 46.0.1 instantiates our 0.2.9-importing (or zero-import) component | Pattern 3 | Both hosts ship WASI `0.2.12` [VERIFIED: wasmtime-wasi wit/deps]; instantiation itself is untested here |
| A6 | Mac-native and Linux-native workers rebuild bit-identically on the same host | PLUG-01 | Only wasm was verified; the two-clean-builds gate will tell |
| A7 | Cross-host (Mac vs VM) wasm rebuild is identical (D-07) | PLUG-01 | If not, D-07's digest match fails; investigate the producers section and the LLVM build |
| A8 | Unified-log predicate for sandbox denials | Seatbelt example | Use klein-rs's oracle method instead |
| A9 | macOS cannot hard-pin rayon threads to P-cores; QoS steering only | CONF-06 | "All P-cores" budgets are approximate; record QoS in the manifest |
| A10 | Upstream `pydantic-monty` 1.1.0 latency is representative of Klein's future embedding | DEP-01 | Label as proxy evidence |
| A11 | RFC 8785 vectors are "third-party corpus bytes" under rule 6, so fetched by recipe | DET-01 | If the operator allows vendoring, simplify |

## Open Questions (RESOLVED)

All five were ruled by the operator on 2026-10-11 and recorded in 01-CONTEXT.md as D-16..D-20.

1. **R0 build path and import policy (blocks `ksci-component`).** RESOLVED by D-16: option (b), zero imports.
   - Known: std wasip2 = 14 WASI imports; zero imports is achievable (no_std, or std on `wasm32-unknown-unknown` +
     `wasm-tools component new`); `serde_json_canonicalizer` needs std.
   - Unclear: whether Klein will link deterministic WASI for plugins or require zero imports.
   - Options:
     - (a) keep wasip2 and gate the import list to the observed 14; ask Klein for deterministic WASI;
     - (b) std on `wasm32-unknown-unknown` + `component new`: zero imports, deviates from the "wasm32-wasip2" wording,
       adds wasm-tools to the reproducibility chain, and makes `getrandom` unbuildable (a free guard);
     - (c) no_std: blocked by the JCS crate.
   - Recommendation: **(b)**, pending operator ruling. Raise it as a checkpoint before Plan 2.
2. **D6 floor:** RESOLVED by D-17: ≥ 49.0.2. ≥ 49.0.2 vs the locked "≥ 49.0.1". Recommend ≥ 49.0.2 (Pitfall B).
3. **Rational lowest-terms enforcement:** RESOLVED by D-20: grammar only in Phase 1. needs bignum gcd. Recommend grammar-only validation in Phase 1 and
   lowest-terms validation in `ksci-num` (Phase 2) with a witness. Otherwise `ksci-wire` takes `dashu-int` now.
4. **Census population for TIR:** RESOLVED by D-18: full TIR plus the 10k OMR sample. the full ~46k SymPy programs (about 2 h on the VM) vs the 1k slice. Recommend full
   TIR plus the 10k OMR sample. It is compute-only and gives a stronger D4 table.
5. **RFC 8785 vectors in git** (A11): RESOLVED by D-19: fetched by recipe. recipe at Cyberphone `19d51d7fe467d4706a3ff08adf8a748f29fc21e0` with
   klein-rs's 18 digests. Spot-checked in this session: `input/values.json c4a041b5…`, `output/values.json 2d5e01a3…`,
   `input/weird.json a3a90526…`, `output/weird.json 6af595a9…` all match.

## Environment Availability

| Dependency | Required By | Available | Version | Fallback |
|------------|------------|-----------|---------|----------|
| Rust 1.99.0 | all crates | ✗ (1.97.0 installed) | — | `rust-toolchain.toml` auto-installs; on channel [VERIFIED] |
| wasm32-wasip2 target | R0 | ✓ for 1.97.0 (added this session) | — | auto via toolchain file |
| wasm-tools | gates | ✗ globally (scratch install only) | 1.261.0 | `cargo install --locked` |
| cargo-deny / nextest / about | guards, tests | ✓ but older | 0.19.6 / 0.9.143 / 0.9.1 | install pins |
| Docker + buildx (amd64 emulation) | oracle, D-10 arch check | ✓ installed; **daemon stopped mid-session** | 28.3.3 | start Docker Desktop; amd64 natively on the VM |
| oras | artifact store | ✗ | — (1.3.4 latest) | brew / release tarball |
| uv | hash locks | ✓ older | 0.12.15 | install 0.13.0 |
| sandbox-exec | Seatbelt ablation | ✓ | macOS 26.5.2 (25F84) | — |
| Metal GPU | probe | ✓ | Apple M3 Max, 30 cores, Metal 4 | — |
| Mac power | fixed box | currently **on battery** | — | box manifest records `pmset -g batt`; refuse to record on battery |
| klein-rs checkout | contracts (read-only) | ✓ `/Users/das/CodeProjects/klein-rs` @ `d4436b3d` | — | — |
| GHCR write token | first push | unknown | — | operator action (checkpoint) |
| Klein VM runner for this repo | CI | unknown (klein-rs has one at repo scope) | — | operator registers it |

**Missing with no fallback:** GHCR credentials and the VM runner registration (both operator actions).
**Missing with fallback:** wasm-tools, oras and pinned tool versions (install steps).

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Rust: `cargo-nextest 0.9.148` + `proptest`; Python: stdlib `unittest` (no pytest in images) |
| Config file | none yet: Wave 0 creates `.config/nextest.toml` with a CI profile and a fixed `PROPTEST_RNG_SEED` |
| Quick run command | `cargo nextest run -p ksci-wire -p ksci-ops` |
| Full suite command | `cargo xtask ci` (fmt, clippy -D warnings, nextest, deny, provenance, dist, gates, conform) |

### Phase Requirements → Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| DET-01 | Two construction orders → same identity; RFC vectors; Rust == Python mirror == rfc8785 | unit + property + differential | `cargo nextest run -p ksci-wire identity`; `python -m unittest standin.tests.test_wire` | ❌ Wave 0 |
| DET-02 | JSON number outside indices/`v` rejected; numeral grammar; 300-digit Int round trip | unit | `cargo nextest run -p ksci-wire numerals` | ❌ |
| DET-03 | Provenance change leaves response digest unchanged | unit | `cargo nextest run -p ksci-wire envelope` | ❌ |
| DET-04 | Small budget → `fuel_exhausted` bytes R0 == R2; Wasmtime fuel/memory → host error (both pins) | integration | `cargo nextest run -p ksci-conform fuel` | ❌ |
| DET-08 | clippy bans, NodeId no-Ord, deny getrandom; each RED once | lint + compile | `cargo clippy --workspace --all-targets -- -D warnings`; `cargo deny check` | ❌ |
| CONF-01 | Same golden bytes on 2 runs × 4 seeds; arm64 == amd64 | container | `cargo xtask oracle golden --verify` (slow; VM) | ❌ |
| CONF-02 | Only recipes/manifests/scores in git; fetch re-hashes | CI check | `cargo xtask check-no-corpus` | ❌ |
| CONF-03 | Three levels scored; ImportError path normalised | unit + run | `python -m unittest conformance.runner.tests` | ❌ |
| CONF-06 | Box manifest schema; refuses under load or on battery | unit | `cargo nextest run -p xtask box_manifest` | ❌ |
| LIC-01 | `rug` → deny RED | CI witness | `cargo deny check licenses bans` | ❌ |
| LIC-02 | Missing header → RED; NOTICE entries | unit | `cargo xtask provenance` | ❌ |
| DEP-01..03 | Request drafts contain evidence, ask, and a RED acceptance test | manual review | `checkpoint:human-verify` | ❌ |
| PLUG-01 | Two clean builds same digests; `verify_strict` pass; byte-flip RED | integration | `cargo xtask dist --twice && cargo xtask verify` | ❌ |
| PLUG-02 | `canonicalize` R0(46.0.1) == R0(49.0.2) == R2; perturbation RED | integration | `cargo nextest run -p ksci-conform r0_r2` | ❌ |
| GUEST-01 | 5 import forms, no tool call; missing name → ImportError (normalised) | unit (CPython) | `python -m unittest standin.tests.test_imports` | ❌ |
| GUEST-02 | Monty-subset lint passes; fixture `class A(B)` RED; Monty parser agrees | lint | `python -m standin.lint standin/sympy` | ❌ |
| ACCEL-01 | Kernels exact vs CPU before timing; manifest attached | manual-run (Mac) | `cargo xtask probe metal` | ❌ |

### Sampling Rate
- **Per task commit:** `cargo nextest run -p <touched crate>` + `cargo clippy -p <crate> -- -D warnings`.
- **Per wave merge:** `cargo xtask ci` (without oracle/probe).
- **Phase gate:** full suite green, all RED witnesses recorded (commit + CI log excerpt per guard), oracle
  reproducibility report, probe report with box manifest, and three request drafts approved.

### Wave 0 Gaps
- [ ] Workspace skeleton: `Cargo.toml`, `rust-toolchain.toml`, `clippy.toml`, `deny.toml`, `.config/nextest.toml`
- [ ] `crates/ksci-wire/tests/` (identity, numerals, envelope, rfc8785-by-recipe)
- [ ] `standin/tests/` (unittest) and `conformance/runner/tests/`
- [ ] `xtask` subcommands: `ci`, `provenance`, `dist`, `verify`, `box-manifest`, `check-no-corpus`
- [ ] Tool installs: wasm-tools 1.261.0, cargo-deny 0.20.2, nextest 0.9.148, oras 1.3.4, uv 0.13.0

## Security Domain

`security_enforcement: true`, ASVS level 1.

### Applicable ASVS Categories
| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V2 Authentication | no (no users) | — |
| V3 Session Management | no | — |
| V4 Access Control | yes (CI, registry) | Private GHCR; runner group restricted to this repo; `push`/`workflow_dispatch` triggers only |
| V5 Input Validation | yes | ksci-wire strict decode: size cap per frame, iterative traversal, index-range and cycle checks, numeral grammar, canonical-input byte check; u32 frame length cap in the worker |
| V6 Cryptography | yes | `ed25519-dalek =3.0.0` `verify_strict` only; `sha2 =0.11.0`; no custom crypto |
| V10 Malicious Code | yes | TIR/OMR programs are untrusted code: run only in `--network=none --read-only --cap-drop=ALL --security-opt=no-new-privileges --pids-limit --memory --user nobody` containers |
| V14 Configuration / Supply chain | yes | cargo-deny; uv `--require-hashes`; images by digest; `oras` fetch then re-hash; exact pins |

### Known Threat Patterns for this stack
| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| Fork PR executes on the self-hosted VM (which also builds klein-rs) | Elevation of privilege | No `pull_request` triggers on self-hosted; approval for outside collaborators; dedicated unprivileged runner user (Pitfall G) |
| Signing-key exfiltration | Information disclosure | Key only in the Mac Keychain; `xtask sign` run by the operator (human-action checkpoint); never on the VM or in agent output |
| Tampered artifact in GHCR | Tampering | Digests in git; re-hash after `oras blob fetch`; signatures over manifest JCS that pins `artifact_digest` |
| Oracle container escape or persistence by corpus code | Tampering / EoP | Hardened `docker run` flags; read-only mounts; tmpfs only |
| Dependency confusion (`monty` vs `pydantic-monty`; ed25519 near-names) | Spoofing | Exact names + hash locks; Package Legitimacy Audit |
| Over-broad Seatbelt grants filed into Klein | EoP | D-15 ablation; no wildcard `iokit-open`/`mach-lookup`; each rule has a RED witness |
| Malformed request DoS (deep nesting, huge numerals) | DoS | Iterative algorithms, size caps, logical step budget; Wasmtime fuel and memory as a backstop host error |

## Sources

### Primary (HIGH: read or run this session)
- klein-rs @ `d4436b3d`:
  - `crates/klein-compose/src/{trust.rs,manifest.rs,id.rs,plan.rs,failure.rs,lock.rs}`
  - `crates/klein-ir/src/digest.rs`
  - `crates/klein-tools/src/process_sandbox.rs`
  - `Cargo.toml`, `rust-toolchain.toml`
  - `design/EXTENSION-PLACEMENT.md`, `.github/workflows/ci.yml`
  - `spikes/jcs-parity/fixtures/PROVENANCE.md`
- Crate sources (crates.io downloads): cubecl, cubecl-core, cubecl-cpp, cubecl-wgpu, cubecl-metal 0.11.0;
  wgpu-hal 30.0.1; wasmtime 46.0.1 / 49.0.2; wasmtime-wasi 46.0.1 / 49.0.2; serde_json_canonicalizer 0.3.2;
  wit-bindgen 0.62.0; egglog-core-relations 3.0.0.
- Local experiments (rustc 1.97.0):
  - wasip2 imports (14);
  - zero-import builds (no_std; wasm32-unknown-unknown + `component new`);
  - two-path reproducibility (identical SHA-256);
  - relaxed-SIMD gate RED/GREEN;
  - egglog wasip2 imports and `cargo tree`;
  - CPython ImportError text.
- Registries: crates.io API; PyPI JSON; Docker Hub registry API (python:3.13-slim-trixie);
  `static.rust-lang.org` channel files; Hugging Face API:
  - TIR rev `77a91d7b7a1a98ac4b1beb7d86c09d156b935dcd`, train parquet LFS oid `66163f9f…61d8` (147,342,955 bytes),
    Apache-2.0;
  - OMR rev `d3d08664755704f422af97d43a7ff0ded4bd95df`, 72 `tir-*` shards of about 214 MB each (e.g.
    `tir-00000` oid `ed2e761c…abdb`), CC-BY-4.0.
- rustsec/advisory-db `crates/wasmtime/` (0222, 0223, 0268, 0269, 0315, 0316, 0325, 0326, 0327).
- SymPy 1.14.0 sources: `__init__.py`, `abc.py`, `external/gmpy.py`, `core/cache.py`, `integrals/integrals.py`;
  mpmath 1.3.0 `libmp/backend.py`.

### Secondary (MEDIUM: official docs)
- [Apple: MTLCompileOptions.fastMathEnabled](https://developer.apple.com/documentation/metal/mtlcompileoptions/fastmathenabled)
  and [mathMode](https://developer.apple.com/documentation/metal/mtlcompileoptions/mathmode)
- [cargo-deny bans config](https://embarkstudios.github.io/cargo-deny/checks/bans/cfg.html),
  [cargo-deny graph config](https://embarkstudios.github.io/cargo-deny/checks/cfg.html)
- [GitHub Actions secure use](https://docs.github.com/en/actions/reference/security/secure-use)
- [oras blob fetch](https://oras.land/docs/commands/oras_blob_fetch)
- [Monty limitations: classes](https://pydantic.dev/docs/monty/limitations/classes/),
  [modules](https://pydantic.dev/docs/monty/limitations/modules/),
  [language](https://pydantic.dev/docs/monty/limitations/language/); pydantic/monty README
- [Chromium gpu.sb](https://chromium.googlesource.com/chromium/src/+/main/sandbox/policy/mac/gpu.sb) (BSD; a candidate
  rule pool, not authoritative for Apple)

### Tertiary (LOW)
- Web search on the Metal watchdog duration: no documented number found
  ([MoltenVK #2080](https://github.com/KhronosGroup/MoltenVK/discussions/2080),
  [Apple forum 681995](https://developer.apple.com/forums/thread/681995),
  [MLX #3583](https://upd.dev/ml-explore/mlx/issues/3583)).
- Web search on wasip2 imports ([wasm32-wasip2 target spec](https://doc.rust-lang.org/beta/nightly-rustc/src/rustc_target/spec/targets/wasm32_wasip2.rs.html),
  [Component Model Rust guide](https://component-model.bytecodealliance.org/language-support/creating-runnable-components/rust.html));
  superseded by the local experiment.

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH. Registry-verified; the shared pins are read from klein-rs.
- Architecture: MEDIUM-HIGH. Contracts are read from klein-rs code. `klein.plugin.v1`, the manifest field values and
  the WIT namespace are ASSUMED pending D3.
- Pitfalls: HIGH for the wasm imports, advisories, CubeCL lowering, fast-math path and runner exposure (each tool
  verified). MEDIUM for Seatbelt (Chromium reference). LOW for the watchdog (measure).

**Research date:** 2026-10-11
**Valid until:** 2026-10-25 for Wasmtime, CubeCL and Monty (fast-moving; new advisories land weekly); 30 days for the
rest.
