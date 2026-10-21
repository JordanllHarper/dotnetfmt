# Known gaps / future improvements

Things intentionally left unsupported by the current formatter (they're
left textually untouched rather than mis-formatted). Noted here so
they're easy to pick up later instead of being rediscovered.

## Control flow

- **Property and list patterns** (`case { Prop: value }:`, `case [1,
  2, ..]:`) — `parse_primary_pattern` only recognizes relational,
  type, and expression patterns (plus `and`/`or`/`not`/`when`
  combinators and parenthesized grouping); property and list pattern
  syntax isn't part of the grammar, so a `case`/arm using one is left
  untouched.

## Formatting / layout

- **Multiple braces per source line, partially handled.**
  `statement::split_brace_segments` now recognizes two shapes and
  splits them onto the separate logical lines the one-statement-per-
  line model expects: a combined close-then-open (`} else {`, `}
  else if (x) {`) splits into `}`, the header, `{`; a single-line
  block body (`if (x) { y(); }`) splits into the header+`{` (kept
  combined, matching how an already-combined header+brace is
  preserved elsewhere), each top-level statement, and `}`. Remaining
  gaps, left untouched same as any other unrecognized construct:
  - A body containing a nested brace (`if (x) { if (y) { z(); } }`)
    isn't split at all — only a flat, brace-free body is supported.
  - A combined close-then-open whose header isn't itself recognized
    (e.g. `} catch (Exception e) {`, since `try`/`catch` aren't part
    of the grammar) is left as one unsplit line.
  - A line combining more than these two shapes at once (e.g. a
    one-line `do { x(); } while (cond);`, where the body's closing
    `}` is followed by more than just `{`) isn't recognized.
- **Multi-line statements/signatures** — every statement/signature
  must fit on one source line; a wrapped method signature or a
  multi-line expression is left untouched.
