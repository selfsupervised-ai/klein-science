# provenance: original
"""`python -m lint <paths...>`: lint every .py file under the paths against the Monty subset.

Violations print one per line as `path:line:col: Rn message`, sorted by path, then line, then column. The exit
status is 0 when there are none, 1 when there are some, and 2 when a path does not exist or none is given.
Run with the stand-in directory on the path: `PYTHONPATH=standin python -m lint standin/sympy`.
"""

import os
import sys

from lint import check


def python_files(path):
    if os.path.isfile(path):
        return [path]
    found = []
    for directory, subdirectories, filenames in os.walk(path):
        subdirectories[:] = sorted(d for d in subdirectories if d != "__pycache__")
        found.extend(os.path.join(directory, name) for name in sorted(filenames) if name.endswith(".py"))
    return found


def main(argv):
    if not argv:
        print("usage: python -m lint <paths...>", file=sys.stderr)
        return 2
    files = []
    for path in argv:
        if not os.path.exists(path):
            print("lint: " + path + ": no such file or directory", file=sys.stderr)
            return 2
        files.extend(python_files(path))
    violations = []
    for path in files:
        with open(path, encoding="utf-8") as handle:
            violations.extend(check(handle.read(), path))
    for violation in sorted(violations):
        print(violation)
    print("lint: " + str(len(files)) + " files, " + str(len(violations)) + " violations", file=sys.stderr)
    return 1 if violations else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
