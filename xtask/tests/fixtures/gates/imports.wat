;; provenance: original
;; A permanent RED fixture for the zero-import gate (`cargo xtask release gates`, D-16): the
;; smallest component with one import, the function `f: func()`, and nothing else. The gate must
;; reject it and name the import.
(component
  (import "f" (func))
)
