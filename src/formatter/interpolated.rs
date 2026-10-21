//! Reformatting for interpolated string literals (`$"..."`,
//! `$@"..."`/`@$"..."`). `token.rs` lexes the whole literal as a
//! single opaque `Token::InterpolatedStr`; this module locates its
//! `{ .. }` holes (via `token::scan_interpolated`) and reformats each
//! hole's expression through the normal tokenize/parse/render
//! pipeline, splicing the result back into the surrounding text.
//! Literal text segments, escaped `{{`/`}}`, and alignment/format
//! specifiers are left untouched. A hole that doesn't parse as a
//! supported expression -- or doesn't fully consume its tokens -- is
//! itself left untouched rather than failing the whole string, per
//! the project's "recognize-and-fix, or leave alone" rule.

use super::expr::{parse_expression, render};
use super::token::{Cursor, scan_interpolated, split_hole, tokenize};

/// Reformats the holes inside an interpolated string literal `raw`
/// (the full token text, prefix and quotes included), leaving
/// everything else untouched.
pub fn format_interpolated_string(raw: &str) -> String {
    let (_, holes) = scan_interpolated(raw);
    let mut result = String::with_capacity(raw.len());
    let mut pos = 0;
    for (start, end) in holes {
        result.push_str(&raw[pos..start]);
        result.push_str(&format_hole(&raw[start..end]));
        pos = end;
    }
    result.push_str(&raw[pos..]);
    result
}

/// Reformats a single hole's text (between its braces, not including
/// them): the expression part is reformatted; any trailing
/// alignment/format-specifier text is preserved verbatim.
fn format_hole(hole: &str) -> String {
    let (expr_text, suffix) = split_hole(hole);
    format!("{}{suffix}", format_hole_expression(expr_text))
}

/// Reformats `expr_text` via the normal tokenize/parse/render
/// pipeline, falling back to the original text unchanged if it
/// doesn't parse as a supported expression or leaves tokens
/// unconsumed.
fn format_hole_expression(expr_text: &str) -> String {
    let tokens = tokenize(expr_text);
    let mut cursor = Cursor::new(&tokens);
    match parse_expression(&mut cursor) {
        Some(expr) if cursor.at_end() => render(&expr),
        _ => expr_text.to_string(),
    }
}
