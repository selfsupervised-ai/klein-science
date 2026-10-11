# klein-science — rules for agents

This is a separate project from klein-rs (Klein's core). It ships **signed Klein plugins** (`klein.plugin.v1`). It never
adds code to klein-rs. Anything it needs from Klein's core is filed as a request against klein-rs (see ROADMAP
"Dependencies on Klein core").

## Binding rules

1. **Symbolica-clean.** No agent, lane, reviewer or tool reads Symbolica's source or internal docs, runs it, benchmarks
   against it or compares output with it. Its source-available licence forbids using it as a reference implementation,
   behavioural oracle, blueprint or benchmark for a competing CAS, including by clean-room teams and AI systems.
2. **Licences.**
   - May be ported, with notices kept in `NOTICE`: SymPy, mpmath, Diofant and FriCAS (BSD-3), and SymEngine (MIT).
   - Permissive building blocks: `dashu`, `num-*`, `astro-float`, `egglog`/`egg`, `faer`, `nalgebra`, rayon, CubeCL.
   - Never linked or shipped: GPL/LGPL code (FLINT, GMP/`rug`, `malachite`, Maxima, Giac, Singular, PARI, Sage).
3. **Offline oracles only.** CPython + SymPy, SymEngine and Giac/FLINT binaries run only in a container at build or test
   time. Their outputs are frozen and pinned by digest, and nothing ships them.
4. **Determinism.**
   - Exact results (Z, Q, Z/p) are byte-identical across the WASM, parallel-CPU and native/GPU implementations. That is
     shown by conformance with a witness seen RED before an operation class is admitted.
   - Float results are tagged approximate and replayed from the record.
   - Expression identity is SHA-256 over the RFC-8785 JCS encoding of the canonical DAG node table: the canonical tree
     serialised at linear size, with shared subterms stored once.
5. **Claims need oracles.** A test is a witness only after it has failed against the unfixed code. Performance claims are
   measured on a fixed box, never estimated.
6. **No benchmark or third-party corpus bytes in git.** Corpora are fetch recipes plus digests.
