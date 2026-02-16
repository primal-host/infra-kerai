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
    "set_perspective",
    "delete_perspective",
    "set_association",
    "delete_association",
    "create_task",
    "update_task_status",
    "create_wallet",
    "transfer_koi",
    "create_bounty",
    "update_bounty_status",
];

/// Validate that op_type is known and node_id requirements are met.
pub fn validate_op(op_type: &str, node_id: Option<&str>, _payload: &Value) {
    if !VALID_OP_TYPES.contains(&op_type) {
        error!("Unknown op_type: '{}'", op_type);
    }

    // These ops do not require node_id (they use agent_id from payload)
    let no_node_id_ops = [
        "insert_node",
        "set_perspective",
        "delete_perspective",
        "set_association",
        "delete_association",
        "create_task",
        "update_task_status",
        "create_wallet",
        "transfer_koi",
        "create_bounty",
        "update_bounty_status",
    ];
    if !no_node_id_ops.contains(&op_type) && node_id.is_none() {
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
        "set_perspective" => apply_set_perspective(payload),
        "delete_perspective" => apply_delete_perspective(payload),
        "set_association" => apply_set_association(payload),
        "delete_association" => apply_delete_association(payload),
        "create_task" => apply_create_task(payload),
        "update_task_status" => apply_update_task_status(payload),
        "create_wallet" => apply_create_wallet(payload),
        "transfer_koi" => apply_transfer_koi(payload),
        "create_bounty" => apply_create_bounty(payload),
        "update_bounty_status" => apply_update_bounty_status(payload),
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

/// UPSERT a perspective. Returns the perspective id.
fn apply_set_perspective(payload: &Value) -> String {
    let agent_id = payload["agent_id"]
        .as_str()
        .unwrap_or_else(|| error!("set_perspective requires 'agent_id' in payload"));
    let node_id = payload["node_id"]
        .as_str()
        .unwrap_or_else(|| error!("set_perspective requires 'node_id' in payload"));
    let weight = payload["weight"]
        .as_f64()
        .unwrap_or_else(|| error!("set_perspective requires 'weight' in payload"));

    let ctx_sql = match payload.get("context_id").and_then(|v| v.as_str()) {
        Some(c) => format!("'{}'::uuid", sql_escape(c)),
        None => "NULL".to_string(),
    };
    let reasoning_sql = match payload.get("reasoning").and_then(|v| v.as_str()) {
        Some(r) => format!("'{}'", sql_escape(r)),
        None => "NULL".to_string(),
    };

    let pid = Spi::get_one::<String>(&format!(
        "INSERT INTO kerai.perspectives (agent_id, node_id, weight, context_id, reasoning)
         VALUES ('{}'::uuid, '{}'::uuid, {}, {}, {})
         ON CONFLICT (agent_id, node_id, context_id)
         DO UPDATE SET weight = EXCLUDED.weight, reasoning = EXCLUDED.reasoning, updated_at = now()
         RETURNING id::text",
        sql_escape(agent_id),
        sql_escape(node_id),
        weight,
        ctx_sql,
        reasoning_sql,
    ))
    .unwrap()
    .unwrap();
    pid
}

/// DELETE a perspective. Returns the agent_id.
fn apply_delete_perspective(payload: &Value) -> String {
    let agent_id = payload["agent_id"]
        .as_str()
        .unwrap_or_else(|| error!("delete_perspective requires 'agent_id' in payload"));
    let node_id = payload["node_id"]
        .as_str()
        .unwrap_or_else(|| error!("delete_perspective requires 'node_id' in payload"));

    let ctx_clause = match payload.get("context_id").and_then(|v| v.as_str()) {
        Some(c) => format!("AND context_id = '{}'::uuid", sql_escape(c)),
        None => "AND context_id IS NULL".to_string(),
    };

    Spi::run(&format!(
        "DELETE FROM kerai.perspectives
         WHERE agent_id = '{}'::uuid AND node_id = '{}'::uuid {}",
        sql_escape(agent_id),
        sql_escape(node_id),
        ctx_clause,
    ))
    .unwrap();
    agent_id.to_string()
}

/// UPSERT an association. Returns the association id.
fn apply_set_association(payload: &Value) -> String {
    let agent_id = payload["agent_id"]
        .as_str()
        .unwrap_or_else(|| error!("set_association requires 'agent_id' in payload"));
    let source_id = payload["source_id"]
        .as_str()
        .unwrap_or_else(|| error!("set_association requires 'source_id' in payload"));
    let target_id = payload["target_id"]
        .as_str()
        .unwrap_or_else(|| error!("set_association requires 'target_id' in payload"));
    let weight = payload["weight"]
        .as_f64()
        .unwrap_or_else(|| error!("set_association requires 'weight' in payload"));
    let relation = payload["relation"]
        .as_str()
        .unwrap_or_else(|| error!("set_association requires 'relation' in payload"));

    let reasoning_sql = match payload.get("reasoning").and_then(|v| v.as_str()) {
        Some(r) => format!("'{}'", sql_escape(r)),
        None => "NULL".to_string(),
    };

    let aid = Spi::get_one::<String>(&format!(
        "INSERT INTO kerai.associations (agent_id, source_id, target_id, weight, relation, reasoning)
         VALUES ('{}'::uuid, '{}'::uuid, '{}'::uuid, {}, '{}', {})
         ON CONFLICT (agent_id, source_id, target_id, relation)
         DO UPDATE SET weight = EXCLUDED.weight, reasoning = EXCLUDED.reasoning, updated_at = now()
         RETURNING id::text",
        sql_escape(agent_id),
        sql_escape(source_id),
        sql_escape(target_id),
        weight,
        sql_escape(relation),
        reasoning_sql,
    ))
    .unwrap()
    .unwrap();
    aid
}

/// INSERT a new task. Returns the generated task UUID.
fn apply_create_task(payload: &Value) -> String {
    let description = payload["description"]
        .as_str()
        .unwrap_or_else(|| error!("create_task requires 'description' in payload"));
    let success_command = payload["success_command"]
        .as_str()
        .unwrap_or_else(|| error!("create_task requires 'success_command' in payload"));

    let scope_sql = match payload.get("scope_node_id").and_then(|v| v.as_str()) {
        Some(s) => format!("'{}'::uuid", sql_escape(s)),
        None => "NULL".to_string(),
    };
    let budget_ops_sql = match payload.get("budget_ops").and_then(|v| v.as_i64()) {
        Some(b) => b.to_string(),
        None => "NULL".to_string(),
    };
    let budget_seconds_sql = match payload.get("budget_seconds").and_then(|v| v.as_i64()) {
        Some(b) => b.to_string(),
        None => "NULL".to_string(),
    };

    let task_id = Spi::get_one::<String>(&format!(
        "INSERT INTO kerai.tasks (description, success_command, scope_node_id, budget_ops, budget_seconds)
         VALUES ('{}', '{}', {}, {}, {})
         RETURNING id::text",
        sql_escape(description),
        sql_escape(success_command),
        scope_sql,
        budget_ops_sql,
        budget_seconds_sql,
    ))
    .unwrap()
    .unwrap();
    task_id
}

/// UPDATE a task's status. Returns the task UUID.
fn apply_update_task_status(payload: &Value) -> String {
    let task_id = payload["task_id"]
        .as_str()
        .unwrap_or_else(|| error!("update_task_status requires 'task_id' in payload"));
    let new_status = payload["new_status"]
        .as_str()
        .unwrap_or_else(|| error!("update_task_status requires 'new_status' in payload"));

    let valid_statuses = ["pending", "running", "succeeded", "failed", "stopped"];
    if !valid_statuses.contains(&new_status) {
        error!(
            "Invalid task status '{}'. Must be one of: pending, running, succeeded, failed, stopped",
            new_status
        );
    }

    Spi::run(&format!(
        "UPDATE kerai.tasks SET status = '{}', updated_at = now() WHERE id = '{}'::uuid",
        sql_escape(new_status),
        sql_escape(task_id),
    ))
    .unwrap();
    task_id.to_string()
}

/// DELETE an association. Returns the agent_id.
fn apply_delete_association(payload: &Value) -> String {
    let agent_id = payload["agent_id"]
        .as_str()
        .unwrap_or_else(|| error!("delete_association requires 'agent_id' in payload"));
    let source_id = payload["source_id"]
        .as_str()
        .unwrap_or_else(|| error!("delete_association requires 'source_id' in payload"));
    let target_id = payload["target_id"]
        .as_str()
        .unwrap_or_else(|| error!("delete_association requires 'target_id' in payload"));
    let relation = payload["relation"]
        .as_str()
        .unwrap_or_else(|| error!("delete_association requires 'relation' in payload"));

    Spi::run(&format!(
        "DELETE FROM kerai.associations
         WHERE agent_id = '{}'::uuid AND source_id = '{}'::uuid
           AND target_id = '{}'::uuid AND relation = '{}'",
        sql_escape(agent_id),
        sql_escape(source_id),
        sql_escape(target_id),
        sql_escape(relation),
    ))
    .unwrap();
    agent_id.to_string()
}

/// INSERT a new wallet. Returns the wallet UUID.
fn apply_create_wallet(payload: &Value) -> String {
    let wallet_type = payload["wallet_type"]
        .as_str()
        .unwrap_or_else(|| error!("create_wallet requires 'wallet_type' in payload"));
    let public_key_hex = payload["public_key_hex"]
        .as_str()
        .unwrap_or_else(|| error!("create_wallet requires 'public_key_hex' in payload"));
    let fingerprint = payload["fingerprint"]
        .as_str()
        .unwrap_or_else(|| error!("create_wallet requires 'fingerprint' in payload"));

    let label_sql = match payload.get("label").and_then(|v| v.as_str()) {
        Some(l) => format!("'{}'", sql_escape(l)),
        None => "NULL".to_string(),
    };
    let instance_sql = match payload.get("instance_id").and_then(|v| v.as_str()) {
        Some(i) => format!("'{}'::uuid", sql_escape(i)),
        None => "NULL".to_string(),
    };

    let wallet_id = Spi::get_one::<String>(&format!(
        "INSERT INTO kerai.wallets (instance_id, public_key, key_fingerprint, wallet_type, label)
         VALUES ({}, '\\x{}'::bytea, '{}', '{}', {})
         RETURNING id::text",
        instance_sql,
        sql_escape(public_key_hex),
        sql_escape(fingerprint),
        sql_escape(wallet_type),
        label_sql,
    ))
    .unwrap()
    .unwrap();
    wallet_id
}

/// Transfer kōi between wallets via ledger INSERT.
fn apply_transfer_koi(payload: &Value) -> String {
    let from_wallet = payload.get("from_wallet").and_then(|v| v.as_str());
    let to_wallet = payload["to_wallet"]
        .as_str()
        .unwrap_or_else(|| error!("transfer_koi requires 'to_wallet' in payload"));
    let amount = payload["amount"]
        .as_i64()
        .unwrap_or_else(|| error!("transfer_koi requires 'amount' in payload"));
    let reason = payload["reason"]
        .as_str()
        .unwrap_or_else(|| error!("transfer_koi requires 'reason' in payload"));
    let timestamp = payload["timestamp"]
        .as_i64()
        .unwrap_or_else(|| error!("transfer_koi requires 'timestamp' in payload"));

    let from_sql = match from_wallet {
        Some(f) => format!("'{}'::uuid", sql_escape(f)),
        None => "NULL".to_string(),
    };
    let ref_id_sql = match payload.get("reference_id").and_then(|v| v.as_str()) {
        Some(r) => format!("'{}'::uuid", sql_escape(r)),
        None => "NULL".to_string(),
    };
    let ref_type_sql = match payload.get("reference_type").and_then(|v| v.as_str()) {
        Some(r) => format!("'{}'", sql_escape(r)),
        None => "NULL".to_string(),
    };

    let ledger_id = Spi::get_one::<String>(&format!(
        "INSERT INTO kerai.ledger (from_wallet, to_wallet, amount, reason, reference_id, reference_type, timestamp)
         VALUES ({}, '{}'::uuid, {}, '{}', {}, {}, {})
         RETURNING id::text",
        from_sql,
        sql_escape(to_wallet),
        amount,
        sql_escape(reason),
        ref_id_sql,
        ref_type_sql,
        timestamp,
    ))
    .unwrap()
    .unwrap();
    ledger_id
}

/// INSERT a new bounty. Returns the bounty UUID.
fn apply_create_bounty(payload: &Value) -> String {
    let poster_wallet = payload["poster_wallet"]
        .as_str()
        .unwrap_or_else(|| error!("create_bounty requires 'poster_wallet' in payload"));
    let scope = payload["scope"]
        .as_str()
        .unwrap_or_else(|| error!("create_bounty requires 'scope' in payload"));
    let description = payload["description"]
        .as_str()
        .unwrap_or_else(|| error!("create_bounty requires 'description' in payload"));
    let reward = payload["reward"]
        .as_i64()
        .unwrap_or_else(|| error!("create_bounty requires 'reward' in payload"));

    let cmd_sql = match payload.get("success_command").and_then(|v| v.as_str()) {
        Some(c) => format!("'{}'", sql_escape(c)),
        None => "NULL".to_string(),
    };
    let expires_sql = match payload.get("expires_at").and_then(|v| v.as_str()) {
        Some(e) => format!("'{}'::timestamptz", sql_escape(e)),
        None => "NULL".to_string(),
    };

    let bounty_id = Spi::get_one::<String>(&format!(
        "INSERT INTO kerai.bounties (poster_wallet, scope, description, success_command, reward, expires_at)
         VALUES ('{}'::uuid, '{}'::ltree, '{}', {}, {}, {})
         RETURNING id::text",
        sql_escape(poster_wallet),
        sql_escape(scope),
        sql_escape(description),
        cmd_sql,
        reward,
        expires_sql,
    ))
    .unwrap()
    .unwrap();
    bounty_id
}

/// UPDATE a bounty's status. Returns the bounty UUID.
fn apply_update_bounty_status(payload: &Value) -> String {
    let bounty_id = payload["bounty_id"]
        .as_str()
        .unwrap_or_else(|| error!("update_bounty_status requires 'bounty_id' in payload"));
    let new_status = payload["new_status"]
        .as_str()
        .unwrap_or_else(|| error!("update_bounty_status requires 'new_status' in payload"));

    let valid_statuses = ["open", "claimed", "paid", "expired", "cancelled"];
    if !valid_statuses.contains(&new_status) {
        error!(
            "Invalid bounty status '{}'. Must be one of: open, claimed, paid, expired, cancelled",
            new_status
        );
    }

    let mut extra_sets = String::new();
    if let Some(claimed_by) = payload.get("claimed_by").and_then(|v| v.as_str()) {
        extra_sets.push_str(&format!(", claimed_by = '{}'::uuid", sql_escape(claimed_by)));
    }
    if new_status == "paid" {
        extra_sets.push_str(", verified_at = now()");
    }

    Spi::run(&format!(
        "UPDATE kerai.bounties SET status = '{}'{}  WHERE id = '{}'::uuid",
        sql_escape(new_status),
        extra_sets,
        sql_escape(bounty_id),
    ))
    .unwrap();
    bounty_id.to_string()
}
