# Phase 1: Contracts, Oracles and Risk Probes - Context

**Gathered:** 2026-10-11
**Status:** Ready for planning

<domain>
## Phase Boundary

Phase 1 (operator milestone M0) fixes the contracts and oracles that every later layer tests against. It also measures
the largest unknowns. It delivers:
- `ksci-wire`: the JCS node table, string-tagged numbers, SHA-256 identity, envelopes and the RFC 8785 vectors;
- the WIT tool with one trivial operation, byte-identical on R0 (`wasm32-wasip2` component) and R2 (native worker);
- reproducible signed `klein.plugin.v1` builds;
- the digest-pinned SymPy oracle and goldens for a TIR slice;
- the determinism and licence guards, each seen RED;
- the stand-in skeleton (imports, Monty-subset lint, CPython `host_shim`);
- the CubeCL Metal probe and the per-box budgets;
- three klein-rs requests (D1, D4, D6).

Requirements: DET-01..04, DET-08, CONF-01..03, CONF-06, LIC-01..02, DEP-01..03, PLUG-01..02, GUEST-01..02, ACCEL-01.
No CAS maths lands here.

</domain>

<decisions>
## Implementation Decisions

### Carried forward (operator rulings 2026-10-11, PROJECT.md Key Decisions, not re-asked)
- Identity is SHA-256 of the JCS of the canonical DAG node table. CLAUDE.md rule 4 was amended to say so on 2026-10-11.
- `fuel_exhausted` comes only from a logical step budget counted identically on R0, R1 and R2. Wasmtime fuel, memory
  caps and GPU timeouts are host errors.
- The Wasmtime request (D6). Builds use `+simd128` and never `+relaxed-simd`. CI runs R0 under Klein's current pin
  and under the D6 floor. **The floor is now ≥ 49.0.2 (D-17 supersedes the earlier ≥ 49.0.1).**
- Golden bytes live in an artifact store by digest. Git holds recipes, manifests and scores.
- Budgets are ratified per fixed box (Mac: all P-cores against one P-core, Metal against that).
- JCS uses `serde_json_canonicalizer` =0.3.2 (the same crate and pin as klein-rs), mirrored once in Python for the
  stand-in.
- The D4 request is widened to the full protocol set from the census.

### Design principles (operator, binding on every plan and task)
- **D-01:** **DRY, SOLID, no bloat.** There is one mechanism per concern: one JCS codec (`ksci-wire`), one digest
  helper, one manifest writer, one fixed-box manifest schema used by the probe and all benchmarks, and one artifact-store
  client used for goldens, corpora and bundles. Add no speculative abstractions, crates, flags or config beyond what a
  Phase 1 requirement needs. Deleting beats adding. A plan that adds a second path to an existing concern is wrong. See
  memory `principled-design`.

### Naming and layout
- **D-02:** The crate prefix is **`ksci-`** (for example `ksci-wire`, `ksci-component`, `ksci-worker`). Every `ks-`
  name in the research docs maps to `ksci-`. — **Reversibility:** costly — every crate path, `use` line and manifest
  name would change.
- **D-03:** CLAUDE.md rule 4 was amended now, before planning, and is done. DET-01 is no longer blocked.

### CI, artifact store and keys
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

### Oracle slice and census
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

### klein-rs requests (D1, D4, D6)
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

### Post-research rulings (operator, 2026-10-11, answering 01-RESEARCH.md Open Questions)
- **D-16:** **R0 imports nothing.** The R0 component is built as std Rust on `wasm32-unknown-unknown` and wrapped with
  `wasm-tools component new` (pinned 1.261.0). It has zero imports. A CI gate fails if the component's import list is
  non-empty, and that gate is seen RED once on a wasip2 build. Reason: klein-rs `design/EXTENSION-PLACEMENT.md` §2 puts a
  WASM plugin as pure computation with no I/O, and effects go only through host capabilities (one mechanism per
  effect, replayable). Zero imports also makes `getrandom` and clock use unbuildable, so it is a free determinism
  guard. This replaces the "`wasm32-wasip2`" wording for R0 everywhere in this phase; wasm-tools becomes part of the
  reproducibility chain. The D6 request may say we would accept deterministic WASI, but we do not depend on it.
- **D-17:** **The D6 floor is Wasmtime ≥ 49.0.2.** Three advisories dated 2026-10-02 (RUSTSEC-2026-0325, 0326, 0327)
  are patched only at ≥ 49.0.2 or 48.0.4. CI's second R0 host pin is 49.0.2.
