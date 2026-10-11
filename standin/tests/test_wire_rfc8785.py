# provenance: original
"""Three-way JCS differential: the Rust codec, the Python mirror and rfc8785 (DET-01).

rfc8785 0.1.4 (Trail of Bits) is a test oracle only (D-01): it is imported here and never by standin/sympy. The corpus
lines are the bytes ksci-wire accepted (test_wire.py sends each through the R2 worker), so mirror == rfc8785 == line
closes the triangle.

Run under the dev venv: cargo xtask tools venv && .venv/bin/python -m unittest discover -s standin -t standin -v
"""

import json
import os
import random
import sys
import unittest

import rfc8785

STANDIN = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
REPO = os.path.dirname(STANDIN)
if STANDIN in sys.path:
    sys.path.remove(STANDIN)
sys.path.insert(0, STANDIN)

import sympy  # noqa: E402
from sympy import _wire  # noqa: E402

if not os.path.abspath(sympy.__file__).startswith(STANDIN + os.sep):
    raise ImportError("sympy resolved to " + sympy.__file__ + ", not the stand-in under " + STANDIN)

CORPUS = os.path.join(REPO, "testdata", "wire", "requests.jsonl")

# A fixed seed, so a failure replays.
SEED = 0x0108


def corpus_lines():
    with open(CORPUS, "rb") as handle:
        lines = handle.read().split(b"\n")
    if lines[-1] != b"":
        raise AssertionError("requests.jsonl must end with a newline")
    return lines[:-1]


def random_scalar_string(rng):
    """A string of Unicode scalar values from every range JCS treats differently."""
    ranges = [(0x00, 0x1F), (0x20, 0x7F), (0x80, 0x7FF), (0x800, 0xD7FF), (0xE000, 0xFFFF), (0x10000, 0x10FFFF)]
    chars = []
    for _ in range(rng.randrange(0, 9)):
        low, high = ranges[rng.randrange(len(ranges))]
        chars.append(chr(rng.randint(low, high)))
    return "".join(chars)


def random_value(rng, depth):
    choice = rng.randrange(8 if depth < 4 else 5)
    if choice == 0:
        return None
    if choice == 1:
        return rng.random() < 0.5
    if choice == 2:
        return rng.randint(-(2**53 - 1), 2**53 - 1)
    if choice == 3:
        return rng.randint(-1000, 1000)
    if choice == 4:
        return random_scalar_string(rng)
    if choice == 5:
        return [random_value(rng, depth + 1) for _ in range(rng.randrange(0, 5))]
    return {random_scalar_string(rng): random_value(rng, depth + 1) for _ in range(rng.randrange(0, 5))}


class Rfc8785Test(unittest.TestCase):
    def assertThreeWay(self, value, expected=None):
        mirror = _wire.dumps(value)
        self.assertEqual(mirror, rfc8785.dumps(value))
        if expected is not None:
            self.assertEqual(mirror, expected)

    def test_rfc8785_corpus(self):
        lines = corpus_lines()
        self.assertGreaterEqual(len(lines), 18)
        for number, line in enumerate(lines, 1):
            with self.subTest(line=number):
                request = json.loads(line)
                self.assertThreeWay(request, line)
                # The canonical table the worker returns encodes the same way under both Python encoders.
                self.assertThreeWay(_wire.canonicalize(request["args"]["expr"]))

    def test_rfc8785_strings(self):
        strings = ["", "\u0000\u001f", "\u007f", " ", "\U0001d465", "é", "é", "\"\\/\b\f\n\r\t"]
        for text in strings:
            with self.subTest(text=ascii(text)):
                self.assertThreeWay(text)
                self.assertThreeWay([text])
        # Precomposed and decomposed stay distinct: no Unicode normalisation in either encoder.
        self.assertNotEqual(_wire.dumps("é"), _wire.dumps("é"))
        # Keys sort by UTF-16 code units: U+1D465 (D835 DC65) before U+FFFF, unlike code point order.
        keys = {"\U0001d465": 1, "￿": 2, "a": 3}
        self.assertThreeWay(keys, '{"a":3,"\U0001d465":1,"￿":2}'.encode("utf-8"))

    def test_rfc8785_containers(self):
        cases = [
            ({}, b"{}"),
            ([], b"[]"),
            ([[], [[]], [[], [[]]]], b"[[],[[]],[[],[[]]]]"),
            ([True, False, None], b"[true,false,null]"),
            (0, b"0"),
            (-1, b"-1"),
            (2**53 - 1, b"9007199254740991"),
            (-(2**53 - 1), b"-9007199254740991"),
            ({"b": {"d": [], "c": {}}, "a": None}, b'{"a":null,"b":{"c":{},"d":[]}}'),
        ]
        for value, expected in cases:
            with self.subTest(value=value):
                self.assertThreeWay(value, expected)

    def test_rfc8785_large_ints_both_reject(self):
        # Beyond +-(2**53 - 1) an integer has no exact IEEE-754 double. rfc8785 raises IntegerDomainError, the
        # mirror ValueError, and the Rust codec would round, so the wire never carries such a JSON number.
        for value in (2**200, -(2**200), 2**53, -(2**53)):
            with self.subTest(value=value):
                with self.assertRaises(rfc8785.IntegerDomainError):
                    rfc8785.dumps(value)
                with self.assertRaises(ValueError):
                    _wire.dumps(value)

    def test_rfc8785_surrogates_both_reject(self):
        for value in ("\ud800", ["a\udfff"], {"\udc00": 1}):
            with self.subTest(value=ascii(value)):
                with self.assertRaises(ValueError):
                    rfc8785.dumps(value)
                with self.assertRaises(ValueError):
                    _wire.dumps(value)

    def test_rfc8785_random_values(self):
        rng = random.Random(SEED)
        for index in range(2000):
            value = random_value(rng, 0)
            with self.subTest(index=index):
                self.assertThreeWay(value)


if __name__ == "__main__":
    unittest.main()
