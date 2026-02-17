/// Node kind enum — type-safe representation of AST node kinds.

/// All possible node kinds stored in kerai.nodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    // Top-level
    Crate,
    Module,
    File,

    // Items
    Fn,
    Struct,
    Enum,
    Variant,
    Field,
    Impl,
    Trait,
    TypeAlias,
    Const,
    Static,
    Use,
    ExternCrate,
    ForeignMod,
    Union,
    TraitAlias,

    // Macros
    MacroDef,
    MacroCall,

    // Attributes and comments
    Attribute,
    DocComment,
    Comment,
    CommentBlock,

    // Blocks and statements
    Block,
    StmtLocal,
    StmtExpr,

    // Expressions
    ExprCall,
    ExprMethodCall,
    ExprIf,
    ExprMatch,
    ExprMatchArm,
    ExprClosure,
    ExprBlock,
    ExprLoop,
    ExprWhile,
    ExprFor,
    ExprReturn,
    ExprBreak,
    ExprContinue,
    ExprAssign,
    ExprBinary,
    ExprUnary,
    ExprField,
    ExprIndex,
    ExprReference,
    ExprStruct,
    ExprTuple,
    ExprArray,
    ExprCast,
    ExprPath,
    ExprRange,
    ExprLet,
    ExprAsync,
    ExprAwait,
    ExprTry,
    ExprYield,
    ExprUnsafe,
    ExprConst,
    ExprRepeat,
    ExprParen,
    ExprOther,

    // Patterns
    PatIdent,
    PatStruct,
    PatTupleStruct,
    PatTuple,
    PatOr,
    PatSlice,
    PatRest,
    PatWild,
    PatRange,
    PatRef,
    PatLit,
    PatPath,
    PatOther,

    // Leaves
    Ident,
    Lit,
    Lifetime,
    GenericParam,

    // Cargo
    CargoToml,
    Dependency,

    // Type nodes
    TypePath,
    TypeReference,
    TypeTuple,
    TypeArray,
    TypeSlice,
    TypeFn,
    TypeImplTrait,
    TypeDynTrait,
    TypeNever,
    TypeInfer,
    TypeOther,

    // Function signature parts
    Param,
    ReturnType,

    // Catch-all "other" variants for item types
    ItemOther,
    ImplItemOther,
    TraitItemOther,
}

