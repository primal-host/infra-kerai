/// Parser module — Rust source → kerai.nodes + kerai.edges.
use pgrx::prelude::*;
use serde_json::json;
use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;
use uuid::Uuid;

mod ast_walker;
mod cargo_parser;
#[allow(dead_code)]
mod comment_extractor;
mod crate_walker;
mod flag_parser;
#[allow(dead_code)]
mod inserter;
pub mod kinds;
#[allow(dead_code)]
mod metadata;
mod normalizer;
#[allow(dead_code)]
mod path_builder;
pub mod markdown;
mod suggestion_rules;

use ast_walker::NodeRow;
use comment_extractor::{CommentBlock, CommentPlacement};
use kinds::Kind;
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

    // 1b. Parse kerai directives (flags + suggestion acknowledgments)
    let directives = flag_parser::parse_kerai_directives(&normalized);
    let kerai_flags = flag_parser::build_flags_metadata(&directives);

    // Collect suggestion comments from previous reconstruction cycle
    let prev_suggestions: Vec<_> = directives
        .iter()
        .filter_map(|d| {
            if let flag_parser::KeraiDirective::SuggestionComment {
                rule_id,
                message: _,
                line,
            } = d
            {
                Some((rule_id.clone(), *line))
            } else {
                None
            }
        })
        .collect();

    // 2. Parse with syn
    let syn_file = match syn::parse_file(&normalized) {
        Ok(f) => f,
        Err(e) => {
            warning!("Failed to parse {}: {}", filename, e);
            return (0, 0);
        }
    };

    // 3. Create file node (with kerai_flags if present)
    let file_node_id = Uuid::new_v4().to_string();
    let path_ctx = PathContext::with_root(path_root);

    let mut file_metadata = json!({"line_count": normalized.lines().count()});
    if let Some(ref flags) = kerai_flags {
        file_metadata
            .as_object_mut()
            .unwrap()
            .insert("kerai_flags".to_string(), flags.clone());
    }

    let file_node = NodeRow {
        id: file_node_id.clone(),
        instance_id: instance_id.to_string(),
        kind: Kind::File.as_str().to_string(),
        language: Some("rust".to_string()),
        content: Some(filename.to_string()),
        parent_id: parent_id.map(|s| s.to_string()),
        position,
        path: path_ctx.path(),
        metadata: file_metadata,
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

    // Also filter out kerai directive comments (flags and suggestion acks)
    blocks.retain(|b| {
        let first_line = b.lines.first().map(|l| l.as_str()).unwrap_or("");
        !first_line.starts_with(" kerai:")
            && !first_line.starts_with("kerai:")
    });

    // 8. Match comment blocks to AST nodes (sets placement)
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

    // 10. Run suggestion rules
    let skip_suggestions = kerai_flags
        .as_ref()
        .and_then(|f| f.get("skip-suggestions").and_then(|v| v.as_bool()))
        .unwrap_or(false)
        || kerai_flags
            .as_ref()
            .and_then(|f| f.get("skip").and_then(|v| v.as_bool()))
            .unwrap_or(false);

    if !skip_suggestions {
        // Build NodeInfo for suggestion rules from AST-walked nodes
        let node_infos: Vec<suggestion_rules::NodeInfo> = nodes
            .iter()
            .filter(|n| n.parent_id.as_deref() == Some(&file_node_id))
            .map(|n| {
                let name = n
                    .metadata
                    .get("name")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .or_else(|| {
                        // For nodes that store name in content (fns, structs, etc.)
                        if matches!(
                            n.kind.as_str(),
                            "fn" | "struct" | "enum" | "trait" | "union" | "type_alias"
                                | "const" | "static"
                        ) {
                            n.content.clone()
                        } else {
                            None
                        }
                    });
                let source = n
                    .metadata
                    .get("source")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                suggestion_rules::NodeInfo {
                    id: n.id.clone(),
                    kind: n.kind.clone(),
                    name,
                    span_start: n.span_start,
                    content: n.content.clone(),
                    source,
                }
            })
            .collect();

        let findings = suggestion_rules::run_rules(&syn_file, &node_infos);

        // Check which suggestions were previously dismissed
        let dismissed = query_dismissed_suggestions(&file_node_id, instance_id);

        // Track which previous suggestion comments are still present
        let prev_rule_lines: HashMap<String, usize> = prev_suggestions
            .iter()
            .map(|(rule_id, line)| (rule_id.clone(), *line))
            .collect();

        for finding in &findings {
            // Skip if this rule was previously dismissed for this target
            let dismiss_key = format!("{}:{}", finding.rule_id, finding.target_node_id);
            if dismissed.contains(&dismiss_key) {
                // Check if the code has changed (target_hash comparison)
                // For now, simple dismissal: if dismissed, skip
                continue;
            }

            // Skip if the suggestion comment is still present in the source
            // (it hasn't been reviewed yet)
            if prev_rule_lines.contains_key(finding.rule_id) {
                continue;
            }

            let suggestion_id = Uuid::new_v4().to_string();
            let content_hash = simple_hash(&finding.target_node_id);

            nodes.push(NodeRow {
                id: suggestion_id.clone(),
                instance_id: instance_id.to_string(),
                kind: Kind::Suggestion.as_str().to_string(),
                language: Some("rust".to_string()),
                content: Some(finding.message.clone()),
                parent_id: Some(file_node_id.clone()),
                position: finding.line,
                path: None,
                metadata: json!({
                    "rule": finding.rule_id,
                    "status": "emitted",
                    "target_hash": content_hash,
                    "severity": finding.severity,
                    "category": finding.category,
                }),
                span_start: Some(finding.line),
                span_end: Some(finding.line),
            });

            edges.push(ast_walker::EdgeRow {
                id: Uuid::new_v4().to_string(),
                source_id: suggestion_id,
                target_id: finding.target_node_id.clone(),
                relation: "suggests".to_string(),
                metadata: json!({"rule": finding.rule_id}),
            });
        }

        // Update status of previous suggestions based on what we found in the source
        update_suggestion_statuses(&prev_suggestions, &findings, &file_node_id);
    }

    let node_count = nodes.len() + 1; // +1 for file node
    let edge_count = edges.len();

    inserter::insert_nodes(&nodes);
    inserter::insert_edges(&edges);

    (node_count, edge_count)
}

