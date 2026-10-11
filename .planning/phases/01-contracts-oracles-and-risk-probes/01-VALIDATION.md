---
phase: "01"
slug: "contracts-oracles-and-risk-probes"
# status lifecycle: draft (seeded by plan-phase) → validated (set by validate-phase §6)
# audit-milestone §5.5 distinguishes NOT-VALIDATED (draft) from PARTIAL (validated + nyquist_compliant: false) (#2117)
status: draft
nyquist_compliant: false
wave_0_complete: false
created: "2026-10-11"
---

# Phase 01 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.
> Seeded from `01-RESEARCH.md` § Validation Architecture.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust: `cargo-nextest 0.9.148` + `proptest` (fixed `PROPTEST_RNG_SEED`); Python: stdlib `unittest` (no pytest in the images) |
| **Config file** | none yet — Wave 0 creates `.config/nextest.toml` (CI profile) |
| **Quick run command** | `cargo nextest run -p <touched crate>` (e.g. `cargo nextest run -p ksci-wire -p ksci-ops`) |
| **Full suite command** | `cargo xtask ci` (fmt, clippy -D warnings, nextest, deny, provenance, dist, gates, conform; excludes oracle and probe) |
| **Estimated runtime** | quick ~30 s; full ~10 min (oracle golden run and Metal probe are separate slow jobs) |

---

## Sampling Rate

- **After every task commit:** `cargo nextest run -p <touched crate>` + `cargo clippy -p <crate> -- -D warnings`
- **After every plan wave:** `cargo xtask ci`
- **Before `/gsd-verify-work`:** full suite green; every guard's RED witness recorded (commit + CI log excerpt); oracle reproducibility report; probe report with box manifest; three request drafts approved
- **Max feedback latency:** 60 seconds for the quick command

---

## Per-Task Verification Map

Task IDs are filled in once PLAN.md files exist; the requirement → command map below is the contract.

| Requirement | Behavior | Test Type | Automated Command | File Exists | Status |
|-------------|----------|-----------|-------------------|-------------|--------|
| DET-01 | Two construction orders → same identity; RFC 8785 vectors; Rust == Python mirror == `rfc8785` | unit + property + differential | `cargo nextest run -p ksci-wire identity`; `python -m unittest standin.tests.test_wire` | ❌ W0 | ⬜ pending |
| DET-02 | JSON number outside indices/`v` rejected; numeral grammar; 300-digit Int round trip | unit | `cargo nextest run -p ksci-wire numerals` | ❌ W0 | ⬜ pending |
| DET-03 | Provenance change leaves response digest unchanged | unit | `cargo nextest run -p ksci-wire envelope` | ❌ W0 | ⬜ pending |
| DET-04 | Small budget → `fuel_exhausted` bytes R0 == R2; Wasmtime fuel/memory → host error (both pins) | integration | `cargo nextest run -p ksci-conform fuel` | ❌ W0 | ⬜ pending |
| DET-08 | clippy bans, NodeId has no `Ord`, deny `getrandom`; each seen RED once | lint + compile | `cargo clippy --workspace --all-targets -- -D warnings`; `cargo deny check` | ❌ W0 | ⬜ pending |
| CONF-01 | Same golden bytes on 2 runs × 4 hash seeds; arm64 == amd64 | container | `cargo xtask oracle golden --verify` (slow; VM) | ❌ W0 | ⬜ pending |
| CONF-02 | Only recipes/manifests/scores in git; fetch re-hashes | CI check | `cargo xtask check-no-corpus` | ❌ W0 | ⬜ pending |
| CONF-03 | Three levels scored; ImportError path normalised | unit + run | `python -m unittest conformance.runner.tests` | ❌ W0 | ⬜ pending |
| CONF-06 | Box manifest schema; refuses under load or on battery | unit | `cargo nextest run -p xtask box_manifest` | ❌ W0 | ⬜ pending |
| LIC-01 | `rug` → `cargo deny` RED | CI witness | `cargo deny check licenses bans` | ❌ W0 | ⬜ pending |
| LIC-02 | Missing provenance header → RED; NOTICE entries present | unit | `cargo xtask provenance` | ❌ W0 | ⬜ pending |
| DEP-01..03 | Request drafts contain evidence, ask, and a RED acceptance test | manual review | `checkpoint:human-verify` | n/a | ⬜ pending |
| PLUG-01 | Two clean builds same digests; `verify_strict` pass; byte-flip RED | integration | `cargo xtask dist --twice && cargo xtask verify` | ❌ W0 | ⬜ pending |
| PLUG-02 | `canonicalize` R0(46.0.1) == R0(49.0.x) == R2; perturbation RED | integration | `cargo nextest run -p ksci-conform r0_r2` | ❌ W0 | ⬜ pending |
| GUEST-01 | 5 import forms, no tool call; missing name → ImportError (normalised) | unit (CPython) | `python -m unittest standin.tests.test_imports` | ❌ W0 | ⬜ pending |
| GUEST-02 | Monty-subset lint passes; fixture `class A(B)` RED; Monty parser agrees | lint | `python -m standin.lint standin/sympy` | ❌ W0 | ⬜ pending |
| ACCEL-01 | Kernels exact vs CPU before timing; box manifest attached | manual-run (Mac) | `cargo xtask probe metal` | ❌ W0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] Workspace skeleton: `Cargo.toml`, `rust-toolchain.toml`, `clippy.toml`, `deny.toml`, `.config/nextest.toml`
- [ ] `crates/ksci-wire/tests/` (identity, numerals, envelope, rfc8785-by-recipe)
- [ ] `standin/tests/` (unittest) and `conformance/runner/tests/`
- [ ] `xtask` subcommands: `ci`, `provenance`, `dist`, `verify`, `box-manifest`, `check-no-corpus`
- [ ] Tool installs: wasm-tools 1.261.0, cargo-deny 0.20.2, nextest 0.9.148, oras 1.3.4, uv 0.13.0

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| klein-rs request drafts (D4 census, D1 Seatbelt rules, D6 Wasmtime floor) | DEP-01, DEP-02, DEP-03 | Filing against klein-rs needs operator approval | Operator reviews each draft for evidence, ask and a RED acceptance test, then approves filing |
| Metal watchdog limit and throughput | ACCEL-01 | Needs the Mac on AC, idle, operator present (watchdog trials can hang the GPU) | Run `cargo xtask probe metal` on the fixed box; confirm the report carries the box manifest |
| Per-box budget ratification | CONF-06 | Operator decision | Operator signs off the budget table |
| Release signing | PLUG-01 | Key lives only in the Mac Keychain | Operator runs `cargo xtask sign`; CI verifies with `verify_strict` |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 60s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
