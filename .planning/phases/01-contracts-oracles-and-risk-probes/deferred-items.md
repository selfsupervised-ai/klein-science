## Deferred Items

- `check provenance` compiles the R0 crates for wasm32-wasip2, while D-16 builds R0 for wasm32-unknown-unknown
  status: open
  **Found by:** plan 01-06 (Task 1), out of its scope.
  **What:** `xtask/src/depinfo.rs` `R0_TARGET = "wasm32-wasip2"` and the `[workspace.metadata.ksci] r0-crates`
  comment still name wasip2. The shipped component is now built by `cargo xtask release dist` for
  wasm32-unknown-unknown with `+simd128`. A cfg that differs between the two targets would be judged on the wrong
  compile. Plan 01-09 (dist gates, D-16 wording) is the natural owner.
- `cargo xtask release dist` passes no `--remap-path-prefix`, so the component embeds host paths
  status: open
  **Found by:** plan 01-06 (Task 1), out of its scope.
  **What:** research Pitfall I. `strings target/dist/science_cas.component.wasm` shows 8 `/Users/…` paths
  (`$CARGO_HOME/registry/src/…` panic locations). Two dist builds in the same checkout gave the same component
  digest (`700d1968…`), but a build with another `$CARGO_HOME` or checkout path would not. Plan 01-09's
  `dist --twice` and reproducibility work owns the remap flags.
