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

    // Collect item-level children ordered by position
    let items = query_child_items(file_node_id);

    for item in &items {
        if let Some(ref source) = item.source {
            // source from ToTokens already includes doc attributes (#[doc = "..."])
            // which prettyplease converts back to /// comments — no need to prepend
            parts.push(source.clone());
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
    content: Option<String>,
    source: Option<String>,
}

fn query_child_items(file_node_id: &str) -> Vec<ChildItem> {
    let mut items = Vec::new();

    Spi::connect(|client| {
        let query = format!(
            "SELECT id::text, content, metadata->>'source' AS source_text \
             FROM kerai.nodes \
             WHERE parent_id = '{}'::uuid \
             AND kind NOT IN ('doc_comment', 'comment', 'attribute') \
             ORDER BY position ASC",
            file_node_id.replace('\'', "''")
        );

        let result = client.select(&query, None, &[]).unwrap();

        for row in result {
            let id: String = row.get_by_name::<String, _>("id")
                .unwrap()
                .unwrap_or_default();
            let content: Option<String> = row.get_by_name::<String, _>("content").unwrap();
            let source: Option<String> = row.get_by_name::<String, _>("source_text").unwrap();

            items.push(ChildItem { id, content, source });
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
