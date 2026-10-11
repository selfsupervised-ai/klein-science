# Phase 1: Contracts, Oracles and Risk Probes - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-10-11
**Phase:** 01-contracts-oracles-and-risk-probes
**Areas discussed:** Naming + rule 4 text, CI/artifact store/keys, Oracle slice + census, Filing klein-rs requests

Mid-discussion, the operator added: "I also want you to follow the DRY SOLID and no bloat principles". This is recorded
as D-01.

---

## Naming + rule 4 text

| Option | Description | Selected |
|--------|-------------|----------|
| ks- | Short; already used in the research docs | |
| klein-science- | Self-explanatory, verbose | |
| ksci- | Short, more recognisable next to klein-rs crates | ✓ |

| Option | Description | Selected |
|--------|-------------|----------|
| First Phase 1 task | The plan amends rule 4 | |
| Amend it now | Edit CLAUDE.md before planning | ✓ |
| I'll word it myself | The operator writes the amendment | |

| Option | Description | Selected |
|--------|-------------|----------|
| You decide | Follow the ARCHITECTURE.md tree; unshipped tools clearly separated | ✓ |
| Separate workspace | Probes and oracle tooling in their own workspace | |

---

## CI, artifact store, keys

| Option | Description | Selected |
|--------|-------------|----------|
| GH Actions + Mac | Hosted Linux CI; the Mac for Metal | |
| Klein VM + Mac | Self-hosted runner on the klein VM; the Mac for Metal | ✓ |
| Mac only | Local `xtask ci` | |

| Option | Description | Selected |
|--------|-------------|----------|
| Private GHCR OCI | `oras`, addressed by sha256 | ✓ |
| Local content store | A directory on the Mac or VM | |
| S3 / object storage | A private bucket | |

| Option | Description | Selected |
|--------|-------------|----------|
| Dev key, labelled | A throwaway key until D3 | |
| Operator release key now | The production key from day one | ✓ |

| Option | Description | Selected |
|--------|-------------|----------|
| Mac signs | The VM builds, the Mac checks digests and signs from the Keychain | ✓ |
| VM runner holds key | Automated signing on the shared box | |
| Hardware key | Manual signing with a YubiKey | |

| Option | Description | Selected |
|--------|-------------|----------|
| Mac + Linux x86_64 | aarch64-apple-darwin, x86_64-linux-gnu, wasm32-wasip2 | ✓ |
| Mac only | | |
| Mac + both Linux arches | | |

---

## Oracle slice + census

| Option | Description | Selected |
|--------|-------------|----------|
| ~1k, stratified | Deterministic, digest-seeded, stratified by module | ✓ |
| Full corpus now | | |
| ~200 smoke slice | | |

| Option | Description | Selected |
|--------|-------------|----------|
| 4 seeds | 0, 1, 2 and a large one, twice each, plus an arm64/amd64 check | ✓ |
| 2 seeds | | |
| 8 seeds | | |

| Option | Description | Selected |
|--------|-------------|----------|
| Dynamic + static | Instrumented dunders plus an AST scan; also measures the meijerint share | ✓ |
| Static AST only | | |
| Dynamic only | | |

| Option | Description | Selected |
|--------|-------------|----------|
| TIR only | OMR pinned later | |
| Pin OMR now, include it | | ✓ |

| Option | Description | Selected |
|--------|-------------|----------|
| Seeded sample by digest | A fixed set of shards or ~10k programs | ✓ |
| Whole dataset | | |

---

## Filing klein-rs requests

| Option | Description | Selected |
|--------|-------------|----------|
| Draft here, you file | Drafts in `.planning/requests/`; the operator approves and files a GitHub issue | ✓ |
| Docs only | | |
| Issues directly | | |

| Option | Description | Selected |
|--------|-------------|----------|
| Measured + acceptance test | Evidence, the exact ask, and a test that is RED today | ✓ |
| Evidence + ask only | | |

| Option | Description | Selected |
|--------|-------------|----------|
| Deny-default + ablation | Each rule has a RED witness | ✓ |
| Start from Chromium GPU | | |

---

## Claude's Discretion

- The repo layout for unshipped probe, oracle and census tooling.
- The trivial operation for the R0/R2 compare, the stand-in skeleton's extent, and the fixed-box manifest fields.

## Deferred Ideas

- Full-corpus TIR goldens: Phase 2.
