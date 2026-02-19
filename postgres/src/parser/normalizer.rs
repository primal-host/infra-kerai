/// Source text normalizer — pure function, no Postgres dependency.
///
/// Called at the top of parse_single_file() before syn::parse_file()
/// and extract_comments() see the source.

/// Normalize source text for consistent parsing.
///
/// Operations in order:
/// 1. Strip UTF-8 BOM if present
/// 2. Normalize CRLF → LF
/// 3. Strip trailing whitespace from every line
/// 4. Collapse 2+ consecutive blank lines → exactly 1 blank line
/// 5. Ensure file ends with exactly one `\n`
pub fn normalize(source: &str) -> String {
    // 1. Strip BOM
    let source = source.strip_prefix('\u{FEFF}').unwrap_or(source);

    // 2. CRLF → LF
    let source = source.replace("\r\n", "\n");

    // 3. Strip trailing whitespace from each line
    // 4. Collapse consecutive blank lines
    let mut result = String::with_capacity(source.len());
    let mut prev_blank = false;

    for line in source.split('\n') {
        let trimmed = line.trim_end();
        let is_blank = trimmed.is_empty();

        if is_blank && prev_blank {
            // Skip: collapsing consecutive blank lines
            continue;
        }

        if !result.is_empty() {
            result.push('\n');
        }
        result.push_str(trimmed);
        prev_blank = is_blank;
    }

    // 5. Ensure exactly one trailing newline
    let trimmed = result.trim_end_matches('\n');
    let mut out = trimmed.to_string();
    out.push('\n');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_bom() {
        let input = "\u{FEFF}fn main() {}";
        let result = normalize(input);
        assert_eq!(result, "fn main() {}\n");
    }

    #[test]
    fn test_crlf_to_lf() {
        let input = "fn main() {\r\n    let x = 1;\r\n}\r\n";
        let result = normalize(input);
        assert_eq!(result, "fn main() {\n    let x = 1;\n}\n");
        assert!(!result.contains('\r'));
    }

    #[test]
    fn test_trailing_whitespace() {
        let input = "fn main() {   \n    let x = 1;  \n}\n";
        let result = normalize(input);
        assert_eq!(result, "fn main() {\n    let x = 1;\n}\n");
    }

    #[test]
    fn test_collapse_blank_lines() {
        let input = "fn a() {}\n\n\n\n\nfn b() {}\n";
        let result = normalize(input);
        assert_eq!(result, "fn a() {}\n\nfn b() {}\n");
    }

    #[test]
    fn test_single_blank_line_preserved() {
        let input = "fn a() {}\n\nfn b() {}\n";
        let result = normalize(input);
        assert_eq!(result, "fn a() {}\n\nfn b() {}\n");
    }

    #[test]
    fn test_ensure_trailing_newline() {
        let input = "fn main() {}";
        let result = normalize(input);
        assert_eq!(result, "fn main() {}\n");
    }

    #[test]
    fn test_no_double_trailing_newline() {
        let input = "fn main() {}\n\n\n";
        let result = normalize(input);
        assert_eq!(result, "fn main() {}\n");
    }

    #[test]
    fn test_combined() {
        let input = "\u{FEFF}fn a() {}   \r\n\r\n\r\n\r\nfn b() {}  \r\n";
        let result = normalize(input);
        assert_eq!(result, "fn a() {}\n\nfn b() {}\n");
    }

    #[test]
    fn test_empty_input() {
        let result = normalize("");
        assert_eq!(result, "\n");
    }

    #[test]
    fn test_only_whitespace() {
        let result = normalize("   \n   \n   ");
        assert_eq!(result, "\n");
    }
}
