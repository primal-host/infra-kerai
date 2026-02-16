/// Assemble Rust source from stored AST nodes via SPI queries.
use pgrx::prelude::*;
use serde_json::Value;

/// Assemble source for a file node by querying its direct children.
/// Returns raw (unformatted) Rust source text.
pub fn assemble_file(file_node_id: &str) -> String {
    let mut parts: Vec<String> = Vec::new();

    // Collect inner doc comments (//! ...) first
    let inner_docs = query_inner_doc_comments(file_node_id);
    for doc in &inner_docs {
        parts.push(format!("//!{}", if doc.is_empty() { String::new() } else { format!(" {}", doc) }));
    }
    if !inner_docs.is_empty() {
        parts.push(String::new());
    }

    // Collect regular comments (non-doc, file-level)
    let comments = query_file_comments(file_node_id);

    // Collect item-level children ordered by position
    let items = query_child_items(file_node_id);

    for item in &items {
        // Prepend outer doc comments for this item
        let doc_comments = query_outer_doc_comments(&item.id);
        for doc in &doc_comments {
            parts.push(format!("///{}", if doc.is_empty() { String::new() } else { format!(" {}", doc) }));
        }

        // Prepend any regular comments that appear before this item's position
        for comment in &comments {
            if item.position > 0 && comment.line < item.span_start.unwrap_or(i32::MAX) {
                parts.push(format!("// {}", comment.text));
            }
        }

        if let Some(ref source) = item.source {
            parts.push(source.clone());
        } else if let Some(ref content) = item.content {
            // Fallback: use content directly (e.g. use statements)
            parts.push(content.clone());
        }
    }

    parts.join("\n")
}

/// A child item extracted from the database.
struct ChildItem {
    id: String,
    #[allow(dead_code)]
    kind: String,
    content: Option<String>,
    source: Option<String>,
    position: i32,
    span_start: Option<i32>,
}

/// A comment extracted from the database.
struct CommentInfo {
    text: String,
    line: i32,
}

fn query_child_items(file_node_id: &str) -> Vec<ChildItem> {
    let mut items = Vec::new();

    Spi::connect(|client| {
        let query = format!(
            "SELECT id::text, kind, content, metadata, position, \
             (metadata->>'source') AS source_text \
             FROM kerai.nodes \
             WHERE parent_id = '{}'::uuid \
             AND kind NOT IN ('doc_comment', 'comment', 'attribute') \
             ORDER BY position ASC",
            file_node_id.replace('\'', "''")
        );

        let result = client.select(&query, None, None).unwrap();

        for row in result {
            let id: String = row.get_by_name("id").unwrap().unwrap_or_default();
            let kind: String = row.get_by_name("kind").unwrap().unwrap_or_default();
            let content: Option<String> = row.get_by_name("content").unwrap();
            let source_text: Option<String> = row.get_by_name("source_text").unwrap();
            let position: i32 = row.get_by_name("position").unwrap().unwrap_or(0);

            // Extract source from metadata JSON if the direct column didn't work
            let source = source_text.or_else(|| {
                let meta_str: Option<String> = row
                    .get_by_name::<pgrx::JsonB>("metadata")
                    .ok()
                    .flatten()
                    .map(|j| {
                        if let Value::Object(m) = &j.0 {
                            m.get("source").and_then(|v| v.as_str()).map(|s| s.to_string())
                        } else {
                            None
                        }
                    })
                    .flatten();
                meta_str
            });

            // Query span_start from a subquery for ordering
            let span_start = {
                let span_query = format!(
                    "SELECT (metadata->>'span_start')::int AS ss FROM kerai.nodes WHERE id = '{}'::uuid",
                    id.replace('\'', "''")
                );
                client
                    .select(&span_query, None, None)
                    .ok()
                    .and_then(|mut r| r.next().and_then(|row| row.get_by_name::<i32>("ss").ok().flatten()))
            };

            items.push(ChildItem {
                id,
                kind,
                content,
                source,
                position,
                span_start,
            });
        }
    });

    items
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

        let result = client.select(&query, None, None).unwrap();
        for row in result {
            let content: String = row.get_by_name("content").unwrap().unwrap_or_default();
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

        let result = client.select(&query, None, None).unwrap();
        for row in result {
            let content: String = row.get_by_name("content").unwrap().unwrap_or_default();
            docs.push(content);
        }
    });

    docs
}

fn query_file_comments(file_node_id: &str) -> Vec<CommentInfo> {
    let mut comments = Vec::new();

    Spi::connect(|client| {
        let query = format!(
            "SELECT content, position FROM kerai.nodes \
             WHERE parent_id = '{}'::uuid \
             AND kind = 'comment' \
             ORDER BY position ASC",
            file_node_id.replace('\'', "''")
        );

        let result = client.select(&query, None, None).unwrap();
        for row in result {
            let text: String = row.get_by_name("content").unwrap().unwrap_or_default();
            let line: i32 = row.get_by_name("position").unwrap().unwrap_or(0);
            comments.push(CommentInfo { text, line });
        }
    });

    comments
}
