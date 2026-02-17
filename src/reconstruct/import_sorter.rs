/// Import sorting — canonical `use` statement ordering for reconstruction.
///
/// Groups imports into three tiers:
///   1. std / core / alloc
///   2. External crates
///   3. crate:: / self:: / super::
///
/// Within each group, imports are sorted alphabetically by their full path.
/// Duplicate paths are deduplicated.

/// Classify a use statement into a sort group.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ImportGroup {
    /// std, core, alloc
    Std = 0,
    /// External crates
    External = 1,
    /// crate::, self::, super::
    Internal = 2,
}

/// A use statement with its sort key and original source.
#[derive(Debug, Clone)]
pub struct ImportEntry {
    pub group: ImportGroup,
    /// The full path for sorting (e.g., "std::collections::HashMap")
    pub sort_key: String,
    /// The original source text to emit
    pub source: String,
    /// The original item ID (for tracking)
    pub id: String,
}

/// Classify a use statement's source text into an import group.
pub fn classify_import(source: &str) -> ImportGroup {
    let trimmed = source.trim();
    // Strip leading "pub " / "pub(crate) " etc.
    let path_part = strip_visibility(trimmed);
    // Strip "use " prefix
    let path = path_part
        .strip_prefix("use ")
        .unwrap_or(path_part)
        .trim();

    if path.starts_with("std::") || path.starts_with("std ;")
        || path.starts_with("core::") || path.starts_with("core ;")
        || path.starts_with("alloc::") || path.starts_with("alloc ;")
        || path == "std" || path == "core" || path == "alloc"
        // Handle spaced token output from quote: "std :: collections"
        || path.starts_with("std ::")
        || path.starts_with("core ::")
        || path.starts_with("alloc ::")
    {
        ImportGroup::Std
    } else if path.starts_with("crate::") || path.starts_with("crate ;")
        || path.starts_with("self::") || path.starts_with("self ;")
        || path.starts_with("super::") || path.starts_with("super ;")
        || path.starts_with("crate ::") || path.starts_with("self ::") || path.starts_with("super ::")
    {
        ImportGroup::Internal
    } else {
        ImportGroup::External
    }
}

/// Extract a sort key from a use statement source.
/// Normalizes whitespace around :: for consistent sorting.
pub fn sort_key(source: &str) -> String {
    let trimmed = source.trim();
    let path_part = strip_visibility(trimmed);
    let path = path_part
        .strip_prefix("use ")
        .unwrap_or(path_part)
        .trim();
    // Normalize "std :: collections :: HashMap ;" → "std::collections::HashMap"
    let normalized = path
        .replace(" :: ", "::")
        .replace(":: ", "::")
        .replace(" ::", "::")
        .trim_end_matches(';')
        .trim()
        .to_string();
    normalized.to_lowercase()
}

/// Strip visibility prefix from a use statement.
fn strip_visibility(s: &str) -> &str {
    if let Some(rest) = s.strip_prefix("pub(crate) ") {
        return rest;
    }
    if let Some(rest) = s.strip_prefix("pub(super) ") {
        return rest;
    }
    if let Some(rest) = s.strip_prefix("pub(self) ") {
        return rest;
    }
    // pub(in path) - find the closing paren
    if s.starts_with("pub(") {
        if let Some(paren_end) = s.find(") ") {
            return &s[paren_end + 2..];
        }
    }
    if let Some(rest) = s.strip_prefix("pub ") {
        return rest;
    }
    s
}

/// Sort import entries into canonical order: std → external → internal,
/// alphabetical within each group. Deduplicates by sort key.
pub fn sort_imports(entries: &mut Vec<ImportEntry>) {
    // Deduplicate by sort key (keep first occurrence)
    let mut seen = std::collections::HashSet::new();
    entries.retain(|e| seen.insert(e.sort_key.clone()));

    entries.sort_by(|a, b| {
        a.group.cmp(&b.group).then_with(|| a.sort_key.cmp(&b.sort_key))
    });
}

/// Sort nested items within braces of a use statement.
/// e.g., "use std::{io, fs, collections}" → "use std::{collections, fs, io}"
pub fn sort_nested_imports(source: &str) -> String {
    // Find the brace group
    let Some(open) = source.find('{') else {
        return source.to_string();
    };
    let Some(close) = source.rfind('}') else {
        return source.to_string();
    };
    if open >= close {
        return source.to_string();
    }

    let prefix = &source[..open + 1];
    let suffix = &source[close..];
    let inner = &source[open + 1..close];

    // Split by comma, trim, sort, rejoin
    let mut items: Vec<&str> = inner.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
    items.sort_unstable_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()));

    format!("{}{}{}", prefix, items.join(", "), suffix)
}

