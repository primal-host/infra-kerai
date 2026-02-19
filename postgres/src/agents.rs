/// Agent management — register, list, get, remove AI agents.
use pgrx::prelude::*;

use crate::sql::sql_escape;

/// Register or update an AI agent. Returns JSON with agent info.
/// kind: 'human', 'llm', 'tool', 'swarm'
#[pg_extern]
fn register_agent(
    name: &str,
    kind: &str,
    model: Option<&str>,
    config: Option<pgrx::JsonB>,
) -> pgrx::JsonB {
    let valid_kinds = ["human", "llm", "tool", "swarm"];
    if !valid_kinds.contains(&kind) {
        error!(
            "Invalid agent kind '{}'. Must be one of: human, llm, tool, swarm",
            kind
        );
    }

    let model_sql = match model {
        Some(m) => format!("'{}'", sql_escape(m)),
        None => "NULL".to_string(),
    };
    let config_sql = match &config {
        Some(c) => format!("'{}'::jsonb", sql_escape(&c.0.to_string())),
        None => "'{}'::jsonb".to_string(),
    };

    // Check if agent already exists by name
    let existing = Spi::get_one::<String>(&format!(
        "SELECT id::text FROM kerai.agents WHERE name = '{}'",
        sql_escape(name),
    ))
    .unwrap_or(None);

    let is_new;
    let agent_id;

    if let Some(eid) = existing {
        // Update kind, model, config
        Spi::run(&format!(
            "UPDATE kerai.agents SET kind = '{}', model = {}, config = {}
             WHERE name = '{}'",
            sql_escape(kind),
            model_sql,
            config_sql,
            sql_escape(name),
        ))
        .unwrap();
        is_new = false;
        agent_id = eid;
    } else {
        // Insert new agent
        let new_id = Spi::get_one::<String>(&format!(
            "INSERT INTO kerai.agents (name, kind, model, config)
             VALUES ('{}', '{}', {}, {})
             RETURNING id::text",
            sql_escape(name),
            sql_escape(kind),
            model_sql,
            config_sql,
        ))
        .unwrap()
        .unwrap();
        is_new = true;
        agent_id = new_id;
    }

    pgrx::JsonB(serde_json::json!({
        "id": agent_id,
        "name": name,
        "kind": kind,
        "model": model,
        "is_new": is_new,
    }))
}

/// List agents with optional kind filter.
#[pg_extern]
fn list_agents(kind_filter: Option<&str>) -> pgrx::JsonB {
    let where_clause = match kind_filter {
        Some(k) => format!("WHERE kind = '{}'", sql_escape(k)),
        None => String::new(),
    };

    let json = Spi::get_one::<pgrx::JsonB>(&format!(
        "SELECT COALESCE(
            jsonb_agg(jsonb_build_object(
                'id', id,
                'name', name,
                'kind', kind,
                'model', model,
                'config', config,
                'created_at', created_at
            ) ORDER BY name),
            '[]'::jsonb
        ) FROM kerai.agents {}",
        where_clause,
    ))
    .unwrap()
    .unwrap_or_else(|| pgrx::JsonB(serde_json::json!([])));
    json
}

/// Get a single agent by name.
#[pg_extern]
fn get_agent(name: &str) -> pgrx::JsonB {
    let row = Spi::get_one::<pgrx::JsonB>(&format!(
        "SELECT jsonb_build_object(
            'id', id,
            'name', name,
            'kind', kind,
            'model', model,
            'config', config,
            'wallet_id', wallet_id,
            'created_at', created_at
        ) FROM kerai.agents WHERE name = '{}'",
        sql_escape(name),
    ))
    .unwrap_or(None);

    match row {
        Some(j) => j,
        None => error!("Agent not found: {}", name),
    }
}

/// Remove an agent by name. Fails if agent has perspectives or associations.
#[pg_extern]
fn remove_agent(name: &str) -> pgrx::JsonB {
    let agent_id = Spi::get_one::<String>(&format!(
        "SELECT id::text FROM kerai.agents WHERE name = '{}'",
        sql_escape(name),
    ))
    .unwrap_or(None);

    let aid = match agent_id {
        Some(id) => id,
        None => {
            return pgrx::JsonB(serde_json::json!({
                "removed": false,
                "name": name,
                "reason": "not found",
            }));
        }
    };

    // Check for existing perspectives
    let has_perspectives = Spi::get_one::<bool>(&format!(
        "SELECT EXISTS(SELECT 1 FROM kerai.perspectives WHERE agent_id = '{}'::uuid)",
        sql_escape(&aid),
    ))
    .unwrap()
    .unwrap_or(false);

    if has_perspectives {
        error!(
            "Cannot remove agent '{}': has existing perspectives. Delete them first.",
            name
        );
    }

    // Check for existing associations
    let has_associations = Spi::get_one::<bool>(&format!(
        "SELECT EXISTS(SELECT 1 FROM kerai.associations WHERE agent_id = '{}'::uuid)",
        sql_escape(&aid),
    ))
    .unwrap()
    .unwrap_or(false);

    if has_associations {
        error!(
            "Cannot remove agent '{}': has existing associations. Delete them first.",
            name
        );
    }

    Spi::run(&format!(
        "DELETE FROM kerai.agents WHERE name = '{}'",
        sql_escape(name),
    ))
    .unwrap();

    pgrx::JsonB(serde_json::json!({
        "removed": true,
        "name": name,
    }))
}
