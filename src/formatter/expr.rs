//! A small recursive-descent/precedence-climbing parser for C#
//! expressions, and a matching pretty-printer.
//!
//! Scope is intentionally limited to "basic" C#: literals, identifiers,
//! member access, calls, indexers, `new Type(...)`/`new(...)` object
//! creation, array creation (`new Type[size]`, `new Type[] { .. }`,
//! `new[] { .. }`, and bare `{ .. }` initializers), unary
//! `!`/`-`/`+`/prefix and postfix `++`/`--`, arithmetic, shift
//! (`<<`/`>>`), relational, equality, bitwise `&`/`^`/`|`, `&&`/`||`,
//! null coalescing (`??`), assignment (including compound
//! assignment), and the `?:` conditional (ternary) operator. Anything
//! outside that (object/collection initializers, lambdas, string
//! interpolation, ...) simply fails to parse, and the caller falls
//! back to leaving the source untouched.
//!
//! Precedence is never inferred by the printer: every parenthesized
//! subexpression in the input becomes an explicit `Paren` node that
//! always prints its own parens, and every other node prints its
//! children as-is. Since the parser only ever reflects the grouping
//! implied by the source, this is sufficient to round-trip spacing
//! without any risk of changing what the expression means.

use crate::formatter::token::{Cursor, Token};

