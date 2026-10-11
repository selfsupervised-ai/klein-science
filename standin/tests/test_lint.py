# provenance: original
"""The Monty-subset lint (GUEST-02), with Monty's own parser as an independent oracle.

Each rule has a fixture that must fail with that rule. The stand-in itself must pass. pydantic-monty 1.1.0 (the
hash-locked dev venv, `cargo xtask tools venv`) must accept every stand-in file at parse time and reject the
inheritance fixture, so the lint and Monty agree. Monty has no parse-only entry point: a fed snippet is parsed in full
before any of it runs, and a construct outside Monty's grammar is reported as MontySyntaxError or as a
NotImplementedError from "the monty syntax parser", with no statement executed.

Run: .venv/bin/python -m unittest discover -s standin -t standin -p 'test_lint.py' -v
pydantic-monty missing is a failure, never a skip.
"""

import os
import re
import subprocess
import sys
import tempfile
import unittest

STANDIN = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
if STANDIN in sys.path:
    sys.path.remove(STANDIN)
sys.path.insert(0, STANDIN)

import lint  # noqa: E402

SYMPY_DIR = os.path.join(STANDIN, "sympy")
LINE = re.compile(r"^(?P<path>[^:]+):(?P<line>\d+):(?P<col>\d+): (?P<rule>R[1-9]) \S.*$")

# The fixtures for the rules Monty 1.1.0 rejects at parse time. R2 (it accepts module-level decorators), R8 and R9
# are stricter than Monty by design: they keep nondeterministic, I/O and dynamic-code paths out of the stand-in.
MONTY_PARSE_REJECTED = {
    "R1": "class A(B): pass\n",
    "R3": "def g():\n    yield 1\n",
    "R4": "match 1:\n    case 1:\n        pass\n",
    "R5": "x = 1\ndel x\n",
    "R6": "async def f(y):\n    async for x in y:\n        pass\n",
    "R7": "try:\n    pass\nexcept* ValueError:\n    pass\n",
}


def rules(source):
    return [violation.rule for violation in lint.check(source, "fixture.py")]


def standin_files():
    found = []
    for directory, subdirectories, filenames in os.walk(SYMPY_DIR):
        subdirectories[:] = sorted(d for d in subdirectories if d != "__pycache__")
        found.extend(os.path.join(directory, name) for name in sorted(filenames) if name.endswith(".py"))
    return found


class Rules(unittest.TestCase):
    def test_lint_clean(self):
        files = standin_files()
        self.assertGreaterEqual(len(files), 4)
        for path in files:
            with self.subTest(path=os.path.relpath(path, STANDIN)):
                with open(path, encoding="utf-8") as handle:
                    self.assertEqual([str(v) for v in lint.check(handle.read(), path)], [])

    def test_lint_R1_inheritance(self):
        self.assertEqual(rules("class A(B): pass\n"), ["R1"])
        self.assertEqual(rules("class A(metaclass=M): pass\n"), ["R1"])
        self.assertEqual(rules("class A:\n    pass\n"), [])

    def test_lint_R2_decorator(self):
        self.assertEqual(rules("@f\ndef g(): pass\n"), ["R2"])
        self.assertEqual(rules("@f\nclass A:\n    pass\n"), ["R2"])
        self.assertEqual(rules("def g(): pass\n"), [])

    def test_lint_R3_yield(self):
        self.assertEqual(rules("def g():\n    yield 1\n"), ["R3"])
        self.assertEqual(rules("def g():\n    yield from h()\n"), ["R3"])

    def test_lint_R4_match(self):
        self.assertEqual(rules(MONTY_PARSE_REJECTED["R4"]), ["R4"])

    def test_lint_R5_del(self):
        self.assertEqual(rules(MONTY_PARSE_REJECTED["R5"]), ["R5"])

    def test_lint_R6_async(self):
        self.assertEqual(rules(MONTY_PARSE_REJECTED["R6"]), ["R6"])
        self.assertEqual(rules("async def f(y):\n    async with y:\n        pass\n"), ["R6"])

    def test_lint_R7_except_star(self):
        self.assertEqual(rules(MONTY_PARSE_REJECTED["R7"]), ["R7"])
        self.assertEqual(rules("try:\n    pass\nexcept ValueError:\n    pass\n"), [])

    def test_lint_R8_imports(self):
        for source in (
            "import subprocess\n",
            "import time\n",
            "import random\n",
            "import os\n",
            "import os.path\n",
            "from os import path\n",
            "import pathlib\n",
            "import datetime\n",
            "import json.decoder\n",
            "from __future__ import annotations\n",
        ):
            with self.subTest(source=source):
                self.assertEqual(rules(source), ["R8"])
        for source in (
            "import json\n",
            "import math, re\n",
            "from sympy import _wire\n",
            "import sympy.abc\n",
            "from sympy.abc import x\n",
            "from . import _wire\n",
        ):
            with self.subTest(source=source):
                self.assertEqual(rules(source), [])

    def test_lint_R9_dynamic(self):
        for source in ('eval("1")\n', 'exec("1")\n', 'compile("1", "x", "exec")\n', '__import__("os")\n', 'open("f")\n'):
            with self.subTest(source=source):
                self.assertEqual(rules(source), ["R9"])
        # A reference that is not a call is the same capability under another name.
        self.assertEqual(rules("run = eval\n"), ["R9"])

    def test_violation_position_and_format(self):
        violations = lint.check("x = 1\n\nclass A(B):\n    pass\n", "pkg/mod.py")
        self.assertEqual(len(violations), 1)
        self.assertEqual((violations[0].line, violations[0].col), (3, 1))
        self.assertRegex(str(violations[0]), LINE)
        self.assertTrue(str(violations[0]).startswith("pkg/mod.py:3:1: R1 "))


