# Known gaps / future improvements

Things intentionally left unsupported by the current formatter (they're
left textually untouched rather than mis-formatted). Noted here so
they're easy to pick up later instead of being rediscovered.

## Object creation

- **Indexed collection initializers** (`new Dictionary<string, int>
  { ["a"] = 1 }`) — each initializer item is parsed as a full
  expression (`parse_assignment`), and `["a"]` isn't a valid primary
  expression on its own (`[` only starts an indexer on an existing
  operand); an item using this form leaves the whole statement
  untouched.

## Types & declarations

- **Qualified/dotted type names** (`System.Collections.Generic.List<int>`)
  in type position — `parse_type` only accepts a single identifier
  base name.

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
