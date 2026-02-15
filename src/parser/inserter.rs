/// Batch SPI INSERT for nodes and edges.
use pgrx::prelude::*;

use super::ast_walker::{EdgeRow, NodeRow};

const BATCH_SIZE: usize = 500;

/// Escape a string for use in a SQL literal (double single quotes).
fn sql_escape(s: &str) -> String {
    s.replace('\'', "''")
}

/// Format an Option<String> as a SQL value.
fn sql_opt_str(val: &Option<String>) -> String {
    match val {
        Some(s) => format!("'{}'", sql_escape(s)),
        None => "NULL".to_string(),
    }
}

/// Format an Option<i32> as a SQL value.
fn sql_opt_int(val: Option<i32>) -> String {
    match val {
        Some(i) => i.to_string(),
        None => "NULL".to_string(),
    }
}

/// Delete all nodes (and edges via CASCADE) for a given file node.
/// Used for idempotent re-parse: delete old data, then re-insert.
pub fn delete_file_nodes(instance_id: &str, filename: &str) {
    // Delete edges where source or target is a child of this file
    Spi::run(&format!(
        "DELETE FROM kerai.edges WHERE source_id IN (
            SELECT id FROM kerai.nodes
            WHERE instance_id = '{}'::uuid
            AND kind = 'file' AND content = '{}'
        ) OR target_id IN (
            SELECT id FROM kerai.nodes
            WHERE instance_id = '{}'::uuid
            AND kind = 'file' AND content = '{}'
        )",
        sql_escape(instance_id),
        sql_escape(filename),
        sql_escape(instance_id),
        sql_escape(filename),
    ))
    .ok();

    // Delete child nodes (anything with parent_id pointing to file's subtree)
    // Use recursive CTE to find all descendants
    Spi::run(&format!(
        "WITH RECURSIVE descendants AS (
            SELECT id FROM kerai.nodes
            WHERE instance_id = '{}'::uuid
            AND kind = 'file' AND content = '{}'
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
            AND kind = 'file' AND content = '{}'
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

/// Insert nodes in batches.
pub fn insert_nodes(nodes: &[NodeRow]) {
    for batch in nodes.chunks(BATCH_SIZE) {
        let mut sql = String::from(
            "INSERT INTO kerai.nodes (id, instance_id, kind, language, content, parent_id, position, path, metadata) VALUES ",
        );

        for (i, node) in batch.iter().enumerate() {
            if i > 0 {
                sql.push_str(", ");
            }
            let metadata_str = sql_escape(&node.metadata.to_string());
            sql.push_str(&format!(
                "('{}'::uuid, '{}'::uuid, '{}', {}, {}, {}, {}, {}, '{}'::jsonb)",
                sql_escape(&node.id),
                sql_escape(&node.instance_id),
                sql_escape(&node.kind),
                sql_opt_str(&node.language),
                sql_opt_str(&node.content),
                match &node.parent_id {
                    Some(pid) => format!("'{}'::uuid", sql_escape(pid)),
                    None => "NULL".to_string(),
                },
                node.position,
                match &node.path {
                    Some(p) => format!("'{}'::ltree", sql_escape(p)),
                    None => "NULL".to_string(),
                },
                metadata_str,
            ));
        }

        Spi::run(&sql).expect("Failed to insert nodes batch");
    }
}

/// Insert edges in batches.
pub fn insert_edges(edges: &[EdgeRow]) {
    if edges.is_empty() {
        return;
    }

    for batch in edges.chunks(BATCH_SIZE) {
        let mut sql = String::from(
            "INSERT INTO kerai.edges (id, source_id, target_id, relation, metadata) VALUES ",
        );

        for (i, edge) in batch.iter().enumerate() {
            if i > 0 {
                sql.push_str(", ");
            }
            let metadata_str = sql_escape(&edge.metadata.to_string());
            sql.push_str(&format!(
                "('{}'::uuid, '{}'::uuid, '{}'::uuid, '{}', '{}'::jsonb)",
                sql_escape(&edge.id),
                sql_escape(&edge.source_id),
                sql_escape(&edge.target_id),
                sql_escape(&edge.relation),
                metadata_str,
            ));
        }

        sql.push_str(" ON CONFLICT (source_id, target_id, relation) DO NOTHING");

        Spi::run(&sql).expect("Failed to insert edges batch");
    }
}
