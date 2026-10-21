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
    "==", "!=", "<=", ">=", "&&", "||", "+=", "-=", "*=", "/=", "%=", "++", "--", "=>", "??",
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
fn scan_string(chars: &mut Peekable<CharIndices<'_>>, start: usize, verbatim: bool) -> usize {
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
fn scan_char(chars: &mut Peekable<CharIndices<'_>>, start: usize) -> usize {
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
