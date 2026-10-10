# Pitfalls Research

**Domain:** a SymPy-parity computer algebra system in Rust. Exact results must be byte-identical across WASM (R0), parallel CPU (R1) and native CPU/GPU (R2, CubeCL on Metal first). The guest surface is a pure-Python `sympy` stand-in running in Monty. Oracles are offline (SymPy in a container, SymEngine, Giac/FLINT binaries). Lean certifies results.
**Researched:** 2026-10-11
**Confidence:** MEDIUM overall. The CAS-algorithm and determinism pitfalls are HIGH: they come from well-established literature, verified docs and the SymPy issue tracker. The GPU/Metal specifics are MEDIUM to LOW (thin public docs, needs the M0 probe). The Monty dunder gap is HIGH (official Pydantic docs, fetched today).

Single source: `.planning/research/SCIENCE-PLUGIN-CAS.md` ("SS"). Milestones M0–M6 are the ones in SS §5. Where this file
**disagrees with or refines** SS, the item is marked **[SS-FLAG]**. Those items are collected again at the end so the
operator can rule on them. SS has not been edited.

Symbolica-clean: no Symbolica material was searched for, read or cited while writing this file.

---

## Critical Pitfalls

### Pitfall 1: Monty does not dispatch the operator dunders the stand-in depends on  **[SS-FLAG]**

**What goes wrong:**
The stand-in's core idea is that `x**2 + 2*x` builds an expression locally. That needs `__add__`, `__radd__`, `__mul__`,
`__pow__`, `__neg__`, `__truediv__` and `__lt__` (which builds a `Relational`) on user-defined classes. Monty's official
limitations page says that `__add__`, `__ne__`, `__bool__`, "etc. are not dispatched for user-defined instances". A class
that defines `__lt__` still raises. The dunders that *are* dispatched are only `__init__`, `__repr__`, `__str__`,
`__enter__`, `__exit__`, `__iter__`, `__next__`, `__contains__`, `__eq__`, `__hash__` and `__index__`. Monty also rejects
base classes (`class Integer(Rational)` fails at parse time), and it rejects method decorators (`@property`,
`@classmethod`).

**Why it happens:**
SS M0 lists "Monty operator overloading" as a spike item, and ROADMAP dependency #4 asks klein-rs for it. The full list of
protocols that LLM-written SymPy code actually touches is much longer than arithmetic, and nobody has enumerated it.

**How to avoid:**
- In M0, before any stand-in code, measure the TIR corpus for the Python protocols it uses on SymPy objects. Turn that
  into one precise request to klein-rs. Likely minimum:
  - arithmetic and reflected forms: `__add__`/`__radd__`, `__sub__`, `__mul__`, `__truediv__`, `__floordiv__`, `__mod__`,
    `__pow__`/`__rpow__`;
  - unary: `__neg__`, `__pos__`, `__abs__`;
  - rich comparisons: `__lt__` and the rest;
  - conversions: `__bool__`, `__float__`, `__int__`, `__complex__`, `__round__`, and `__format__` (TIR programs use
    `f"{val:.4f}"` constantly);
  - containers and calls: `__getitem__`/`__len__` (matrices, solution dicts), and `__call__` (`Function('f')(x)`,
    `Lambda`, and the callables `lambdify` returns).
- Model the type hierarchy (`isinstance(r, Rational)`, `Integer` being a `Number` being an `Expr`) without inheritance.
  Use a single `Expr` class with a kind tag, and either a Monty-side `isinstance` hook or a stand-in `isinstance` shim.
  This has to be decided in M0.
- Confirm in M0 that Monty's numeric semantics match CPython on the inputs the stand-in sees:
  - bigints;
  - the shortest-repr float, which `Float(0.1)` depends on;
  - `1/2` giving float vs `Rational(1, 2)`;
  - `2**100`.

**Warning signs:**
- The stand-in prototype only ever builds expressions through `Add(x, y)` calls rather than operators.
- TIR programs fail with `TypeError` on `float(sol)` or f-strings.
- `sorted(solutions)` raises.

**Phase to address:** M0 (spike; it blocks M1). The klein-rs request must go in before M1 is planned. Confidence: HIGH.

---

### Pitfall 2: Canonicalisation that is "smarter" (or different) than SymPy's automatic evaluation

**What goes wrong:**
A Rust CAS designed from first principles normalises eagerly: it expands, cancels rational functions and sorts by an
internal key. SymPy's automatic evaluation is deliberately light and idiosyncratic:
- `Add.flatten` collects like terms with numeric coefficients.
- `Mul.flatten` merges powers and distributes a lone Number over an Add (`2*(x+1)` becomes `2*x + 2`).
- `(x**2-1)/(x-1)` is *not* cancelled.
- `sqrt(8)` becomes `2*sqrt(2)`.
- `exp(log(x))` becomes `x`, but `log(exp(x))` stays unless `x` is real.

Every divergence changes printed output. It also changes which later rewrites fire. The printed output *is* the semantics
an LLM program observes.

**Why it happens:**
Developers port algorithms (gcd, factor) but treat the `core` constructors (`Add.flatten`, `Mul.flatten`, `Pow.__new__`,
`Number` arithmetic, `Function.eval`) as "obvious". SymEngine also differs from SymPy in automatic-evaluation details, so
copying SymEngine's core semantics (MIT, permitted) does not give SymPy parity.

**How to avoid:**
- Port SymPy's `core` evaluation rules (BSD-3, NOTICE kept) as the specification for construction-time evaluation, line
  by line. Do not reinvent them.
