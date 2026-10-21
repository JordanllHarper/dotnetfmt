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
///
/// `pub(super)`: also used by `expr.rs`'s `new Type(...)` parsing,
/// where `<` is likewise unambiguous (only ever a generic-argument
/// opener in the type position right after `new`).
pub(super) fn parse_type(c: &mut Cursor) -> Option<String> {
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

/// Formats a variable declaration's type and declarator list, e.g.
/// `int x=1,y=2` -> `int x = 1, y = 2`, without a trailing `;`.
/// Returns `None` (rather than guessing) for anything that isn't
/// unambiguously a declaration -- notably, it requires a type
/// followed directly by an identifier, which an expression statement
/// never has.
fn try_format_declarators(tokens: &[Token]) -> Option<String> {
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

    Some(format!("{type_name} {}", declarators.join(", ")))
}

/// Formats a variable declaration statement, e.g. `int x=1,y=2` or
/// `var x=someFunction()`. `tokens` must not include the trailing
/// `;`.
pub fn try_format_declaration(tokens: &[Token]) -> Option<String> {
    Some(format!("{};", try_format_declarators(tokens)?))
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
/// with an optional `= default` on each parameter. Also recognizes a
/// constructor (`modifiers* name(...)`, no return type) and a generic
/// method's type parameter list (`modifiers* type name<T, ...>(...)`).
/// `tokens` must not include a trailing `{`, if the source had one --
/// the caller reattaches it.
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

    // A constructor has no return type -- just its (class) name
    // directly before `(`. Distinguishing the two doesn't need any
    // knowledge of the enclosing class: an ordinary method always has
    // *two* identifiers before `(` (return type, then name), so
    // seeing `(` right after the first one means it was the name all
    // along, not a return type.
    let first = parse_type(&mut c)?;
    let (return_type, name) = if matches!(c.peek(), Some(Token::Symbol('('))) {
        (None, first)
    } else {
        (Some(first), as_ident(c.next()?)?.to_string())
    };

    let type_params = if matches!(c.peek(), Some(Token::Symbol('<'))) {
        Some(parse_type_parameter_list(&mut c)?)
    } else {
        None
    };

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
    let return_type = if let Some(rt) = return_type {
        format!("{rt} ")
    } else {
        String::new()
    };
    let type_params = if let Some(params) = type_params {
        format!("<{}>", params.join(", "))
    } else {
        String::new()
    };
    Some(format!(
        "{modifiers}{return_type}{name}{type_params}({})",
        params.join(", ")
    ))
}

/// Parses a method's generic type parameter list, `<T, U>`, with the
/// opening `<` not yet consumed. Unlike a type *argument* list (see
/// [`parse_type`]), each type *parameter* is a plain identifier --
/// it's declaring a new name, not referencing an existing type, so it
/// can't itself be generic.
fn parse_type_parameter_list(c: &mut Cursor) -> Option<Vec<String>> {
    c.next(); // '<'
    let mut params = Vec::new();
    loop {
        params.push(as_ident(c.next()?)?.to_string());
        match c.peek() {
            Some(Token::Symbol(',')) => {
                c.next();
            }
            _ => break,
        }
    }
    match c.next()? {
        Token::Symbol('>') => Some(params),
        _ => None,
    }
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

/// Formats a `switch` statement's header: `switch (expression)`.
/// `tokens` must not include a trailing `{`, if the source had one --
/// the caller reattaches it.
fn try_format_switch_statement(tokens: &[Token]) -> Option<String> {
    try_format_paren_condition("switch", tokens)
}

/// Words that are contextual keywords in pattern position and so
/// can't double as a type pattern's binding identifier -- e.g. `case
/// int and > 0:` must keep `and` available to [`parse_and_pattern`]
/// rather than letting the type pattern swallow it as a binding named
/// `and`.
const PATTERN_KEYWORDS: &[&str] = &["when", "and", "or", "not"];

/// Parses a single relational/type/expression pattern -- the base
/// case of the pattern grammar, with no `and`/`or`/`not`/`when`.
/// Recognizes, in order:
///
///   - a **relational pattern** (`> 5`, `<= 0`): a bare relational
///     operator followed by a constant expression;
///   - a **type pattern** (`int i`, `List<int> items`): a type
///     (reusing [`parse_type`]) immediately followed by a binding
///     identifier (or `_` discard) -- tried with backtracking, since
///     a leading identifier could just as easily start a plain
///     expression pattern instead (`Color.Red`, `someConstant`);
///   - otherwise, a plain **expression pattern** (literal, identifier,
///     member access, or `_`).
fn parse_primary_pattern(c: &mut Cursor) -> Option<String> {
    let relational_op = match c.peek() {
        Some(Token::Symbol(op @ ('<' | '>'))) => Some(op.to_string()),
        Some(Token::Op(op @ ("<=" | ">="))) => Some(op.to_string()),
        _ => None,
    };

    if let Some(op) = relational_op {
        c.next();
        return Some(format!("{op} {}", render(&parse_expression(c)?)));
    }

    let checkpoint = c.save();
    let type_pattern = parse_type(c).and_then(|type_name| match c.peek() {
        Some(Token::Ident(name)) if !PATTERN_KEYWORDS.contains(&name) => {
            let name = name.to_string();
            c.next();
            Some(format!("{type_name} {name}"))
        }
        _ => None,
    });

    match type_pattern {
        Some(pattern) => Some(pattern),
        None => {
            c.restore(checkpoint);
            Some(render(&parse_expression(c)?))
        }
    }
}

/// Parses a `not` pattern (`not null`), or falls through to a
/// primary pattern. Right-associative, so `not not p` parses as
/// `not (not p)`.
fn parse_not_pattern(c: &mut Cursor) -> Option<String> {
    if matches!(c.peek(), Some(Token::Ident("not"))) {
        c.next();
        let operand = parse_not_pattern(c)?;
        return Some(format!("not {operand}"));
    }
    parse_primary_pattern(c)
}

/// Parses `and`-combined patterns (`int i and > 0`); `and` binds
/// tighter than `or`, matching C#'s pattern-combinator precedence.
fn parse_and_pattern(c: &mut Cursor) -> Option<String> {
    let mut left = parse_not_pattern(c)?;
    while matches!(c.peek(), Some(Token::Ident("and"))) {
        c.next();
        let right = parse_not_pattern(c)?;
        left = format!("{left} and {right}");
    }
    Some(left)
}

/// Parses `or`-combined patterns (`> 0 or < -10`), optionally
/// followed by a `when` guard clause -- the full pattern grammar
/// shared by `case` labels and `switch` expression arms.
fn parse_switch_pattern(c: &mut Cursor) -> Option<String> {
    let mut left = parse_and_pattern(c)?;
    while matches!(c.peek(), Some(Token::Ident("or"))) {
        c.next();
        let right = parse_and_pattern(c)?;
        left = format!("{left} or {right}");
    }

    if matches!(c.peek(), Some(Token::Ident("when"))) {
        c.next();
        let guard = parse_expression(c)?;
        return Some(format!("{left} when {}", render(&guard)));
    }

    Some(left)
}

/// Formats a `case` label: `case pattern:`, where `pattern` may
/// include a `when` guard (see [`parse_switch_pattern`]). `tokens`
/// must not include the trailing `:`.
fn try_format_case_label(tokens: &[Token]) -> Option<String> {
    let mut c = Cursor::new(tokens);

    if as_ident(c.next()?) != Some("case") {
        return None;
    }

    let pattern = parse_switch_pattern(&mut c)?;

    if !c.at_end() {
        return None;
    }

    Some(format!("case {pattern}:"))
}

/// Formats a bare `default:` label. `tokens` must not include the
/// trailing `:`.
fn try_format_default_label(tokens: &[Token]) -> Option<String> {
    let mut c = Cursor::new(tokens);

    if as_ident(c.next()?) != Some("default") {
        return None;
    }

    if !c.at_end() {
        return None;
    }

    Some("default:".to_string())
}

/// Formats the header line of a declaration initialized by a
/// `switch` expression: `type name = expr switch`. Only a single
/// declarator is supported. The expression's arms (see
/// [`try_format_switch_arm`]) and closing `};` live on their own
/// lines and aren't parsed here. `tokens` must not include a trailing
/// `{`, if the source had one -- the caller reattaches it.
fn try_format_switch_expression_header(tokens: &[Token]) -> Option<String> {
    let mut c = Cursor::new(tokens);

    let type_name = parse_type(&mut c)?;
    let name = as_ident(c.next()?)?.to_string();

    if !matches!(c.next()?, Token::Symbol('=')) {
        return None;
    }

    let scrutinee = parse_expression(&mut c)?;

    if as_ident(c.next()?) != Some("switch") {
        return None;
    }

    if !c.at_end() {
        return None;
    }

    Some(format!(
        "{type_name} {name} = {} switch",
        render(&scrutinee)
    ))
}

/// Formats one arm of a `switch` expression: `pattern => result`,
/// where `pattern` may include a `when` guard (see
/// [`parse_switch_pattern`]). `tokens` must not include the trailing
/// `,`, if the source had one -- the caller reattaches it.
fn try_format_switch_arm(tokens: &[Token]) -> Option<String> {
    let mut c = Cursor::new(tokens);
    let arm = parse_switch_arm(&mut c)?;
    if !c.at_end() {
        return None;
    }
    Some(arm)
}

/// Parses one `switch` arm's `pattern => result` (see
/// [`parse_switch_pattern`]), without requiring the cursor to be
/// fully consumed afterward -- shared by [`try_format_switch_arm`]
/// (one arm per line) and [`try_format_inline_switch_expression`]
/// (all arms on one line).
fn parse_switch_arm(c: &mut Cursor) -> Option<String> {
    let pattern = parse_switch_pattern(c)?;
    if !matches!(c.next()?, Token::Op("=>")) {
        return None;
    }
    let result = parse_expression(c)?;
    Some(format!("{pattern} => {}", render(&result)))
}

/// Formats a fully inline declaration whose initializer is a
/// complete `switch` expression, all on one line: `type name = expr
/// switch { pattern => result, ... };`. Only a single declarator is
/// supported, and only in a declaration's initializer position --
/// `return`/argument position and multi-line arm lists aren't
/// recognized here (see [`try_format_switch_expression_header`] for
/// the multi-line form). `tokens` must not include the trailing `;`.
fn try_format_inline_switch_expression(tokens: &[Token]) -> Option<String> {
    let mut c = Cursor::new(tokens);

    let type_name = parse_type(&mut c)?;
    let name = as_ident(c.next()?)?.to_string();

    if !matches!(c.next()?, Token::Symbol('=')) {
        return None;
    }

    let scrutinee = parse_expression(&mut c)?;

    if as_ident(c.next()?) != Some("switch") {
        return None;
    }

    if !matches!(c.next()?, Token::Symbol('{')) {
        return None;
    }

    let mut arms = Vec::new();
    if !matches!(c.peek(), Some(Token::Symbol('}'))) {
        loop {
            arms.push(parse_switch_arm(&mut c)?);

            match c.peek() {
                Some(Token::Symbol(',')) => {
                    c.next();
                    if matches!(c.peek(), Some(Token::Symbol('}'))) {
                        break;
                    }
                }
                _ => break,
            }
        }
    }

    if !matches!(c.next()?, Token::Symbol('}')) {
        return None;
    }

    if !c.at_end() {
        return None;
    }

    Some(format!(
        "{type_name} {name} = {} switch {{ {} }};",
        render(&scrutinee),
        arms.join(", ")
    ))
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

/// Formats a `foreach` statement's header: `foreach (type name in
/// collection)`. `tokens` must not include a trailing `{`, if the
/// source had one -- the caller reattaches it.
fn try_format_foreach_statement(tokens: &[Token]) -> Option<String> {
    let mut c = Cursor::new(tokens);

    if as_ident(c.next()?) != Some("foreach") {
        return None;
    }

    if !matches!(c.next()?, Token::Symbol('(')) {
        return None;
    }

    let element_type = parse_type(&mut c)?;
    let name = as_ident(c.next()?)?.to_string();

    if as_ident(c.next()?) != Some("in") {
        return None;
    }

    let collection = parse_expression(&mut c)?;

    if !matches!(c.next()?, Token::Symbol(')')) {
        return None;
    }

    if !c.at_end() {
        return None;
    }

    Some(format!(
        "foreach ({element_type} {name} in {})",
        render(&collection)
    ))
}

/// Splits a `for` header's inner tokens (everything after the
/// opening `(`) into its `init` and `condition` segments, plus
/// everything after the second `;` (the increment list, followed by
/// the header's closing `)`). `;` can't appear inside an expression
/// or type in C#, so a plain linear scan (no paren/bracket depth
/// tracking) safely finds the header's own top-level separators.
fn split_for_header<'a, 'b>(
    tokens: &'b [Token<'a>],
) -> Option<(&'b [Token<'a>], &'b [Token<'a>], &'b [Token<'a>])> {
    let first_semi = tokens
        .iter()
        .position(|t| matches!(t, Token::Symbol(';')))?;
    let (init, after_init) = (&tokens[..first_semi], &tokens[first_semi + 1..]);

    let second_semi = after_init
        .iter()
        .position(|t| matches!(t, Token::Symbol(';')))?;
    let (condition, after_condition) = (&after_init[..second_semi], &after_init[second_semi + 1..]);

    Some((init, condition, after_condition))
}

