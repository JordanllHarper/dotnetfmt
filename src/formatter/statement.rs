//! Statement-level formatting: variable declarations and method
//! signatures, both built on top of the type parser and the
//! expression parser in [`super::expr`].

use crate::formatter::expr::{parse_expression, render};
use crate::formatter::token::{Cursor, Token, as_ident, tokenize};

/// Modifiers recognized ahead of a method's return type. Not
/// exhaustive -- just the common ones a "basic" formatter should not
/// choke on.
const MODIFIERS: &[&str] = &[
    "public",
    "private",
    "protected",
    "internal",
    "static",
    "virtual",
    "override",
    "sealed",
    "abstract",
    "async",
    "new",
    "partial",
    "extern",
    "unsafe",
    "readonly",
];

/// Parses a type reference: a base identifier, an optional generic
/// argument list (`<T, U>`), and optional trailing array rank
/// specifiers (`[]`, `[,]`, ...). Renders it back in canonical form.
///
/// This only handles "type position" parsing (called where the
/// grammar already expects a type), so treating `<` as opening
/// generic arguments rather than a relational operator is safe.
fn parse_type(c: &mut Cursor) -> Option<String> {
    let mut rendered = as_ident(c.next()?)?.to_string();

    if matches!(c.peek(), Some(Token::Symbol('<'))) {
        c.next();
        let mut args = Vec::new();
        loop {
            args.push(parse_type(c)?);
            match c.peek() {
                Some(Token::Symbol(',')) => {
                    c.next();
                }
                _ => break,
            }
        }
        match c.next()? {
            Token::Symbol('>') => {}
            _ => return None,
        }
        rendered = format!("{rendered}<{}>", args.join(", "));
    }

    loop {
        if !matches!(c.peek(), Some(Token::Symbol('['))) {
            break;
        }
        let checkpoint = c.save();
        c.next();
        let mut commas = 0;
        while matches!(c.peek(), Some(Token::Symbol(','))) {
            c.next();
            commas += 1;
        }
        if matches!(c.peek(), Some(Token::Symbol(']'))) {
            c.next();
            rendered = format!("{rendered}[{}]", ",".repeat(commas));
        } else {
            c.restore(checkpoint);
            break;
        }
    }

    Some(rendered)
}

/// Formats a variable declaration statement, e.g. `int x=1,y=2` or
/// `var x=someFunction()`. `tokens` must not include the trailing
/// `;`. Returns `None` (rather than guessing) for anything that
/// isn't unambiguously a declaration -- notably, it requires a type
/// followed directly by an identifier, which an expression statement
/// never has.
pub fn try_format_declaration(tokens: &[Token]) -> Option<String> {
    let mut c = Cursor::new(tokens);
    let type_name = parse_type(&mut c)?;

    if !matches!(c.peek(), Some(Token::Ident(_))) {
        return None;
    }

    let mut declarators = Vec::new();
    loop {
        let name = as_ident(c.next()?)?.to_string();
        let declarator = if matches!(c.peek(), Some(Token::Symbol('='))) {
            c.next();
            let value = parse_expression(&mut c)?;
            format!("{name} = {}", render(&value))
        } else {
            name
        };
        declarators.push(declarator);

        match c.peek() {
            Some(Token::Symbol(',')) => {
                c.next();
            }
            None => break,
            _ => return None,
        }
    }

    if !c.at_end() {
        return None;
    }

    Some(format!("{type_name} {};", declarators.join(", ")))
}

/// Formats an expression-statement, e.g. `Foo(a,b)` or `x+=1`.
/// `tokens` must not include the trailing `;`.
pub fn try_format_expression_statement(tokens: &[Token]) -> Option<String> {
    let mut c = Cursor::new(tokens);
    let expr = parse_expression(&mut c)?;
    if !c.at_end() {
        return None;
    }
    Some(format!("{};", render(&expr)))
}

