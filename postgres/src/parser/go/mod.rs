/// Go parser module — Go source → kerai.nodes + kerai.edges via tree-sitter.
use pgrx::prelude::*;
use serde_json::json;
use std::path::Path;
use std::time::Instant;
use uuid::Uuid;

use crate::parser::ast_walker::{EdgeRow, NodeRow};
use crate::parser::comment_extractor::{CommentBlock, CommentPlacement};
use crate::parser::inserter;
use crate::parser::kinds::Kind;
use crate::parser::normalizer;
use crate::parser::path_builder::PathContext;
use crate::parser::treesitter::{self, TsLanguage};

#[allow(dead_code)]
pub mod kinds;
mod metadata;
pub mod suggestion_rules;
mod walker;

/// Parse Go source text directly into kerai.nodes and kerai.edges.
///
/// Returns JSON: `{file, language, nodes, edges, elapsed_ms}`.
#[pg_extern]
fn parse_go_source(source: &str, filename: &str) -> pgrx::JsonB {
    let start = Instant::now();
    let instance_id = super::get_self_instance_id();

    // Delete existing nodes for this filename (idempotent re-parse)
    inserter::delete_file_nodes(&instance_id, filename);

    let (node_count, edge_count) = parse_go_single(source, filename, &instance_id, None);

    // Auto-mint reward
    if node_count > 0 {
        let details = json!({"file": filename, "language": "go", "nodes": node_count, "edges": edge_count});
        let details_str = details.to_string().replace('\'', "''");
        let _ = Spi::get_one::<pgrx::JsonB>(&format!(
            "SELECT kerai.mint_reward('parse_go_source', '{}'::jsonb)",
            details_str,
        ));
    }

    let elapsed = start.elapsed();
    pgrx::JsonB(json!({
        "file": filename,
        "language": "go",
        "nodes": node_count,
        "edges": edge_count,
        "elapsed_ms": elapsed.as_millis() as u64,
    }))
}

/// Parse a Go file from disk into kerai.nodes and kerai.edges.
///
/// Returns JSON: `{file, language, nodes, edges, elapsed_ms}`.
#[pg_extern]
fn parse_go_file(path: &str) -> pgrx::JsonB {
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

    let (node_count, edge_count) = parse_go_single(&source, &filename, &instance_id, None);

    // Auto-mint reward
    if node_count > 0 {
        let details = json!({"file": filename, "language": "go", "nodes": node_count, "edges": edge_count});
        let details_str = details.to_string().replace('\'', "''");
        let _ = Spi::get_one::<pgrx::JsonB>(&format!(
            "SELECT kerai.mint_reward('parse_go_file', '{}'::jsonb)",
            details_str,
        ));
    }

    let elapsed = start.elapsed();
    pgrx::JsonB(json!({
        "file": filename,
        "language": "go",
        "nodes": node_count,
        "edges": edge_count,
        "elapsed_ms": elapsed.as_millis() as u64,
    }))
}