class CommandLine(unittest.TestCase):
    def run_lint(self, *paths):
        env = dict(os.environ, PYTHONPATH=STANDIN, PYTHONDONTWRITEBYTECODE="1")
        return subprocess.run(
            [sys.executable, "-m", "lint", *paths], capture_output=True, text=True, env=env, check=False
        )

    def test_lint_sorted_output(self):
        with tempfile.TemporaryDirectory() as tmp:
            with open(os.path.join(tmp, "b.py"), "w", encoding="utf-8") as handle:
                handle.write("import os\n\nclass A(B):\n    pass\nx = 1\ndel x\n")
            with open(os.path.join(tmp, "a.py"), "w", encoding="utf-8") as handle:
                handle.write("x = 1\n\n\ndef g():\n    yield x\n")
            result = self.run_lint(tmp)
            self.assertEqual(result.returncode, 1, result.stderr)
            lines = result.stdout.splitlines()
            found = []
            for line in lines:
                match = LINE.match(line)
                self.assertIsNotNone(match, line)
                found.append((os.path.basename(match["path"]), int(match["line"]), match["rule"]))
            self.assertEqual(found, [("a.py", 5, "R3"), ("b.py", 1, "R8"), ("b.py", 3, "R1"), ("b.py", 6, "R5")])

    def test_lint_clean_exit(self):
        result = self.run_lint(SYMPY_DIR)
        self.assertEqual((result.returncode, result.stdout), (0, ""), result.stderr)

    def test_lint_red_witness_inheritance(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = os.path.join(tmp, "witness.py")
            with open(path, "w", encoding="utf-8") as handle:
                handle.write("class A(B): pass\n")
            result = self.run_lint(path)
            self.assertEqual(result.returncode, 1)
            self.assertEqual(result.stdout.splitlines()[0].split(": ")[1].split(" ")[0], "R1")

    def test_missing_path_is_usage_error(self):
        self.assertEqual(self.run_lint().returncode, 2)
        self.assertEqual(self.run_lint(os.path.join(STANDIN, "no-such-dir")).returncode, 2)


class MontyAgrees(unittest.TestCase):
    """pydantic-monty 1.1.0 as the independent oracle for the subset (T-01-33)."""

    @classmethod
    def setUpClass(cls):
        import pydantic_monty

        cls.monty = pydantic_monty
        cls.pool = pydantic_monty.Monty()
        cls.pool.__enter__()

    @classmethod
    def tearDownClass(cls):
        cls.pool.__exit__(None, None, None)

    def outcome(self, source):
        """("ok" | "parse_rejected" | "runtime_error", detail) for one fed snippet in a fresh session."""
        monty = self.monty
        with self.pool.checkout() as session:
            try:
                session.feed_run(source, print_callback=monty.CollectString())
            except monty.MontySyntaxError as error:
                return ("parse_rejected", error.display("type-msg"))
            except monty.MontyRuntimeError as error:
                detail = error.display("type-msg")
                inner = error.exception()
                if isinstance(inner, NotImplementedError) and "monty syntax parser" in str(inner):
                    return ("parse_rejected", detail)
                return ("runtime_error", detail)
        return ("ok", "")

    def test_monty_agrees(self):
        # Every stand-in file parses under Monty. A runtime error is allowed: Monty 1.1.0 cannot import the
        # stand-in's own submodules (no user-module import; D4 evidence).
        for path in standin_files():
            with open(path, encoding="utf-8") as handle:
                source = handle.read()
            with self.subTest(path=os.path.relpath(path, STANDIN)):
                kind, detail = self.outcome(source)
                self.assertNotEqual(kind, "parse_rejected", detail)
        # Monty rejects the inheritance fixture, and every other fixture whose rule mirrors a Monty parser limit.
        for rule, source in MONTY_PARSE_REJECTED.items():
            with self.subTest(rule=rule):
                self.assertEqual(rules(source), [rule])
                kind, detail = self.outcome(source)
                self.assertEqual(kind, "parse_rejected", detail)
        # And the agreement is not vacuous: Monty runs a lint-clean snippet.
        self.assertEqual(self.outcome("def f(*args, **kwargs):\n    return 1\n__all__ = ['f']\n")[0], "ok")


if __name__ == "__main__":
    unittest.main()
