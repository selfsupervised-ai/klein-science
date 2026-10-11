# provenance: original
"""Select the stratified golden slice of SymPy-using corpus programs, without an RNG (D-09).

    slice.py --input <programs.jsonl> --input <names.json> --out <dir>
             --parquet-sha256 <hex> [--n 1000] [--meta key=value]...

The two inputs come in that order: the extraction's programs.jsonl ({"case_id", "code", ...} per
line, case_id = sha256 of the code's UTF-8 bytes) and the SymPy introspection table (plan 01-05).
Runs in the base image with conformance/ on PYTHONPATH; standard library only.

Selection:
- seed = sha256(ascii(parquet sha256 hex) || b"golden-slice-v1"): fixed by the corpus digest.
- A program is SymPy-using when census.scan finds a sympy import. Unparseable programs are counted
  as syntax_errors and never selected.
- Each name the program uses maps to its module in the introspection table; the subpackage is the
  module's second dotted component ("abc:" names are "core"). The program's stratum is the first
  subpackage present in STRATUM_PRIORITY; a program whose names map to none of them is "other",
  and one with no names is "none".
- Allocation of n over the strata: every stratum gets at least min(population, FLOOR); the rest is
  proportional to population, apportioned by largest remainder (see allocate).
- Within a stratum, programs are ranked by sha256(seed || bytes.fromhex(case_id)) ascending and the
  first k are taken. Input order never matters.

Outputs:
- <out>/slice.jsonl: {"case_id", "code", "stratum"} per selected program, sorted by case_id;
- <out>/slice.json: the manifest, with no code: the seed, n, per-stratum population and selected
  counts, sympy_using, syntax_errors, and every --meta pair verbatim.

Exit status: 0 on success, 2 on invalid arguments or input.
"""

import argparse
import hashlib
import json
import os
import re
import sys
from fractions import Fraction

from census import scan

SLICE_TAG = b"golden-slice-v1"
SCHEMA = "ksci.golden-slice.v1"
STRATUM_PRIORITY = (
    "integrals",
    "series",
    "solvers",
    "matrices",
    "polys",
    "ntheory",
    "simplify",
    "calculus",
    "sets",
    "logic",
    "functions",
    "printing",
    "core",
)
FLOOR = 10


def derive_seed(parquet_sha256_hex):
    """The selection seed: sha256(ascii(parquet sha256 hex) || b"golden-slice-v1"), as bytes."""
    return hashlib.sha256(parquet_sha256_hex.encode("ascii") + SLICE_TAG).digest()


def rank_key(seed, case_id):
    return hashlib.sha256(seed + bytes.fromhex(case_id)).digest()


def subpackage_of(name, modules):
    if name.startswith("abc:"):
        return "core"
    module = modules.get(name)
    if module is None:
        return None
    parts = module.split(".")
    if len(parts) >= 2 and parts[0] == "sympy":
        return parts[1]
    return None


def stratum_of(names, modules):
    """The stratum of a program from its SymPy names and the name-to-module table."""
    present = {subpackage_of(name, modules) for name in names}
    for stratum in STRATUM_PRIORITY:
        if stratum in present:
            return stratum
    return "other" if names else "none"


def allocate(populations, n):
    """How many programs each stratum gives to a slice of n.

    Everything is taken when the populations total at most n. Otherwise a stratum whose
    proportional quota is below its floor min(population, FLOOR) is fixed at that floor, and the
    quotas of the rest are recomputed over what is left, until no quota is below its floor. The
    remaining strata get the integer part of their quota, and the leftover goes one each by largest
    remainder (ties: larger population, then name). Exact rational arithmetic throughout.
    """
    total = sum(populations.values())
    if total <= n:
        return {s: populations[s] for s in sorted(populations)}
    floors = {s: min(p, FLOOR) for s, p in populations.items()}
    fixed = {}
    while True:
        free = sorted(s for s in populations if s not in fixed)
        m = n - sum(fixed.values())
        if m < 0:
            raise ValueError(f"the per-stratum floors {sum(fixed.values())} exceed n={n}")
        p_free = sum(populations[s] for s in free)
        quotas = {s: Fraction(m * populations[s], p_free) if p_free else Fraction(0) for s in free}
        below = [s for s in free if quotas[s] < floors[s]]
        if not below:
            break
        for s in below:
            fixed[s] = floors[s]
    k = dict(fixed)
    for s in free:
        k[s] = quotas[s].numerator // quotas[s].denominator
    leftover = m - sum(k[s] for s in free)
    by_remainder = sorted(free, key=lambda s: (-(quotas[s] - k[s]), -populations[s], s))
    for s in by_remainder[:leftover]:
        k[s] += 1
    return {s: k[s] for s in sorted(populations)}


