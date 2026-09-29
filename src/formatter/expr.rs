//! A small recursive-descent/precedence-climbing parser for C#
//! expressions, and a matching pretty-printer.
//!
//! Scope is intentionally limited to "basic" C#: literals, identifiers,
//! member access, calls, `new Type(...)`/`new(...)` object creation,
//! unary `!`/`-`/`+`/prefix and postfix `++`/`--`, arithmetic,
//! relational, equality, `&&`/`||`, and assignment (including compound
//! assignment). Anything outside that (ternary, array creation/object
//! initializers, indexers, lambdas, string interpolation, ...) simply
//! fails to parse, and the caller falls back to leaving the source
//! untouched.
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
    /// `new Type(args)`, or `new(args)` when the type is inferred
    /// from context (target-typed `new`). Array creation
    /// (`new int[5]`) and object/collection initializers (`new Foo {
    /// ... }`) aren't represented -- see [`parse_primary`].
    New(Option<String>, Vec<Expr>),
    Unary(&'static str, Box<Expr>),
    /// Postfix `++`/`--`, e.g. `i++`. Kept distinct from `Unary`
    /// (prefix) since the operator renders after the operand and, per
    /// C#'s grammar, can't itself be chained further.
    PostfixUnary(&'static str, Box<Expr>),
    Binary(&'static str, Box<Expr>, Box<Expr>),
    Assign(&'static str, Box<Expr>, Box<Expr>),
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
        Expr::New(type_name, args) => {
            let args = args.iter().map(render).collect::<Vec<_>>().join(", ");
            match type_name {
                Some(name) => format!("new {name}({args})"),
                None => format!("new({args})"),
            }
        }
        Expr::Unary(op, operand) => format!("{op}{}", render(operand)),
        Expr::PostfixUnary(op, operand) => format!("{}{op}", render(operand)),
        Expr::Binary(op, left, right) => format!("{} {op} {}", render(left), render(right)),
        Expr::Assign(op, left, right) => format!("{} {op} {}", render(left), render(right)),
    }
}

/// Parses a full expression, starting at the lowest precedence level
/// (assignment).
pub fn parse_expression(c: &mut Cursor) -> Option<Expr> {
    parse_assignment(c)
}

fn parse_assignment(c: &mut Cursor) -> Option<Expr> {
    let left = parse_logical_or(c)?;

    let op = match c.peek() {
        Some(Token::Symbol('=')) => "=",
        Some(Token::Op(op @ ("+=" | "-=" | "*=" | "/=" | "%="))) => op,
        _ => return Some(left),
    };
    c.next();
    let right = parse_assignment(c)?; // right-associative
    Some(Expr::Assign(op, Box::new(left), Box::new(right)))
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
    let mut left = parse_equality(c)?;
    while matches!(c.peek(), Some(Token::Op("&&"))) {
        c.next();
        let right = parse_equality(c)?;
        left = Expr::Binary("&&", Box::new(left), Box::new(right));
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
    let mut left = parse_additive(c)?;
    loop {
        let op = match c.peek() {
            Some(Token::Op(op @ ("<=" | ">="))) => op,
            Some(Token::Symbol('<')) => "<",
            Some(Token::Symbol('>')) => ">",
            _ => break,
        };
        c.next();
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
                expr = Expr::Call(Box::new(expr), parse_arg_list(c)?);
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

/// Parses a parenthesized, comma-separated argument list: `(a, b,
/// ...)`. The opening `(` must already be consumed by the caller.
fn parse_arg_list(c: &mut Cursor) -> Option<Vec<Expr>> {
    let mut args = Vec::new();
    if !matches!(c.peek(), Some(Token::Symbol(')'))) {
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
        Token::Symbol(')') => Some(args),
        _ => None,
    }
}

fn parse_primary(c: &mut Cursor) -> Option<Expr> {
    match c.next()? {
        Token::Ident("new") => {
            // `new(args)` (target-typed `new`) has no type name --
            // distinguished from `new Type(args)` by whether a `(`
            // immediately follows.
            let type_name = if matches!(c.peek(), Some(Token::Symbol('('))) {
                None
            } else {
                Some(super::statement::parse_type(c)?)
            };
            match c.next()? {
                Token::Symbol('(') => {}
                _ => return None,
            }
            Some(Expr::New(type_name, parse_arg_list(c)?))
        }
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
        _ => None,
    }
}
