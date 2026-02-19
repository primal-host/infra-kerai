/// Walk the file tree at HEAD and produce nodes via parser dispatch or opaque storage.
use git2::Repository;
use serde_json::json;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::parser::ast_walker::NodeRow;
use crate::parser::inserter;

use super::kinds;
use super::language_detect::{classify, LanguageClass, ParseableLanguage};

/// Maximum size for storing opaque text source in metadata.
const OPAQUE_TEXT_MAX: usize = 100 * 1024; // 100 KB

/// Files larger than this are treated as binary regardless of extension.
const TEXT_SIZE_LIMIT: usize = 1024 * 1024; // 1 MB

/// Number of files to batch before flushing opaque nodes.
const FILE_BATCH: usize = 50;

/// Stats returned from a tree walk.
pub struct TreeWalkStats {
    pub files: usize,
    pub parsed: usize,
    pub opaque_text: usize,
    pub opaque_binary: usize,
    pub directories: usize,
}

/// Walk the file tree at HEAD. Dispatches parseable files to their respective parsers
/// and stores opaque files as metadata-only nodes.
pub fn walk_tree(
    repo: &Repository,
    repo_node_id: &str,
    instance_id: &str,
) -> Result<TreeWalkStats, String> {
    let head = repo
        .head()
        .map_err(|e| format!("no HEAD: {}", e))?;
    let tree = head
        .peel_to_tree()
        .map_err(|e| format!("HEAD has no tree: {}", e))?;

    let mut stats = TreeWalkStats {
        files: 0,
        parsed: 0,
        opaque_text: 0,
        opaque_binary: 0,
        directories: 0,
    };

    // Track directory nodes: path → node_id
    let mut dir_nodes: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();

    // Pending opaque nodes to batch-insert
    let mut pending_nodes: Vec<NodeRow> = Vec::new();

    // Walk the tree recursively
    tree.walk(git2::TreeWalkMode::PreOrder, |root, entry| {
        let name = match entry.name() {
            Some(n) => n,
            None => return git2::TreeWalkResult::Ok,
        };

        // Skip .git directory entries
        if name == ".git" {
            return git2::TreeWalkResult::Skip;
        }

        let full_path = if root.is_empty() {
            name.to_string()
        } else {
            format!("{}{}", root, name)
        };

        match entry.kind() {
            Some(git2::ObjectType::Tree) => {
                // Directory node
                let dir_id = Uuid::new_v4().to_string();

                // Find parent directory
                let parent_id = if root.is_empty() {
                    repo_node_id.to_string()
                } else {
                    let parent_path = root.trim_end_matches('/');
                    dir_nodes
                        .get(parent_path)
                        .cloned()
                        .unwrap_or_else(|| repo_node_id.to_string())
                };

                let dir_path_key = format!("{}{}", root, name);
                dir_nodes.insert(dir_path_key, dir_id.clone());

                pending_nodes.push(NodeRow {
                    id: dir_id,
                    instance_id: instance_id.to_string(),
                    kind: kinds::REPO_DIRECTORY.to_string(),
                    language: None,
                    content: Some(name.to_string()),
                    parent_id: Some(parent_id),
                    position: stats.directories as i32,
                    path: None,
                    metadata: json!({"path": full_path}),
                    span_start: None,
                    span_end: None,
                });
                stats.directories += 1;

                // Flush batch if needed
                if pending_nodes.len() >= FILE_BATCH {
                    inserter::insert_nodes(&pending_nodes);
                    pending_nodes.clear();
                }
            }
            Some(git2::ObjectType::Blob) => {
                stats.files += 1;

                // Find parent directory
                let parent_id = if root.is_empty() {
                    repo_node_id.to_string()
                } else {
                    let parent_path = root.trim_end_matches('/');
                    dir_nodes
                        .get(parent_path)
                        .cloned()
                        .unwrap_or_else(|| repo_node_id.to_string())
                };

                // Read blob content
                let blob = match repo.find_blob(entry.id()) {
                    Ok(b) => b,
                    Err(_) => return git2::TreeWalkResult::Ok,
                };
                let content = blob.content();
                let size = content.len();

                // Classify the file
                let sample = if size > 8192 { &content[..8192] } else { content };
                let class = classify(&full_path, Some(sample));

                match class {
                    LanguageClass::Parseable(lang) => {
                        // Flush pending opaque nodes before calling parser
                        // (parsers do their own SPI calls)
                        if !pending_nodes.is_empty() {
                            inserter::insert_nodes(&pending_nodes);
                            pending_nodes.clear();
                        }

                        // Try to read as UTF-8
                        if let Ok(source) = std::str::from_utf8(content) {
                            dispatch_parser(
                                lang,
                                source,
                                &full_path,
                                instance_id,
                                &parent_id,
                            );
                            stats.parsed += 1;
                        } else {
                            // Not valid UTF-8 — store as opaque binary
                            let hash = sha256_hex(content);
                            pending_nodes.push(make_binary_node(
                                &full_path,
                                name,
                                instance_id,
                                &parent_id,
                                size,
                                &hash,
                                stats.files as i32,
                            ));
                            stats.opaque_binary += 1;
                        }
                    }
                    LanguageClass::OpaqueText(lang) => {
                        if size > TEXT_SIZE_LIMIT {
                            // Too large for text — treat as binary
                            let hash = sha256_hex(content);
                            pending_nodes.push(make_binary_node(
                                &full_path,
                                name,
                                instance_id,
                                &parent_id,
                                size,
                                &hash,
                                stats.files as i32,
                            ));
                            stats.opaque_binary += 1;
                        } else if let Ok(source) = std::str::from_utf8(content) {
                            let truncated = size > OPAQUE_TEXT_MAX;
                            let stored_source = if truncated {
                                &source[..OPAQUE_TEXT_MAX]
                            } else {
                                source
                            };

                            let line_count = source.lines().count();

                            pending_nodes.push(NodeRow {
                                id: Uuid::new_v4().to_string(),
                                instance_id: instance_id.to_string(),
                                kind: kinds::REPO_OPAQUE_TEXT.to_string(),
                                language: Some(lang),
                                content: Some(name.to_string()),
                                parent_id: Some(parent_id),
                                position: stats.files as i32,
                                path: None,
                                metadata: json!({
                                    "path": full_path,
                                    "size": size,
                                    "line_count": line_count,
                                    "truncated": truncated,
                                    "source": stored_source,
                                }),
                                span_start: None,
                                span_end: None,
                            });
                            stats.opaque_text += 1;
                        } else {
                            // Not valid UTF-8 — binary fallback
                            let hash = sha256_hex(content);
                            pending_nodes.push(make_binary_node(
                                &full_path,
                                name,
                                instance_id,
                                &parent_id,
                                size,
                                &hash,
                                stats.files as i32,
                            ));
                            stats.opaque_binary += 1;
                        }
                    }
                    LanguageClass::Binary => {
                        let hash = sha256_hex(content);
                        pending_nodes.push(make_binary_node(
                            &full_path,
                            name,
                            instance_id,
                            &parent_id,
                            size,
                            &hash,
                            stats.files as i32,
                        ));
                        stats.opaque_binary += 1;
                    }
                }

                // Flush batch if needed
                if pending_nodes.len() >= FILE_BATCH {
                    inserter::insert_nodes(&pending_nodes);
                    pending_nodes.clear();
                }
            }
            _ => {}
        }

        git2::TreeWalkResult::Ok
    })
    .map_err(|e| format!("tree walk failed: {}", e))?;

    // Flush remaining nodes
    if !pending_nodes.is_empty() {
        inserter::insert_nodes(&pending_nodes);
    }

    Ok(stats)
}

