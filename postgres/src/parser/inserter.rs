/// Batch SPI INSERT for nodes and edges.
use pgrx::prelude::*;

use super::ast_walker::{EdgeRow, NodeRow};
use crate::sql::{sql_escape, sql_jsonb, sql_ltree, sql_opt_text, sql_uuid};

const BATCH_SIZE: usize = 500;

/// Delete all nodes (and edges via CASCADE) for a given file node.
/// Used for idempotent re-parse: delete old data, then re-insert.
pub fn delete_file_nodes(instance_id: &str, filename: &str) {
    let inst = sql_uuid(instance_id);
    let fname = sql_escape(filename);

    // Delete edges where source or target is a child of this file
    Spi::run(&format!(
        "DELETE FROM kerai.edges WHERE source_id IN (
            SELECT id FROM kerai.nodes
            WHERE instance_id = {inst}
            AND kind = 'file' AND content = '{fname}'
        ) OR target_id IN (
            SELECT id FROM kerai.nodes
            WHERE instance_id = {inst}
            AND kind = 'file' AND content = '{fname}'
        )",
    ))
    .ok();

    // Delete child nodes (anything with parent_id pointing to file's subtree)
    // Use recursive CTE to find all descendants
    Spi::run(&format!(
        "WITH RECURSIVE descendants AS (
            SELECT id FROM kerai.nodes
            WHERE instance_id = {inst}
            AND kind = 'file' AND content = '{fname}'
            UNION ALL
            SELECT n.id FROM kerai.nodes n
            JOIN descendants d ON n.parent_id = d.id
        )
        DELETE FROM kerai.edges WHERE source_id IN (SELECT id FROM descendants)
            OR target_id IN (SELECT id FROM descendants)",
    ))
    .ok();

    Spi::run(&format!(
        "WITH RECURSIVE descendants AS (
            SELECT id FROM kerai.nodes
            WHERE instance_id = {inst}
            AND kind = 'file' AND content = '{fname}'
            UNION ALL
            SELECT n.id FROM kerai.nodes n
            JOIN descendants d ON n.parent_id = d.id
        )
        DELETE FROM kerai.nodes WHERE id IN (SELECT id FROM descendants)",
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
            sql.push_str(&format!(
                "({}, {}, '{}', {}, {}, {}, {}, {}, {})",
                sql_uuid(&node.id),
                sql_uuid(&node.instance_id),
                sql_escape(&node.kind),
                sql_opt_text(&node.language),
                sql_opt_text(&node.content),
                match &node.parent_id {
                    Some(pid) => sql_uuid(pid),
                    None => "NULL".to_string(),
                },
                node.position,
                match &node.path {
                    Some(p) => sql_ltree(p),
                    None => "NULL".to_string(),
                },
                sql_jsonb(&node.metadata),
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
            sql.push_str(&format!(
                "({}, {}, {}, '{}', {})",
                sql_uuid(&edge.id),
                sql_uuid(&edge.source_id),
                sql_uuid(&edge.target_id),
                sql_escape(&edge.relation),
                sql_jsonb(&edge.metadata),
            ));
        }

        sql.push_str(" ON CONFLICT (source_id, target_id, relation) DO NOTHING");

        Spi::run(&sql).expect("Failed to insert edges batch");
    }
}
