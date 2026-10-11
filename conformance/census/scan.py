# provenance: original
"""The one AST name scanner (D-01): stub interface, implemented in the GREEN step."""

import ast


def parse(code):
    return ast.parse(code)


def sympy_imports(tree):
    return set()


def sympy_names(tree, all_names, abc_names):
    return set()
