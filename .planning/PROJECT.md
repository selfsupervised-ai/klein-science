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
