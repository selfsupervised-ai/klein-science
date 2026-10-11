# `klein.science.expr.v1`: the expression node table

This is the wire contract for expressions exchanged with the klein-science plugins. `crates/ksci-wire` implements it,
and its tests (`crates/ksci-wire/tests/contract.rs`) cite the sections below. Schema version: `v = 1`.

## Document shape

An expression is one JSON object with exactly three keys:

```json
{"nodes":[ <node>, ... ],"root":<index>,"v":1}
```

- `nodes` is a non-empty array of nodes. A node refers to its children by their index into `nodes`.
- `root` is the index of the expression's root node.
- `v` is the schema version, exactly `1`.

Any other key, a missing key or a repeated key is rejected. A cycle anywhere in the table is rejected, whether or not the root
reaches it. An index must be below `nodes.len()`; `nodes.len() - 1` is the largest valid index.

## Node kinds

Each node is a JSON array whose first element is the kind. The kind set is closed; any other kind is rejected.

| Kind | Wire form | Arity and content |
|---|---|---|
| `Int` | `["Int", n]` | `n` is an Int numeral |
| `Rat` | `["Rat", p, q]` | `p` is a non-zero Int numeral; `q` matches `[1-9][0-9]*` and is not `"1"` |
| `Float` | `["Float", man, exp, prec]` | value `man * 2**exp` at `prec` bits; `man` and `exp` are Int numerals, `prec` matches `[1-9][0-9]*` |
| `Sym` | `["Sym", name, {assumption: bool, ...}]` | `name` is a non-empty string; assumption keys match `[a-z][a-z_]*`, values are JSON booleans; `{}` is allowed |
| `Atom` | `["Atom", name]` | `name` is one of the named atoms below |
| `Add` | `["Add", i, j, ...]` | two or more child indices |
| `Mul` | `["Mul", i, j, ...]` | two or more child indices |
| `Pow` | `["Pow", base, exp]` | exactly two child indices |
| `Fn` | `["Fn", name, i, ...]` | `name` is a non-empty string, then zero or more child indices |

Symbol assumptions are part of the symbol: `x` with `{"positive":true}` and `x` with `{}` are different nodes.

## Numeral grammar

Numerals are JSON strings, never JSON numbers. The grammar admits exactly one spelling per value.

- **Int:** `0|-?[1-9][0-9]*`. Rejected spellings include `""`, `"-0"`, `"01"`, `"+1"`, `"1.0"`, `"1e3"` and any
  whitespace.
- **Rat:** `p` is an Int other than `"0"`; `q` matches `[1-9][0-9]*` and is not `"1"`. The sign lives on `p`.
- **Float:** `man` and `exp` are Ints; `prec` matches `[1-9][0-9]*`. The wire layer never rounds: the three strings
  are carried as given.
- **Step budget** (in requests): `0|[1-9][0-9]*`, at most `18446744073709551615` (`2**64 - 1`).

Numerals have no length limit other than the document cap: a 300-digit Int round-trips byte for byte.

### Lowest terms (D-20)

At this layer a Rat is checked against the grammar only. `["Rat","2","4"]` is accepted here. Checking that `p/q`
is in lowest terms needs a bignum gcd and lands in `ksci-num` (Phase 2) with its own witness.

## Named atoms

Special values are atoms with exactly these names, compared byte for byte:

| Name | Meaning |
|---|---|
| `oo` | positive infinity |
| `-oo` | negative infinity |
| `zoo` | complex infinity |
| `nan` | not a number |
| `pi` | pi |
| `E` | Euler's number |
| `I` | the imaginary unit |

Any other name (`Oo`, `inf`, `infinity`, ...) is rejected.

## Where JSON numbers may appear

A JSON number may appear only:

- as `v`;
- as `root`;
- as a child index inside an `Add`, `Mul` or `Pow` node (positions 1 onward) or an `Fn` node (positions 2 onward).

Each of these must be a non-negative integer below `2**32`. A JSON number anywhere else (a numeral, a name, a kind,
an assumption value, a bare node), or a non-integer, negative or too-large number in an allowed position (`1.0`,
`-1`, `4294967296`), is rejected with `JsonNumberNotAllowed` and the JSON pointer of the number.

## Strings and encoding

- Documents are UTF-8 JSON. A lone surrogate escape such as `"\ud800"` is not valid JSON text and is rejected.
- Names are compared as exact UTF-8 bytes. There is no Unicode normalisation: `"x"` and `"x\u0301"` are different
  names.
- Object keys are written in RFC 8785 order (UTF-16 code units). Assumption keys are ASCII, so this is byte order.
- No object may repeat a key, at any depth, in a table, a request or a response. Keys are compared after
  unescaping, so `"v"` and `"\u0076"` are the same key. A repeat is rejected as `DuplicateKey` with the JSON pointer
  of the repeated key, even when both copies carry the same value. JSON parsers disagree on which copy wins, so
  none does.

## Canonical form

