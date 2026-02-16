/// Format Rust source through prettyplease for canonical output.

/// Parse and reformat Rust source. Falls back to raw input on parse failure.
pub fn format_source(raw: &str) -> String {
    match syn::parse_file(raw) {
        Ok(parsed) => prettyplease::unparse(&parsed),
        Err(_) => raw.to_string(),
    }
}
