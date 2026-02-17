/// Go AST node kind constants, prefixed with `go_` to avoid collisions
/// with Rust kinds in the `kerai.nodes.kind` column.

// Top-level
pub const GO_PACKAGE: &str = "go_package";
pub const GO_IMPORT: &str = "go_import";
pub const GO_IMPORT_SPEC: &str = "go_import_spec";

// Declarations
pub const GO_FUNC: &str = "go_func";
pub const GO_METHOD: &str = "go_method";
pub const GO_TYPE_DECL: &str = "go_type_decl";
pub const GO_TYPE_SPEC: &str = "go_type_spec";
pub const GO_STRUCT: &str = "go_struct";
pub const GO_INTERFACE: &str = "go_interface";
pub const GO_FIELD: &str = "go_field";
pub const GO_METHOD_SPEC: &str = "go_method_spec";
pub const GO_VAR_DECL: &str = "go_var_decl";
pub const GO_VAR_SPEC: &str = "go_var_spec";
pub const GO_CONST_DECL: &str = "go_const_decl";
pub const GO_CONST_SPEC: &str = "go_const_spec";

// Statements
pub const GO_BLOCK: &str = "go_block";
pub const GO_IF: &str = "go_if";
pub const GO_FOR: &str = "go_for";
pub const GO_SWITCH: &str = "go_switch";
pub const GO_TYPE_SWITCH: &str = "go_type_switch";
pub const GO_SELECT: &str = "go_select";
pub const GO_RETURN: &str = "go_return";
pub const GO_GO: &str = "go_go";
pub const GO_DEFER: &str = "go_defer";
pub const GO_SHORT_VAR: &str = "go_short_var";
pub const GO_ASSIGNMENT: &str = "go_assignment";
pub const GO_EXPRESSION_STMT: &str = "go_expression_stmt";
pub const GO_SEND_STMT: &str = "go_send_stmt";
pub const GO_INC_STMT: &str = "go_inc_stmt";
pub const GO_DEC_STMT: &str = "go_dec_stmt";
pub const GO_LABELED_STMT: &str = "go_labeled_stmt";
pub const GO_FALLTHROUGH: &str = "go_fallthrough";
pub const GO_BREAK: &str = "go_break";
pub const GO_CONTINUE: &str = "go_continue";
pub const GO_GOTO: &str = "go_goto";
pub const GO_RANGE: &str = "go_range";

// Expressions
pub const GO_CALL: &str = "go_call";
pub const GO_SELECTOR: &str = "go_selector";
pub const GO_COMPOSITE_LIT: &str = "go_composite_lit";
pub const GO_FUNC_LIT: &str = "go_func_lit";
pub const GO_INDEX: &str = "go_index";
pub const GO_SLICE: &str = "go_slice";
pub const GO_TYPE_ASSERTION: &str = "go_type_assertion";
pub const GO_UNARY: &str = "go_unary";
pub const GO_BINARY: &str = "go_binary";
pub const GO_PAREN: &str = "go_paren";

// Type expressions
pub const GO_POINTER_TYPE: &str = "go_pointer_type";
pub const GO_ARRAY_TYPE: &str = "go_array_type";
pub const GO_SLICE_TYPE: &str = "go_slice_type";
pub const GO_MAP_TYPE: &str = "go_map_type";
pub const GO_CHANNEL_TYPE: &str = "go_channel_type";
pub const GO_FUNC_TYPE: &str = "go_func_type";
pub const GO_QUALIFIED_TYPE: &str = "go_qualified_type";

// Literals
pub const GO_INT_LIT: &str = "go_int_lit";
pub const GO_FLOAT_LIT: &str = "go_float_lit";
pub const GO_STRING_LIT: &str = "go_string_lit";
pub const GO_RUNE_LIT: &str = "go_rune_lit";
pub const GO_TRUE: &str = "go_true";
pub const GO_FALSE: &str = "go_false";
pub const GO_NIL: &str = "go_nil";
pub const GO_IOTA: &str = "go_iota";

