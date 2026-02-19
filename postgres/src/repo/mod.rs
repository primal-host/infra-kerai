/// Repository ingestion module — clone, parse, and store git repositories.
use pgrx::prelude::*;
use serde_json::json;
use std::path::Path;
use std::time::Instant;
use uuid::Uuid;

use crate::parser::ast_walker::NodeRow;
use crate::parser::inserter;
use crate::sql::{sql_escape, sql_opt_text, sql_text, sql_uuid};

mod census;
mod cloner;
mod commit_walker;
pub mod kinds;
mod language_detect;
mod tree_walker;

/// Get the self instance ID from the database.
fn get_self_instance_id() -> String {
    Spi::get_one::<String>("SELECT id::text FROM kerai.instances WHERE is_self = true")
        .expect("Failed to query self instance")
        .expect("No self instance found — run kerai.bootstrap_instance() first")
}

/// Mirror a git repository: clone (or fetch), walk commits, parse files.
///
/// Returns JSON with stats: `{repo, url, commits, files, parsed, opaque_text, opaque_binary, elapsed_ms}`.
#[pg_extern]
fn mirror_repo(url: &str) -> pgrx::JsonB {
    mirror_repo_inner(url, None)
}

/// Mirror a git repository at a specific branch or tag.
///
/// Returns JSON with stats.
#[pg_extern]
fn mirror_repo_at(url: &str, refspec: &str) -> pgrx::JsonB {
    mirror_repo_inner(url, Some(refspec))
}

/// Inner implementation for mirror_repo and mirror_repo_at.
fn mirror_repo_inner(url: &str, _refspec: Option<&str>) -> pgrx::JsonB {
    let start = Instant::now();
    let instance_id = get_self_instance_id();

    // Check for existing repository entry
    let existing = lookup_repo(&instance_id, url);

    match existing {
        Some((repo_id, local_path, old_head, repo_node_id)) => {
            // Existing repo — fetch and check for changes
            let repo = cloner::open_repo(Path::new(&local_path))
                .unwrap_or_else(|e| pgrx::error!("Failed to open repo: {}", e));

            cloner::fetch_repo(&repo)
                .unwrap_or_else(|e| pgrx::error!("Failed to fetch: {}", e));

            let new_head = cloner::head_sha(&repo)
                .unwrap_or_else(|e| pgrx::error!("Failed to get HEAD: {}", e));

            if Some(new_head.as_str()) == old_head.as_deref() {
                // No changes
                let elapsed = start.elapsed();
                return pgrx::JsonB(json!({
                    "status": "up_to_date",
                    "repo": repo_id,
                    "url": url,
                    "head": new_head,
                    "elapsed_ms": elapsed.as_millis() as u64,
                }));
            }

            // Incremental update: walk new commits
            let (commit_count, _oid_map) =
                commit_walker::walk_commits(&repo, &repo_node_id, &instance_id, old_head.as_deref())
                    .unwrap_or_else(|e| pgrx::error!("Commit walk failed: {}", e));

            // Incremental file update
            let tree_stats = if let Some(ref old) = old_head {
                tree_walker::walk_tree_incremental(&repo, &repo_node_id, &instance_id, old)
                    .unwrap_or_else(|e| pgrx::error!("Incremental tree walk failed: {}", e))
            } else {
                tree_walker::walk_tree(&repo, &repo_node_id, &instance_id)
                    .unwrap_or_else(|e| pgrx::error!("Tree walk failed: {}", e))
            };

            // Update repository record
            update_repo_head(&repo_id, &new_head);

            // Mint reward
            mint_mirror_reward(&instance_id, url, commit_count, &tree_stats);

            let elapsed = start.elapsed();
            pgrx::JsonB(json!({
                "status": "updated",
                "repo": repo_id,
                "url": url,
                "head": new_head,
                "commits": commit_count,
                "files": tree_stats.files,
                "parsed": tree_stats.parsed,
                "opaque_text": tree_stats.opaque_text,
                "opaque_binary": tree_stats.opaque_binary,
                "elapsed_ms": elapsed.as_millis() as u64,
            }))
        }
        None => {
            // New repo — clone
            let name = cloner::repo_name_from_url(url);
            let short_id = &Uuid::new_v4().to_string()[..8];
            let dest = cloner::clone_path(&name, short_id);

            // Ensure parent directory exists
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent).ok();
            }

            let repo = cloner::clone_repo(url, &dest)
                .unwrap_or_else(|e| pgrx::error!("Clone failed: {}", e));

            let head_sha = cloner::head_sha(&repo)
                .unwrap_or_else(|e| pgrx::error!("Failed to get HEAD: {}", e));

            // Create repo root node
            let repo_node_id = Uuid::new_v4().to_string();
            let repo_node = NodeRow {
                id: repo_node_id.clone(),
                instance_id: instance_id.clone(),
                kind: kinds::REPO_REPOSITORY.to_string(),
                language: None,
                content: Some(name.clone()),
                parent_id: None,
                position: 0,
                path: None,
                metadata: json!({
                    "url": url,
                    "head": head_sha,
                }),
                span_start: None,
                span_end: None,
            };
            inserter::insert_nodes(&[repo_node]);

            // Insert repository record
            let repo_id = insert_repo_record(
                &instance_id,
                url,
                &name,
                &dest.to_string_lossy(),
                &head_sha,
                &repo_node_id,
            );

            // Walk commit graph
            let (commit_count, _oid_map) =
                commit_walker::walk_commits(&repo, &repo_node_id, &instance_id, None)
                    .unwrap_or_else(|e| pgrx::error!("Commit walk failed: {}", e));

            // Walk file tree
            let tree_stats = tree_walker::walk_tree(&repo, &repo_node_id, &instance_id)
                .unwrap_or_else(|e| pgrx::error!("Tree walk failed: {}", e));

            // Mint reward
            mint_mirror_reward(&instance_id, url, commit_count, &tree_stats);

            let elapsed = start.elapsed();
            pgrx::JsonB(json!({
                "status": "cloned",
                "repo": repo_id,
                "url": url,
                "head": head_sha,
                "commits": commit_count,
                "files": tree_stats.files,
                "parsed": tree_stats.parsed,
                "opaque_text": tree_stats.opaque_text,
                "opaque_binary": tree_stats.opaque_binary,
                "directories": tree_stats.directories,
                "elapsed_ms": elapsed.as_millis() as u64,
            }))
        }
    }
}

