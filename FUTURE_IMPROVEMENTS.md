# Known gaps / future improvements

Things intentionally left unsupported by the current formatter (they're
left textually untouched rather than mis-formatted). Noted here so
they're easy to pick up later instead of being rediscovered.

## Object creation

- **Multi-dimensional array creation with explicit sizes** (`new
  int[2, 3]`) — only a single bracketed size expression is parsed
  (`new Type[size]`); the no-size forms (`new Type[,] { .. }`) support
  any rank via a nested `{ .. }` initializer instead.
- **Object/collection initializers** (`new Foo { X = 1 }`, `new Foo()
  { X = 1 }`) — `expr.rs`'s `new` case only recognizes array creation
  brackets before a `{ .. }` initializer, not a bare type name or a
  constructor argument list.

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

- **`for` loop init with no type** — `for (i = 0, j = 10; ...; ...)`
  isn't recognized; only a declaration (`for (int i = 0, j = 10; ...)`)
  is, since `format_for_init` only tries `try_format_declarators`.
- **`foreach` with tuple deconstruction** (`foreach (var (k, v) in
  dict)`) — the element position only accepts `type name`, not a
  tuple pattern.
- **Parenthesized sub-patterns** (`(int i and > 0) or < -10`) —
  `parse_and_pattern`/`parse_or_pattern` chain flat left-to-right;
  there's no `Paren`-equivalent pattern node, so grouping parentheses
  around a sub-pattern aren't parsed.
- **`switch` expressions outside a declaration's initializer** (`return
  v switch { ... };`, `Foo(v switch { ... })`) — both the multi-line
  form (`try_format_switch_expression_header`) and the single-line
  form (`try_format_inline_switch_expression`) only recognize `type
  name = expr switch ...`; a switch expression anywhere else is left
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
