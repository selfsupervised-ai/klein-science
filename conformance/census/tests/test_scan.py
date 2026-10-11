# provenance: original
"""census.scan: which programs use SymPy, and which SymPy names they use.

Run: python3 -m unittest discover -s conformance -t conformance
"""

import unittest
import warnings

from census import scan

# A small stand-in for the introspection table's names (plan 01-05's names.json).
ALL_NAMES = frozenset({"solve", "symbols", "Symbol", "Eq", "integrate", "pi", "sqrt", "polys"})
ABC_NAMES = frozenset({"x", "y", "z"})


def imports(code):
    return scan.sympy_imports(scan.parse(code))


def names(code):
    return scan.sympy_names(scan.parse(code), ALL_NAMES, ABC_NAMES)


class ScanImports(unittest.TestCase):
    def test_scan_imports_each_form_is_sympy_using(self):
        cases = {
            "import sympy": {"sympy"},
            "import sympy as sp": {"sympy"},
            "from sympy import x": {"sympy"},
            "from sympy.abc import x": {"sympy.abc"},
            "import sympy.solvers": {"sympy.solvers"},
        }
        for code, expected in cases.items():
            with self.subTest(code=code):
                self.assertEqual(imports(code), expected)

    def test_scan_imports_numpy_alone_is_not_sympy_using(self):
        self.assertEqual(imports("import numpy"), set())
        self.assertEqual(imports("import numpy as np\nfrom math import sqrt"), set())

    def test_scan_imports_nested_and_relative(self):
        self.assertEqual(imports("def f():\n    import sympy as s\n    return s"), {"sympy"})
        self.assertEqual(imports("from .sympy import solve"), set())
        self.assertEqual(imports("import sympyx"), set())


class ScanNames(unittest.TestCase):
    def test_scan_names_module_alias_attribute(self):
        self.assertEqual(names("import sympy as sp\nsp.solve(1)"), {"solve"})

    def test_scan_names_from_import(self):
        self.assertEqual(names("from sympy import symbols, solve"), {"symbols", "solve"})

    def test_scan_names_star_import_resolved_by_reference(self):
        self.assertEqual(names("from sympy import *\nx = symbols('x')"), {"symbols"})

    def test_scan_names_abc(self):
        self.assertEqual(names("from sympy.abc import x"), {"abc:x"})

    def test_scan_names_submodule_import_binds_sympy(self):
        self.assertEqual(names("import sympy.solvers\nsympy.solve(1)"), {"solve"})

    def test_scan_names_other_module_alias_is_not_sympy(self):
        self.assertEqual(names("import numpy as np\nnp.solve(1)"), set())

    def test_scan_names_attribute_outside_table_is_not_a_name(self):
        self.assertEqual(names("import sympy as sp\nsp.not_a_name(1)\nsp.pi"), {"pi"})


class ScanSyntaxError(unittest.TestCase):
    def test_scan_syntax_error_returns_none(self):
        for code in ("def (:", "x = 1\0", "(" * 1000):
            with self.subTest(code=code[:10]):
                self.assertIsNone(scan.parse(code))

    def test_scan_parse_valid(self):
        self.assertIsNotNone(scan.parse("import sympy\n"))

    def test_scan_parse_ignores_compiler_warnings(self):
        # An invalid escape is a SyntaxWarning; under warnings-as-errors it must not make a valid
        # program look unparseable.
        with warnings.catch_warnings():
            warnings.simplefilter("error")
            self.assertIsNotNone(scan.parse("s = '\\('\n"))


if __name__ == "__main__":
    unittest.main()