#[derive(Debug, Clone)]
pub enum Expr {
    /// A literal exactly as written in the source (number, string,
    /// char, or the keywords `true`/`false`/`null`).
    Literal(String),
    Ident(String),
    Paren(Box<Expr>),
    Member(Box<Expr>, String),
    Call(Box<Expr>, Vec<Expr>),
    /// An indexer access: `target[args]`. C# indexers can take
    /// multiple comma-separated arguments (multi-dimensional
    /// indexers, e.g. `matrix[i, j]`), so this mirrors `Call`'s
    /// argument-list shape rather than holding a single index.
    Index(Box<Expr>, Vec<Expr>),
    /// `new Type(args)`, or `new(args)` when the type is inferred
    /// from context (target-typed `new`). Object/collection
    /// initializers (`new Foo { X = 1 }`) aren't represented -- see
    /// [`parse_primary`].
    New(Option<String>, Vec<Expr>),
    /// Array creation: `new Type[size]`, `new Type[] { ... }`, or
    /// `new[] { ... }`. `type_name` already includes any `[]`/`[,]`
    /// rank that `parse_type` folds into the type itself (the
    /// no-size forms); it's `None` only for the implicitly-typed
    /// `new[]` form. `size` holds the bracketed size expression for
    /// the `new Type[size]` form. `elements` holds a trailing
    /// `{ ... }` initializer's items when one is present.
    ArrayCreation(Option<String>, Option<Box<Expr>>, Option<Vec<Expr>>),
    /// A bare `{ ... }` initializer list -- valid directly in a
    /// declarator initializer (`int[] arr = { 1, 2, 3 };`), and
    /// nested inside another initializer for jagged/multi-dimensional
    /// arrays (`new int[,] { { 1, 2 }, { 3, 4 } }`).
    ArrayInitializer(Vec<Expr>),
    Unary(&'static str, Box<Expr>),
    /// Postfix `++`/`--`, e.g. `i++`. Kept distinct from `Unary`
    /// (prefix) since the operator renders after the operand and, per
    /// C#'s grammar, can't itself be chained further.
    PostfixUnary(&'static str, Box<Expr>),
    Binary(&'static str, Box<Expr>, Box<Expr>),
    Assign(&'static str, Box<Expr>, Box<Expr>),
    /// `cond ? when_true : when_false`.
    Ternary(Box<Expr>, Box<Expr>, Box<Expr>),
}

pub fn render(expr: &Expr) -> String {
    match expr {
        Expr::Literal(s) | Expr::Ident(s) => s.clone(),
        Expr::Paren(inner) => format!("({})", render(inner)),
        Expr::Member(left, name) => format!("{}.{}", render(left), name),
        Expr::Call(callee, args) => {
            let args = args.iter().map(render).collect::<Vec<_>>().join(", ");
            format!("{}({})", render(callee), args)
        }
        Expr::Index(target, args) => {
            let args = args.iter().map(render).collect::<Vec<_>>().join(", ");
            format!("{}[{}]", render(target), args)
        }
        Expr::New(type_name, args) => {
            let args = args.iter().map(render).collect::<Vec<_>>().join(", ");
            match type_name {
                Some(name) => format!("new {name}({args})"),
                None => format!("new({args})"),
            }
        }
        Expr::ArrayCreation(type_name, size, elements) => {
            let type_part = if let Some(name) = type_name {
                format!(" {name}")
            } else {
                String::new()
            };
            // `type_name` already includes the brackets for the
            // no-size forms (`new int[] { .. }`, `new int[,] { .. }`)
            // -- only the sized and implicitly-typed (`new[]`) forms
            // need brackets rendered here.
            let brackets = if size.is_some() || type_name.is_none() {
                format!("[{}]", size.as_deref().map(render).unwrap_or_default())
            } else {
                String::new()
            };
            let init_part = if let Some(elements) = elements {
                format!(" {}", render_initializer(elements))
            } else {
                String::new()
            };
            format!("new{type_part}{brackets}{init_part}")
        }
        Expr::ArrayInitializer(elements) => render_initializer(elements),
        Expr::Unary(op, operand) => format!("{op}{}", render(operand)),
        Expr::PostfixUnary(op, operand) => format!("{}{op}", render(operand)),
        Expr::Binary(op, left, right) => format!("{} {op} {}", render(left), render(right)),
        Expr::Assign(op, left, right) => format!("{} {op} {}", render(left), render(right)),
        Expr::Ternary(cond, when_true, when_false) => {
            format!("{} ? {} : {}", render(cond), render(when_true), render(when_false))
        }
    }
}

fn render_initializer(elements: &[Expr]) -> String {
    if elements.is_empty() {
        return "{ }".to_string();
    }
    let elements = elements.iter().map(render).collect::<Vec<_>>().join(", ");
    format!("{{ {elements} }}")
}

/// Parses a full expression, starting at the lowest precedence level
/// (assignment).
pub fn parse_expression(c: &mut Cursor) -> Option<Expr> {
    parse_assignment(c)
}

fn parse_assignment(c: &mut Cursor) -> Option<Expr> {
    let left = parse_ternary(c)?;

    let op = match c.peek() {
        Some(Token::Symbol('=')) => "=",
        Some(Token::Op(op @ ("+=" | "-=" | "*=" | "/=" | "%="))) => op,
        _ => return Some(left),
    };
    c.next();
    let right = parse_assignment(c)?; // right-associative
    Some(Expr::Assign(op, Box::new(left), Box::new(right)))
}

/// `cond ? when_true : when_false`, right-associative (so `a ? b : c ? d
/// : e` parses as `a ? b : (c ? d : e)`). The branches are parsed at
/// assignment precedence so both nested ternaries and assignments
/// (`a ? b : c = d`) are allowed inside them, matching C#'s grammar.
fn parse_ternary(c: &mut Cursor) -> Option<Expr> {
    let cond = parse_null_coalescing(c)?;
    if !matches!(c.peek(), Some(Token::Symbol('?'))) {
        return Some(cond);
    }
    c.next();
    let when_true = parse_assignment(c)?;
    if !matches!(c.next()?, Token::Symbol(':')) {
        return None;
    }
    let when_false = parse_assignment(c)?;
    Some(Expr::Ternary(Box::new(cond), Box::new(when_true), Box::new(when_false)))
}

/// `a ?? b`, right-associative (so `a ?? b ?? c` parses as `a ?? (b ??
/// c)`), binding tighter than `?:` but looser than `||`.
fn parse_null_coalescing(c: &mut Cursor) -> Option<Expr> {
    let left = parse_logical_or(c)?;
    if !matches!(c.peek(), Some(Token::Op("??"))) {
        return Some(left);
    }
    c.next();
    let right = parse_null_coalescing(c)?;
    Some(Expr::Binary("??", Box::new(left), Box::new(right)))
}

fn parse_logical_or(c: &mut Cursor) -> Option<Expr> {
    let mut left = parse_logical_and(c)?;
    while matches!(c.peek(), Some(Token::Op("||"))) {
        c.next();
        let right = parse_logical_and(c)?;
        left = Expr::Binary("||", Box::new(left), Box::new(right));
    }
    Some(left)
}

fn parse_logical_and(c: &mut Cursor) -> Option<Expr> {
    let mut left = parse_bitwise_or(c)?;
    while matches!(c.peek(), Some(Token::Op("&&"))) {
        c.next();
        let right = parse_bitwise_or(c)?;
        left = Expr::Binary("&&", Box::new(left), Box::new(right));
    }
    Some(left)
}

fn parse_bitwise_or(c: &mut Cursor) -> Option<Expr> {
    let mut left = parse_bitwise_xor(c)?;
    while matches!(c.peek(), Some(Token::Symbol('|'))) {
        c.next();
        let right = parse_bitwise_xor(c)?;
        left = Expr::Binary("|", Box::new(left), Box::new(right));
    }
    Some(left)
}

fn parse_bitwise_xor(c: &mut Cursor) -> Option<Expr> {
    let mut left = parse_bitwise_and(c)?;
    while matches!(c.peek(), Some(Token::Symbol('^'))) {
        c.next();
        let right = parse_bitwise_and(c)?;
        left = Expr::Binary("^", Box::new(left), Box::new(right));
    }
    Some(left)
}

fn parse_bitwise_and(c: &mut Cursor) -> Option<Expr> {
    let mut left = parse_equality(c)?;
    while matches!(c.peek(), Some(Token::Symbol('&'))) {
        c.next();
        let right = parse_equality(c)?;
        left = Expr::Binary("&", Box::new(left), Box::new(right));
    }
    Some(left)
}

fn parse_equality(c: &mut Cursor) -> Option<Expr> {
    let mut left = parse_relational(c)?;
    while let Some(Token::Op(op @ ("==" | "!="))) = c.peek() {
        c.next();
        let right = parse_relational(c)?;
        left = Expr::Binary(op, Box::new(left), Box::new(right));
    }
    Some(left)
}

fn parse_relational(c: &mut Cursor) -> Option<Expr> {
    let mut left = parse_shift(c)?;
    loop {
        let op = match c.peek() {
            Some(Token::Op(op @ ("<=" | ">="))) => op,
            Some(Token::Symbol('<')) => "<",
            Some(Token::Symbol('>')) => ">",
            _ => break,
        };
        c.next();
        let right = parse_shift(c)?;
        left = Expr::Binary(op, Box::new(left), Box::new(right));
    }
    Some(left)
}

/// `<<`/`>>`. The lexer never merges these into a single token (see
/// the comment on `MULTI_CHAR_OPS` in `token.rs`), so each is
/// recognized here as two adjacent, identical `<`/`>` `Symbol`s; a
/// lone `<`/`>` (relational) is left in place -- via `restore` -- for
/// `parse_relational` to pick up.
fn parse_shift(c: &mut Cursor) -> Option<Expr> {
    let mut left = parse_additive(c)?;
    loop {
        let checkpoint = c.save();
        let Some(Token::Symbol(first @ ('<' | '>'))) = c.peek() else {
            break;
        };
        c.next();
        if !matches!(c.peek(), Some(Token::Symbol(s)) if s == first) {
            c.restore(checkpoint);
            break;
        }
        c.next();
        let op = if first == '<' { "<<" } else { ">>" };
        let right = parse_additive(c)?;
        left = Expr::Binary(op, Box::new(left), Box::new(right));
    }
    Some(left)
}

fn parse_additive(c: &mut Cursor) -> Option<Expr> {
    let mut left = parse_multiplicative(c)?;
    loop {
        let op = match c.peek() {
            Some(Token::Symbol('+')) => "+",
            Some(Token::Symbol('-')) => "-",
            _ => break,
        };
        c.next();
        let right = parse_multiplicative(c)?;
        left = Expr::Binary(op, Box::new(left), Box::new(right));
    }
    Some(left)
}

fn parse_multiplicative(c: &mut Cursor) -> Option<Expr> {
    let mut left = parse_unary(c)?;
    loop {
        let op = match c.peek() {
            Some(Token::Symbol('*')) => "*",
            Some(Token::Symbol('/')) => "/",
            Some(Token::Symbol('%')) => "%",
            _ => break,
        };
        c.next();
        let right = parse_unary(c)?;
        left = Expr::Binary(op, Box::new(left), Box::new(right));
    }
    Some(left)
}

fn parse_unary(c: &mut Cursor) -> Option<Expr> {
    let op = match c.peek() {
        Some(Token::Symbol('!')) => Some("!"),
        Some(Token::Symbol('-')) => Some("-"),
        Some(Token::Symbol('+')) => Some("+"),
        Some(Token::Op(op @ ("++" | "--"))) => Some(op),
        _ => None,
    };
    if let Some(op) = op {
        c.next();
        let operand = parse_unary(c)?;
        return Some(Expr::Unary(op, Box::new(operand)));
    }
    parse_postfix(c)
}

fn parse_postfix(c: &mut Cursor) -> Option<Expr> {
    let mut expr = parse_primary(c)?;
    loop {
        match c.peek() {
            Some(Token::Symbol('.')) => {
                c.next();
                let name = crate::formatter::token::as_ident(c.next()?)?.to_string();
                expr = Expr::Member(Box::new(expr), name);
            }
            Some(Token::Symbol('(')) => {
                c.next();
                expr = Expr::Call(Box::new(expr), parse_arg_list(c, ')')?);
            }
            Some(Token::Symbol('[')) => {
                c.next();
                expr = Expr::Index(Box::new(expr), parse_arg_list(c, ']')?);
            }
            _ => break,
        }
    }

    if let Some(Token::Op(op @ ("++" | "--"))) = c.peek() {
        c.next();
        return Some(Expr::PostfixUnary(op, Box::new(expr)));
    }

    Some(expr)
}

/// Parses a comma-separated argument list delimited by `close` (`)`
/// for calls/`new`, `]` for indexers). The opening delimiter must
/// already be consumed by the caller.
fn parse_arg_list(c: &mut Cursor, close: char) -> Option<Vec<Expr>> {
    let mut args = Vec::new();
    if !matches!(c.peek(), Some(Token::Symbol(s)) if s == close) {
        loop {
            args.push(parse_assignment(c)?);
            match c.peek() {
                Some(Token::Symbol(',')) => {
                    c.next();
                }
                _ => break,
            }
        }
    }
    match c.next()? {
        Token::Symbol(s) if s == close => Some(args),
        _ => None,
    }
}

fn parse_primary(c: &mut Cursor) -> Option<Expr> {
    match c.next()? {
        Token::Ident("new") => parse_new(c),
        Token::Ident(s) => Some(Expr::Ident(s.to_string())),
        Token::Int(s) | Token::Float(s) | Token::Str(s) | Token::Char(s) => {
            Some(Expr::Literal(s.to_string()))
        }
        Token::Symbol('(') => {
            let inner = parse_assignment(c)?;
            match c.next()? {
                Token::Symbol(')') => Some(Expr::Paren(Box::new(inner))),
                _ => None,
            }
        }
        Token::Symbol('{') => Some(Expr::ArrayInitializer(parse_arg_list(c, '}')?)),
        _ => None,
    }
}

/// Parses the `new` keyword's forms: target-typed `new(args)`, `new
/// Type(args)` object creation, and array creation (`new Type[size]`,
/// `new Type[] { .. }` / `new Type[,] { .. }`, or implicitly-typed
/// `new[] { .. }`). `new Foo { X = 1 }` object/collection initializers
/// aren't supported -- that falls through to `None` below, since a
/// bare `Type` isn't distinguishable from an array's element type
/// until brackets show up.
fn parse_new(c: &mut Cursor) -> Option<Expr> {
    if matches!(c.peek(), Some(Token::Symbol('['))) {
        return parse_implicit_array_creation(c);
    }

    // `new(args)` (target-typed `new`) has no type name -- distinguished
    // from `new Type(args)` by whether a `(` immediately follows.
    if matches!(c.peek(), Some(Token::Symbol('('))) {
        c.next();
        return Some(Expr::New(None, parse_arg_list(c, ')')?));
    }

    let type_name = super::statement::parse_type(c)?;
    match c.peek() {
        Some(Token::Symbol('(')) => {
            c.next();
            Some(Expr::New(Some(type_name), parse_arg_list(c, ')')?))
        }
        Some(Token::Symbol('[')) => parse_sized_array_creation(c, type_name),
        // `new Type[] { .. }` / `new Type[,] { .. }` -- the empty rank
        // brackets were already folded into `type_name` by
        // `parse_type`. A plain `new Foo { .. }` object initializer
        // (no brackets) falls through to `_ => None` instead.
        Some(Token::Symbol('{')) if type_name.ends_with(']') => {
            Some(Expr::ArrayCreation(Some(type_name), None, Some(parse_array_initializer(c)?)))
        }
        _ => None,
    }
}

/// `new[] { .. }` -- implicitly-typed array creation; the brackets
/// are always empty since the element type (and so the array's rank)
/// is inferred from the initializer.
fn parse_implicit_array_creation(c: &mut Cursor) -> Option<Expr> {
    c.next(); // '['
    if !matches!(c.next()?, Token::Symbol(']')) {
        return None;
    }
    Some(Expr::ArrayCreation(None, None, Some(parse_array_initializer(c)?)))
}

/// `new Type[size]`, with an optional `{ .. }` initializer.
fn parse_sized_array_creation(c: &mut Cursor, type_name: String) -> Option<Expr> {
    c.next(); // '['
    let size = Box::new(parse_expression(c)?);
    if !matches!(c.next()?, Token::Symbol(']')) {
        return None;
    }
    let elements = if matches!(c.peek(), Some(Token::Symbol('{'))) {
        Some(parse_array_initializer(c)?)
    } else {
        None
    };
    Some(Expr::ArrayCreation(Some(type_name), Some(size), elements))
}

/// Parses a `{ .. }` initializer list, with the opening `{` not yet
/// consumed.
fn parse_array_initializer(c: &mut Cursor) -> Option<Vec<Expr>> {
    if !matches!(c.next()?, Token::Symbol('{')) {
        return None;
    }
    parse_arg_list(c, '}')
}