impl Kind {
    /// The SQL/storage string value for this kind.
    pub fn as_str(&self) -> &'static str {
        match self {
            // Top-level
            Kind::Crate => "crate",
            Kind::Module => "module",
            Kind::File => "file",
            // Items
            Kind::Fn => "fn",
            Kind::Struct => "struct",
            Kind::Enum => "enum",
            Kind::Variant => "variant",
            Kind::Field => "field",
            Kind::Impl => "impl",
            Kind::Trait => "trait",
            Kind::TypeAlias => "type_alias",
            Kind::Const => "const",
            Kind::Static => "static",
            Kind::Use => "use",
            Kind::ExternCrate => "extern_crate",
            Kind::ForeignMod => "foreign_mod",
            Kind::Union => "union",
            Kind::TraitAlias => "trait_alias",
            // Macros
            Kind::MacroDef => "macro_def",
            Kind::MacroCall => "macro_call",
            // Attributes and comments
            Kind::Attribute => "attribute",
            Kind::DocComment => "doc_comment",
            Kind::Comment => "comment",
            Kind::CommentBlock => "comment_block",
            // Blocks and statements
            Kind::Block => "block",
            Kind::StmtLocal => "stmt_local",
            Kind::StmtExpr => "stmt_expr",
            // Expressions
            Kind::ExprCall => "expr_call",
            Kind::ExprMethodCall => "expr_method_call",
            Kind::ExprIf => "expr_if",
            Kind::ExprMatch => "expr_match",
            Kind::ExprMatchArm => "expr_match_arm",
            Kind::ExprClosure => "expr_closure",
            Kind::ExprBlock => "expr_block",
            Kind::ExprLoop => "expr_loop",
            Kind::ExprWhile => "expr_while",
            Kind::ExprFor => "expr_for",
            Kind::ExprReturn => "expr_return",
            Kind::ExprBreak => "expr_break",
            Kind::ExprContinue => "expr_continue",
            Kind::ExprAssign => "expr_assign",
            Kind::ExprBinary => "expr_binary",
            Kind::ExprUnary => "expr_unary",
            Kind::ExprField => "expr_field",
            Kind::ExprIndex => "expr_index",
            Kind::ExprReference => "expr_reference",
            Kind::ExprStruct => "expr_struct",
            Kind::ExprTuple => "expr_tuple",
            Kind::ExprArray => "expr_array",
            Kind::ExprCast => "expr_cast",
            Kind::ExprPath => "expr_path",
            Kind::ExprRange => "expr_range",
            Kind::ExprLet => "expr_let",
            Kind::ExprAsync => "expr_async",
            Kind::ExprAwait => "expr_await",
            Kind::ExprTry => "expr_try",
            Kind::ExprYield => "expr_yield",
            Kind::ExprUnsafe => "expr_unsafe",
            Kind::ExprConst => "expr_const",
            Kind::ExprRepeat => "expr_repeat",
            Kind::ExprParen => "expr_paren",
            Kind::ExprOther => "expr_other",
            // Patterns
            Kind::PatIdent => "pat_ident",
            Kind::PatStruct => "pat_struct",
            Kind::PatTupleStruct => "pat_tuple_struct",
            Kind::PatTuple => "pat_tuple",
            Kind::PatOr => "pat_or",
            Kind::PatSlice => "pat_slice",
            Kind::PatRest => "pat_rest",
            Kind::PatWild => "pat_wild",
            Kind::PatRange => "pat_range",
            Kind::PatRef => "pat_ref",
            Kind::PatLit => "pat_lit",
            Kind::PatPath => "pat_path",
            Kind::PatOther => "pat_other",
            // Leaves
            Kind::Ident => "ident",
            Kind::Lit => "lit",
            Kind::Lifetime => "lifetime",
            Kind::GenericParam => "generic_param",
            // Cargo
            Kind::CargoToml => "cargo_toml",
            Kind::Dependency => "dependency",
            // Types
            Kind::TypePath => "type_path",
            Kind::TypeReference => "type_reference",
            Kind::TypeTuple => "type_tuple",
            Kind::TypeArray => "type_array",
            Kind::TypeSlice => "type_slice",
            Kind::TypeFn => "type_fn",
            Kind::TypeImplTrait => "type_impl_trait",
            Kind::TypeDynTrait => "type_dyn_trait",
            Kind::TypeNever => "type_never",
            Kind::TypeInfer => "type_infer",
            Kind::TypeOther => "type_other",
            // Function signature
            Kind::Param => "param",
            Kind::ReturnType => "return_type",
            // Catch-all others
            Kind::ItemOther => "item_other",
            Kind::ImplItemOther => "impl_item_other",
            Kind::TraitItemOther => "trait_item_other",
        }
    }

    /// All Kind variants, for exhaustive iteration and testing.
    pub const ALL: &'static [Kind] = &[
        Kind::Crate, Kind::Module, Kind::File,
        Kind::Fn, Kind::Struct, Kind::Enum, Kind::Variant, Kind::Field,
        Kind::Impl, Kind::Trait, Kind::TypeAlias, Kind::Const, Kind::Static,
        Kind::Use, Kind::ExternCrate, Kind::ForeignMod, Kind::Union, Kind::TraitAlias,
        Kind::MacroDef, Kind::MacroCall,
        Kind::Attribute, Kind::DocComment, Kind::Comment, Kind::CommentBlock,
        Kind::Block, Kind::StmtLocal, Kind::StmtExpr,
        Kind::ExprCall, Kind::ExprMethodCall, Kind::ExprIf, Kind::ExprMatch,
        Kind::ExprMatchArm, Kind::ExprClosure, Kind::ExprBlock, Kind::ExprLoop,
        Kind::ExprWhile, Kind::ExprFor, Kind::ExprReturn, Kind::ExprBreak,
        Kind::ExprContinue, Kind::ExprAssign, Kind::ExprBinary, Kind::ExprUnary,
        Kind::ExprField, Kind::ExprIndex, Kind::ExprReference, Kind::ExprStruct,
        Kind::ExprTuple, Kind::ExprArray, Kind::ExprCast, Kind::ExprPath,
        Kind::ExprRange, Kind::ExprLet, Kind::ExprAsync, Kind::ExprAwait,
        Kind::ExprTry, Kind::ExprYield, Kind::ExprUnsafe, Kind::ExprConst,
        Kind::ExprRepeat, Kind::ExprParen, Kind::ExprOther,
        Kind::PatIdent, Kind::PatStruct, Kind::PatTupleStruct, Kind::PatTuple,
        Kind::PatOr, Kind::PatSlice, Kind::PatRest, Kind::PatWild, Kind::PatRange,
        Kind::PatRef, Kind::PatLit, Kind::PatPath, Kind::PatOther,
        Kind::Ident, Kind::Lit, Kind::Lifetime, Kind::GenericParam,
        Kind::CargoToml, Kind::Dependency,
        Kind::TypePath, Kind::TypeReference, Kind::TypeTuple, Kind::TypeArray,
        Kind::TypeSlice, Kind::TypeFn, Kind::TypeImplTrait, Kind::TypeDynTrait,
        Kind::TypeNever, Kind::TypeInfer, Kind::TypeOther,
        Kind::Param, Kind::ReturnType,
        Kind::ItemOther, Kind::ImplItemOther, Kind::TraitItemOther,
    ];
}

