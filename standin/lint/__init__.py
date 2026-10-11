# provenance: original
"""Monty-subset lint for the stand-in (GUEST-02).

The stand-in (`standin/sympy`) runs as guest code inside Klein's Monty sandbox, so it must stay inside the language
subset Monty parses and must not reach for nondeterminism, I/O or dynamic code. `check` walks a module's AST and
reports every construct outside that subset:

- R1: a class with bases, keywords or a metaclass (Monty has no inheritance);
- R2: a decorator on a function or class;
- R3: `yield` or `yield from`;
- R4: `match`;
- R5: `del`;
- R6: `async for`, `async with`, or an `async for` comprehension;
- R7: `except*`;
- R8: an import outside the allowed modules: Monty's module list without the nondeterministic or I/O modules, plus
  the stand-in's own package `sympy`, its submodules and relative imports;
- R9: a reference to `eval`, `exec`, `compile`, `__import__`, `open` or `__builtins__`. A reference, not only a call:
  `run = eval` is the same capability under another name.

R1 and R3 to R7 mirror Monty 1.1.0's parse-time rejections; `standin/tests/test_lint.py` checks them against Monty's
own parser. R2, R8 and R9 are stricter than Monty by design.

This is CPython-only tooling that lives outside the `sympy` package and is never shipped. Standard library only.
"""

import ast
import dataclasses

# Monty 1.1.0's module list: asyncio, base64, binascii, collections, copy, dataclasses, datetime, functools, hashlib,
# itertools, json, math, os, os.path, pathlib, posixpath, random, re, sys, time, typing, unicodedata. Removed here:
# time, random, os, os.path, pathlib, posixpath and datetime (clocks, randomness and file system access).
ALLOWED_MODULES = frozenset(
    {
        "asyncio",
        "base64",
        "binascii",
        "collections",
        "copy",
        "dataclasses",
        "functools",
        "hashlib",
        "itertools",
        "json",
        "math",
        "re",
        "sys",
        "typing",
        "unicodedata",
    }
)
OWN_PACKAGE = "sympy"
DYNAMIC_NAMES = frozenset({"eval", "exec", "compile", "__import__", "open", "__builtins__"})


@dataclasses.dataclass(frozen=True, order=True)
class Violation:
    """One construct outside the subset. Ordering is by path, then line, then column."""

    path: str
    line: int
    col: int
    rule: str
    message: str

    def __str__(self):
        return self.path + ":" + str(self.line) + ":" + str(self.col) + ": " + self.rule + " " + self.message


def module_allowed(name):
    """Whether an absolute import of the dotted module `name` is allowed (exact names; no prefix matching)."""
    return name in ALLOWED_MODULES or name == OWN_PACKAGE or name.startswith(OWN_PACKAGE + ".")


def check(source, path):
    """Every violation in `source` (the text of the module at `path`), sorted."""
    try:
        tree = ast.parse(source, path)
    except SyntaxError as error:
        return [Violation(path, error.lineno or 1, (error.offset or 0) + 1, "R0", "syntax error: " + str(error.msg))]
    found = []

    def report(node, rule, message):
        found.append(Violation(path, node.lineno, node.col_offset + 1, rule, message))

    for node in ast.walk(tree):
        if isinstance(node, ast.ClassDef) and (node.bases or node.keywords):
            report(node, "R1", "class " + node.name + " has bases or keywords: Monty has no inheritance or metaclasses")
        if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef, ast.ClassDef)):
            for decorator in node.decorator_list:
                report(decorator, "R2", "decorator on " + node.name + " is outside the Monty subset")
        if isinstance(node, (ast.Yield, ast.YieldFrom)):
            report(node, "R3", "yield is outside the Monty subset")
        if isinstance(node, ast.Match):
            report(node, "R4", "match is outside the Monty subset")
        if isinstance(node, ast.Delete):
            report(node, "R5", "del is outside the Monty subset")
        if isinstance(node, (ast.AsyncFor, ast.AsyncWith)):
            report(node, "R6", "async for and async with are outside the Monty subset")
        if isinstance(node, (ast.ListComp, ast.SetComp, ast.DictComp, ast.GeneratorExp)):
            if any(generator.is_async for generator in node.generators):
                report(node, "R6", "async comprehension is outside the Monty subset")
        if isinstance(node, ast.TryStar):
            report(node, "R7", "except* is outside the Monty subset")
        if isinstance(node, ast.Import):
            for alias in node.names:
                if not module_allowed(alias.name):
                    report(node, "R8", "import of " + alias.name + " is outside the allowed modules")
        if isinstance(node, ast.ImportFrom) and node.level == 0:
            if not module_allowed(node.module):
                report(node, "R8", "import from " + node.module + " is outside the allowed modules")
        if isinstance(node, ast.Name) and node.id in DYNAMIC_NAMES:
            report(node, "R9", node.id + " is dynamic code or I/O and is not allowed")
    return sorted(found)
