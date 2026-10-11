# provenance: original
"""Turn a pinned corpus parquet into programs.jsonl (runs inside the extraction image).

    extract.py --input <parquet> --out <dir> --format tir

Format `tir` (NuminaMath-TIR, conformance/corpora/tir.toml): the code of a row is every ```python
fenced block of its `solution` column, in order. A fence is a line whose stripped text is exactly
"```python" (opening) or "```" (closing); the block is the lines strictly between them, joined with
"\n". A block left open runs to the end of the solution. The row's program is its blocks joined
with "\n"; a row with no block has no program. The unit is the row, not the block (FEATURES.md
"Measured Basis").

case_id is the sha256 of the program's UTF-8 bytes. Programs are deduplicated by case_id, keeping
the lowest row index; every further row with the same program counts as a duplicate.

Outputs:
- <out>/programs.jsonl: one {"case_id", "code", "row"} per distinct program, sorted by case_id;
- <out>/extract.json: the format, the input's sha256, the pyarrow version and the counts rows,
  rows_with_code, distinct_programs and duplicates.

The parquet is untrusted third-party bytes (T-01-19). Before it is opened, this process gives up
every capability the launcher left the harness (it needs none), so pyarrow parses it as plain
uid 65534 with no capabilities, inside the network-less, read-only container.

Exit status: 0 on success, 2 on invalid arguments or input, 4 when the capabilities cannot be dropped.
"""

import argparse
import ctypes
import hashlib
import json
import os
import sys

LINUX_CAPABILITY_VERSION_3 = 0x20080522


class _CapHeader(ctypes.Structure):
    _fields_ = [("version", ctypes.c_uint32), ("pid", ctypes.c_int)]


class _CapData(ctypes.Structure):
    _fields_ = [
        ("effective", ctypes.c_uint32),
        ("permitted", ctypes.c_uint32),
        ("inheritable", ctypes.c_uint32),
    ]


def drop_capabilities():
    """Clears every capability set of this process and verifies it in /proc/self/status."""
    libc = ctypes.CDLL(None, use_errno=True)
    data = (_CapData * 2)()
    header = _CapHeader(LINUX_CAPABILITY_VERSION_3, 0)
    if libc.capset(ctypes.byref(header), data) != 0:
        print(f"extract: capset failed: errno {ctypes.get_errno()}", file=sys.stderr)
        sys.exit(4)
    with open("/proc/self/status") as f:
        caps = {k: v.split() for k, _, v in (line.partition(":") for line in f) if k.startswith("Cap")}
    for key in ("CapEff", "CapPrm", "CapInh", "CapAmb"):
        if caps.get(key) != ["0" * 16]:
            print(f"extract: after the drop {key} is {caps.get(key)}", file=sys.stderr)
            sys.exit(4)


def python_blocks(text):
    """The ```python fenced blocks of a markdown text, in order (see the module docstring)."""
    blocks = []
    current = None
    for line in text.split("\n"):
        if current is None:
            if line.strip() == "```python":
                current = []
        elif line.strip() == "```":
            blocks.append("\n".join(current))
            current = None
        else:
            current.append(line)
    if current is not None:
        blocks.append("\n".join(current))
    return blocks


def tir_program(solution):
    """The program of one TIR row, or None when its solution has no ```python block."""
    if solution is None:
        return None
    blocks = python_blocks(solution)
    if not blocks:
        return None
    return "\n".join(blocks)


def extract_tir(solutions):
    """(programs sorted by case_id, counts) from the `solution` values in row order."""
    first = {}
    rows_with_code = 0
    duplicates = 0
    for row, solution in enumerate(solutions):
        program = tir_program(solution)
        if program is None:
            continue
        rows_with_code += 1
        case_id = hashlib.sha256(program.encode("utf-8")).hexdigest()
        if case_id in first:
            duplicates += 1
        else:
            first[case_id] = (row, program)
    programs = [
        {"case_id": case_id, "code": program, "row": row}
        for case_id, (row, program) in sorted(first.items())
    ]
    counts = {
        "rows": len(solutions),
        "rows_with_code": rows_with_code,
        "distinct_programs": len(programs),
        "duplicates": duplicates,
    }
    return programs, counts


def file_sha256(path):
    digest = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--input", required=True)
    parser.add_argument("--out", required=True)
    parser.add_argument("--format", required=True, choices=["tir"])
    args = parser.parse_args()

    drop_capabilities()
    import pyarrow
    import pyarrow.parquet

    input_sha256 = file_sha256(args.input)
    table = pyarrow.parquet.read_table(args.input, columns=["solution"])
    column = table.column("solution")
    if not pyarrow.types.is_string(column.type) and not pyarrow.types.is_large_string(column.type):
        print(f"extract: column solution has type {column.type}, expected string", file=sys.stderr)
        sys.exit(2)
    programs, counts = extract_tir(column.to_pylist())

    with open(os.path.join(args.out, "programs.jsonl"), "w", encoding="utf-8") as f:
        for record in programs:
            f.write(json.dumps(record, ensure_ascii=False, sort_keys=True) + "\n")
    summary = {
        "format": args.format,
        "input_sha256": input_sha256,
        "pyarrow_version": pyarrow.__version__,
        **counts,
    }
    with open(os.path.join(args.out, "extract.json"), "w", encoding="utf-8") as f:
        f.write(json.dumps(summary, ensure_ascii=False, sort_keys=True) + "\n")
    print("extract: " + " ".join(f"{k}={v}" for k, v in counts.items()))


if __name__ == "__main__":
    main()
