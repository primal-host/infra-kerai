/// Node kind constants mapping syn types to database kind strings.

// Top-level
pub const CRATE: &str = "crate";
pub const MODULE: &str = "module";
pub const FILE: &str = "file";

// Items
pub const FN: &str = "fn";
pub const STRUCT: &str = "struct";
pub const ENUM: &str = "enum";
pub const VARIANT: &str = "variant";
pub const FIELD: &str = "field";
pub const IMPL: &str = "impl";
pub const TRAIT: &str = "trait";
pub const TYPE_ALIAS: &str = "type_alias";
pub const CONST: &str = "const";
pub const STATIC: &str = "static";
pub const USE: &str = "use";
pub const EXTERN_CRATE: &str = "extern_crate";
pub const FOREIGN_MOD: &str = "foreign_mod";
pub const UNION: &str = "union";
pub const TRAIT_ALIAS: &str = "trait_alias";

// Macros
pub const MACRO_DEF: &str = "macro_def";
pub const MACRO_CALL: &str = "macro_call";

// Attributes and comments
pub const ATTRIBUTE: &str = "attribute";
pub const DOC_COMMENT: &str = "doc_comment";
pub const COMMENT: &str = "comment";

// Blocks and statements
pub const BLOCK: &str = "block";
pub const STMT_LOCAL: &str = "stmt_local";
pub const STMT_EXPR: &str = "stmt_expr";

// Expressions
pub const EXPR_CALL: &str = "expr_call";
pub const EXPR_METHOD_CALL: &str = "expr_method_call";
pub const EXPR_IF: &str = "expr_if";
pub const EXPR_MATCH: &str = "expr_match";
pub const EXPR_MATCH_ARM: &str = "expr_match_arm";
pub const EXPR_CLOSURE: &str = "expr_closure";
pub const EXPR_BLOCK: &str = "expr_block";
pub const EXPR_LOOP: &str = "expr_loop";
pub const EXPR_WHILE: &str = "expr_while";
pub const EXPR_FOR: &str = "expr_for";
pub const EXPR_RETURN: &str = "expr_return";
pub const EXPR_BREAK: &str = "expr_break";
pub const EXPR_CONTINUE: &str = "expr_continue";
pub const EXPR_ASSIGN: &str = "expr_assign";
pub const EXPR_BINARY: &str = "expr_binary";
pub const EXPR_UNARY: &str = "expr_unary";
pub const EXPR_FIELD: &str = "expr_field";
pub const EXPR_INDEX: &str = "expr_index";
pub const EXPR_REFERENCE: &str = "expr_reference";
pub const EXPR_STRUCT: &str = "expr_struct";
pub const EXPR_TUPLE: &str = "expr_tuple";
pub const EXPR_ARRAY: &str = "expr_array";
pub const EXPR_CAST: &str = "expr_cast";
pub const EXPR_PATH: &str = "expr_path";
pub const EXPR_RANGE: &str = "expr_range";
pub const EXPR_LET: &str = "expr_let";
pub const EXPR_ASYNC: &str = "expr_async";
pub const EXPR_AWAIT: &str = "expr_await";
pub const EXPR_TRY: &str = "expr_try";
pub const EXPR_YIELD: &str = "expr_yield";
pub const EXPR_UNSAFE: &str = "expr_unsafe";
pub const EXPR_CONST: &str = "expr_const";
pub const EXPR_REPEAT: &str = "expr_repeat";
pub const EXPR_PAREN: &str = "expr_paren";
pub const EXPR_OTHER: &str = "expr_other";

// Patterns
pub const PAT_IDENT: &str = "pat_ident";
pub const PAT_STRUCT: &str = "pat_struct";
pub const PAT_TUPLE_STRUCT: &str = "pat_tuple_struct";
pub const PAT_TUPLE: &str = "pat_tuple";
pub const PAT_OR: &str = "pat_or";
pub const PAT_SLICE: &str = "pat_slice";
pub const PAT_REST: &str = "pat_rest";
pub const PAT_WILD: &str = "pat_wild";
pub const PAT_RANGE: &str = "pat_range";
pub const PAT_REF: &str = "pat_ref";
pub const PAT_LIT: &str = "pat_lit";
pub const PAT_PATH: &str = "pat_path";
pub const PAT_OTHER: &str = "pat_other";

// Leaves
pub const IDENT: &str = "ident";
pub const LIT: &str = "lit";
pub const LIFETIME: &str = "lifetime";
pub const GENERIC_PARAM: &str = "generic_param";

// Cargo
pub const CARGO_TOML: &str = "cargo_toml";
pub const DEPENDENCY: &str = "dependency";

// Type nodes
pub const TYPE_PATH: &str = "type_path";
pub const TYPE_REFERENCE: &str = "type_reference";
pub const TYPE_TUPLE: &str = "type_tuple";
pub const TYPE_ARRAY: &str = "type_array";
pub const TYPE_SLICE: &str = "type_slice";
pub const TYPE_FN: &str = "type_fn";
pub const TYPE_IMPL_TRAIT: &str = "type_impl_trait";
pub const TYPE_DYN_TRAIT: &str = "type_dyn_trait";
pub const TYPE_NEVER: &str = "type_never";
pub const TYPE_INFER: &str = "type_infer";
pub const TYPE_OTHER: &str = "type_other";

// Function signature parts
pub const PARAM: &str = "param";
pub const RETURN_TYPE: &str = "return_type";
