# Known gaps / future improvements

Things intentionally left unsupported by the current formatter (they're
left textually untouched rather than mis-formatted). Noted here so
they're easy to pick up later instead of being rediscovered.

## String interpolation

`$"{x}"` currently tokenizes as a bare `$` symbol followed by an
ordinary string; the interpolation holes aren't parsed as expressions
at all. Pieces needed, to be tackled step by step:

- **Tokenizing `$"..."` as a single token.** Recognize the `$` prefix
  and lex the whole interpolated string (quotes, literal text, and
  `{...}` holes) as one token, so it round-trips safely wherever it
  appears instead of splitting into a stray `$` symbol plus an
  ordinary string.
- **Escaped braces** (`{{`, `}}`) — these render as literal `{`/`}`
  and must not be mistaken for the start/end of a hole.
- **Hole boundary scanning with nested depth tracking.** A hole can
  contain nested `(`/`[`/`{` (calls, indexers, object/collection
  initializers), so finding its matching closing `}` means tracking
  bracket depth, not just scanning to the next `}`.
- **Nested string/char literals inside a hole** (e.g. `{x ?? "none"}`)
  — quote characters inside a hole must not be mistaken for the end
  of the interpolated string itself.
- **Reformatting each hole's expression.** Re-tokenize and re-parse
  the hole's expression text through the existing
  `tokenize`/`parse_expression`/`render` pipeline and splice the
  reformatted result back in; literal text segments between holes are
  left untouched.
- **Alignment/format specifiers** (`{expr,width}`, `{expr:format}`,
  `{expr,width:format}`) — only the expression part (up to the first
  top-level `,` or `:`) gets reformatted; the alignment/format text
  itself is preserved verbatim.
- **The verbatim form** (`$@"..."` / `@$"..."`) — uses `""` for a
  literal quote instead of backslash escapes, but holes work the same
  way.
- **Per-hole fallback.** If one hole's expression doesn't parse (e.g.
  it uses a construct outside this formatter's expression grammar),
  that hole's text is left untouched rather than failing the whole
  string or the whole line.
- Not planned: nested interpolated strings inside a hole
  (`$"{$"{x}"}"`) — rare enough to stay out of scope; such a hole
  simply won't reformat (falls under the per-hole fallback above).

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
