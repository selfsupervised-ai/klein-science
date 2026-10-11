# provenance: original
"""Write the SymPy introspection table to <out>/names.json (runs inside the sympy oracle image).

The table lists every name in `sympy.__all__`, in that order, with the module it comes from and
its kind (function, class, module or instance), and the public `sympy.abc` names bound to a
Symbol. It also records the versions, the ground types and the mpmath backend. Plans 01-07
(strata) and 01-12 (the stand-in name table) consume it by digest.

`cargo xtask oracle introspect` runs this on linux/arm64 and linux/amd64 and requires the two
canonical outputs to be byte-identical, so nothing platform-specific may appear here.

Exit status: 0 on success, 3 when --require-sympy-env is given and the ground types or the
mpmath backend are not "python".
"""

import argparse
import inspect
import json
import os
import platform
import sys

# sympy/__init__.py assigns a 902-name `__all__` literal, then `__all__.extend(...)` adds 19
# subpackage names (algebras ... utilities). The runtime list, which `from sympy import *` binds,
# therefore has 921 names; the 19 are the ones of kind "module". Observed in this image.
EXPECTED_LITERAL = 902
EXPECTED_MODULES = 19
EXPECTED_ALL = EXPECTED_LITERAL + EXPECTED_MODULES


def kind_of(obj):
    if inspect.ismodule(obj):
        return "module"
    if inspect.isclass(obj):
        return "class"
    if inspect.isroutine(obj):
        return "function"
    return "instance"


def module_of(obj):
    module = inspect.getmodule(obj)
    if module is not None:
        return module.__name__
    return type(obj).__module__


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--out", required=True)
    parser.add_argument("--require-sympy-env", action="store_true")
    args = parser.parse_args()

    import mpmath
    import mpmath.libmp.backend
    import sympy
    import sympy.abc
    import sympy.external.gmpy

    ground_types = sympy.external.gmpy.GROUND_TYPES
    mpmath_backend = mpmath.libmp.backend.BACKEND
    if args.require_sympy_env and (ground_types != "python" or mpmath_backend != "python"):
        print(
            f"introspect: ground types {ground_types!r} and mpmath backend {mpmath_backend!r}, "
            "both must be 'python'",
            file=sys.stderr,
        )
        sys.exit(3)

    names = list(sympy.__all__)
    assert len(names) == EXPECTED_ALL, f"sympy.__all__ has {len(names)} names, expected {EXPECTED_ALL}"
    assert len(set(names)) == len(names), "sympy.__all__ repeats a name"
    table = []
    for name in names:
        obj = getattr(sympy, name)
        table.append({"name": name, "module": module_of(obj), "kind": kind_of(obj)})
    # The extended tail is all subpackages. (The literal list may name modules too: `evalf` is
    # bound to the module sympy.core.evalf at run time.)
    tail = table[EXPECTED_LITERAL:]
    assert all(row["kind"] == "module" for row in tail), f"non-module in the extended tail: {tail}"

    abc = sorted(
        name
        for name, value in vars(sympy.abc).items()
        if not name.startswith("_") and isinstance(value, sympy.Symbol)
    )

    result = {
        "sympy_version": sympy.__version__,
        "mpmath_version": mpmath.__version__,
        "python_version": platform.python_version(),
        "ground_types": ground_types,
        "mpmath_backend": mpmath_backend,
        "all": table,
        "abc": abc,
    }
    with open(os.path.join(args.out, "names.json"), "w", encoding="utf-8") as f:
        f.write(json.dumps(result, ensure_ascii=False, sort_keys=True) + "\n")


if __name__ == "__main__":
    main()
