;; provenance: original
;; A permanent RED fixture for the relaxed-SIMD gate (`wasm-tools validate
;; --features=-relaxed-simd`): one function using `f32x4.relaxed_madd`, whose results are
;; implementation-defined and would break replay. The gate must reject it.
(module
  (func (export "madd") (param v128 v128 v128) (result v128)
    local.get 0
    local.get 1
    local.get 2
    f32x4.relaxed_madd))
