/// Builds ltree-safe paths from AST context.
///
/// ltree labels: alphanumeric + underscore, max 255 chars.
/// We lowercase and replace non-alphanumeric chars with underscores.

/// Sanitize an identifier for use as an ltree label segment.
pub fn sanitize_label(ident: &str) -> String {
    if ident.is_empty() {
        return "_".to_string();
    }
    let sanitized: String = ident
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '_' { c } else { '_' })
        .collect();
    // ltree labels can't start with a digit
    if sanitized.starts_with(|c: char| c.is_ascii_digit()) {
        format!("_{}", sanitized)
    } else {
        sanitized
    }
}

/// Context stack for building ltree paths.
pub struct PathContext {
    segments: Vec<String>,
}

impl PathContext {
    pub fn new() -> Self {
        Self {
            segments: Vec::new(),
        }
    }

    pub fn with_root(root: &str) -> Self {
        Self {
            segments: vec![sanitize_label(root)],
        }
    }

    pub fn push(&mut self, segment: &str) {
        self.segments.push(sanitize_label(segment));
    }

    pub fn pop(&mut self) {
        self.segments.pop();
    }

    /// Build the current ltree path as a dot-separated string.
    pub fn path(&self) -> Option<String> {
        if self.segments.is_empty() {
            None
        } else {
            Some(self.segments.join("."))
        }
    }

    /// Build a child path without modifying the context.
    pub fn child_path(&self, segment: &str) -> String {
        let mut segments = self.segments.clone();
        segments.push(sanitize_label(segment));
        segments.join(".")
    }
}