/// Formats a `for` loop's optional init segment: empty, or a
/// declaration (reusing [`try_format_declarators`]).
fn format_for_init(tokens: &[Token]) -> Option<String> {
    if tokens.is_empty() {
        return Some(String::new());
    }
    try_format_declarators(tokens)
}

/// Formats a `for` loop's optional condition segment: empty, or a
/// single expression that must consume the whole segment.
fn format_for_condition(tokens: &[Token]) -> Option<String> {
    if tokens.is_empty() {
        return Some(String::new());
    }
    let mut c = Cursor::new(tokens);
    let expr = parse_expression(&mut c)?;
    if !c.at_end() {
        return None;
    }
    Some(render(&expr))
}

/// Formats a `for` loop's comma-separated increment list and consumes
/// the header's closing `)`. `tokens` starts right after the second
/// `;` and must contain nothing but the increment expressions
/// followed by `)`.
fn format_for_increment(tokens: &[Token]) -> Option<String> {
    let mut c = Cursor::new(tokens);

    let mut increments = Vec::new();
    if !matches!(c.peek(), Some(Token::Symbol(')'))) {
        loop {
            increments.push(render(&parse_expression(&mut c)?));
            match c.peek() {
                Some(Token::Symbol(',')) => {
                    c.next();
                }
                _ => break,
            }
        }
    }

    if !matches!(c.next()?, Token::Symbol(')')) {
        return None;
    }
    if !c.at_end() {
        return None;
    }

    Some(increments.join(", "))
}

