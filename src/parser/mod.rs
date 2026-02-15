/// Parser module — Rust source → kerai.nodes + kerai.edges.
use pgrx::prelude::*;
use serde_json::json;
use std::path::Path;
use std::time::Instant;
use uuid::Uuid;

mod ast_walker;
mod cargo_parser;
#[allow(dead_code)]
mod comment_extractor;
mod crate_walker;
#[allow(dead_code)]
mod inserter;
#[allow(dead_code)]
mod kinds;
#[allow(dead_code)]
mod metadata;
#[allow(dead_code)]
mod path_builder;

use ast_walker::NodeRow;
use path_builder::PathContext;

/// Get the self instance ID from the database.
fn get_self_instance_id() -> String {
    Spi::get_one::<String>("SELECT id::text FROM kerai.instances WHERE is_self = true")
        .expect("Failed to query self instance")
        .expect("No self instance found — run kerai.bootstrap_instance() first")
}

/// Parse an entire Rust crate into kerai.nodes and kerai.edges.
#[pg_extern]
fn parse_crate(path: &str) -> pgrx::JsonB {
    let start = Instant::now();
    let crate_root = Path::new(path);

    if !crate_root.exists() {
        pgrx::error!("Crate path does not exist: {}", path);
    }

    let cargo_path = crate_root.join("Cargo.toml");
    if !cargo_path.exists() {
        pgrx::error!("No Cargo.toml found at: {}", cargo_path.display());
    }

    let instance_id = get_self_instance_id();

    // Parse Cargo.toml
    let (cargo_nodes, crate_node_id, crate_name) =
        cargo_parser::parse_cargo_toml(&cargo_path, &instance_id)
            .unwrap_or_else(|e| pgrx::error!("Failed to parse Cargo.toml: {}", e));

    inserter::insert_nodes(&cargo_nodes);
    let mut total_nodes = cargo_nodes.len();
    let mut total_edges = 0usize;

    // Discover .rs files
    let rs_files = crate_walker::discover_rs_files(crate_root)
        .unwrap_or_else(|e| pgrx::error!("Failed to discover .rs files: {}", e));

    let file_count = rs_files.len();

    for (file_idx, file_path) in rs_files.iter().enumerate() {
        let source = match std::fs::read_to_string(file_path) {
            Ok(s) => s,
            Err(e) => {
                warning!("Skipping {}: {}", file_path.display(), e);
                continue;
            }
        };

        let filename = file_path
            .strip_prefix(crate_root)
            .unwrap_or(file_path)
            .to_string_lossy()
            .to_string();

        let (nodes, edges) = parse_single_file(
            &source,
            &filename,
            &instance_id,
            Some(&crate_node_id),
            &crate_name,
            file_idx as i32,
        );

        total_nodes += nodes;
        total_edges += edges;
    }

    let elapsed = start.elapsed();
    pgrx::JsonB(json!({
        "crate": crate_name,
        "files": file_count,
        "nodes": total_nodes,
        "edges": total_edges,
        "elapsed_ms": elapsed.as_millis() as u64,
    }))
}

/// Parse a single Rust file into kerai.nodes and kerai.edges.
#[pg_extern]
fn parse_file(path: &str) -> pgrx::JsonB {
    let start = Instant::now();
    let file_path = Path::new(path);

    if !file_path.exists() {
        pgrx::error!("File does not exist: {}", path);
    }

    let source = std::fs::read_to_string(file_path)
        .unwrap_or_else(|e| pgrx::error!("Failed to read file: {}", e));

    let instance_id = get_self_instance_id();
    let filename = file_path
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string());

    // Delete existing nodes for this file (idempotent re-parse)
    inserter::delete_file_nodes(&instance_id, &filename);

    let (node_count, edge_count) =
        parse_single_file(&source, &filename, &instance_id, None, &filename, 0);

    let elapsed = start.elapsed();
    pgrx::JsonB(json!({
        "file": filename,
        "nodes": node_count,
        "edges": edge_count,
        "elapsed_ms": elapsed.as_millis() as u64,
    }))
}

