## Deferred Items

- `check provenance` compiles the R0 crates for wasm32-wasip2, while D-16 builds R0 for wasm32-unknown-unknown
  status: resolved
  **Resolved by:** plan 01-09 (commit b26741a). `depinfo::R0_TARGET` is now `wasm32-unknown-unknown` and
  `depinfo::R0_RUSTFLAGS` (`+simd128`) is the one definition both `release dist` and `check provenance` use;
  `compile_inputs` passes the flags as `CARGO_ENCODED_RUSTFLAGS`.
  **Found by:** plan 01-06 (Task 1), out of its scope.
  **What:** `xtask/src/depinfo.rs` `R0_TARGET = "wasm32-wasip2"` and the `[workspace.metadata.ksci] r0-crates`
  comment still name wasip2. The shipped component is now built by `cargo xtask release dist` for
  wasm32-unknown-unknown with `+simd128`. A cfg that differs between the two targets would be judged on the wrong
  compile. Plan 01-09 (dist gates, D-16 wording) is the natural owner.
- `cargo xtask release dist` passes no `--remap-path-prefix`, so the component embeds host paths
  status: resolved
  **Resolved by:** plan 01-09 (commits b26741a, 6e8f06b, 198953a). Every release build remaps the tree to `/src`
  and `$CARGO_HOME` to `/cargo`; `dist --twice` builds from two paths, compares digests and refuses any
  artifact holding `$HOME`, either tree or `$CARGO_HOME`.
  **Found by:** plan 01-06 (Task 1), out of its scope.
  **What:** research Pitfall I. `strings target/dist/science_cas.component.wasm` shows 8 `/Users/…` paths
  (`$CARGO_HOME/registry/src/…` panic locations). Two dist builds in the same checkout gave the same component
  digest (`700d1968…`), but a build with another `$CARGO_HOME` or checkout path would not. Plan 01-09's
  `dist --twice` and reproducibility work owns the remap flags.
- `cargo xtask ci` runs no host-side Python suite, so the stand-in lint and tests are not CI-gated yet
  status: open
  **Found by:** plan 01-12 (Task 2), out of its scope (its files do not include `xtask/`).
  **What:** threat T-01-32's mitigation says the Monty-subset lint "runs in `cargo xtask ci` through the conformance
  and standin suites", and plan 01-17 assumes "the host-side `unittest discover` in xtask ci". `xtask/src/ci.rs`
  `STEPS` holds only cargo commands, and no phase-01 plan adds a Python step. The fix is one `cargo xtask` command
  that runs `.venv/bin/python -m unittest discover` for `standin/` and `conformance/` plus
  `PYTHONPATH=standin python -m lint standin/sympy` (with `PYTHONDONTWRITEBYTECODE=1`), and a CI step for it. It
  needs the dev venv (`cargo xtask tools venv`) and, for `standin/tests/test_wire.py`, a release `ksci-worker`.
