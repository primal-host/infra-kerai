/// Assemble Rust source from stored AST nodes via SPI queries.
use pgrx::prelude::*;

use crate::parser::kinds::Kind;
use super::derive_orderer;
use super::import_sorter::{self, ImportEntry};

/// Options controlling reconstruction intelligence features.
#[derive(Debug, Clone)]
pub struct AssemblyOptions {
    pub sort_imports: bool,
    pub order_derives: bool,
    pub suggestions: bool,
}

impl Default for AssemblyOptions {
    fn default() -> Self {
        Self {
            sort_imports: true,
            order_derives: true,
            suggestions: true,
        }
    }
}

/// Assemble source for a file node by querying its direct children.
/// Returns raw (unformatted) Rust source text.
pub fn assemble_file(file_node_id: &str) -> String {
    assemble_file_with_options(file_node_id, &AssemblyOptions::default())
}

/// Assemble source with explicit options.
pub fn assemble_file_with_options(file_node_id: &str, options: &AssemblyOptions) -> String {
    // Check for kerai flags stored on the file node
    let flags = query_file_flags(file_node_id);
    let sort_imports = options.sort_imports && !flags.skip_sort_imports && !flags.skip_all;
    let order_derives = options.order_derives && !flags.skip_order_derives && !flags.skip_all;
    let emit_suggestions = options.suggestions && !flags.skip_suggestions && !flags.skip_all;

    let mut parts: Vec<String> = Vec::new();

    // Collect inner doc comments (//! ...) first
    let inner_docs = query_inner_doc_comments(file_node_id);
    for doc in &inner_docs {
        if doc.is_empty() {
            parts.push("//!".to_string());
        } else {
            parts.push(format!("//! {}", doc));
        }
    }
    if !inner_docs.is_empty() {
        parts.push(String::new());
    }

    // Collect suggestions keyed by target node ID
    let suggestion_map = if emit_suggestions {
        query_suggestions(file_node_id)
    } else {
        std::collections::HashMap::new()
    };

    // Collect all direct children ordered by position
    let items = query_child_items(file_node_id);

    // Collect IDs of comment nodes that appear as direct children
    let comment_str = Kind::Comment.as_str();
    let comment_block_str = Kind::CommentBlock.as_str();
    let use_str = Kind::Use.as_str();

    let direct_comment_ids: std::collections::HashSet<String> = items
        .iter()
        .filter(|i| i.kind == comment_str || i.kind == comment_block_str)
        .map(|i| i.id.clone())
        .collect();

    if sort_imports {
        // Partition items into: use items, comments-above-use items, and everything else
        emit_sorted_imports(&items, use_str, comment_str, comment_block_str, &mut parts);

        // Emit remaining non-use items
        for item in &items {
            if item.kind == use_str {
                continue; // already emitted
            }
            if is_comment_kind(&item.kind, comment_str, comment_block_str) {
                if item.consumed_by_import_sort {
                    continue;
                }
                let placement = item.placement.as_deref().unwrap_or("above");
                if placement == "trailing" {
                    continue;
                }
                if let Some(ref content) = item.content {
                    emit_comment(&mut parts, content, item.style.as_deref().unwrap_or("line"));
                }
                continue;
            }

            // Emit suggestions above this item
            emit_suggestions_for_item(&mut parts, &item.id, &suggestion_map);
            emit_item(&mut parts, item, order_derives, &direct_comment_ids);
        }
    } else {
        // No import sorting — emit everything in position order
        for item in &items {
            if is_comment_kind(&item.kind, comment_str, comment_block_str) {
                let placement = item.placement.as_deref().unwrap_or("above");
                if placement == "trailing" {
                    continue;
                }
                if let Some(ref content) = item.content {
                    emit_comment(&mut parts, content, item.style.as_deref().unwrap_or("line"));
                }
                continue;
            }

            // Emit suggestions above this item
            emit_suggestions_for_item(&mut parts, &item.id, &suggestion_map);
            emit_item(&mut parts, item, order_derives, &direct_comment_ids);
        }
    }

    parts.join("\n")
}