/// Query previously dismissed suggestion rule+target pairs for a file.
fn query_dismissed_suggestions(file_node_id: &str, _instance_id: &str) -> std::collections::HashSet<String> {
    let mut dismissed = std::collections::HashSet::new();

    Spi::connect(|client| {
        let query = format!(
            "SELECT n.metadata->>'rule' AS rule, \
             e.target_id::text AS target_id \
             FROM kerai.nodes n \
             JOIN kerai.edges e ON e.source_id = n.id \
             WHERE n.parent_id = '{}'::uuid \
             AND n.kind = 'suggestion' \
             AND n.metadata->>'status' = 'dismissed' \
             AND e.relation = 'suggests'",
            file_node_id.replace('\'', "''")
        );

        let result = client.select(&query, None, &[]).unwrap();
        for row in result {
            let rule: String = row.get_by_name::<String, _>("rule").unwrap().unwrap_or_default();
            let target: String = row.get_by_name::<String, _>("target_id").unwrap().unwrap_or_default();
            dismissed.insert(format!("{}:{}", rule, target));
        }
    });

    dismissed
}

/// Update suggestion node statuses based on what we found during re-parse.
///
/// If a `// kerai:` suggestion comment was removed:
/// - And the code changed → mark as "applied"
/// - And the code didn't change → mark as "dismissed"
fn update_suggestion_statuses(
    _prev_suggestions: &[(String, usize)],
    _findings: &[suggestion_rules::Finding],
    file_node_id: &str,
) {
    // Query existing "emitted" suggestions for this file
    let mut emitted_suggestions: Vec<(String, String)> = Vec::new(); // (node_id, rule_id)

    Spi::connect(|client| {
        let query = format!(
            "SELECT id::text, metadata->>'rule' AS rule \
             FROM kerai.nodes \
             WHERE parent_id = '{}'::uuid \
             AND kind = 'suggestion' \
             AND metadata->>'status' = 'emitted'",
            file_node_id.replace('\'', "''")
        );

        let result = client.select(&query, None, &[]).unwrap();
        for row in result {
            let id: String = row
                .get_by_name::<String, _>("id")
                .unwrap()
                .unwrap_or_default();
            let rule: String = row
                .get_by_name::<String, _>("rule")
                .unwrap()
                .unwrap_or_default();
            emitted_suggestions.push((id, rule));
        }
    });

    // For each previously emitted suggestion, check if the comment is still present
    for (suggestion_id, rule_id) in &emitted_suggestions {
        let comment_still_present = _prev_suggestions
            .iter()
            .any(|(r, _)| r == rule_id);

        if !comment_still_present {
            // The suggestion comment was removed — mark as dismissed
            // (In a more sophisticated implementation, we'd check if the code changed
            // to distinguish "dismissed" from "applied")
            Spi::run(&format!(
                "UPDATE kerai.nodes SET metadata = jsonb_set(metadata, '{{status}}', '\"dismissed\"') \
                 WHERE id = '{}'::uuid",
                suggestion_id.replace('\'', "''")
            ))
            .ok();
        }
    }
}

/// Simple hash for content comparison.
fn simple_hash(s: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    s.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
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
