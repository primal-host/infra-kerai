/// LaTeX AST node kind constants, prefixed with `latex_` to avoid collisions
/// with other language kinds in the `kerai.nodes.kind` column.

// Document structure
pub const LATEX_DOCUMENT: &str = "latex_document";
pub const LATEX_PREAMBLE: &str = "latex_preamble";
pub const LATEX_DOCUMENTCLASS: &str = "latex_documentclass";
pub const LATEX_USEPACKAGE: &str = "latex_usepackage";

// Sectioning
pub const LATEX_PART: &str = "latex_part";
pub const LATEX_CHAPTER: &str = "latex_chapter";
pub const LATEX_SECTION: &str = "latex_section";
pub const LATEX_SUBSECTION: &str = "latex_subsection";
pub const LATEX_SUBSUBSECTION: &str = "latex_subsubsection";
pub const LATEX_PARAGRAPH: &str = "latex_paragraph";

// Environments
pub const LATEX_ENVIRONMENT: &str = "latex_environment";
pub const LATEX_MATH_ENV: &str = "latex_math_env";
pub const LATEX_FIGURE: &str = "latex_figure";
pub const LATEX_TABLE: &str = "latex_table";
pub const LATEX_THEOREM: &str = "latex_theorem";
pub const LATEX_DEFINITION: &str = "latex_definition";
pub const LATEX_PROOF: &str = "latex_proof";

// Math
pub const LATEX_INLINE_MATH: &str = "latex_inline_math";
pub const LATEX_DISPLAY_MATH: &str = "latex_display_math";

// References and cross-refs
pub const LATEX_CITATION: &str = "latex_citation";
pub const LATEX_LABEL: &str = "latex_label";
pub const LATEX_REF: &str = "latex_ref";
pub const LATEX_CAPTION: &str = "latex_caption";
pub const LATEX_FOOTNOTE: &str = "latex_footnote";

// File inclusion
pub const LATEX_INPUT: &str = "latex_input";
pub const LATEX_INCLUDE: &str = "latex_include";

// Generic command
pub const LATEX_COMMAND: &str = "latex_command";

// Text content
pub const LATEX_TEXT: &str = "latex_text";

// BibTeX
pub const BIB_ENTRY: &str = "bib_entry";
pub const BIB_FIELD: &str = "bib_field";

// Catch-all
pub const LATEX_OTHER: &str = "latex_other";

/// Semantic environments that get specialized kind constants.
const THEOREM_ENVS: &[&str] = &[
    "theorem", "lemma", "proposition", "corollary", "conjecture", "claim",
];
const DEFINITION_ENVS: &[&str] = &[
    "definition", "example", "remark", "notation", "assumption", "axiom",
];
const PROOF_ENVS: &[&str] = &["proof"];
const MATH_ENVS: &[&str] = &[
    "equation", "equation*", "align", "align*", "gather", "gather*",
    "multline", "multline*", "eqnarray", "eqnarray*", "displaymath",
    "math", "flalign", "flalign*", "alignat", "alignat*",
];

/// Map a LaTeX environment name to a kerai kind constant.
pub fn env_name_to_kind(name: &str) -> &'static str {
    if THEOREM_ENVS.contains(&name) {
        LATEX_THEOREM
    } else if DEFINITION_ENVS.contains(&name) {
        LATEX_DEFINITION
    } else if PROOF_ENVS.contains(&name) {
        LATEX_PROOF
    } else if MATH_ENVS.contains(&name) {
        LATEX_MATH_ENV
    } else if name == "figure" || name == "figure*" {
        LATEX_FIGURE
    } else if name == "table" || name == "table*" || name == "tabular" || name == "tabular*" {
        LATEX_TABLE
    } else {
        LATEX_ENVIRONMENT
    }
}

/// Map a tree-sitter LaTeX node kind to a kerai kind constant.
pub fn ts_kind_to_latex_kind(ts_kind: &str) -> &'static str {
    match ts_kind {
        "document" => LATEX_DOCUMENT,
        "inline_formula" | "text_mode" => LATEX_INLINE_MATH,
        "displayed_equation" | "math_environment" => LATEX_DISPLAY_MATH,
        _ => LATEX_OTHER,
    }
}

/// Sectioning command name to kerai kind.
pub fn section_cmd_to_kind(cmd: &str) -> Option<&'static str> {
    match cmd {
        "\\part" => Some(LATEX_PART),
        "\\chapter" => Some(LATEX_CHAPTER),
        "\\section" => Some(LATEX_SECTION),
        "\\subsection" => Some(LATEX_SUBSECTION),
        "\\subsubsection" => Some(LATEX_SUBSUBSECTION),
        "\\paragraph" => Some(LATEX_PARAGRAPH),
        _ => None,
    }
}

/// Return the nesting depth for a section command (lower = higher level).
pub fn section_depth(cmd: &str) -> Option<u8> {
    match cmd {
        "\\part" => Some(0),
        "\\chapter" => Some(1),
        "\\section" => Some(2),
        "\\subsection" => Some(3),
        "\\subsubsection" => Some(4),
        "\\paragraph" => Some(5),
        _ => None,
    }
}
