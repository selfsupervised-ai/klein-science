# provenance: original
"""The one AST name scanner (D-01): which programs use SymPy, and which SymPy names they use.

Programs are parsed with `ast`, never executed. The golden slice (conformance/oracles/sympy/slice.py)
and the static census (plan 01-17) both use this module, so they count the same things.

A SymPy *name* (FEATURES.md "Measured Basis") is a top-level SymPy name the program imports or uses:
- every name in `from sympy import a, b` (as written, so a name SymPy does not have still counts);
- after `from sympy import *`, every Name the program references that is in `all_names`
  (`sympy.__all__`, from the introspection table), since that is what the star import bound;
- every attribute `X` of a name bound to the sympy module (`import sympy`, `import sympy as sp`,
  `import sympy.solvers`, which binds `sympy`) when `X` is in `all_names`;
- `abc:{name}` for `from sympy.abc import {name}`, and after `from sympy.abc import *` for every
  referenced Name in `abc_names`.
Methods such as `.subs` are not names. Relative imports never refer to SymPy.

Standard library only.
"""

import ast
import warnings


def parse(code):
    """The module AST of `code`, or None when it does not parse. Never raises for bad code.

    Compiler warnings (for example SyntaxWarning for an invalid escape sequence) are the
    program's, not the census's, so they are silenced rather than printed.
    """
    try:
        with warnings.catch_warnings():
            warnings.simplefilter("ignore")
            return ast.parse(code)
    except (SyntaxError, ValueError, RecursionError, MemoryError):
        return None


def _is_sympy(module):
    return module == "sympy" or module.startswith("sympy.")


def sympy_imports(tree):
    """The sympy module paths the program imports (`sympy`, `sympy.abc`, ...), anywhere in it."""
    found = set()
    for node in ast.walk(tree):
        if isinstance(node, ast.Import):
            found.update(alias.name for alias in node.names if _is_sympy(alias.name))
        elif isinstance(node, ast.ImportFrom):
            if node.level == 0 and node.module is not None and _is_sympy(node.module):
                found.add(node.module)
    return found


def sympy_names(tree, all_names, abc_names):
    """The SymPy names the program uses (see the module docstring)."""
    names = set()
    module_aliases = set()
    star = False
    abc_star = False
    for node in ast.walk(tree):
        if isinstance(node, ast.Import):
            for alias in node.names:
                if alias.name == "sympy":
                    module_aliases.add(alias.asname or "sympy")
                elif alias.name.startswith("sympy.") and alias.asname is None:
                    # `import sympy.solvers` binds the name `sympy` to the sympy module.
                    module_aliases.add("sympy")
        elif isinstance(node, ast.ImportFrom) and node.level == 0:
            if node.module == "sympy":
                for alias in node.names:
                    if alias.name == "*":
                        star = True
                    else:
                        names.add(alias.name)
            elif node.module == "sympy.abc":
                for alias in node.names:
                    if alias.name == "*":
                        abc_star = True
                    else:
                        names.add("abc:" + alias.name)
    for node in ast.walk(tree):
        if isinstance(node, ast.Attribute):
            value = node.value
            if (
                isinstance(value, ast.Name)
                and value.id in module_aliases
                and node.attr in all_names
            ):
                names.add(node.attr)
        elif isinstance(node, ast.Name):
            if star and node.id in all_names:
                names.add(node.id)
            if abc_star and node.id in abc_names:
                names.add("abc:" + node.id)
    return names