/// Formats a method signature: `modifiers* type name(type name, ...)`,
/// with an optional `= default` on each parameter. `tokens` must not
/// include a trailing `{`, if the source had one -- the caller
/// reattaches it.
fn try_format_method_signature(tokens: &[Token]) -> Option<String> {
    let mut c = Cursor::new(tokens);

    let mut modifiers = Vec::new();
    loop {
        let checkpoint = c.save();
        match c.peek().and_then(as_ident) {
            Some(word) if MODIFIERS.contains(&word) => {
                c.next();
                modifiers.push(word.to_string());
            }
            _ => {
                c.restore(checkpoint);
                break;
            }
        }
    }

    let return_type = parse_type(&mut c)?;
    let name = as_ident(c.next()?)?.to_string();

    match c.next()? {
        Token::Symbol('(') => {}
        _ => return None,
    }

    let mut params = Vec::new();
    if !matches!(c.peek(), Some(Token::Symbol(')'))) {
        loop {
            let param_type = parse_type(&mut c)?;
            let param_name = as_ident(c.next()?)?.to_string();
            let param = if matches!(c.peek(), Some(Token::Symbol('='))) {
                c.next();
                let default = parse_expression(&mut c)?;
                format!("{param_type} {param_name} = {}", render(&default))
            } else {
                format!("{param_type} {param_name}")
            };
            params.push(param);

            match c.peek() {
                Some(Token::Symbol(',')) => {
                    c.next();
                }
                _ => break,
            }
        }
    }

    match c.next()? {
        Token::Symbol(')') => {}
        _ => return None,
    }

    if !c.at_end() {
        return None;
    }

    let modifiers = modifiers
        .iter()
        .map(|m| format!("{m} "))
        .collect::<String>();
    Some(format!(
        "{modifiers}{return_type} {name}({})",
        params.join(", ")
    ))
}

/// Formats a class declaration: `modifiers* class Name<T, ...>? (:
/// Base, ...)?`. `tokens` must not include a trailing `{`, if the
/// source had one -- the caller reattaches it.
fn try_format_class_declaration(tokens: &[Token]) -> Option<String> {
    let mut c = Cursor::new(tokens);

    let mut modifiers = Vec::new();
    loop {
        let checkpoint = c.save();
        match c.peek().and_then(as_ident) {
            Some(word) if MODIFIERS.contains(&word) => {
                c.next();
                modifiers.push(word.to_string());
            }
            _ => {
                c.restore(checkpoint);
                break;
            }
        }
    }

    if as_ident(c.next()?) != Some("class") {
        return None;
    }

    let name = parse_type(&mut c)?;

    let mut bases = Vec::new();
    if matches!(c.peek(), Some(Token::Symbol(':'))) {
        c.next();
        loop {
            bases.push(parse_type(&mut c)?);
            match c.peek() {
                Some(Token::Symbol(',')) => {
                    c.next();
                }
                _ => break,
            }
        }
    }

    if !c.at_end() {
        return None;
    }

    let modifiers = modifiers
        .iter()
        .map(|m| format!("{m} "))
        .collect::<String>();
    let base_list = if bases.is_empty() {
        String::new()
    } else {
        format!(" : {}", bases.join(", "))
    };
    Some(format!("{modifiers}class {name}{base_list}"))
}

/// Formats a `keyword (condition)` statement header -- the shape
/// shared by `if` and `while`. `tokens` must not include a trailing
/// `{`, if the source had one -- the caller reattaches it.
fn try_format_paren_condition(keyword: &'static str, tokens: &[Token]) -> Option<String> {
    let mut c = Cursor::new(tokens);

    if as_ident(c.next()?) != Some(keyword) {
        return None;
    }

    if !matches!(c.next()?, Token::Symbol('(')) {
        return None;
    }

    let condition = parse_expression(&mut c)?;

    if !matches!(c.next()?, Token::Symbol(')')) {
        return None;
    }

    if !c.at_end() {
        return None;
    }

    Some(format!("{keyword} ({})", render(&condition)))
}

