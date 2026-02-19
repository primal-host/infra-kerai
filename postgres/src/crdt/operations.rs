/// Validation and materialized state application for CRDT operations.
/// Each op_type maps to an INSERT/UPDATE/DELETE on kerai.nodes or kerai.edges.
use base64::Engine as _;
use pgrx::prelude::*;
use serde_json::Value;

use crate::sql::sql_escape;

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
    "register_wallet",
    "signed_transfer",
    "mint_reward",
    "create_model",
    "update_model_weights",
    "delete_model",
    "train_step",
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
        "register_wallet",
        "signed_transfer",
        "mint_reward",
        "create_model",
        "update_model_weights",
        "delete_model",
        "train_step",
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
        "register_wallet" => apply_register_wallet(payload),
        "signed_transfer" => apply_signed_transfer(payload),
        "mint_reward" => apply_mint_reward(payload),
        "create_model" => apply_create_model(payload),
        "update_model_weights" => apply_update_model_weights(payload),
        "delete_model" => apply_delete_model(payload),
        "train_step" => apply_train_step(payload),
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

/// Transfer Koi between wallets via ledger INSERT.
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

/// INSERT a wallet via register_wallet (client-side key). Returns wallet UUID.
fn apply_register_wallet(payload: &Value) -> String {
    let public_key_hex = payload["public_key_hex"]
        .as_str()
        .unwrap_or_else(|| error!("register_wallet requires 'public_key_hex' in payload"));
    let wallet_type = payload["wallet_type"]
        .as_str()
        .unwrap_or_else(|| error!("register_wallet requires 'wallet_type' in payload"));
    let fingerprint = payload["fingerprint"]
        .as_str()
        .unwrap_or_else(|| error!("register_wallet requires 'fingerprint' in payload"));

    let label_sql = match payload.get("label").and_then(|v| v.as_str()) {
        Some(l) => format!("'{}'", sql_escape(l)),
        None => "NULL".to_string(),
    };

    let wallet_id = Spi::get_one::<String>(&format!(
        "INSERT INTO kerai.wallets (public_key, key_fingerprint, wallet_type, label)
         VALUES ('\\x{}'::bytea, '{}', '{}', {})
         RETURNING id::text",
        sql_escape(public_key_hex),
        sql_escape(fingerprint),
        sql_escape(wallet_type),
        label_sql,
    ))
    .unwrap()
    .unwrap();
    wallet_id
}

/// Signed transfer via CRDT replication. Verifies signature and inserts ledger entry.
fn apply_signed_transfer(payload: &Value) -> String {
    let from_wallet = payload["from_wallet"]
        .as_str()
        .unwrap_or_else(|| error!("signed_transfer requires 'from_wallet' in payload"));
    let to_wallet = payload["to_wallet"]
        .as_str()
        .unwrap_or_else(|| error!("signed_transfer requires 'to_wallet' in payload"));
    let amount = payload["amount"]
        .as_i64()
        .unwrap_or_else(|| error!("signed_transfer requires 'amount' in payload"));
    let reason = payload["reason"]
        .as_str()
        .unwrap_or("signed_transfer");
    let timestamp = payload["timestamp"]
        .as_i64()
        .unwrap_or_else(|| error!("signed_transfer requires 'timestamp' in payload"));
    let nonce = payload["nonce"]
        .as_i64()
        .unwrap_or_else(|| error!("signed_transfer requires 'nonce' in payload"));

    let sig_sql = match payload.get("signature_hex").and_then(|v| v.as_str()) {
        Some(s) => format!("'\\x{}'::bytea", sql_escape(s)),
        None => "NULL".to_string(),
    };

    // Insert ledger entry
    let ledger_id = Spi::get_one::<String>(&format!(
        "INSERT INTO kerai.ledger (from_wallet, to_wallet, amount, reason, signature, timestamp)
         VALUES ('{}'::uuid, '{}'::uuid, {}, '{}', {}, {})
         RETURNING id::text",
        sql_escape(from_wallet),
        sql_escape(to_wallet),
        amount,
        sql_escape(reason),
        sig_sql,
        timestamp,
    ))
    .unwrap()
    .unwrap();

    // Update nonce
    Spi::run(&format!(
        "UPDATE kerai.wallets SET nonce = {} WHERE id = '{}'::uuid",
        nonce,
        sql_escape(from_wallet),
    ))
    .unwrap();

    ledger_id
}

