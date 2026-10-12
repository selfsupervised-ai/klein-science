# provenance: original
"""runner.score: the three-level scorer (CONF-03).

Run: env PYTHONPATH=standin python3 -m unittest discover -s conformance -t conformance
"""

import json
import os
import random
import subprocess
import sys
import tempfile
import unittest

from runner import score

CONFORMANCE = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
SCORE_PY = os.path.join(CONFORMANCE, "runner", "score.py")


def rec(case_id, status="ok", stdout="", exc_type=None, message="m", truncated=False):
    """A run_programs.py record."""
    exception = None if exc_type is None else {"type": exc_type, "message": message}
    return {
        "case_id": case_id,
        "status": status,
        "stdout": stdout,
        "stdout_truncated": truncated,
        "exception": exception,
    }


def levels(golden, candidate):
    r = score.score_case(golden, candidate)
    return (r["level1"], r["level2"], r["level3"])


class Levels(unittest.TestCase):
    def test_score_levels(self):
        g = rec("a", stdout="a\n5\n")
        self.assertEqual(levels(g, rec("a", stdout="a\n5\n")), (True, True, True))
        # Adjacency: only the trailing newline differs.
        self.assertEqual(levels(g, rec("a", stdout="a\n5")), (False, True, True))
        self.assertEqual(levels(g, rec("a", stdout="b\n5\n")), (False, True, True))
        self.assertEqual(
            levels(g, rec("a", "error", "a\n5\n", "ValueError")), (False, False, False)
        )

    def test_score_truncation_is_part_of_level1(self):
        g = rec("a", stdout="x\n")
        self.assertEqual(levels(g, rec("a", stdout="x\n", truncated=True)), (False, True, True))

    def test_score_error_class(self):
        g = rec("a", "error", exc_type="ImportError", message="cannot import name 'x'")
        other = rec("a", "error", exc_type="ImportError", message="something else")
        self.assertTrue(score.score_case(g, other)["level3"])
        self.assertFalse(score.score_case(g, other)["error_message_equal"])
        self.assertFalse(score.score_case(g, rec("a", "error", exc_type="ValueError"))["level3"])
        self.assertFalse(score.score_case(g, rec("a", "ok"))["level3"])
        t = rec("a", "timeout")
        self.assertEqual(levels(t, rec("a", "timeout")), (True, True, True))
        self.assertEqual(levels(t, rec("a", "ok")), (False, False, False))
        # Error message equality is reported for error goldens only.
        self.assertIsNone(score.score_case(rec("a"), rec("a"))["error_message_equal"])

    def test_score_empty(self):
        self.assertEqual(levels(rec("a", stdout=""), rec("a", stdout="")), (True, True, True))
        self.assertEqual(score.final_answer(""), "")
        self.assertEqual(score.final_answer("\n  \n"), "")
        self.assertEqual(score.final_answer("a\n  5  \n\n"), "5")
        self.assertEqual(levels(rec("a", stdout=""), rec("a", stdout="x")), (False, False, True))

    def test_score_missing(self):
        golden = [rec("a", stdout="1\n"), rec("b", "error", exc_type="ValueError")]
        aggregate, cases = score.score(golden, [rec("a", stdout="1\n")])
        self.assertEqual(aggregate["missing"], 1)
        self.assertEqual(aggregate["scored"], 2)
        self.assertEqual((aggregate["level1"], aggregate["level2"], aggregate["level3"]), (1, 1, 1))
        missing = [c for c in cases if c["case_id"] == "b"][0]
        self.assertTrue(missing["missing"])
        self.assertEqual(
            (missing["level1"], missing["level2"], missing["level3"]), (False, False, False)
        )
        self.assertFalse(missing["error_message_equal"])

    def test_score_excluded(self):
        golden = [rec("a", stdout="1\n"), rec("b", stdout="2\n")]
        candidate = [rec("a", stdout="1\n"), rec("b", stdout="wrong\n"), rec("c", stdout="3\n")]
        aggregate, cases = score.score(golden, candidate, exclude=["b", "c"])
        self.assertEqual((aggregate["scored"], aggregate["excluded"]), (1, 2))
        self.assertEqual((aggregate["missing"], aggregate["extra"]), (0, 0))
        self.assertEqual(aggregate["level1"], 1)
        self.assertEqual(aggregate["level1_percent"], "100.0")
        self.assertEqual([c["case_id"] for c in cases], ["a"])
        # A candidate for a case that is neither golden nor excluded is reported, not scored.
        aggregate, _ = score.score(golden, candidate, exclude=["b"])
        self.assertEqual(aggregate["extra"], 1)

    def test_score_order(self):
        golden = [
            rec("%02d" % i, ["ok", "error", "timeout"][i % 3], "out %d\n" % i,
                "ValueError" if i % 3 == 1 else None)
            for i in range(30)
        ]
        candidate = [dict(r, stdout=r["stdout"] if i % 4 else "other\n") for i, r in enumerate(golden)]
        reference = score.encode(*score.score(golden, candidate, exclude=["03"]))
        rng = random.Random(7)
        for _ in range(5):
            g, c = list(golden), list(candidate)
            rng.shuffle(g)
            rng.shuffle(c)
            self.assertEqual(score.encode(*score.score(g, c, exclude=["03"])), reference)
        _, cases = score.score(list(reversed(golden)), candidate)
        self.assertEqual([c["case_id"] for c in cases], sorted(c["case_id"] for c in cases))

    def test_score_rejects_duplicate_and_malformed_records(self):
        with self.assertRaises(ValueError):
            score.score([rec("a"), rec("a")], [])
        with self.assertRaises(ValueError):
            score.score([rec("a", "skipped")], [])
        with self.assertRaises(ValueError):
            score.score([rec("a", "error")], [])

    def test_percent_has_one_decimal(self):
        self.assertEqual(score.percent(0, 0), "0.0")
        self.assertEqual(score.percent(1, 3), "33.3")
        self.assertEqual(score.percent(2, 3), "66.7")
        self.assertEqual(score.percent(1, 8), "12.5")
        self.assertEqual(score.percent(9, 9), "100.0")


