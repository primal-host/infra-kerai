/// Perspective and association CRUD — weighted views of the codebase.
use pgrx::prelude::*;

use crate::sql::sql_escape;

/// Resolve agent name to agent_id. Errors if not found.
fn resolve_agent(name: &str) -> String {
    Spi::get_one::<String>(&format!(
        "SELECT id::text FROM kerai.agents WHERE name = '{}'",
        sql_escape(name),
    ))
    .unwrap_or(None)
    .unwrap_or_else(|| error!("Agent not found: {}", name))
}

/// Set or update a perspective (agent's weighted view of a node).
/// Weight should be -1.0 to 1.0. UPSERTs on (agent_id, node_id, context_id).
#[pg_extern]
fn set_perspective(
    agent_name: &str,
    node_id: pgrx::Uuid,
    weight: f64,
    context_id: Option<pgrx::Uuid>,
    reasoning: Option<&str>,
) -> pgrx::JsonB {
    if !(-1.0..=1.0).contains(&weight) {
        error!("Weight must be between -1.0 and 1.0, got {}", weight);
    }

    let agent_id = resolve_agent(agent_name);
    let nid = node_id.to_string();

    let ctx_sql = match context_id {
        Some(c) => format!("'{}'::uuid", c),
        None => "NULL".to_string(),
    };
    let reasoning_sql = match reasoning {
        Some(r) => format!("'{}'", sql_escape(r)),
        None => "NULL".to_string(),
    };

    let pid = Spi::get_one::<String>(&format!(
        "INSERT INTO kerai.perspectives (agent_id, node_id, weight, context_id, reasoning)
         VALUES ('{}'::uuid, '{}'::uuid, {}, {}, {})
         ON CONFLICT (agent_id, node_id, context_id)
         DO UPDATE SET weight = EXCLUDED.weight, reasoning = EXCLUDED.reasoning, updated_at = now()
         RETURNING id::text",
        sql_escape(&agent_id),
        sql_escape(&nid),
        weight,
        ctx_sql,
        reasoning_sql,
    ))
    .unwrap()
    .unwrap();

    pgrx::JsonB(serde_json::json!({
        "id": pid,
        "agent": agent_name,
        "node_id": nid,
        "weight": weight,
        "context_id": context_id.map(|c| c.to_string()),
    }))
}

/// Delete a perspective.
#[pg_extern]
fn delete_perspective(
    agent_name: &str,
    node_id: pgrx::Uuid,
    context_id: Option<pgrx::Uuid>,
) -> pgrx::JsonB {
    let agent_id = resolve_agent(agent_name);
    let nid = node_id.to_string();

    let ctx_clause = match context_id {
        Some(c) => format!("AND context_id = '{}'::uuid", c),
        None => "AND context_id IS NULL".to_string(),
    };

    let deleted = Spi::get_one::<i64>(&format!(
        "WITH deleted AS (
            DELETE FROM kerai.perspectives
            WHERE agent_id = '{}'::uuid AND node_id = '{}'::uuid {}
            RETURNING id
        ) SELECT count(*)::bigint FROM deleted",
        sql_escape(&agent_id),
        sql_escape(&nid),
        ctx_clause,
    ))
    .unwrap()
    .unwrap_or(0);

    pgrx::JsonB(serde_json::json!({
        "deleted": deleted > 0,
        "agent": agent_name,
        "node_id": nid,
    }))
}

/// Query an agent's perspectives with optional context and weight threshold.
#[pg_extern]
fn get_perspectives(
    agent_name: &str,
    context_id: Option<pgrx::Uuid>,
    min_weight: Option<f64>,
) -> pgrx::JsonB {
    let agent_id = resolve_agent(agent_name);

    let mut conditions = vec![format!(
        "p.agent_id = '{}'::uuid",
        sql_escape(&agent_id)
    )];

    if let Some(ctx) = context_id {
        conditions.push(format!("p.context_id = '{}'::uuid", ctx));
    }
    if let Some(mw) = min_weight {
        conditions.push(format!("p.weight >= {}", mw));
    }

    let where_clause = conditions.join(" AND ");

    let json = Spi::get_one::<pgrx::JsonB>(&format!(
        "SELECT COALESCE(
            jsonb_agg(jsonb_build_object(
                'id', p.id,
                'node_id', p.node_id,
                'weight', p.weight,
                'context_id', p.context_id,
                'reasoning', p.reasoning,
                'node_kind', n.kind,
                'node_content', n.content,
                'updated_at', p.updated_at
            ) ORDER BY p.weight DESC),
            '[]'::jsonb
        ) FROM kerai.perspectives p
        JOIN kerai.nodes n ON n.id = p.node_id
        WHERE {}",
        where_clause,
    ))
    .unwrap()
    .unwrap_or_else(|| pgrx::JsonB(serde_json::json!([])));
    json
}

