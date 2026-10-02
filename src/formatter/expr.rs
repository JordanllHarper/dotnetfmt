//! A small recursive-descent/precedence-climbing parser for C#
//! expressions, and a matching pretty-printer.
//!
//! Scope is intentionally limited to "basic" C#: literals, identifiers,
//! member access, calls, indexers, `new Type(...)`/`new(...)` object
//! creation, unary `!`/`-`/`+`/prefix and postfix `++`/`--`,
//! arithmetic, relational, equality, `&&`/`||`, assignment (including
//! compound assignment), and the `?:` conditional (ternary) operator.
//! Anything outside that (array creation/object initializers, lambdas,
//! string interpolation, ...) simply fails to parse, and the caller
//! falls back to leaving the source untouched.
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
        Expr::Unary(op, operand) => format!("{op}{}", render(operand)),
        Expr::PostfixUnary(op, operand) => format!("{}{op}", render(operand)),
        Expr::Binary(op, left, right) => format!("{} {op} {}", render(left), render(right)),
        Expr::Assign(op, left, right) => format!("{} {op} {}", render(left), render(right)),
        Expr::Ternary(cond, when_true, when_false) => {
            format!("{} ? {} : {}", render(cond), render(when_true), render(when_false))
        }
    }
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
    let cond = parse_logical_or(c)?;
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
            Some(Expr::New(type_name, parse_arg_list(c, ')')?))
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