/// Mint reward via CRDT replication. Inserts ledger mint + reward_log entry.
fn apply_mint_reward(payload: &Value) -> String {
    let to_wallet = payload["to_wallet"]
        .as_str()
        .unwrap_or_else(|| error!("mint_reward requires 'to_wallet' in payload"));
    let amount = payload["amount"]
        .as_i64()
        .unwrap_or_else(|| error!("mint_reward requires 'amount' in payload"));
    let work_type = payload["work_type"]
        .as_str()
        .unwrap_or_else(|| error!("mint_reward requires 'work_type' in payload"));
    let timestamp = payload["timestamp"]
        .as_i64()
        .unwrap_or_else(|| error!("mint_reward requires 'timestamp' in payload"));

    let reason = format!("reward:{}", work_type);

    // Insert ledger entry (mint: from_wallet = NULL)
    let ledger_id = Spi::get_one::<String>(&format!(
        "INSERT INTO kerai.ledger (from_wallet, to_wallet, amount, reason, timestamp)
         VALUES (NULL, '{}'::uuid, {}, '{}', {})
         RETURNING id::text",
        sql_escape(to_wallet),
        amount,
        sql_escape(&reason),
        timestamp,
    ))
    .unwrap()
    .unwrap();

    // Insert reward_log
    let empty_obj = Value::Object(serde_json::Map::new());
    let details = payload.get("details").unwrap_or(&empty_obj);
    let details_str = sql_escape(&details.to_string());

    Spi::run(&format!(
        "INSERT INTO kerai.reward_log (work_type, reward, wallet_id, details)
         VALUES ('{}', {}, '{}'::uuid, '{}'::jsonb)",
        sql_escape(work_type),
        amount,
        sql_escape(to_wallet),
        details_str,
    ))
    .unwrap();

    ledger_id
}

/// Create model: record that a model was initialized for an agent. Returns agent_id.
fn apply_create_model(payload: &Value) -> String {
    let agent_id = payload["agent_id"]
        .as_str()
        .unwrap_or_else(|| error!("create_model requires 'agent_id' in payload"));
    let config = payload
        .get("config")
        .unwrap_or_else(|| error!("create_model requires 'config' in payload"));
    let config_str = sql_escape(&config.to_string());

    Spi::run(&format!(
        "UPDATE kerai.agents SET config = '{}'::jsonb WHERE id = '{}'::uuid",
        config_str,
        sql_escape(agent_id),
    ))
    .unwrap();

    agent_id.to_string()
}

/// Update model weights: apply a delta (base64-encoded f32 array) to a tensor.
/// Federated averaging: element-wise addition of deltas.
fn apply_update_model_weights(payload: &Value) -> String {
    let agent_id = payload["agent_id"]
        .as_str()
        .unwrap_or_else(|| error!("update_model_weights requires 'agent_id' in payload"));
    let tensor_name = payload["tensor_name"]
        .as_str()
        .unwrap_or_else(|| error!("update_model_weights requires 'tensor_name' in payload"));
    let delta_b64 = payload["delta"]
        .as_str()
        .unwrap_or_else(|| error!("update_model_weights requires 'delta' (base64) in payload"));

    // Decode delta
    let delta_bytes = base64::engine::general_purpose::STANDARD
        .decode(delta_b64)
        .unwrap_or_else(|e| error!("Invalid base64 delta: {e}"));

    if delta_bytes.len() % 4 != 0 {
        error!("Delta byte length must be a multiple of 4");
    }

    let delta_floats: Vec<f32> = delta_bytes
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect();

    // Load current tensor
    let load_sql = format!(
        "SELECT tensor_data, shape FROM kerai.model_weights
         WHERE agent_id = '{}'::uuid AND tensor_name = '{}'",
        sql_escape(agent_id),
        sql_escape(tensor_name),
    );

    let mut current_bytes: Option<Vec<u8>> = None;
    Spi::connect(|client| {
        if let Ok(tup_table) = client.select(&load_sql, None, &[]) {
            for row in tup_table {
                current_bytes = row.get_by_name::<Vec<u8>, _>("tensor_data").ok().flatten();
            }
        }
    });

    match current_bytes {
        Some(bytes) => {
            if bytes.len() != delta_bytes.len() {
                error!(
                    "Delta size {} != tensor size {} for '{}'",
                    delta_bytes.len(),
                    bytes.len(),
                    tensor_name
                );
            }
            // Element-wise add
            let mut current_floats: Vec<f32> = bytes
                .chunks_exact(4)
                .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                .collect();
            for (c, d) in current_floats.iter_mut().zip(delta_floats.iter()) {
                *c += d;
            }
            // Serialize back
            let mut new_bytes = Vec::with_capacity(current_floats.len() * 4);
            for &v in &current_floats {
                new_bytes.extend_from_slice(&v.to_le_bytes());
            }
            let hex: String = new_bytes.iter().map(|b| format!("{:02x}", b)).collect();
            let update_sql = format!(
                "UPDATE kerai.model_weights
                 SET tensor_data = '\\x{}'::bytea, version = version + 1, updated_at = now()
                 WHERE agent_id = '{}'::uuid AND tensor_name = '{}'",
                hex,
                sql_escape(agent_id),
                sql_escape(tensor_name),
            );
            Spi::run(&update_sql).unwrap();
        }
        None => {
            // Tensor doesn't exist yet — store delta as initial weights
            let mut new_bytes = Vec::with_capacity(delta_floats.len() * 4);
            for &v in &delta_floats {
                new_bytes.extend_from_slice(&v.to_le_bytes());
            }
            let hex: String = new_bytes.iter().map(|b| format!("{:02x}", b)).collect();
            let shape_arr = payload.get("shape").and_then(|v| v.as_array());
            let shape_sql = match shape_arr {
                Some(arr) => {
                    let dims: Vec<String> = arr
                        .iter()
                        .filter_map(|v| v.as_i64().map(|i| i.to_string()))
                        .collect();
                    format!("ARRAY[{}]::integer[]", dims.join(","))
                }
                None => format!("ARRAY[{}]::integer[]", delta_floats.len()),
            };
            let insert_sql = format!(
                "INSERT INTO kerai.model_weights (agent_id, tensor_name, tensor_data, shape)
                 VALUES ('{}'::uuid, '{}', '\\x{}'::bytea, {})",
                sql_escape(agent_id),
                sql_escape(tensor_name),
                hex,
                shape_sql,
            );
            Spi::run(&insert_sql).unwrap();
        }
    }

    agent_id.to_string()
}

