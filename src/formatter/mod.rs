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
mod interpolated;
mod statement;
mod token;

use std::fmt;

use crate::editorconfig::{EditorConfig, IndentSize, IndentStyle};
use token::{Token, tokenize};

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
    let columns = match config.indent_size {
        IndentSize::Columns(n) => n,
        IndentSize::UseTabWidth => config.tab_width,
    };
    match config.indent_style {
        IndentStyle::Tab => {
            let tab_width = config.tab_width.max(1);
            let tabs = columns.div_ceil(tab_width).max(1);
            "\t".repeat(tabs as usize)
        }
        IndentStyle::Space => " ".repeat(columns as usize),
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

    let opens = tokens
        .iter()
        .filter(|t| matches!(t, Token::Symbol('{')))
        .count() as i32;
    let closes = tokens
        .iter()
        .filter(|t| matches!(t, Token::Symbol('}')))
        .count() as i32;
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

    #[test]
    fn tab_indent_uses_multiple_tabs_when_narrower_than_indent_size() {
        // indent_size=4 columns, tab_width=2 columns/tab -> 2 tabs per level.
        let config = EditorConfig {
            indent_style: IndentStyle::Tab,
            indent_size: IndentSize::Columns(4),
            tab_width: 2,
        };
        let input = "void Foo()\r\n{\r\nvar x=5;\r\n}";
        let expected = "void Foo()\r\n{\r\n\t\tvar x = 5;\r\n}";
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
    fn formats_bitwise_operators() {
        assert_eq!(fmt("var x=a&b;"), "var x = a & b;");
        assert_eq!(fmt("var x=a^b;"), "var x = a ^ b;");
        assert_eq!(fmt("var x=a|b;"), "var x = a | b;");
    }

    #[test]
    fn formats_shift_operators() {
        assert_eq!(fmt("var x=a<<2;"), "var x = a << 2;");
        assert_eq!(fmt("var x=a>>2;"), "var x = a >> 2;");
    }

    #[test]
    fn shift_binds_tighter_than_relational() {
        assert_eq!(fmt("var x=a<<b<c;"), "var x = a << b < c;");
    }

    #[test]
    fn nested_generic_closing_brackets_still_parse() {
        assert_eq!(
            fmt("List<List<int>> x=new List<List<int>>();"),
            "List<List<int>> x = new List<List<int>>();"
        );
    }

    #[test]
    fn formats_null_coalescing_operator() {
        assert_eq!(fmt("var x=a??b;"), "var x = a ?? b;");
    }

    #[test]
    fn nested_null_coalescing_is_right_associative() {
        assert_eq!(fmt("var x=a??b??c;"), "var x = a ?? b ?? c;");
    }

    #[test]
    fn null_coalescing_binds_looser_than_logical_or_and_tighter_than_ternary() {
        assert_eq!(fmt("var x=a||b??c?d:e;"), "var x = a || b ?? c ? d : e;");
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

    #[test]
    fn ternary_expression_is_formatted() {
        assert_eq!(fmt("var x=a?b:c;"), "var x = a ? b : c;");
    }

    #[test]
    fn nested_ternary_expression_is_right_associative() {
        assert_eq!(fmt("var x=a?b:c?d:e;"), "var x = a ? b : c ? d : e;");
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

    // --- string interpolation ------------------------------------------------

    #[test]
    fn interpolated_string_without_holes_is_recognized() {
        assert_eq!(fmt("var s=$\"hello world\";"), "var s = $\"hello world\";");
    }

    #[test]
    fn interpolated_string_hole_is_reformatted() {
        assert_eq!(
            fmt("var s=$\"Hello {name+1}\";"),
            "var s = $\"Hello {name + 1}\";"
        );
    }

    #[test]
    fn interpolated_string_multiple_holes_are_reformatted() {
        assert_eq!(
            fmt("var s=$\"{a+b} and {c*d}\";"),
            "var s = $\"{a + b} and {c * d}\";"
        );
    }

    #[test]
    fn interpolated_string_escaped_braces_are_preserved() {
        assert_eq!(fmt("var s=$\"{{literal}}\";"), "var s = $\"{{literal}}\";");
    }

    #[test]
    fn interpolated_string_nested_call_args_are_not_mistaken_for_alignment() {
        // The comma inside `Foo(a,b)` is nested inside parens, so it
        // must not be mistaken for the hole's own alignment separator.
        assert_eq!(fmt("var s=$\"{Foo(a,b)}\";"), "var s = $\"{Foo(a, b)}\";");
    }

    #[test]
    fn interpolated_string_format_specifier_is_preserved() {
        assert_eq!(
            fmt("var s=$\"{value+1:N2}\";"),
            "var s = $\"{value + 1:N2}\";"
        );
    }

    #[test]
    fn interpolated_string_alignment_is_preserved() {
        assert_eq!(
            fmt("var s=$\"{value+1,10}\";"),
            "var s = $\"{value + 1,10}\";"
        );
    }

    #[test]
    fn interpolated_string_nested_string_literal_in_hole_is_preserved() {
        assert_eq!(
            fmt("var s=$\"{x??\"none\"}\";"),
            "var s = $\"{x ?? \"none\"}\";"
        );
    }

    #[test]
    fn interpolated_string_unparseable_hole_falls_back() {
        // Lambdas aren't part of the expression grammar; this hole is
        // left untouched rather than failing the whole string.
        assert_eq!(
            fmt("var s=$\"{items.Select(x=>x+1)}\";"),
            "var s = $\"{items.Select(x=>x+1)}\";"
        );
    }

    #[test]
    fn verbatim_interpolated_string_dollar_at_is_recognized() {
        assert_eq!(
            fmt("var s=$@\"C:\\{dir+1}\\path\";"),
            "var s = $@\"C:\\{dir + 1}\\path\";"
        );
    }

    #[test]
    fn nested_interpolated_string_hole_is_reformatted_recursively() {
        // A hole's own expression is reformatted through the same
        // pipeline, so a nested interpolated string's holes are
        // reformatted too -- an emergent property of the recursion,
        // not special-cased.
        assert_eq!(
            fmt("var s=$\"outer {$\"inner {x+1}\"} end\";"),
            "var s = $\"outer {$\"inner {x + 1}\"} end\";"
        );
    }

    #[test]
    fn unterminated_interpolated_string_is_left_untouched() {
        // No closing quote, and so no terminating `;` either -- none
        // of the statement-shape parsers recognize this, so (as with
        // any other incomplete line) the whole line is left alone
        // rather than guessed at.
        assert_eq!(fmt("var s=$\"unterminated {x"), "var s=$\"unterminated {x");
    }

    #[test]
    fn adjacent_holes_with_no_literal_text_between_are_reformatted() {
        assert_eq!(fmt("var s=$\"{a}{b}{c}\";"), "var s = $\"{a}{b}{c}\";");
    }

    #[test]
    fn verbatim_interpolated_string_at_dollar_is_recognized() {
        assert_eq!(
            fmt("var s=@$\"C:\\{dir+1}\\path\";"),
            "var s = @$\"C:\\{dir + 1}\\path\";"
        );
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
    fn qualified_type_declaration_is_formatted() {
        assert_eq!(
            fmt("System.Text.StringBuilder   sb;"),
            "System.Text.StringBuilder sb;"
        );
    }

    #[test]
    fn qualified_generic_type_declaration_is_formatted() {
        assert_eq!(
            fmt("System.Collections.Generic.List<int>   list=someFunction();"),
            "System.Collections.Generic.List<int> list = someFunction();"
        );
    }

    #[test]
    fn qualified_type_as_generic_argument_is_formatted() {
        assert_eq!(
            fmt("List<System.String>   names;"),
            "List<System.String> names;"
        );
    }

    #[test]
    fn qualified_type_in_new_expression_is_formatted() {
        assert_eq!(
            fmt("var sb=new   System.Text.StringBuilder();"),
            "var sb = new System.Text.StringBuilder();"
        );
    }

    #[test]
    fn qualified_return_type_is_formatted() {
        assert_eq!(fmt("System.String   Foo()"), "System.String Foo()");
    }

    #[test]
    fn qualified_foreach_element_type_is_formatted() {
        assert_eq!(
            fmt("foreach(System.String   s   in   items)"),
            "foreach (System.String s in items)"
        );
    }

    #[test]
    fn global_alias_qualifier_is_formatted() {
        assert_eq!(
            fmt("global::System.String   s;"),
            "global::System.String s;"
        );
    }

    #[test]
    fn extern_alias_qualifier_is_formatted() {
        assert_eq!(fmt("MyAlias::Foo   f;"), "MyAlias::Foo f;");
    }

    #[test]
    fn trailing_alias_qualifier_without_identifier_is_left_alone() {
        // `::` not followed by an identifier isn't guessed at; `global`
        // is still returned as the parsed type, so the leftover `::`
        // trips the declarator check and the whole line is left
        // untouched.
        let line = "global::;";
        assert_eq!(fmt(line), line);
    }

    #[test]
    fn trailing_dot_in_type_position_is_left_alone() {
        // A `.` not followed by an identifier isn't guessed at; every
        // statement shape that could still make sense of the leftover
        // dot fails too, so the whole (malformed) line is left
        // untouched rather than mangled.
        let line = "System.Collections.;";
        assert_eq!(fmt(line), line);
    }

    #[test]
    fn custom_type_declaration_is_formatted() {
        assert_eq!(fmt("Customer    c;"), "Customer c;");
    }

    // --- object creation -----------------------------------------------

    #[test]
    fn object_creation_with_no_args_is_formatted() {
        assert_eq!(fmt("var x=new Foo();"), "var x = new Foo();");
    }

    #[test]
    fn object_creation_with_args_is_formatted() {
        assert_eq!(fmt("var x=new Foo(1,2);"), "var x = new Foo(1, 2);");
    }

    #[test]
    fn object_creation_with_generic_type_is_formatted() {
        assert_eq!(
            fmt("var seen=new List<int>();"),
            "var seen = new List<int>();"
        );
    }

    #[test]
    fn target_typed_object_creation_is_formatted() {
        assert_eq!(fmt("List<int>   x=new();"), "List<int> x = new();");
    }

    #[test]
    fn object_creation_as_call_argument_is_formatted() {
        assert_eq!(fmt("Foo(new   Bar( ));"), "Foo(new Bar());");
    }

    #[test]
    fn object_creation_with_chained_member_access_is_formatted() {
        assert_eq!(
            fmt("var x=new Foo().DoThing();"),
            "var x = new Foo().DoThing();"
        );
    }

    #[test]
    fn sized_array_creation_is_formatted() {
        assert_eq!(fmt("var x=new   int[5];"), "var x = new int[5];");
    }

    #[test]
    fn multi_dimensional_sized_array_creation_is_formatted() {
        assert_eq!(fmt("var x=new int[2,3];"), "var x = new int[2, 3];");
    }

    #[test]
    fn multi_dimensional_sized_array_creation_with_initializer_is_formatted() {
        assert_eq!(
            fmt("var x=new int[2,3]{1,2,3,4,5,6};"),
            "var x = new int[2, 3] { 1, 2, 3, 4, 5, 6 };"
        );
    }

    #[test]
    fn sized_array_creation_with_initializer_is_formatted() {
        assert_eq!(
            fmt("var x=new int[5]{1,2,3,4,5};"),
            "var x = new int[5] { 1, 2, 3, 4, 5 };"
        );
    }

    #[test]
    fn array_creation_with_initializer_is_formatted() {
        assert_eq!(
            fmt("var x=new int[]{1,2,3};"),
            "var x = new int[] { 1, 2, 3 };"
        );
    }

    #[test]
    fn implicitly_typed_array_creation_is_formatted() {
        assert_eq!(fmt("var x=new[]{1,2,3};"), "var x = new[] { 1, 2, 3 };");
    }

    #[test]
    fn empty_array_initializer_is_formatted() {
        assert_eq!(fmt("var x=new int[]{};"), "var x = new int[] { };");
    }

    #[test]
    fn bare_array_initializer_in_declarator_is_formatted() {
        assert_eq!(fmt("int[]   arr={1,2,3};"), "int[] arr = { 1, 2, 3 };");
    }

    #[test]
    fn nested_array_initializer_is_formatted() {
        assert_eq!(
            fmt("var x=new int[,]{{1,2},{3,4}};"),
            "var x = new int[,] { { 1, 2 }, { 3, 4 } };"
        );
    }

    #[test]
    fn object_initializer_without_parens_is_formatted() {
        assert_eq!(fmt("var x=new Foo{X=1};"), "var x = new Foo { X = 1 };");
    }

    #[test]
    fn object_initializer_with_empty_parens_is_formatted() {
        // The empty `()` is preserved as written, not dropped --
        // `new Foo()` and `new Foo` without parens are both valid but
        // distinct source forms.
        assert_eq!(fmt("var x=new Foo(){X=1};"), "var x = new Foo() { X = 1 };");
    }

    #[test]
    fn object_initializer_with_constructor_args_is_formatted() {
        assert_eq!(
            fmt("var x=new Foo(1,2){X=1,Y=2};"),
            "var x = new Foo(1, 2) { X = 1, Y = 2 };"
        );
    }

    #[test]
    fn target_typed_new_with_initializer_is_formatted() {
        assert_eq!(fmt("Foo x=new(){X=1};"), "Foo x = new() { X = 1 };");
    }

    #[test]
    fn collection_initializer_is_formatted() {
        assert_eq!(
            fmt("var x=new List<int>{1,2,3};"),
            "var x = new List<int> { 1, 2, 3 };"
        );
    }

    #[test]
    fn nested_object_initializer_is_formatted() {
        assert_eq!(
            fmt("var x=new Foo{Bar=new Baz{X=1}};"),
            "var x = new Foo { Bar = new Baz { X = 1 } };"
        );
    }

    #[test]
    fn empty_object_initializer_is_formatted() {
        assert_eq!(fmt("var x=new Foo{};"), "var x = new Foo { };");
    }

    #[test]
    fn malformed_object_initializer_is_left_alone() {
        let line = "var x = new Foo { X = };";
        assert_eq!(fmt(line), line);
    }

    #[test]
    fn indexed_collection_initializer_is_formatted() {
        assert_eq!(
            fmt("var x=new Dictionary<string,int>{[\"a\"]=1};"),
            "var x = new Dictionary<string, int> { [\"a\"] = 1 };"
        );
    }

    #[test]
    fn indexed_collection_initializer_with_multiple_entries_is_formatted() {
        assert_eq!(
            fmt("var x=new Dictionary<string,int>{[\"a\"]=1,[\"b\"]=2};"),
            "var x = new Dictionary<string, int> { [\"a\"] = 1, [\"b\"] = 2 };"
        );
    }

    #[test]
    fn indexed_collection_initializer_with_multiple_keys_is_formatted() {
        // A multi-dimensional indexer (e.g. a 2D grid type) can take
        // more than one key expression per entry.
        assert_eq!(
            fmt("var x=new Grid{[0,0]=1};"),
            "var x = new Grid { [0, 0] = 1 };"
        );
    }

    #[test]
    fn malformed_indexed_collection_initializer_is_left_alone() {
        let line = "var x = new Dictionary<string, int> { [\"a\"] 1 };";
        assert_eq!(fmt(line), line);
    }

    // --- indexers --------------------------------------------------------

    #[test]
    fn indexer_read_is_formatted() {
        assert_eq!(fmt("var x=seen[0];"), "var x = seen[0];");
    }

    #[test]
    fn indexer_with_string_key_is_formatted() {
        assert_eq!(fmt("var x=map[\"key\"];"), "var x = map[\"key\"];");
    }

    #[test]
    fn multi_dimensional_indexer_is_formatted() {
        assert_eq!(fmt("var x=matrix[i,j];"), "var x = matrix[i, j];");
    }

    #[test]
    fn chained_indexers_are_formatted() {
        assert_eq!(fmt("var x=matrix[i][j];"), "var x = matrix[i][j];");
    }

    #[test]
    fn indexer_combined_with_member_access_is_formatted() {
        assert_eq!(fmt("var x=obj.Items[0];"), "var x = obj.Items[0];");
        assert_eq!(
            fmt("var x=seen[0].ToString();"),
            "var x = seen[0].ToString();"
        );
    }

    #[test]
    fn indexer_assignment_is_formatted() {
        assert_eq!(fmt("seen[0]=5;"), "seen[0] = 5;");
        assert_eq!(fmt("seen[0]+=1;"), "seen[0] += 1;");
    }

    #[test]
    fn indexer_as_call_argument_is_formatted() {
        assert_eq!(fmt("Foo(seen[ 0 ]);"), "Foo(seen[0]);");
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
        assert_eq!(fmt("void Foo(int x,string y)"), "void Foo(int x, string y)");
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
        assert_eq!(fmt("void Foo(int x=5)"), "void Foo(int x = 5)");
    }

    #[test]
    fn constructor_with_no_return_type_is_formatted() {
        assert_eq!(fmt("public   Foo(int   x)"), "public Foo(int x)");
    }

    #[test]
    fn no_arg_constructor_with_trailing_brace_is_formatted() {
        assert_eq!(fmt("public Foo(){"), "public Foo() {");
    }

    #[test]
    fn generic_method_type_parameter_list_is_formatted() {
        assert_eq!(fmt("T   Foo<T>(T   x)"), "T Foo<T>(T x)");
    }

    #[test]
    fn generic_method_with_multiple_type_parameters_is_formatted() {
        assert_eq!(
            fmt("public static TResult Map<TInput,TResult>(TInput x)"),
            "public static TResult Map<TInput, TResult>(TInput x)"
        );
    }

    #[test]
    fn generic_method_type_parameter_constraint_is_formatted() {
        assert_eq!(
            fmt("T   Foo<T>(T x)where T:class"),
            "T Foo<T>(T x) where T : class"
        );
    }

    #[test]
    fn generic_method_with_multiple_constraints_is_formatted() {
        assert_eq!(
            fmt("T Foo<T>(T x) where T : IDisposable, new()"),
            "T Foo<T>(T x) where T : IDisposable, new()"
        );
    }

    #[test]
    fn generic_method_with_multiple_where_clauses_is_formatted() {
        assert_eq!(
            fmt("void Foo<T,U>(T x,U y) where T : class where U : struct"),
            "void Foo<T, U>(T x, U y) where T : class where U : struct"
        );
    }

    // --- class declarations ------------------------------------------

    #[test]
    fn class_declaration_is_formatted() {
        assert_eq!(fmt("class   Foo"), "class Foo");
    }

    #[test]
    fn class_declaration_with_trailing_brace_is_formatted() {
        assert_eq!(
            fmt("public static class  Program  {"),
            "public static class Program {"
        );
    }

    #[test]
    fn class_declaration_with_base_list_is_formatted() {
        assert_eq!(
            fmt("public class Foo:Bar,IBaz"),
            "public class Foo : Bar, IBaz"
        );
    }

    #[test]
    fn generic_class_declaration_is_formatted() {
        assert_eq!(fmt("class Foo<T>"), "class Foo<T>");
    }

    #[test]
    fn generic_class_declaration_with_constraint_is_formatted() {
        assert_eq!(
            fmt("class   Foo<T>where T:class"),
            "class Foo<T> where T : class"
        );
    }

    #[test]
    fn generic_class_declaration_with_base_list_and_constraint_is_formatted() {
        assert_eq!(
            fmt("public class Foo<T>:IBaz where T : struct"),
            "public class Foo<T> : IBaz where T : struct"
        );
    }

    // --- if statements --------------------------------------------------

    #[test]
    fn if_statement_condition_is_formatted() {
        assert_eq!(fmt("if(sum>0)"), "if (sum > 0)");
    }

    #[test]
    fn if_statement_with_trailing_brace_is_formatted() {
        assert_eq!(fmt("if (sum>0){"), "if (sum > 0) {");
    }

    #[test]
    fn if_statement_with_logical_condition_is_formatted() {
        assert_eq!(fmt("if(sum!=0&&sum<100)"), "if (sum != 0 && sum < 100)");
    }

    #[test]
    fn malformed_if_statement_is_left_alone() {
        assert_eq!(fmt("if (sum>0 extra"), "if (sum>0 extra");
    }

    // --- else statements --------------------------------------------------

    #[test]
    fn bare_else_is_formatted() {
        assert_eq!(fmt("else"), "else");
    }

    #[test]
    fn bare_else_with_trailing_brace_is_formatted() {
        assert_eq!(fmt("else{"), "else {");
    }

    #[test]
    fn else_if_condition_is_formatted() {
        assert_eq!(fmt("else   if(sum>0)"), "else if (sum > 0)");
    }

    #[test]
    fn else_if_with_trailing_brace_is_formatted() {
        assert_eq!(fmt("else if(sum>0){"), "else if (sum > 0) {");
    }

    #[test]
    fn malformed_else_statement_is_left_alone() {
        assert_eq!(fmt("else while (true)"), "else while (true)");
    }

    #[test]
    fn bare_else_on_its_own_line_is_reindented_and_reformatted() {
        let input = concat!(
            "void Foo()\r\n",
            "{\r\n",
            "if(x>0)\r\n",
            "{\r\n",
            "Console.WriteLine(\"positive\");\r\n",
            "}\r\n",
            "else\r\n",
            "{\r\n",
            "Console.WriteLine(\"non-positive\");\r\n",
            "}\r\n",
            "}",
        );
        let expected = concat!(
            "void Foo()\r\n",
            "{\r\n",
            "    if (x > 0)\r\n",
            "    {\r\n",
            "        Console.WriteLine(\"positive\");\r\n",
            "    }\r\n",
            "    else\r\n",
            "    {\r\n",
            "        Console.WriteLine(\"non-positive\");\r\n",
            "    }\r\n",
            "}",
        );
        assert_eq!(fmt(input), expected);
    }

    // --- while loops -----------------------------------------------------

    #[test]
    fn while_statement_condition_is_formatted() {
        assert_eq!(fmt("while(sum>0)"), "while (sum > 0)");
    }

    #[test]
    fn while_statement_with_trailing_brace_is_formatted() {
        assert_eq!(fmt("while (sum>0){"), "while (sum > 0) {");
    }

    #[test]
    fn malformed_while_statement_is_left_alone() {
        assert_eq!(fmt("while (sum>0 extra"), "while (sum>0 extra");
    }

    #[test]
    fn while_loop_is_reindented_and_reformatted() {
        let input = concat!(
            "void Foo()\r\n",
            "{\r\n",
            "while(sum>0)\r\n",
            "{\r\n",
            "sum-=1;\r\n",
            "}\r\n",
            "}",
        );
        let expected = concat!(
            "void Foo()\r\n",
            "{\r\n",
            "    while (sum > 0)\r\n",
            "    {\r\n",
            "        sum -= 1;\r\n",
            "    }\r\n",
            "}",
        );
        assert_eq!(fmt(input), expected);
    }

    // --- do/while loops ----------------------------------------------

    #[test]
    fn bare_do_is_formatted() {
        assert_eq!(fmt("do"), "do");
    }

    #[test]
    fn bare_do_with_trailing_brace_is_formatted() {
        assert_eq!(fmt("do{"), "do {");
    }

    #[test]
    fn do_while_tail_condition_is_formatted() {
        assert_eq!(fmt("while(sum>0);"), "while (sum > 0);");
    }

    #[test]
    fn malformed_do_while_tail_is_left_alone() {
        assert_eq!(fmt("while(sum>0 extra);"), "while(sum>0 extra);");
    }

    #[test]
    fn do_while_loop_is_reindented_and_reformatted() {
        let input = concat!(
            "void Foo()\r\n",
            "{\r\n",
            "do\r\n",
            "{\r\n",
            "sum-=1;\r\n",
            "}\r\n",
            "while(sum>0);\r\n",
            "}",
        );
        let expected = concat!(
            "void Foo()\r\n",
            "{\r\n",
            "    do\r\n",
            "    {\r\n",
            "        sum -= 1;\r\n",
            "    }\r\n",
            "    while (sum > 0);\r\n",
            "}",
        );
        assert_eq!(fmt(input), expected);
    }

    #[test]
    fn combined_do_while_close_condition_is_formatted() {
        assert_eq!(fmt("} while(sum>0);"), "} while (sum > 0);");
    }

    #[test]
    fn malformed_combined_do_while_close_is_left_alone() {
        assert_eq!(fmt("} while(sum>0 extra);"), "} while(sum>0 extra);");
    }

    #[test]
    fn do_while_loop_with_combined_close_is_reindented_and_reformatted() {
        let input = concat!(
            "void Foo()\r\n",
            "{\r\n",
            "do\r\n",
            "{\r\n",
            "sum-=1;\r\n",
            "} while(sum>0);\r\n",
            "}",
        );
        let expected = concat!(
            "void Foo()\r\n",
            "{\r\n",
            "    do\r\n",
            "    {\r\n",
            "        sum -= 1;\r\n",
            "    } while (sum > 0);\r\n",
            "}",
        );
        assert_eq!(fmt(input), expected);
    }

    // --- foreach loops -----------------------------------------------

    #[test]
    fn foreach_statement_is_formatted() {
        assert_eq!(
            fmt("foreach(int   x   in   items)"),
            "foreach (int x in items)"
        );
    }

    #[test]
    fn foreach_statement_with_var_is_formatted() {
        assert_eq!(fmt("foreach(var x in items)"), "foreach (var x in items)");
    }

    #[test]
    fn foreach_statement_with_trailing_brace_is_formatted() {
        assert_eq!(
            fmt("foreach (var x in items){"),
            "foreach (var x in items) {"
        );
    }

    #[test]
    fn foreach_statement_with_complex_collection_is_formatted() {
        assert_eq!(
            fmt("foreach(var x in list.GetItems())"),
            "foreach (var x in list.GetItems())"
        );
    }

    #[test]
    fn foreach_statement_with_custom_type_is_formatted() {
        assert_eq!(
            fmt("foreach(Customer c   in   customers)"),
            "foreach (Customer c in customers)"
        );
    }

    #[test]
    fn foreach_statement_with_generic_type_is_formatted() {
        assert_eq!(
            fmt("foreach(List<int> nums in nestedLists)"),
            "foreach (List<int> nums in nestedLists)"
        );
    }

    #[test]
    fn foreach_loop_with_typed_element_is_reindented_and_reformatted() {
        let input = concat!(
            "void Foo()\r\n",
            "{\r\n",
            "foreach(string   name   in   names)\r\n",
            "{\r\n",
            "Console.WriteLine(name);\r\n",
            "}\r\n",
            "}",
        );
        let expected = concat!(
            "void Foo()\r\n",
            "{\r\n",
            "    foreach (string name in names)\r\n",
            "    {\r\n",
            "        Console.WriteLine(name);\r\n",
            "    }\r\n",
            "}",
        );
        assert_eq!(fmt(input), expected);
    }

    #[test]
    fn malformed_foreach_statement_is_left_alone() {
        assert_eq!(fmt("foreach (var x items)"), "foreach (var x items)");
    }

    #[test]
    fn foreach_with_tuple_deconstruction_is_formatted() {
        assert_eq!(
            fmt("foreach(var   (k,v)   in   dict)"),
            "foreach (var (k, v) in dict)"
        );
    }

    #[test]
    fn foreach_with_two_element_tuple_deconstruction_and_trailing_brace_is_formatted() {
        assert_eq!(
            fmt("foreach (var (name, age) in people){"),
            "foreach (var (name, age) in people) {"
        );
    }

    #[test]
    fn malformed_foreach_tuple_deconstruction_is_left_alone() {
        let line = "foreach (var (k, v items)";
        assert_eq!(fmt(line), line);
    }

    #[test]
    fn foreach_loop_is_reindented_and_reformatted() {
        let input = concat!(
            "void Foo()\r\n",
            "{\r\n",
            "foreach(var x   in   items)\r\n",
            "{\r\n",
            "Console.WriteLine(x);\r\n",
            "}\r\n",
            "}",
        );
        let expected = concat!(
            "void Foo()\r\n",
            "{\r\n",
            "    foreach (var x in items)\r\n",
            "    {\r\n",
            "        Console.WriteLine(x);\r\n",
            "    }\r\n",
            "}",
        );
        assert_eq!(fmt(input), expected);
    }

    // --- C-style for loops -------------------------------------------

    #[test]
    fn for_statement_is_formatted() {
        assert_eq!(
            fmt("for(int   i=0;i<10;i++)"),
            "for (int i = 0; i < 10; i++)"
        );
    }

    #[test]
    fn for_statement_with_trailing_brace_is_formatted() {
        assert_eq!(
            fmt("for (int i = 0; i < 10; i++){"),
            "for (int i = 0; i < 10; i++) {"
        );
    }

    #[test]
    fn for_statement_with_multiple_declarators_and_increments_is_formatted() {
        assert_eq!(
            fmt("for(int i=0,j=10;i<j;i++,j--)"),
            "for (int i = 0, j = 10; i < j; i++, j--)"
        );
    }

    #[test]
    fn for_statement_with_empty_parts_is_formatted() {
        assert_eq!(fmt("for(;;)"), "for (; ; )");
    }

    #[test]
    fn for_statement_with_empty_init_is_formatted() {
        assert_eq!(fmt("for(;i<10;i++)"), "for (; i < 10; i++)");
    }

    #[test]
    fn for_statement_with_empty_condition_is_formatted() {
        assert_eq!(fmt("for(int i=0;;i++)"), "for (int i = 0; ; i++)");
    }

    #[test]
    fn for_statement_with_empty_increment_is_formatted() {
        assert_eq!(fmt("for(int i=0;i<10;)"), "for (int i = 0; i < 10; )");
    }

    #[test]
    fn for_statement_with_untyped_assignment_init_is_formatted() {
        assert_eq!(fmt("for(i=0;i<10;i++)"), "for (i = 0; i < 10; i++)");
    }

    #[test]
    fn for_statement_with_multiple_untyped_init_expressions_is_formatted() {
        assert_eq!(
            fmt("for(i=0,j=10;i<j;i++,j--)"),
            "for (i = 0, j = 10; i < j; i++, j--)"
        );
    }

    #[test]
    fn malformed_for_statement_is_left_alone() {
        assert_eq!(
            fmt("for(int i=0;i<10 extra;i++)"),
            "for(int i=0;i<10 extra;i++)"
        );
    }

    #[test]
    fn for_loop_is_reindented_and_reformatted() {
        let input = concat!(
            "void Foo()\r\n",
            "{\r\n",
            "for(int   i=0;i<10;i++)\r\n",
            "{\r\n",
            "Console.WriteLine(i);\r\n",
            "}\r\n",
            "}",
        );
        let expected = concat!(
            "void Foo()\r\n",
            "{\r\n",
            "    for (int i = 0; i < 10; i++)\r\n",
            "    {\r\n",
            "        Console.WriteLine(i);\r\n",
            "    }\r\n",
            "}",
        );
        assert_eq!(fmt(input), expected);
    }

    // --- switch statements ------------------------------------------

    #[test]
    fn switch_statement_condition_is_formatted() {
        assert_eq!(fmt("switch(x)"), "switch (x)");
    }

    #[test]
    fn switch_statement_with_trailing_brace_is_formatted() {
        assert_eq!(fmt("switch (x){"), "switch (x) {");
    }

    #[test]
    fn case_label_is_formatted() {
        assert_eq!(fmt("case   1:"), "case 1:");
    }

    #[test]
    fn case_label_with_string_is_formatted() {
        assert_eq!(fmt("case \"foo\":"), "case \"foo\":");
    }

    #[test]
    fn case_label_with_member_access_is_formatted() {
        assert_eq!(fmt("case Color.Red:"), "case Color.Red:");
    }

    #[test]
    fn default_label_is_formatted() {
        assert_eq!(fmt("default  :"), "default:");
    }

    #[test]
    fn case_label_with_type_pattern_is_formatted() {
        assert_eq!(fmt("case   int   i:"), "case int i:");
    }

    #[test]
    fn case_label_with_qualified_type_pattern_is_formatted() {
        // Emerges for free from parse_type's qualified-name support --
        // `parse_primary_pattern`'s type pattern reuses parse_type.
        assert_eq!(fmt("case System.String   s:"), "case System.String s:");
    }

    #[test]
    fn case_label_with_generic_type_pattern_is_formatted() {
        assert_eq!(fmt("case List<int>   items:"), "case List<int> items:");
    }

    #[test]
    fn case_label_with_discard_type_pattern_is_formatted() {
        assert_eq!(fmt("case int   _:"), "case int _:");
    }

    #[test]
    fn case_label_with_relational_pattern_is_formatted() {
        assert_eq!(fmt("case   >5:"), "case > 5:");
        assert_eq!(fmt("case<=0:"), "case <= 0:");
    }

    #[test]
    fn case_label_with_when_guard_is_formatted() {
        assert_eq!(fmt("case int   i   when   i>0:"), "case int i when i > 0:");
    }

    #[test]
    fn case_label_with_relational_pattern_and_when_guard_is_formatted() {
        assert_eq!(fmt("case >0   when   sum<100:"), "case > 0 when sum < 100:");
    }

    #[test]
    fn case_label_with_member_access_is_still_an_expression_pattern() {
        // Regression check: `Color.Red` shouldn't be misparsed as a
        // type pattern `Color` binding `Red`, since backtracking
        // should reject it at the `.` and fall back to an expression.
        assert_eq!(fmt("case   Color.Red:"), "case Color.Red:");
    }

    #[test]
    fn case_label_with_and_pattern_is_formatted() {
        assert_eq!(fmt("case int   i   and   >0:"), "case int i and > 0:");
    }

    #[test]
    fn case_label_with_or_pattern_is_formatted() {
        assert_eq!(fmt("case >0   or   <-10:"), "case > 0 or < -10:");
    }

    #[test]
    fn case_label_with_not_pattern_is_formatted() {
        assert_eq!(fmt("case   not   null:"), "case not null:");
    }

    #[test]
    fn case_label_with_and_binds_tighter_than_or() {
        // `and` should bind tighter than `or`, matching C#'s pattern
        // precedence: `a or b and c` means `a or (b and c)`, not `(a
        // or b) and c`. The input has no explicit grouping, so this
        // just confirms parsing doesn't choke on the mixed combinator
        // chain and preserves left-to-right order.
        assert_eq!(
            fmt("case >0   or   int   i   and   <100:"),
            "case > 0 or int i and < 100:"
        );
    }

    #[test]
    fn case_label_with_combinators_and_when_guard_is_formatted() {
        assert_eq!(
            fmt("case int   i   and   >0   when   i<100:"),
            "case int i and > 0 when i < 100:"
        );
    }

    #[test]
    fn case_label_with_parenthesized_sub_pattern_is_formatted() {
        assert_eq!(
            fmt("case (int   i   and   >0)   or   <-10:"),
            "case (int i and > 0) or < -10:"
        );
    }

    #[test]
    fn case_label_with_parenthesized_sub_pattern_and_when_guard_is_formatted() {
        assert_eq!(
            fmt("case (int i and >0) or <-10 when flag:"),
            "case (int i and > 0) or < -10 when flag:"
        );
    }

    #[test]
    fn malformed_case_label_is_left_alone() {
        assert_eq!(fmt("case 1 extra:"), "case 1 extra:");
    }

    #[test]
    fn malformed_parenthesized_sub_pattern_is_left_alone() {
        let line = "case (int i and >0 or <-10:";
        assert_eq!(fmt(line), line);
    }

    // --- property and list patterns ----------------------------------

    #[test]
    fn case_label_with_empty_property_pattern_is_formatted() {
        assert_eq!(fmt("case   {   }  :"), "case { }:");
    }

    #[test]
    fn case_label_with_property_pattern_is_formatted() {
        assert_eq!(fmt("case   {   Length:0  }  :"), "case { Length: 0 }:");
    }

    #[test]
    fn case_label_with_typed_property_pattern_is_formatted() {
        assert_eq!(fmt("case   Point{X:0,Y:0}:"), "case Point { X: 0, Y: 0 }:");
    }

    #[test]
    fn case_label_with_extended_property_pattern_is_formatted() {
        assert_eq!(
            fmt("case   {   Address.City  :  \"X\" }:"),
            "case { Address.City: \"X\" }:"
        );
    }

    #[test]
    fn case_label_with_property_pattern_designation_is_formatted() {
        assert_eq!(fmt("case   {   X:0  }   p  :"), "case { X: 0 } p:");
    }

    #[test]
    fn case_label_with_nested_property_pattern_is_formatted() {
        assert_eq!(
            fmt("case   {   Location  :  { X:0, Y:0 }  }:"),
            "case { Location: { X: 0, Y: 0 } }:"
        );
    }

    #[test]
    fn case_label_with_combinator_inside_property_pattern_is_formatted() {
        assert_eq!(
            fmt("case   {   Age  :  >0   and   <100  }:"),
            "case { Age: > 0 and < 100 }:"
        );
    }

    #[test]
    fn case_label_with_empty_list_pattern_is_formatted() {
        assert_eq!(fmt("case   [   ]  :"), "case []:");
    }

    #[test]
    fn case_label_with_list_pattern_is_formatted() {
        assert_eq!(fmt("case   [1,2,3]  :"), "case [1, 2, 3]:");
    }

    #[test]
    fn case_label_with_slice_pattern_is_formatted() {
        assert_eq!(fmt("case   [1,  ..  ,9]  :"), "case [1, .., 9]:");
    }

    #[test]
    fn case_label_with_bound_slice_pattern_is_formatted() {
        assert_eq!(
            fmt("case   [1,  ..  var   rest  ,9]  :"),
            "case [1, .. var rest, 9]:"
        );
    }

    #[test]
    fn case_label_with_typed_list_pattern_is_formatted() {
        assert_eq!(fmt("case   List<int>[1,2]:"), "case List<int> [1, 2]:");
    }

    #[test]
    fn case_label_with_list_pattern_designation_is_formatted() {
        assert_eq!(fmt("case   [1,2]   rest  :"), "case [1, 2] rest:");
    }

    #[test]
    fn case_label_with_nested_list_pattern_is_formatted() {
        assert_eq!(fmt("case   [[1,2],[3,4]]:"), "case [[1, 2], [3, 4]]:");
    }

    #[test]
    fn malformed_property_pattern_is_left_alone() {
        let line = "case { X: }:";
        assert_eq!(fmt(line), line);
    }

    #[test]
    fn malformed_list_pattern_is_left_alone() {
        let line = "case [1, . 2]:";
        assert_eq!(fmt(line), line);
    }

    #[test]
    fn switch_statement_is_reindented_and_reformatted() {
        let input = concat!(
            "void Foo()\r\n",
            "{\r\n",
            "switch(x)\r\n",
            "{\r\n",
            "case   1:\r\n",
            "DoOne();\r\n",
            "break;\r\n",
            "default:\r\n",
            "DoDefault();\r\n",
            "break;\r\n",
            "}\r\n",
            "}",
        );
        let expected = concat!(
            "void Foo()\r\n",
            "{\r\n",
            "    switch (x)\r\n",
            "    {\r\n",
            "        case 1:\r\n",
            "        DoOne();\r\n",
            "        break;\r\n",
            "        default:\r\n",
            "        DoDefault();\r\n",
            "        break;\r\n",
            "    }\r\n",
            "}",
        );
        assert_eq!(fmt(input), expected);
    }

    // --- switch expressions -------------------------------------------

    #[test]
    fn switch_expression_header_is_formatted() {
        assert_eq!(fmt("var result=value switch"), "var result = value switch");
    }

    #[test]
    fn switch_expression_header_with_trailing_brace_is_formatted() {
        assert_eq!(
            fmt("var result=value switch{"),
            "var result = value switch {"
        );
    }

    #[test]
    fn switch_expression_arm_is_formatted() {
        assert_eq!(fmt("1=>\"one\","), "1 => \"one\",");
    }

    #[test]
    fn switch_expression_discard_arm_without_comma_is_formatted() {
        assert_eq!(fmt("_=>\"other\""), "_ => \"other\"");
    }

    #[test]
    fn switch_expression_arm_with_type_pattern_is_formatted() {
        assert_eq!(fmt("int   i=>\"int\","), "int i => \"int\",");
    }

    #[test]
    fn switch_expression_arm_with_relational_pattern_is_formatted() {
        assert_eq!(fmt(">0=>\"positive\","), "> 0 => \"positive\",");
    }

    #[test]
    fn switch_expression_arm_with_when_guard_is_formatted() {
        assert_eq!(
            fmt("int   i   when   i>0=>\"positive int\","),
            "int i when i > 0 => \"positive int\","
        );
    }

    #[test]
    fn switch_expression_arm_with_and_pattern_is_formatted() {
        assert_eq!(
            fmt("int   i   and   >0=>\"positive int\","),
            "int i and > 0 => \"positive int\","
        );
    }

    #[test]
    fn switch_expression_arm_with_or_pattern_is_formatted() {
        assert_eq!(
            fmt(">0   or   <-10=>\"extreme\","),
            "> 0 or < -10 => \"extreme\","
        );
    }

    #[test]
    fn switch_expression_arm_with_not_pattern_is_formatted() {
        assert_eq!(fmt("not   null=>\"present\","), "not null => \"present\",");
    }

    #[test]
    fn malformed_switch_expression_arm_is_left_alone() {
        assert_eq!(fmt("1=>\"one\" extra,"), "1=>\"one\" extra,");
    }

    #[test]
    fn switch_expression_is_reindented_and_reformatted() {
        let input = concat!(
            "void Foo()\r\n",
            "{\r\n",
            "var result=value switch\r\n",
            "{\r\n",
            "1=>\"one\",\r\n",
            "2=>\"two\",\r\n",
            "_=>\"other\",\r\n",
            "};\r\n",
            "}",
        );
        let expected = concat!(
            "void Foo()\r\n",
            "{\r\n",
            "    var result = value switch\r\n",
            "    {\r\n",
            "        1 => \"one\",\r\n",
            "        2 => \"two\",\r\n",
            "        _ => \"other\",\r\n",
            "    };\r\n",
            "}",
        );
        assert_eq!(fmt(input), expected);
    }

    #[test]
    fn inline_switch_expression_is_formatted() {
        assert_eq!(
            fmt("var label=value switch{1=>\"one\",_=>\"other\"};"),
            "var label = value switch { 1 => \"one\", _ => \"other\" };"
        );
    }

    #[test]
    fn inline_switch_expression_with_trailing_comma_is_formatted() {
        assert_eq!(
            fmt("var label=value switch{1=>\"one\",_=>\"other\",};"),
            "var label = value switch { 1 => \"one\", _ => \"other\" };"
        );
    }

    #[test]
    fn inline_switch_expression_with_pattern_combinators_is_formatted() {
        assert_eq!(
            fmt("var label=value switch{int   i   and   >0=>\"pos\",_=>\"other\"};"),
            "var label = value switch { int i and > 0 => \"pos\", _ => \"other\" };"
        );
    }

    #[test]
    fn malformed_inline_switch_expression_is_left_alone() {
        let input = "var label=value switch{1=>\"one\" extra};";
        assert_eq!(fmt(input), input);
    }

    #[test]
    fn switch_expression_as_call_argument_is_formatted() {
        // A switch expression is now part of the general expression
        // grammar, so it composes anywhere an expression can appear --
        // not just a declaration's initializer.
        assert_eq!(
            fmt("Foo(value   switch{1=>\"one\",_=>\"other\"});"),
            "Foo(value switch { 1 => \"one\", _ => \"other\" });"
        );
    }

    #[test]
    fn switch_expression_in_return_statement_is_formatted() {
        assert_eq!(
            fmt("return value switch{1=>\"one\",_=>\"other\"};"),
            "return value switch { 1 => \"one\", _ => \"other\" };"
        );
    }

    #[test]
    fn switch_expression_header_in_return_statement_is_formatted() {
        assert_eq!(fmt("return value   switch"), "return value switch");
    }

    #[test]
    fn multi_line_switch_expression_in_return_statement_is_reindented_and_reformatted() {
        let input = concat!(
            "string Foo()\r\n",
            "{\r\n",
            "return value switch\r\n",
            "{\r\n",
            "1 => \"one\",\r\n",
            "_ => \"other\",\r\n",
            "};\r\n",
            "}",
        );
        let expected = concat!(
            "string Foo()\r\n",
            "{\r\n",
            "    return value switch\r\n",
            "    {\r\n",
            "        1 => \"one\",\r\n",
            "        _ => \"other\",\r\n",
            "    };\r\n",
            "}",
        );
        assert_eq!(fmt(input), expected);
    }

    // --- return statements ----------------------------------------------

    #[test]
    fn bare_return_statement_is_formatted() {
        assert_eq!(fmt("return   ;"), "return;");
    }

    #[test]
    fn return_statement_with_expression_is_formatted() {
        assert_eq!(fmt("return   x+1;"), "return x + 1;");
    }

    #[test]
    fn return_statement_with_call_is_formatted() {
        assert_eq!(
            fmt("return someFunction(a,b);"),
            "return someFunction(a, b);"
        );
    }

    #[test]
    fn malformed_return_statement_is_left_alone() {
        let line = "return x + ;";
        assert_eq!(fmt(line), line);
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
    fn if_blocks_are_reformatted_and_reindented() {
        let input = "void Foo()\r\n{\r\nif (x>0)\r\n{\r\n}\r\n}";
        let expected = "void Foo()\r\n{\r\n    if (x > 0)\r\n    {\r\n    }\r\n}";
        assert_eq!(fmt(input), expected);
    }
}
