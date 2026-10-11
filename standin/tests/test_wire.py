# provenance: original
"""The Python mirror against the Rust R2 worker (DET-01, GUEST-02).

Every line of testdata/wire/requests.jsonl goes through `sympy._tool.call`, the Python JCS mirror and `host_shim` to
`ksci-worker` and back. The request bytes `call` builds must be the corpus line, the mirror must re-encode the worker's
response to the exact bytes it sent, and an exact result must be the mirror's own canonical table with the mirror's
own identity.

Run: python3 -m unittest discover -s standin -t standin -p 'test_wire.py' -v
The worker is $KSCI_WORKER, or <repo>/target/release/ksci-worker. A missing worker fails; it is never skipped.
"""

import hashlib
import importlib
import json
import os
import sys
import unittest

STANDIN = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
REPO = os.path.dirname(STANDIN)
if STANDIN in sys.path:
    sys.path.remove(STANDIN)
sys.path.insert(0, STANDIN)

import sympy  # noqa: E402
from sympy import _tool, _wire  # noqa: E402

import host_shim  # noqa: E402

if not os.path.abspath(sympy.__file__).startswith(STANDIN + os.sep):
    raise ImportError("sympy resolved to " + sympy.__file__ + ", not the stand-in under " + STANDIN)

CORPUS = os.path.join(REPO, "testdata", "wire", "requests.jsonl")
DEFAULT_WORKER = os.path.join(REPO, "target", "release", "ksci-worker")

# schema/expr.v1.md, "Identity": the canonical table of x**2 + 1 and its identity.
X2_PLUS_1 = b'{"nodes":[["Sym","x",{}],["Int","2"],["Pow",0,1],["Int","1"],["Add",2,3]],"root":4,"v":1}'
X2_PLUS_1_IDENTITY = "37f7939bd191aa48173b1b6a45838b6b246aeb8a001d276f9b1c2798342cb964"


def corpus_lines():
    with open(CORPUS, "rb") as handle:
        data = handle.read()
    lines = data.split(b"\n")
    if lines[-1] != b"":
        raise AssertionError("requests.jsonl must end with a newline")
    return lines[:-1]


def worker_path():
    return os.environ.get("KSCI_WORKER", DEFAULT_WORKER)


def reachable_count(table):
    """Distinct node indices reachable from the root: the steps science.cas.canonicalize charges.

    Written independently of _wire so the expected status does not come from the code under test.
    """
    nodes = table["nodes"]
    seen = set()
    todo = [table["root"]]
    while todo:
        index = todo.pop()
        if index in seen:
            continue
        seen.add(index)
        node = nodes[index]
        if node[0] in ("Add", "Mul", "Pow"):
            todo.extend(node[1:])
        elif node[0] == "Fn":
            todo.extend(node[2:])
    return len(seen)


def recording(inner, log):
    def transport(request):
        reply = inner(request)
        log.append((request, reply))
        return reply

    return transport


