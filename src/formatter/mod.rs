//! Formats a `.cs` file's contents.
//!
//! Each line is treated as (at most) one logical statement: a
//! variable declaration, an expression statement, a method signature,
//! or a brace on its own. Indentation is recomputed from scratch on
//! every line, tracked as a brace-depth counter across the whole
//! file, using `config`'s `indent_style`/`indent_size`/`tab_width`.
//! Anything not recognized by the statement-level parsers
//! (see [`statement`]) is left textually as-is, only reindented.

mod expr;
mod statement;
mod token;

use std::fmt;

use crate::editorconfig::{EditorConfig, IndentSize, IndentStyle};
use token::{tokenize, Token};

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
pub fn format(contents: &str, config: &EditorConfig) -> Result<String, FormatError> {
    let indent_unit = indent_unit(config);
    let mut depth: i32 = 0;

    let formatted = contents
        .split("\r\n")
        .map(|line| {
            let (rendered, new_depth) = format_line(line, depth, &indent_unit);
            depth = new_depth;
            rendered
        })
        .collect::<Vec<_>>()
        .join("\r\n");

    Ok(formatted)
}

fn indent_unit(config: &EditorConfig) -> String {
    match config.indent_style {
        IndentStyle::Tab => "\t".to_string(),
        IndentStyle::Space => {
            let columns = match config.indent_size {
                IndentSize::Columns(n) => n,
                IndentSize::UseTabWidth => config.tab_width,
            };
            " ".repeat(columns as usize)
        }
    }
}

