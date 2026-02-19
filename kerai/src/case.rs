/// Convert a snake_case string to camelCase.
///
/// `created_at` → `createdAt`
/// `instance_id` → `instanceId`
/// `kind` → `kind` (single word unchanged)
/// `already_camelCase` segments are preserved.
pub fn to_camel(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut capitalize_next = false;

    for ch in s.chars() {
        if ch == '_' {
            capitalize_next = true;
        } else if capitalize_next {
            result.push(ch.to_ascii_uppercase());
            capitalize_next = false;
        } else {
            result.push(ch);
        }
    }

    result
}

/// Convert a camelCase string to snake_case.
///
/// `createdAt` → `created_at`
/// `instanceId` → `instance_id`
/// `kind` → `kind` (single word unchanged)
/// `HTMLParser` → `html_parser` (runs of uppercase treated as acronym)
pub fn to_snake(s: &str) -> String {
    let mut result = String::with_capacity(s.len() + 4);
    let chars: Vec<char> = s.chars().collect();

    for (i, &ch) in chars.iter().enumerate() {
        if ch.is_ascii_uppercase() {
            let prev_lower = i > 0 && chars[i - 1].is_ascii_lowercase();
            let next_lower = i + 1 < chars.len() && chars[i + 1].is_ascii_lowercase();
            // Insert underscore before uppercase if preceded by lowercase,
            // or if this starts a new word after an acronym run (e.g. the P in HTMLParser)
            if prev_lower || (i > 0 && chars[i - 1].is_ascii_uppercase() && next_lower) {
                result.push('_');
            }
            result.push(ch.to_ascii_lowercase());
        } else {
            result.push(ch);
        }
    }

    result
}

/// Normalize an identifier for case-insensitive comparison.
///
/// Strips underscores and lowercases everything, so `createdAt`, `created_at`,
/// `CreatedAt`, and `CREATED_AT` all produce `createdat`.
pub fn normalize(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    for ch in s.chars() {
        if ch != '_' {
            result.push(ch.to_ascii_lowercase());
        }
    }
    result
}

/// Case-insensitive identifier equality.
///
/// Returns true if two identifiers refer to the same thing regardless of
/// casing convention: `createdAt` == `created_at` == `CreatedAt`.
pub fn eq_insensitive(a: &str, b: &str) -> bool {
    normalize(a) == normalize(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snake_to_camel() {
        assert_eq!(to_camel("created_at"), "createdAt");
        assert_eq!(to_camel("instance_id"), "instanceId");
        assert_eq!(to_camel("kind"), "kind");
        assert_eq!(to_camel("node_id"), "nodeId");
        assert_eq!(to_camel("public_key_hex"), "publicKeyHex");
    }

    #[test]
    fn camel_to_snake() {
        assert_eq!(to_snake("createdAt"), "created_at");
        assert_eq!(to_snake("instanceId"), "instance_id");
        assert_eq!(to_snake("kind"), "kind");
        assert_eq!(to_snake("nodeId"), "node_id");
        assert_eq!(to_snake("publicKeyHex"), "public_key_hex");
    }

    #[test]
    fn acronym_handling() {
        assert_eq!(to_snake("HTMLParser"), "html_parser");
        assert_eq!(to_snake("parseJSON"), "parse_json");
        assert_eq!(to_snake("IOError"), "io_error");
    }

    #[test]
    fn roundtrip_snake_through_camel() {
        for s in ["created_at", "instance_id", "node_id", "public_key_hex"] {
            assert_eq!(to_snake(&to_camel(s)), s);
        }
    }

    #[test]
    fn single_word_unchanged() {
        assert_eq!(to_camel("kind"), "kind");
        assert_eq!(to_snake("kind"), "kind");
    }

    #[test]
    fn normalize_all_forms() {
        let forms = ["createdAt", "created_at", "CreatedAt", "CREATED_AT", "createdat"];
        let expected = "createdat";
        for form in forms {
            assert_eq!(normalize(form), expected, "failed for: {form}");
        }
    }

    #[test]
    fn eq_insensitive_matches() {
        assert!(eq_insensitive("createdAt", "created_at"));
        assert!(eq_insensitive("NodeId", "node_id"));
        assert!(eq_insensitive("nodeId", "NODEID"));
        assert!(!eq_insensitive("nodeId", "edgeId"));
    }

    #[test]
    fn empty_and_edge_cases() {
        assert_eq!(to_camel(""), "");
        assert_eq!(to_snake(""), "");
        assert_eq!(to_camel("_"), "");
        assert_eq!(to_camel("__double__"), "Double");
        assert_eq!(to_snake("A"), "a");
    }
}