impl std::fmt::Display for Kind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for Kind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "crate" => Ok(Kind::Crate),
            "module" => Ok(Kind::Module),
            "file" => Ok(Kind::File),
            "fn" => Ok(Kind::Fn),
            "struct" => Ok(Kind::Struct),
            "enum" => Ok(Kind::Enum),
            "variant" => Ok(Kind::Variant),
            "field" => Ok(Kind::Field),
            "impl" => Ok(Kind::Impl),
            "trait" => Ok(Kind::Trait),
            "type_alias" => Ok(Kind::TypeAlias),
            "const" => Ok(Kind::Const),
            "static" => Ok(Kind::Static),
            "use" => Ok(Kind::Use),
            "extern_crate" => Ok(Kind::ExternCrate),
            "foreign_mod" => Ok(Kind::ForeignMod),
            "union" => Ok(Kind::Union),
            "trait_alias" => Ok(Kind::TraitAlias),
            "macro_def" => Ok(Kind::MacroDef),
            "macro_call" => Ok(Kind::MacroCall),
            "attribute" => Ok(Kind::Attribute),
            "doc_comment" => Ok(Kind::DocComment),
            "comment" => Ok(Kind::Comment),
            "comment_block" => Ok(Kind::CommentBlock),
            "block" => Ok(Kind::Block),
            "stmt_local" => Ok(Kind::StmtLocal),
            "stmt_expr" => Ok(Kind::StmtExpr),
            "expr_call" => Ok(Kind::ExprCall),
            "expr_method_call" => Ok(Kind::ExprMethodCall),
            "expr_if" => Ok(Kind::ExprIf),
            "expr_match" => Ok(Kind::ExprMatch),
            "expr_match_arm" => Ok(Kind::ExprMatchArm),
            "expr_closure" => Ok(Kind::ExprClosure),
            "expr_block" => Ok(Kind::ExprBlock),
            "expr_loop" => Ok(Kind::ExprLoop),
            "expr_while" => Ok(Kind::ExprWhile),
            "expr_for" => Ok(Kind::ExprFor),
            "expr_return" => Ok(Kind::ExprReturn),
            "expr_break" => Ok(Kind::ExprBreak),
            "expr_continue" => Ok(Kind::ExprContinue),
            "expr_assign" => Ok(Kind::ExprAssign),
            "expr_binary" => Ok(Kind::ExprBinary),
            "expr_unary" => Ok(Kind::ExprUnary),
            "expr_field" => Ok(Kind::ExprField),
            "expr_index" => Ok(Kind::ExprIndex),
            "expr_reference" => Ok(Kind::ExprReference),
            "expr_struct" => Ok(Kind::ExprStruct),
            "expr_tuple" => Ok(Kind::ExprTuple),
            "expr_array" => Ok(Kind::ExprArray),
            "expr_cast" => Ok(Kind::ExprCast),
            "expr_path" => Ok(Kind::ExprPath),
            "expr_range" => Ok(Kind::ExprRange),
            "expr_let" => Ok(Kind::ExprLet),
            "expr_async" => Ok(Kind::ExprAsync),
            "expr_await" => Ok(Kind::ExprAwait),
            "expr_try" => Ok(Kind::ExprTry),
            "expr_yield" => Ok(Kind::ExprYield),
            "expr_unsafe" => Ok(Kind::ExprUnsafe),
            "expr_const" => Ok(Kind::ExprConst),
            "expr_repeat" => Ok(Kind::ExprRepeat),
            "expr_paren" => Ok(Kind::ExprParen),
            "expr_other" => Ok(Kind::ExprOther),
            "pat_ident" => Ok(Kind::PatIdent),
            "pat_struct" => Ok(Kind::PatStruct),
            "pat_tuple_struct" => Ok(Kind::PatTupleStruct),
            "pat_tuple" => Ok(Kind::PatTuple),
            "pat_or" => Ok(Kind::PatOr),
            "pat_slice" => Ok(Kind::PatSlice),
            "pat_rest" => Ok(Kind::PatRest),
            "pat_wild" => Ok(Kind::PatWild),
            "pat_range" => Ok(Kind::PatRange),
            "pat_ref" => Ok(Kind::PatRef),
            "pat_lit" => Ok(Kind::PatLit),
            "pat_path" => Ok(Kind::PatPath),
            "pat_other" => Ok(Kind::PatOther),
            "ident" => Ok(Kind::Ident),
            "lit" => Ok(Kind::Lit),
            "lifetime" => Ok(Kind::Lifetime),
            "generic_param" => Ok(Kind::GenericParam),
            "cargo_toml" => Ok(Kind::CargoToml),
            "dependency" => Ok(Kind::Dependency),
            "type_path" => Ok(Kind::TypePath),
            "type_reference" => Ok(Kind::TypeReference),
            "type_tuple" => Ok(Kind::TypeTuple),
            "type_array" => Ok(Kind::TypeArray),
            "type_slice" => Ok(Kind::TypeSlice),
            "type_fn" => Ok(Kind::TypeFn),
            "type_impl_trait" => Ok(Kind::TypeImplTrait),
            "type_dyn_trait" => Ok(Kind::TypeDynTrait),
            "type_never" => Ok(Kind::TypeNever),
            "type_infer" => Ok(Kind::TypeInfer),
            "type_other" => Ok(Kind::TypeOther),
            "param" => Ok(Kind::Param),
            "return_type" => Ok(Kind::ReturnType),
            "item_other" => Ok(Kind::ItemOther),
            "impl_item_other" => Ok(Kind::ImplItemOther),
            "trait_item_other" => Ok(Kind::TraitItemOther),
            other => Err(format!("unknown kind: {}", other)),
        }
    }
}