/// Format sorted imports into source lines with group separators.
pub fn format_sorted_imports(entries: &[ImportEntry]) -> Vec<String> {
    let mut lines = Vec::new();
    let mut last_group: Option<ImportGroup> = None;

    for entry in entries {
        if let Some(prev) = last_group {
            if prev != entry.group {
                lines.push(String::new()); // blank line between groups
            }
        }
        lines.push(sort_nested_imports(&entry.source));
        last_group = Some(entry.group);
    }

    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_std() {
        assert_eq!(classify_import("use std::collections::HashMap;"), ImportGroup::Std);
        assert_eq!(classify_import("use core::fmt;"), ImportGroup::Std);
        assert_eq!(classify_import("use alloc::vec::Vec;"), ImportGroup::Std);
    }

    #[test]
    fn test_classify_external() {
        assert_eq!(classify_import("use serde::Deserialize;"), ImportGroup::External);
        assert_eq!(classify_import("use uuid::Uuid;"), ImportGroup::External);
        assert_eq!(classify_import("pub use pgrx::prelude::*;"), ImportGroup::External);
    }

    #[test]
    fn test_classify_internal() {
        assert_eq!(classify_import("use crate::parser::kinds::Kind;"), ImportGroup::Internal);
        assert_eq!(classify_import("use super::formatter;"), ImportGroup::Internal);
        assert_eq!(classify_import("use self::helper;"), ImportGroup::Internal);
    }

    #[test]
    fn test_classify_spaced_tokens() {
        // quote! output has spaces around ::
        assert_eq!(classify_import("use std :: collections :: HashMap ;"), ImportGroup::Std);
        assert_eq!(classify_import("use crate :: parser :: kinds ;"), ImportGroup::Internal);
    }

    #[test]
    fn test_sort_key_normalization() {
        assert_eq!(sort_key("use std :: collections :: HashMap ;"), "std::collections::hashmap");
        assert_eq!(sort_key("use serde::Deserialize;"), "serde::deserialize");
    }

    #[test]
    fn test_sort_imports_groups() {
        let mut entries = vec![
            ImportEntry {
                group: classify_import("use crate::sql;"),
                sort_key: sort_key("use crate::sql;"),
                source: "use crate::sql;".into(),
                id: "3".into(),
            },
            ImportEntry {
                group: classify_import("use serde::Deserialize;"),
                sort_key: sort_key("use serde::Deserialize;"),
                source: "use serde::Deserialize;".into(),
                id: "2".into(),
            },
            ImportEntry {
                group: classify_import("use std::collections::HashMap;"),
                sort_key: sort_key("use std::collections::HashMap;"),
                source: "use std::collections::HashMap;".into(),
                id: "1".into(),
            },
        ];

        sort_imports(&mut entries);

        assert_eq!(entries[0].source, "use std::collections::HashMap;");
        assert_eq!(entries[1].source, "use serde::Deserialize;");
        assert_eq!(entries[2].source, "use crate::sql;");
    }

    #[test]
    fn test_dedup() {
        let mut entries = vec![
            ImportEntry {
                group: ImportGroup::Std,
                sort_key: "std::io".into(),
                source: "use std::io;".into(),
                id: "1".into(),
            },
            ImportEntry {
                group: ImportGroup::Std,
                sort_key: "std::io".into(),
                source: "use std::io;".into(),
                id: "2".into(),
            },
        ];

        sort_imports(&mut entries);
        assert_eq!(entries.len(), 1);
    }

    #[test]
    fn test_sort_nested() {
        let result = sort_nested_imports("use std::{io, fs, collections};");
        assert_eq!(result, "use std::{collections, fs, io};");
    }

    #[test]
    fn test_format_with_separators() {
        let entries = vec![
            ImportEntry {
                group: ImportGroup::Std,
                sort_key: "std::io".into(),
                source: "use std::io;".into(),
                id: "1".into(),
            },
            ImportEntry {
                group: ImportGroup::External,
                sort_key: "serde::deserialize".into(),
                source: "use serde::Deserialize;".into(),
                id: "2".into(),
            },
            ImportEntry {
                group: ImportGroup::Internal,
                sort_key: "crate::sql".into(),
                source: "use crate::sql;".into(),
                id: "3".into(),
            },
        ];

        let lines = format_sorted_imports(&entries);
        assert_eq!(lines, vec![
            "use std::io;",
            "",
            "use serde::Deserialize;",
            "",
            "use crate::sql;",
        ]);
    }
}