/// A suggestion to emit as a // kerai: comment.
struct SuggestionForEmit {
    message: String,
    rule_id: String,
}

/// Query active suggestions for a file, grouped by target node ID.
fn query_suggestions(
    file_node_id: &str,
) -> std::collections::HashMap<String, Vec<SuggestionForEmit>> {
    let mut map: std::collections::HashMap<String, Vec<SuggestionForEmit>> =
        std::collections::HashMap::new();

    Spi::connect(|client| {
        let query = format!(
            "SELECT n.content, n.metadata->>'rule' AS rule, \
             e.target_id::text AS target_id \
             FROM kerai.nodes n \
             JOIN kerai.edges e ON e.source_id = n.id \
             WHERE n.parent_id = '{}'::uuid \
             AND n.kind = 'suggestion' \
             AND n.metadata->>'status' = 'emitted' \
             AND e.relation = 'suggests' \
             ORDER BY n.position ASC",
            file_node_id.replace('\'', "''")
        );

        let result = client.select(&query, None, &[]).unwrap();
        for row in result {
            let message: String = row
                .get_by_name::<String, _>("content")
                .unwrap()
                .unwrap_or_default();
            let rule: String = row
                .get_by_name::<String, _>("rule")
                .unwrap()
                .unwrap_or_default();
            let target: String = row
                .get_by_name::<String, _>("target_id")
                .unwrap()
                .unwrap_or_default();

            map.entry(target)
                .or_default()
                .push(SuggestionForEmit {
                    message,
                    rule_id: rule,
                });
        }
    });

    map
}

/// Emit `// kerai:` suggestion comments for a target item.
fn emit_suggestions_for_item(
    parts: &mut Vec<String>,
    item_id: &str,
    suggestion_map: &std::collections::HashMap<String, Vec<SuggestionForEmit>>,
) {
    if let Some(suggestions) = suggestion_map.get(item_id) {
        for suggestion in suggestions {
            parts.push(format!(
                "// kerai: {} ({})",
                suggestion.message, suggestion.rule_id
            ));
        }
    }
}

/// Emit sorted imports and any comments that were directly above use items.
fn emit_sorted_imports(
    items: &[ChildItem],
    use_str: &str,
    comment_str: &str,
    comment_block_str: &str,
    parts: &mut Vec<String>,
) {
    // Build import entries from use items
    let mut import_entries: Vec<ImportEntry> = Vec::new();

    for item in items {
        if item.kind != use_str {
            continue;
        }
        let source = item.source.as_deref()
            .or(item.content.as_deref())
            .unwrap_or("");
        if source.is_empty() {
            continue;
        }

        let group = import_sorter::classify_import(source);
        let key = import_sorter::sort_key(source);

        import_entries.push(ImportEntry {
            group,
            sort_key: key,
            source: source.to_string(),
            id: item.id.clone(),
        });
    }

    if import_entries.is_empty() {
        return;
    }

    import_sorter::sort_imports(&mut import_entries);
    let import_lines = import_sorter::format_sorted_imports(&import_entries);

    for line in &import_lines {
        parts.push(line.clone());
    }

    // Add blank line after imports before other items
    if !import_lines.is_empty() {
        parts.push(String::new());
    }

}

fn is_comment_kind(kind: &str, comment_str: &str, comment_block_str: &str) -> bool {
    kind == comment_str || kind == comment_block_str
}

