/// Derive ordering — alphabetical normalization of `#[derive(...)]` attributes.
///
/// Sorts the trait list within each `#[derive(...)]` attribute alphabetically.
/// Multiple `#[derive(...)]` on the same item are sorted independently (not merged).

/// Sort derive traits within a `#[derive(...)]` attribute in a source string.
/// Processes all `#[derive(...)]` occurrences in the input.
pub fn order_derives(source: &str) -> String {
    let mut result = String::with_capacity(source.len());
    let mut remaining = source;

    while let Some(derive_start) = find_derive_start(remaining) {
        // Everything before this derive
        result.push_str(&remaining[..derive_start]);

        let after_prefix = &remaining[derive_start..];

        // Find the opening paren
        let Some(paren_open) = after_prefix.find('(') else {
            result.push_str(after_prefix);
            remaining = "";
            break;
        };

        // Find matching closing paren (handle nested parens for things like derive(Foo(Bar)))
        let Some(paren_close) = find_matching_paren(after_prefix, paren_open) else {
            result.push_str(after_prefix);
            remaining = "";
            break;
        };

        let prefix = &after_prefix[..paren_open + 1]; // "#[derive("
        let inner = &after_prefix[paren_open + 1..paren_close];
        let suffix_start = paren_close; // ")]..."

        // Sort the traits
        let mut traits: Vec<&str> = inner
            .split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect();
        traits.sort_unstable_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()));

        result.push_str(prefix);
        result.push_str(&traits.join(", "));
        result.push_str(&after_prefix[suffix_start..suffix_start + 2]); // ")]"

        remaining = &after_prefix[suffix_start + 2..];
    }

    result.push_str(remaining);
    result
}

/// Find the start position of a `#[derive(` pattern.
fn find_derive_start(s: &str) -> Option<usize> {
    let mut search_from = 0;
    while search_from < s.len() {
        let Some(pos) = s[search_from..].find("#[derive(") else {
            return None;
        };
        return Some(search_from + pos);
    }
    None
}

/// Find the matching closing paren for an opening paren, handling nesting.
fn find_matching_paren(s: &str, open_pos: usize) -> Option<usize> {
    let mut depth = 0;
    for (i, ch) in s[open_pos..].char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(open_pos + i);
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sort_derives_basic() {
        let input = "#[derive(Serialize, Clone, Debug)]";
        let expected = "#[derive(Clone, Debug, Serialize)]";
        assert_eq!(order_derives(input), expected);
    }

    #[test]
    fn test_sort_derives_already_sorted() {
        let input = "#[derive(Clone, Debug, Eq, Hash, PartialEq)]";
        assert_eq!(order_derives(input), input);
    }

    #[test]
    fn test_sort_derives_single() {
        let input = "#[derive(Debug)]";
        assert_eq!(order_derives(input), input);
    }

    #[test]
    fn test_sort_derives_case_insensitive() {
        let input = "#[derive(serde::Serialize, Clone)]";
        let expected = "#[derive(Clone, serde::Serialize)]";
        assert_eq!(order_derives(input), expected);
    }

    #[test]
    fn test_sort_derives_in_full_source() {
        let input = "#[derive(Serialize, Clone, Debug)]\nstruct Foo {\n    x: i32,\n}";
        let expected = "#[derive(Clone, Debug, Serialize)]\nstruct Foo {\n    x: i32,\n}";
        assert_eq!(order_derives(input), expected);
    }

    #[test]
    fn test_sort_derives_multiple() {
        let input = "#[derive(Serialize, Clone)]\n#[derive(Hash, Eq)]\nstruct Foo;";
        let expected = "#[derive(Clone, Serialize)]\n#[derive(Eq, Hash)]\nstruct Foo;";
        assert_eq!(order_derives(input), expected);
    }

    #[test]
    fn test_no_derive() {
        let input = "struct Foo { x: i32 }";
        assert_eq!(order_derives(input), input);
    }

    #[test]
    fn test_empty_derive() {
        let input = "#[derive()]\nstruct Foo;";
        assert_eq!(order_derives(input), input);
    }
}
