# provenance: original
"""conformance/oracles/sympy/slice.py: the stratified, hash-ranked golden slice (D-09).

slice.py is a script, not a package module, so it is imported by path.
Run: python3 -m unittest discover -s conformance -t conformance
"""

import hashlib
import importlib.util
import pathlib
import unittest

SLICE_PATH = pathlib.Path(__file__).resolve().parents[2] / "oracles" / "sympy" / "slice.py"
_spec = importlib.util.spec_from_file_location("ksci_golden_slice", SLICE_PATH)
golden_slice = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(golden_slice)

PARQUET = "66163f9fdf2220fed2116142781fe7019a117191e7f08a3339a70a98401061d8"

# A small stand-in for names.json (plan 01-05's introspection table).
TABLE = {
    "all": [
        {"name": "solve", "module": "sympy.solvers.solvers", "kind": "function"},
        {"name": "integrate", "module": "sympy.integrals.integrals", "kind": "function"},
        {"name": "symbols", "module": "sympy.core.symbol", "kind": "function"},
        {"name": "factorint", "module": "sympy.ntheory.factor_", "kind": "function"},
        {"name": "latex", "module": "sympy.printing.latex", "kind": "function"},
        {"name": "plot", "module": "sympy.plotting.plot", "kind": "function"},
    ],
    "abc": ["x", "y"],
}


def program(code):
    return {"case_id": hashlib.sha256(code.encode("utf-8")).hexdigest(), "code": code}


def corpus():
    """Deterministic programs over several strata, plus non-SymPy and unparseable ones."""
    out = []
    for i in range(40):
        out.append(program(f"import sympy as sp\nprint(sp.solve({i}))"))
    for i in range(25):
        out.append(program(f"from sympy import integrate, solve\nintegrate({i})"))
    for i in range(12):
        out.append(program(f"from sympy.abc import x\nprint(x + {i})"))
    for i in range(5):
        out.append(program(f"import sympy\nsympy.plot({i})"))
    for i in range(3):
        out.append(program(f"import sympy\nprint({i})"))
    for i in range(7):
        out.append(program(f"import math\nprint(math.sqrt({i}))"))
    out.append(program("import sympy\ndef (:"))
    return out


def selected_ids(programs, n):
    seed = golden_slice.derive_seed(PARQUET)
    selected, _ = golden_slice.select(programs, TABLE, seed, n)
    return [r["case_id"] for r in selected]


class Seed(unittest.TestCase):
    def test_seed_is_derived_from_the_parquet_digest(self):
        expected = hashlib.sha256(PARQUET.encode("ascii") + b"golden-slice-v1").digest()
        self.assertEqual(golden_slice.derive_seed(PARQUET), expected)


class Strata(unittest.TestCase):
    def test_stratum_priority(self):
        modules = {row["name"]: row["module"] for row in TABLE["all"]}
        cases = [
            ({"solve", "integrate"}, "integrals"),
            ({"solve", "symbols"}, "solvers"),
            ({"abc:x"}, "core"),
            ({"plot"}, "other"),
            ({"not_in_sympy"}, "other"),
            (set(), "none"),
        ]
        for names, expected in cases:
            with self.subTest(names=sorted(names)):
                self.assertEqual(golden_slice.stratum_of(names, modules), expected)


class Allocation(unittest.TestCase):
    def test_slice_allocation_floor_per_stratum(self):
        k = golden_slice.allocate({"a": 900, "b": 90, "c": 10}, 100)
        self.assertEqual(k, {"a": 80, "b": 10, "c": 10})

    def test_slice_allocation_largest_remainder(self):
        k = golden_slice.allocate({"a": 333, "b": 333, "c": 334}, 100)
        self.assertEqual(k, {"a": 33, "b": 33, "c": 34})

    def test_slice_allocation_small_strata_taken_whole(self):
        k = golden_slice.allocate({"a": 500, "b": 3, "c": 0}, 50)
        self.assertEqual(k, {"a": 47, "b": 3, "c": 0})
        self.assertEqual(sum(k.values()), 50)

    def test_slice_allocation_takes_everything_when_population_is_small(self):
        self.assertEqual(golden_slice.allocate({"a": 4, "b": 2}, 100), {"a": 4, "b": 2})


class Selection(unittest.TestCase):
    def test_slice_rank_deterministic_and_order_free(self):
        programs = corpus()
        first = selected_ids(programs, 50)
        self.assertEqual(len(first), 50)
        self.assertEqual(first, selected_ids(programs, 50))
        self.assertEqual(first, selected_ids(list(reversed(programs)), 50))
        by_digest = sorted(programs, key=lambda p: hashlib.sha256(p["code"].encode()).digest()[::-1])
        self.assertEqual(first, selected_ids(by_digest, 50))

    def test_slice_rank_takes_lowest_keys_per_stratum(self):
        programs = corpus()
        seed = golden_slice.derive_seed(PARQUET)
        selected, stats = golden_slice.select(programs, TABLE, seed, 50)
        solvers = [p["case_id"] for p in programs if "sp.solve" in p["code"]]
        ranked = sorted(solvers, key=lambda c: hashlib.sha256(seed + bytes.fromhex(c)).digest())
        k = stats["strata"]["solvers"]["selected"]
        chosen = sorted(r["case_id"] for r in selected if r["stratum"] == "solvers")
        self.assertEqual(chosen, sorted(ranked[:k]))

    def test_slice_stats(self):
        seed = golden_slice.derive_seed(PARQUET)
        selected, stats = golden_slice.select(corpus(), TABLE, seed, 50)
        self.assertEqual(stats["sympy_using"], 85)
        self.assertEqual(stats["syntax_errors"], 1)
        populations = {s: v["population"] for s, v in stats["strata"].items()}
        self.assertEqual(
            populations, {"core": 12, "integrals": 25, "none": 3, "other": 5, "solvers": 40}
        )
        self.assertEqual(sum(v["selected"] for v in stats["strata"].values()), len(selected))
        self.assertEqual([r["case_id"] for r in selected], sorted(r["case_id"] for r in selected))


if __name__ == "__main__":
    unittest.main()