/// Emit a single non-comment, non-use item.
fn emit_item(
    parts: &mut Vec<String>,
    item: &ChildItem,
    order_derives: bool,
    direct_comment_ids: &std::collections::HashSet<String>,
) {
    if let Some(ref source) = item.source {
        let processed = if order_derives {
            derive_orderer::order_derives(source)
        } else {
            source.clone()
        };

        // Check for trailing comments
        let trailing = query_trailing_comments(&item.id, direct_comment_ids);
        if let Some(ref trail) = trailing {
            let suffix = if trail.style.as_deref() == Some("block") {
                format!(" /* {} */", trail.content)
            } else {
                format!(" // {}", trail.content)
            };
            let mut lines: Vec<&str> = processed.lines().collect();
            if let Some(last) = lines.last_mut() {
                let combined = format!("{}{}", last, suffix);
                let prev_lines = &lines[..lines.len() - 1];
                let mut combined_source = prev_lines.join("\n");
                if !combined_source.is_empty() {
                    combined_source.push('\n');
                }
                combined_source.push_str(&combined);
                parts.push(combined_source);
            } else {
                parts.push(processed);
            }
        } else {
            parts.push(processed);
        }
    } else {
        // No source metadata — prepend doc comments manually
        let doc_comments = query_outer_doc_comments(&item.id);
        for doc in &doc_comments {
            if doc.is_empty() {
                parts.push("///".to_string());
            } else {
                parts.push(format!("/// {}", doc));
            }
        }

        if let Some(ref content) = item.content {
            parts.push(content.clone());
        }
    }
}

/// Emit a comment (line or block style) into the parts list.
fn emit_comment(parts: &mut Vec<String>, content: &str, style: &str) {
    if style == "block" {
        parts.push(format!("/* {} */", content));
    } else {
        for line in content.split('\n') {
            if line.is_empty() {
                parts.push("//".to_string());
            } else {
                parts.push(format!("// {}", line));
            }
        }
    }
}

/// Flags parsed from file node metadata (set during parsing from // kerai: comments).
struct FileFlags {
    skip_sort_imports: bool,
    skip_order_derives: bool,
    skip_suggestions: bool,
    skip_all: bool,
}

fn query_file_flags(file_node_id: &str) -> FileFlags {
    let mut flags = FileFlags {
        skip_sort_imports: false,
        skip_order_derives: false,
        skip_suggestions: false,
        skip_all: false,
    };

    Spi::connect(|client| {
        let query = format!(
            "SELECT metadata->'kerai_flags' AS flags \
             FROM kerai.nodes WHERE id = '{}'::uuid",
            file_node_id.replace('\'', "''")
        );

        let result = client.select(&query, None, &[]).unwrap();
        for row in result {
            if let Some(flags_json) = row.get_by_name::<pgrx::JsonB, _>("flags").unwrap() {
                let v = &flags_json.0;
                if v.get("skip-sort-imports").and_then(|v| v.as_bool()) == Some(true) {
                    flags.skip_sort_imports = true;
                }
                if v.get("skip-order-derives").and_then(|v| v.as_bool()) == Some(true) {
                    flags.skip_order_derives = true;
                }
                if v.get("skip-suggestions").and_then(|v| v.as_bool()) == Some(true) {
                    flags.skip_suggestions = true;
                }
                if v.get("skip").and_then(|v| v.as_bool()) == Some(true) {
                    flags.skip_all = true;
                }
            }
        }
    });

    flags
}

struct ChildItem {
    id: String,
    kind: String,
    content: Option<String>,
    source: Option<String>,
    placement: Option<String>,
    style: Option<String>,
    /// Set to true when this comment was above a use item and was consumed by import sorting.
    consumed_by_import_sort: bool,
}

