/// LaTeX parser module — LaTeX/BibTeX source → kerai.nodes + kerai.edges via tree-sitter + biblatex.
use pgrx::prelude::*;
use serde_json::json;
use std::path::Path;
use std::time::Instant;
use uuid::Uuid;

use crate::parser::ast_walker::{EdgeRow, NodeRow};
use crate::parser::inserter;
use crate::parser::kinds::Kind;
use crate::parser::path_builder::PathContext;
use crate::parser::treesitter::{self, TsLanguage};

pub mod kinds;
mod metadata;
mod walker;
mod bibtex;

/// Parse LaTeX source text directly into kerai.nodes and kerai.edges.
///
/// Returns JSON: `{file, language, nodes, edges, elapsed_ms}`.
#[pg_extern]
fn parse_latex_source(source: &str, filename: &str) -> pgrx::JsonB {
    let start = Instant::now();
    let instance_id = super::get_self_instance_id();

    // Delete existing nodes for this filename (idempotent re-parse)
    inserter::delete_file_nodes(&instance_id, filename);

    let (node_count, edge_count) = parse_latex_single(source, filename, &instance_id, None);

    // Auto-mint reward
    if node_count > 0 {
        let details = json!({"file": filename, "language": "latex", "nodes": node_count, "edges": edge_count});
        let details_str = details.to_string().replace('\'', "''");
        let _ = Spi::get_one::<pgrx::JsonB>(&format!(
            "SELECT kerai.mint_reward('parse_latex_source', '{}'::jsonb)",
            details_str,
        ));
    }

    let elapsed = start.elapsed();
    pgrx::JsonB(json!({
        "file": filename,
        "language": "latex",
        "nodes": node_count,
        "edges": edge_count,
        "elapsed_ms": elapsed.as_millis() as u64,
    }))
}

/// Parse a LaTeX file from disk into kerai.nodes and kerai.edges.
///
/// Returns JSON: `{file, language, nodes, edges, elapsed_ms}`.
#[pg_extern]
fn parse_latex_file(path: &str) -> pgrx::JsonB {
    let start = Instant::now();
    let file_path = Path::new(path);

    if !file_path.exists() {
        pgrx::error!("File does not exist: {}", path);
    }

    let source = std::fs::read_to_string(file_path)
        .unwrap_or_else(|e| pgrx::error!("Failed to read file: {}", e));

    let instance_id = super::get_self_instance_id();
    let filename = file_path
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string());

    // Delete existing nodes for this file (idempotent re-parse)
    inserter::delete_file_nodes(&instance_id, &filename);

    let (node_count, edge_count) = parse_latex_single(&source, &filename, &instance_id, None);

    // Auto-mint reward
    if node_count > 0 {
        let details = json!({"file": filename, "language": "latex", "nodes": node_count, "edges": edge_count});
        let details_str = details.to_string().replace('\'', "''");
        let _ = Spi::get_one::<pgrx::JsonB>(&format!(
            "SELECT kerai.mint_reward('parse_latex_file', '{}'::jsonb)",
            details_str,
        ));
    }

    let elapsed = start.elapsed();
    pgrx::JsonB(json!({
        "file": filename,
        "language": "latex",
        "nodes": node_count,
        "edges": edge_count,
        "elapsed_ms": elapsed.as_millis() as u64,
    }))
}

/// Parse BibTeX source text directly into kerai.nodes and kerai.edges.
///
/// Returns JSON: `{file, language, nodes, edges, elapsed_ms}`.
#[pg_extern]
fn parse_bibtex_source(source: &str, filename: &str) -> pgrx::JsonB {
    let start = Instant::now();
    let instance_id = super::get_self_instance_id();

    // Delete existing nodes for this filename (idempotent re-parse)
    inserter::delete_file_nodes(&instance_id, filename);

    let (node_count, edge_count) = parse_bibtex_single(source, filename, &instance_id, None);

    // Auto-mint reward
    if node_count > 0 {
        let details = json!({"file": filename, "language": "bibtex", "nodes": node_count, "edges": edge_count});
        let details_str = details.to_string().replace('\'', "''");
        let _ = Spi::get_one::<pgrx::JsonB>(&format!(
            "SELECT kerai.mint_reward('parse_bibtex_source', '{}'::jsonb)",
            details_str,
        ));
    }

    let elapsed = start.elapsed();
    pgrx::JsonB(json!({
        "file": filename,
        "language": "bibtex",
        "nodes": node_count,
        "edges": edge_count,
        "elapsed_ms": elapsed.as_millis() as u64,
    }))
}