/// Parse Rust source text directly (not from a file).
#[pg_extern]
fn parse_source(source: &str, filename: &str) -> pgrx::JsonB {
    let start = Instant::now();
    let instance_id = get_self_instance_id();

    // Delete existing nodes for this filename (idempotent)
    inserter::delete_file_nodes(&instance_id, filename);

    let (node_count, edge_count) =
        parse_single_file(source, filename, &instance_id, None, filename, 0);

    let elapsed = start.elapsed();
    pgrx::JsonB(json!({
        "file": filename,
        "nodes": node_count,
        "edges": edge_count,
        "elapsed_ms": elapsed.as_millis() as u64,
    }))
}

/// Internal: parse a single file's source, insert nodes/edges, return counts.
fn parse_single_file(
    source: &str,
    filename: &str,
    instance_id: &str,
    parent_id: Option<&str>,
    path_root: &str,
    position: i32,
) -> (usize, usize) {
    // Parse with syn
    let syn_file = match syn::parse_file(source) {
        Ok(f) => f,
        Err(e) => {
            warning!("Failed to parse {}: {}", filename, e);
            return (0, 0);
        }
    };

    // Create file node
    let file_node_id = Uuid::new_v4().to_string();
    let path_ctx = PathContext::with_root(path_root);

    let file_node = NodeRow {
        id: file_node_id.clone(),
        instance_id: instance_id.to_string(),
        kind: kinds::FILE.to_string(),
        language: Some("rust".to_string()),
        content: Some(filename.to_string()),
        parent_id: parent_id.map(|s| s.to_string()),
        position,
        path: path_ctx.path(),
        metadata: json!({"line_count": source.lines().count()}),
        span_start: None,
        span_end: None,
    };

    inserter::insert_nodes(&[file_node]);

    // Walk AST
    let (mut nodes, mut edges) =
        ast_walker::walk_file(&syn_file, &file_node_id, instance_id, path_ctx);

    // Extract comments and add as nodes/edges
    let comments = comment_extractor::extract_comments(source);
    for comment in &comments {
        if comment.is_doc {
            // Doc comments are already handled via syn attributes, skip
            continue;
        }
        let comment_id = Uuid::new_v4().to_string();
        let kind = kinds::COMMENT;

        nodes.push(NodeRow {
            id: comment_id.clone(),
            instance_id: instance_id.to_string(),
            kind: kind.to_string(),
            language: Some("rust".to_string()),
            content: Some(comment.text.clone()),
            parent_id: Some(file_node_id.clone()),
            position: comment.line as i32,
            path: None,
            metadata: json!({"line": comment.line, "col": comment.col}),
            span_start: Some(comment.line as i32),
            span_end: Some(comment.line as i32),
        });

        // Find nearest AST node after this comment to create "documents" edge
        if let Some(target_node) = find_nearest_node_after_line(&nodes, comment.line as i32) {
            edges.push(ast_walker::EdgeRow {
                id: Uuid::new_v4().to_string(),
                source_id: comment_id,
                target_id: target_node,
                relation: "documents".to_string(),
                metadata: json!({}),
            });
        }
    }

    let node_count = nodes.len() + 1; // +1 for file node
    let edge_count = edges.len();

    inserter::insert_nodes(&nodes);
    inserter::insert_edges(&edges);

    (node_count, edge_count)
}

/// Find the nearest AST node with span_start >= the given line.
fn find_nearest_node_after_line(nodes: &[NodeRow], line: i32) -> Option<String> {
    let mut best: Option<(i32, &str)> = None;

    for node in nodes {
        if let Some(start) = node.span_start {
            if start >= line {
                match best {
                    Some((best_line, _)) if start < best_line => {
                        best = Some((start, &node.id));
                    }
                    None => {
                        best = Some((start, &node.id));
                    }
                    _ => {}
                }
            }
        }
    }

    best.map(|(_, id)| id.to_string())
}
