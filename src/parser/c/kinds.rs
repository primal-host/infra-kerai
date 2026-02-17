/// C AST node kind constants, prefixed with `c_` to avoid collisions
/// with Rust and Go kinds in the `kerai.nodes.kind` column.
// Preprocessor
pub const C_INCLUDE: &str = "c_include";
pub const C_DEFINE: &str = "c_define";
pub const C_MACRO: &str = "c_macro";
pub const C_IFDEF: &str = "c_ifdef";
pub const C_IF_DIRECTIVE: &str = "c_if_directive";
pub const C_PRAGMA: &str = "c_pragma";

// Declarations
pub const C_FUNCTION: &str = "c_function";
pub const C_DECLARATION: &str = "c_declaration";
pub const C_TYPEDEF: &str = "c_typedef";
pub const C_STRUCT: &str = "c_struct";
pub const C_UNION: &str = "c_union";
pub const C_ENUM: &str = "c_enum";
pub const C_FIELD: &str = "c_field";
pub const C_ENUMERATOR: &str = "c_enumerator";
pub const C_PARAM: &str = "c_param";
pub const C_INIT_DECLARATOR: &str = "c_init_declarator";

// Declarators
pub const C_POINTER_DECL: &str = "c_pointer_decl";
pub const C_ARRAY_DECL: &str = "c_array_decl";
pub const C_FUNC_DECL: &str = "c_func_decl";
pub const C_PAREN_DECL: &str = "c_paren_decl";

// Statements
pub const C_BLOCK: &str = "c_block";
pub const C_IF: &str = "c_if";
pub const C_FOR: &str = "c_for";
pub const C_WHILE: &str = "c_while";
pub const C_DO_WHILE: &str = "c_do_while";
pub const C_SWITCH: &str = "c_switch";
pub const C_CASE: &str = "c_case";
pub const C_RETURN: &str = "c_return";
pub const C_BREAK: &str = "c_break";
pub const C_CONTINUE: &str = "c_continue";
pub const C_GOTO: &str = "c_goto";
pub const C_LABEL: &str = "c_label";
pub const C_EXPR_STMT: &str = "c_expr_stmt";

// Expressions
pub const C_CALL: &str = "c_call";
pub const C_BINARY: &str = "c_binary";
pub const C_UNARY: &str = "c_unary";
pub const C_ASSIGNMENT: &str = "c_assignment";
pub const C_TERNARY: &str = "c_ternary";
pub const C_FIELD_ACCESS: &str = "c_field_access";
pub const C_SUBSCRIPT: &str = "c_subscript";
pub const C_CAST: &str = "c_cast";
pub const C_SIZEOF: &str = "c_sizeof";
pub const C_PAREN: &str = "c_paren";
pub const C_UPDATE: &str = "c_update";

// Type specifiers
pub const C_PRIMITIVE_TYPE: &str = "c_primitive_type";
pub const C_SIZED_TYPE: &str = "c_sized_type";
pub const C_TYPE_IDENT: &str = "c_type_ident";

// Literals
pub const C_NUMBER_LIT: &str = "c_number_lit";
pub const C_STRING_LIT: &str = "c_string_lit";
pub const C_CHAR_LIT: &str = "c_char_lit";
pub const C_TRUE: &str = "c_true";
pub const C_FALSE: &str = "c_false";
pub const C_NULL: &str = "c_null";

// Identifiers
pub const C_IDENT: &str = "c_ident";

// Catch-all
pub const C_OTHER: &str = "c_other";

/// Map a tree-sitter C node kind string to a kerai C kind constant.
pub fn ts_kind_to_c_kind(ts_kind: &str) -> &'static str {
    match ts_kind {
        // Preprocessor
        "preproc_include" => C_INCLUDE,
        "preproc_def" => C_DEFINE,
        "preproc_function_def" => C_MACRO,
        "preproc_ifdef" => C_IFDEF,
        "preproc_if" => C_IF_DIRECTIVE,
        "preproc_call" => C_PRAGMA,
        // Declarations
        "function_definition" => C_FUNCTION,
        "declaration" => C_DECLARATION,
        "type_definition" => C_TYPEDEF,
        "struct_specifier" => C_STRUCT,
        "union_specifier" => C_UNION,
        "enum_specifier" => C_ENUM,
        "field_declaration" => C_FIELD,
        "enumerator" => C_ENUMERATOR,
        "parameter_declaration" => C_PARAM,
        "init_declarator" => C_INIT_DECLARATOR,
        // Declarators
        "pointer_declarator" => C_POINTER_DECL,
        "array_declarator" => C_ARRAY_DECL,
        "function_declarator" => C_FUNC_DECL,
        "parenthesized_declarator" => C_PAREN_DECL,
        // Statements
        "compound_statement" => C_BLOCK,
        "if_statement" => C_IF,
        "for_statement" => C_FOR,
        "while_statement" => C_WHILE,
        "do_statement" => C_DO_WHILE,
        "switch_statement" => C_SWITCH,
        "case_statement" => C_CASE,
        "return_statement" => C_RETURN,
        "break_statement" => C_BREAK,
        "continue_statement" => C_CONTINUE,
        "goto_statement" => C_GOTO,
        "labeled_statement" => C_LABEL,
        "expression_statement" => C_EXPR_STMT,
        // Expressions
        "call_expression" => C_CALL,
        "binary_expression" => C_BINARY,
        "unary_expression" => C_UNARY,
        "assignment_expression" => C_ASSIGNMENT,
        "conditional_expression" => C_TERNARY,
        "field_expression" => C_FIELD_ACCESS,
        "subscript_expression" => C_SUBSCRIPT,
        "cast_expression" => C_CAST,
        "sizeof_expression" => C_SIZEOF,
        "parenthesized_expression" => C_PAREN,
        "update_expression" => C_UPDATE,
        // Type specifiers
        "primitive_type" => C_PRIMITIVE_TYPE,
        "sized_type_specifier" => C_SIZED_TYPE,
        "type_identifier" => C_TYPE_IDENT,
        // Literals
        "number_literal" => C_NUMBER_LIT,
        "string_literal" | "concatenated_string" => C_STRING_LIT,
        "char_literal" => C_CHAR_LIT,
        "true" => C_TRUE,
        "false" => C_FALSE,
        "null" => C_NULL,
        // Identifiers
        "identifier" | "field_identifier" => C_IDENT,
        // Catch-all
        _ => C_OTHER,
    }
}