Any valid table, including one with its nodes in any order, with structurally duplicate nodes, or with unreachable
nodes, has exactly one canonical table:

1. Walk the table from `root` with an iterative post-order depth-first search, visiting each node's children in the
   order given. There is no recursion.
2. When a node completes, rewrite its children to their canonical indices and encode it with JCS.
3. Deduplicate on those encoded bytes (a map keyed by the bytes). A node whose bytes were already emitted takes the
   existing index; otherwise it is appended. Each structurally distinct node is emitted once, at first completion.
4. Unreachable nodes are never visited, so they are dropped. The root completes last, so it is the last node.

**Argument order is preserved.** `Add(y, x)` and `Add(x, y)` are different tables at this layer. Sorting
commutative arguments belongs to the expression core (CORE-01), not to the wire.

Nodes that differ only in kind stay distinct: `["Int","2"]` and a `Float` with the same value are two nodes.

## Identity

```
identity = lowercase-hex( SHA-256( JCS( canonical table ) ) )
```

JCS is RFC 8785, produced by `serde_json_canonicalizer` =0.3.2, the same crate and pin as klein-rs. Every table has
exactly one canonical form, and the numeral grammar admits one spelling per value, so equal expressions have equal
identities however they were built.

Example: `x**2 + 1` canonicalises to

```json
{"nodes":[["Sym","x",{}],["Int","2"],["Pow",0,1],["Int","1"],["Add",2,3]],"root":4,"v":1}
```

with identity `37f7939bd191aa48173b1b6a45838b6b246aeb8a001d276f9b1c2798342cb964`.

## Document size and depth caps

A document longer than 64 MiB (`MAX_DOCUMENT_BYTES` = 67108864 bytes) is rejected before it is parsed. A document
of exactly 64 MiB passes the size check. The cap applies to tables, requests and responses alike.

Arrays and objects nest at most 32 deep (`MAX_DEPTH`). The first array or object past that depth is rejected as
`TooDeep` with its JSON pointer. The deepest valid document, a request whose argument table holds a symbol with
assumptions, nests 6 deep.

## Envelopes

A tool call exchanges a request and a response. Both are JCS documents, and both are hashed. A record pairs their
digests with provenance, which is never inside the hashed bytes.

### Request

```json
{"args":{"<name>":<table>, ...},"budget":{"steps":"<steps>"},"op":"<operation>","v":1}
```

- `args` maps argument names to node tables (the shape above), for example `{"expr":<table>}`.
- `budget.steps` is the logical step budget, a step-budget numeral string (`0` to `18446744073709551615`). A JSON
  number here is rejected.
- `op` names the operation, for example `science.cas.canonicalize`.
- `v` is exactly `1`. Unknown keys are rejected at every level.

### Response

```json
{"identity":"<hex>"|null,"numeric_class":"exact","result":<table>|null,"status":"exact"|"fuel_exhausted","v":1}
```

- `status` is `exact`, or `fuel_exhausted` when the logical step budget ran out.
- `numeric_class` is `exact` in Phase 1, whatever the status. Any other value is rejected.
- With status `exact`, `result` is the result table **in canonical form** (it equals its own canonical table), and
  `identity` is exactly that table's identity: 64 lowercase hex digits. A missing result or identity, a
  non-canonical result, or any other identity is rejected.
- With status `fuel_exhausted`, `result` and `identity` are both `null`. A result or an identity is rejected.
- The keys are always present, and unknown keys (such as `provenance`) are rejected.

So a response has one byte string per outcome: an exact result has one canonical table and one identity.

Example: the response for `x**2 + 1` with an exact result is

```json
{"identity":"37f7939bd191aa48173b1b6a45838b6b246aeb8a001d276f9b1c2798342cb964","numeric_class":"exact","result":{"nodes":[["Sym","x",{}],["Int","2"],["Pow",0,1],["Int","1"],["Add",2,3]],"root":4,"v":1},"status":"exact","v":1}
```

and a budget-exhausted response is
`{"identity":null,"numeric_class":"exact","result":null,"status":"fuel_exhausted","v":1}`.

### Record

```json
{"provenance":{"backend":"...","device":"...","driver":"...","impl":"...","wall_ms":"<decimal>"},"request_digest":"<hex>","response_digest":"<hex>"}
```

`request_digest` and `response_digest` are lowercase-hex SHA-256 of the request and response bytes. Every
provenance field is a string, including `wall_ms`. Two records of the same request and response have the same
digests whatever their provenance.

### Canonical input

A request is accepted only if its bytes equal their own JCS re-encoding. Any other spelling (whitespace, key order,
an unnecessary escape, a trailing newline) is rejected as `NonCanonical`, so each request has exactly one byte
string and one digest. A duplicated key is rejected before this check, as `DuplicateKey`. Responses are read back
under the same rules, and then checked against the Response rules above.

### What is hashed

Only request bytes and response bytes are hashed. Provenance lives only in the record, and the request and response
schemas reject a `provenance` key, so implementation, backend, device, driver and wall time can never change a
digest.