// Deprecated &str constants — use Kind::Variant instead.
// Kept for transitional compatibility with string comparisons.
#[deprecated(note = "use Kind::Crate")]
pub const CRATE: &str = "crate";
#[deprecated(note = "use Kind::Module")]
pub const MODULE: &str = "module";
#[deprecated(note = "use Kind::File")]
pub const FILE: &str = "file";
#[deprecated(note = "use Kind::Fn")]
pub const FN: &str = "fn";
#[deprecated(note = "use Kind::Struct")]
pub const STRUCT: &str = "struct";
#[deprecated(note = "use Kind::Enum")]
pub const ENUM: &str = "enum";
#[deprecated(note = "use Kind::Variant")]
pub const VARIANT: &str = "variant";
#[deprecated(note = "use Kind::Field")]
pub const FIELD: &str = "field";
#[deprecated(note = "use Kind::Impl")]
pub const IMPL: &str = "impl";
#[deprecated(note = "use Kind::Trait")]
pub const TRAIT: &str = "trait";
#[deprecated(note = "use Kind::TypeAlias")]
pub const TYPE_ALIAS: &str = "type_alias";
#[deprecated(note = "use Kind::Const")]
pub const CONST: &str = "const";
#[deprecated(note = "use Kind::Static")]
pub const STATIC: &str = "static";
#[deprecated(note = "use Kind::Use")]
pub const USE: &str = "use";
#[deprecated(note = "use Kind::ExternCrate")]
pub const EXTERN_CRATE: &str = "extern_crate";
#[deprecated(note = "use Kind::ForeignMod")]
pub const FOREIGN_MOD: &str = "foreign_mod";
#[deprecated(note = "use Kind::Union")]
pub const UNION: &str = "union";
#[deprecated(note = "use Kind::TraitAlias")]
pub const TRAIT_ALIAS: &str = "trait_alias";
#[deprecated(note = "use Kind::MacroDef")]
pub const MACRO_DEF: &str = "macro_def";
#[deprecated(note = "use Kind::MacroCall")]
pub const MACRO_CALL: &str = "macro_call";
#[deprecated(note = "use Kind::Attribute")]
pub const ATTRIBUTE: &str = "attribute";
#[deprecated(note = "use Kind::DocComment")]
pub const DOC_COMMENT: &str = "doc_comment";
#[deprecated(note = "use Kind::Comment")]
pub const COMMENT: &str = "comment";
#[deprecated(note = "use Kind::CommentBlock")]
pub const COMMENT_BLOCK: &str = "comment_block";
#[deprecated(note = "use Kind::Block")]
pub const BLOCK: &str = "block";
#[deprecated(note = "use Kind::StmtLocal")]
pub const STMT_LOCAL: &str = "stmt_local";
#[deprecated(note = "use Kind::StmtExpr")]
pub const STMT_EXPR: &str = "stmt_expr";
#[deprecated(note = "use Kind::ExprCall")]
pub const EXPR_CALL: &str = "expr_call";
#[deprecated(note = "use Kind::ExprMethodCall")]
pub const EXPR_METHOD_CALL: &str = "expr_method_call";
#[deprecated(note = "use Kind::ExprIf")]
pub const EXPR_IF: &str = "expr_if";
#[deprecated(note = "use Kind::ExprMatch")]
pub const EXPR_MATCH: &str = "expr_match";
#[deprecated(note = "use Kind::ExprMatchArm")]
pub const EXPR_MATCH_ARM: &str = "expr_match_arm";
#[deprecated(note = "use Kind::ExprClosure")]
pub const EXPR_CLOSURE: &str = "expr_closure";
#[deprecated(note = "use Kind::ExprBlock")]
pub const EXPR_BLOCK: &str = "expr_block";
#[deprecated(note = "use Kind::ExprLoop")]
pub const EXPR_LOOP: &str = "expr_loop";
#[deprecated(note = "use Kind::ExprWhile")]
pub const EXPR_WHILE: &str = "expr_while";
#[deprecated(note = "use Kind::ExprFor")]
pub const EXPR_FOR: &str = "expr_for";
#[deprecated(note = "use Kind::ExprReturn")]
pub const EXPR_RETURN: &str = "expr_return";
#[deprecated(note = "use Kind::ExprBreak")]
pub const EXPR_BREAK: &str = "expr_break";
#[deprecated(note = "use Kind::ExprContinue")]
pub const EXPR_CONTINUE: &str = "expr_continue";
#[deprecated(note = "use Kind::ExprAssign")]
pub const EXPR_ASSIGN: &str = "expr_assign";
#[deprecated(note = "use Kind::ExprBinary")]
pub const EXPR_BINARY: &str = "expr_binary";
#[deprecated(note = "use Kind::ExprUnary")]
pub const EXPR_UNARY: &str = "expr_unary";
#[deprecated(note = "use Kind::ExprField")]
pub const EXPR_FIELD: &str = "expr_field";
#[deprecated(note = "use Kind::ExprIndex")]
pub const EXPR_INDEX: &str = "expr_index";
#[deprecated(note = "use Kind::ExprReference")]
pub const EXPR_REFERENCE: &str = "expr_reference";
#[deprecated(note = "use Kind::ExprStruct")]
pub const EXPR_STRUCT: &str = "expr_struct";
#[deprecated(note = "use Kind::ExprTuple")]
pub const EXPR_TUPLE: &str = "expr_tuple";
#[deprecated(note = "use Kind::ExprArray")]
pub const EXPR_ARRAY: &str = "expr_array";
#[deprecated(note = "use Kind::ExprCast")]
pub const EXPR_CAST: &str = "expr_cast";
#[deprecated(note = "use Kind::ExprPath")]
pub const EXPR_PATH: &str = "expr_path";
#[deprecated(note = "use Kind::ExprRange")]
pub const EXPR_RANGE: &str = "expr_range";
#[deprecated(note = "use Kind::ExprLet")]
pub const EXPR_LET: &str = "expr_let";
#[deprecated(note = "use Kind::ExprAsync")]
pub const EXPR_ASYNC: &str = "expr_async";
#[deprecated(note = "use Kind::ExprAwait")]
pub const EXPR_AWAIT: &str = "expr_await";
#[deprecated(note = "use Kind::ExprTry")]
pub const EXPR_TRY: &str = "expr_try";
#[deprecated(note = "use Kind::ExprYield")]
pub const EXPR_YIELD: &str = "expr_yield";
#[deprecated(note = "use Kind::ExprUnsafe")]
pub const EXPR_UNSAFE: &str = "expr_unsafe";
#[deprecated(note = "use Kind::ExprConst")]
pub const EXPR_CONST: &str = "expr_const";
#[deprecated(note = "use Kind::ExprRepeat")]
pub const EXPR_REPEAT: &str = "expr_repeat";
#[deprecated(note = "use Kind::ExprParen")]
pub const EXPR_PAREN: &str = "expr_paren";
#[deprecated(note = "use Kind::ExprOther")]
pub const EXPR_OTHER: &str = "expr_other";
#[deprecated(note = "use Kind::PatIdent")]
pub const PAT_IDENT: &str = "pat_ident";
#[deprecated(note = "use Kind::PatStruct")]
pub const PAT_STRUCT: &str = "pat_struct";
#[deprecated(note = "use Kind::PatTupleStruct")]
pub const PAT_TUPLE_STRUCT: &str = "pat_tuple_struct";
#[deprecated(note = "use Kind::PatTuple")]
pub const PAT_TUPLE: &str = "pat_tuple";
#[deprecated(note = "use Kind::PatOr")]
pub const PAT_OR: &str = "pat_or";
#[deprecated(note = "use Kind::PatSlice")]
pub const PAT_SLICE: &str = "pat_slice";
#[deprecated(note = "use Kind::PatRest")]
pub const PAT_REST: &str = "pat_rest";
#[deprecated(note = "use Kind::PatWild")]
pub const PAT_WILD: &str = "pat_wild";
#[deprecated(note = "use Kind::PatRange")]
pub const PAT_RANGE: &str = "pat_range";
#[deprecated(note = "use Kind::PatRef")]
pub const PAT_REF: &str = "pat_ref";
#[deprecated(note = "use Kind::PatLit")]
pub const PAT_LIT: &str = "pat_lit";
#[deprecated(note = "use Kind::PatPath")]
pub const PAT_PATH: &str = "pat_path";
#[deprecated(note = "use Kind::PatOther")]
pub const PAT_OTHER: &str = "pat_other";
#[deprecated(note = "use Kind::Ident")]
pub const IDENT: &str = "ident";
#[deprecated(note = "use Kind::Lit")]
pub const LIT: &str = "lit";
#[deprecated(note = "use Kind::Lifetime")]
pub const LIFETIME: &str = "lifetime";
#[deprecated(note = "use Kind::GenericParam")]
pub const GENERIC_PARAM: &str = "generic_param";
#[deprecated(note = "use Kind::CargoToml")]
pub const CARGO_TOML: &str = "cargo_toml";
#[deprecated(note = "use Kind::Dependency")]
pub const DEPENDENCY: &str = "dependency";
#[deprecated(note = "use Kind::TypePath")]
pub const TYPE_PATH: &str = "type_path";
#[deprecated(note = "use Kind::TypeReference")]
pub const TYPE_REFERENCE: &str = "type_reference";
#[deprecated(note = "use Kind::TypeTuple")]
pub const TYPE_TUPLE: &str = "type_tuple";
#[deprecated(note = "use Kind::TypeArray")]
pub const TYPE_ARRAY: &str = "type_array";
#[deprecated(note = "use Kind::TypeSlice")]
pub const TYPE_SLICE: &str = "type_slice";
#[deprecated(note = "use Kind::TypeFn")]
pub const TYPE_FN: &str = "type_fn";
#[deprecated(note = "use Kind::TypeImplTrait")]
pub const TYPE_IMPL_TRAIT: &str = "type_impl_trait";
#[deprecated(note = "use Kind::TypeDynTrait")]
pub const TYPE_DYN_TRAIT: &str = "type_dyn_trait";
#[deprecated(note = "use Kind::TypeNever")]
pub const TYPE_NEVER: &str = "type_never";
#[deprecated(note = "use Kind::TypeInfer")]
pub const TYPE_INFER: &str = "type_infer";
#[deprecated(note = "use Kind::TypeOther")]
pub const TYPE_OTHER: &str = "type_other";
#[deprecated(note = "use Kind::Param")]
pub const PARAM: &str = "param";
#[deprecated(note = "use Kind::ReturnType")]
pub const RETURN_TYPE: &str = "return_type";
