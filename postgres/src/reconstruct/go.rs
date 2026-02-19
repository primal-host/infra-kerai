/// Reconstruct Go source files from stored AST nodes.
use pgrx::prelude::*;
use serde_json::json;

use crate::parser::kinds::Kind;
use crate::sql::sql_escape;

/// Reconstruct a Go source file from its stored AST nodes.
///
/// Takes the UUID of a file-kind node and returns Go source text.
#[pg_extern]
fn reconstruct_go_file(file_node_id: pgrx::Uuid) -> String {
    let id_str = file_node_id.to_string();

    // Validate that the node exists and is a Go file node
    let (kind, language) = Spi::connect(|client| {
        let query = format!(
            "SELECT kind, language FROM kerai.nodes WHERE id = '{}'::uuid",
            sql_escape(&id_str)
        );
        let result = client.select(&query, None, &[]).unwrap();
        let mut kind = String::new();
        let mut lang = String::new();
        for row in result {
            kind = row
                .get_by_name::<String, _>("kind")
                .unwrap()
                .unwrap_or_default();
            lang = row
                .get_by_name::<String, _>("language")
                .unwrap()
                .unwrap_or_default();
        }
        (kind, lang)
    });

    if kind != "file" {
        pgrx::error!(
            "Node {} is kind '{}', expected 'file'",
            id_str,
            kind
        );
    }

    if language != "go" {
        pgrx::error!(
            "Node {} has language '{}', expected 'go'",
            id_str,
            language
        );
    }

    assemble_go_file(&id_str)
}

/// Internal: assemble Go source from child nodes.
fn assemble_go_file(file_node_id: &str) -> String {
    let items = query_child_items(file_node_id);
    let mut parts: Vec<String> = Vec::new();

    let comment_str = Kind::Comment.as_str();
    let comment_block_str = Kind::CommentBlock.as_str();

    for item in &items {
        if item.kind == comment_str || item.kind == comment_block_str {
            // Reconstruct comment
            let style = item
                .metadata
                .get("style")
                .and_then(|v| v.as_str())
                .unwrap_or("line");
            if style == "block" {
                parts.push(format!("/* {} */", item.content));
            } else {
                // Multi-line comment blocks
                for line in item.content.split('\n') {
                    if line.is_empty() {
                        parts.push("//".to_string());
                    } else {
                        parts.push(format!("// {}", line));
                    }
                }
            }
        } else if let Some(source) = item
            .metadata
            .get("source")
            .and_then(|v| v.as_str())
        {
            parts.push(source.to_string());
        } else {
            // Fallback: use content
            if !item.content.is_empty() {
                parts.push(item.content.clone());
            }
        }
    }

    let mut result = parts.join("\n\n");
    // Ensure trailing newline
    if !result.ends_with('\n') {
        result.push('\n');
    }
    result
}

/// A child item from the database.
struct ChildItem {
    kind: String,
    content: String,
    metadata: serde_json::Value,
}

/// Query direct children of a file node, ordered by position.
fn query_child_items(file_node_id: &str) -> Vec<ChildItem> {
    let mut items = Vec::new();

    Spi::connect(|client| {
        let query = format!(
            "SELECT kind, content, metadata FROM kerai.nodes \
             WHERE parent_id = '{}'::uuid \
             ORDER BY position ASC, id ASC",
            sql_escape(file_node_id)
        );

        let result = client.select(&query, None, &[]).unwrap();
        for row in result {
            let kind: String = row
                .get_by_name::<String, _>("kind")
                .unwrap()
                .unwrap_or_default();
            let content: String = row
                .get_by_name::<String, _>("content")
                .unwrap()
                .unwrap_or_default();
            let metadata: pgrx::JsonB = row
                .get_by_name::<pgrx::JsonB, _>("metadata")
                .unwrap()
                .unwrap_or(pgrx::JsonB(json!({})));

            items.push(ChildItem {
                kind,
                content,
                metadata: metadata.0,
            });
        }
    });

    items
}