- **D-18:** **The census runs over all ~46k SymPy-using TIR programs plus the ~10k OMR sample** (refines D-12; compute
  only, on the VM).
- **D-19:** **RFC 8785 test vectors are fetched by recipe**, never vendored: Cyberphone commit
  `19d51d7fe467d4706a3ff08adf8a748f29fc21e0` with klein-rs's 18 digests, re-hashed after fetch (rule 6).
- **D-20:** **Rationals are checked against the numeral grammar only in Phase 1.** Lowest-terms validation lands in
  `ksci-num` in Phase 2 with its own witness, so `ksci-wire` takes no bignum dependency now. (Research recommendation,
  taken as the default; not separately asked.)

### Claude's Discretion
- The repo layout for unshipped tooling. Follow the ARCHITECTURE.md tree (`crates/`, `plugin/`, `xtask/`,
  `conformance/golden/` for manifests). Put the probe, the oracle images and the census tooling in a clearly unshipped
  location, outside the shipped bundles and outside the `cargo-deny` scope of the shipped crates where that applies.
- Which trivial operation runs through the WIT tool on R0 and R2, how far the stand-in skeleton goes beyond GUEST-01/02,
  and the fields of the fixed-box manifest. All within the requirements and D-01.

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Project rules and decisions
- `CLAUDE.md`: the binding rules (Symbolica-clean, licences, offline oracles, determinism with the amended rule 4,
  claims need oracles, no corpus bytes in git).
- `.claude/CLAUDE.md`: the stack pins, version facts and "What NOT to use".
- `.planning/PROJECT.md`: the operator rulings of 2026-10-11 (Key Decisions table).
- `.planning/REQUIREMENTS.md`: the text of DET, CONF, LIC, DEP, PLUG, GUEST and ACCEL-01.
- `.planning/ROADMAP.md`: the Phase 1 success criteria, "Dependencies on Klein core" (D1–D6) and Infrastructure.

### Design and research
- `.planning/research/SCIENCE-PLUGIN-CAS.md`: the single source for design and decisions.
- `.planning/research/SUMMARY.md`: the "Phase 1 (M0)" delivery list, "Operator decisions needed" and "Gaps to Address".
- `.planning/research/ARCHITECTURE.md`: crate boundaries (`ks-wire` → `ksci-wire`), the repo tree, the build order and
  the data flow.
- `.planning/research/STACK.md`: the exact pins; §8 WASM component; §9 JCS and identity; §10 signing; §11 offline
  oracle harness; deny.toml; CubeCL Metal facts.
- `.planning/research/PITFALLS.md`: pitfalls 1, 4, 5, 13, 15–19, 24 and 25 (the ones Phase 1 must avoid), and the M0
  witness table.
- `.planning/research/FEATURES.md`: the TIR census method and the corpus pins (NuminaMath-TIR rev and parquet digest).

### External (read-only)
- klein-rs `klein-compose/src/trust.rs` (`verify_detached`, `verify_strict`), `design/16-dependencies.md`,
  `Cargo.lock` and `rust-toolchain.toml`. Read-only; never edited.
- RFC 8785 (JCS) and its test vectors.

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets
- None. The repo is greenfield: only planning docs, `CLAUDE.md`, `LICENSE` and `README.md` exist.

### Established Patterns
- None in code yet. Phase 1 sets them, and D-01 (DRY, SOLID, no bloat) applies from the first crate.
- klein-rs patterns to match without editing klein-rs: `serde_json_canonicalizer` =0.3.2, `sha2` =0.11.0,
  `ed25519-dalek` =3.0.0, and raw detached Ed25519 with `verify_strict`.

### Integration Points
- Klein's tool WIT and the `klein.plugin.v1` manifest format, consumed by klein-rs's plugin loader. D3 (`PluginArchive`)
  isn't there yet, so CI verifies the signature and digest itself.
- Klein's Wasmtime host (pin 46.0.1, and ≥ 49.0.1 after D6). The R0 artifact is tested under both.

</code_context>

<specifics>
## Specific Ideas

- The Mac signing step's digest match against the VM build is also the cross-host reproducibility evidence. It is one
  mechanism, not a separate check.
- The same instrumented oracle run serves the protocol census and the `meijerint` share measurement.
- The probe's fixed-box manifest schema is the same one every later benchmark uses.

</specifics>

<deferred>
## Deferred Ideas

- Full-corpus TIR goldens: Phase 2.
- The numpy/scipy facade decision: after Phase 4. OMR shards are now pinned early for the census, and they serve that
  decision too.

</deferred>

---

*Phase: 01-contracts-oracles-and-risk-probes*
*Context gathered: 2026-10-11*
