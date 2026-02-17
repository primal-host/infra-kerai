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
mod normalizer;
#[allow(dead_code)]
mod path_builder;
pub mod markdown;

use ast_walker::NodeRow;
use comment_extractor::{CommentBlock, CommentPlacement};
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

    // Auto-mint reward for crate parsing
    let details = json!({
        "crate": crate_name,
        "files": file_count,
        "nodes": total_nodes,
        "edges": total_edges,
    });
    let details_str = details.to_string().replace('\'', "''");
    let _ = Spi::get_one::<pgrx::JsonB>(&format!(
        "SELECT kerai.mint_reward('parse_crate', '{}'::jsonb)",
        details_str,
    ));

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

    // Auto-mint reward for file parsing
    if node_count > 0 {
        let details = json!({"file": filename, "nodes": node_count, "edges": edge_count});
        let details_str = details.to_string().replace('\'', "''");
        let _ = Spi::get_one::<pgrx::JsonB>(&format!(
            "SELECT kerai.mint_reward('parse_file', '{}'::jsonb)",
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

/// Parse Rust source text directly (not from a file).
#[pg_extern]
fn parse_source(source: &str, filename: &str) -> pgrx::JsonB {
    let start = Instant::now();
    let instance_id = get_self_instance_id();

    // Delete existing nodes for this filename (idempotent)
    inserter::delete_file_nodes(&instance_id, filename);

    let (node_count, edge_count) =
        parse_single_file(source, filename, &instance_id, None, filename, 0);

    // Auto-mint reward for source parsing
    if node_count > 0 {
        let details = json!({"file": filename, "nodes": node_count, "edges": edge_count});
        let details_str = details.to_string().replace('\'', "''");
        let _ = Spi::get_one::<pgrx::JsonB>(&format!(
            "SELECT kerai.mint_reward('parse_file', '{}'::jsonb)",
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

/// Internal: parse a single file's source, insert nodes/edges, return counts.
fn parse_single_file(
    source: &str,
    filename: &str,
    instance_id: &str,
    parent_id: Option<&str>,
    path_root: &str,
    position: i32,
) -> (usize, usize) {
    // 1. Normalize source
    let normalized = normalizer::normalize(source);

    // 2. Parse with syn
    let syn_file = match syn::parse_file(&normalized) {
        Ok(f) => f,
        Err(e) => {
            warning!("Failed to parse {}: {}", filename, e);
            return (0, 0);
        }
    };

    // 3. Create file node
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
        metadata: json!({"line_count": normalized.lines().count()}),
        span_start: None,
        span_end: None,
    };

    inserter::insert_nodes(&[file_node]);

    // 4. Walk AST
    let (mut nodes, mut edges) =
        ast_walker::walk_file(&syn_file, &file_node_id, instance_id, path_ctx);

    // 4b. Normalize top-level item positions to use span_start (line numbers)
    // so they interleave correctly with comments (which also use line numbers).
    for node in &mut nodes {
        if node.parent_id.as_deref() == Some(&file_node_id) {
            if let Some(start) = node.span_start {
                node.position = start;
            }
        }
    }

    // 5. Collect string literal exclusion zones
    let exclusions = comment_extractor::collect_string_spans(&syn_file);

    // 6. Extract comments with exclusion zones
    let raw_comments = comment_extractor::extract_comments(&normalized, &exclusions);

    // 7. Group consecutive line comments into blocks
    let mut blocks = comment_extractor::group_comments(raw_comments);

    // Filter out doc comments (already handled via syn attributes)
    blocks.retain(|b| !b.is_doc);

    // 8. Match comment blocks to AST nodes (sets placement)
    let matches = match_comments_to_ast(&mut blocks, &nodes);

    // 9. Create NodeRow + EdgeRow for each comment block
    for (block_idx, block) in blocks.iter().enumerate() {
        let comment_id = Uuid::new_v4().to_string();
        let kind = if !block.is_block_style && block.lines.len() > 1 {
            kinds::COMMENT_BLOCK
        } else {
            kinds::COMMENT
        };
        let style = if block.is_block_style { "block" } else { "line" };
        let placement = match block.placement {
            CommentPlacement::Above => "above",
            CommentPlacement::Trailing => "trailing",
            CommentPlacement::Between => "between",
            CommentPlacement::Eof => "eof",
        };

        let content = block.lines.join("\n");

        nodes.push(NodeRow {
            id: comment_id.clone(),
            instance_id: instance_id.to_string(),
            kind: kind.to_string(),
            language: Some("rust".to_string()),
            content: Some(content),
            parent_id: Some(file_node_id.clone()),
            position: block.start_line as i32,
            path: None,
            metadata: json!({
                "start_line": block.start_line,
                "end_line": block.end_line,
                "col": block.col,
                "placement": placement,
                "style": style,
                "line_count": block.lines.len(),
            }),
            span_start: Some(block.start_line as i32),
            span_end: Some(block.end_line as i32),
        });

        // Create "documents" edge if matched to a node
        if let Some(ref target_id) = matches[block_idx] {
            edges.push(ast_walker::EdgeRow {
                id: Uuid::new_v4().to_string(),
                source_id: comment_id,
                target_id: target_id.clone(),
                relation: "documents".to_string(),
                metadata: json!({"placement": placement}),
            });
        }
    }

    let node_count = nodes.len() + 1; // +1 for file node
    let edge_count = edges.len();

    inserter::insert_nodes(&nodes);
    inserter::insert_edges(&edges);

    (node_count, edge_count)
}

/// Match comment blocks to AST nodes and classify placement.
///
/// Returns a Vec with one entry per block: Some(node_id) for the target,
/// or None for eof comments.
fn match_comments_to_ast(
    blocks: &mut [CommentBlock],
    nodes: &[NodeRow],
) -> Vec<Option<String>> {
    // Build sorted list of top-level AST nodes by span_start
    // (only nodes with span info, excluding comments themselves)
    let mut ast_spans: Vec<(i32, &str)> = nodes
        .iter()
        .filter(|n| {
            n.span_start.is_some()
                && n.kind != kinds::COMMENT
                && n.kind != kinds::COMMENT_BLOCK
        })
        .map(|n| (n.span_start.unwrap(), n.id.as_str()))
        .collect();
    ast_spans.sort_by_key(|&(line, _)| line);

    let mut results = Vec::with_capacity(blocks.len());

    for block in blocks.iter_mut() {
        let start = block.start_line as i32;
        let end = block.end_line as i32;

        // Find the nearest AST node AFTER this comment block
        let next_node = ast_spans
            .iter()
            .find(|&&(line, _)| line > end);

        // Find the nearest AST node BEFORE or AT the start line
        let prev_node = ast_spans
            .iter()
            .rev()
            .find(|&&(line, _)| line < start);

        // Find AST node ON the same line as a single-line comment
        let same_line_node = if start == end {
            ast_spans.iter().find(|&&(line, _)| line == start)
        } else {
            None
        };

        // Classify placement
        if let Some(&(node_line, node_id)) = same_line_node {
            // Trailing: comment is on the same line as an AST node
            // (AST node starts on same line, code is before the comment)
            if node_line == start {
                block.placement = CommentPlacement::Trailing;
                results.push(Some(node_id.to_string()));
                continue;
            }
        }

        match next_node {
            Some(&(next_line, next_id)) => {
                if next_line == end + 1 {
                    // Above: directly above next node (no blank line gap)
                    block.placement = CommentPlacement::Above;
                    results.push(Some(next_id.to_string()));
                } else if prev_node.is_some() {
                    // Between: gap before next node AND a previous node exists
                    block.placement = CommentPlacement::Between;
                    results.push(Some(next_id.to_string()));
                } else {
                    // Above (with gap): no previous node, comment is before first node
                    block.placement = CommentPlacement::Above;
                    results.push(Some(next_id.to_string()));
                }
            }
            None => {
                // Eof: no AST node after this comment
                block.placement = CommentPlacement::Eof;
                results.push(None);
            }
        }
    }

    results
}
