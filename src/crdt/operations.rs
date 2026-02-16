/// Validation and materialized state application for CRDT operations.
/// Each op_type maps to an INSERT/UPDATE/DELETE on kerai.nodes or kerai.edges.
use pgrx::prelude::*;
use serde_json::Value;

/// Escape a string for use in a SQL literal (double single quotes).
fn sql_escape(s: &str) -> String {
    s.replace('\'', "''")
}

/// Valid operation types.
const VALID_OP_TYPES: &[&str] = &[
    "insert_node",
    "update_content",
    "update_metadata",
    "move_node",
    "delete_node",
    "insert_edge",
    "delete_edge",
];

/// Validate that op_type is known and node_id requirements are met.
pub fn validate_op(op_type: &str, node_id: Option<&str>, _payload: &Value) {
    if !VALID_OP_TYPES.contains(&op_type) {
        error!("Unknown op_type: '{}'", op_type);
    }

    // insert_node does not require node_id; all others do
    if op_type != "insert_node" && node_id.is_none() {
        error!("op_type '{}' requires a node_id", op_type);
    }
}

/// Dispatch an operation to the appropriate apply handler.
/// Returns the affected node_id as a string (generated for insert_node, echoed otherwise).
pub fn apply(
    op_type: &str,
    node_id: Option<&str>,
    payload: &Value,
    instance_id: &str,
) -> String {
    match op_type {
        "insert_node" => apply_insert_node(payload, instance_id),
        "update_content" => {
            let nid = node_id.unwrap();
            apply_update_content(nid, payload);
            nid.to_string()
        }
        "update_metadata" => {
            let nid = node_id.unwrap();
            apply_update_metadata(nid, payload);
            nid.to_string()
        }
        "move_node" => {
            let nid = node_id.unwrap();
            apply_move_node(nid, payload);
            nid.to_string()
        }
        "delete_node" => {
            let nid = node_id.unwrap();
            apply_delete_node(nid, payload);
            nid.to_string()
        }
        "insert_edge" => {
            let nid = node_id.unwrap();
            apply_insert_edge(nid, payload);
            nid.to_string()
        }
        "delete_edge" => {
            let nid = node_id.unwrap();
            apply_delete_edge(nid, payload);
            nid.to_string()
        }
        _ => error!("Unknown op_type: '{}'", op_type),
    }
}

/// INSERT a new node. Returns the generated UUID.
fn apply_insert_node(payload: &Value, instance_id: &str) -> String {
    let kind = payload["kind"]
        .as_str()
        .unwrap_or_else(|| error!("insert_node requires 'kind' in payload"));

    let language = payload.get("language").and_then(|v| v.as_str());
    let content = payload.get("content").and_then(|v| v.as_str());
    let parent_id = payload.get("parent_id").and_then(|v| v.as_str());
    let position = payload.get("position").and_then(|v| v.as_i64()).unwrap_or(0);
    let path = payload.get("path").and_then(|v| v.as_str());
    let empty_obj = Value::Object(serde_json::Map::new());
    let metadata = payload.get("metadata").unwrap_or(&empty_obj);

    let lang_sql = match language {
        Some(l) => format!("'{}'", sql_escape(l)),
        None => "NULL".to_string(),
    };
    let content_sql = match content {
        Some(c) => format!("'{}'", sql_escape(c)),
        None => "NULL".to_string(),
    };
    let parent_sql = match parent_id {
        Some(p) => format!("'{}'::uuid", sql_escape(p)),
        None => "NULL".to_string(),
    };
    let path_sql = match path {
        Some(p) => format!("'{}'::ltree", sql_escape(p)),
        None => "NULL".to_string(),
    };
    let meta_str = sql_escape(&metadata.to_string());

    let new_id = Spi::get_one::<String>(&format!(
        "INSERT INTO kerai.nodes (instance_id, kind, language, content, parent_id, position, path, metadata)
         VALUES ('{}'::uuid, '{}', {}, {}, {}, {}, {}, '{}'::jsonb)
         RETURNING id::text",
        sql_escape(instance_id),
        sql_escape(kind),
        lang_sql,
        content_sql,
        parent_sql,
        position,
        path_sql,
        meta_str,
    ))
    .unwrap()
    .unwrap();

    new_id
}

/// UPDATE the content field of a node.
fn apply_update_content(node_id: &str, payload: &Value) {
    let new_content = payload["new_content"]
        .as_str()
        .unwrap_or_else(|| error!("update_content requires 'new_content' in payload"));

    Spi::run(&format!(
        "UPDATE kerai.nodes SET content = '{}' WHERE id = '{}'::uuid",
        sql_escape(new_content),
        sql_escape(node_id),
    ))
    .unwrap();
}