class WireTest(unittest.TestCase):
    def tearDown(self):
        host_shim.reset()
        _tool._transport = None

    def require_worker(self):
        path = worker_path()
        if not os.path.isfile(path):
            self.fail("ksci-worker not found at " + path + "; run: cargo build -p ksci-worker --release")
        return path

    def test_dumps_roundtrip_corpus(self):
        lines = corpus_lines()
        self.assertGreaterEqual(len(lines), 18)
        for number, line in enumerate(lines, 1):
            with self.subTest(line=number):
                self.assertEqual(_wire.dumps(json.loads(line)), line)

    def test_canonicalize_schema_example(self):
        # Nodes out of order, as in the second corpus line, canonicalise to the schema's example and identity.
        table = {"nodes": [["Add", 3, 1], ["Int", "1"], ["Int", "2"], ["Pow", 4, 2], ["Sym", "x", {}]], "root": 0, "v": 1}
        canonical = _wire.dumps(_wire.canonicalize(table))
        self.assertEqual(canonical, X2_PLUS_1)
        self.assertEqual(hashlib.sha256(canonical).hexdigest(), X2_PLUS_1_IDENTITY)

    def test_worker_differential(self):
        host_shim.install(self.require_worker())
        log = []
        _tool._transport = recording(_tool._transport, log)
        lines = corpus_lines()
        exact = 0
        exhausted = 0
        saw_steps_1 = False
        for number, line in enumerate(lines, 1):
            request = json.loads(line)
            steps = request["budget"]["steps"]
            with self.subTest(line=number, steps=steps):
                response = _tool.call(request["op"], request["args"], int(steps))
                sent, (tag, payload) = log[-1]
                # call() rebuilds the corpus bytes exactly, and the mirror re-encodes the worker's bytes exactly.
                self.assertEqual(sent, line)
                self.assertEqual(tag, 0)
                self.assertEqual(_wire.dumps(response), payload)
                expr = request["args"]["expr"]
                if int(steps) >= reachable_count(expr):
                    exact += 1
                    self.assertEqual(response["status"], "exact")
                    canonical = _wire.canonicalize(expr)
                    self.assertEqual(response["result"], canonical)
                    self.assertEqual(response["identity"], hashlib.sha256(_wire.dumps(response["result"])).hexdigest())
                    self.assertEqual(response["identity"], hashlib.sha256(_wire.dumps(canonical)).hexdigest())
                else:
                    exhausted += 1
                    self.assertEqual(response["status"], "fuel_exhausted")
                    self.assertIsNone(response["result"])
                    self.assertIsNone(response["identity"])
                if steps == "1":
                    saw_steps_1 = True
                    self.assertEqual(response["status"], "fuel_exhausted")
        self.assertTrue(saw_steps_1, "the corpus must hold a steps \"1\" request")
        self.assertGreaterEqual(exact, 10)
        self.assertGreaterEqual(exhausted, 3)
        self.assertEqual(host_shim.calls, len(lines))

    def test_worker_error_tag(self):
        host_shim.install(self.require_worker())
        table = json.loads(X2_PLUS_1)
        with self.assertRaisesRegex(RuntimeError, "^klein tool error unsupported-op: "):
            _tool.call("science.cas.no_such_op", {"expr": table}, 10)
        self.assertEqual(host_shim.calls, 1)

    def test_rejects_float_and_surrogate(self):
        with self.assertRaises(TypeError):
            _wire.dumps(1.5)
        with self.assertRaises(TypeError):
            _wire.dumps({"a": [0.0]})
        for bad in ("\ud800", "a\udfff", {"\udc00": 1}):
            with self.subTest(value=repr(bad)):
                with self.assertRaisesRegex(ValueError, "^JCS: lone surrogate U\\+D[89A-F][0-9A-F]{2} "):
                    _wire.dumps(bad)
        # Integers beyond IEEE-754's exact range would be rounded by the Rust codec, so the mirror refuses them.
        self.assertEqual(_wire.dumps(2**53 - 1), b"9007199254740991")
        self.assertEqual(_wire.dumps(-(2**53 - 1)), b"-9007199254740991")
        for bad in (2**53, -(2**53), 2**200):
            with self.subTest(value=bad):
                with self.assertRaises(ValueError):
                    _wire.dumps(bad)

    def test_strings_and_key_order(self):
        self.assertEqual(
            _wire.dumps("\u0000\u001f\b\f\n\r\t\"\\\u007f /"),
            b'"\\u0000\\u001f\\b\\f\\n\\r\\t\\"\\\\\x7f\xe2\x80\xa8/"',
        )
        # No Unicode normalisation: precomposed and decomposed stay distinct.
        self.assertEqual(_wire.dumps("é"), b'"\xc3\xa9"')
        self.assertEqual(_wire.dumps("é"), b'"e\xcc\x81"')
        # UTF-16 code units: U+1D465 is D835 DC65, which sorts before U+FFFF (code point order would not).
        self.assertEqual(
            _wire.dumps({"￿": 1, "\U0001d465": 2, "a": 3}),
            '{"a":3,"\U0001d465":2,"￿":1}'.encode("utf-8"),
        )

    def test_no_transport(self):
        with self.assertRaisesRegex(RuntimeError, "^no klein tool transport installed$"):
            _tool.call("science.cas.canonicalize", {"expr": json.loads(X2_PLUS_1)}, 10)
        saved = os.environ.pop("KSCI_WORKER", None)
        try:
            host_shim.install()
            with self.assertRaisesRegex(RuntimeError, "^no klein tool transport: set KSCI_WORKER$"):
                _tool.call("science.cas.canonicalize", {"expr": json.loads(X2_PLUS_1)}, 10)
        finally:
            if saved is not None:
                os.environ["KSCI_WORKER"] = saved
        with self.assertRaises(ValueError):
            host_shim.install("target/release/ksci-worker")
        with self.assertRaises(ValueError):
            host_shim.install(os.path.join(REPO, "no-such-worker"))

    def test_no_call_on_import(self):
        host_shim.install(self.require_worker())
        importlib.reload(sympy)
        self.assertEqual(host_shim.calls, 0)
        self.assertIsNone(host_shim._process)


if __name__ == "__main__":
    unittest.main()
