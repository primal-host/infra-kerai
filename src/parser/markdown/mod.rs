/// Markdown parser module — CommonMark source → kerai.nodes + kerai.edges.
use pgrx::prelude::*;
use serde_json::json;
use std::time::Instant;
use uuid::Uuid;

use crate::parser::ast_walker::{NodeRow, EdgeRow};
use crate::parser::inserter;
use crate::parser::path_builder::PathContext;

pub mod kinds;
mod walker;

/// Parse a markdown document into kerai.nodes and kerai.edges.
///
/// Returns JSON: `{file, nodes, edges, elapsed_ms}`.
#[pg_extern]
fn parse_markdown(source: &str, filename: &str) -> pgrx::JsonB {
    let start = Instant::now();
    let instance_id = super::get_self_instance_id();

    // Delete existing nodes for this filename (idempotent re-parse)
    inserter::delete_file_nodes(&instance_id, filename);

    let path_ctx = PathContext::with_root(filename);

    // Create document root node
    let doc_node_id = Uuid::new_v4().to_string();
    let doc_node = NodeRow {
        id: doc_node_id.clone(),
        instance_id: instance_id.clone(),
        kind: kinds::DOCUMENT.to_string(),
        language: Some("markdown".to_string()),
        content: Some(filename.to_string()),
        parent_id: None,
        position: 0,
        path: path_ctx.path(),
        metadata: json!({"line_count": source.lines().count()}),
        span_start: None,
        span_end: None,
    };
    inserter::insert_nodes(&[doc_node]);

    // Walk markdown and collect nodes/edges
    let (nodes, edges) = walker::walk_markdown(source, filename, &instance_id, &doc_node_id);

    let node_count = nodes.len() + 1; // +1 for document node
    let edge_count = edges.len();

    inserter::insert_nodes(&nodes);
    inserter::insert_edges(&edges);

    let elapsed = start.elapsed();
    pgrx::JsonB(json!({
        "file": filename,
        "nodes": node_count,
        "edges": edge_count,
        "elapsed_ms": elapsed.as_millis() as u64,
    }))
}
