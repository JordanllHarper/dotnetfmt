//! Whitespace formatting for C# source text.
//!
//! Scope for this pass:
//!   - Indentation is not touched yet. Any leading whitespace on a line is
//!     captured as-is and reattached unchanged; a later step will replace
//!     it with indentation computed from `indent_size`/`indent_style`/
//!     `tab_width`.
//!   - Only one statement shape is recognized: a single-line local
//!     variable declaration, `var <name> = <value>;` (e.g. `var x = 5;`),
//!     however it is spaced in the input (`var x=5;`,
//!     `var                x =               5;`, etc.). `<value>` may be
//!     a bare atom (`5`, `someValue`), or a function call with zero or one
//!     argument (`someFunction()`, `someFunction(a)`, however spaced) --
//!     a call with more than one argument is not recognized yet.
//!   - Any line that doesn't match that exact shape is left completely
//!     unchanged. This is deliberate: we only rewrite what we can
//!     confidently understand, and leave everything else alone rather
//!     than guessing.
//!   - Per the project's assumption, line endings are CRLF.

use std::{fmt, iter::Peekable};

use crate::editorconfig::EditorConfig;

/// Errors produced while formatting.
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

/// Recognizes `var <name> = <value>;` in whatever spacing it appears, and
/// rewrites it with normalized spacing: a single space between tokens, no
/// space before `;`, no space inside `()`. Returns `None` for anything
/// else, including lines that merely start with `var` but don't fully
/// match a recognized shape -- that's left untouched rather than guessed
/// at.
fn try_format_variable_declaration(rest: &str) -> Option<String> {
    let tokens = tokenize(rest);
    if tokens.len() < 5 {
        return None;
    }

    let keyword = as_atom(tokens[0])?;
    let name = as_atom(tokens[1])?;
    let eq = as_symbol(tokens[2])?;
    let semi = as_symbol(tokens[tokens.len() - 1])?;

    if keyword != "var" || eq != '=' || semi != ';' {
        return None;
    }

    let value = format_value(&tokens[3..tokens.len() - 1])?;

    Some(format!("var {name} = {value};"))
}

/// Recognizes a `<value>` expression (the right-hand side of `=` in a
/// variable declaration): a bare atom (`5`, `someValue`), or a function
/// call with zero or one argument (`someFunction()`, `someFunction(a)`,
/// however spaced -- the argument itself must be a bare atom, not a
/// nested expression). Returns its normalized text, or `None` if
/// `value_tokens` matches none of those shapes.
fn format_value(value_tokens: &[Token]) -> Option<String> {
    match value_tokens {
        [atom] => Some(as_atom(*atom)?.to_string()),
        [name, open, close] => {
            let name = as_atom(*name)?;
            if as_symbol(*open)? == '(' && as_symbol(*close)? == ')' {
                Some(format!("{name}()"))
            } else {
                None
            }
        }
        [name, open, args @ .., close] => {
            let name = as_atom(*name)?;
            let formatted_args = args
                .split(|token| matches!(token, Token::Symbol(',')))
                .map(to_formatted_arguments)
                .collect::<Option<Vec<String>>>()?
                .join(", ");

            if as_symbol(*open)? == '(' && as_symbol(*close)? == ')' {
                Some(format!("{name}({formatted_args})"))
            } else {
                None
            }
        }
        _ => None,
    }
}

fn to_formatted_arguments(tokens: &[Token]) -> Option<String> {
    match tokens {
        [atom] => Some(as_atom(*atom)?.to_string()),
        [left, Token::Symbol('.'), right] => {
            let left = as_atom(*left)?;
            let right = as_atom(*right)?;
            Some(format!("{left}.{right}"))
        }
        [left, op, right] => {
            let left = as_atom(*left)?;
            let right = as_atom(*right)?;
            let op = as_symbol(*op)?;

            Some(format!("{left} {op} {right}"))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editorconfig::{EditorConfig, IndentSize, IndentStyle};

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
    fn formats_empty_function_call_value() {
        let result = format("var x=someFunction();", &config()).unwrap();
        assert_eq!(result, "var x = someFunction();");
    }

    #[test]
    fn collapses_whitespace_around_empty_call_parens() {
        let result = format("var x = someFunction (   )   ;", &config()).unwrap();
        assert_eq!(result, "var x = someFunction();");
    }

    #[test]
    fn formats_single_argument_function_call_value() {
        let result = format("var x=someFunction(a);", &config()).unwrap();
        assert_eq!(result, "var x = someFunction(a);");
    }

    #[test]
    fn collapses_whitespace_around_single_argument() {
        let result = format("var x = someFunction(a     );", &config()).unwrap();
        assert_eq!(result, "var x = someFunction(a);");
    }

    #[test]
    fn collapses_whitespace_around_multiple_arguments() {
        let result = format(
            "var x = someFunction(            a       ,        b       );",
            &config(),
        )
        .unwrap();
        assert_eq!(result, "var x = someFunction(a, b);");
    }

    #[test]
    fn expands_whitespace_around_multiple_arguments() {
        let result = format("var x = someFunction(a,b);", &config()).unwrap();
        assert_eq!(result, "var x = someFunction(a, b);");
    }

    #[test]
    fn formats_three_or_more_arguments() {
        let result = format("var x = someFunction(a,   b ,c);", &config()).unwrap();
        assert_eq!(result, "var x = someFunction(a, b, c);");
    }

    #[test]
    fn arithmetic_expression_argument_is_not_recognized() {
        // Regression test: an earlier implementation of multi-argument
        // support panicked here instead of leaving the line untouched,
        // because it assumed every non-comma token between the parens
        // was a bare atom.
        let result = format("var x = someFunction(a + b);", &config()).unwrap();
        assert_eq!(result, "var x = someFunction(a + b);");
    }

    #[test]
    fn member_access_argument_is_recognized() {
        let result = format("var x = someFunction(a.b, c);", &config()).unwrap();
        assert_eq!(result, "var x = someFunction(a.b, c);");
    }

    #[test]
    fn collapses_whitespace_around_multiple_arguments_with_member_access() {
        let result = format("var x = someFunction   (a    .    b   , c)  ;", &config()).unwrap();
        assert_eq!(result, "var x = someFunction(a.b, c);");
    }

    #[test]
    fn expands_whitespace_around_multiple_arguments_with_member_access() {
        let result = format("var x=someFunction(a.b,c);", &config()).unwrap();
        assert_eq!(result, "var x = someFunction(a.b, c);");
    }

    #[test]
    fn missing_comma_between_arguments_is_not_recognized() {
        let result = format("var x = someFunction(a b);", &config()).unwrap();
        assert_eq!(result, "var x = someFunction(a b);");
    }

    #[test]
    fn trailing_comma_is_not_recognized() {
        let result = format("var x = someFunction(a, b,);", &config()).unwrap();
        assert_eq!(result, "var x = someFunction(a, b,);");
    }

    #[test]
    fn leading_comma_is_not_recognized() {
        let result = format("var x = someFunction(, a);", &config()).unwrap();
        assert_eq!(result, "var x = someFunction(, a);");
    }

    #[test]
    fn doubled_comma_is_not_recognized() {
        let result = format("var x = someFunction(a,, b);", &config()).unwrap();
        assert_eq!(result, "var x = someFunction(a,, b);");
    }

    #[test]
    fn multiple_lines_are_joined_with_crlf() {
        let input = "var x=5;\r\nvar y   =   10;";
        let result = format(input, &config()).unwrap();
        assert_eq!(result, "var x = 5;\r\nvar y = 10;");
    }
}