def select(programs, table, seed, n):
    """(selected records sorted by case_id, stats) for the programs and introspection table."""
    modules = {row["name"]: row["module"] for row in table["all"]}
    all_names = frozenset(modules)
    abc_names = frozenset(table["abc"])
    syntax_errors = 0
    by_stratum = {}
    for program in programs:
        tree = scan.parse(program["code"])
        if tree is None:
            syntax_errors += 1
            continue
        if not scan.sympy_imports(tree):
            continue
        names = scan.sympy_names(tree, all_names, abc_names)
        by_stratum.setdefault(stratum_of(names, modules), []).append(program)
    populations = {s: len(ps) for s, ps in by_stratum.items()}
    k = allocate(populations, n)
    selected = []
    for stratum, members in by_stratum.items():
        ranked = sorted(members, key=lambda p: (rank_key(seed, p["case_id"]), p["case_id"]))
        for program in ranked[: k[stratum]]:
            selected.append(
                {"case_id": program["case_id"], "code": program["code"], "stratum": stratum}
            )
    selected.sort(key=lambda r: r["case_id"])
    stats = {
        "strata": {
            s: {"population": populations[s], "selected": k[s]} for s in sorted(populations)
        },
        "sympy_using": sum(populations.values()),
        "syntax_errors": syntax_errors,
    }
    return selected, stats


def fail(message):
    print("slice: " + message, file=sys.stderr)
    sys.exit(2)


def read_programs(path):
    programs = []
    seen = set()
    with open(path, encoding="utf-8") as f:
        for number, line in enumerate(f, 1):
            record = json.loads(line)
            case_id, code = record.get("case_id"), record.get("code")
            if not isinstance(code, str) or not isinstance(case_id, str):
                fail(f"programs line {number}: case_id and code must be strings")
            if hashlib.sha256(code.encode("utf-8")).hexdigest() != case_id:
                fail(f"programs line {number}: case_id is not the sha256 of the code")
            if case_id in seen:
                fail(f"programs line {number}: case_id {case_id} repeats")
            seen.add(case_id)
            programs.append({"case_id": case_id, "code": code})
    return programs


def read_table(path):
    with open(path, encoding="utf-8") as f:
        table = json.load(f)
    if not isinstance(table, dict) or not isinstance(table.get("all"), list) or not isinstance(
        table.get("abc"), list
    ):
        fail("the second input is not an introspection table (names.json)")
    return table


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--input", action="append", default=[])
    parser.add_argument("--out", required=True)
    parser.add_argument("--parquet-sha256", required=True)
    parser.add_argument("--n", type=int, default=1000)
    parser.add_argument("--meta", action="append", default=[])
    args = parser.parse_args()

    if len(args.input) != 2:
        fail("expected --input <programs.jsonl> --input <names.json>, in that order")
    if not re.fullmatch(r"[0-9a-f]{64}", args.parquet_sha256):
        fail("--parquet-sha256 must be 64 lowercase hex digits")
    if args.n <= 0:
        fail("--n must be positive")

    programs = read_programs(args.input[0])
    table = read_table(args.input[1])
    seed = derive_seed(args.parquet_sha256)
    selected, stats = select(programs, table, seed, args.n)

    manifest = {
        "schema": SCHEMA,
        "parquet_sha256": args.parquet_sha256,
        "seed": seed.hex(),
        "n": args.n,
        "stratum_priority": list(STRATUM_PRIORITY),
        "floor": FLOOR,
        "programs": len(programs),
        "selected": len(selected),
        **stats,
    }
    for pair in args.meta:
        key, sep, value = pair.partition("=")
        if not sep or not re.fullmatch(r"[a-z][a-z0-9_]*", key):
            fail(f"--meta must be key=value with a lowercase key: {pair!r}")
        if key in manifest:
            fail(f"--meta key {key!r} is already in the manifest")
        manifest[key] = value

    with open(os.path.join(args.out, "slice.jsonl"), "w", encoding="utf-8") as f:
        for record in selected:
            f.write(json.dumps(record, ensure_ascii=False, sort_keys=True) + "\n")
    with open(os.path.join(args.out, "slice.json"), "w", encoding="utf-8") as f:
        f.write(json.dumps(manifest, ensure_ascii=False, sort_keys=True) + "\n")
    print(
        f"slice: programs={len(programs)} sympy_using={stats['sympy_using']} "
        f"syntax_errors={stats['syntax_errors']} selected={len(selected)}"
    )
    for stratum, counts in stats["strata"].items():
        print(f"slice: {stratum} population={counts['population']} selected={counts['selected']}")


if __name__ == "__main__":
    main()
