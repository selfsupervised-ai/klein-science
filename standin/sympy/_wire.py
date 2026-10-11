# provenance: original
"""The Python JCS mirror of `ksci-wire`: the one Python encoder (D-01).

`dumps` is RFC 8785 (JCS) over the wire's value domain: None, bool, int, str, list, and dict with str keys. It agrees
byte for byte with the Rust codec (`serde_json_canonicalizer`) and with the `rfc8785` test oracle; the differential in
`standin/tests` holds the three to that.

`canonicalize` is the canonical form of a `klein.science.expr.v1` node table, exactly as `ksci_wire::Table::canonicalize`
computes it (schema/expr.v1.md, "Canonical form").

This module runs inside the guest, so it keeps to the Monty subset: no classes, decorators, generators, `match` or
`del`, and no imports.
"""

# RFC 8785 serialises numbers as IEEE-754 doubles. An integer outside +-(2**53 - 1) has no exact double, so the Rust
# codec would round it and the bytes would no longer say the value. The wire carries such values as decimal strings,
# so the mirror refuses them instead of rounding.
MAX_SAFE_INT = 9007199254740991

_HEX = "0123456789abcdef"

_SHORT_ESCAPES = {
    '"': '\\"',
    "\\": "\\\\",
    "\b": "\\b",
    "\f": "\\f",
    "\n": "\\n",
    "\r": "\\r",
    "\t": "\\t",
}


def _string(text, parts):
    parts.append('"')
    for ch in text:
        code = ord(ch)
        if ch in _SHORT_ESCAPES:
            parts.append(_SHORT_ESCAPES[ch])
        elif code < 0x20:
            parts.append("\\u00" + _HEX[code >> 4] + _HEX[code & 15])
        elif code >= 0xD800 and code <= 0xDFFF:
            raise ValueError("JCS: lone surrogate U+" + _hex4(code) + " is not a Unicode scalar value")
        else:
            parts.append(ch)
    parts.append('"')


def _hex4(code):
    return _HEX[(code >> 12) & 15] + _HEX[(code >> 8) & 15] + _HEX[(code >> 4) & 15] + _HEX[code & 15]


def _utf16_key(key):
    # RFC 8785 section 3.2.3: object members are sorted by the UTF-16 code units of their names.
    return key.encode("utf-16-be")


def _encode(value, parts):
    if value is None:
        parts.append("null")
    elif value is True:
        parts.append("true")
    elif value is False:
        parts.append("false")
    elif isinstance(value, float):
        raise TypeError("JCS: floats are outside the wire domain; numerals travel as strings")
    elif isinstance(value, int):
        if value > MAX_SAFE_INT or value < -MAX_SAFE_INT:
            raise ValueError("JCS: integer outside +-(2**53 - 1) has no exact IEEE-754 double")
        parts.append(str(value))
    elif isinstance(value, str):
        _string(value, parts)
    elif isinstance(value, list):
        parts.append("[")
        first = True
        for item in value:
            if not first:
                parts.append(",")
            first = False
            _encode(item, parts)
        parts.append("]")
    elif isinstance(value, dict):
        keys = list(value.keys())
        for key in keys:
            if not isinstance(key, str):
                raise TypeError("JCS: object keys must be strings")
        keys = sorted(keys, key=_utf16_key)
        parts.append("{")
        first = True
        for key in keys:
            if not first:
                parts.append(",")
            first = False
            _string(key, parts)
            parts.append(":")
            _encode(value[key], parts)
        parts.append("}")
    else:
        raise TypeError("JCS: unsupported type " + type(value).__name__)


def dumps(value):
    """The RFC 8785 bytes of `value`.

    A float raises TypeError; a string with a lone surrogate, or an integer outside +-(2**53 - 1), raises ValueError.
    Strings are never normalised.
    """
    parts = []
    _encode(value, parts)
    return "".join(parts).encode("utf-8")


def _children(node):
    """The child indices of a node, in argument order."""
    kind = node[0]
    if kind == "Add" or kind == "Mul":
        return node[1:]
    if kind == "Pow":
        return node[1:3]
    if kind == "Fn":
        return node[2:]
    if kind == "Int" or kind == "Rat" or kind == "Float" or kind == "Sym" or kind == "Atom":
        return []
    raise ValueError("expr.v1: unknown node kind " + repr(kind))


def _remap(node, canonical):
    """The node with each child replaced by its canonical index, argument order kept."""
    kind = node[0]
    if kind == "Add" or kind == "Mul" or kind == "Pow":
        head = [kind]
        rest = _children(node)
    elif kind == "Fn":
        head = [kind, node[1]]
        rest = node[2:]
    else:
        return list(node)
    for child in rest:
        mapped = canonical[child]
        if mapped is None:
            raise ValueError("expr.v1: the table has a cycle")
        head.append(mapped)
    return head


def canonicalize(table):
    """The canonical table of a `klein.science.expr.v1` node table, as `ksci_wire::Table::canonicalize` builds it.

    An iterative post-order walk from the root visits children in the order given. A completed node is rewritten to
    canonical child indices, keyed by its JCS bytes, and appended only if those bytes are new. Unreachable nodes are
    never visited, and the root completes last.
    """
    nodes = table["nodes"]
    root = table["root"]
    canonical = [None] * len(nodes)
    entered = [False] * len(nodes)
    out = []
    seen = {}
    entered[root] = True
    stack = [[root, 0, _children(nodes[root])]]
    while len(stack) > 0:
        frame = stack[-1]
        node = frame[0]
        pos = frame[1]
        kids = frame[2]
        if pos < len(kids):
            frame[1] = pos + 1
            child = kids[pos]
            # An entered child is already complete: a valid table has no cycle.
            if not entered[child]:
                entered[child] = True
                stack.append([child, 0, _children(nodes[child])])
        else:
            stack.pop()
            mapped = _remap(nodes[node], canonical)
            key = dumps(mapped)
            if key in seen:
                index = seen[key]
            else:
                out.append(mapped)
                index = len(out) - 1
                seen[key] = index
            canonical[node] = index
    return {"nodes": out, "root": canonical[root], "v": 1}
