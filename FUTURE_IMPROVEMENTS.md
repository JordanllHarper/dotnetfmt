# Known gaps / future improvements

Things intentionally left unsupported by the current formatter (they're
left textually untouched rather than mis-formatted). Noted here so
they're easy to pick up later instead of being rediscovered.

## Types & declarations

- **The `global::` alias qualifier** (`global::System.String`) — `::`
  lexes as two separate `:` symbols (there's no `::` operator), so
  `parse_type` parses `global` as a type on its own and trips on the
  `:` that follows; a type using this qualifier is left untouched.

## Methods & functions

- **Generic type parameter constraints** (`T Foo<T>(T x) where T :
  class`) — `try_format_method_signature` parses the `<T, ...>` list
  itself but not a trailing `where` clause; the leftover tokens mean
  the whole signature is left untouched rather than dropping the
  constraint.

## Control flow

- **Property and list patterns** (`case { Prop: value }:`, `case [1,
  2, ..]:`) — `parse_primary_pattern` only recognizes relational,
  type, and expression patterns (plus `and`/`or`/`not`/`when`
  combinators and parenthesized grouping); property and list pattern
  syntax isn't part of the grammar, so a `case`/arm using one is left
  untouched.

## Formatting / layout

- **Multiple braces per source line** (`if (x) { y(); }`,
  `} else {`) — the brace-depth indent tracker assumes at most one
  open or one close brace matters per line for *this* line's own
  indent level; it still counts every brace on the line for the
  running depth, but doesn't reformat such lines onto multiple lines.
- **Multi-line statements/signatures** — every statement/signature
  must fit on one source line; a wrapped method signature or a
  multi-line expression is left untouched.