/// Formats an `if` statement's header: `if (condition)`. `tokens`
/// must not include a trailing `{`, if the source had one -- the
/// caller reattaches it.
fn try_format_if_statement(tokens: &[Token]) -> Option<String> {
    try_format_paren_condition("if", tokens)
}

/// Formats a `while` statement's header: `while (condition)`.
/// `tokens` must not include a trailing `{`, if the source had one --
/// the caller reattaches it.
fn try_format_while_statement(tokens: &[Token]) -> Option<String> {
    try_format_paren_condition("while", tokens)
}

/// Formats an `else` statement's header: `else` or `else if
/// (condition)`. `tokens` must not include a trailing `{`, if the
/// source had one -- the caller reattaches it.
fn try_format_else_statement(tokens: &[Token]) -> Option<String> {
    let mut c = Cursor::new(tokens);

    if as_ident(c.next()?) != Some("else") {
        return None;
    }

    if c.at_end() {
        return Some("else".to_string());
    }

    let if_header = try_format_if_statement(&tokens[1..])?;
    Some(format!("else {if_header}"))
}

/// Formats a `do` statement's header: the bare `do` keyword that
/// opens a `do`/`while` loop. `tokens` must not include a trailing
/// `{`, if the source had one -- the caller reattaches it.
fn try_format_do_statement(tokens: &[Token]) -> Option<String> {
    let mut c = Cursor::new(tokens);

    if as_ident(c.next()?) != Some("do") {
        return None;
    }

    if !c.at_end() {
        return None;
    }

    Some("do".to_string())
}

/// Formats the closing `while (condition);` of a `do`/`while` loop.
/// `tokens` must not include the trailing `;`.
fn try_format_do_while_tail(tokens: &[Token]) -> Option<String> {
    let header = try_format_paren_condition("while", tokens)?;
    Some(format!("{header};"))
}

/// Formats the combined `} while (condition);` that closes a
/// `do`/`while` loop when `while` shares a line with the closing
/// brace. `tokens` must not include the trailing `;`.
fn try_format_do_while_close(tokens: &[Token]) -> Option<String> {
    if !matches!(tokens.first(), Some(Token::Symbol('}'))) {
        return None;
    }
    let tail = try_format_do_while_tail(&tokens[1..])?;
    Some(format!("}} {tail}"))
}

/// Formats a single logical line of statement content (already
/// stripped of leading/trailing whitespace and any indentation).
/// Returns `None` when nothing about the content is recognized, in
/// which case the caller leaves the original text untouched.
pub fn format_statement_content(content: &str) -> Option<String> {
    let tokens = tokenize(content);
    if tokens.is_empty() {
        return None;
    }

    if matches!(tokens.last(), Some(Token::Symbol(';'))) {
        let body = &tokens[..tokens.len() - 1];
        return try_format_declaration(body)
            .or_else(|| try_format_do_while_tail(body))
            .or_else(|| try_format_do_while_close(body))
            .or_else(|| try_format_expression_statement(body));
    }

    if tokens.len() == 1 && matches!(tokens[0], Token::Symbol('{') | Token::Symbol('}')) {
        return None;
    }

    let (sig_tokens, trailing_brace) = match tokens.last() {
        Some(Token::Symbol('{')) => (&tokens[..tokens.len() - 1], true),
        _ => (&tokens[..], false),
    };

    let signature = try_format_method_signature(sig_tokens)
        .or_else(|| try_format_class_declaration(sig_tokens))
        .or_else(|| try_format_if_statement(sig_tokens))
        .or_else(|| try_format_else_statement(sig_tokens))
        .or_else(|| try_format_while_statement(sig_tokens))
        .or_else(|| try_format_do_statement(sig_tokens))?;
    Some(if trailing_brace {
        format!("{signature} {{")
    } else {
        signature
    })
}
