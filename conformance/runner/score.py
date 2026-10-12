# provenance: original
"""Score candidate records against golden records on three levels (CONF-03).

Both inputs are records.jsonl files of conformance/oracles/run_programs.py, the one executor (D-01): one JSON
object per case with case_id, status ("ok" | "error" | "timeout"), stdout, stdout_truncated, and exception
({type, message} or null).

Levels, each judged per golden case:
- level 3, error class equal: the same status and, when the status is error, the same exception type name;
- level 2, final answer equal: level 3, and the same final answer. The final answer is the last non-empty line of
  stdout with surrounding whitespace removed, or "" when there is none (research assumption A3);
- level 1, stdout byte-equal: level 3, and the same stdout and stdout_truncated.
So level 1 implies level 2, which implies level 3: an output match never counts when the outcome differs.

For a golden error case, `error_message_equal` also reports whether the candidate raised the same type with the same
message. ImportError messages embed the module file path ("... from 'sympy' (/path/__init__.py)"), so for that type
one trailing parenthesised group is removed from both sides first (normalise_import_message); every other message is
compared verbatim.

`level1_message_equal` is a stricter count beside the three levels, which it does not change: level 1 and, for a
golden error case, `error_message_equal`. For every other case it equals level 1. It keeps a candidate that raises
the golden's exception type with other text (for example the stand-in's unavailable-name NotImplementedError against
SymPy's own) from counting as a match.

No case is dropped. A golden case with no candidate record fails all three levels and counts in `missing`. Excluded
case_ids (the golden manifest's adjudication list) are not scored and count in `excluded`. A candidate record for a
case that is neither golden nor excluded is not scored and counts in `extra`.

Outputs: the aggregate (counts, and the share of the scored cases for each level and for level1_message_equal, as a
string with one decimal) encoded with the stand-in's JCS mirror `sympy._wire.dumps`, the one Python encoder; and
per-case results as JSON Lines in case_id order, each line JCS. Neither output holds stdout, messages or code. The input order never changes the bytes.

Standard library plus the stand-in mirror. Run with PYTHONPATH=standin:conformance:
  python conformance/runner/score.py --golden G --candidate C [--exclude FILE] --out AGG --cases-out CASES
"""

import argparse
import json
import re
import sys

from sympy import _wire

STATUSES = ("ok", "error", "timeout")
_TRAILING_PAREN = re.compile(r"\s\([^()]*\)$")


def normalise_import_message(message):
    """`message` without one trailing ` (<...>)` group: the module path CPython appends to ImportError text."""
    return _TRAILING_PAREN.sub("", message, count=1)


def final_answer(stdout):
    """The last non-empty line of `stdout`, whitespace-trimmed, or "" when every line is blank."""
    for line in reversed(stdout.split("\n")):
        text = line.strip()
        if text:
            return text
    return ""


def _message(record):
    exception = record["exception"]
    if exception["type"] == "ImportError":
        return normalise_import_message(exception["message"])
    return exception["message"]


def score_case(golden, candidate):
    """The levels of one golden record against its candidate record (None when the candidate is missing)."""
    error = golden["status"] == "error"
    if candidate is None:
        return {
            "level1": False,
            "level2": False,
            "level3": False,
            "error_message_equal": False if error else None,
            "level1_message_equal": False,
        }
    level3 = golden["status"] == candidate["status"] and (
        not error or golden["exception"]["type"] == candidate["exception"]["type"]
    )
    level2 = level3 and final_answer(golden["stdout"]) == final_answer(candidate["stdout"])
    level1 = (
        level3
        and golden["stdout"] == candidate["stdout"]
        and golden["stdout_truncated"] == candidate["stdout_truncated"]
    )
    message_equal = (level3 and _message(golden) == _message(candidate)) if error else None
    return {
        "level1": level1,
        "level2": level2,
        "level3": level3,
        "error_message_equal": message_equal,
        "level1_message_equal": level1 and message_equal is not False,
    }


