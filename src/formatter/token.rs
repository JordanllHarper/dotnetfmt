//! Lexical tokenization shared by the expression parser and the
//! statement-level (declaration / method signature) parsers.

use std::iter::Peekable;
use std::str::CharIndices;

/// A lexical token. Classified only as finely as parsing/spacing
/// decisions need -- e.g. all keywords and identifiers are `Ident`,
/// since which ones are meaningful (`var`, modifiers, ...) is decided
/// by the parsers that consume them, not by the lexer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Token<'a> {
    /// An identifier or keyword: `[A-Za-z_][A-Za-z0-9_]*`.
    Ident(&'a str),
    /// An integer literal, including any `0x`/`_`/`u`/`l` decoration.
    Int(&'a str),
    /// A floating-point literal (has a `.`, an exponent, or an
    /// `f`/`d`/`m` suffix).
    Float(&'a str),
    /// A string literal, `"..."` or verbatim `@"..."`, quotes included.
    Str(&'a str),
    /// An interpolated string literal, `$"..."`, `$@"..."`, or
    /// `@$"..."`, prefix and quotes included, holes and all --
    /// `expr.rs` reformats each `{ .. }` hole when it consumes this
    /// token (see `interpolated.rs`).
    InterpolatedStr(&'a str),
    /// A char literal, `'...'`, quotes included.
    Char(&'a str),
    /// A single-character punctuator not covered by `Op`.
    Symbol(char),
    /// A fixed multi-character operator, e.g. `"=="`, `"+="`.
    Op(&'static str),
}

// `<<`/`>>` are deliberately not listed here: lexing them as single
// tokens would make `>>` ambiguous with the double `>` that closes a
// nested generic (`List<List<int>>`). Instead `expr.rs`'s shift parser
// pairs up two adjacent `<`/`>` `Symbol`s itself, which is unambiguous
// since that only happens in expression context, never while parsing
// a type's generic argument list.
const MULTI_CHAR_OPS: &[&str] = &[
    "==", "!=", "<=", ">=", "&&", "||", "+=", "-=", "*=", "/=", "%=", "++", "--", "=>", "??", "::",
];

fn is_ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_'
}

fn is_ident_continue(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Tokenizes a single line/statement of source text.
pub fn tokenize(text: &str) -> Vec<Token<'_>> {
    let mut tokens = Vec::new();
    let mut chars = text.char_indices().peekable();

    while let Some(&(start, ch)) = chars.peek() {
        match ch {
            ch if ch.is_whitespace() => {
                chars.next();
            }
            '$' | '@'
                if text[start..].starts_with("$\"")
                    || text[start..].starts_with("$@\"")
                    || text[start..].starts_with("@$\"") =>
            {
                let (len, _) = scan_interpolated(&text[start..]);
                let end = start + len;
                tokens.push(Token::InterpolatedStr(&text[start..end]));
                advance_to(&mut chars, end);
            }
            '"' => {
                let end = scan_string(&mut chars, start, false);
                tokens.push(Token::Str(&text[start..end]));
            }
            '@' if matches!(peek_next(&mut chars), Some('"')) => {
                chars.next();
                let end = scan_string(&mut chars, start, true);
                tokens.push(Token::Str(&text[start..end]));
            }
            '\'' => {
                let end = scan_char(&mut chars, start);
                tokens.push(Token::Char(&text[start..end]));
            }
            ch if is_ident_start(ch) => {
                chars.next();
                let mut end = start + ch.len_utf8();
                while let Some(&(_, c)) = chars.peek() {
                    if !is_ident_continue(c) {
                        break;
                    }
                    end += c.len_utf8();
                    chars.next();
                }
                tokens.push(Token::Ident(&text[start..end]));
            }
            ch if ch.is_ascii_digit() => {
                let end = scan_number(&mut chars, start);
                let is_float = text[start..end].contains('.')
                    || text[start..end].contains(['e', 'E'])
                    || text[start..end].ends_with(['f', 'F', 'd', 'D', 'm', 'M']);
                tokens.push(if is_float {
                    Token::Float(&text[start..end])
                } else {
                    Token::Int(&text[start..end])
                });
            }
            _ => {
                if let Some(op) = MULTI_CHAR_OPS
                    .iter()
                    .find(|op| text[start..].starts_with(*op))
                {
                    for _ in 0..op.len() {
                        chars.next();
                    }
                    tokens.push(Token::Op(op));
                } else {
                    tokens.push(Token::Symbol(ch));
                    chars.next();
                }
            }
        }
    }

    tokens
}

fn peek_next(chars: &mut Peekable<CharIndices<'_>>) -> Option<char> {
    // Cheap: `CharIndices` just wraps a pointer/length over `text`, so
    // cloning it doesn't allocate or copy the underlying string.
    chars.clone().nth(1).map(|e| e.1)
}

/// Advances `chars` one char at a time until its next position is `>=
/// end`. Used after jumping straight to an already-computed end
/// offset (as `scan_interpolated` returns), since `end` is a byte
/// offset and `chars` can only be driven forward char-by-char.
fn advance_to(chars: &mut Peekable<CharIndices<'_>>, end: usize) {
    while let Some(&(pos, _)) = chars.peek() {
        if pos >= end {
            break;
        }
        chars.next();
    }
}

/// Consumes an integer/float literal (digits, a single fractional
/// `.digits`, an optional exponent, and a trailing alphabetic suffix)
/// starting at `start`, returning its end offset.
fn scan_number(chars: &mut Peekable<CharIndices<'_>>, start: usize) -> usize {
    let mut end = start;
    while let Some(&(_, c)) = chars.peek() {
        let is_fraction_dot = c == '.' && matches!(peek_next(chars), Some(d) if d.is_ascii_digit());
        if c.is_ascii_digit() || c == '_' || is_fraction_dot {
            end += c.len_utf8();
            chars.next();
        } else {
            break;
        }
    }

    end += scan_exponent(chars);

    while let Some(&(_, c)) = chars.peek() {
        if !c.is_ascii_alphabetic() {
            break;
        }
        end += c.len_utf8();
        chars.next();
    }

    end
}

/// Consumes an `e`/`E` exponent suffix (with an optional `+`/`-` sign and
/// at least one digit) if one is present at the cursor, returning the
/// number of bytes consumed. Leaves `chars` untouched and returns 0 if
/// what follows isn't a valid exponent (e.g. `e` with no digit after it).
fn scan_exponent(chars: &mut Peekable<CharIndices<'_>>) -> usize {
    let mut lookahead = chars.clone();
    if !matches!(lookahead.next(), Some((_, 'e' | 'E'))) {
        return 0;
    }

    let mut len = 1;
    if matches!(lookahead.peek(), Some((_, '+' | '-'))) {
        len += 1;
        lookahead.next();
    }

    if !matches!(lookahead.peek(), Some((_, d)) if d.is_ascii_digit()) {
        return 0;
    }

    while let Some(&(_, d)) = lookahead.peek() {
        if !d.is_ascii_digit() {
            break;
        }
        len += d.len_utf8();
        lookahead.next();
    }

    *chars = lookahead;
    len
}

/// Consumes a `"..."` or verbatim `@"..."` string literal starting at
/// `start` (with the opening `"`/`@"` not yet consumed from `chars`),
/// returning its end offset.
pub(crate) fn scan_string(
    chars: &mut Peekable<CharIndices<'_>>,
    start: usize,
    verbatim: bool,
) -> usize {
    let opening_len = if verbatim { 2 } else { 1 };
    chars.next(); // opening quote
    let mut end = start + opening_len;

    loop {
        match chars.peek().copied() {
            None => break,
            Some((_, '"')) => {
                chars.next();
                end += 1;
                if verbatim && matches!(chars.peek(), Some((_, '"'))) {
                    chars.next();
                    end += 1;
                    continue;
                }
                break;
            }
            Some((_, '\\')) if !verbatim => {
                chars.next();
                end += 1;
                if let Some((_, c)) = chars.next() {
                    end += c.len_utf8();
                }
            }
            Some((_, c)) => {
                chars.next();
                end += c.len_utf8();
            }
        }
    }

    end
}

/// Consumes a `'...'` char literal starting at `start` (with the
/// opening `'` not yet consumed from `chars`), returning its end
/// offset.
pub(crate) fn scan_char(chars: &mut Peekable<CharIndices<'_>>, start: usize) -> usize {
    chars.next(); // opening quote
    let mut end = start + 1;

    loop {
        match chars.peek() {
            None => break,
            Some((_, '\'')) => {
                chars.next();
                end += 1;
                break;
            }
            Some((_, '\\')) => {
                chars.next();
                end += 1;
                if let Some((_, c)) = chars.next() {
                    end += c.len_utf8();
                }
            }
            Some((_, c)) => {
                let c = *c;
                chars.next();
                end += c.len_utf8();
            }
        }
    }

    end
}

/// Scans a `$"..."`/`$@"..."`/`@$"..."` interpolated string literal
/// starting at the beginning of `text` (its `$`/`@` prefix not yet
/// consumed), returning the byte length of the whole literal and the
/// byte ranges (relative to `text`) of each `{ .. }` hole's inner
/// text, not including the braces. Used both to find the token's end
/// while lexing and, later, to locate each hole's text for
/// reformatting (see `interpolated.rs`) -- one scan, two uses, so the
/// nested-depth/nested-literal handling only lives in one place.
pub fn scan_interpolated(text: &str) -> (usize, Vec<(usize, usize)>) {
    let mut chars = text.char_indices().peekable();
    let mut verbatim = false;
    loop {
        match chars.peek().copied() {
            Some((_, '$')) => {
                chars.next();
            }
            Some((_, '@')) => {
                chars.next();
                verbatim = true;
            }
            _ => break,
        }
    }

    let mut holes = Vec::new();
    if !matches!(chars.peek(), Some((_, '"'))) {
        return (text.len(), holes);
    }
    chars.next(); // opening quote

    loop {
        match chars.peek().copied() {
            None => return (text.len(), holes),
            Some((end, '"')) => {
                chars.next();
                if verbatim && matches!(chars.peek(), Some((_, '"'))) {
                    chars.next();
                    continue;
                }
                return (end + 1, holes);
            }
            Some((_, '\\')) if !verbatim => {
                chars.next();
                chars.next();
            }
            Some((start, '{')) => {
                chars.next();
                if matches!(chars.peek(), Some((_, '{'))) {
                    chars.next();
                    continue;
                }
                let hole_start = start + 1;
                if let Some(hole_end) = skip_hole_body(text, &mut chars) {
                    holes.push((hole_start, hole_end));
                }
            }
            Some((_, '}')) => {
                chars.next();
                if matches!(chars.peek(), Some((_, '}'))) {
                    chars.next();
                }
            }
            Some(_) => {
                chars.next();
            }
        }
    }
}

/// Consumes a hole's body (with its opening `{` already consumed),
/// tracking nested `{`/`}` depth -- which covers nested object/array
/// initializers, since `(`/`[` nesting can only ever close via a
/// matching `)`/`]`, never a stray `}` -- and skipping over nested
/// string/char literals so their own quotes/braces aren't mistaken
/// for the hole's structure. Returns the byte offset (relative to
/// `text`) of the hole's closing `}`, or `None` if the hole runs off
/// the end of `text` unterminated.
fn skip_hole_body(text: &str, chars: &mut Peekable<CharIndices<'_>>) -> Option<usize> {
    let mut depth = 0u32;
    loop {
        match chars.peek().copied() {
            None => return None,
            Some((end, '}')) if depth == 0 => {
                chars.next();
                return Some(end);
            }
            Some((_, '{')) => {
                chars.next();
                depth += 1;
            }
            Some((_, '}')) => {
                chars.next();
                depth -= 1;
            }
            Some((pos, '"')) => {
                scan_string(chars, pos, false);
            }
            Some((pos, '@')) if text[pos..].starts_with("@\"") => {
                chars.next();
                scan_string(chars, pos, true);
            }
            Some((pos, '\'')) => {
                scan_char(chars, pos);
            }
            Some(_) => {
                chars.next();
            }
        }
    }
}

/// Splits a hole's inner text at its first top-level `,` or `:` --
/// the start of an alignment/format-specifier (`{expr,width}`,
/// `{expr:format}`, `{expr,width:format}`), which is only
/// reformatted up to that point; the rest is preserved verbatim.
/// Nested `(`/`[`/`{` brackets and nested string/char literals
/// protect their own `,`/`:` from being mistaken for the split point
/// (e.g. `Foo(a, b)` or a nested ternary `a ? b : c` once
/// parenthesized).
pub fn split_hole(hole: &str) -> (&str, &str) {
    let mut chars = hole.char_indices().peekable();
    let mut depth = 0u32;
    loop {
        match chars.peek().copied() {
            None => return (hole, ""),
            Some((_, '(' | '[' | '{')) => {
                chars.next();
                depth += 1;
            }
            Some((_, ')' | ']' | '}')) => {
                chars.next();
                depth = depth.saturating_sub(1);
            }
            Some((i, ',' | ':')) if depth == 0 => return (&hole[..i], &hole[i..]),
            Some((pos, '"')) => {
                scan_string(&mut chars, pos, false);
            }
            Some((pos, '@')) if hole[pos..].starts_with("@\"") => {
                chars.next();
                scan_string(&mut chars, pos, true);
            }
            Some((pos, '\'')) => {
                scan_char(&mut chars, pos);
            }
            Some(_) => {
                chars.next();
            }
        }
    }
}

/// A cursor over a token slice, used by the recursive-descent parsers.
#[derive(Clone)]
pub struct Cursor<'a, 'b> {
    tokens: &'b [Token<'a>],
    pos: usize,
}

impl<'a, 'b> Cursor<'a, 'b> {
    pub fn new(tokens: &'b [Token<'a>]) -> Self {
        Self { tokens, pos: 0 }
    }

    pub fn peek(&self) -> Option<Token<'a>> {
        self.tokens.get(self.pos).copied()
    }

    pub fn next(&mut self) -> Option<Token<'a>> {
        let tok = self.peek();
        if tok.is_some() {
            self.pos += 1;
        }
        tok
    }

    pub fn at_end(&self) -> bool {
        self.pos >= self.tokens.len()
    }

    pub fn save(&self) -> usize {
        self.pos
    }

    pub fn restore(&mut self, pos: usize) {
        self.pos = pos;
    }
}

pub fn as_ident<'a>(token: Token<'a>) -> Option<&'a str> {
    match token {
        Token::Ident(s) => Some(s),
        _ => None,
    }
}
