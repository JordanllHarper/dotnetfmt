//! whitespace formatting for c# source text.
//!
//! scope for this pass:
//!   - indentation is not touched yet. any leading whitespace on a line is
//!     captured as-is and reattached unchanged; a later step will replace
//!     it with indentation computed from `indent_size`/`indent_style`/
//!     `tab_width`.
//!   - only one construct is recognized: a single-line local variable
//!     declaration of the shape `var <name> = <value>;` (e.g. `var x = 5;`),
//!     however it is spaced in the input (`var x=5;`,
//!     `var                x =               5;`, etc.).
//!   - any line that doesn't match that exact shape is left completely
//!     unchanged. this is deliberate: we only rewrite what we can
//!     confidently understand, and leave everything else (including
//!     constructs like function calls, which are a future step) alone
//!     rather than guessing.
//!   - per the project's assumption, line endings are crlf.

use std::{fmt, iter::peekable};

use crate::editorconfig::editorconfig;

/// errors produced while formatting.
///
/// No cases are produced yet -- this pass only ever recognizes-and-fixes
/// or leaves-untouched, it never rejects input. This type is reserved for
/// once a later construct can be recognized as intended but structurally
/// invalid (e.g. a variable declaration missing its terminating `;`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormatError {}

impl fmt::Display for FormatError {
    fn fmt(&self, _f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {}
    }
}

impl std::error::Error for FormatError {}

/// Formats `contents` (the full text of a `.cs` file) according to
/// `config`.
///
/// `config` is not consulted yet -- it will be once indentation handling
/// is implemented -- but is threaded through now so the public API is
/// stable as that support is added.
pub fn format(contents: &str, _config: &EditorConfig) -> Result<String, FormatError> {
    let formatted = contents
        .split("\r\n")
        .map(format_line)
        .collect::<Vec<_>>()
        .join("\r\n");

    Ok(formatted)
}

/// Formats a single line (no CRLF included), preserving its leading
/// whitespace untouched and leaving the line as-is if it doesn't match a
/// construct we recognize.
fn format_line(line: &str) -> String {
    let indent_len = line.len() - line.trim_start().len();
    let (indent, rest) = line.split_at(indent_len);

    match try_format_variable_declaration(rest) {
        Some(formatted) => format!("{indent}{formatted}"),
        None => line.to_string(),
    }
}

/// A lexical token, classified only as finely as spacing decisions need.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Token<'a> {
    /// A run of ASCII letters, digits, and underscores. This covers both
    /// identifiers/keywords (`var`, `x`) and integer literals (`5`) --
    /// for spacing purposes they behave the same, since neither contains
    /// internal whitespace.
    Atom(&'a str),
    /// Any other single, non-whitespace character (`=`, `;`, ...).
    Symbol(char),
}

fn as_atom(token: Token<'_>) -> Option<&str> {
    match token {
        Token::Atom(s) => Some(s),
        Token::Symbol(_) => None,
    }
}

fn as_symbol(token: Token<'_>) -> Option<char> {
    match token {
        Token::Symbol(c) => Some(c),
        Token::Atom(_) => None,
    }
}

/// Splits a line into tokens, discarding whitespace runs (they only ever
/// act as token separators at this stage).
fn tokenize(line: &str) -> Vec<Token<'_>> {
    let mut tokens = Vec::new();
    let mut chars = line.char_indices().peekable();

    while let Some(&(start, ch)) = chars.peek() {
        if ch.is_whitespace() {
            chars.next();
            continue;
        }

        if is_valid_variable_char(&ch) {
            let initial_end = start + ch.len_utf8();
            chars.next();
            let end = peek_while(
                &mut chars,
                initial_end,
                |(_, c)| is_valid_variable_char(c),
                |(_, item)| item.len_utf8(),
            );
            tokens.push(Token::Atom(&line[start..end]));
        } else {
            tokens.push(Token::Symbol(ch));
            chars.next();
        }
    }

    tokens
}

fn peek_while<I, T: std::iter::Iterator<Item = I>>(
    chars: &mut Peekable<T>,
    initial_end: usize,
    continue_while: fn(&I) -> bool,
    advance_increment: fn(&I) -> usize,
) -> usize {
    let mut end = initial_end;
    while let Some(inner_ch) = chars.peek() {
        if !continue_while(inner_ch) {
            break;
        }
        end += advance_increment(inner_ch);
        chars.next();
    }

    end
}

fn is_valid_variable_char(c: &char) -> bool {
    c.is_ascii_alphanumeric() || *c == '_'
}

/// Recognizes `var <name> = <value>;` (exactly five tokens) in whatever
/// spacing it appears, and rewrites it with normalized spacing: a single
/// space between tokens, no space before `;`. Returns `None` for anything
/// else, including lines that merely start with `var` but don't fully
/// match this shape -- that's left untouched rather than guessed at.
fn try_format_variable_declaration(rest: &str) -> Option<String> {
    let tokens = tokenize(rest);
    if tokens.len() != 5 {
        return None;
    }

    let keyword = as_atom(tokens[0])?;
    let name = as_atom(tokens[1])?;
    let eq = as_symbol(tokens[2])?;
    let value = as_atom(tokens[3])?;
    let semi = as_symbol(tokens[4])?;

    if keyword != "var" || eq != '=' || semi != ';' {
        return None;
    }

    Some(format!("var {name} = {value};"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editorconfig::{IndentSize, IndentStyle};

    fn config() -> EditorConfig {
        EditorConfig {
            indent_style: IndentStyle::Space,
            indent_size: IndentSize::Columns(4),
            tab_width: 4,
        }
    }

    #[test]
    fn already_formatted_is_unchanged() {
        let result = format("var x = 5;", &config()).unwrap();
        assert_eq!(result, "var x = 5;");
    }

    #[test]
    fn collapses_missing_spaces() {
        let result = format("var x=5;", &config()).unwrap();
        assert_eq!(result, "var x = 5;");
    }

    #[test]
    fn collapses_excess_whitespace() {
        let result = format("var                x =               5;", &config()).unwrap();
        assert_eq!(result, "var x = 5;");
    }

    #[test]
    fn tabs_are_treated_as_whitespace_too() {
        let result = format("var\tx\t=\t5;", &config()).unwrap();
        assert_eq!(result, "var x = 5;");
    }

    #[test]
    fn leading_indentation_is_preserved_untouched() {
        let result = format("    var x    =    5;", &config()).unwrap();
        assert_eq!(result, "    var x = 5;");
    }

    #[test]
    fn unrecognized_lines_are_left_alone() {
        let result = format("Foo(a, b);", &config()).unwrap();
        assert_eq!(result, "Foo(a, b);");
    }

    #[test]
    fn multiple_lines_are_joined_with_crlf() {
        let input = "var x=5;\r\nvar y   =   10;";
        let result = format(input, &config()).unwrap();
        assert_eq!(result, "var x = 5;\r\nvar y = 10;");
    }
}