def _check(record, what):
    """`record` if it has the run_programs.py record shape; else ValueError."""
    if not isinstance(record, dict):
        raise ValueError(f"{what}: a record must be an object")
    case_id = record.get("case_id")
    if not isinstance(case_id, str) or not case_id:
        raise ValueError(f"{what}: no case_id")
    status = record.get("status")
    if status not in STATUSES:
        raise ValueError(f"{what} {case_id}: unknown status {status!r}")
    if not isinstance(record.get("stdout"), str) or not isinstance(record.get("stdout_truncated"), bool):
        raise ValueError(f"{what} {case_id}: stdout must be a string and stdout_truncated a bool")
    exception = record.get("exception")
    if status == "error":
        if not (
            isinstance(exception, dict)
            and isinstance(exception.get("type"), str)
            and isinstance(exception.get("message"), str)
        ):
            raise ValueError(f"{what} {case_id}: an error record needs exception {{type, message}}")
    elif exception is not None:
        raise ValueError(f"{what} {case_id}: a {status} record has an exception")
    return record


def _by_case(records, what):
    out = {}
    for record in records:
        record = _check(record, what)
        if record["case_id"] in out:
            raise ValueError(f"{what}: case_id {record['case_id']} appears twice")
        out[record["case_id"]] = record
    return out


def percent(count, total):
    """`count / total` as a percentage string with one decimal, rounded half up; "0.0" when total is 0."""
    if total == 0:
        return "0.0"
    tenths = (2000 * count + total) // (2 * total)
    return f"{tenths // 10}.{tenths % 10}"


def score(golden, candidate, exclude=()):
    """The aggregate and the per-case results (in case_id order) of `candidate` against `golden`."""
    golden = _by_case(golden, "golden")
    candidate = _by_case(candidate, "candidate")
    excluded = set(exclude)
    cases = []
    counts = {"level1": 0, "level2": 0, "level3": 0, "level1_message_equal": 0}
    error_cases = 0
    error_message_equal = 0
    missing = 0
    for case_id in sorted(golden):
        if case_id in excluded:
            continue
        other = candidate.get(case_id)
        result = score_case(golden[case_id], other)
        for level in counts:
            counts[level] += result[level]
        if result["error_message_equal"] is not None:
            error_cases += 1
            error_message_equal += result["error_message_equal"]
        missing += other is None
        cases.append({"case_id": case_id, "missing": other is None, **result})
    scored = len(cases)
    aggregate = {
        "scored": scored,
        "excluded": len(excluded),
        "missing": missing,
        "extra": sum(1 for c in candidate if c not in golden and c not in excluded),
        "error_cases": error_cases,
        "error_message_equal": error_message_equal,
    }
    for level, count in counts.items():
        aggregate[level] = count
        aggregate[level + "_percent"] = percent(count, scored)
    return aggregate, cases


def encode(aggregate, cases):
    """The JCS bytes of the aggregate, and the per-case JSON Lines (one JCS object and "\\n" per case)."""
    return _wire.dumps(aggregate), b"".join(_wire.dumps(case) + b"\n" for case in cases)


def _read_jsonl(path):
    with open(path, encoding="utf-8") as f:
        return [json.loads(line) for line in f if line.strip()]


def _read_exclude(path):
    if path is None:
        return []
    with open(path, encoding="utf-8") as f:
        return [line.strip() for line in f if line.strip()]


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--golden", required=True, help="golden records.jsonl")
    parser.add_argument("--candidate", required=True, help="candidate records.jsonl")
    parser.add_argument("--exclude", help="file of case_ids not to score, one per line")
    parser.add_argument("--out", required=True, help="the aggregate (JCS)")
    parser.add_argument("--cases-out", required=True, help="per-case results (JSON Lines)")
    args = parser.parse_args(argv)
    try:
        aggregate, cases = score(
            _read_jsonl(args.golden), _read_jsonl(args.candidate), _read_exclude(args.exclude)
        )
    except ValueError as e:
        print(f"score: {e}", file=sys.stderr)
        return 2
    aggregate_bytes, cases_bytes = encode(aggregate, cases)
    with open(args.out, "wb") as f:
        f.write(aggregate_bytes)
    with open(args.cases_out, "wb") as f:
        f.write(cases_bytes)
    print(
        "score: scored={scored} excluded={excluded} missing={missing} extra={extra} "
        "level1={level1_percent}% level2={level2_percent}% level3={level3_percent}% "
        "level1_message_equal={level1_message_equal_percent}% "
        "error_message_equal={error_message_equal}/{error_cases}".format(**aggregate)
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
