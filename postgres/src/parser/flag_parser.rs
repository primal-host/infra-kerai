/// Parse `// kerai:` directives from source comments.
///
/// Classifies kerai-prefixed comments into:
/// - Flags: `// kerai:skip-sort-imports`, `// kerai:skip`, etc.
/// - Suggestion acknowledgments: `// kerai: message text (rule_id)`

use serde_json::{json, Value};

/// A kerai directive found in source.
#[derive(Debug, Clone)]
pub enum KeraiDirective {
    /// A skip flag — store in file node metadata.
    Flag(String), // e.g., "skip-sort-imports", "skip"
    /// A suggestion comment that was left by a previous reconstruction.
    SuggestionComment {
        rule_id: String,
        message: String,
        line: usize,
    },
}

/// Parse all kerai directives from source text.
pub fn parse_kerai_directives(source: &str) -> Vec<KeraiDirective> {
    let mut directives = Vec::new();

    for (line_idx, line) in source.lines().enumerate() {
        let trimmed = line.trim();

        // Match "// kerai:..." comments
        let content = if let Some(rest) = trimmed.strip_prefix("// kerai:") {
            rest
        } else if let Some(rest) = trimmed.strip_prefix("//kerai:") {
            rest
        } else {
            continue;
        };

        let content = content.trim();

        // Check for flags (no space after colon, or known flag patterns)
        if is_flag(content) {
            directives.push(KeraiDirective::Flag(content.to_string()));
            continue;
        }

        // Check for suggestion comments: "message text (rule_id)"
        if let Some((message, rule_id)) = parse_suggestion_comment(content) {
            directives.push(KeraiDirective::SuggestionComment {
                rule_id,
                message,
                line: line_idx + 1, // 1-based line number
            });
        }
    }

    directives
}

/// Check if a kerai directive is a known flag.
fn is_flag(content: &str) -> bool {
    matches!(
        content,
        "skip" | "skip-sort-imports" | "skip-order-derives" | "skip-suggestions"
    )
}

/// Parse a suggestion comment: "message text (rule_id)" → (message, rule_id).
fn parse_suggestion_comment(content: &str) -> Option<(String, String)> {
    // Look for trailing "(rule_id)"
    let trimmed = content.trim();
    if let Some(paren_start) = trimmed.rfind('(') {
        if trimmed.ends_with(')') {
            let rule_id = trimmed[paren_start + 1..trimmed.len() - 1].trim();
            if !rule_id.is_empty() && rule_id.chars().all(|c| c.is_alphanumeric() || c == '_') {
                let message = trimmed[..paren_start].trim().to_string();
                return Some((message, rule_id.to_string()));
            }
        }
    }
    None
}

/// Build the kerai_flags JSON value from parsed flag directives.
pub fn build_flags_metadata(directives: &[KeraiDirective]) -> Option<Value> {
    let mut flags = serde_json::Map::new();

    for directive in directives {
        if let KeraiDirective::Flag(ref flag) = directive {
            flags.insert(flag.clone(), json!(true));
        }
    }

    if flags.is_empty() {
        None
    } else {
        Some(Value::Object(flags))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_skip_flag() {
        let source = "// kerai:skip\nfn foo() {}";
        let directives = parse_kerai_directives(source);
        assert_eq!(directives.len(), 1);
        match &directives[0] {
            KeraiDirective::Flag(f) => assert_eq!(f, "skip"),
            _ => panic!("expected flag"),
        }
    }

    #[test]
    fn test_parse_skip_sort_imports() {
        let source = "// kerai:skip-sort-imports\nuse std::io;";
        let directives = parse_kerai_directives(source);
        assert_eq!(directives.len(), 1);
        match &directives[0] {
            KeraiDirective::Flag(f) => assert_eq!(f, "skip-sort-imports"),
            _ => panic!("expected flag"),
        }
    }

    #[test]
    fn test_parse_suggestion_comment() {
        let source = "// kerai: consider &str instead of &String (prefer_str_slice)\nfn foo(s: &String) {}";
        let directives = parse_kerai_directives(source);
        assert_eq!(directives.len(), 1);
        match &directives[0] {
            KeraiDirective::SuggestionComment { rule_id, message, line } => {
                assert_eq!(rule_id, "prefer_str_slice");
                assert_eq!(message, "consider &str instead of &String");
                assert_eq!(*line, 1);
            }
            _ => panic!("expected suggestion comment"),
        }
    }

    #[test]
    fn test_no_kerai_comments() {
        let source = "// regular comment\nfn foo() {}";
        let directives = parse_kerai_directives(source);
        assert!(directives.is_empty());
    }

    #[test]
    fn test_build_flags_metadata() {
        let directives = vec![
            KeraiDirective::Flag("skip-sort-imports".to_string()),
            KeraiDirective::Flag("skip-order-derives".to_string()),
        ];
        let flags = build_flags_metadata(&directives);
        assert!(flags.is_some());
        let val = flags.unwrap();
        assert_eq!(val.get("skip-sort-imports").and_then(|v| v.as_bool()), Some(true));
        assert_eq!(val.get("skip-order-derives").and_then(|v| v.as_bool()), Some(true));
    }

    #[test]
    fn test_mixed_directives() {
        let source = "// kerai:skip-suggestions\n// kerai: function names should be snake_case (non_snake_fn)\nfn MyFunc() {}";
        let directives = parse_kerai_directives(source);
        assert_eq!(directives.len(), 2);
        assert!(matches!(&directives[0], KeraiDirective::Flag(f) if f == "skip-suggestions"));
        assert!(matches!(&directives[1], KeraiDirective::SuggestionComment { rule_id, .. } if rule_id == "non_snake_fn"));
    }
}
