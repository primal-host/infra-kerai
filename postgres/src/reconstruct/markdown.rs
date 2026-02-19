/// Reconstruct CommonMark from stored document nodes via SPI queries.
use pgrx::prelude::*;

use crate::parser::markdown::kinds;

/// Child node from the database.
struct MdNode {
    id: String,
    kind: String,
    content: Option<String>,
    metadata: serde_json::Value,
}

/// Reconstruct a markdown document from its stored node tree.
/// Takes the UUID of a document-kind node and returns CommonMark text.
#[pg_extern]
fn reconstruct_markdown(document_node_id: pgrx::Uuid) -> String {
    let id_str = document_node_id.to_string();

    // Validate that the node exists and is a document node
    let kind = Spi::get_one::<String>(&format!(
        "SELECT kind FROM kerai.nodes WHERE id = '{}'::uuid",
        id_str.replace('\'', "''")
    ))
    .expect("Failed to query node")
    .unwrap_or_else(|| pgrx::error!("Node not found: {}", id_str));

    if kind != "document" {
        pgrx::error!(
            "Node {} is kind '{}', expected 'document'",
            id_str,
            kind
        );
    }

    let mut output = String::new();
    reconstruct_children(&id_str, &mut output, 0);
    output.trim_end().to_string()
}

/// Recursively reconstruct children of a node.
fn reconstruct_children(parent_id: &str, output: &mut String, depth: usize) {
    let children = query_children(parent_id);

    for child in &children {
        emit_node(child, output, depth);
    }
}

/// Emit a single node as CommonMark.
fn emit_node(node: &MdNode, output: &mut String, depth: usize) {
    match node.kind.as_str() {
        kinds::HEADING => {
            let level = node.metadata.get("level")
                .and_then(|v| v.as_u64())
                .unwrap_or(1) as usize;
            let hashes = "#".repeat(level);
            let text = node.content.as_deref().unwrap_or("");
            output.push_str(&format!("{} {}\n\n", hashes, text));

            // Recurse into heading's children (sub-sections and content)
            reconstruct_children(&node.id, output, depth);
        }

        kinds::PARAGRAPH => {
            let text = node.content.as_deref().unwrap_or("");
            if !text.is_empty() {
                output.push_str(text);
                output.push_str("\n\n");
            }
        }

        kinds::BLOCKQUOTE => {
            // Collect blockquote content from children
            let children = query_children(&node.id);
            for child in &children {
                let text = child.content.as_deref().unwrap_or("");
                for line in text.lines() {
                    output.push_str(&format!("> {}\n", line));
                }
            }
            output.push('\n');
        }

        kinds::CODE_BLOCK => {
            let lang = node.metadata.get("language")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let content = node.content.as_deref().unwrap_or("");
            output.push_str(&format!("```{}\n{}\n```\n\n", lang, content.trim_end()));
        }

        kinds::LIST => {
            let ordered = node.metadata.get("ordered")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let start = node.metadata.get("start")
                .and_then(|v| v.as_u64())
                .unwrap_or(1);
            let items = query_children(&node.id);
            for (i, item) in items.iter().enumerate() {
                let prefix = if ordered {
                    format!("{}. ", start as usize + i)
                } else {
                    "- ".to_string()
                };
                let text = item.content.as_deref().unwrap_or("");
                output.push_str(&format!("{}{}\n", prefix, text));
            }
            output.push('\n');
        }

        kinds::LIST_ITEM => {
            // Handled by parent LIST
            let text = node.content.as_deref().unwrap_or("");
            output.push_str(&format!("- {}\n", text));
        }

        kinds::THEMATIC_BREAK => {
            output.push_str("---\n\n");
        }

        kinds::TABLE => {
            let children = query_children(&node.id);
            for (i, child) in children.iter().enumerate() {
                emit_table_row(child, output);
                if i == 0 {
                    // After header row, emit separator
                    let cells = query_children(&child.id);
                    let sep: Vec<String> = cells.iter().map(|_| "---".to_string()).collect();
                    output.push_str(&format!("| {} |\n", sep.join(" | ")));
                }
            }
            output.push('\n');
        }

        kinds::LINK => {
            let text = node.content.as_deref().unwrap_or("");
            let url = node.metadata.get("url")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            output.push_str(&format!("[{}]({})", text, url));
        }

        kinds::IMAGE => {
            let alt = node.content.as_deref().unwrap_or("");
            let url = node.metadata.get("url")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            output.push_str(&format!("![{}]({})\n\n", alt, url));
        }

        kinds::HTML_BLOCK => {
            let content = node.content.as_deref().unwrap_or("");
            output.push_str(content);
            output.push('\n');
        }

        // Inline formatting nodes — emit content directly
        kinds::EMPHASIS | kinds::STRONG | kinds::STRIKETHROUGH
        | kinds::TEXT | kinds::INLINE_CODE => {
            if let Some(text) = &node.content {
                output.push_str(text);
            }
        }

        _ => {
            // Unknown kind — emit content if present, then recurse
            if let Some(text) = &node.content {
                output.push_str(text);
                output.push_str("\n\n");
            }
            reconstruct_children(&node.id, output, depth + 1);
        }
    }
}

/// Emit a table row (head or body).
fn emit_table_row(node: &MdNode, output: &mut String) {
    let cells = query_children(&node.id);
    let cell_texts: Vec<String> = cells.iter()
        .map(|c| c.content.as_deref().unwrap_or("").to_string())
        .collect();
    output.push_str(&format!("| {} |\n", cell_texts.join(" | ")));
}

/// Query direct children of a node, ordered by position.
fn query_children(parent_id: &str) -> Vec<MdNode> {
    let mut children = Vec::new();

    Spi::connect(|client| {
        let query = format!(
            "SELECT id::text, kind, content, metadata \
             FROM kerai.nodes \
             WHERE parent_id = '{}'::uuid \
             ORDER BY position ASC",
            parent_id.replace('\'', "''")
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
            let metadata: pgrx::JsonB = row.get_by_name::<pgrx::JsonB, _>("metadata")
                .unwrap()
                .unwrap_or_else(|| pgrx::JsonB(serde_json::json!({})));

            children.push(MdNode { id, kind, content, metadata: metadata.0 });
        }
    });

    children
}
