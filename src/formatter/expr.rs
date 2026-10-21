//! A small recursive-descent/precedence-climbing parser for C#
//! expressions, and a matching pretty-printer.
//!
//! Scope is intentionally limited to "basic" C#: literals, identifiers,
//! member access, calls, indexers, `new Type(...)`/`new(...)` object
//! creation (with an optional trailing object/collection initializer,
//! whose items may themselves be indexed entries -- `["a"] = 1` --
//! or -- for a named type -- a bare `new Foo { .. }` with no argument
//! list at all), array creation (`new Type[size, ...]` for any rank,
//! `new Type[] { .. }`, `new[] { .. }`, and bare `{ .. }`
//! initializers), unary `!`/`-`/`+`/prefix and postfix `++`/`--`,
//! arithmetic, shift (`<<`/`>>`), relational, equality, bitwise
//! `&`/`^`/`|`, `&&`/`||`, null coalescing (`??`), assignment
//! (including compound assignment), the `?:` conditional (ternary)
//! operator, interpolated string literals (`$"..."`, holes
//! reformatted recursively -- see `interpolated.rs`), and `switch`
//! expressions (`scrutinee switch { pattern => result, ... }`,
//! patterns per [`parse_switch_pattern`]). Anything outside that
//! (lambdas, ...) simply fails to parse, and the caller falls back to
//! leaving the source untouched.
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
    /// A literal as written in the source (number, string, char, or
    /// the keywords `true`/`false`/`null`) -- except an interpolated
    /// string's holes, which are reformatted (see
    /// `interpolated.rs`); everything outside the holes is still
    /// exactly as written.
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
    /// from context (target-typed `new`), with an optional trailing
    /// object/collection initializer (`new Foo { X = 1 }`, `new Foo()
    /// { X = 1 }`). `args` is `None` when no parens were written at
    /// all (only possible with a named type plus an initializer --
    /// target-typed `new` always requires `()`), so the printer can
    /// tell that apart from empty parens (`new Foo()`) and reproduce
    /// the source as written rather than normalizing one to the
    /// other. Each initializer item is parsed as a full expression, so
    /// a member initializer (`X = 1`) is just `Expr::Assign` and a
    /// collection item (`1`) is a plain expression; an indexed
    /// collection initializer entry (`["a"] = 1`) is the one shape
    /// that isn't a plain expression on its own, so it gets its own
    /// `IndexedInitializerEntry` node.
    New(Option<String>, Option<Vec<Expr>>, Option<Vec<Expr>>),
    /// Array creation: `new Type[size, ...]` (one size expression per
    /// dimension), `new Type[] { ... }`, or `new[] { ... }`.
    /// `type_name` already includes any `[]`/`[,]` rank that
    /// `parse_type` folds into the type itself (the no-size forms);
    /// it's `None` only for the implicitly-typed `new[]` form. `sizes`
    /// holds the bracketed size expressions for the `new Type[size,
    /// ...]` form. `elements` holds a trailing `{ ... }` initializer's
    /// items when one is present.
    ArrayCreation(Option<String>, Option<Vec<Expr>>, Option<Vec<Expr>>),
    /// A bare `{ ... }` initializer list -- valid directly in a
    /// declarator initializer (`int[] arr = { 1, 2, 3 };`), and
    /// nested inside another initializer for jagged/multi-dimensional
    /// arrays (`new int[,] { { 1, 2 }, { 3, 4 } }`).
    ArrayInitializer(Vec<Expr>),
    /// An indexed collection initializer entry: `[key, ...] = value`
    /// (e.g. `new Dictionary<string, int> { ["a"] = 1 }`). Kept
    /// distinct from `Index` since there's no target expression --
    /// the bracketed key list is initializer syntax standing alone,
    /// not an indexer applied to an operand, so it isn't reachable
    /// from `parse_primary`/`parse_postfix` at all.
    IndexedInitializerEntry(Vec<Expr>, Box<Expr>),
    Unary(&'static str, Box<Expr>),
    /// Postfix `++`/`--`, e.g. `i++`. Kept distinct from `Unary`
    /// (prefix) since the operator renders after the operand and, per
    /// C#'s grammar, can't itself be chained further.
    PostfixUnary(&'static str, Box<Expr>),
    Binary(&'static str, Box<Expr>, Box<Expr>),
    Assign(&'static str, Box<Expr>, Box<Expr>),
    /// `cond ? when_true : when_false`.
    Ternary(Box<Expr>, Box<Expr>, Box<Expr>),
    /// `scrutinee switch { pattern => result, ... }`. Patterns are
    /// rendered to their final text up front by [`parse_switch_pattern`]
    /// rather than modeled as their own `Expr` variant -- they're a
    /// distinct grammar (relational/type/expression patterns combined
    /// with `and`/`or`/`not`/`when`), not reducible to expressions, and
    /// nothing downstream needs to inspect their structure.
    Switch(Box<Expr>, Vec<(String, Expr)>),
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
        Expr::New(type_name, args, initializer) => {
            let name_part = if let Some(name) = type_name {
                format!("new {name}")
            } else {
                "new".to_string()
            };
            let args_part = if let Some(args) = args {
                format!(
                    "({})",
                    args.iter().map(render).collect::<Vec<_>>().join(", ")
                )
            } else {
                String::new()
            };
            let init_part = if let Some(items) = initializer {
                format!(" {}", render_initializer(items))
            } else {
                String::new()
            };
            format!("{name_part}{args_part}{init_part}")
        }
        Expr::ArrayCreation(type_name, sizes, elements) => {
            let type_part = if let Some(name) = type_name {
                format!(" {name}")
            } else {
                String::new()
            };
            // `type_name` already includes the brackets for the
            // no-size forms (`new int[] { .. }`, `new int[,] { .. }`)
            // -- only the sized and implicitly-typed (`new[]`) forms
            // need brackets rendered here.
            let brackets = if sizes.is_some() || type_name.is_none() {
                let sizes = sizes
                    .as_deref()
                    .map(|sizes| sizes.iter().map(render).collect::<Vec<_>>().join(", "))
                    .unwrap_or_default();
                format!("[{sizes}]")
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
        Expr::IndexedInitializerEntry(keys, value) => {
            let keys = keys.iter().map(render).collect::<Vec<_>>().join(", ");
            format!("[{keys}] = {}", render(value))
        }
        Expr::Unary(op, operand) => format!("{op}{}", render(operand)),
        Expr::PostfixUnary(op, operand) => format!("{}{op}", render(operand)),
        Expr::Binary(op, left, right) => format!("{} {op} {}", render(left), render(right)),
        Expr::Assign(op, left, right) => format!("{} {op} {}", render(left), render(right)),
        Expr::Ternary(cond, when_true, when_false) => {
            format!(
                "{} ? {} : {}",
                render(cond),
                render(when_true),
                render(when_false)
            )
        }
        Expr::Switch(scrutinee, arms) => {
            let arms = arms
                .iter()
                .map(|(pattern, result)| format!("{pattern} => {}", render(result)))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{} switch {{ {arms} }}", render(scrutinee))
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
    Some(Expr::Ternary(
        Box::new(cond),
        Box::new(when_true),
        Box::new(when_false),
    ))
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

    if matches!(c.peek(), Some(Token::Ident("switch"))) {
        // Only commit to a switch expression once `{` is confirmed --
        // the multi-line header form (`expr switch` with the arm list
        // on later lines) must leave `switch` unconsumed so
        // `statement.rs`'s header parser can pick it up instead.
        let checkpoint = c.save();
        c.next();
        if matches!(c.peek(), Some(Token::Symbol('{'))) {
            return parse_switch_tail(c, expr);
        }
        c.restore(checkpoint);
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
        Token::InterpolatedStr(s) => Some(Expr::Literal(
            super::interpolated::format_interpolated_string(s),
        )),
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
/// Type(args)` object creation (either optionally followed by a
/// trailing object/collection initializer, or -- for a named type --
/// replaced by one entirely: `new Foo { X = 1 }`), and array creation
/// (`new Type[size, ...]`, `new Type[] { .. }` / `new Type[,] { .. }`,
/// or implicitly-typed `new[] { .. }`).
fn parse_new(c: &mut Cursor) -> Option<Expr> {
    if matches!(c.peek(), Some(Token::Symbol('['))) {
        return parse_implicit_array_creation(c);
    }

    // `new(args)` (target-typed `new`) has no type name -- distinguished
    // from `new Type(args)` by whether a `(` immediately follows.
    // Parens are mandatory here (there's no `new { .. }` form), so
    // `args` is always `Some`.
    if matches!(c.peek(), Some(Token::Symbol('('))) {
        c.next();
        let args = parse_arg_list(c, ')')?;
        let initializer = if matches!(c.peek(), Some(Token::Symbol('{'))) {
            Some(parse_array_initializer(c)?)
        } else {
            None
        };
        return Some(Expr::New(None, Some(args), initializer));
    }

    let type_name = super::statement::parse_type(c)?;
    match c.peek() {
        Some(Token::Symbol('(')) => {
            c.next();
            let args = parse_arg_list(c, ')')?;
            let initializer = if matches!(c.peek(), Some(Token::Symbol('{'))) {
                Some(parse_array_initializer(c)?)
            } else {
                None
            };
            Some(Expr::New(Some(type_name), Some(args), initializer))
        }
        Some(Token::Symbol('[')) => parse_sized_array_creation(c, type_name),
        // `new Type[] { .. }` / `new Type[,] { .. }` -- the empty rank
        // brackets were already folded into `type_name` by
        // `parse_type`.
        Some(Token::Symbol('{')) if type_name.ends_with(']') => Some(Expr::ArrayCreation(
            Some(type_name),
            None,
            Some(parse_array_initializer(c)?),
        )),
        // `new Foo { X = 1 }` -- an object/collection initializer with
        // no constructor argument list at all (not even empty parens).
        Some(Token::Symbol('{')) => Some(Expr::New(
            Some(type_name),
            None,
            Some(parse_array_initializer(c)?),
        )),
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
    Some(Expr::ArrayCreation(
        None,
        None,
        Some(parse_array_initializer(c)?),
    ))
}

/// `new Type[size, ...]` -- one size expression per dimension for a
/// multi-dimensional array (`new int[2, 3]`) -- with an optional
/// `{ .. }` initializer.
fn parse_sized_array_creation(c: &mut Cursor, type_name: String) -> Option<Expr> {
    c.next(); // '['
    let sizes = parse_arg_list(c, ']')?;
    let elements = if matches!(c.peek(), Some(Token::Symbol('{'))) {
        Some(parse_array_initializer(c)?)
    } else {
        None
    };
    Some(Expr::ArrayCreation(Some(type_name), Some(sizes), elements))
}

/// Parses a `{ .. }` initializer list, with the opening `{` not yet
/// consumed.
fn parse_array_initializer(c: &mut Cursor) -> Option<Vec<Expr>> {
    if !matches!(c.next()?, Token::Symbol('{')) {
        return None;
    }
    parse_initializer_items(c, '}')
}

/// Parses a comma-separated initializer item list delimited by
/// `close`, mirroring `parse_arg_list` but recognizing the indexed
/// collection initializer entry form (`[key, ...] = value`) that
/// `parse_assignment` can't produce on its own, since a bare `[..]`
/// isn't a valid primary expression anywhere else in the grammar. The
/// opening delimiter must already be consumed by the caller.
fn parse_initializer_items(c: &mut Cursor, close: char) -> Option<Vec<Expr>> {
    let mut items = Vec::new();
    if !matches!(c.peek(), Some(Token::Symbol(s)) if s == close) {
        loop {
            items.push(parse_initializer_item(c)?);
            match c.peek() {
                Some(Token::Symbol(',')) => {
                    c.next();
                }
                _ => break,
            }
        }
    }
    match c.next()? {
        Token::Symbol(s) if s == close => Some(items),
        _ => None,
    }
}

/// A single initializer item: either an indexed collection
/// initializer entry (`[key, ...] = value`) or a plain expression
/// (a member initializer `X = 1`, or a collection item `1`).
fn parse_initializer_item(c: &mut Cursor) -> Option<Expr> {
    if matches!(c.peek(), Some(Token::Symbol('['))) {
        c.next();
        let keys = parse_arg_list(c, ']')?;
        if !matches!(c.next()?, Token::Symbol('=')) {
            return None;
        }
        let value = parse_assignment(c)?;
        return Some(Expr::IndexedInitializerEntry(keys, Box::new(value)));
    }
    parse_assignment(c)
}

/// Parses a `switch` expression's `{ pattern => result, ... }` arm
/// list, with `scrutinee` and the `switch` keyword already consumed.
fn parse_switch_tail(c: &mut Cursor, scrutinee: Expr) -> Option<Expr> {
    if !matches!(c.next()?, Token::Symbol('{')) {
        return None;
    }

    let mut arms = Vec::new();
    if !matches!(c.peek(), Some(Token::Symbol('}'))) {
        loop {
            arms.push(parse_switch_arm(c)?);
            if let Some(Token::Symbol(',')) = c.peek() {
                c.next();
                if matches!(c.peek(), Some(Token::Symbol('}'))) {
                    break;
                }
            } else {
                break;
            }
        }
    }

    if !matches!(c.next()?, Token::Symbol('}')) {
        return None;
    }

    Some(Expr::Switch(Box::new(scrutinee), arms))
}

/// Parses one `switch` arm: `pattern => result`, where `pattern` may
/// include a `when` guard (see [`parse_switch_pattern`]). Shared by
/// [`parse_switch_tail`] (an inline switch expression) and
/// `statement.rs`'s per-line arm formatting for the multi-line form.
pub(super) fn parse_switch_arm(c: &mut Cursor) -> Option<(String, Expr)> {
    let pattern = parse_switch_pattern(c)?;
    if !matches!(c.next()?, Token::Op("=>")) {
        return None;
    }
    let result = parse_assignment(c)?;
    Some((pattern, result))
}

/// Words that are contextual keywords in pattern position and so
/// can't double as a type pattern's binding identifier -- e.g. `case
/// int and > 0:` must keep `and` available to [`parse_and_pattern`]
/// rather than letting the type pattern swallow it as a binding named
/// `and`.
const PATTERN_KEYWORDS: &[&str] = &["when", "and", "or", "not"];

/// Parses a single relational/type/expression pattern, or a
/// parenthesized sub-pattern -- the base case of the pattern grammar,
/// with no `and`/`or`/`not`/`when` of its own. Recognizes, in order:
///
///   - a **parenthesized pattern** (`(int i and > 0)`), grouping any
///     `or`-combined sub-pattern so it can be combined further by the
///     caller (e.g. `(int i and > 0) or < -10`);
///   - a **relational pattern** (`> 5`, `<= 0`): a bare relational
///     operator followed by a constant expression;
///   - a **type pattern** (`int i`, `List<int> items`): a type
///     (reusing [`super::statement::parse_type`]) immediately followed
///     by a binding identifier (or `_` discard) -- tried with
///     backtracking, since a leading identifier could just as easily
///     start a plain expression pattern instead (`Color.Red`,
///     `someConstant`);
///   - otherwise, a plain **expression pattern** (literal, identifier,
///     member access, or `_`).
pub(super) fn parse_primary_pattern(c: &mut Cursor) -> Option<String> {
    if matches!(c.peek(), Some(Token::Symbol('('))) {
        c.next();
        let inner = parse_or_pattern(c)?;
        if !matches!(c.next()?, Token::Symbol(')')) {
            return None;
        }
        return Some(format!("({inner})"));
    }

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
    let type_pattern = super::statement::parse_type(c).and_then(|type_name| match c.peek() {
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

/// Parses `or`-combined patterns (`> 0 or < -10`); `or` binds loosest
/// among the combinators, and is the level a parenthesized
/// sub-pattern groups (see [`parse_primary_pattern`]).
fn parse_or_pattern(c: &mut Cursor) -> Option<String> {
    let mut left = parse_and_pattern(c)?;
    while matches!(c.peek(), Some(Token::Ident("or"))) {
        c.next();
        let right = parse_and_pattern(c)?;
        left = format!("{left} or {right}");
    }
    Some(left)
}

/// Parses an `or`-pattern optionally followed by a `when` guard
/// clause -- the full pattern grammar shared by `case` labels and
/// `switch` expression arms. `when` binds outside any parenthesized
/// grouping, so it's handled here rather than in [`parse_or_pattern`].
pub(super) fn parse_switch_pattern(c: &mut Cursor) -> Option<String> {
    let left = parse_or_pattern(c)?;

    if matches!(c.peek(), Some(Token::Ident("when"))) {
        c.next();
        let guard = parse_expression(c)?;
        return Some(format!("{left} when {}", render(&guard)));
    }

    Some(left)
}
