# provenance: original
"""The stand-in's import surface (GUEST-01).

The five import forms a guest program uses must work with no tool call, the stand-in must bind exactly SymPy 1.14.0's
runtime `__all__` (921 names) and `sympy.abc` names, and a missing name must fail with CPython's own ImportError or
AttributeError text for a module named `sympy`.

Run: python3 -m unittest discover -s standin -t standin -p 'test_imports.py' -v
"""

import ast
import hashlib
import importlib
import inspect
import os
import re
import sys
import unittest

STANDIN = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
if STANDIN in sys.path:
    sys.path.remove(STANDIN)
sys.path.insert(0, STANDIN)

import sympy  # noqa: E402
from sympy import _tool  # noqa: E402

import host_shim  # noqa: E402

if not os.path.abspath(sympy.__file__).startswith(STANDIN + os.sep):
    raise ImportError("sympy resolved to " + sympy.__file__ + ", not the stand-in under " + STANDIN)

# Computed from the introspection table (blob 011f497f...1da9, conformance/oracles/images.json), not from the
# generated files: SHA-256 of the names joined by "\n", in `sympy.__all__` order and in sorted `sympy.abc` order.
ALL_NAMES_SHA256 = "fa0874d8e8a9153f6245af170763032146f10b0ba4f5ca92deb57d71e1d7b8f5"
ABC_NAMES_SHA256 = "78e6b2acd8ae0313f32fcb7636f7d10e910c88bc0b46814af5381c7b00f64d0d"
LITERAL_COUNT = 902
SUBPACKAGES = [
    "algebras", "assumptions", "calculus", "concrete", "discrete", "external", "functions", "geometry",
    "interactive", "multipledispatch", "ntheory", "parsing", "plotting", "polys", "printing", "release",
    "strategies", "tensor", "utilities",
]

# Pattern 6: CPython appends " (<module file>)" to a missing-name ImportError. Remove only that.
TRAILING_PATH = re.compile(r"\s\([^()]*\)$")


def normalise(message):
    return TRAILING_PATH.sub("", message)


def digest(names):
    return hashlib.sha256("\n".join(names).encode("utf-8")).hexdigest()


