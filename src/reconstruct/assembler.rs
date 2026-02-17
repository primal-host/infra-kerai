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

    // Collect item-level children ordered by position (including comments now)
    let items = query_child_items(file_node_id);

    for item in &items {
        // Handle comment/comment_block nodes directly
        if item.kind == "comment" || item.kind == "comment_block" {
            let placement = item.placement.as_deref().unwrap_or("above");
            if placement == "eof" || placement == "above" || placement == "between" {
                if let Some(ref content) = item.content {
                    let style = item.style.as_deref().unwrap_or("line");
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
            }
            // Trailing comments are handled inline with their target item
            continue;
        }

        // Query above comments for this item (via documents edges)
        let above_comments = query_comments_for_item(&item.id, "above");
        for comment in &above_comments {
            let style = comment.style.as_deref().unwrap_or("line");
            if style == "block" {
                parts.push(format!("/* {} */", comment.content));
            } else {
                for line in comment.content.split('\n') {
                    if line.is_empty() {
                        parts.push("//".to_string());
                    } else {
                        parts.push(format!("// {}", line));
                    }
                }
            }
        }

        // Between comments (emit before the item, with blank line separation)
        let between_comments = query_comments_for_item(&item.id, "between");
        for comment in &between_comments {
            let style = comment.style.as_deref().unwrap_or("line");
            if style == "block" {
                parts.push(format!("/* {} */", comment.content));
            } else {
                for line in comment.content.split('\n') {
                    if line.is_empty() {
                        parts.push("//".to_string());
                    } else {
                        parts.push(format!("// {}", line));
                    }
                }
            }
        }

        if let Some(ref source) = item.source {
            // source from ToTokens already includes doc attributes (#[doc = "..."])
            // which prettyplease converts back to /// comments — no need to prepend
            // Check for trailing comments on same line
            let trailing = query_comments_for_item(&item.id, "trailing");
            if !trailing.is_empty() {
                // Append trailing comment to the last line of source
                let trail_text = &trailing[0].content;
                let style = trailing[0].style.as_deref().unwrap_or("line");
                let suffix = if style == "block" {
                    format!(" /* {} */", trail_text)
                } else {
                    format!(" // {}", trail_text)
                };
                // Append to last line
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

fn query_comments_for_item(item_node_id: &str, placement: &str) -> Vec<CommentForItem> {
    let mut comments = Vec::new();

    Spi::connect(|client| {
        let query = format!(
            "SELECT n.content, n.metadata->>'style' AS style \
             FROM kerai.nodes n \
             JOIN kerai.edges e ON e.source_id = n.id \
             WHERE e.target_id = '{}'::uuid \
             AND e.relation = 'documents' \
             AND n.kind IN ('comment', 'comment_block') \
             AND COALESCE(n.metadata->>'placement', 'above') = '{}' \
             ORDER BY n.position ASC",
            item_node_id.replace('\'', "''"),
            placement.replace('\'', "''"),
        );

        let result = client.select(&query, None, &[]).unwrap();
        for row in result {
            let content: String = row.get_by_name::<String, _>("content")
                .unwrap()
                .unwrap_or_default();
            let style: Option<String> = row.get_by_name::<String, _>("style").unwrap();
            comments.push(CommentForItem { content, style });
        }
    });

    comments
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