/// Parse Go source, insert nodes/edges, return counts.
///
/// `parent_id` allows parenting the file node under a repo directory node.
pub(crate) fn parse_go_single(
    source: &str,
    filename: &str,
    instance_id: &str,
    parent_id: Option<&str>,
) -> (usize, usize) {
    // 1. Normalize source
    let normalized = normalizer::normalize(source);

    // 2. Parse with tree-sitter
    let tree = match treesitter::parse(&normalized, TsLanguage::Go) {
        Some(t) => t,
        None => {
            warning!("Failed to parse Go source: {}", filename);
            return (0, 0);
        }
    };

    // 3. Create file node
    let file_node_id = Uuid::new_v4().to_string();
    let path_ctx = PathContext::with_root(filename);

    let file_node = NodeRow {
        id: file_node_id.clone(),
        instance_id: instance_id.to_string(),
        kind: Kind::File.as_str().to_string(),
        language: Some("go".to_string()),
        content: Some(filename.to_string()),
        parent_id: parent_id.map(|s| s.to_string()),
        position: 0,
        path: path_ctx.path(),
        metadata: json!({"line_count": normalized.lines().count()}),
        span_start: None,
        span_end: None,
    };
    inserter::insert_nodes(&[file_node]);

    // 4. Walk Go CST
    let (mut nodes, mut edges) =
        walker::walk_go_file(&tree, &normalized, &file_node_id, instance_id, path_ctx);

    // 4b. Normalize top-level positions to span_start (line numbers)
    for node in &mut nodes {
        if node.parent_id.as_deref() == Some(&file_node_id) {
            if let Some(start) = node.span_start {
                node.position = start;
            }
        }
    }

    // 5. Collect string literal exclusion zones from tree-sitter
    let exclusions = walker::collect_string_spans(&tree, &normalized);

    // 6. Extract comments with exclusion zones
    let raw_comments =
        crate::parser::comment_extractor::extract_comments(&normalized, &exclusions);

    // 7. Group consecutive line comments into blocks
    let mut blocks = crate::parser::comment_extractor::group_comments(raw_comments);

    // Filter out doc comments (Go doesn't have /// but filter anyway for consistency)
    blocks.retain(|b| !b.is_doc);

    // 8. Match comment blocks to AST nodes
    let matches = match_comments_to_ast(&mut blocks, &nodes);

    // 9. Create NodeRow + EdgeRow for each comment block
    for (block_idx, block) in blocks.iter().enumerate() {
        let comment_id = Uuid::new_v4().to_string();
        let kind = if !block.is_block_style && block.lines.len() > 1 {
            Kind::CommentBlock
        } else {
            Kind::Comment
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
            kind: kind.as_str().to_string(),
            language: Some("go".to_string()),
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
            edges.push(EdgeRow {
                id: Uuid::new_v4().to_string(),
                source_id: comment_id,
                target_id: target_id.clone(),
                relation: "documents".to_string(),
                metadata: json!({"placement": placement}),
            });
        }
    }

    // 10. Run Go suggestion rules
    let node_infos: Vec<suggestion_rules::GoNodeInfo> = nodes
        .iter()
        .filter(|n| n.parent_id.as_deref() == Some(&file_node_id))
        .map(|n| {
            let has_doc = edges.iter().any(|e| e.target_id == n.id && e.relation == "documents");
            suggestion_rules::GoNodeInfo {
                id: n.id.clone(),
                kind: n.kind.clone(),
                name: n.content.clone(),
                span_start: n.span_start,
                exported: n
                    .metadata
                    .get("exported")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
                has_doc,
                returns: n
                    .metadata
                    .get("returns")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
            }
        })
        .collect();

    // Get package name for stutter check
    let pkg_name = nodes
        .iter()
        .find(|n| n.kind == kinds::GO_PACKAGE)
        .and_then(|n| n.content.clone());

    let findings = suggestion_rules::run_go_rules(&node_infos, pkg_name.as_deref());

    for finding in &findings {
        let suggestion_id = Uuid::new_v4().to_string();

        nodes.push(NodeRow {
            id: suggestion_id.clone(),
            instance_id: instance_id.to_string(),
            kind: Kind::Suggestion.as_str().to_string(),
            language: Some("go".to_string()),
            content: Some(finding.message.clone()),
            parent_id: Some(file_node_id.clone()),
            position: finding.line,
            path: None,
            metadata: json!({
                "rule": finding.rule_id,
                "status": "emitted",
                "severity": finding.severity,
                "category": finding.category,
            }),
            span_start: Some(finding.line),
            span_end: Some(finding.line),
        });

        edges.push(EdgeRow {
            id: Uuid::new_v4().to_string(),
            source_id: suggestion_id,
            target_id: finding.target_node_id.clone(),
            relation: "suggests".to_string(),
            metadata: json!({"rule": finding.rule_id}),
        });
    }

    let node_count = nodes.len() + 1; // +1 for file node
    let edge_count = edges.len();

    inserter::insert_nodes(&nodes);
    inserter::insert_edges(&edges);

    (node_count, edge_count)
}

/// Match comment blocks to AST nodes and classify placement.
/// Reuses the same algorithm as the Rust parser.
fn match_comments_to_ast(
    blocks: &mut [CommentBlock],
    nodes: &[NodeRow],
) -> Vec<Option<String>> {
    let mut ast_spans: Vec<(i32, &str)> = nodes
        .iter()
        .filter(|n| {
            n.span_start.is_some()
                && n.kind != Kind::Comment.as_str()
                && n.kind != Kind::CommentBlock.as_str()
        })
        .map(|n| (n.span_start.unwrap(), n.id.as_str()))
        .collect();
    ast_spans.sort_by_key(|&(line, _)| line);

    let mut results = Vec::with_capacity(blocks.len());

    for block in blocks.iter_mut() {
        let start = block.start_line as i32;
        let end = block.end_line as i32;

        let next_node = ast_spans.iter().find(|&&(line, _)| line > end);

        let prev_node = ast_spans.iter().rev().find(|&&(line, _)| line < start);

        let same_line_node = if start == end {
            ast_spans.iter().find(|&&(line, _)| line == start)
        } else {
            None
        };

        if let Some(&(node_line, node_id)) = same_line_node {
            if node_line == start {
                block.placement = CommentPlacement::Trailing;
                results.push(Some(node_id.to_string()));
                continue;
            }
        }

        match next_node {
            Some(&(next_line, next_id)) => {
                if next_line == end + 1 {
                    block.placement = CommentPlacement::Above;
                    results.push(Some(next_id.to_string()));
                } else if prev_node.is_some() {
                    block.placement = CommentPlacement::Between;
                    results.push(Some(next_id.to_string()));
                } else {
                    block.placement = CommentPlacement::Above;
                    results.push(Some(next_id.to_string()));
                }
            }
            None => {
                block.placement = CommentPlacement::Eof;
                results.push(None);
            }
        }
    }

    results
}