class ImportSurface(unittest.TestCase):
    def setUp(self):
        # host_shim with no worker: the installed transport refuses every call. The spy records any attempt.
        self.saved_worker = os.environ.pop("KSCI_WORKER", None)
        host_shim.install()
        self.attempts = []
        refuse = _tool._transport

        def spy(request):
            self.attempts.append(request)
            return refuse(request)

        _tool._transport = spy

    def tearDown(self):
        host_shim.reset()
        _tool._transport = None
        if self.saved_worker is not None:
            os.environ["KSCI_WORKER"] = self.saved_worker

    def assert_no_tool_call(self):
        self.assertEqual(host_shim.calls, 0)
        self.assertEqual(self.attempts, [])

    def run_form(self, source):
        # sympy.abc is not imported by host_shim, so dropping it makes the abc form execute that module's body here.
        sys.modules.pop("sympy.abc", None)
        if hasattr(sympy, "abc"):
            delattr(sympy, "abc")
        namespace = {}
        exec(source, namespace)
        self.assert_no_tool_call()
        return namespace

    def test_import_sympy(self):
        namespace = self.run_form("import sympy")
        self.assertIs(namespace["sympy"], sympy)
        self.assertEqual(set(namespace) - {"__builtins__"}, {"sympy"})

    def test_import_sympy_as_sp(self):
        namespace = self.run_form("import sympy as sp")
        self.assertIs(namespace["sp"], sympy)

    def test_from_sympy_import_symbols(self):
        namespace = self.run_form("from sympy import symbols")
        self.assertIs(namespace["symbols"], sympy.symbols)

    def test_from_sympy_import_star_binds_exactly_all(self):
        namespace = self.run_form("from sympy import *")
        bound = set(namespace) - {"__builtins__"}
        self.assertEqual(bound, set(sympy.__all__))
        self.assertEqual(len(bound), LITERAL_COUNT + len(SUBPACKAGES))

    def test_from_sympy_abc_import_x(self):
        namespace = self.run_form("from sympy.abc import x")
        self.assertTrue(inspect.isfunction(namespace["x"]))
        self.assertTrue(os.path.abspath(sys.modules["sympy.abc"].__file__).startswith(STANDIN + os.sep))

    def test_all_is_sympy_runtime_all_in_order(self):
        self.assertIsInstance(sympy.__all__, list)
        self.assertEqual(len(sympy.__all__), 921)
        self.assertEqual(len(set(sympy.__all__)), 921)
        self.assertEqual(digest(sympy.__all__), ALL_NAMES_SHA256)
        self.assertEqual(sympy.__all__[LITERAL_COUNT:], SUBPACKAGES)
        abc = importlib.import_module("sympy.abc")
        self.assertEqual(len(abc.__all__), 76)
        self.assertEqual(digest(abc.__all__), ABC_NAMES_SHA256)

    def test_every_name_is_a_plain_function_naming_itself(self):
        abc = importlib.import_module("sympy.abc")
        for module, prefix in ((sympy, "sympy."), (abc, "sympy.abc.")):
            for name in module.__all__:
                value = getattr(module, name)
                if module is sympy and name == "__version__":
                    self.assertEqual(value, "1.14.0")
                    continue
                with self.subTest(name=prefix + name):
                    self.assertTrue(inspect.isfunction(value))
                    with self.assertRaises(NotImplementedError) as caught:
                        value()
                    self.assertEqual(str(caught.exception), prefix + name + " is not available in the klein-science stand-in")
        self.assert_no_tool_call()

    def test_symbols_raises_not_implemented_naming_it(self):
        with self.assertRaises(NotImplementedError) as caught:
            sympy.symbols("x")
        self.assertIn("sympy.symbols", str(caught.exception))
        self.assert_no_tool_call()

    def test_missing_name_raises_sympy_import_error_text(self):
        with self.assertRaises(ImportError) as caught:
            exec("from sympy import binom", {})
        message = str(caught.exception)
        self.assertTrue(message.endswith(")"), message)
        self.assertEqual(normalise(message), "cannot import name 'binom' from 'sympy'")
        self.assert_no_tool_call()

    def test_missing_abc_name_raises_import_error_text(self):
        with self.assertRaises(ImportError) as caught:
            exec("from sympy.abc import nonexistent_name", {})
        self.assertEqual(normalise(str(caught.exception)), "cannot import name 'nonexistent_name' from 'sympy.abc'")

    def test_missing_attribute_raises_attribute_error_text(self):
        with self.assertRaises(AttributeError) as caught:
            sympy.not_a_name
        self.assertEqual(str(caught.exception), "module 'sympy' has no attribute 'not_a_name'")

    def test_names_are_exact_str_without_case_folding(self):
        # EDGE GUEST-01 encoding: `Symbol` exists, `SYMBOL` and `symbol` do not.
        self.assertIn("Symbol", sympy.__all__)
        for name in ("SYMBOL", "symbol"):
            with self.subTest(name=name):
                with self.assertRaises(ImportError) as caught:
                    exec("from sympy import " + name, {})
                self.assertEqual(normalise(str(caught.exception)), "cannot import name '" + name + "' from 'sympy'")
        # The normaliser removes only a trailing parenthesised path; a message without one is unchanged.
        self.assertEqual(normalise("cannot import name 'x' from 'sympy'"), "cannot import name 'x' from 'sympy'")

    def test_module_bodies_are_static(self):
        # Importing runs only the module body. A body of a docstring, plain defs and constant assignments cannot
        # make a tool call, whatever a later edit adds inside a function.
        for filename in ("__init__.py", "abc.py"):
            path = os.path.join(STANDIN, "sympy", filename)
            with open(path, encoding="utf-8") as handle:
                tree = ast.parse(handle.read(), path)
            for index, node in enumerate(tree.body):
                with self.subTest(file=filename, line=node.lineno):
                    if index == 0:
                        self.assertIsInstance(node, ast.Expr)
                        self.assertIsInstance(node.value, ast.Constant)
                    elif isinstance(node, ast.FunctionDef):
                        self.assertEqual(node.decorator_list, [])
                    else:
                        self.assertIsInstance(node, ast.Assign)
                        self.assertIn(node.targets[0].id, ("__all__", "__version__"))


if __name__ == "__main__":
    unittest.main()
