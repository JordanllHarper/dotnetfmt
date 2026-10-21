# dotnetfmt

A line-oriented C# formatter: each line is treated as (at most) one
logical statement, reindented and reformatted by whichever
recursive-descent parser recognizes it (`formatter::expr`,
`formatter::statement`). Anything a parser doesn't recognize is left
textually untouched rather than guessed at or mangled — this is the
core contract of the whole tool and should never be compromised for
the sake of "probably fine" guessing.

## Project standards

- **Recognize-and-fix, or leave untouched. Never guess.** Every parse
  function returns `Option`/fails closed; there is no "best effort"
  partial formatting of a construct the grammar doesn't model. If a
  construct is knowingly unsupported, note it in
  `FUTURE_IMPROVEMENTS.md` under the relevant section instead of
  half-implementing it.
- **Precedence is explicit, never inferred.** The expression AST
  mirrors the source's grouping exactly (every parenthesized
  subexpression becomes a `Paren` node that always prints its own
  parens); the printer never re-derives precedence. Don't "simplify"
  by dropping parens based on operator precedence.
- **Prefer `let-else` or `matches!`/`if let` over a two-arm `match`**
  when a match is only being used to test one pattern and fall through
  otherwise (e.g. extracting a token and breaking/returning on
  mismatch). Reserve a full `match` for when there are genuinely
  several cases to branch on.
- **Doc comments explain *why*, not *what*.** A comment on a parser
  function should call out non-obvious grammar decisions, ambiguities
  resolved a particular way, or constraints from the surrounding
  design (see `expr.rs`'s module doc and the `MULTI_CHAR_OPS` comment
  in `token.rs` for the tone to match) — not restate what the code
  already says.
- **No backwards-compatibility shims.** This is a from-scratch tool
  with no external consumers of internal APIs; rename and restructure
  freely rather than keeping old names/paths around.
- **Record deliberate gaps in `FUTURE_IMPROVEMENTS.md`.** When a
  construct is intentionally left unsupported (rather than an
  oversight), add a bullet under the matching section (or a new one)
  explaining what's missing and why, so it's easy to pick up later
  instead of being rediscovered from scratch.
- **When a new convention is established — by explicit user
  direction, or by a corrected approach during review — add it to
  this file.** This list is only useful if it stays current; don't
  let a standard live only in conversation history or a commit
  message.

## Testing

- Tests live inline in `#[cfg(test)] mod tests` at the bottom of the
  file they test (see `formatter/mod.rs`), not in a separate
  `tests/` tree.
- Name tests after the behavior being asserted (e.g.
  `shift_binds_tighter_than_relational`,
  `nested_ternary_expression_is_right_associative`), not after the
  function under test — a reader should be able to tell what broke
  from the test name alone in CI output.
- When adding an operator/construct to the expression grammar, cover:
  the happy path, precedence relative to its neighbors, and (if
  right-associative) chained associativity.
- Run `cargo test` and `cargo clippy --all-targets` before considering
  a change done.
