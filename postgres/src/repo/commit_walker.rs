/// Walk the commit graph of a repository and produce NodeRow/EdgeRow vectors.
use git2::{Oid, Repository, Sort};
use serde_json::json;
use uuid::Uuid;

use crate::parser::ast_walker::{EdgeRow, NodeRow};
use crate::parser::inserter;

use super::kinds;

const COMMIT_BATCH: usize = 1000;

/// Walk commits from HEAD back to `stop_at` (exclusive).
/// If `stop_at` is None, walks the entire history.
///
/// Returns (commit_count, commit_node_ids) where commit_node_ids maps
/// git OID → kerai node UUID for edge creation.
pub fn walk_commits(
    repo: &Repository,
    repo_node_id: &str,
    instance_id: &str,
    stop_at: Option<&str>,
) -> Result<(usize, std::collections::HashMap<String, String>), String> {
    let mut revwalk = repo
        .revwalk()
        .map_err(|e| format!("revwalk init failed: {}", e))?;
    revwalk.set_sorting(Sort::TOPOLOGICAL | Sort::TIME).ok();
    revwalk
        .push_head()
        .map_err(|e| format!("push_head failed: {}", e))?;

    let stop_oid = stop_at.and_then(|s| Oid::from_str(s).ok());

    let mut nodes: Vec<NodeRow> = Vec::new();
    let mut edges: Vec<EdgeRow> = Vec::new();
    let mut oid_to_node: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let mut count = 0usize;

    for oid_result in revwalk {
        let oid = oid_result.map_err(|e| format!("revwalk error: {}", e))?;

        // Stop if we've reached the previous HEAD
        if stop_oid == Some(oid) {
            break;
        }

        let commit = repo
            .find_commit(oid)
            .map_err(|e| format!("find_commit failed: {}", e))?;

        let node_id = Uuid::new_v4().to_string();
        let sha = oid.to_string();
        oid_to_node.insert(sha.clone(), node_id.clone());

        let author = commit.author();
        let author_name = author.name().unwrap_or("unknown").to_string();
        let author_email = author.email().unwrap_or("").to_string();
        let message = commit.message().unwrap_or("").to_string();
        let time = commit.time();

        nodes.push(NodeRow {
            id: node_id.clone(),
            instance_id: instance_id.to_string(),
            kind: kinds::REPO_COMMIT.to_string(),
            language: None,
            content: Some(message.lines().next().unwrap_or("").to_string()),
            parent_id: Some(repo_node_id.to_string()),
            position: count as i32,
            path: None,
            metadata: json!({
                "sha": sha,
                "author_name": author_name,
                "author_email": author_email,
                "message": message,
                "timestamp": time.seconds(),
                "parent_count": commit.parent_count(),
            }),
            span_start: None,
            span_end: None,
        });

        // Parent commit edges (deferred — parent may not have a node yet)
        for parent_id in 0..commit.parent_count() {
            if let Ok(parent) = commit.parent(parent_id) {
                let parent_sha = parent.id().to_string();
                edges.push(EdgeRow {
                    id: Uuid::new_v4().to_string(),
                    source_id: node_id.clone(),
                    target_id: parent_sha, // placeholder — resolved below
                    relation: "parent_commit".to_string(),
                    metadata: json!({}),
                });
            }
        }

        count += 1;

        // Batch insert
        if nodes.len() >= COMMIT_BATCH {
            inserter::insert_nodes(&nodes);
            nodes.clear();
        }
    }

    // Flush remaining nodes
    if !nodes.is_empty() {
        inserter::insert_nodes(&nodes);
    }

    // Resolve parent_commit edges: replace SHA placeholders with node UUIDs
    let resolved_edges: Vec<EdgeRow> = edges
        .into_iter()
        .filter_map(|mut edge| {
            if let Some(parent_node_id) = oid_to_node.get(&edge.target_id) {
                edge.target_id = parent_node_id.clone();
                Some(edge)
            } else {
                None // parent commit not in our walk range
            }
        })
        .collect();

    if !resolved_edges.is_empty() {
        for batch in resolved_edges.chunks(COMMIT_BATCH) {
            inserter::insert_edges(batch);
        }
    }

    Ok((count, oid_to_node))
}