// Switch/select clauses
pub const GO_CASE: &str = "go_case";
pub const GO_DEFAULT_CASE: &str = "go_default_case";
pub const GO_COMM_CLAUSE: &str = "go_comm_clause";

// Identifiers
pub const GO_IDENT: &str = "go_ident";

// Catch-all
pub const GO_OTHER: &str = "go_other";

/// Map a tree-sitter node kind string to a kerai Go kind constant.
pub fn ts_kind_to_go_kind(ts_kind: &str) -> &'static str {
    match ts_kind {
        "package_clause" => GO_PACKAGE,
        "import_declaration" => GO_IMPORT,
        "import_spec" => GO_IMPORT_SPEC,
        "function_declaration" => GO_FUNC,
        "method_declaration" => GO_METHOD,
        "type_declaration" => GO_TYPE_DECL,
        "type_spec" => GO_TYPE_SPEC,
        "struct_type" => GO_STRUCT,
        "interface_type" => GO_INTERFACE,
        "field_declaration" => GO_FIELD,
        "method_spec" => GO_METHOD_SPEC,
        "var_declaration" => GO_VAR_DECL,
        "var_spec" => GO_VAR_SPEC,
        "const_declaration" => GO_CONST_DECL,
        "const_spec" => GO_CONST_SPEC,
        "block" => GO_BLOCK,
        "if_statement" => GO_IF,
        "for_statement" => GO_FOR,
        "expression_switch_statement" => GO_SWITCH,
        "type_switch_statement" => GO_TYPE_SWITCH,
        "select_statement" => GO_SELECT,
        "return_statement" => GO_RETURN,
        "go_statement" => GO_GO,
        "defer_statement" => GO_DEFER,
        "short_var_declaration" => GO_SHORT_VAR,
        "assignment_statement" => GO_ASSIGNMENT,
        "expression_statement" => GO_EXPRESSION_STMT,
        "send_statement" => GO_SEND_STMT,
        "inc_statement" => GO_INC_STMT,
        "dec_statement" => GO_DEC_STMT,
        "labeled_statement" => GO_LABELED_STMT,
        "fallthrough_statement" => GO_FALLTHROUGH,
        "break_statement" => GO_BREAK,
        "continue_statement" => GO_CONTINUE,
        "goto_statement" => GO_GOTO,
        "range_clause" => GO_RANGE,
        "call_expression" => GO_CALL,
        "selector_expression" => GO_SELECTOR,
        "composite_literal" => GO_COMPOSITE_LIT,
        "func_literal" => GO_FUNC_LIT,
        "index_expression" => GO_INDEX,
        "slice_expression" => GO_SLICE,
        "type_assertion_expression" => GO_TYPE_ASSERTION,
        "unary_expression" => GO_UNARY,
        "binary_expression" => GO_BINARY,
        "parenthesized_expression" => GO_PAREN,
        "pointer_type" => GO_POINTER_TYPE,
        "array_type" => GO_ARRAY_TYPE,
        "slice_type" => GO_SLICE_TYPE,
        "map_type" => GO_MAP_TYPE,
        "channel_type" => GO_CHANNEL_TYPE,
        "function_type" => GO_FUNC_TYPE,
        "qualified_type" => GO_QUALIFIED_TYPE,
        "int_literal" => GO_INT_LIT,
        "float_literal" => GO_FLOAT_LIT,
        "interpreted_string_literal" | "raw_string_literal" => GO_STRING_LIT,
        "rune_literal" => GO_RUNE_LIT,
        "true" => GO_TRUE,
        "false" => GO_FALSE,
        "nil" => GO_NIL,
        "iota" => GO_IOTA,
        "expression_case" | "type_case" => GO_CASE,
        "default_case" => GO_DEFAULT_CASE,
        "communication_case" => GO_COMM_CLAUSE,
        "identifier" | "field_identifier" | "type_identifier" | "package_identifier" => GO_IDENT,
        _ => GO_OTHER,
    }
}