- Keep three things separate:
  1. *identity* (the SHA-256 over the JCS tree, Pitfall 4);
  2. *internal canonical order*, used for hash-consing and arg order;
  3. *SymPy print order* (`sort_key`/`default_sort_key`, `as_ordered_terms`, and `StrPrinter._print_Add`'s ordering).
- Use SymEngine only as a *math-equivalence and speed* oracle, never as a printed-form oracle.

**Warning signs:**
- Golden diffs where both sides are mathematically equal but the term order or grouping differs.
- `expand` is called implicitly anywhere in constructors.
- Tests compare `str()` against a hand-written expectation instead of real SymPy output.

**Phase to address:** M1. Confidence: HIGH.

---

### Pitfall 3: Order nondeterminism from hashers, interning ids and parallel splits

**What goes wrong:**
Output differs run to run or R0 vs R2, even for exact results. The usual causes:
- `std::collections::HashMap`/`HashSet` use a per-process random SipHash key. `hashbrown`'s default (foldhash) and
  `ahash::RandomState::new()` are also seeded at runtime. Any iteration over them that reaches output (args order,
  printing, solution lists, Gröbner basis order) is nondeterministic.
- Node ids from *lock-free concurrent interning* (the idea SS §4 keeps from HVM) depend on thread races. Ordering or
  tie-breaking by node id therefore makes the result depend on the schedule.
- Ordering by pointer address depends on ASLR.
- Rayon's `reduce`/`fold` split the work adaptively. `find_any` returns whichever item finishes first.
- Work partitioned by the *number of cores* (N = 14 on the Mac, something else in CI) changes which CRT primes or blocks
  are combined first. Any early-termination heuristic then differs by machine.

**Why it happens:**
Exact arithmetic *is* order-independent, so people assume the output is too. The *representation* is not order-independent
unless every ordering decision is a pure function of content.

**How to avoid:**
- Ban `RandomState`-seeded maps in crates that produce output. Enforce it with `clippy::disallowed_types` for
  `std::collections::HashMap/HashSet` and `ahash::RandomState`. Allow `BTreeMap`, `IndexMap` with insertion order derived
  from content, or a fixed-seed hasher only for lookup-only tables.
- Sort args only by a structural content key (a structural compare, or the content digest). Never sort by interning id.
- Partition work by a fixed chunk size that does not depend on N. Merge in index order (as SS R1 already says).
  Termination decisions read only the merged prefix.
- Use `find_first`, not `find_any`. Float reductions use the fixed-tree reductions SS already mandates.
- Run a CI job that executes the conformance corpus several times with:
  1. different `RAYON_NUM_THREADS`;
  2. a shuffled interning pre-warm;
  3. R0 vs R1 vs R2.

  It asserts byte-identical JCS. Before the job is trusted, it must be seen RED by deliberately inserting a `HashMap`
  iteration (rule 5).

**Warning signs:**
- A flaky golden test that "passes on rerun".
- Output differs between `cargo test` and `cargo test -- --test-threads=1`.
- Any `HashMap` in a function whose return value is printed.

**Phase to address:** M1 (the DAG and interning). The multi-thread-count CI job also lands in M1, before any R1/R2
admission. Confidence: HIGH.

---

### Pitfall 4: JCS (RFC 8785) cannot carry CAS numbers as JSON numbers

**What goes wrong:**
RFC 8785 requires JSON numbers to be IEEE-754 doubles, serialised exactly as ECMAScript `Number.prototype.toString` would.
It RECOMMENDS strings for anything larger or more precise. Several things go wrong if the CAS ignores this:
- A big `Integer`, a `Rational`'s numerator or denominator, or an mpf mantissa emitted as a JSON number is silently
  rounded by a conforming canonicaliser.
- Two different expressions get the same SHA-256 identity.
- `-0.0`, NaN, `oo`, `zoo` and `nan` have no JSON-number form.
- JCS sorts object keys by **UTF-16 code units**. A Rust implementation that sorts by UTF-8 bytes differs for non-BMP
  characters, for example math-alphanumeric symbol names such as `𝑥` used as map keys.

**Why it happens:**
"Use JCS for identity" is decided before the number encoding is designed. Small test integers pass.

**How to avoid:**
- Define `klein.science.expr.v1` so that *every* numeric leaf is a tagged string:
  - integers in decimal;
  - rationals as `p/q` in lowest terms with a positive denominator;
  - mpf as an exact `(sign, mantissa, exponent, precision)` tuple of strings;
  - special values as named atoms.
- Use arrays rather than symbol-keyed objects wherever possible.
- Include the symbol assumptions in the symbol's node (Pitfall 9): `Symbol('x')` and `Symbol('x', positive=True)` are
  different SymPy objects.
- Normalise symbol names consistently. Python NFKC-normalises identifiers, but `Symbol('ℌ')` keeps its string.
- Add property tests:
  - round-trip `2**200`;
  - `Rational(-3, 6)` gives the canonical form;
  - non-BMP key ordering against an independent JCS implementation, used as an oracle.

**Warning signs:**
- Any `serde_json::Number` in the expr schema.
- Identity collisions between a large integer and a nearby one.

**Phase to address:** M0 (the WIT/schema design) and M1 (implementation). Confidence: HIGH (RFC text verified).

---

### Pitfall 5: `fuel_exhausted` and memory/stack limits that differ between R0 and R2  **[SS-FLAG]**

**What goes wrong:**
SS gives `SolveResult` a status of `fuel_exhausted`, and R0 is "fuel-bounded". Wasmtime fuel counts *wasm instructions*:
- R2 native has no wasm instructions.
- R1 instances are each fuelled separately.
- Fuel accounting may change between Wasmtime versions, and Klein's kernel, not klein-science, picks that version.

So the *same* problem can be `exact` on R2 and `fuel_exhausted` on R0. That breaks byte-identity of the result envelope.
The same applies to:
- wasm32's 4 GiB linear-memory cap, against unbounded native memory;
- Wasmtime's default wasm stack limit (about 512 KiB) against native 8 MiB main / 2 MiB rayon stacks. A deeply nested
  expression built by an LLM loop recurses fine natively and traps in R0.

**Why it happens:**
Resource limits are treated as a sandbox concern. They are actually part of the result semantics.

**How to avoid:**
- Introduce a **logical fuel**: algorithm-level work units counted identically in every implementation, for example per
  rewrite, per reduction step, per prime, per e-graph iteration. `fuel_exhausted` is reported only from logical fuel.
- Wasmtime fuel and memory caps stay as a *safety backstop*. Hitting the backstop is a host error, not a typed result,
  and it is excluded from conformance.
- Use explicit-stack (iterative) traversals for all expression walks, so depth limits are a logical parameter.
- Problems whose working set exceeds R0's caps are R2-only. They are recorded and replayed from the record, because R0
  cannot be their replay reference.
- Use fuel-based interruption, never epoch interruption. Wasmtime's docs call epochs non-deterministic.

**Warning signs:**
- Conformance rows that are `exact` on native and trap or exhaust on WASM.
- Recursion in `subs`, `expand`, printing or JCS serialisation.

**Phase to address:** M0 (the schema and the decision; it changes SS's envelope semantics) and M1 (implementation).
Confidence: HIGH for the mechanism, MEDIUM for whether fuel counts drift across Wasmtime versions (not verified, so treat
them as unstable).

---

### Pitfall 6: Expression swell in "textbook" algorithms

**What goes wrong:**
The textbook versions of these algorithms produce intermediates that are exponentially larger than input or output:
- Euclidean PRS gcd over Q;
- Gaussian elimination over Q;
- naive Buchberger over Q;
- partial fractions over the splitting field;
- repeated `simplify` on intermediate results;
- symbolic determinants by cofactor expansion.

The result is timeouts on modest inputs, or memory blowups that hit R0's 4 GiB cap first.

**Why it happens:**
The SymPy code paths that *are* portable sometimes have dense and sparse variants, and a slow fallback is ported first
"to get it working". Also, intermediate swell does not show up on toy tests.

**How to avoid:**
- Use these from the start, as SS's performance levers already list:
  - subresultant PRS as the reference;
  - Brown/Zippel modular GCD;
  - Bareiss fraction-free or multi-modular elimination;
  - modular F4 with rational reconstruction;
  - Hensel lifting.
- Every algorithm gets a *swell* benchmark (coefficient size of the output vs the peak of the intermediates) in addition
  to a speed benchmark.
- Put Fateman, `expand2` and `lwbench`-style workloads into the M1/M2 budget suite early.

**Warning signs:**
- Peak memory grows much faster than output size.
- Gcd of two degree-20 bivariate polynomials takes more than a second.
- A test suite that only uses univariate integer examples.

**Phase to address:** M2 (polys) and M5 (matrices). Confidence: HIGH.

---

### Pitfall 7: Randomised and heuristic algorithms whose output or termination depends on luck

**What goes wrong:**
Several core algorithms are randomised:
- Cantor–Zassenhaus;
- Zippel's sparse interpolation (random evaluation points);
- Pollard rho in `factorint`, which SymPy seeds explicitly;
- Miller–Rabin with random bases;
- Schwartz–Zippel identity tests.

Multi-modular methods also hit *unlucky primes* (bad reduction) and need a stopping rule. If any of these read OS
randomness, or stop on "stabilised over k primes" without verification, the exact output or the
`complete`/`certifiable` flags can differ across runs and implementations. Factor order and normalisation (sign,
monic vs primitive, `factor_list` ordering) also differ from SymPy unless they are ported.

**Why it happens:**
Probabilistic algorithms are correct "with high probability", and nobody budgets for making them reproducible.

**How to avoid:**
- Use one deterministic PRNG, seeded from the input's content digest. Never use OS randomness in the CAS crates; deny
  `getrandom` in CAS crates via `cargo-deny`/feature checks.
- Use a fixed published table of primes, identical on CPU and GPU. If the GPU needs 31-bit primes (Pitfall 13), the CPU
  uses the same table for the conformance path.
- Detect unlucky primes and skip them deterministically, in index order.
- Terminate on a proven bound (Hadamard, Mignotte, Landau–Mignotte) or on an exact verification such as trial
  division or multiplication back. Do not terminate on stabilisation alone.
- Use deterministic BPSW for primality. SymPy's `isprime` uses BPSW above its deterministic Miller–Rabin range.
- Port SymPy's factor and result normalisation and ordering.

**Warning signs:**
- `rand::thread_rng()` anywhere in a CAS crate.
- The prime list is generated at run time by "next prime above random".
- `factor` output order differs from SymPy on golden rows.

**Phase to address:** M2. Confidence: HIGH.

---

### Pitfall 8: AC blowup and nondeterministic budgets in e-graph simplify

**What goes wrong:**
Associativity and commutativity rules make an e-graph grow combinatorially (exponential in the worst case, as the 2026
literature on AC e-graphs restates). Saturation never finishes. Bounding it with a *time* limit makes the result depend on
machine speed. egg's `Runner` defaults are `iter_limit = 30`, `node_limit = 10,000` and **`time_limit = 5 seconds`**, and
its default scheduler is `BackoffScheduler`. If extraction ties on cost, the winner depends on e-class ids, which depend on
rule-application order.

**Why it happens:**
Library defaults are used unchanged, and AC rules are written as ordinary rewrites because "the e-graph handles loops".

**How to avoid:**
- Never put raw `assoc`/`comm` rewrites into the e-graph. Keep Add and Mul as *flattened, canonically sorted n-ary nodes*
  (the canonical form from Pitfall 2). Express AC matching through that canonical form, or through a
  multiset-matching primitive.
- Set budgets only through deterministic limits: nodes, iterations and logical fuel (Pitfall 5). Disable time limits.
  Pin the scheduler and its parameters in the bundle data that SS §3 makes the strategy carrier.
- Make extraction tie-breaking total and content-based, using a structural compare on the extracted term and never the
  e-class id.
- Before admitting egglog in R1/R2, confirm by test that it is deterministic for identical input and identical rule
  order. This is unverified; treat it as an assumption to witness.

**Warning signs:**
- e-graph node counts that hit `node_limit` on small inputs.
- `simplify` output that changes when run on a loaded machine.
- Any `Instant` or `Duration` in the simplify path.

**Phase to address:** M3. Confidence: HIGH for the blowup and the egg defaults (docs.rs verified). Egglog's determinism is
unverified.

---

### Pitfall 9: Assumptions treated as an M3 feature when they decide M1 evaluation  **[SS-FLAG]**

**What goes wrong:**
SS puts "assumptions" in M3. In SymPy, the *old* assumptions system (`symbols('x', positive=True)`, the `is_positive`
three-valued properties, and the fact-deduction rules in `assumptions.py`/`facts.py`) runs at **construction time**:
- `sqrt(x**2)` becomes `x` only when `x` is nonnegative;
- `Abs(x)` becomes `x` when `x` is positive;
- `(x**a)**b` merges only under conditions;
- `log(exp(x))` becomes `x` only when `x` is real.

An M1 core without assumptions prints different results for a large share of TIR programs, which routinely declare
`positive=True` or `real=True`. Symbol identity must include the assumptions. The *new* system (`ask`, `Q`, `refine`,
SAT-based) is separate. It is slower, incomplete, and inconsistent with the old one in places. Building it first is a
classic scope trap.

**Why it happens:**
"Assumptions" sounds like a reasoning feature, not a core-evaluation dependency.

**How to avoid:**
- M1 ports the old assumptions: per-symbol facts, the deduction rules, `is_*` on core classes, and three-valued logic
  where `None` means unknown.
- M3 adds `ask`/`refine`/`Q` and the assumption-aware `simplify`/`solve` paths.
- Memoisation keys (SymPy's `@cacheit` has a history of cache-dependent bugs) must include every input that affects
  evaluation: assumptions, the global evaluation flags (`evaluate=False`) and precision.
- Never cache a `fuel_exhausted` or partial result as if it were final.

**Warning signs:**
- `sqrt(x**2)` prints `sqrt(x**2)` for a positive `x`.
- A memo table keyed only by the node id.
- An `ask` implementation started before `is_positive` works.

**Phase to address:** M1 (old assumptions) and M3 (new assumptions). Confidence: HIGH.

---

### Pitfall 10: "simplify" is ill-defined, and egglog will not reproduce SymPy's printed forms  **[SS-FLAG]**

**What goes wrong:**
SymPy's `simplify` is a heuristic pipeline:
- it tries `powsimp`, `cancel`, `together`, `factor_terms`, `trigsimp`, `radsimp`, `logcombine` and others in a fixed
  order;
- it keeps the shortest result by `measure=count_ops` with `ratio=1.7`.

Its output changes between SymPy releases. A cost-based egglog extractor will often find a *different*, and arguably
simpler, form. Every such row then fails string-equality against golden output, even though it is mathematically right.
SS §3 makes the strategy bundle data so autoresearch can improve it. "Improving" simplify therefore actively *reduces*
string parity.

**Why it happens:**
"Simpler" has no canonical definition. Parity and quality pull in opposite directions.

**How to avoid:**
- Define two strategies, both as bundle data:
  1. **`sympy-compatible`** (the default for the stand-in): a port of SymPy's `simplify` pipeline and its sub-simplifiers
     (BSD-3). Its target is the same printed result.
  2. **`klein-egglog`** (opt-in, and the one autoresearch improves): bounded saturation with an explicit cost model.
- Score `simplify` and `integrate` rows by **semantic equivalence**: a symbolic check, high-precision numeric checks at
  random points with exact zero-testing where possible, and a `count_ops` comparison. String match is a separate,
  secondary metric.
- SS M3 says "simplify (egglog)". This file recommends that egglog *augments* a ported SymPy pipeline rather than replacing
  it.

**Warning signs:**
- TIR parity drops whenever the simplify strategy improves.
- Conformance only reports string match.

**Phase to address:** M3. Confidence: HIGH for the mechanism. The share of affected TIR rows is not measured [U].

---

### Pitfall 11: Branch cuts, principal values, and the gap between CAS and Lean semantics

**What goes wrong:**
Several identities are false off the positive reals:
- `sqrt(x*y) = sqrt(x)*sqrt(y)`;
- `(x**a)**b = x**(a*b)`;
- `log(x*y) = log(x) + log(y)`;
- `atan(y/x)` vs `atan2`.

Rewrites that ignore principal branches give wrong answers silently (`powdenest(force=True)` exists precisely because
these are unsafe). For Lean, Mathlib's total functions add a second semantic gap:
- `Real.sqrt` of a negative is `0`;
- `x / 0 = 0`;
- `Real.log` uses junk values at and below 0 (log of zero is junk).

A CAS identity can then be "certified" in Lean for the wrong reason, or a miniF2F-style statement can collapse to `0 = 0`.
Public miniF2F audits document exactly this faithfulness defect for log-of-zero junk.

**Why it happens:**
Rewrite rules are written over "numbers". The domain (C with principal branches, R, or Lean's total R) is implicit.

**How to avoid:**
- Every rewrite rule in the bundle declares its domain side-conditions (`x >= 0`, `x != 0`, `im(x) in (-pi, pi]`).
  The rule fires only when the assumptions system (Pitfall 9) proves them.
- The Lean statement translator emits explicit hypotheses (`0 ≤ x`, `x ≠ 0`) for every `sqrt`, `log`, division and
  non-integer power. A `certifiable` flag requires a *faithfulness check*: the Lean statement, with junk values excluded
  by its hypotheses, must imply the CAS claim.
- SS §6 already marks "simplification across branch cuts" as heuristic-only, so never set `certifiable` on such results.

**Warning signs:**
- Lean proofs that succeed suspiciously easily with `simp` on division or `sqrt` goals.
- Rewrite rules with no side-condition field.

**Phase to address:** M1 (rule-schema side-conditions), M3 (assumption-gated rewrites) and M4 (the Lean faithfulness
check). Confidence: HIGH for the CAS side. MEDIUM for the Lean specifics: `Real.sqrt` is verified, and `div_zero` and
`Real.log_zero` are well known but not re-verified today.

---

### Pitfall 12: Risch and integration scope creep  **[SS-FLAG partial]**

**What goes wrong:**
"Integrate (table → manualintegrate → Risch)" in M4 quietly becomes "implement Risch". Here is where SymPy stands:
- Its `risch_integrate` covers only the *transcendental* elementary case (exp/log towers).
- Algebraic extensions raise `NotImplementedError`.
- A `NonElementaryIntegral` result is a *proof* that no elementary antiderivative exists, while `NotImplementedError`
  means "unknown".

The algebraic case (Trager/Bronstein) is a multi-year effort. Among open systems only FriCAS (BSD, portable) is
substantially complete, and its SPAD/Boot sources are hard to port.

SymPy's `integrate` is also a *cascade* of methods: polynomial/rational, `heurisch` (Risch–Norman), `manualintegrate`,
`risch`, and `meijerint` for many definite integrals. Different methods give different, equivalent antiderivatives
(they differ by a constant or a rewriting). SS defers Meijer G to M6, but SymPy's *definite* integration often goes through
`meijerint`, so M4 definite-integral parity will be limited until M6.

**Why it happens:**
"Risch is a decision procedure" sounds like the principled choice. Integration is the most visible CAS feature.

**How to avoid:**
- M4 implements exactly SymPy's transcendental Risch, port-for-port, with the same three-way outcome (elementary,
  proven non-elementary, unknown) mapped onto `complete`/`certifiable`.
- The algebraic case is an explicit non-goal until M6+. Any FriCAS port is its own planned phase.
- Score integration by derivative check (`diff(F, x) - f` zero-tests to 0), not by string equality.
- Measure early how many TIR definite integrals SymPy routes through `meijerint`. If it is many, move a minimal `meijerint`
  from M6 to M4.

**Warning signs:**
- An M4 plan with a task named "algebraic Risch".
- Definite-integral rows stuck at `unevaluated` while SymPy succeeds.

**Phase to address:** M4. The `meijerint` measurement belongs in M0/M1's TIR profiling. Confidence: MEDIUM-HIGH. The Risch
scope is verified; the `meijerint` share of TIR is unmeasured.

---

### Pitfall 13: GPU integer arithmetic assumptions that do not hold on Metal or WGSL

**What goes wrong:**
NTT, CRT and modular linear-algebra kernels are usually written with 64-bit primes and a 64×64 → 128 multiply, or at
least a u64 `mulhi`. On the M0 target this breaks in several ways:
- Metal has **no f64**, as SS already notes.
- Apple GPUs support 64-bit integers, but developer reports say 64-bit integer arithmetic is slow enough that 32-bit is
  preferable. Whether it is emulated is not documented.
- WGSL has no u64 except through wgpu's `SHADER_INT64` feature (Vulkan, DX12, Metal MSL 2.3+), and it has no `mulhi`.
- Metal's 64-bit atomics are limited (Dawn notes u64 min/max only, on newer GPU families).
- CubeCL compiles kernels at run time. An instruction the backend lacks is a run-time compile error, not a build error.

**Why it happens:**
CUDA-centric NTT literature and code assume `__umul64hi`.

**How to avoid:**
- In M0's Metal probe, measure first:
  1. u32 `mulhi` throughput;
  2. u64 multiply throughput;
  3. which CubeCL backend (the wgpu/WGSL one or the native Metal/MSL one, `cubecl-cpp`) exposes which ops.
- Design GPU modular arithmetic around **≤31-bit primes with 32-bit Montgomery/Barrett** reduction, and a portable
  `mulhi` fallback (16-bit limb split) for WGSL. Results are combined by CRT on the CPU.
- Use the *same* prime table on R0/R1 for the conformance path (Pitfall 7).
- Run conformance per backend: CubeCL-Metal, CubeCL-wgpu, and later CUDA. An operation class is admitted per backend
  only after each has been seen RED, then green.

**Warning signs:**
- Kernels that compile on CUDA and fail at run time on Metal.
- NTT throughput on Metal well below the u32 roofline.

**Phase to address:** M0 (probe), then M2 (NTT/CRT) and M5 (modular linear algebra). Confidence: MEDIUM. Metal and
CubeCL integer details are thinly documented, so this is exactly what the M0 probe is for.

---

### Pitfall 14: GPU determinism leaks: autotune, float atomics, fast-math, driver drift and watchdog kills

**What goes wrong:**
- **Autotune.** CubeCL's autotune benchmarks candidate kernels and caches the fastest one per key (on disk or shipped).
  Its docs say candidates "would all output the same result". That holds for exact kernels, but **not** for f32 kernels,
  whose reduction trees differ. The kernel chosen also depends on timing noise and hardware.
- **Float atomics.** Atomic float accumulation is order-nondeterministic. SS already bans it.
- **Fast math.** Metal shader compilation has had fast math on by default (`fastMathEnabled`, now `mathMode`). That
  permits reassociation, so Metal f32 results differ from CPU f32 even with fixed reductions.
- **Driver drift.** macOS updates change MTLCompilerService output. The CubeCL kernel cache pinned by digest (SS §4)
  covers the *source* or IR, but not the driver's machine code.
- **Watchdog.** macOS terminates long command buffers with a timeout error
  (`kIOGPUCommandBufferCallbackErrorTimeout`). A 10^6-degree NTT dispatched as one buffer can die nondeterministically
  under load.

**Why it happens:**
Each of these is invisible on a single developer run.

**How to avoid:**
- Disable autotune for every kernel whose output is not exact. For exact kernels, autotune is allowed, but conformance
  must run under *each* candidate. Pin the tune cache by digest in the lock alongside the kernel cache.
- Compile Metal kernels with safe or precise math explicitly. Record backend, device, OS build and driver/compiler
  version in every approximate result, as SS requires.
- Chunk dispatches below a measured safe duration. Treat a GPU timeout as a host error with a CPU fallback, never as a
  result.

**Warning signs:**
- An f32 grid's output digest changes after a reboot or an OS update.
- Intermittent `MTLCommandBufferStatus.error`.

**Phase to address:** M0 (probe: measure timeouts and fast-math defaults), then M1 (`lambdify` grids) and M2/M5 (exact
kernels). Confidence: MEDIUM. The autotune mechanism is verified from the CubeCL/Burn docs. The fast-math default comes
from older Apple docs plus a forum thread. The timeout error code is verified.

---

### Pitfall 15: macOS Seatbelt and Metal access is an unknown that can sink R2-on-Mac

**What goes wrong:**
R2 runs as a signed stdio worker inside `ProcessSandbox`. Metal needs, at minimum:
- `iokit-open` on the GPU's IOAccelerator user-client classes;
- `mach-lookup` to the Metal/AGX compiler services (MTLCompilerService, plus the AGX compiler service, whose name varies
  by device; WebKit had to match it by prefix);
- read access to the system Metal frameworks;
- write access to a shader cache directory.

Over-broad grants (blanket `iokit-open`) defeat the sandbox. Under-grants show up as "failed to create command queue" with
no useful error. A public Claude Code issue shows exactly this failure under a Seatbelt profile. Chromium's and WebKit's
GPU profiles are the best public references. Neither lists AGX class names authoritatively.

**Why it happens:**
Apple does not document the minimal profile, and the service names change between OS releases and devices.

**How to avoid:**
- Make this M0's *first* GPU task, as SS already calls it the top risk. Iterate a deny-by-default profile against
  Console.app `deny iokit-open`/`deny mach-lookup` logs on the M3 Max.
- File the measured rule set as the concrete `ComputeDevice` request to klein-rs.
- Point the Metal shader cache and the CubeCL cache at the sandbox's private tmp, or at a digest-pinned read-only cache.
- Re-run the probe on every macOS update; add it to the fixed-box checklist.

**Warning signs:**
- Metal works under `cargo run` but not inside `ProcessSandbox`.
- The profile accumulates wildcard `iokit-open` or `mach-lookup` rules.

**Phase to address:** M0 (the measurement). The rules are owned by klein-rs (dependency #1). Confidence: MEDIUM, from
Chromium/WebKit profiles and issue reports. Nothing authoritative from Apple.

---

### Pitfall 16: Oracle drift and golden-output flakiness

**What goes wrong:**
Golden outputs from "real SymPy" are not a single thing:
- **Version.** SymPy 1.13 made `Integer(1) == Float(1.0)` False. That is a deliberate break, and a later 1.13.x patch
  fixed further Float/Rational inconsistencies.
- **mpmath pin.** The current SymPy is **1.14.0** (PyPI, 2025-04-27). It pins `mpmath<1.4,>=1.1.0`, while mpmath 1.4.0 and
  1.4.1 are out (2026-02/03). An unpinned container either breaks or silently mixes versions. The `science.numerics`
  mpmath port must also choose 1.3.x or 1.4.x semantics deliberately.
- **Ground types.** `SYMPY_GROUND_TYPES` defaults to `auto`: it prefers python-flint, then gmpy2, then pure Python. The
  same SymPy version can therefore run on different arithmetic backends depending on what is installed. python-flint is
  LGPL FLINT, which is fine offline, but it is a hidden variable.
- **Hash seed.** SymPy's own test runner randomises `PYTHONHASHSEED`. Its tracker has spurious, hash-seed-dependent test
  failures. LLM programs that iterate Python `set`s or `dict`s of SymPy objects print in hash order, and that order
  depends on Python's randomised string hashing.
- **Dummy names.** `Dummy` symbols carry global counters (`_Dummy_123`) that depend on history.
- **Timeouts.** SymPy calls that take minutes give "timeout" goldens, which are not reproducible.

**Why it happens:**
"Run SymPy in a container" feels deterministic. It is only deterministic once every environmental input is pinned.

**How to avoid:**
- Pin, by image digest:
  - CPython version;
  - `sympy==1.14.0` and `mpmath==1.3.0` (or whatever is ratified), with no gmpy2 or python-flint installed unless that
    is chosen deliberately;
  - `SYMPY_GROUND_TYPES=python` (explicit);
  - `SYMPY_USE_CACHE` (explicit);
  - the platform architecture.
- Generate every golden **under at least three `PYTHONHASHSEED` values**. Rows whose output varies are tagged
  `order_insensitive`, compared as multisets or after canonical sorting, or quarantined.
- Replace `Dummy` names with canonical placeholders before comparison.
- Record a timeout as `oracle_timeout`, never as an expected output.
- Upgrade the SymPy oracle only as a deliberate, versioned event, with a diff report of every changed golden.

**Warning signs:**
- A golden regenerated "just to refresh" changes rows nobody touched.
- `_Dummy_` appears in stored goldens.
- The container `pip install`s without a lock.

**Phase to address:** M0 (the oracle pipeline). Confidence: HIGH (PyPI metadata and release announcements verified
today).

---

### Pitfall 17: SymPy is wrong sometimes, and "parity" would freeze its bugs

**What goes wrong:**
SymPy's tracker has **467 issues labelled "Wrong Result", 80 of them open** (GitHub search, 2026-10-11). Of those, 57 are
also labelled integrals, 47 series, 34 solvers and 26 simplify, and the labels overlap. A conformance gate that scores
string-equality against SymPy forces the CAS to reproduce wrong answers. Simply "fixing" them breaks LLM programs that
depend on SymPy's *result shape*: `solve` returns a list, a list of dicts or a list of tuples depending on the input.

**Why it happens:**
Golden outputs are treated as ground truth. Adjudication is ad hoc.

**How to avoid:**
- Keep the adjudication list (SS §5, part 4) as versioned data. Each entry has:
  - the SymPy issue URL;
  - the input;
  - SymPy's output;
  - the correct output;
  - the **independent** oracle that decided it: SymEngine, Giac or FLINT binaries offline, high-precision mpmath numeric
    evaluation, or a Lean proof;
  - the reviewer.
- Rule: *match SymPy's result shape and type always; match SymPy's mathematics only where SymPy is right.*
- An adjudication needs two agreeing independent oracles, or one Lean proof.
- Never add an adjudication entry just to make a failing row pass. Count entries per milestone and review them.

**Warning signs:**
- The adjudication list grows faster than fixes.
- Entries without an oracle citation.
- Entries that disagree with SymPy but cite no issue.

**Phase to address:** M0 (the schema and process), then every milestone. Confidence: HIGH (tracker counts measured
today).

---

### Pitfall 18: Licence contamination through ports, transitive crates and LLM recall

**What goes wrong:**
- **Transitive deps.** A transitive dependency pulls in `rug`/`gmp-mpfr-sys` (LGPL) or `malachite`. Malachite is
  LGPL-3.0, and its author states it is largely *translated from GMP and FLINT C code*, so even "porting from malachite"
  is porting GMP and FLINT.
- **LLM recall.** Agents asked for a "fast NTT", "van Hoeij factoring" or "Zassenhaus with FLINT-style tricks" reproduce
  GPL/LGPL code structure from training memory (FLINT, Giac, Maxima, PARI, Singular).
- **FLINT's licence.** FLINT is LGPL (moving to LGPL-3+ around 3.1; Debian reports most files LGPL-3+ by 3.5). That makes
  it look harmless, but CLAUDE.md bans linking or shipping any GPL or LGPL code.
- **Oracle containers.** Giac/FLINT binaries are allowed offline. An agent that reads their *source* while debugging a
  mismatch, then writes the fix, has contaminated the fix.

**Why it happens:**
Licence review happens at the dependency level. LLM output and "inspiration" are invisible to `cargo-deny`.

**How to avoid:**
- Add a `cargo-deny` licence allowlist: MIT, Apache-2.0, BSD-2/3, ISC, Zlib, Unicode, and the LLVM exception. Explicitly
  ban `rug`, `gmp-mpfr-sys`, `malachite*` and `flint*` crates. Run it in CI from the first commit, and see it RED on a
  deliberately added `rug` dependency.
- Add a **provenance header** per algorithm module, naming one of:
  - a paper (DOI);
  - a BSD/MIT source file and commit (SymPy, SymEngine, mpmath, Diofant, FriCAS);
  - "original".

  Review rejects modules without one. NOTICE carries the BSD/MIT notices.
- Agent prompts never name GPL or LGPL projects as sources of technique. Use papers instead: Monagan–Pearce, van Hoeij,
  Faugère, Harvey's NTT papers.
- Containers that run GPL oracles do not mount their source trees. Agents debug mismatches from inputs and outputs only.
- Optionally (an operator decision), run an offline clone-detection pass of the codebase against GPL corpora. A tool
  reports match locations only; no agent reads the GPL text.

**Warning signs:**
- `Cargo.lock` gains `gmp-mpfr-sys`.
- Code comments say "as in FLINT" or "like Giac".
- Unusually specific magic constants or tuning tables with no paper citation.

**Phase to address:** M0 (`cargo-deny`, provenance template, prompt rules), then continuously. Confidence: HIGH (malachite
licence and author statement verified; FLINT licence from distribution metadata).

---

### Pitfall 19: Performance claims measured on a laptop that is not a fixed box  **[SS-FLAG]**

**What goes wrong:**
SS budgets reference a "16-core CPU" ("16 cores ≥ 8× faster than one" and "GPU ≥ 5× faster than a 16-core CPU"). The
operator's M3 Max has **14 heterogeneous cores** (performance plus efficiency cores, per ROADMAP), so those budgets cannot
be measured on the M0/M1 box as written. Laptop measurements also vary with:
- thermal state, power source and Low Power Mode;
- memory pressure (ROADMAP: the Mac pages when klein-rs builds run alongside);
- Spotlight and Time Machine.

Unified memory makes host-to-device transfer nearly free on the Mac. GPU speedups measured there therefore overstate what a
discrete L40S/H100 achieves, and they do not transfer. Including or excluding first-run MSL compilation changes small-kernel
results by orders of magnitude.

**Why it happens:**
Budgets are written before the box exists. Rule 5 ("measured on a fixed box") is satisfied in letter by "the Mac".

**How to avoid:**
- Ratify per-box budgets. For the Mac, use "all P-cores vs one P-core", with the core count stated.
- Write a fixed-box manifest that every benchmark result records:
  - hardware and OS build;
  - power source and thermal state;
  - background-load check;
  - toolchain digests;
  - whether compile and transfer time are included (both numbers are reported).
- Refuse to record results when the box is under memory pressure or klein-rs builds are running.
- Repeat runs and report the median and spread with a noise threshold. Any budget comparison that compares to SymEngine
  runs on the same box in the same session.
- Re-measure GPU budgets on CUDA before claiming them for CUDA.

**Warning signs:**
- Budgets quoted with no box manifest.
- Speedups that change by more than 20% between identical runs.
- "Estimated" numbers in a PR description.

**Phase to address:** M0 (manifest and box ratification), then every milestone's budget gate. Confidence: HIGH.

---

## Moderate Pitfalls

### Pitfall 20: SymPy's test suite cannot run unmodified against a Monty stand-in

**What goes wrong:**
SS §5 lists "SymPy's own tests, run against the stand-in". SymPy's tests:
- subclass `Function`, `Basic` and `Expr` heavily, which Monty cannot do (no inheritance);
- use internals (`_args`, `func`, `srepr`, `Basic.__new__`, `_eval_*` hooks);
- run under pytest in CPython.

**How to avoid:**
- Make the stand-in valid in both the Monty subset *and* CPython, and run SymPy tests under CPython against the stand-in,
  bridged to the CAS. Run the TIR corpus under Monty.
- Curate the subset of SymPy tests that use only the public API, and track it as data.
- Report "tests not applicable (uses internals or subclassing)" separately from failures, so pass rates are not inflated
  or deflated.

**Phase to address:** M0 (dual-runtime decision), then M1 onwards. Confidence: HIGH.

### Pitfall 21: Printing and parsing are part of the semantics

**What goes wrong:**
LLM programs mostly `print()` results. The outputs that matter are:
- `str` and `repr` (identical for SymPy objects in StrPrinter mode);
- `latex()`;
- `pretty()`, occasionally;
- `Float` printing, which depends on precision: `Float(0.1)` prints `0.100000000000000`.

`sympify` and `parse_expr` apply transformations such as `^` handling and implicit multiplication only on request. Getting
any of these slightly wrong fails rows that are mathematically correct.

**How to avoid:**
Port `StrPrinter`, `LatexPrinter` and mpmath's `to_str` as code (BSD) with golden tests. Treat the printers as M1 core, not
polish.

**Phase to address:** M1. Confidence: HIGH.

### Pitfall 22: Software multiprecision `evalf` dismissed as "approximate"  **[SS-FLAG, refinement]**

**What goes wrong:**
SS tags all float results approximate and replays them from the record. mpmath-style arbitrary precision is integer
arithmetic on mantissa and exponent. A faithful port *can* be byte-identical across R0, R1 and R2. Treating it as
approximate gives up replay-by-recomputation and hides divergence between implementations.

Mixing `astro-float` for some functions with an mpmath port for others makes last-digit results differ from SymPy's
`evalf` goldens. SymPy's `evalf` is also adaptive: it raises working precision until the result is accurate to the
requested digits.

**How to avoid:**
- Use one MP float implementation for parity work, the mpmath port.
- Treat its results as **deterministic-approximate**: approximate in mathematics, exact in bytes, and conformance-gated
  like the exact classes.
- Reserve "replay from record" for hardware f32/f64.

This is an operator decision.

**Phase to address:** M1 (`evalf`) and the `science.numerics` plugin. Confidence: MEDIUM.

### Pitfall 23: Gruntz limits and series: ported bugs and missing assumptions

**What goes wrong:**
SymPy's `limit` runs heuristics first, then `gruntz`. Both have a long tail of wrong results: the tracker has 47
"Wrong Result" issues labelled series. Diofant (BSD) reworked parts of `gruntz`, but its API and automatic evaluation
diverge from SymPy's. Porting Diofant code verbatim therefore breaks SymPy output parity in surrounding code. Limits with
parameters need the assumptions from Pitfall 9.

**How to avoid:**
- Port the algorithm from Gruntz's thesis and SymPy's code. Take *bug fixes* from Diofant selectively, each one with a
  witness test.
- Check every limit numerically at high precision in conformance.

**Phase to address:** M4. Confidence: MEDIUM.

### Pitfall 24: WASM toolchain and component-model churn

**What goes wrong:**
SS targets `wasm32-wasip2`. WASI 0.3 (native async in the component model) was released 2026-06-11, and Wasmtime enabled
component-model async by default around v46. The WIT and toolchain ecosystem (`wit-bindgen`, `cargo-component`, the guest
toolchains) is moving. A rich WIT surface (records, variants, resources for expressions) has to be reworked at every
transition, and Klein's kernel, not this repo, picks the Wasmtime version.

SIMD128 is deterministic, but **relaxed-simd** is not unless Wasmtime's `relaxed_simd_deterministic` is set. NaN bit
patterns are not deterministic without `cranelift_nan_canonicalization`. There are no wasm threads, so do not plan on
`wasi-threads`; SS's R1 design (N independent instances) is correct.

**How to avoid:**
- Keep the WIT surface minimal: one typed operation call carrying JCS bytes in and out, plus a version. Expression
  structure is not mirrored in WIT.
- Never enable relaxed-simd in R0 builds.
- Ask klein-rs to document its NaN-canonicalisation and relaxed-simd settings. That matters for float replay, not for
  exact classes.
- Pin the build toolchain by digest so the reproducible `.wasm` digest is stable.

**Phase to address:** M0 (WIT tool). Confidence: MEDIUM-HIGH. WASI 0.3's date is from wasi.dev; the Wasmtime version is
from secondary sources.

### Pitfall 25: Goldens derived from third-party corpora committed to git

**What goes wrong:**
Rule 6 bans third-party corpus bytes in git. Golden outputs keyed by NuminaMath-TIR rows embed problem content
(expressions, constants, program text). Committing them violates rule 6 in spirit, and possibly the dataset's licence
terms.

**How to avoid:**
- Store goldens as artifacts addressed by digest, next to the corpus fetch recipe.
- Commit only the recipe, the digests and aggregate scores.
- Hand-written seed tests are fine in git.

**Phase to address:** M0 (oracle pipeline). Confidence: HIGH, as an application of the binding rules.

### Pitfall 26: Content hashing every node with SHA-256

**What goes wrong:**
SS defines expression identity as SHA-256 over the JCS tree. Computing it for every intermediate node during `expand` or
gcd costs roughly 10–100× an Fx-style hash plus a JCS serialisation, and it dominates small-expression benchmarks.

**How to avoid:**
- Intern on a fast structural hash with full structural equality on collision.
- Compute SHA-256/JCS lazily: at the tool boundary, for records, and for certificates.
- Make sure the fast hash never influences order (Pitfall 3).

**Phase to address:** M1. Confidence: HIGH as an engineering judgement. The ratio is an estimate and must be measured
(rule 5).

### Pitfall 27: Lean certificates that are too big or too slow, plus Mathlib churn

**What goes wrong:**
- `linear_combination` or `ring` certificates with huge Gröbner cofactors time out in Lean.
- `decide` on large naturals is slow in the kernel.
- `norm_num` primality for large primes needs Pratt certificates.
- Mathlib renames break certificate templates. `polyrith` was removed when its Sage backend shut down, and SS §6 plans to
  restore it with this CAS as backend.

**How to avoid:**
- Cap certificate size, and return `certifiable=false` above it.
- Prefer `norm_num` extensions with explicit witnesses.
- Pin the Mathlib version through the Lean environment manifest by digest.
- Run a template regression suite against each Mathlib bump.

**Phase to address:** M2 (ideal-membership and factorisation certificates) and M4 (the loop). Confidence: MEDIUM.

---

## Minor Pitfalls

### Pitfall 28: Python-number coercion mismatches
`1/2*x` in Python gives `0.5*x` (a Float), while `x/2` gives `x/2` (a Rational). LLM code depends on this. The stand-in
must reproduce CPython's evaluation order and coercions exactly, including `int` vs `Integer` hashing equality. **M0/M1.**

### Pitfall 29: Result-shape drift in `solve`
`solve` returns different container shapes depending on its arguments (`dict=True`, `set=True`, single vs multiple
symbols). Port the shape logic exactly, and test shapes separately from values. **M3.**

### Pitfall 30: Unicode symbol names
NFKC normalisation and the UTF-16 key ordering (Pitfall 4) mean Greek letters and subscripts need explicit tests. **M1.**

### Pitfall 31: `lambdify` semantics
SymPy's `lambdify` picks modules (`math`, `numpy`, `mpmath`) and their branch and precision conventions. The GPU grid path
must declare which convention it matches. **M1.**

---

## Technical Debt Patterns

| Shortcut | Immediate Benefit | Long-term Cost | When Acceptable |
|----------|-------------------|----------------|-----------------|
| `HashMap` with the default hasher in core crates | Fast to write | Nondeterministic output; hard to find later | Never in output-producing crates |
| Time-limited e-graph runs (egg default 5 s) | Simplify "always returns" | Machine-dependent results; breaks byte-identity | Never; use node, iteration and logical-fuel limits |
| Wasmtime fuel as the `fuel_exhausted` signal | No extra counter code | R0 and R2 disagree; changes with Klein's Wasmtime upgrades | Never for typed results; backstop only |
| String-equality scoring for simplify and integrate | Simple harness | Penalises correct answers; rewards copying SymPy's quirks | Only as a secondary metric |
| Dense or naive gcd and elimination over Q "for now" | Quick M2 progress | Swell; rewrite needed before budgets | Prototype branches only, never admitted |
| Unpinned SymPy container | Easy setup | Golden drift; unexplained failures | Never |
| Building the new `ask` assumptions before the old ones | Feels principled | M1 parity blocked | Never; old assumptions first |
| One giant GPU dispatch | Simpler kernel code | macOS watchdog kills under load | Only below a measured safe duration |
| Autotune on float kernels | Peak speed | Nondeterministic f32 output | Never on non-exact kernels |

## Integration Gotchas

| Integration | Common Mistake | Correct Approach |
|-------------|----------------|------------------|
| Monty guest | Assuming CPython's dunder protocol | Enumerate the required dunders from TIR and file one klein-rs request; shim `isinstance` (Pitfall 1) |
| SymPy oracle container | `pip install sympy` unpinned; ground types `auto`; random hash seed | Image digest; `sympy==1.14.0`, `mpmath==1.3.0`; `SYMPY_GROUND_TYPES=python`; multi-seed goldens |
| SymEngine oracle | Using it for printed-form parity | Use it for speed and math equivalence only |
| Giac/FLINT oracle | Mounting source trees while debugging | Binaries only; agents see inputs and outputs only |
| CubeCL | Assuming autotune and the kernel cache are deterministic and covered by the digest | Pin the tune cache too; disable autotune on float kernels; conformance per candidate |
| Metal under Seatbelt | Blanket `iokit-open` and `mach-lookup` | Measured minimal rules from Console.app deny logs, as a klein-rs request |
| Wasmtime (owned by klein-rs) | Relying on fuel or NaN behaviour without knowing kernel settings | Logical fuel; document Klein's relaxed-simd and NaN settings |
| Lean/Mathlib | Unpinned Mathlib; total-function junk values | Digest-pinned environment; faithfulness hypotheses |

## Performance Traps

| Trap | Symptoms | Prevention | When It Breaks |
|------|----------|------------|----------------|
| SHA-256 per intermediate node | `expand` slow on many small terms | Fast interning hash; lazy SHA-256 at the boundary | Thousands of nodes per operation |
| Generic bigint on small values | 10× slower than SymEngine on small cases | Small-integer fast paths (SS lever) | Every benchmark with small coefficients |
| Recursive traversals | Stack overflow in R0 only | Explicit stacks | Nesting depth of a few thousand in wasm |
| 64-bit modular arithmetic on Apple GPU | NTT far below u32 roofline | 31-bit primes, 32-bit Montgomery | All Metal NTT and CRT work |
| R0 4 GiB linear memory | Traps on large Gröbner bases or matrices | R2-only admission plus recorded replay | Working sets above ~3 GiB |
| Unbounded AC saturation | Node limit hit on small inputs | Flattened n-ary AC nodes, no AC rewrites | Around 6–8 summands with comm and assoc rules |
| GPU first-run compile in timings | Huge variance on small kernels | Report warm and cold separately | Every small-kernel benchmark |

## Security Mistakes

| Mistake | Risk | Prevention |
|---------|------|------------|
| Broad Seatbelt grants for Metal | A sandbox escape surface through the IOKit user clients | Minimal measured rules; a re-probe per OS update |
| R2 worker reading OS randomness or the network | Nondeterminism; exfiltration path | No network (SS); deny `getrandom` in CAS crates; seed from the content digest |
| Unbounded inputs from LLM code (`factorint(10**400+1)`, huge `expand`) | DoS of the host and CPU | Logical fuel plus backstop memory and fuel caps; typed `fuel_exhausted` |
| Shader or kernel cache writable and shared | Cache poisoning across runs | Digest-pinned read-only cache; private tmp |
| Oracle containers with network at golden-generation time | Unpinned fetches change goldens | Offline containers; inputs by digest |

## UX Pitfalls (the user is the LLM author and the operator)

| Pitfall | User Impact | Better Approach |
|---------|-------------|-----------------|
| Returning `unevaluated` silently where SymPy returns an answer | LLM programs print `Integral(...)` and fail the task | `SolveResult.status` plus a `complete` flag; log a parity gap |
| Error messages unlike SymPy's (`NotImplementedError`, `TypeError`) | LLM retry logic, tuned on SymPy, misfires | Raise SymPy's exception types and messages |
| Simplify "better" than SymPy | Downstream string checks in LLM code fail | Default to the `sympy-compatible` strategy |
| `float(expr)` or f-string failures from missing dunders | The most common TIR patterns crash | Pitfall 1 dunder list |

## "Looks Done But Isn't" Checklist

- [ ] **Determinism:** the byte-identical claim was tested under several thread counts, R0/R1/R2, and a deliberate `HashMap` injection seen RED.
- [ ] **Canonical form:** term order verified against real SymPy `str()` output, not hand-written expectations.
- [ ] **Assumptions:** `sqrt(x**2)`, `Abs(x)` and `log(exp(x))` were checked with `positive=True` and `real=True` symbols.
- [ ] **JCS identity:** a 300-digit integer and a big rational round-trip without loss; `oo`, `zoo` and `nan` are encoded.
- [ ] **Fuel:** `fuel_exhausted` comes from logical fuel and is identical on R0 and R2.
- [ ] **GPU kernel admitted:** conformance was run under every autotune candidate and on each backend; RED seen first.
- [ ] **Goldens:** generated under three or more hash seeds; `Dummy` names normalised; container pinned by digest; not committed to git.
- [ ] **Adjudication entry:** cites a SymPy issue and two independent oracles (or a Lean proof).
- [ ] **Simplify and integrate scored** by semantic equivalence (derivative check, numeric zero-test), not only by string.
- [ ] **Lean certificate:** the statement carries explicit domain hypotheses; there is no reliance on junk values.
- [ ] **Licence:** `cargo-deny` was RED once on `rug`; every algorithm module has a provenance header.
- [ ] **Performance claim:** it has a fixed-box manifest, median and spread, warm and cold reported, and a same-session SymEngine baseline.
- [ ] **Metal in sandbox:** it runs under the actual `ProcessSandbox` profile, not under `cargo run`.

## Recovery Strategies

| Pitfall | Recovery Cost | Recovery Steps |
|---------|---------------|----------------|
| Hash-order nondeterminism discovered late | MEDIUM | Lint ban plus the multi-thread CI job; bisect with the JCS diff; re-baseline goldens |
| Canonical form diverges from SymPy core | HIGH | Re-port `Add/Mul/Pow.flatten`; every downstream golden shifts, so do it before M2 |
| Wasmtime fuel used as the result status | MEDIUM | Add logical fuel; reclassify old `fuel_exhausted` records as host errors |
| JCS numbers as JSON numbers | HIGH after records exist | Schema v2; identity digests change; migrate records by re-canonicalisation |
| GPL-derived code found | HIGH | Remove the module; clean-room rewrite from papers by a lane that never saw the code; audit git history |
| Unpinned oracle drift | LOW–MEDIUM | Pin, regenerate, and diff-review every changed golden |
| egg time limit in simplify | LOW | Set `time_limit` to off and use node, iteration and logical fuel; re-run conformance |
| Metal Seatbelt blocked | MEDIUM | R2 on Mac stays CPU-only (rayon) until klein-rs ships the rules; GPU work moves to the deferred remote CUDA box (needs operator approval and spend) |

## Pitfall-to-Phase Mapping

| Pitfall | Prevention Phase | Verification |
|---------|------------------|--------------|
| 1 Monty dunders and inheritance | M0 | TIR protocol census; the klein-rs request filed; a stand-in prototype runs `x**2 + 2*x`, `float(sol)` and f-strings in Monty |
| 2 SymPy-faithful automatic evaluation | M1 | `core` golden suite from pinned SymPy; zero term-order diffs |
| 3 Order nondeterminism | M1 (before any R1/R2 admission) | Multi-thread-count, R0/R1/R2 byte-identity CI, seen RED |
| 4 JCS number encoding | M0 schema, M1 implementation | Big-number and special-value round-trip property tests |
| 5 Logical fuel and resource limits | M0 decision, M1 implementation | Same `fuel_exhausted` row on R0 and R2; deep-nesting test passes in R0 |
| 6 Expression swell | M2, M5 | Swell benchmarks; Fateman, `expand2` and `lwbench` within budget |
| 7 Randomised algorithms | M2 | Repeated runs byte-identical; no `getrandom` in the dependency graph |
| 8 e-graph AC blowup and time limits | M3 | No time limit in config (grep plus test); node counts bounded on the AC stress set |
| 9 Assumptions in core | M1 (old), M3 (new) | The assumption-dependent evaluation suite passes |
| 10 Simplify definition | M3 | Equivalence scoring in place; `sympy-compatible` strategy parity measured |
| 11 Branch cuts and Lean semantics | M1 schema, M3 rules, M4 Lean | Every rule has side-conditions; faithfulness check rejects junk-value proofs |
| 12 Risch scope and meijerint | M0/M1 (measure), M4 | Three-way Risch outcome mapped; algebraic case explicitly out of scope |
| 13 GPU integer arithmetic | M0 probe, M2, M5 | Measured u32/u64 throughput; per-backend conformance |
| 14 GPU determinism leaks | M0 probe, M1/M2/M5 | Tune cache pinned; fast-math off; chunked dispatch; no timeouts under load |
| 15 Seatbelt and Metal | M0 | Metal works inside `ProcessSandbox` with a minimal profile |
| 16 Oracle drift | M0 | Pinned image; multi-seed goldens; a no-op regeneration gives zero diff |
| 17 SymPy wrong answers | M0 process, all milestones | Every adjudication entry has an issue and oracles |
| 18 Licence contamination | M0, continuous | `cargo-deny` RED-then-green; provenance headers enforced in review |
| 19 Fixed-box performance | M0, every budget gate | Manifest attached to every budget result; budgets ratified per box |
| 20 SymPy tests under Monty | M0 decision, M1 onwards | Dual-runtime stand-in; curated test subset as data |
| 21 Printing and parsing | M1 | Printer goldens (`str`, `latex`, Float) |
| 22 Deterministic MP evalf | M1 (operator decision) | `evalf` byte-identical R0/R2 if adopted |
| 23 Gruntz | M4 | High-precision numeric check of every limit |
| 24 WASM churn | M0 | Minimal WIT; relaxed-simd off; toolchain pinned |
| 25 Goldens in git | M0 | No corpus-derived bytes in git (CI check) |
| 26 SHA-256 per node | M1 | Benchmark of interning cost |
| 27 Lean certificates | M2, M4 | Size caps; Mathlib bump regression suite |

## Items that disagree with or refine the single source [SS-FLAG] (for operator ruling)

1. **Monty dunders (Pitfall 1).** SS M0 treats operator overloading as a spike item. Monty's official docs (fetched
   2026-10-11) say arithmetic, comparison, `__bool__` and conversion dunders are *not dispatched*, and inheritance is
   rejected. The klein-rs dependency #4 request should list the full protocol set, and it gates M1.
2. **`fuel_exhausted` semantics (Pitfall 5).** For byte-identity, `fuel_exhausted` must be based on algorithm-level logical
   fuel. Wasmtime fuel serves only as a host backstop.
3. **Assumptions placement (Pitfall 9).** Old-style per-symbol assumptions belong in M1, because core evaluation depends on
   them. M3 keeps `ask`/`refine`.
4. **Simplify strategy (Pitfall 10).** Recommend a ported SymPy `simplify` pipeline as the default (`sympy-compatible`).
   egglog becomes a bounded, opt-in strategy rather than *the* simplify. Score by equivalence.
5. **Meijer G timing (Pitfall 12).** SymPy's definite integration often uses `meijerint`. Measure the TIR share early; if
   it is large, pull a minimal `meijerint` into M4.
6. **Budgets naming 16 cores (Pitfall 19).** The M3 Max has 14 heterogeneous cores. Ratify per-box budgets, and note that
   GPU speedups from unified memory do not transfer to CUDA.
7. **MP `evalf` determinism (Pitfall 22, optional).** Software multiprecision results can be byte-identical and
   conformance-gated instead of replay-only.
8. **SymPy tests "against the stand-in" (Pitfall 20).** These need a CPython run of the stand-in, because the tests subclass
   SymPy types, which Monty cannot do.
9. **Goldens and rule 6 (Pitfall 25).** Goldens derived from corpora are stored by digest, not in git.

## Sources

- Monty class limitations (official, fetched 2026-10-11): https://pydantic.dev/docs/monty/limitations/classes/ (HIGH)
- RFC 8785 JCS: https://www.rfc-editor.org/rfc/rfc8785.html (HIGH)
- Wasmtime deterministic execution (fuel vs epochs, NaN canonicalisation, relaxed-simd): https://docs.wasmtime.dev/examples-deterministic-wasm-execution.html ; https://docs.wasmtime.dev/examples-interrupting-wasm.html (HIGH)
- WASI 0.3 release: https://wasi.dev/releases/wasi-p3 ; https://bytecodealliance.org/articles/WASI-0.3 (MEDIUM-HIGH)
- egg `Runner` defaults (iter 30, nodes 10,000, time 5 s, BackoffScheduler): https://docs.rs/egg/latest/egg/struct.Runner.html (HIGH)
- AC blowup in e-graphs: https://compilers.cs.uni-saarland.de/papers/ullrich_egraphs2026-paper17.pdf ; https://odr.chalmers.se/items/2cad1f28-78b6-40de-860d-1ad19f0d1195/full (MEDIUM)
- CubeCL autotune: https://burn.dev/books/cubecl/core-features/autotune.html ; https://burn.dev/blog/autotune-for-gpu-kernels ; https://docs.rs/crate/cubecl/latest (MEDIUM)
- wgpu `SHADER_INT64`: https://newreleases.io/project/github/gfx-rs/wgpu/release/v0.20.0 ; Dawn Metal atomic64: https://dawn.googlesource.com/dawn/+/b620b2fe514a5ee2e6a2d924fd998c514019da87 (MEDIUM)
- Apple GPU 64-bit integer speed (anecdotal): https://developer.apple.com/forums/thread/718737 (LOW)
- Metal math mode: https://developer.apple.com/documentation/metal/mtlcompileoptions/mathmode ; https://developer.apple.com/forums/thread/118472 (MEDIUM)
- Metal command buffer timeout: https://developer.apple.com/documentation/metal/mtlcommandbuffererror-swift.struct/code ; https://github.com/KhronosGroup/MoltenVK/discussions/2080 (MEDIUM)
- Seatbelt and Metal: Chromium GPU profile https://gitcode.com/openharmony-tpc/chromium_src/blob/master/sandbox/policy/mac/gpu.sb ; WebKit profile https://opensource.apple.com/source/WebKit2/WebKit2-7611.2.7.0.4/WebProcess/com.apple.WebProcess.sb.in.auto.html ; https://claudeissues.com/issue/37481-excludedcommands-does-not-bypass-macos-sandbox-for-metal-gpu-iokit-access (MEDIUM)
- SymPy 1.13 Float/Rational equality change: https://groups.google.com/g/sympy/c/eWNRsZBEA5g (HIGH)
- SymPy 1.14.0 and its `mpmath<1.4` pin; mpmath 1.4.0/1.4.1 dates: PyPI JSON API, queried 2026-10-11 (HIGH)
- SymPy ground types: https://groups.google.com/g/sympy/c/eWNRsZBEA5g (MEDIUM-HIGH)
- SymPy hash-order notes: https://github.com/sympy/sympy/wiki/Development-Tips:-Comparisons-in-Python ; https://github.com/sympy/sympy/issues/15487 (MEDIUM)
- SymPy "Wrong Result" label counts: GitHub search API, `repo:sympy/sympy label:"Wrong Result"`, 2026-10-11 (HIGH)
- SymPy Risch scope: https://diofant.readthedocs.io/en/latest/_sources/release/notes-0.7.3.rst.txt ; https://groups.google.com/g/sympy/c/JGT6dk9vR6o (MEDIUM-HIGH)
- malachite licence and GMP/FLINT derivation: https://users.rust-lang.org/t/malachite-a-new-arbitrary-precision-arithmetic-library/76587 ; https://metadata.ftp-master.debian.org/changelogs/main/r/rust-malachite-base/stable_copyright (HIGH)
- FLINT licence history: https://debbugs.gnu.org/db/69/69871.html ; https://dfsg-new-queue.debian.org/reviews/flint/3.5.0-1~exp1 (MEDIUM)
- Crate licences (dashu MIT/Apache, egglog MIT, egg MIT, cubecl MIT/Apache, astro-float MIT, wasmtime Apache-2.0 with the LLVM exception): crates.io API, 2026-10-11 (HIGH)
- Lean/Mathlib junk values: https://leansearch.net/doc/Mathlib.Data.Real.Sqrt.html ; https://huggingface.co/datasets/cat-searcher/minif2f-lean4/discussions/2 ; https://www.ma.imperial.ac.uk/~buzzard/xena/formalising-mathematics-2023/Part_A/section02reals/reals.html (MEDIUM-HIGH)
- Domain knowledge (MEDIUM, standard CAS literature, not re-fetched): expression swell and modular/fraction-free methods (Geddes–Czapor–Labahn; von zur Gathen–Gerhard), Monagan–Pearce sparse arithmetic, BPSW, Gruntz's thesis.

---
*Pitfalls research for: SymPy-parity Rust CAS with deterministic multi-backend exact arithmetic*
*Researched: 2026-10-11*
