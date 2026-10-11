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

Any other key, or a missing key, is rejected. A cycle anywhere in the table is rejected, whether or not the root
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
- Names are compared as exact UTF-8 bytes. There is no Unicode normalisation: `"x"` and `"x́"` are different
  names.
- Object keys are written in RFC 8785 order (UTF-16 code units). Assumption keys are ASCII, so this is byte order.

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

## Document size cap

A document longer than 64 MiB (`MAX_DOCUMENT_BYTES` = 67108864 bytes) is rejected before it is parsed. A document
of exactly 64 MiB passes the size check.
