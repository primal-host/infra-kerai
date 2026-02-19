/// Task management — create, get, list, update status for swarm tasks.
use pgrx::prelude::*;

use crate::sql::sql_escape;

/// Create a new task with status='pending'.
#[pg_extern]
fn create_task(
    description: &str,
    success_command: &str,
    scope_node_id: Option<pgrx::Uuid>,
    budget_ops: Option<i32>,
    budget_seconds: Option<i32>,
) -> pgrx::JsonB {
    let scope_sql = match scope_node_id {
        Some(id) => format!("'{}'::uuid", id),
        None => "NULL".to_string(),
    };
    let budget_ops_sql = match budget_ops {
        Some(b) => b.to_string(),
        None => "NULL".to_string(),
    };
    let budget_seconds_sql = match budget_seconds {
        Some(b) => b.to_string(),
        None => "NULL".to_string(),
    };

    let row = Spi::get_one::<pgrx::JsonB>(&format!(
        "INSERT INTO kerai.tasks (description, success_command, scope_node_id, budget_ops, budget_seconds)
         VALUES ('{}', '{}', {}, {}, {})
         RETURNING jsonb_build_object(
             'id', id,
             'description', description,
             'success_command', success_command,
             'scope_node_id', scope_node_id,
             'budget_ops', budget_ops,
             'budget_seconds', budget_seconds,
             'status', status,
             'created_at', created_at
         )",
        sql_escape(description),
        sql_escape(success_command),
        scope_sql,
        budget_ops_sql,
        budget_seconds_sql,
    ))
    .unwrap()
    .unwrap();
    row
}

/// Get a single task by ID, including swarm agent name if linked.
#[pg_extern]
fn get_task(task_id: pgrx::Uuid) -> pgrx::JsonB {
    let row = Spi::get_one::<pgrx::JsonB>(&format!(
        "SELECT jsonb_build_object(
            'id', t.id,
            'description', t.description,
            'success_command', t.success_command,
            'scope_node_id', t.scope_node_id,
            'budget_ops', t.budget_ops,
            'budget_seconds', t.budget_seconds,
            'status', t.status,
            'agent_kind', t.agent_kind,
            'agent_model', t.agent_model,
            'agent_count', t.agent_count,
            'swarm_id', t.swarm_id,
            'swarm_name', a.name,
            'created_at', t.created_at,
            'updated_at', t.updated_at
        )
        FROM kerai.tasks t
        LEFT JOIN kerai.agents a ON t.swarm_id = a.id
        WHERE t.id = '{}'::uuid",
        task_id,
    ))
    .unwrap_or(None);

    match row {
        Some(j) => j,
        None => error!("Task not found: {}", task_id),
    }
}

/// List tasks, optionally filtered by status.
#[pg_extern]
fn list_tasks(status_filter: Option<&str>) -> pgrx::JsonB {
    let where_clause = match status_filter {
        Some(s) => format!("WHERE t.status = '{}'", sql_escape(s)),
        None => String::new(),
    };

    let json = Spi::get_one::<pgrx::JsonB>(&format!(
        "SELECT COALESCE(
            jsonb_agg(jsonb_build_object(
                'id', t.id,
                'description', t.description,
                'status', t.status,
                'agent_kind', t.agent_kind,
                'agent_count', t.agent_count,
                'swarm_name', a.name,
                'created_at', t.created_at,
                'updated_at', t.updated_at
            ) ORDER BY t.created_at DESC),
            '[]'::jsonb
        )
        FROM kerai.tasks t
        LEFT JOIN kerai.agents a ON t.swarm_id = a.id
        {}",
        where_clause,
    ))
    .unwrap()
    .unwrap_or_else(|| pgrx::JsonB(serde_json::json!([])));
    json
}

/// Update a task's status. Validates status is one of: pending, running, succeeded, failed, stopped.
#[pg_extern]
fn update_task_status(task_id: pgrx::Uuid, new_status: &str) -> pgrx::JsonB {
    let valid_statuses = ["pending", "running", "succeeded", "failed", "stopped"];
    if !valid_statuses.contains(&new_status) {
        error!(
            "Invalid task status '{}'. Must be one of: pending, running, succeeded, failed, stopped",
            new_status
        );
    }

    // Verify task exists
    let exists = Spi::get_one::<bool>(&format!(
        "SELECT EXISTS(SELECT 1 FROM kerai.tasks WHERE id = '{}'::uuid)",
        task_id,
    ))
    .unwrap()
    .unwrap_or(false);

    if !exists {
        error!("Task not found: {}", task_id);
    }

    Spi::run(&format!(
        "UPDATE kerai.tasks SET status = '{}', updated_at = now() WHERE id = '{}'::uuid",
        sql_escape(new_status),
        task_id,
    ))
    .unwrap();

    let row = Spi::get_one::<pgrx::JsonB>(&format!(
        "SELECT jsonb_build_object(
            'id', id,
            'status', status,
            'updated_at', updated_at
        ) FROM kerai.tasks WHERE id = '{}'::uuid",
        task_id,
    ))
    .unwrap()
    .unwrap();
    row
}
