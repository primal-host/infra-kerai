/// Assemble Rust source from stored AST nodes via SPI queries.
use pgrx::prelude::*;

/// Assemble source for a file node by querying its direct children.
/// Returns raw (unformatted) Rust source text.
pub fn assemble_file(file_node_id: &str) -> String {
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

    // Collect all direct children ordered by span_start (interleaves items and comments)
    let items = query_child_items(file_node_id);

    // Collect IDs of comment nodes that appear as direct children,
    // so we can skip them when queried via edges (avoid duplication)
    let direct_comment_ids: std::collections::HashSet<String> = items
        .iter()
        .filter(|i| i.kind == "comment" || i.kind == "comment_block")
        .map(|i| i.id.clone())
        .collect();

    for item in &items {
        // Handle comment/comment_block nodes — emit directly in position
        if item.kind == "comment" || item.kind == "comment_block" {
            let placement = item.placement.as_deref().unwrap_or("above");
            // Skip trailing comments here — they'll be appended to their target item
            if placement == "trailing" {
                continue;
            }
            if let Some(ref content) = item.content {
                emit_comment(&mut parts, content, item.style.as_deref().unwrap_or("line"));
            }
            continue;
        }

        if let Some(ref source) = item.source {
            // source from ToTokens already includes doc attributes (#[doc = "..."])
            // which prettyplease converts back to /// comments
            // Check for trailing comments on same line
            let trailing = query_trailing_comments(&item.id, &direct_comment_ids);
            if let Some(ref trail) = trailing {
                let suffix = if trail.style.as_deref() == Some("block") {
                    format!(" /* {} */", trail.content)
                } else {
                    format!(" // {}", trail.content)
                };
                let mut lines: Vec<&str> = source.lines().collect();
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
                    parts.push(source.clone());
                }
            } else {
                parts.push(source.clone());
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

    parts.join("\n")
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

struct ChildItem {
    id: String,
    kind: String,
    content: Option<String>,
    source: Option<String>,
    placement: Option<String>,
    style: Option<String>,
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
             AND kind NOT IN ('doc_comment', 'attribute') \
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

            items.push(ChildItem { id, kind, content, source, placement, style });
        }
    });

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
