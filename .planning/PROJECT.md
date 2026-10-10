# klein-science

**What:** signed science plugins for Klein: a SymPy-parity Rust CAS on CPUs and GPUs, numerics, and the Lean prover agent.

**Core value:** an LLM writes a mathematical problem as ordinary SymPy code. Klein computes it exactly and quickly through
this plugin, replays it byte-for-byte, and Lean certifies the parts that can be certified.

**Constraints:**
- licence-clean (see CLAUDE.md);
- Symbolica-clean;
- deterministic exact results across all implementations;
- runs on CPUs and GPUs (CUDA, ROCm, Metal);
- ships only as signed `klein.plugin.v1` bundles provisioned into Klein by digest;
- never edits klein-rs.

**Owner:** Arijit Das (operator). Created 2026-10-11, split out of klein-rs (formerly that repo's Phase 04.3).

## Key Decisions

Operator rulings of 2026-10-11, made on the research in `research/SUMMARY.md` ("Operator decisions needed").

| Decision | Rationale | Outcome |
|----------|-----------|---------|
| Identity = SHA-256 of the JCS canonical DAG **node table** (amends rule 4's "canonical tree" wording) | A literal tree is exponential for shared subterms; the node table is the same tree at linear size | — Pending (amend CLAUDE.md rule 4) |
| Pull the algebraic `solve` core forward (with POLY root-finding); the ≥60% TIR gate moves to the end of M2 | `solve` is in 51% of SymPy programs; M1 alone reaches 36.6% | — Pending |
| Per-symbol `is_*` assumptions in M1; `ask`/`refine` stay in M3 | Construction-time evaluation depends on them | — Pending |
| Exact domain matrices (`ks-dmat`) in M2; `Matrix` API and GPU LA budgets stay in M5 | F4, resultants and linsolve need them | — Pending |
| `lambdify` moves to M4 with numerics | 0.3% of TIR; a GPU/numerics differentiator | — Pending |
| Default `simplify` = ported SymPy pipeline; bounded egglog is opt-in strategy data | Preserves SymPy string parity | — Pending |
| Widen klein-rs dependency #4 (Monty) to the full protocol set from an M0 census; CPython `host_shim` numbers are labelled proxy until it lands | Monty dispatches no arithmetic/comparison dunders and has no inheritance | — Pending |
| `fuel_exhausted` comes only from a logical step budget; Wasmtime fuel is a host error | Wasmtime fuel cannot match across R0/R1/R2 | — Pending |
| Request klein-rs move Wasmtime to ≥ 49.0.1 (deterministic relaxed-SIMD, NaN canonicalisation) | 46.0.1 has RUSTSEC advisories incl. a fuel bypass | — Pending |
| `science.numerics`: same repo, separate signed bundle; `evalf` in `science.cas` | Resolves SS §3 vs §8.6 | — Pending |
| Golden bytes in an artifact store by digest; git holds recipes, manifests, scores | Rule 6: goldens embed corpus content | — Pending |
| Performance budgets ratified per fixed box (Mac: P-cores vs one P-core, Metal vs that) | "16 cores" is unmeasurable on the 14-core M3 Max | — Pending |
| egglog's `im-rc` (MPL-2.0) question deferred to M3 | Decide when simplify work starts; `egg` 0.11 is the fallback | — Pending |
| Roadmap in horizontal layers (M0–M6); v1 = M0–M5, M6 is v2 | Upper CAS layers depend on lower ones | — Pending |

---
*Last updated: 2026-10-11 after research and requirements*
