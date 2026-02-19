/// Markdown parser module — CommonMark source → kerai.nodes + kerai.edges.
use pgrx::prelude::*;
use serde_json::json;
use std::time::Instant;
use uuid::Uuid;

use crate::parser::ast_walker::NodeRow;
use crate::parser::inserter;
use crate::parser::path_builder::PathContext;
use crate::sql::sql_escape;

/// Delete existing markdown document nodes and their children for a given filename.
fn delete_markdown_nodes(instance_id: &str, filename: &str) {
    // Delete edges first, then nodes via recursive CTE
    Spi::run(&format!(
        "WITH RECURSIVE descendants AS (
            SELECT id FROM kerai.nodes
            WHERE instance_id = '{}'::uuid
            AND kind = 'document' AND content = '{}'
            UNION ALL
            SELECT n.id FROM kerai.nodes n
            JOIN descendants d ON n.parent_id = d.id
        )
        DELETE FROM kerai.edges WHERE source_id IN (SELECT id FROM descendants)
            OR target_id IN (SELECT id FROM descendants)",
        sql_escape(instance_id),
        sql_escape(filename),
    ))
    .ok();

    Spi::run(&format!(
        "WITH RECURSIVE descendants AS (
            SELECT id FROM kerai.nodes
            WHERE instance_id = '{}'::uuid
            AND kind = 'document' AND content = '{}'
            UNION ALL
            SELECT n.id FROM kerai.nodes n
            JOIN descendants d ON n.parent_id = d.id
        )
        DELETE FROM kerai.nodes WHERE id IN (SELECT id FROM descendants)",
        sql_escape(instance_id),
        sql_escape(filename),
    ))
    .ok();
}

#[allow(dead_code)]
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
    // delete_file_nodes looks for kind='file'; markdown uses kind='document',
    // so delete document subtree explicitly.
    delete_markdown_nodes(&instance_id, filename);
    inserter::delete_file_nodes(&instance_id, filename);

    let (node_count, edge_count) = parse_markdown_single(source, filename, &instance_id, None);

    // Auto-mint reward for markdown parsing
    if node_count > 0 {
        let details = json!({"file": filename, "nodes": node_count, "edges": edge_count});
        let details_str = details.to_string().replace('\'', "''");
        let _ = Spi::get_one::<pgrx::JsonB>(&format!(
            "SELECT kerai.mint_reward('parse_markdown', '{}'::jsonb)",
            details_str,
        ));
    }

    let elapsed = start.elapsed();
    pgrx::JsonB(json!({
        "file": filename,
        "nodes": node_count,
        "edges": edge_count,
        "elapsed_ms": elapsed.as_millis() as u64,
    }))
}

/// Parse markdown source, insert nodes/edges, return counts.
///
/// `parent_id` allows parenting the document node under a repo directory node.
pub(crate) fn parse_markdown_single(
    source: &str,
    filename: &str,
    instance_id: &str,
    parent_id: Option<&str>,
) -> (usize, usize) {
    let path_ctx = PathContext::with_root(filename);

    // Create document root node
    let doc_node_id = Uuid::new_v4().to_string();
    let doc_node = NodeRow {
        id: doc_node_id.clone(),
        instance_id: instance_id.to_string(),
        kind: kinds::DOCUMENT.to_string(),
        language: Some("markdown".to_string()),
        content: Some(filename.to_string()),
        parent_id: parent_id.map(|s| s.to_string()),
        position: 0,
        path: path_ctx.path(),
        metadata: json!({"line_count": source.lines().count()}),
        span_start: None,
        span_end: None,
    };
    inserter::insert_nodes(&[doc_node]);

    // Walk markdown and collect nodes/edges
    let (nodes, edges) = walker::walk_markdown(source, filename, instance_id, &doc_node_id);

    let node_count = nodes.len() + 1; // +1 for document node
    let edge_count = edges.len();

    inserter::insert_nodes(&nodes);
    inserter::insert_edges(&edges);

    (node_count, edge_count)
}