/// Parse a BibTeX file from disk into kerai.nodes and kerai.edges.
///
/// Returns JSON: `{file, language, nodes, edges, elapsed_ms}`.
#[pg_extern]
fn parse_bibtex_file(path: &str) -> pgrx::JsonB {
    let start = Instant::now();
    let file_path = Path::new(path);

    if !file_path.exists() {
        pgrx::error!("File does not exist: {}", path);
    }

    let source = std::fs::read_to_string(file_path)
        .unwrap_or_else(|e| pgrx::error!("Failed to read file: {}", e));

    let instance_id = super::get_self_instance_id();
    let filename = file_path
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string());

    // Delete existing nodes for this file (idempotent re-parse)
    inserter::delete_file_nodes(&instance_id, &filename);

    let (node_count, edge_count) = parse_bibtex_single(&source, &filename, &instance_id, None);

    // Auto-mint reward
    if node_count > 0 {
        let details = json!({"file": filename, "language": "bibtex", "nodes": node_count, "edges": edge_count});
        let details_str = details.to_string().replace('\'', "''");
        let _ = Spi::get_one::<pgrx::JsonB>(&format!(
            "SELECT kerai.mint_reward('parse_bibtex_file', '{}'::jsonb)",
            details_str,
        ));
    }

    let elapsed = start.elapsed();
    pgrx::JsonB(json!({
        "file": filename,
        "language": "bibtex",
        "nodes": node_count,
        "edges": edge_count,
        "elapsed_ms": elapsed.as_millis() as u64,
    }))
}

/// Link citation nodes to bib_entry nodes across the database.
///
/// Finds all `latex_citation` nodes and matches their keys to `bib_entry` nodes,
/// creating `cites` edges. This should be called after parsing both .tex and .bib files.
///
/// Returns JSON: `{linked, unresolved}`.
#[pg_extern]
fn link_citations() -> pgrx::JsonB {
    let start = Instant::now();
    let mut linked = 0u64;
    let mut unresolved = 0u64;

    // Build a map of cite_key → bib_entry node ID
    let mut bib_map: std::collections::HashMap<String, String> = std::collections::HashMap::new();

    Spi::connect(|client| {
        let result = client
            .select(
                "SELECT id::text, content FROM kerai.nodes WHERE kind = 'bib_entry'",
                None,
                &[],
            )
            .expect("Failed to query bib_entry nodes");

        for row in result {
            let id: String = row
                .get_by_name::<String, _>("id")
                .expect("id column")
                .unwrap_or_default();
            let key: String = row
                .get_by_name::<String, _>("content")
                .expect("content column")
                .unwrap_or_default();
            if !key.is_empty() {
                bib_map.insert(key, id);
            }
        }
    });

    if bib_map.is_empty() {
        return pgrx::JsonB(json!({
            "linked": 0,
            "unresolved": 0,
            "elapsed_ms": start.elapsed().as_millis() as u64,
            "message": "No bib_entry nodes found. Parse .bib files first."
        }));
    }

    // Find all citation nodes and their keys
    let mut citations: Vec<(String, Vec<String>)> = Vec::new();

    Spi::connect(|client| {
        let result = client
            .select(
                "SELECT id::text, metadata->'keys' AS keys FROM kerai.nodes WHERE kind = 'latex_citation'",
                None,
                &[],
            )
            .expect("Failed to query citation nodes");

        for row in result {
            let id: String = row
                .get_by_name::<String, _>("id")
                .expect("id column")
                .unwrap_or_default();
            let keys_json: Option<pgrx::JsonB> = row
                .get_by_name::<pgrx::JsonB, _>("keys")
                .unwrap_or(None);

            if let Some(pgrx::JsonB(keys_val)) = keys_json {
                if let Some(arr) = keys_val.as_array() {
                    let keys: Vec<String> = arr
                        .iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect();
                    if !keys.is_empty() {
                        citations.push((id, keys));
                    }
                }
            }
        }
    });

    // Create edges for matched citations
    let mut edges: Vec<EdgeRow> = Vec::new();

    for (cite_node_id, keys) in &citations {
        for key in keys {
            if let Some(bib_node_id) = bib_map.get(key) {
                edges.push(EdgeRow {
                    id: Uuid::new_v4().to_string(),
                    source_id: cite_node_id.clone(),
                    target_id: bib_node_id.clone(),
                    relation: "cites".to_string(),
                    metadata: json!({"key": key}),
                });
                linked += 1;
            } else {
                unresolved += 1;
            }
        }
    }

    if !edges.is_empty() {
        inserter::insert_edges(&edges);
    }

    let elapsed = start.elapsed();
    pgrx::JsonB(json!({
        "linked": linked,
        "unresolved": unresolved,
        "elapsed_ms": elapsed.as_millis() as u64,
    }))
}

