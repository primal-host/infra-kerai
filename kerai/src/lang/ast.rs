use std::fmt;

use super::expr::Expr;

/// Notation mode for interpreting function-call lines.
///
/// Controls which token in `a b c` is the function vs arguments:
/// - Prefix:  `a(b, c)` — first token is function
/// - Infix:   `b(a, c)` — second token is function
/// - Postfix: `c(a, b)` — last token is function
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Notation {
    Prefix,
    Infix,
    Postfix,
}

impl Default for Notation {
    fn default() -> Self {
        Notation::Prefix
    }
}

impl fmt::Display for Notation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Notation::Prefix => write!(f, "prefix"),
            Notation::Infix => write!(f, "infix"),
            Notation::Postfix => write!(f, "postfix"),
        }
    }
}

/// A single parsed line from a `.kerai` file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Line {
    /// Blank line (preserved for round-tripping).
    Empty,

    /// Comment line (`# ...` or `// ...`).
    Comment { text: String },

    /// Definition: `:name target` — alias or function binding.
    /// Records the active notation mode as function metadata.
    Definition {
        name: String,
        target: String,
        notation: Notation,
    },

    /// Type annotation: `name: type` (reserved for future use).
    TypeAnnotation { name: String, type_expr: String },

    /// Function call: `name arg1 arg2` — interpretation depends on notation mode.
    /// Args are `Expr` trees — flat lines produce `Expr::Atom` leaves,
    /// parenthesized expressions produce nested `Expr::Apply` trees.
    Call {
        function: String,
        args: Vec<Expr>,
        notation: Notation,
    },

    /// Parser directive: `kerai.*` lines that have side effects on parser state.
    Directive { name: String, args: Vec<String> },
}

/// A parsed `.kerai` document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Document {
    pub lines: Vec<Line>,
    pub default_notation: Notation,
}

impl Document {
    pub fn new() -> Self {
        Document {
            lines: Vec::new(),
            default_notation: Notation::Prefix,
        }
    }
}

impl Default for Document {
    fn default() -> Self {
        Self::new()
    }
}