/// Formats a C-style `for` statement's header: `for (init;
/// condition; increment)`, where `init` is an optional declaration
/// and `increment` is a comma-separated list of expressions; any of
/// the three parts may be empty (e.g. `for (;;)`). `tokens` must not
/// include a trailing `{`, if the source had one -- the caller
/// reattaches it.
fn try_format_for_statement(tokens: &[Token]) -> Option<String> {
    let mut c = Cursor::new(tokens);

    if as_ident(c.next()?) != Some("for") {
        return None;
    }

    if !matches!(c.next()?, Token::Symbol('(')) {
        return None;
    }

    let (init_tokens, condition_tokens, increment_tokens) = split_for_header(&tokens[c.save()..])?;

    let init = format_for_init(init_tokens)?;
    let condition = format_for_condition(condition_tokens)?;
    let increment = format_for_increment(increment_tokens)?;

    Some(format!("for ({init}; {condition}; {increment})"))
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
            .or_else(|| try_format_inline_switch_expression(body))
            .or_else(|| try_format_do_while_tail(body))
            .or_else(|| try_format_do_while_close(body))
            .or_else(|| try_format_expression_statement(body));
    }

    if matches!(tokens.last(), Some(Token::Symbol(':'))) {
        let body = &tokens[..tokens.len() - 1];
        return try_format_case_label(body).or_else(|| try_format_default_label(body));
    }

    if matches!(tokens.last(), Some(Token::Symbol(','))) {
        let body = &tokens[..tokens.len() - 1];
        return try_format_switch_arm(body).map(|arm| format!("{arm},"));
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
        .or_else(|| try_format_do_statement(sig_tokens))
        .or_else(|| try_format_foreach_statement(sig_tokens))
        .or_else(|| try_format_for_statement(sig_tokens))
        .or_else(|| try_format_switch_statement(sig_tokens))
        .or_else(|| try_format_switch_expression_header(sig_tokens))
        .or_else(|| try_format_switch_arm(sig_tokens))?;
    Some(if trailing_brace {
        format!("{signature} {{")
    } else {
        signature
    })
}