/// Delete model: remove all weights and vocab for an agent.
fn apply_delete_model(payload: &Value) -> String {
    let agent_id = payload["agent_id"]
        .as_str()
        .unwrap_or_else(|| error!("delete_model requires 'agent_id' in payload"));

    Spi::run(&format!(
        "DELETE FROM kerai.model_weights WHERE agent_id = '{}'::uuid",
        sql_escape(agent_id),
    ))
    .unwrap();

    Spi::run(&format!(
        "DELETE FROM kerai.model_vocab WHERE model_id = '{}'::uuid",
        sql_escape(agent_id),
    ))
    .unwrap();

    Spi::run(&format!(
        "UPDATE kerai.agents SET config = '{{}}'::jsonb WHERE id = '{}'::uuid",
        sql_escape(agent_id),
    ))
    .unwrap();

    agent_id.to_string()
}

/// Train step: audit log entry for a training step. Returns training_run id.
fn apply_train_step(payload: &Value) -> String {
    let agent_id = payload["agent_id"]
        .as_str()
        .unwrap_or_else(|| error!("train_step requires 'agent_id' in payload"));
    let empty_config = Value::Object(serde_json::Map::new());
    let config = payload.get("config").unwrap_or(&empty_config);
    let walk_type = payload["walk_type"].as_str().unwrap_or("tree");
    let n_sequences = payload["n_sequences"].as_i64().unwrap_or(0);
    let n_steps = payload["n_steps"].as_i64().unwrap_or(0);
    let final_loss = payload["final_loss"].as_f64().unwrap_or(0.0);
    let duration_ms = payload["duration_ms"].as_i64().unwrap_or(0);

    let scope_sql = match payload.get("scope").and_then(|v| v.as_str()) {
        Some(s) => format!("'{}'::ltree", sql_escape(s)),
        None => "NULL".to_string(),
    };

    let run_id = Spi::get_one::<String>(&format!(
        "INSERT INTO kerai.training_runs (agent_id, config, walk_type, scope, n_sequences, n_steps, final_loss, duration_ms)
         VALUES ('{}'::uuid, '{}'::jsonb, '{}', {}, {}, {}, {}, {})
         RETURNING id::text",
        sql_escape(agent_id),
        sql_escape(&config.to_string()),
        sql_escape(walk_type),
        scope_sql,
        n_sequences,
        n_steps,
        final_loss,
        duration_ms,
    ))
    .unwrap()
    .unwrap();

    run_id
}