/// Set or update an association (agent's weighted link between two nodes).
#[pg_extern]
fn set_association(
    agent_name: &str,
    source_id: pgrx::Uuid,
    target_id: pgrx::Uuid,
    weight: f64,
    relation: &str,
    reasoning: Option<&str>,
) -> pgrx::JsonB {
    if !(-1.0..=1.0).contains(&weight) {
        error!("Weight must be between -1.0 and 1.0, got {}", weight);
    }

    let agent_id = resolve_agent(agent_name);
    let sid = source_id.to_string();
    let tid = target_id.to_string();

    let reasoning_sql = match reasoning {
        Some(r) => format!("'{}'", sql_escape(r)),
        None => "NULL".to_string(),
    };

    let aid = Spi::get_one::<String>(&format!(
        "INSERT INTO kerai.associations (agent_id, source_id, target_id, weight, relation, reasoning)
         VALUES ('{}'::uuid, '{}'::uuid, '{}'::uuid, {}, '{}', {})
         ON CONFLICT (agent_id, source_id, target_id, relation)
         DO UPDATE SET weight = EXCLUDED.weight, reasoning = EXCLUDED.reasoning, updated_at = now()
         RETURNING id::text",
        sql_escape(&agent_id),
        sql_escape(&sid),
        sql_escape(&tid),
        weight,
        sql_escape(relation),
        reasoning_sql,
    ))
    .unwrap()
    .unwrap();

    pgrx::JsonB(serde_json::json!({
        "id": aid,
        "agent": agent_name,
        "source_id": sid,
        "target_id": tid,
        "weight": weight,
        "relation": relation,
    }))
}

/// Delete an association.
#[pg_extern]
fn delete_association(
    agent_name: &str,
    source_id: pgrx::Uuid,
    target_id: pgrx::Uuid,
    relation: &str,
) -> pgrx::JsonB {
    let agent_id = resolve_agent(agent_name);
    let sid = source_id.to_string();
    let tid = target_id.to_string();

    let deleted = Spi::get_one::<i64>(&format!(
        "WITH deleted AS (
            DELETE FROM kerai.associations
            WHERE agent_id = '{}'::uuid AND source_id = '{}'::uuid
              AND target_id = '{}'::uuid AND relation = '{}'
            RETURNING id
        ) SELECT count(*)::bigint FROM deleted",
        sql_escape(&agent_id),
        sql_escape(&sid),
        sql_escape(&tid),
        sql_escape(relation),
    ))
    .unwrap()
    .unwrap_or(0);

    pgrx::JsonB(serde_json::json!({
        "deleted": deleted > 0,
        "agent": agent_name,
        "source_id": sid,
        "target_id": tid,
        "relation": relation,
    }))
}

/// Query an agent's associations with optional source and relation filter.
#[pg_extern]
fn get_associations(
    agent_name: &str,
    source_id: Option<pgrx::Uuid>,
    relation: Option<&str>,
) -> pgrx::JsonB {
    let agent_id = resolve_agent(agent_name);

    let mut conditions = vec![format!(
        "a.agent_id = '{}'::uuid",
        sql_escape(&agent_id)
    )];

    if let Some(sid) = source_id {
        conditions.push(format!("a.source_id = '{}'::uuid", sid));
    }
    if let Some(rel) = relation {
        conditions.push(format!("a.relation = '{}'", sql_escape(rel)));
    }

    let where_clause = conditions.join(" AND ");

    let json = Spi::get_one::<pgrx::JsonB>(&format!(
        "SELECT COALESCE(
            jsonb_agg(jsonb_build_object(
                'id', a.id,
                'source_id', a.source_id,
                'target_id', a.target_id,
                'weight', a.weight,
                'relation', a.relation,
                'reasoning', a.reasoning,
                'updated_at', a.updated_at
            ) ORDER BY a.weight DESC),
            '[]'::jsonb
        ) FROM kerai.associations a
        WHERE {}",
        where_clause,
    ))
    .unwrap()
    .unwrap_or_else(|| pgrx::JsonB(serde_json::json!([])));
    json
}
