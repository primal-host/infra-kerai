/// Shared tree-sitter parser infrastructure for multi-language support.

pub mod cursor;

/// Supported tree-sitter languages.
pub enum TsLanguage {
    Go,
    C,
    Latex,
}

impl TsLanguage {
    pub fn ts_language(&self) -> tree_sitter::Language {
        match self {
            TsLanguage::Go => tree_sitter_go::LANGUAGE.into(),
            TsLanguage::C => tree_sitter_c::LANGUAGE.into(),
            TsLanguage::Latex => tree_sitter_latex::language().into(),
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            TsLanguage::Go => "go",
            TsLanguage::C => "c",
            TsLanguage::Latex => "latex",
        }
    }
}

/// Parse source text with the given tree-sitter language grammar.
pub fn parse(source: &str, lang: TsLanguage) -> Option<tree_sitter::Tree> {
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&lang.ts_language()).ok()?;
    parser.parse(source, None)
}