class ImportMessage(unittest.TestCase):
    def test_normalise_import_message(self):
        self.assertEqual(
            score.normalise_import_message("cannot import name 'binom' from 'sympy' (/a/b/__init__.py)"),
            "cannot import name 'binom' from 'sympy'",
        )
        self.assertEqual(
            score.normalise_import_message("cannot import name 'binom' from 'sympy'"),
            "cannot import name 'binom' from 'sympy'",
        )
        # Only one trailing parenthesis goes, and nothing else.
        self.assertEqual(score.normalise_import_message("a (b) (c)"), "a (b)")
        self.assertEqual(score.normalise_import_message("a (b) c"), "a (b) c")

    def test_import_paths_do_not_affect_message_equality(self):
        g = rec("a", "error", exc_type="ImportError",
                message="cannot import name 'binom' from 'sympy' (/usr/lib/sympy/__init__.py)")
        c = rec("a", "error", exc_type="ImportError",
                message="cannot import name 'binom' from 'sympy' (/opt/ksci/standin/sympy/__init__.py)")
        self.assertTrue(score.score_case(g, c)["error_message_equal"])
        # Other exception types are compared verbatim.
        g = rec("a", "error", exc_type="ValueError", message="bad (1)")
        c = rec("a", "error", exc_type="ValueError", message="bad (2)")
        self.assertFalse(score.score_case(g, c)["error_message_equal"])


class Command(unittest.TestCase):
    def test_cli_writes_jcs_aggregate_and_sorted_cases(self):
        with tempfile.TemporaryDirectory() as d:
            def write(name, records):
                path = os.path.join(d, name)
                with open(path, "w", encoding="utf-8") as f:
                    for r in records:
                        f.write(json.dumps(r) + "\n")
                return path

            golden = write("g.jsonl", [rec("b", stdout="2\n"), rec("a", stdout="1\n")])
            candidate = write("c.jsonl", [rec("a", stdout="1\n")])
            exclude = os.path.join(d, "exclude.txt")
            with open(exclude, "w", encoding="utf-8") as f:
                f.write("z\n")
            out, cases_out = os.path.join(d, "agg.json"), os.path.join(d, "cases.jsonl")
            env = dict(os.environ)
            env["PYTHONPATH"] = os.pathsep.join(
                [os.path.join(os.path.dirname(CONFORMANCE), "standin"), CONFORMANCE]
            )
            subprocess.run(
                [sys.executable, SCORE_PY, "--golden", golden, "--candidate", candidate,
                 "--exclude", exclude, "--out", out, "--cases-out", cases_out],
                check=True, env=env, stdout=subprocess.DEVNULL,
            )
            with open(out, "rb") as f:
                aggregate = f.read()
            parsed = json.loads(aggregate)
            self.assertEqual(aggregate, score.encode(parsed, [])[0])
            self.assertEqual((parsed["scored"], parsed["excluded"], parsed["missing"]), (2, 1, 1))
            with open(cases_out, encoding="utf-8") as f:
                ids = [json.loads(line)["case_id"] for line in f]
            self.assertEqual(ids, ["a", "b"])
            self.assertNotIn(b'"stdout"', aggregate)


if __name__ == "__main__":
    unittest.main()