/// Dispatch a parseable file to the appropriate language parser.
fn dispatch_parser(
    lang: ParseableLanguage,
    source: &str,
    filename: &str,
    instance_id: &str,
    parent_id: &str,
) {
    match lang {
        ParseableLanguage::Rust => {
            crate::parser::parse_single_file(
                source,
                filename,
                instance_id,
                Some(parent_id),
                filename,
                0,
            );
        }
        ParseableLanguage::Go => {
            crate::parser::go::parse_go_single(source, filename, instance_id, Some(parent_id));
        }
        ParseableLanguage::C => {
            crate::parser::c::parse_c_single(source, filename, instance_id, Some(parent_id));
        }
        ParseableLanguage::Markdown => {
            crate::parser::markdown::parse_markdown_single(
                source,
                filename,
                instance_id,
                Some(parent_id),
            );
        }
    }
}

/// Create a binary file node.
fn make_binary_node(
    full_path: &str,
    name: &str,
    instance_id: &str,
    parent_id: &str,
    size: usize,
    sha256: &str,
    position: i32,
) -> NodeRow {
    NodeRow {
        id: Uuid::new_v4().to_string(),
        instance_id: instance_id.to_string(),
        kind: kinds::REPO_OPAQUE_BINARY.to_string(),
        language: None,
        content: Some(name.to_string()),
        parent_id: Some(parent_id.to_string()),
        position,
        path: None,
        metadata: json!({
            "path": full_path,
            "size": size,
            "sha256": sha256,
        }),
        span_start: None,
        span_end: None,
    }
}

