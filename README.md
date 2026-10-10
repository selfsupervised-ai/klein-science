# klein-science

Science plugins for [Klein](https://github.com/selfsupervised-ai/klein-rs).

- **`science.cas`**: a licence-clean Rust computer-algebra system that works toward SymPy parity and runs on CPUs and GPUs.
- **`science.numerics`**: arbitrary-precision numerics, quadrature and ODE solvers.
- **`science.prover`**: the Lean prover agent (a `.harness` plus skills).

LLMs write ordinary SymPy-style code. Klein runs the heavy symbolic and numeric work through this plugin as typed,
recorded tool calls. Lean checks the certificates the CAS produces.

Status: planning. See [.planning/ROADMAP.md](.planning/ROADMAP.md) and
[.planning/research/SCIENCE-PLUGIN-CAS.md](.planning/research/SCIENCE-PLUGIN-CAS.md).

## Licence

Apache-2.0. Code ported from SymPy, mpmath, Diofant or FriCAS (BSD-3) and from SymEngine (MIT) keeps its notices in
`NOTICE`.

**Symbolica-clean:** this project never uses Symbolica's source, binaries, documentation internals or outputs. Do not open
pull requests based on them.