/// Parse LaTeX source, insert nodes/edges, return counts.
pub(crate) fn parse_latex_single(
    source: &str,
    filename: &str,
    instance_id: &str,
    parent_id: Option<&str>,
) -> (usize, usize) {
    // Parse with tree-sitter
    let tree = match treesitter::parse(source, TsLanguage::Latex) {
        Some(t) => t,
        None => {
            warning!("Failed to parse LaTeX source: {}", filename);
            return (0, 0);
        }
    };

    // Create file node
    let file_node_id = Uuid::new_v4().to_string();
    let path_ctx = PathContext::with_root(filename);

    let file_node = NodeRow {
        id: file_node_id.clone(),
        instance_id: instance_id.to_string(),
        kind: Kind::File.as_str().to_string(),
        language: Some("latex".to_string()),
        content: Some(filename.to_string()),
        parent_id: parent_id.map(|s| s.to_string()),
        position: 0,
        path: path_ctx.path(),
        metadata: json!({"line_count": source.lines().count()}),
        span_start: None,
        span_end: None,
    };
    inserter::insert_nodes(&[file_node]);

    // Walk LaTeX CST
    let (nodes, edges, _pending_cites) =
        walker::walk_latex_file(&tree, source, &file_node_id, instance_id, path_ctx);

    let node_count = nodes.len() + 1; // +1 for file node
    let edge_count = edges.len();

    inserter::insert_nodes(&nodes);
    inserter::insert_edges(&edges);

    (node_count, edge_count)
}

/// Parse BibTeX source, insert nodes/edges, return counts.
pub(crate) fn parse_bibtex_single(
    source: &str,
    filename: &str,
    instance_id: &str,
    parent_id: Option<&str>,
) -> (usize, usize) {
    // Create file node
    let file_node_id = Uuid::new_v4().to_string();
    let mut path_ctx = PathContext::with_root(filename);

    let file_node = NodeRow {
        id: file_node_id.clone(),
        instance_id: instance_id.to_string(),
        kind: Kind::File.as_str().to_string(),
        language: Some("bibtex".to_string()),
        content: Some(filename.to_string()),
        parent_id: parent_id.map(|s| s.to_string()),
        position: 0,
        path: path_ctx.path(),
        metadata: json!({"line_count": source.lines().count()}),
        span_start: None,
        span_end: None,
    };
    inserter::insert_nodes(&[file_node]);

    // Parse BibTeX
    let (nodes, edges) =
        bibtex::parse_bibtex(source, &file_node_id, instance_id, &mut path_ctx);

    let node_count = nodes.len() + 1; // +1 for file node
    let edge_count = edges.len();

    inserter::insert_nodes(&nodes);
    inserter::insert_edges(&edges);

    (node_count, edge_count)
}
