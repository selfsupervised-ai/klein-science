# provenance: original
"""The stand-in's single tool-call entry point.

`call` builds a canonical `klein.science.expr.v1` request, encodes it with the one Python encoder (`_wire.dumps`) and
hands the bytes to `_transport`. In Klein the host installs the transport; under CPython, `host_shim.install` does
(tests only, never shipped). With no transport installed every call fails.

The transport takes the request bytes and returns `(tag, payload)`: tag 0 carries response JCS bytes, tags 1 to 3 a
UTF-8 error message (schema/expr.v1.md, "Errors"; the R2 frame tags).
"""

import json

from . import _wire

_transport = None

_ERROR_TAGS = {1: "bad-request", 2: "unsupported-op", 3: "internal"}


def call(op, args, steps):
    """Run tool operation `op` on `args` (argument name to node table) within a logical budget of `steps`."""
    if _transport is None:
        raise RuntimeError("no klein tool transport installed")
    request = {"args": args, "budget": {"steps": str(steps)}, "op": op, "v": 1}
    reply = _transport(_wire.dumps(request))
    tag = reply[0]
    payload = reply[1]
    if tag == 0:
        return json.loads(payload)
    if tag in _ERROR_TAGS:
        raise RuntimeError("klein tool error " + _ERROR_TAGS[tag] + ": " + payload.decode("utf-8", "replace"))
    raise RuntimeError("klein tool transport returned unknown tag " + str(tag))