/// Formats one line at the given brace `depth` (the depth in effect
/// *before* this line), returning the rendered line and the depth in
/// effect after it.
fn format_line(line: &str, depth: i32, indent_unit: &str) -> (String, i32) {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return (String::new(), depth);
    }

    let tokens = tokenize(trimmed);
    let starts_with_close = matches!(tokens.first(), Some(Token::Symbol('}')));
    let render_depth = if starts_with_close { depth - 1 } else { depth }.max(0);

    let opens = tokens.iter().filter(|t| matches!(t, Token::Symbol('{'))).count() as i32;
    let closes = tokens.iter().filter(|t| matches!(t, Token::Symbol('}'))).count() as i32;
    let new_depth = (depth + opens - closes).max(0);

    let content =
        statement::format_statement_content(trimmed).unwrap_or_else(|| trimmed.to_string());
    let indent = indent_unit.repeat(render_depth as usize);

    (format!("{indent}{content}"), new_depth)
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

    fn fmt(input: &str) -> String {
        format(input, &config()).unwrap()
    }

    // --- whitespace / spacing basics -----------------------------------

    #[test]
    fn already_formatted_is_unchanged() {
        assert_eq!(fmt("var x = 5;"), "var x = 5;");
    }

    #[test]
    fn collapses_missing_spaces() {
        assert_eq!(fmt("var x=5;"), "var x = 5;");
    }

    #[test]
    fn collapses_excess_whitespace() {
        assert_eq!(fmt("var                x =               5;"), "var x = 5;");
    }

    #[test]
    fn tabs_are_treated_as_whitespace_too() {
        assert_eq!(fmt("var\tx\t=\t5;"), "var x = 5;");
    }

    #[test]
    fn multiple_lines_are_joined_with_crlf() {
        let input = "var x=5;\r\nvar y   =   10;";
        assert_eq!(fmt(input), "var x = 5;\r\nvar y = 10;");
    }

    #[test]
    fn a_genuinely_unsupported_construct_is_left_alone() {
        // Ternary conditionals are out of scope; the line should be
        // reindented (trivially, at depth 0) but not rewritten.
        let result = fmt("var x = cond ? a : b;");
        assert_eq!(result, "var x = cond ? a : b;");
    }

    // --- indentation / brace depth --------------------------------------

    #[test]
    fn top_level_stray_indentation_is_normalized() {
        assert_eq!(fmt("    var x    =    5;"), "var x = 5;");
    }

    #[test]
    fn statements_inside_a_method_body_are_indented_one_level() {
        let input = "void Foo()\r\n{\r\nvar x=5;\r\n}";
        let expected = "void Foo()\r\n{\r\n    var x = 5;\r\n}";
        assert_eq!(fmt(input), expected);
    }

    #[test]
    fn nested_blocks_indent_further() {
        let input = "void Foo()\r\n{\r\nif (true)\r\n{\r\nvar x=5;\r\n}\r\n}";
        let expected =
            "void Foo()\r\n{\r\n    if (true)\r\n    {\r\n        var x = 5;\r\n    }\r\n}";
        assert_eq!(fmt(input), expected);
    }

    #[test]
    fn tab_indent_style_is_respected() {
        let config = EditorConfig {
            indent_style: IndentStyle::Tab,
            indent_size: IndentSize::Columns(4),
            tab_width: 4,
        };
        let input = "void Foo()\r\n{\r\nvar x=5;\r\n}";
        let expected = "void Foo()\r\n{\r\n\tvar x = 5;\r\n}";
        assert_eq!(format(input, &config).unwrap(), expected);
    }

    // --- function calls ---------------------------------------------------

    #[test]
    fn formats_empty_function_call_value() {
        assert_eq!(fmt("var x=someFunction();"), "var x = someFunction();");
    }

    #[test]
    fn collapses_whitespace_around_empty_call_parens() {
        assert_eq!(
            fmt("var w = someFunction (   )   ;"),
            "var w = someFunction();"
        );
    }

    #[test]
    fn formats_single_argument_function_call_value() {
        assert_eq!(fmt("var x=someFunction(a);"), "var x = someFunction(a);");
    }

    #[test]
    fn formats_three_or_more_arguments() {
        assert_eq!(
            fmt("var x = someFunction(a,   b ,c);"),
            "var x = someFunction(a, b, c);"
        );
    }

    #[test]
    fn missing_comma_between_arguments_is_not_recognized() {
        assert_eq!(
            fmt("var x = someFunction(a b);"),
            "var x = someFunction(a b);"
        );
    }

    #[test]
    fn trailing_comma_is_not_recognized() {
        assert_eq!(
            fmt("var x = someFunction(a, b,);"),
            "var x = someFunction(a, b,);"
        );
    }

    #[test]
    fn nested_calls_are_now_recognized() {
        assert_eq!(
            fmt("var x=outer(inner(a,b),c);"),
            "var x = outer(inner(a, b), c);"
        );
    }

    #[test]
    fn chained_method_calls_are_now_recognized() {
        assert_eq!(fmt("var x=a.b(c).d(e);"), "var x = a.b(c).d(e);");
    }

    #[test]
    fn call_plus_call_is_now_actively_reformatted() {
        assert_eq!(
            fmt("var x=someFunction( )+otherFunction(  );"),
            "var x = someFunction() + otherFunction();"
        );
    }

    #[test]
    fn chained_operators_with_a_call_are_now_actively_reformatted() {
        assert_eq!(
            fmt("var x=y+someFunction( )+z;"),
            "var x = y + someFunction() + z;"
        );
    }

    // --- operators --------------------------------------------------------

    #[test]
    fn collapses_whitespace_around_arithmetic_op() {
        assert_eq!(fmt("var x = 5    +    3;"), "var x = 5 + 3;");
    }

    #[test]
    fn collapses_whitespace_around_accessor() {
        assert_eq!(fmt("var x = a .    b;"), "var x = a.b;");
    }

    #[test]
    fn formats_relational_operators() {
        assert_eq!(fmt("var x=a<b;"), "var x = a < b;");
        assert_eq!(fmt("var x=a>=b;"), "var x = a >= b;");
    }

    #[test]
    fn formats_equality_operators() {
        assert_eq!(fmt("var x=a==b;"), "var x = a == b;");
        assert_eq!(fmt("var x=a!=b;"), "var x = a != b;");
    }

    #[test]
    fn formats_logical_operators() {
        assert_eq!(fmt("var x=a&&b||c;"), "var x = a && b || c;");
    }

    #[test]
    fn formats_unary_not_and_minus() {
        assert_eq!(fmt("var x=!a;"), "var x = !a;");
        assert_eq!(fmt("var x=-a;"), "var x = -a;");
    }

    #[test]
    fn formats_prefix_increment() {
        assert_eq!(fmt("var x=++a;"), "var x = ++a;");
    }

    #[test]
    fn formats_parenthesized_expressions() {
        assert_eq!(fmt("var x=(a+b)*c;"), "var x = (a + b) * c;");
    }

    #[test]
    fn nested_parens_are_preserved() {
        assert_eq!(fmt("var x=((a+b));"), "var x = ((a + b));");
    }

    #[test]
    fn respects_operator_precedence_when_reflowing() {
        assert_eq!(fmt("var x=a+b*c;"), "var x = a + b * c;");
    }

    #[test]
    fn compound_assignment_is_formatted() {
        assert_eq!(fmt("x+=1;"), "x += 1;");
        assert_eq!(fmt("x   -=   y;"), "x -= y;");
    }

    #[test]
    fn plain_assignment_expression_statement_is_formatted() {
        assert_eq!(fmt("x=5;"), "x = 5;");
    }

    // --- literals / data types --------------------------------------------

    #[test]
    fn string_literal_contents_are_preserved() {
        assert_eq!(
            fmt("var s=\"hello   world\";"),
            "var s = \"hello   world\";"
        );
    }

    #[test]
    fn char_literal_is_recognized() {
        assert_eq!(fmt("var c='x';"), "var c = 'x';");
    }

    #[test]
    fn bool_and_null_literals_are_recognized() {
        assert_eq!(fmt("var a=true;"), "var a = true;");
        assert_eq!(fmt("var b=null;"), "var b = null;");
    }

    #[test]
    fn float_literal_suffixes_are_preserved() {
        assert_eq!(fmt("double d=1.5;"), "double d = 1.5;");
        assert_eq!(fmt("var m=1.5m;"), "var m = 1.5m;");
    }

    // --- variable declarations ---------------------------------------------

    #[test]
    fn explicit_builtin_type_declaration_is_formatted() {
        assert_eq!(fmt("int x=5;"), "int x = 5;");
        assert_eq!(fmt("bool   flag=true;"), "bool flag = true;");
    }

    #[test]
    fn declaration_without_initializer_is_formatted() {
        assert_eq!(fmt("int    x;"), "int x;");
    }

    #[test]
    fn multiple_declarators_are_formatted() {
        assert_eq!(fmt("int x=1,y=2;"), "int x = 1, y = 2;");
        assert_eq!(fmt("int a,b,c;"), "int a, b, c;");
    }

    #[test]
    fn generic_type_declaration_is_formatted() {
        assert_eq!(
            fmt("List<int>    numbers=someFunction();"),
            "List<int> numbers = someFunction();"
        );
        assert_eq!(
            fmt("Dictionary<string,int> map;"),
            "Dictionary<string, int> map;"
        );
    }

    #[test]
    fn array_type_declaration_is_formatted() {
        assert_eq!(fmt("int[]    values;"), "int[] values;");
        assert_eq!(fmt("int[,] grid;"), "int[,] grid;");
    }

    #[test]
    fn custom_type_declaration_is_formatted() {
        assert_eq!(fmt("Customer    c;"), "Customer c;");
    }

    // --- method / function declarations ------------------------------------

    #[test]
    fn no_arg_method_signature_is_formatted() {
        assert_eq!(fmt("void   Foo(   )"), "void Foo()");
    }

    #[test]
    fn method_signature_with_trailing_brace_is_formatted() {
        assert_eq!(fmt("void Foo(){"), "void Foo() {");
    }

    #[test]
    fn method_signature_with_params_is_formatted() {
        assert_eq!(
            fmt("void Foo(int x,string y)"),
            "void Foo(int x, string y)"
        );
    }

    #[test]
    fn method_signature_with_modifiers_and_return_type_is_formatted() {
        assert_eq!(
            fmt("public   static   int   Add(int a,int b)"),
            "public static int Add(int a, int b)"
        );
    }

    #[test]
    fn method_signature_with_default_parameter_is_formatted() {
        assert_eq!(
            fmt("void Foo(int x=5)"),
            "void Foo(int x = 5)"
        );
    }

    #[test]
    fn full_method_body_is_reindented_and_reformatted() {
        let input = concat!(
            "public   static   void Add(int a,int b)\r\n",
            "{\r\n",
            "var sum=a+b;\r\n",
            "Console.WriteLine(sum);\r\n",
            "}",
        );
        let expected = concat!(
            "public static void Add(int a, int b)\r\n",
            "{\r\n",
            "    var sum = a + b;\r\n",
            "    Console.WriteLine(sum);\r\n",
            "}",
        );
        assert_eq!(fmt(input), expected);
    }

    #[test]
    fn unrelated_lines_that_look_like_blocks_are_left_alone_but_reindented() {
        // `if (...)` isn't a declaration or a method signature, so its
        // content is untouched -- only its indentation is recomputed.
        let input = "void Foo()\r\n{\r\nif (x>0)\r\n{\r\n}\r\n}";
        let expected = "void Foo()\r\n{\r\n    if (x>0)\r\n    {\r\n    }\r\n}";
        assert_eq!(fmt(input), expected);
    }
}