/// Language census for a repository.
///
/// Returns JSON: `{repo_id, total_files, total_lines, languages: {...}}`.
#[pg_extern]
fn repo_census(repo_id: pgrx::Uuid) -> pgrx::JsonB {
    let repo_id_str = repo_id.to_string();

    // Look up node_id from repositories table
    let node_id = Spi::get_one::<String>(&format!(
        "SELECT node_id::text FROM kerai.repositories WHERE id = {}",
        sql_uuid(&repo_id_str),
    ))
    .expect("Failed to query repository")
    .unwrap_or_else(|| pgrx::error!("Repository not found: {}", repo_id_str));

    pgrx::JsonB(census::repo_census(&node_id))
}

/// List all mirrored repositories.
///
/// Returns JSON array of repository records.
#[pg_extern]
fn list_repos() -> pgrx::JsonB {
    let mut repos = Vec::new();

    Spi::connect(|client| {
        let result = client
            .select(
                "SELECT id::text, url, name, head_commit, last_sync::text, \
                 node_id::text, metadata, created_at::text \
                 FROM kerai.repositories ORDER BY created_at DESC",
                None,
                &[],
            )
            .unwrap();

        for row in result {
            let id: String = row.get_by_name("id").unwrap().unwrap_or_default();
            let url: String = row.get_by_name("url").unwrap().unwrap_or_default();
            let name: String = row.get_by_name("name").unwrap().unwrap_or_default();
            let head: Option<String> = row.get_by_name("head_commit").unwrap();
            let last_sync: Option<String> = row.get_by_name("last_sync").unwrap();
            let node_id: Option<String> = row.get_by_name("node_id").unwrap();
            let created: Option<String> = row.get_by_name("created_at").unwrap();

            repos.push(json!({
                "id": id,
                "url": url,
                "name": name,
                "head_commit": head,
                "last_sync": last_sync,
                "node_id": node_id,
                "created_at": created,
            }));
        }
    });

    pgrx::JsonB(json!(repos))
}

