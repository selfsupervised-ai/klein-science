# Feature Research

**Domain:** a SymPy-parity Rust CAS that runs LLM-written SymPy programs (a pure-Python `sympy` stand-in in the Monty
guest), with numerics (an mpmath port, quadrature, ODE) and a Lean prover plugin that checks CAS certificates
**Researched:** 2026-10-11
**Confidence:** HIGH for corpus frequencies (measured, recipe below); MEDIUM for Lean certificate practicality and Monty
limits (official docs, cross-checked); MEDIUM for milestone coverage (a static upper bound, not execution)

Single source: `research/SCIENCE-PLUGIN-CAS.md`. Where this file disagrees with it, the disagreement is flagged in
"Disagreements with the single source" and not silently applied.

---

## Measured Basis (re-measured 2026-10-11)

The single source says that in a 2,000-row NuminaMath-TIR sample 63% of programs use SymPy, 122 distinct names appear,
the top-50 names cover 92% and the top-80 cover 97%. I re-measured this on the **full corpus** and on a second, newer
corpus, using a static `ast` census. Programs were parsed and never executed, and SymPy itself was not run: star imports
were resolved from `sympy/__init__.py` `__all__`, which was parsed as text.

**Recipe (fetch plus digest; no corpus bytes in git):**
- NuminaMath-TIR: `huggingface.co/datasets/AI-MO/NuminaMath-TIR`, revision `77a91d7b7a1a98ac4b1beb7d86c09d156b935dcd`,
  `data/train-00000-of-00001.parquet`, SHA-256 `66163f9fdf2220fed2116142781fe7019a117191e7f08a3339a70a98401061d8`,
  72,441 rows. Code is the `` ```python `` blocks in `solution`. The unit is a row (all of its blocks). The sample is
  `random.seed(20261011)` with 2,000 rows.
- OpenMathReasoning TIR (DeepSeek-R1 era; the AIMO-2 winning data): `nvidia/OpenMathReasoning` split `tir` (1,718,466
  rows). 2,000 rows were taken as 20 pages of 100 contiguous rows at seeded offsets through the datasets-server `rows` API.
  Code is in `<tool_call>` blocks. The rows are **not digest-pinned yet**: pin the parquet shards before using this as a
  conformance corpus.
- "Name" means a SymPy top-level or submodule name that is imported or used as `sp.X`. Methods such as `.subs` are counted
  separately. "Coverage at top-K" means the share of SymPy programs whose whole name set is within the K most frequent
  names.

| Metric | Single source (2k sample) | Re-measured, 2k sample | Re-measured, full 72,441 | OpenMathReasoning TIR, 2k |
|---|---|---|---|---|
| Programs using SymPy | 63% | 64.0% | 64.0% (46,380) | **19.3%** |
| Distinct names | 122 | 142 | 424 | 100 |
| Top-50 coverage | 92% | 90.6% | 91.2% | 89.1% |
| Top-80 coverage | 97% | 95.4% | 95.8% | 96.1% |
| Top-100 coverage | — | 97.1% | 97.0% | 100% |

The numbers agree within method noise. My count includes imported-but-unused names and hallucinated names, which raises
the distinct-name count. **The long tail grows with corpus size (424 names), but coverage is stable: about 91% at top-50
and about 97% at top-100.**

**Other measured facts that drive features** (NuminaMath-TIR, share of the 46,380 SymPy programs unless stated):

| Fact | Value | Feature consequence |
|---|---|---|
| Import `sympy` but use **no** SymPy name | 10.0% | `import sympy` must be cheap and never fail; Monty runs the rest |
| Call `print(` | 99.1% | The **printed `str()` form is the interface the LLM reads**, so printer parity is table stakes |
| At least one output shows a traceback/error | 11.7% | **Error parity** (exception class plus message): the LLM self-repairs from it |
| At least one output shows a timeout | 2.6% | Map fuel exhaustion to a Python-visible exception |
| Print a float with ≥10 digits | 18.6% | `evalf`/`N` formatting parity at 15 significant digits (mpmath `to_str`) |
| Use `solve(...)[0]` | 12.9% | The shape of `solve`'s return value (list / list of dicts / dict / list of tuples) must match |
| Use `symbols(..., integer/positive/real=True)` | 8.0% | Symbol assumptions are needed as soon as `solve` is; they filter solutions |
| Use `.is_integer` / `.is_real` / … checks | 4.1% | Assumption queries on results |
| Loops / `if` / user `def` / list comprehensions | 19.8% / 17.6% / 13.8% / 12.1% | Ordinary Python must work: Monty's job, not the CAS's |
| `<` `>` `<=` `>=` comparisons in code | 18.2% | Comparison dunders that return Relationals, plus `__bool__` |
| f-strings or `.format` | 7.3% | `__str__`/`__format__` on expressions |
| Co-imports with SymPy: itertools / math / numpy / scipy | 1.9 / 1.4 / 1.1 / 0.5% | Mixed-library programs are rare in NuminaMath |
| Geometry (`Point`, `Line`, `Circle`, …) | 0.90% | Long tail; not needed for the M5 97% gate |
| `sympy.stats` | 0.18% | Long tail |

**OpenMathReasoning (R1-era, AIME-level) differs sharply.** Of programs with code, 33.8% import `math`, 23.8% `numpy`,
19.3% `sympy` and 13.9% `scipy`. Among its SymPy programs, `integrate` is 14.5% (vs 1.4%) and `simplify` 8.0% (vs 0.4%).
The scipy names are `integrate.quad` (118 of 2,000 rows), `optimize.fsolve` (73) and `optimize.minimize` (32); the numpy
names are `roots`, `linalg.solve` and `linspace`. Programs also rely on Jupyter-style display of the last expression,
not `print`. **Newer reasoning models use SymPy less and numerics more.** This affects scope; see the flags at the end.

---

## SymPy Name to Milestone Map (top 60 of 424, NuminaMath-TIR full corpus)

The percentage is the share of SymPy programs that use the name. The milestone is where the single source's §5 scope
puts the name. `err` means a name that does not exist in SymPy (an LLM hallucination): it is "covered" from M0 by raising
the same `ImportError`/`AttributeError` that SymPy raises.

| # | Name | % | M | # | Name | % | M | # | Name | % | M |
|---|---|---|---|---|---|---|---|---|---|---|---|
| 1 | `symbols` | 70.0 | M1 | 21 | `rad` | 1.2 | M1 | 41 | `re` | 0.6 | M1 |
| 2 | `solve` | 51.3 | **M3** | 22 | `oo` | 1.1 | M1 | 42 | `factorint` | 0.6 | M2 |
| 3 | `Eq` | 27.3 | M1 | 23 | `isprime` | 1.1 | M2 | 43 | `binom` | 0.5 | err |
| 4 | `sqrt` | 15.2 | M1 | 24 | `exp` | 1.0 | M1 | 44 | `deg` | 0.5 | M1 |
| 5 | `diff` | 8.4 | M1 | 25 | `tan` | 1.0 | M1 | 45 | `Max` | 0.4 | M1 |
| 6 | `Rational` | 5.8 | M1 | 26 | `floor` | 0.9 | M1 | 46 | `simplify` | 0.4 | M3 |
| 7 | `pi` | 5.5 | M1 | 27 | `N` | 0.9 | M1 | 47 | `nsolve` | 0.4 | M3 |
| 8 | `sin` | 5.0 | M1 | 28 | `collect` | 0.9 | M3 | 48 | `solve_univariate_inequality` | 0.4 | M3 |
| 9 | `cos` | 4.6 | M1 | 29 | `Piecewise` | 0.9 | M3 | 49 | `divisors` | 0.3 | M2 |
| 10 | `Abs` | 4.0 | M1 | 30 | `gcd` | 0.9 | M2 | 50 | `root` | 0.3 | M1 |
| 11 | `binomial` | 3.6 | M1 | 31 | `S` | 0.8 | M1 | 51 | `summation` | 0.3 | M4 |
| 12 | `log` | 3.3 | M1 | 32 | `ln` | 0.8 | M1 | 52 | `Min` | 0.3 | M1 |
| 13 | `Symbol` | 3.0 | M1 | 33 | `primerange` | 0.8 | M2 | 53 | `Line` | 0.3 | M6 |
| 14 | `expand` | 3.0 | M1 | 34 | `Interval` | 0.7 | M3 | 54 | `lcm` | 0.3 | M2 |
| 15 | `Matrix` | 2.7 | M5 | 35 | `Point` | 0.7 | M6 | 55 | `atan` | 0.3 | M1 |
| 16 | `I` | 1.6 | M1 | 36 | `And` | 0.7 | M3 | 56 | `cbrt` | 0.3 | M1 |
| 17 | `factorial` | 1.5 | M1 | 37 | `im` | 0.7 | M1 | 57 | `Mod` | 0.3 | M1 |
| 18 | `integrate` | 1.4 | M4 | 38 | `Poly` | 0.7 | M2 | 58 | `Sum` | 0.3 | M4 |
| 19 | `Function` | 1.4 | M1 | 39 | `acos` | 0.7 | M1 | 59 | `conjugate` | 0.3 | M1 |
| 20 | `limit` | 1.2 | M4 | 40 | `ceiling` | 0.6 | M1 | 60 | `primefactors` | 0.2 | M2 |

Ranks 61–100: `Integer`, `asin`, `Gt`, `Implies`, `factor`, `Pow`, `FiniteSet`, `div`, `Or`, `Lt`, `Ge`, `satisfiable`,
`E`, `Le`, `mod_inverse`, `nsimplify`, `discriminant`, `dsolve`, `Not`, `Point3D`, `Intersection`, `atan2`, `degree`,
`cot`, `is_square`, `Permutation`, `sympify`, `linsolve`, `Add`, `cdf`, `multiset_permutations`, `minimize` (err),
`rsolve`, `stirling`, `Normal`, `Binomial`, `density`, `PermutationGroup`, `sympy.abc`. Each is ≤0.2%.

**Top methods** (share of SymPy programs; these are attribute calls, so `append` and the like are Python's own):
`.subs` 26.5%, `.evalf` 7.9%, `.is_real` 2.0%, `.is_integer` 2.0%, `.coeff` 1.8%, `.dot` 1.6%, `.simplify` 1.3%,
`.rhs` 0.6%, `.norm` 0.5%, `.intersection` 0.5%, `.cross` 0.4%, `.distance` 0.4%, `.lhs` 0.3%, `.det` 0.3%,
`.all_coeffs` 0.3%, `.as_coefficients_dict` 0.3%, `.doit` 0.3%, `.expand` 0.2%, `.coeff_monomial` 0.2%, `.args`,
`.coeffs`, `.as_numer_denom`, `.as_ordered_terms`, `.degree`, `.limit_denominator`, `.as_coeff_exponent`, `.has`,
`.rewrite`, `.removeO`, `.as_poly`, `.eigenvals`, `.inv`, `.T`, `.rank` (each ≤0.1%).

**Cumulative coverage by milestone** (the share of SymPy programs whose names all belong to milestones ≤ Mk). This is an
**upper bound**: it checks names, not whether a given `solve`/`integrate` call falls within the algorithm that milestone
ships.

| After | SymPy programs | All code rows (non-SymPy rows counted as passing) | Single-source gate |
|---|---|---|---|
| M0 | 10.1% | 42.4% | baseline |
| M1 | **36.6%** | **59.4%** | ≥60% of TIR |
| M2 | 41.1% | 62.3% | — |
| M3 | 91.9% | 94.8% | ≥90% of TIR |
| M4 | 95.5% | 97.1% | — |
| M5 | 98.7% | 99.2% | ≥97% of TIR |
| M1 + algebraic solve core | **75.5%** | **84.3%** | (proposed) |

`solve` is the single gate. 46.2% of SymPy programs need nothing beyond M1 except `solve`. 38.9% need only M1 plus
`solve` with no trig, exp, log, `Abs`, `floor`, `I` or `Function`, which is the purely algebraic case. `Eq` appears
together with `solve` in 95.1% of the programs that use `Eq`.

---

## Suggested REQ-ID Prefixes

| Prefix | Category | Plugin |
|---|---|---|
| `GUEST` | `sympy` stand-in in Monty: imports, operator protocol, Python interop, error and timeout parity | `science.cas` (guest side) |
| `PRINT` | `str()` printing parity, float formatting, safe parsing (`sympify`/`S("…")`) | `science.cas` |
| `CORE` | expression DAG, numbers, constants, elementary/combinatorial functions, auto-evaluation, relationals, `subs`/`expand`/`diff` | `science.cas` |
| `NUM` | `evalf`/`N`, mpmath port, `nsolve`, quadrature, ODE, `lambdify` | `science.numerics` |
| `POLY` | domains, `Poly`, gcd/lcm, `factor`, division, resultants and discriminants, Gröbner | `science.cas` |
| `NT` | `isprime`, `primerange`, `factorint`, `divisors`, `totient`, `mod_inverse`, `crt` | `science.cas` |
| `SOLVE` | `solve`, `solveset`, `linsolve`, `nonlinsolve`, inequalities, `SolveResult` | `science.cas` |
| `SIMP` | `simplify` (bounded egglog), `collect`, `nsimplify`, `Piecewise`, assumptions | `science.cas` |
| `CALC` | `limit`, `integrate`, `series`, `summation`/`Sum`, `dsolve`, `rsolve`, `minimum`/`maximum` | `science.cas` |
| `MAT` | `Matrix`, `det`/`inv`/`rref`/`eigenvals`, exact GPU linear algebra | `science.cas` |
| `DISC` | sets, logic (`satisfiable`), combinatorics (`Permutation`, `multiset_*`) | `science.cas` |
| `GEOM`, `STATS` | `sympy.geometry`, `sympy.stats` (long tail) | `science.cas` |
| `CERT` | CAS→Lean certificates and their Lean proof templates | `science.cas` + `science.prover` |
| `PROVER` | Lean→CAS calls (`kleinCall "science.cas.*"`), a polyrith backend, the prover harness and skills | `science.prover` |
| `ACCEL` | R1 parallel map and R2 native/GPU implementations, with admission per operation class | `science.cas` |
| `DET` | JCS/SHA-256 identity, byte-identical replay, float tagging, fuel | all |
| `CONF` | oracle pipeline, corpora as recipes, scoring, the adjudication list | repo/CI |

---

## Feature Landscape

### Table Stakes (Users Expect These)

The "user" is an LLM writing ordinary SymPy and reading printed output. Missing any item below makes a measurable share
of programs fail or diverge.

#### GUEST: the stand-in surface

| Feature | Why Expected | Complexity | Notes |
|---|---|---|---|
| GUEST-imports: `import sympy [as sp]`, `from sympy import …`, `from sympy import *`, `from sympy.abc import x`, submodule paths (`sympy.ntheory`, `sympy.geometry`, `sympy.stats`, …) | 100% of SymPy programs; 10% do nothing else | LOW | The import must not trigger a tool call. A missing name must raise SymPy's exact `ImportError`/`AttributeError` text (hallucinated `binom` 0.5%, `minimize`) |
| GUEST-operators: `+ - * / ** -x` with reflected forms against Python `int`/`float`; `< > <= >=` building Relationals; structural `==`; `__hash__` (Symbols as dict keys in solve results); `__bool__` raising SymPy's `TypeError: cannot determine truth value of Relational`; `int()`/`float()`; `__getitem__` (Matrix); `__call__` (`Function('f')(x)`) | Every expression is built this way | **HIGH, blocked externally** | **Monty today does not dispatch `__add__`, `__lt__`, `__ne__`, `__bool__`, `__getitem__` or `__call__`, and has no inheritance** (pydantic.dev Monty limitations). This is ROADMAP dependency #4 on klein-rs; nothing in GUEST works without it |
| GUEST-python-interop: `1/2*x` float trap (matches SymPy: `0.5*x`), `sorted()`/`max()` over SymPy numbers, iteration over `solve` results, `range` with `Integer` (needs `__index__`; Monty documents dispatching it for indexing only, so `range()` is unverified [U]) | 18–20% of programs use loops, comparisons and comprehensions | MEDIUM | Semantics must match CPython+SymPy exactly, including where SymPy itself is surprising |
| GUEST-methods: `.subs` (26.5%), `.evalf` (7.9%), `.is_integer`/`.is_real` (2.0% each), `.coeff`, `.simplify`, `.rhs`/`.lhs`, `.doit`, `.as_coefficients_dict`, `.all_coeffs`, `.free_symbols`, `.args`, `.as_numer_denom` | The second most common usage after names | MEDIUM | `.subs` is so frequent that it should run guest-local or as a cheap call; per-call latency matters more than throughput here |
| GUEST-errors: exception class and message parity for common failures (`NotImplementedError` from solve/integrate, relational truthiness, `TypeError` on bad args, `ValueError`) | 11.7% of programs show a traceback, and the LLM's repair loop reads it | MEDIUM | Score error parity separately in CONF |
| GUEST-fuel: fuel exhaustion surfaces as a deterministic Python exception, and `SolveResult.status = fuel_exhausted` | 2.6% of programs show a timeout | LOW | Deterministic fuel replaces wall-clock timeouts, which is also a differentiator |

#### PRINT: what the LLM reads

| Feature | Why Expected | Complexity | Notes |
|---|---|---|---|
| PRINT-str: byte parity with SymPy's default `str()`/`repr` printer: term and factor ordering, `sqrt(2)/2`, `-1/2 + sqrt(3)*I/2`, `x**2`, `oo`, `zoo`, `nan`, containers (`{x: 1}`, `[(1, 2)]`, `{n: 4, M: 5}`) | 99.1% of programs print; outputs show `p/q` 8.6%, `**` 7.5%, `sqrt(` 6.7% | **HIGH** | Term ordering (SymPy's monomial `sort_key` in the printer) is the hardest part. Port it from SymPy (BSD-3). It does not affect the canonical DAG order, which is our own (DET) |
| PRINT-float: `Float` and `evalf`/`N` formatting at 15 significant digits by default and `n` digits on request, with mpmath rounding | 18.6% of programs print ≥10-digit floats | MEDIUM | Needs the mpmath `to_str` port in M1, ahead of the rest of `science.numerics` |
| PRINT-parse: safe `sympify`/`S("1/3")`/`parse_expr`, with **no `eval`** | <0.1% of TIR, but needed for tool I/O, the Lean round trip and the adjudication list | MEDIUM | A grammar parser only; SymPy's `eval`-based path is an anti-feature |

#### CORE (M1)

| Feature | Why Expected | Complexity | Notes |
|---|---|---|---|
| CORE-symbols: `symbols`/`Symbol` with assumptions (`integer`, `positive`, `real`, `nonnegative`, …), `Function`, `Dummy`, `Wild` | `symbols` 70%; assumption keywords 8% | MEDIUM | Old-style assumptions on symbols are needed in M1 because `solve` filters on them |
| CORE-numbers: `Integer`, `Rational`, `Float` (arbitrary precision), `S`, `I`, `pi`, `E`, `oo`, `zoo`, `nan`; exact Z/Q arithmetic | `Rational` 5.8%, `pi` 5.5%, `I` 1.6% | MEDIUM | `dashu` for Z/Q; a small-integer fast path |
| CORE-functions: `sqrt` (15.2%), `sin`/`cos`/`tan` and inverses, `atan2`, `log`/`ln`, `exp`, `Abs` (4.0%), `floor`/`ceiling`, `Max`/`Min`, `re`/`im`/`conjugate`/`arg`/`sign`, `Mod`, `root`/`cbrt`, `rad`/`deg`, `binomial` (3.6%), `factorial` (1.5%) | Most of the top 60 | **HIGH** | **Automatic evaluation** is the real work: `sqrt(8)→2*sqrt(2)`, `sin(pi/6)→1/2`, `binomial(10,3)→120`. Port SymPy's `eval` classmethods rule by rule, with property tests |
| CORE-relational: `Eq` (27.3%), `Ne`, `Gt`/`Lt`/`Ge`/`Le`, `.lhs`/`.rhs` | `Eq` is the third most common name | LOW | Mostly an argument to `solve` |
| CORE-ops: `subs`, `expand` (3.0%), `diff` (8.4%), `Derivative`, `Lambda`, `Pow`/`Add`/`Mul` constructors, `prod` | Core manipulation | MEDIUM | `expand` needs the sparse polynomial fast path (Monagan–Pearce) for performance, not correctness |
| CORE-evalf: `N`/`evalf` to arbitrary precision for the elementary functions and constants | `N` 0.9%, `.evalf` 7.9% | MEDIUM | Correct rounding means mpmath's `mpf` semantics; port from mpmath (BSD) |

#### SOLVE / SIMP (M3)

| Feature | Why Expected | Complexity | Notes |
|---|---|---|---|
| SOLVE-algebraic: `solve` for linear systems and polynomial equations/systems with radicals (degree ≤4), rational-function equations, and symbol-assumption filtering | `solve` 51.3%; 38.9% of SymPy programs need only M1 plus algebraic solve | HIGH | **Return-shape parity is critical:** single univariate → list; system → dict (linear) or list of tuples/dicts; `dict=True` → list of dicts. 12.9% index `solve(...)[0]` |
| SOLVE-transcendental: trig/exp/log equations via SymPy's `_tsolve`/`_invert` heuristics; `Abs`/`floor` piecewise cases | the remaining ~7% of solve-only programs | HIGH | Return `SolveResult{complete:false}` honestly when the heuristic path is used |
| SOLVE-others: `nsolve` (0.4%), `solve_univariate_inequality` (0.4%), `reduce_rational_inequalities`, `linsolve`, `nonlinsolve`, `solveset` | Each ≤0.4% | MEDIUM | `nsolve` needs the mpmath `findroot` port (NUM) |
| SIMP-simplify: `simplify` (0.4% as a name, 1.3% as a method; 8% in OpenMathReasoning), `collect` (0.9%), `nsimplify`, `together`, `apart`, `trigsimp` | Normalising answers | HIGH | Bounded egglog; the strategy is bundle data. Output must equal SymPy's in the common cases, so start from SymPy's `simplify` heuristic order, not an e-graph optimum |
| SIMP-assumptions: `.is_integer`/`.is_real`/`.is_positive` on results; `ask`/`Q` (rare) | 4.1% | MEDIUM | Three-valued (True/False/None), matching SymPy |
| SIMP-piecewise-sets: `Piecewise` (0.9%), `Interval` (0.7%), `And`/`Or`/`Not`, `FiniteSet`, `Union`/`Intersection` as results of solveset and inequalities | ~2% combined | MEDIUM | Sets beyond these move to DISC (M5) |

#### POLY / NT (M2)

| Feature | Why Expected | Complexity | Notes |
|---|---|---|---|
| NT-primes: `isprime` (1.1%), `primerange` (0.8%), `factorint` (0.6%), `divisors`, `primefactors`, `totient`, `mod_inverse`, `nextprime`/`prime`, `crt`/`solve_congruence`, `divisor_count`, `is_square` | ~4% combined; olympiad staples | MEDIUM | `isprime` must be deterministic (BPSW plus Miller–Rabin bases, as in SymPy). `factorint` output dict ordering follows SymPy |
| POLY-basic: `gcd`/`lcm` (0.9/0.3%), `Poly` (0.7%), `factor` (0.2%), `div`, `discriminant`, `degree`, `.all_coeffs`/`.coeff` | Common, cheap | MEDIUM | Domains ZZ/QQ/GF(p)/algebraic come first; factoring over ZZ uses Zassenhaus first and van Hoeij later |
| POLY-advanced: Gröbner (Buchberger then F4), resultants, multivariate factoring | Rare in TIR (<0.1%) but the base for `solve` on polynomial systems and for CERT ideal membership | HIGH | Track cofactors from the start: CERT needs them |

#### CALC (M4), MAT/DISC (M5)

| Feature | Why Expected | Complexity | Notes |
|---|---|---|---|
| CALC-integrate: table → `manualintegrate` → Risch | 1.4% (14.5% in OpenMathReasoning) | HIGH | Definite integrals are heuristic-only (single source §6) |
| CALC-limit: Gruntz | 1.2% | HIGH | |
| CALC-sums: `summation`/`Sum` (0.6% combined), Gosper | | MEDIUM | Gosper is the prerequisite for WZ |
| CALC-other: `series`/`.removeO`, `dsolve` (0.1%), `rsolve` (0.1%), `minimum`/`maximum` | | MEDIUM–HIGH | |
| MAT-exact: `Matrix` (2.7%), `.det`/`.inv`/`.rref`/`.eigenvals`/`.dot`/`.cross`/`.norm`/`.T` | 2.7% plus methods | MEDIUM | Fraction-free exact arithmetic; GPU only for large instances |
| DISC: `satisfiable`, `Implies`, `Permutation`, `multiset_permutations`, `subsets`, `stirling` | <0.5% combined | MEDIUM | |

### Differentiators (Competitive Advantage)

These follow from the core value: compute exactly, replay byte-for-byte, and have Lean certify what can be certified.

| Feature | Value Proposition | Complexity | Notes |
|---|---|---|---|
| DET-replay: expression identity = SHA-256 over JCS; exact results byte-identical across R0/R1/R2; floats tagged `approximate` and replayed from the record | SymPy output depends on hash seeds and caches; ours is reproducible and auditable | MEDIUM | Already binding (CLAUDE.md rule 4). Canonical order is ours; the SymPy print order is only a printer concern |
| SOLVE-result-honesty: `SolveResult{complete, certifiable, status}` | SymPy cannot say whether `solve` found **all** solutions; LLMs then box incomplete answers | MEDIUM | `complete=true` only when the algorithm is a decision procedure (linear, univariate polynomial with full factorisation, zero-dimensional Gröbner) |
| CERT-* (see below): CAS answers plus Lean-checkable certificates | No mainstream CAS ships kernel-checked certificates for routine answers | MEDIUM–HIGH | `science.prover` turns them into sealed statements plus proof templates; `lean.certify` decides |
| PROVER-polyrith: restore Mathlib's `polyrith` with Klein's CAS as backend (Gröbner cofactors → `linear_combination (exp := n)`) | Mathlib's `polyrith` "no longer works" since the SageCell server shut down; `grobner` (via `grind`) closes goals but gives no `linear_combination` suggestion | MEDIUM | A clear, wanted gap. It needs POLY-advanced with cofactor tracking. It is offline and deterministic, unlike the old web call |
| GUEST-fuel-determinism: fuel-bounded, deterministic budgets instead of wall-clock timeouts | 2.6% of TIR programs time out in SymPy; ours fail identically on every replay | LOW | |
| ACCEL-gpu-exact: dense modular linear algebra, NTT, CRT and F4 on GPU (Metal first), admitted per operation class only after R0/R1/R2 are byte-identical | Exact speed-ups that stay deterministic | HIGH | **Honest scope:** TIR programs are tiny, and GPU does not matter for TIR pass rates. It matters for large determinants, big polynomial products, Gröbner and `lambdify` grids. Per-call latency matters more for TIR |
| NUM-batch: `lambdify` grids and batch `evalf`, quadrature and ODE ensembles on GPU (f32 on Metal; f64 on CPU) | Numerics at scale | MEDIUM–HIGH | `lambdify` is 0.3% of TIR programs, so it is a differentiator, not table stakes |
| CONF-adjudication: a public list of cases where SymPy is wrong, with our corrected answer and evidence | Beats SymPy where SymPy has bugs, without silently diverging | LOW (process) | Each entry needs an oracle (Giac/FLINT offline, or a Lean proof) |
| SIMP-strategy-as-data: simplification rule sets and their order are bundle data that autoresearch can improve | Improve answers without code changes | MEDIUM | Must keep SymPy-equal output on the conformance corpus, so changes are gated by CONF |
| Speed: ≥20× faster than SymPy, native ≤2× SymEngine | SymPy's slowness causes TIR timeouts | HIGH | Budgets are [U] in the single source and must be measured on the fixed box (rule 5) |

### Anti-Features (Commonly Requested, Often Problematic)

Do not build these early, or at all.

| Feature | Why Requested | Why Problematic | Alternative |
|---|---|---|---|
| `eval`-based `sympify`/`parse_expr` (SymPy's default) | Parity | Executes arbitrary code inside the guest; it is a sandbox hole | A grammar-based parser producing the same trees (PRINT-parse) |
| User subclassing of SymPy classes (`class f(Function): @classmethod def eval…`) | Advanced SymPy idiom | Monty has no inheritance; 0.0% of TIR programs define a class | Reject with a clear error. Revisit only if Monty gains inheritance |
| Plotting (`plot`, `plot_implicit`, matplotlib) | Appears in a handful of programs | No display; non-deterministic; 0.1% | Raise SymPy-identical `ImportError`/no-op with a message |
| `pprint`, unicode pretty printing, `init_printing`, LaTeX printer early | Familiar from notebooks | Large printers, ~0% of TIR; `str()` is what is read | `str()` parity first; LaTeX in M6 |
| Reproducing SymPy's hash-seed-dependent orders (set printing order, `solve` order for some systems) | "Byte parity" | It is non-deterministic even in CPython SymPy; it fights DET | Canonical order, with each such case recorded in the adjudication list |
| Unbounded `simplify`/e-graph saturation | "Find the simplest form" | AC rules blow up; runtime is unpredictable | Bounded egglog with fuel; SymPy's heuristic order as the baseline |
| Full Risch before table/`manualintegrate` | Completeness | Huge, and TIR integrals are elementary textbook forms | Table → manualintegrate first (M4), Risch after |
| Runtime choice of CPU/GPU backend | "Use the GPU when available" | Breaks replay; the single source seals the implementation set in the lock | Lock-sealed set; admission per operation class |
| Bit-identical float results across GPU/CPU | Consistency | Impossible across drivers; Metal has no f64 | Tag `approximate`, record backend/device/driver, replay the recorded value |
| A broad numpy/scipy emulation | OpenMathReasoning programs use numpy 24% and scipy 14% | A huge surface, mostly float and non-deterministic | Defer. If adopted, a **narrow** facade (`scipy.integrate.quad`, `scipy.optimize.fsolve`, `numpy.roots`) over `science.numerics`. Operator decision |
| `sympy.physics`, `tensor`, `vector`, `codegen`, `plotting`, `parsing.latex` before M6 | Parity checklist | ~0% of TIR | M6 long tail, module by module |
| `native_decide` in certificate proofs | Fast kernel checks | Trusts the compiler, outside the Lean kernel TCB | `decide +kernel`, `norm_num`, `ring`, `linear_combination` (PrimeCert and `sos` both avoid `native_decide`) |
| Certifying limits, definite integrals, branch-cut simplification or float `evalf` | "Certify everything" | The single source §6 marks these heuristic-only; there are no practical certificates | `certifiable=false`, honestly |

---

## Lean Certificates: What Is Practical First (CERT)

Ranked by (TIR frequency × ease of checking). Every Lean side uses kernel-checked tactics only.

| Rank | Certificate | CAS produces | Lean checks with | Needs | Complexity | When |
|---|---|---|---|---|---|---|
| 1 | **Solution substitution** ("these values satisfy the equations") | the `solve` result | `norm_num` / `ring_nf` / `simp` after `subst`; radicals via `Real.sq_sqrt`/`Real.sqrt_eq_iff` | SOLVE | LOW | M3 (first certificate; it covers the 51% `solve` share) |
| 2 | **Polynomial identity / factorisation product** (`expand`, `factor`, `together`, `apart`) | both sides | `ring` / `field_simp; ring` (with nonzero-denominator side goals) | CORE, POLY | LOW | M2 |
| 3 | **Numeric facts**: gcd/lcm values, small primality, `factorint` products, divisor lists | the value | `norm_num` (Mathlib has `Nat.Prime`, gcd and related extensions); products by `norm_num` | NT | LOW | M2 |
| 4 | **Large primality: Pratt or Pocklington** | factorisation of p−1 (full for Pratt; a partial factor > √N for Pocklington) and witnesses | Mathlib `lucas_primality` plus `reduce_mod_char`/`norm_num` for the modular powers; or the PrimeCert (MIT) Pocklington format | NT `factorint`, modular powering | MEDIUM | M2. Needed above ~25 bits: Mathlib's `norm_num` `Nat.Prime` proofs become deep and can overflow the kernel stack. Prefer **Pocklington** (needs only partial factorisation) and fall back to Pratt |
| 5 | **Solve completeness, univariate polynomial** | factorisation `p = c·∏(x−rᵢ)·q` with `q` having no roots in the domain | `ring` for the product; `mul_eq_zero`; `q` has no roots by discriminant/`nlinarith` or degree ≤2 | POLY factor | MEDIUM | M3. This is what lets `SolveResult.certifiable` mean "all solutions" |
| 6 | **Ideal membership**: `g = Σ cᵢ·hᵢ`, or radical `gⁿ = Σ cᵢ·hᵢ` | cofactors cᵢ (and n) | `linear_combination (exp := n) Σ cᵢ * hᵢ` | POLY Gröbner **with cofactor tracking** | MEDIUM | M2–M3. The same machinery is the polyrith backend (PROVER) |
| 7 | **Linear algebra**: inverse, solution of Ax=b, rank witness | B with AB=I; x; pivot minors | `decide +kernel`/`norm_num [Matrix.mul_apply, Fin.sum_univ_succ]` | MAT | MEDIUM | M5 |
| 8 | **Antiderivative check** | F with F′ = f | `deriv`/`HasDerivAt` lemmas plus `field_simp; ring`, with domain side conditions | CALC | MEDIUM–HIGH | M4. Indefinite integrals only |
| 9 | **SOS for `nlinarith`/positivity** | exact rational SOS `p = σ₀ + Σ σᵢ gᵢ` | either hints (`nlinarith [sq_nonneg q₁, …]`) or an exact identity by `ring` plus `positivity`; the `leanprover/sos` package checks the same form with `decide +kernel` | an SDP solver plus exact rounding (LDLᵀ) | **HIGH** | M4–M5. An SDP solver is not on the approved building-block list; Clarabel.rs (Apache-2.0) supports SDP. That is an operator decision. `sos` itself uses CSDP (EPL-1.0) through FFI, so do not link it |
| 10 | **WZ pairs** | rational certificate R(n,k) | rational-function identity by `field_simp; ring`, plus a telescoping lemma and boundary/denominator side goals | Gosper and Zeilberger (M4) | **HIGH** | M4 at the earliest; research-grade. There is no WZ library in Mathlib; WZ-LLM (ICML 2026) reaches 34% on 100 classic identities |

**Recommendation.** Ship ranks 1–3 first: almost free, and they cover the high-frequency TIR operations. Add 4–6 in
M2–M3 (Pocklington primality, solve completeness, ideal membership plus polyrith), because they reuse NT/POLY machinery
that has to exist anyway. Defer SOS and WZ: each needs a large new component (an SDP solver; Zeilberger) plus Lean-side
glue that does not exist in Mathlib today.

---

## Feature Dependencies

```
[Monty operator protocol: dunders, __bool__, __hash__, __call__, __getitem__]   (klein-rs dependency #4)
    └──required by──> GUEST-operators ──> every CORE/SOLVE/... feature reachable from LLM code

[CORE-numbers (dashu Z/Q) + hash-consed DAG + DET identity]
    ├──> CORE-functions (auto-evaluation) ──> PRINT-str (needs canonical forms to print)
    ├──> CORE-evalf ──requires──> NUM mpmath port (mpf, to_str) ──> PRINT-float
    └──> POLY-basic (domains, Poly, gcd)
              ├──> NT-primes ──> CERT-4 Pocklington/Pratt (needs factorint of p−1)
              ├──> POLY factor ──> CERT-2 identity, CERT-5 solve completeness
              └──> POLY-advanced Gröbner (with cofactors) ──> CERT-6 linear_combination ──> PROVER-polyrith
                                                        └──> SOLVE polynomial systems

[CORE-symbols assumptions] ──> SOLVE-algebraic ──> CERT-1 substitution
SOLVE-algebraic ──requires──> POLY-basic (roots of univariate polynomials, radicals for degree ≤4)
SOLVE-transcendental ──requires──> SIMP (inversion, Piecewise) 
SOLVE nsolve ──requires──> NUM findroot
CALC-integrate ──requires──> POLY (Hermite/Rothstein–Trager) + SIMP; ──> CERT-8
CALC-sums (Gosper) ──> CERT-10 WZ (also needs Zeilberger)
MAT-exact ──> ACCEL-gpu-exact (modular dense LA, CRT) ──> CERT-7
CERT-9 SOS ──requires──> SDP solver (not yet approved) + exact rounding + POLY

[CONF oracle pipeline: CPython+SymPy in a container, frozen goldens by digest] ──gates──> every milestone's "passes when"
[ACCEL admission] ──requires──> R0 reference + witness seen RED for each operation class

PRINT-str ──conflicts──> DET canonical order   (resolved: the printer re-sorts for display only; identity never uses print order)
Reproducing SymPy hash-seed orders ──conflicts──> DET
Runtime backend selection ──conflicts──> lock-sealed implementation set
```

### Dependency Notes

- **Everything in GUEST requires Monty operator dispatch.** As documented today, Monty dispatches only `__init__`,
  `__repr__`/`__str__`, `__eq__`/`__hash__`, `__enter__`/`__exit__`, `__iter__`/`__next__`, `__contains__` and
  `__index__` (for indexing). It has no inheritance, and instances are always truthy. Monty's "host classes", which
  suspend calls to the host, might be the route, but arithmetic dispatch on them is not documented. This is the M0 spike's
  first question, and it is on klein-rs.
- **`solve` requires POLY root-finding, not just M3 heuristics.** The algebraic solve core (linear systems, univariate
  polynomials through degree 4 with radicals, rational roots and factor-based splitting) depends on POLY-basic factoring.
  That is why moving solve earlier also pulls part of M2 earlier.
- **PRINT-float requires the mpmath port early.** `evalf` output is 18.6% of printed output, so the mpf formatter cannot
  wait for the `science.numerics` milestone.
- **CERT-6 and PROVER-polyrith require cofactor tracking in Gröbner from day one.** Retrofitting cofactors into an F4
  implementation is expensive. Design the Buchberger/F4 interfaces to return a representation matrix optionally.
- **CERT-4 requires `factorint`.** Pocklington needs a factored part of N−1 greater than √N; Pratt needs the full
  recursive factorisation.
- **Printer vs canonical order.** SymPy's printed term order is a printer property. Our DAG order is a canonical,
  hash-independent order (DET). They do not conflict as long as the printer sorts for display only.

---

## MVP Definition

### Launch With (v1): what M0–M3 must deliver to validate the concept

- [ ] GUEST-imports, GUEST-operators (on Monty with operator dispatch), GUEST-errors, GUEST-fuel: nothing runs without them
- [ ] PRINT-str and PRINT-float: the LLM reads printed output in 99% of programs
- [ ] CORE (M1 scope as in the single source), including symbol assumptions
- [ ] **SOLVE-algebraic** (linear plus polynomial with radicals, return-shape parity, assumption filtering): the single
      largest name after `symbols`. It takes M1-scope coverage from 36.6% to ~75.5% of SymPy programs
- [ ] NT-primes and POLY-basic (gcd, factor, Poly): cheap, frequent in olympiad problems, and the base for CERT
- [ ] SIMP-simplify (bounded), `collect`, `Piecewise`, `Interval`, `And`/`Or`: completes the M3 ≥90% gate
- [ ] CERT 1–3 (substitution, `ring` identities, `norm_num` facts) with `SolveResult.certifiable`
- [ ] CONF: the oracle pipeline, both corpora as recipes plus digests, and scoring at three levels (stdout byte-equal,
      final answer equal, error-class equal)
- [ ] DET: JCS identity and R0 replay

### Add After Validation (v1.x)

- [ ] CALC: `integrate` (table → manualintegrate), `limit` (Gruntz), `summation`/Gosper, `series`. Trigger: M3 gates
      pass, and the OpenMathReasoning-style corpus shows `integrate` at 14.5%
- [ ] CERT 4–6 (Pocklington/Pratt, solve completeness, `linear_combination`) and PROVER-polyrith. Trigger: Lean loop
      (M4 gate)
- [ ] MAT-exact, DISC, CERT-7. Trigger: the M5 ≥97% gate
- [ ] ACCEL R1/R2 for the admitted operation classes. Trigger: measured budgets on the Mac's Metal GPU
- [ ] NUM `nsolve`, quad, ODE, `lambdify` grids

### Future Consideration (v2+)

- [ ] CERT-9 SOS and CERT-10 WZ: each needs a new large component and Lean glue
- [ ] GEOM (0.90%), STATS (0.18%), physics/tensor/codegen: the M6 long tail
- [ ] A narrow `scipy`/`numpy` facade over `science.numerics`, if the operator decides the R1-era corpus is in scope
- [ ] Full Risch; Meijer G; PDEs

---

## Feature Prioritization Matrix

| Feature | User Value | Implementation Cost | Priority |
|---|---|---|---|
| GUEST-operators (Monty dispatch) | HIGH | HIGH (external) | P1 |
| GUEST-imports / errors / fuel | HIGH | LOW–MEDIUM | P1 |
| PRINT-str | HIGH | HIGH | P1 |
| PRINT-float + mpmath formatter | HIGH | MEDIUM | P1 |
| CORE-numbers / symbols / relational / ops | HIGH | MEDIUM | P1 |
| CORE-functions auto-evaluation | HIGH | HIGH | P1 |
| SOLVE-algebraic (+ return-shape parity) | HIGH | HIGH | P1 |
| NT-primes, POLY-basic | MEDIUM | MEDIUM | P1 |
| SIMP-simplify (bounded), collect, Piecewise, assumptions | MEDIUM | HIGH | P1 |
| CERT 1–3 | HIGH (core value) | LOW | P1 |
| CONF pipeline + DET identity | HIGH | MEDIUM | P1 |
| SOLVE-transcendental, inequalities, nsolve | MEDIUM | HIGH | P2 |
| CALC integrate / limit / sums / series | MEDIUM (HIGH for R1-era corpora) | HIGH | P2 |
| CERT 4–6, PROVER-polyrith | HIGH (core value) | MEDIUM | P2 |
| MAT-exact, DISC | LOW–MEDIUM | MEDIUM | P2 |
| ACCEL R1/R2 GPU exact | MEDIUM (low for TIR) | HIGH | P2 |
| NUM quad/ODE/lambdify grids | MEDIUM | MEDIUM–HIGH | P2 |
| CERT-9 SOS, CERT-10 WZ | MEDIUM | HIGH | P3 |
| GEOM, STATS, physics, codegen | LOW | HIGH (breadth) | P3 |
| numpy/scipy facade | MEDIUM (R1-era) | HIGH | P3 (operator decision) |

---

## Competitor Feature Analysis

Only permissive or offline-oracle systems are analysed (the Symbolica-clean rule excludes Symbolica entirely).

| Feature | SymPy (reference, BSD-3) | SymEngine (MIT; offline oracle) | Mathlib tactics | Our approach |
|---|---|---|---|---|
| Surface | The full Python API (the parity target) | A fast C++ core, a smaller API, `symengine.py` | n/a | A pure-Python stand-in in Monty, with heavy ops as typed tool calls |
| Printing | `str` printer with `sort_key` term order | Its own printer, different order | n/a | Byte parity with SymPy's `str`; canonical DAG order kept separate |
| Determinism | Hash-seed and cache dependent in places | Mostly deterministic | Kernel-checked | JCS SHA-256 identity; byte-identical R0/R1/R2 |
| Solve completeness | Unknown to the caller | Limited solve | n/a | `SolveResult{complete, certifiable, status}` |
| Timeouts | Wall-clock in harnesses (2.6% of TIR) | — | `maxHeartbeats` | Deterministic fuel |
| Polynomial certificates | None emitted | None | `polyrith` dead (Sage server gone); `grobner` closes goals without a certificate; `linear_combination` checks certificates | Gröbner cofactors → `linear_combination (exp := n)`; a polyrith backend |
| Primality certificates | None | None | `norm_num` up to ~25 bits; `lucas_primality`; PrimeCert (Pocklington, MIT) outside Mathlib | Pocklington/Pratt certificates from `factorint` |
| SOS | None | None | `nlinarith` takes hints; the `leanprover/sos` package (CSDP via FFI) | Later: exact rational SOS, SDP solver to be decided |
| GPU | None | None | n/a | CubeCL (Metal first) for exact modular/NTT/CRT, admitted per operation class |

---

## Disagreements with the Single Source (flagged, not applied)

1. **The M1 gate "≥60% of TIR" does not fit M1's scope if TIR means SymPy programs.** With `solve` in M3, M1 covers
   36.6% of SymPy programs. It reaches 59.4% of all code rows only if programs that never use SymPy count as passing, and
   that also presupposes Monty runs them. **Options:**
   - (a) **Recommended.** Move an algebraic solve core (linear, univariate polynomial with radicals, symbol-assumption
     filtering, return-shape parity) and the POLY root-finding it needs into M1/M2. That gives about 75.5% of SymPy
     programs (84.3% of rows) as an upper bound.
   - (b) Restate the M1 gate as "≥60% of all TIR rows".
   - (c) Keep it as is and accept that the M1 gate fails.
2. **The headline numbers re-measure slightly lower.** Re-measured: 64.0% SymPy share; top-50 91.2% and top-80 95.8% on
   the full corpus (90.6% and 95.4% on a 2k sample); 97% needs about the top 100. The difference is method (I count
   imported-but-unused and hallucinated names). Recommend pinning the census script and the corpus digest in CONF so the
   number is reproducible.
3. **Monty's current limits are a harder blocker than "M0 spike" suggests.**
   - No arithmetic or comparison dunder dispatch, no `__bool__`/`__call__`/`__getitem__`, no inheritance.
   - `fractions` and `decimal` are not importable (0.4% of NuminaMath rows and 1.7% of OpenMathReasoning rows use
     `fractions`).
   - These have to be resolved in klein-rs (dependency #4) before any GUEST feature can be measured.
4. **The core-value premise is weaker for R1-era corpora.** In OpenMathReasoning TIR only 19.3% of programs use SymPy;
   `math`, `numpy` and `scipy` dominate. The single source's guest surface is `sympy` only. Whether a narrow
   numpy/scipy facade over `science.numerics` is in scope is an operator decision.
5. **`lambdify` is in the M1 scope but appears in 0.3% of TIR programs.** It is a GPU differentiator (NUM-batch), not a
   TIR table stake. It could move to M4/M5 with `science.numerics` without affecting any gate.
6. **SOS certificates need an SDP solver that is not on the approved building-block list.** Candidates: Clarabel.rs
   (Apache-2.0, supports SDP). CSDP (EPL-1.0, used by `leanprover/sos`) should not be linked. This is an operator decision
   before CERT-9.

---

## Sources

- Corpus census (measured here; HIGH):
  - AI-MO/NuminaMath-TIR, rev `77a91d7b…`, parquet SHA-256 `66163f9f…` — https://huggingface.co/datasets/AI-MO/NuminaMath-TIR
  - nvidia/OpenMathReasoning `tir` split, 2,000-row seeded sample — https://huggingface.co/datasets/nvidia/OpenMathReasoning
  - SymPy `__init__.py` `__all__` (parsed as text) — https://github.com/sympy/sympy/blob/master/sympy/__init__.py
- Monty limits (MEDIUM; official docs, two pages cross-checked):
  - https://pydantic.dev/docs/monty/limitations/classes/
  - https://pydantic.dev/docs/monty/limitations/modules/
- Mathlib `polyrith` docstring (SageCell shut down; `grobner` alternative) (MEDIUM) —
  https://florisvandoorn.com/LeanCourse25/docs/Mathlib/Tactic/Polyrith.html
- Mathlib `norm_num` `Nat.Prime` extension (~25-bit depth limit) (MEDIUM) —
  https://florisvandoorn.com/LeanCourse25/docs/Mathlib/Tactic/NormNum/Prime.html
- PrimeCert (Pocklington, MIT, no `native_decide`) (MEDIUM) — https://reservoir.lean-lang.org/@b-mehta/PrimeCert
- Lucas certificates in practice (arXiv 2608.08643) (MEDIUM) — https://arxiv.org/pdf/2608.08643
- `leanprover/sos` (Harrison SOS, CSDP, `decide +kernel`) (MEDIUM) — https://reservoir.lean-lang.org/@leanprover/sos
- NSPI, LLM + SOS → Lean (arXiv 2605.15445) (LOW; abstract only) — https://arxiv.org/abs/2605.15445
- Automated Tactics for Polynomial Reasoning in Lean 4 (arXiv 2604.13514) (LOW; abstract only) —
  https://arxiv.org/abs/2604.13514
- WZ-LLM (arXiv 2605.04472, ICML 2026; 34% on LCI-Test) (MEDIUM) — https://arxiv.org/abs/2605.04472
- Linear combination / radical membership in polyrith (MEDIUM) — https://github.com/leanprover-community/mathlib4/commit/a051ac5f
- Lean `grind` commutative-ring solver (MEDIUM) —
  https://lean-lang.org/doc/reference/latest/The--grind--tactic/Algebraic-Solver-_LPAR_Commutative-Rings___-Fields_RPAR_/
- Clarabel (Apache-2.0, SDP support) (MEDIUM) — https://pypi.org/project/clarabel/0.8.1
- SymPy printer / `solve` return shapes / Float formatting: SymPy behaviour from training knowledge (MEDIUM); to be pinned
  by the CONF golden-output pipeline, not asserted

---
*Feature research for: SymPy-parity Rust CAS for LLM-written math programs, with numerics and Lean certificates*
*Researched: 2026-10-11*