/// Compute SHA-256 hex digest.
fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}

/// Walk the tree for incremental updates: only process files changed between
/// old_tree and new_tree.
pub fn walk_tree_incremental(
    repo: &Repository,
    repo_node_id: &str,
    instance_id: &str,
    old_head: &str,
) -> Result<TreeWalkStats, String> {
    let old_commit = repo
        .find_commit(
            git2::Oid::from_str(old_head)
                .map_err(|e| format!("invalid old HEAD: {}", e))?,
        )
        .map_err(|e| format!("find old commit: {}", e))?;
    let old_tree = old_commit
        .tree()
        .map_err(|e| format!("old commit tree: {}", e))?;

    let head = repo
        .head()
        .map_err(|e| format!("no HEAD: {}", e))?;
    let new_tree = head
        .peel_to_tree()
        .map_err(|e| format!("HEAD has no tree: {}", e))?;

    let diff = repo
        .diff_tree_to_tree(Some(&old_tree), Some(&new_tree), None)
        .map_err(|e| format!("diff failed: {}", e))?;

    let mut stats = TreeWalkStats {
        files: 0,
        parsed: 0,
        opaque_text: 0,
        opaque_binary: 0,
        directories: 0,
    };

    let mut pending_nodes: Vec<NodeRow> = Vec::new();

    // Collect changed file paths
    let deltas: Vec<_> = (0..diff.deltas().len())
        .filter_map(|i| diff.get_delta(i))
        .collect();

    for delta in &deltas {
        let new_file = delta.new_file();
        let path = match new_file.path() {
            Some(p) => p.to_string_lossy().to_string(),
            None => continue,
        };
        let name = path.rsplit('/').next().unwrap_or(&path).to_string();

        match delta.status() {
            git2::Delta::Deleted => {
                // Delete nodes for this file path
                delete_file_by_path(instance_id, &path);
            }
            git2::Delta::Added | git2::Delta::Modified => {
                // Delete old nodes if modified
                if delta.status() == git2::Delta::Modified {
                    delete_file_by_path(instance_id, &path);
                }

                // Read blob
                let blob = match repo.find_blob(new_file.id()) {
                    Ok(b) => b,
                    Err(_) => continue,
                };
                let content = blob.content();
                let size = content.len();

                let sample = if size > 8192 { &content[..8192] } else { content };
                let class = classify(&path, Some(sample));

                stats.files += 1;

                match class {
                    LanguageClass::Parseable(lang) => {
                        if !pending_nodes.is_empty() {
                            inserter::insert_nodes(&pending_nodes);
                            pending_nodes.clear();
                        }

                        if let Ok(source) = std::str::from_utf8(content) {
                            dispatch_parser(lang, source, &path, instance_id, repo_node_id);
                            stats.parsed += 1;
                        } else {
                            let hash = sha256_hex(content);
                            pending_nodes.push(make_binary_node(
                                &path,
                                &name,
                                instance_id,
                                repo_node_id,
                                size,
                                &hash,
                                stats.files as i32,
                            ));
                            stats.opaque_binary += 1;
                        }
                    }
                    LanguageClass::OpaqueText(lang) => {
                        if size > TEXT_SIZE_LIMIT {
                            let hash = sha256_hex(content);
                            pending_nodes.push(make_binary_node(
                                &path,
                                &name,
                                instance_id,
                                repo_node_id,
                                size,
                                &hash,
                                stats.files as i32,
                            ));
                            stats.opaque_binary += 1;
                        } else if let Ok(source) = std::str::from_utf8(content) {
                            let truncated = size > OPAQUE_TEXT_MAX;
                            let stored = if truncated {
                                &source[..OPAQUE_TEXT_MAX]
                            } else {
                                source
                            };

                            pending_nodes.push(NodeRow {
                                id: Uuid::new_v4().to_string(),
                                instance_id: instance_id.to_string(),
                                kind: kinds::REPO_OPAQUE_TEXT.to_string(),
                                language: Some(lang),
                                content: Some(name.clone()),
                                parent_id: Some(repo_node_id.to_string()),
                                position: stats.files as i32,
                                path: None,
                                metadata: json!({
                                    "path": path,
                                    "size": size,
                                    "line_count": source.lines().count(),
                                    "truncated": truncated,
                                    "source": stored,
                                }),
                                span_start: None,
                                span_end: None,
                            });
                            stats.opaque_text += 1;
                        } else {
                            let hash = sha256_hex(content);
                            pending_nodes.push(make_binary_node(
                                &path,
                                &name,
                                instance_id,
                                repo_node_id,
                                size,
                                &hash,
                                stats.files as i32,
                            ));
                            stats.opaque_binary += 1;
                        }
                    }
                    LanguageClass::Binary => {
                        let hash = sha256_hex(content);
                        pending_nodes.push(make_binary_node(
                            &path,
                            &name,
                            instance_id,
                            repo_node_id,
                            size,
                            &hash,
                            stats.files as i32,
                        ));
                        stats.opaque_binary += 1;
                    }
                }

                if pending_nodes.len() >= FILE_BATCH {
                    inserter::insert_nodes(&pending_nodes);
                    pending_nodes.clear();
                }
            }
            _ => {}
        }
    }

    if !pending_nodes.is_empty() {
        inserter::insert_nodes(&pending_nodes);
    }

    Ok(stats)
}