/// UPDATE the metadata field of a node (JSONB merge via ||).
fn apply_update_metadata(node_id: &str, payload: &Value) {
    let merge = payload
        .get("merge")
        .unwrap_or_else(|| error!("update_metadata requires 'merge' in payload"));

    let merge_str = sql_escape(&merge.to_string());
    Spi::run(&format!(
        "UPDATE kerai.nodes SET metadata = metadata || '{}'::jsonb WHERE id = '{}'::uuid",
        merge_str,
        sql_escape(node_id),
    ))
    .unwrap();
}

/// UPDATE the parent_id and/or position of a node.
fn apply_move_node(node_id: &str, payload: &Value) {
    let mut sets = Vec::new();
    if let Some(new_parent) = payload.get("new_parent_id").and_then(|v| v.as_str()) {
        sets.push(format!("parent_id = '{}'::uuid", sql_escape(new_parent)));
    }
    if let Some(new_pos) = payload.get("new_position").and_then(|v| v.as_i64()) {
        sets.push(format!("position = {}", new_pos));
    }
    if sets.is_empty() {
        return;
    }
    Spi::run(&format!(
        "UPDATE kerai.nodes SET {} WHERE id = '{}'::uuid",
        sets.join(", "),
        sql_escape(node_id),
    ))
    .unwrap();
}

/// DELETE a node. If cascade=true, recursively delete children. Otherwise reparent children.
fn apply_delete_node(node_id: &str, payload: &Value) {
    let cascade = payload.get("cascade").and_then(|v| v.as_bool()).unwrap_or(false);
    let escaped_id = sql_escape(node_id);

    if cascade {
        // Delete edges referencing any descendant, then delete descendants
        Spi::run(&format!(
            "WITH RECURSIVE descendants AS (
                SELECT id FROM kerai.nodes WHERE id = '{0}'::uuid
                UNION ALL
                SELECT n.id FROM kerai.nodes n JOIN descendants d ON n.parent_id = d.id
            )
            DELETE FROM kerai.edges WHERE source_id IN (SELECT id FROM descendants)
                OR target_id IN (SELECT id FROM descendants)",
            escaped_id,
        ))
        .unwrap();

        Spi::run(&format!(
            "WITH RECURSIVE descendants AS (
                SELECT id FROM kerai.nodes WHERE id = '{0}'::uuid
                UNION ALL
                SELECT n.id FROM kerai.nodes n JOIN descendants d ON n.parent_id = d.id
            )
            DELETE FROM kerai.nodes WHERE id IN (SELECT id FROM descendants)",
            escaped_id,
        ))
        .unwrap();
    } else {
        // Reparent children to the deleted node's parent
        Spi::run(&format!(
            "UPDATE kerai.nodes SET parent_id = (
                SELECT parent_id FROM kerai.nodes WHERE id = '{0}'::uuid
            ) WHERE parent_id = '{0}'::uuid",
            escaped_id,
        ))
        .unwrap();

        // Delete edges referencing this node
        Spi::run(&format!(
            "DELETE FROM kerai.edges WHERE source_id = '{0}'::uuid OR target_id = '{0}'::uuid",
            escaped_id,
        ))
        .unwrap();

        // Delete the node itself
        Spi::run(&format!(
            "DELETE FROM kerai.nodes WHERE id = '{}'::uuid",
            escaped_id,
        ))
        .unwrap();
    }
}

/// INSERT an edge. ON CONFLICT DO NOTHING for idempotency.
fn apply_insert_edge(source_id: &str, payload: &Value) {
    let target_id = payload["target_id"]
        .as_str()
        .unwrap_or_else(|| error!("insert_edge requires 'target_id' in payload"));
    let relation = payload["relation"]
        .as_str()
        .unwrap_or_else(|| error!("insert_edge requires 'relation' in payload"));
    let empty_obj = Value::Object(serde_json::Map::new());
    let metadata = payload.get("metadata").unwrap_or(&empty_obj);
    let meta_str = sql_escape(&metadata.to_string());

    Spi::run(&format!(
        "INSERT INTO kerai.edges (source_id, target_id, relation, metadata)
         VALUES ('{}'::uuid, '{}'::uuid, '{}', '{}'::jsonb)
         ON CONFLICT (source_id, target_id, relation) DO NOTHING",
        sql_escape(source_id),
        sql_escape(target_id),
        sql_escape(relation),
        meta_str,
    ))
    .unwrap();
}

/// DELETE an edge by source, target, and relation.
fn apply_delete_edge(source_id: &str, payload: &Value) {
    let target_id = payload["target_id"]
        .as_str()
        .unwrap_or_else(|| error!("delete_edge requires 'target_id' in payload"));
    let relation = payload["relation"]
        .as_str()
        .unwrap_or_else(|| error!("delete_edge requires 'relation' in payload"));

    Spi::run(&format!(
        "DELETE FROM kerai.edges WHERE source_id = '{}'::uuid AND target_id = '{}'::uuid AND relation = '{}'",
        sql_escape(source_id),
        sql_escape(target_id),
        sql_escape(relation),
    ))
    .unwrap();
}