fn query_child_items(file_node_id: &str) -> Vec<ChildItem> {
    let mut items = Vec::new();

    Spi::connect(|client| {
        // Order by position (line number for both items and comments)
        let query = format!(
            "SELECT id::text, kind, content, \
             metadata->>'source' AS source_text, \
             metadata->>'placement' AS placement, \
             metadata->>'style' AS style \
             FROM kerai.nodes \
             WHERE parent_id = '{}'::uuid \
             AND kind NOT IN ('doc_comment', 'attribute', 'suggestion') \
             ORDER BY position ASC",
            file_node_id.replace('\'', "''")
        );

        let result = client.select(&query, None, &[]).unwrap();

        for row in result {
            let id: String = row.get_by_name::<String, _>("id")
                .unwrap()
                .unwrap_or_default();
            let kind: String = row.get_by_name::<String, _>("kind")
                .unwrap()
                .unwrap_or_default();
            let content: Option<String> = row.get_by_name::<String, _>("content").unwrap();
            let source: Option<String> = row.get_by_name::<String, _>("source_text").unwrap();
            let placement: Option<String> = row.get_by_name::<String, _>("placement").unwrap();
            let style: Option<String> = row.get_by_name::<String, _>("style").unwrap();

            items.push(ChildItem {
                id, kind, content, source, placement, style,
                consumed_by_import_sort: false,
            });
        }
    });

    // Mark comments that are directly above use items as consumed_by_import_sort.
    // When imports are sorted, these comments lose their positional meaning.
    let use_str = Kind::Use.as_str();
    let comment_str = Kind::Comment.as_str();
    let comment_block_str = Kind::CommentBlock.as_str();

    for i in 0..items.len() {
        if items[i].kind == use_str {
            // Look backwards for adjacent comments (above placement)
            let mut j = i;
            while j > 0 {
                j -= 1;
                if is_comment_kind(&items[j].kind, comment_str, comment_block_str) {
                    let placement = items[j].placement.as_deref().unwrap_or("above");
                    if placement == "above" {
                        items[j].consumed_by_import_sort = true;
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            }
        }
    }

    items
}

struct CommentForItem {
    content: String,
    style: Option<String>,
}

/// Query trailing comments for an item via documents edges.
/// Only returns comments that are also direct children (in the given set).
fn query_trailing_comments(
    item_node_id: &str,
    direct_ids: &std::collections::HashSet<String>,
) -> Option<CommentForItem> {
    let mut comments = Vec::new();

    Spi::connect(|client| {
        let query = format!(
            "SELECT n.id::text, n.content, n.metadata->>'style' AS style \
             FROM kerai.nodes n \
             JOIN kerai.edges e ON e.source_id = n.id \
             WHERE e.target_id = '{}'::uuid \
             AND e.relation = 'documents' \
             AND n.kind IN ('comment', 'comment_block') \
             AND COALESCE(n.metadata->>'placement', 'above') = 'trailing' \
             ORDER BY n.position ASC",
            item_node_id.replace('\'', "''"),
        );

        let result = client.select(&query, None, &[]).unwrap();
        for row in result {
            let id: String = row.get_by_name::<String, _>("id")
                .unwrap()
                .unwrap_or_default();
            let content: String = row.get_by_name::<String, _>("content")
                .unwrap()
                .unwrap_or_default();
            let style: Option<String> = row.get_by_name::<String, _>("style").unwrap();
            if direct_ids.contains(&id) {
                comments.push(CommentForItem { content, style });
            }
        }
    });

    comments.into_iter().next()
}

fn query_inner_doc_comments(file_node_id: &str) -> Vec<String> {
    let mut docs = Vec::new();

    Spi::connect(|client| {
        let query = format!(
            "SELECT content FROM kerai.nodes \
             WHERE parent_id = '{}'::uuid \
             AND kind = 'doc_comment' \
             AND (metadata->>'inner')::boolean = true \
             ORDER BY position ASC",
            file_node_id.replace('\'', "''")
        );

        let result = client.select(&query, None, &[]).unwrap();
        for row in result {
            let content: String = row.get_by_name::<String, _>("content")
                .unwrap()
                .unwrap_or_default();
            docs.push(content);
        }
    });

    docs
}

fn query_outer_doc_comments(item_node_id: &str) -> Vec<String> {
    let mut docs = Vec::new();

    Spi::connect(|client| {
        let query = format!(
            "SELECT n.content FROM kerai.nodes n \
             JOIN kerai.edges e ON e.source_id = n.id \
             WHERE e.target_id = '{}'::uuid \
             AND e.relation = 'documents' \
             AND n.kind = 'doc_comment' \
             AND COALESCE((n.metadata->>'inner')::boolean, false) = false \
             ORDER BY n.position ASC",
            item_node_id.replace('\'', "''")
        );

        let result = client.select(&query, None, &[]).unwrap();
        for row in result {
            let content: String = row.get_by_name::<String, _>("content")
                .unwrap()
                .unwrap_or_default();
            docs.push(content);
        }
    });

    docs
}