/// Delete nodes for a file identified by its path in metadata.
fn delete_file_by_path(instance_id: &str, path: &str) {
    use crate::sql::{sql_escape, sql_uuid};
    use pgrx::prelude::*;

    let inst = sql_uuid(instance_id);
    let escaped_path = sql_escape(path);

    // Delete edges then nodes via recursive CTE
    Spi::run(&format!(
        "WITH RECURSIVE descendants AS (
            SELECT id FROM kerai.nodes
            WHERE instance_id = {inst}
            AND (
                (kind = 'file' AND content = '{escaped_path}')
                OR (kind IN ('repo_opaque_text', 'repo_opaque_binary') AND content = '{name}' AND metadata->>'path' = '{escaped_path}')
            )
            UNION ALL
            SELECT n.id FROM kerai.nodes n
            JOIN descendants d ON n.parent_id = d.id
        )
        DELETE FROM kerai.edges WHERE source_id IN (SELECT id FROM descendants)
            OR target_id IN (SELECT id FROM descendants)",
        name = sql_escape(path.rsplit('/').next().unwrap_or(path)),
    ))
    .ok();

    Spi::run(&format!(
        "WITH RECURSIVE descendants AS (
            SELECT id FROM kerai.nodes
            WHERE instance_id = {inst}
            AND (
                (kind = 'file' AND content = '{escaped_path}')
                OR (kind IN ('repo_opaque_text', 'repo_opaque_binary') AND content = '{name}' AND metadata->>'path' = '{escaped_path}')
            )
            UNION ALL
            SELECT n.id FROM kerai.nodes n
            JOIN descendants d ON n.parent_id = d.id
        )
        DELETE FROM kerai.nodes WHERE id IN (SELECT id FROM descendants)",
        name = sql_escape(path.rsplit('/').next().unwrap_or(path)),
    ))
    .ok();
}