/// Drop a mirrored repository: delete all nodes, edges, the repository record,
/// and the local clone directory.
///
/// Returns JSON: `{dropped: true, repo_id, nodes_deleted}`.
#[pg_extern]
fn drop_repo(repo_id: pgrx::Uuid) -> pgrx::JsonB {
    let repo_id_str = repo_id.to_string();

    // Look up repository
    let (node_id, local_path) = Spi::connect(|client| {
        let query = format!(
            "SELECT node_id::text, local_path FROM kerai.repositories WHERE id = {}",
            sql_uuid(&repo_id_str),
        );
        let result = client.select(&query, None, &[]).unwrap();
        let mut node_id = None;
        let mut local_path = None;
        for row in result {
            node_id = row.get_by_name::<String, _>("node_id").unwrap();
            local_path = row.get_by_name::<String, _>("local_path").unwrap();
        }
        (node_id, local_path)
    });

    let node_id = node_id.unwrap_or_else(|| pgrx::error!("Repository not found: {}", repo_id_str));

    // Delete repository record first (FK references node_id)
    let n_id = sql_uuid(&node_id);
    Spi::run(&format!(
        "DELETE FROM kerai.repositories WHERE id = {}",
        sql_uuid(&repo_id_str),
    ))
    .ok();

    // Delete all edges under the repo root via recursive CTE
    Spi::run(&format!(
        "WITH RECURSIVE descendants AS (
            SELECT id FROM kerai.nodes WHERE id = {n_id}
            UNION ALL
            SELECT n.id FROM kerai.nodes n
            JOIN descendants d ON n.parent_id = d.id
        )
        DELETE FROM kerai.edges WHERE source_id IN (SELECT id FROM descendants)
            OR target_id IN (SELECT id FROM descendants)",
    ))
    .ok();

    // Delete all nodes under the repo root
    let deleted = Spi::get_one::<i64>(&format!(
        "WITH RECURSIVE descendants AS (
            SELECT id FROM kerai.nodes WHERE id = {n_id}
            UNION ALL
            SELECT n.id FROM kerai.nodes n
            JOIN descendants d ON n.parent_id = d.id
        ),
        deleted AS (
            DELETE FROM kerai.nodes WHERE id IN (SELECT id FROM descendants) RETURNING 1
        )
        SELECT count(*)::bigint FROM deleted",
    ))
    .unwrap()
    .unwrap_or(0);

    // Remove local clone directory
    if let Some(path) = local_path {
        std::fs::remove_dir_all(Path::new(&path)).ok();
    }

    pgrx::JsonB(json!({
        "dropped": true,
        "repo_id": repo_id_str,
        "nodes_deleted": deleted,
    }))
}

// --- Helper functions ---

/// Look up an existing repository by URL.
/// Returns (repo_id, local_path, head_commit, node_id).
fn lookup_repo(
    instance_id: &str,
    url: &str,
) -> Option<(String, String, Option<String>, String)> {
    let mut result = None;

    Spi::connect(|client| {
        let query = format!(
            "SELECT id::text, local_path, head_commit, node_id::text \
             FROM kerai.repositories \
             WHERE instance_id = {} AND url = {}",
            sql_uuid(instance_id),
            sql_text(url),
        );

        let rows = client.select(&query, None, &[]).unwrap();
        for row in rows {
            let id: String = row.get_by_name("id").unwrap().unwrap_or_default();
            let path: String = row.get_by_name("local_path").unwrap().unwrap_or_default();
            let head: Option<String> = row.get_by_name("head_commit").unwrap();
            let node_id: String = row.get_by_name("node_id").unwrap().unwrap_or_default();
            result = Some((id, path, head, node_id));
        }
    });

    result
}

/// Insert a new repository record. Returns the generated UUID.
fn insert_repo_record(
    instance_id: &str,
    url: &str,
    name: &str,
    local_path: &str,
    head_commit: &str,
    node_id: &str,
) -> String {
    let id = Uuid::new_v4().to_string();

    Spi::run(&format!(
        "INSERT INTO kerai.repositories (id, instance_id, url, name, local_path, head_commit, last_sync, node_id) \
         VALUES ({}, {}, {}, {}, {}, {}, now(), {})",
        sql_uuid(&id),
        sql_uuid(instance_id),
        sql_text(url),
        sql_text(name),
        sql_text(local_path),
        sql_opt_text(&Some(head_commit.to_string())),
        sql_uuid(node_id),
    ))
    .expect("Failed to insert repository record");

    id
}

/// Update a repository's head_commit and last_sync.
fn update_repo_head(repo_id: &str, new_head: &str) {
    Spi::run(&format!(
        "UPDATE kerai.repositories SET head_commit = {}, last_sync = now() WHERE id = {}",
        sql_text(new_head),
        sql_uuid(repo_id),
    ))
    .ok();
}

/// Mint a reward for mirror_repo work.
fn mint_mirror_reward(
    _instance_id: &str,
    url: &str,
    commits: usize,
    stats: &tree_walker::TreeWalkStats,
) {
    let details = json!({
        "url": url,
        "commits": commits,
        "files": stats.files,
        "parsed": stats.parsed,
        "opaque_text": stats.opaque_text,
        "opaque_binary": stats.opaque_binary,
    });
    let details_str = sql_escape(&details.to_string());
    let _ = Spi::get_one::<pgrx::JsonB>(&format!(
        "SELECT kerai.mint_reward('mirror_repo', '{}'::jsonb)",
        details_str,
    ));
}
