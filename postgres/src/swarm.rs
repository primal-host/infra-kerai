/// Swarm management — launch, stop, record results, observability.
use pgrx::prelude::*;

use crate::sql::sql_escape;

/// Launch a swarm for a task. Creates a swarm agent, links it to the task, sets status='running'.
#[pg_extern]
fn launch_swarm(
    task_id: pgrx::Uuid,
    agent_count: i32,
    agent_kind: &str,
    agent_model: Option<&str>,
) -> pgrx::JsonB {
    // Verify task exists and is pending
    let status = Spi::get_one::<String>(&format!(
        "SELECT status FROM kerai.tasks WHERE id = '{}'::uuid",
        task_id,
    ))
    .unwrap_or(None);

    match status.as_deref() {
        None => error!("Task not found: {}", task_id),
        Some("pending") => {}
        Some(s) => error!("Task must be 'pending' to launch swarm, currently '{}'", s),
    }

    // Create swarm agent with name derived from task_id
    let task_short = &task_id.to_string()[..8];
    let swarm_name = format!("swarm-{}", task_short);

    let model_sql = match agent_model {
        Some(m) => format!("'{}'", sql_escape(m)),
        None => "NULL".to_string(),
    };

    let swarm_id = Spi::get_one::<String>(&format!(
        "INSERT INTO kerai.agents (name, kind, model)
         VALUES ('{}', 'swarm', {})
         ON CONFLICT (name) DO UPDATE SET kind = 'swarm', model = EXCLUDED.model
         RETURNING id::text",
        sql_escape(&swarm_name),
        model_sql,
    ))
    .unwrap()
    .unwrap();

    // Link swarm to task and set running
    let agent_model_sql = match agent_model {
        Some(m) => format!("'{}'", sql_escape(m)),
        None => "NULL".to_string(),
    };

    Spi::run(&format!(
        "UPDATE kerai.tasks
         SET status = 'running',
             swarm_id = '{}'::uuid,
             agent_kind = '{}',
             agent_model = {},
             agent_count = {},
             updated_at = now()
         WHERE id = '{}'::uuid",
        sql_escape(&swarm_id),
        sql_escape(agent_kind),
        agent_model_sql,
        agent_count,
        task_id,
    ))
    .unwrap();

    pgrx::JsonB(serde_json::json!({
        "task_id": task_id.to_string(),
        "swarm_id": swarm_id,
        "swarm_name": swarm_name,
        "agent_kind": agent_kind,
        "agent_model": agent_model,
        "agent_count": agent_count,
        "status": "running",
    }))
}

/// Stop a running swarm. Sets task status='stopped'.
#[pg_extern]
fn stop_swarm(task_id: pgrx::Uuid) -> pgrx::JsonB {
    let status = Spi::get_one::<String>(&format!(
        "SELECT status FROM kerai.tasks WHERE id = '{}'::uuid",
        task_id,
    ))
    .unwrap_or(None);

    match status.as_deref() {
        None => error!("Task not found: {}", task_id),
        Some("running") => {}
        Some(s) => error!("Task must be 'running' to stop, currently '{}'", s),
    }

    Spi::run(&format!(
        "UPDATE kerai.tasks SET status = 'stopped', updated_at = now() WHERE id = '{}'::uuid",
        task_id,
    ))
    .unwrap();

    pgrx::JsonB(serde_json::json!({
        "task_id": task_id.to_string(),
        "status": "stopped",
    }))
}

/// Record a test result for a task from a named agent.
#[pg_extern]
fn record_test_result(
    task_id: pgrx::Uuid,
    agent_name: &str,
    passed: bool,
    output: Option<&str>,
    duration_ms: Option<i32>,
    ops_count: Option<i32>,
) -> pgrx::JsonB {
    // Resolve agent by name
    let agent_id = Spi::get_one::<String>(&format!(
        "SELECT id::text FROM kerai.agents WHERE name = '{}'",
        sql_escape(agent_name),
    ))
    .unwrap_or(None);

    let aid = match agent_id {
        Some(id) => id,
        None => error!("Agent not found: {}", agent_name),
    };

    // Capture current version vector
    let vv = Spi::get_one::<pgrx::JsonB>(
        "SELECT COALESCE(
            jsonb_object_agg(author, max_seq),
            '{}'::jsonb
        ) FROM kerai.version_vector",
    )
    .unwrap()
    .unwrap_or_else(|| pgrx::JsonB(serde_json::json!({})));

    let output_sql = match output {
        Some(o) => format!("'{}'", sql_escape(o)),
        None => "NULL".to_string(),
    };
    let duration_sql = match duration_ms {
        Some(d) => d.to_string(),
        None => "NULL".to_string(),
    };
    let ops_sql = match ops_count {
        Some(o) => o.to_string(),
        None => "NULL".to_string(),
    };
    let vv_str = sql_escape(&vv.0.to_string());

    let result_id = Spi::get_one::<String>(&format!(
        "INSERT INTO kerai.test_results (task_id, agent_id, version_vector, passed, output, duration_ms, ops_count)
         VALUES ('{}'::uuid, '{}'::uuid, '{}'::jsonb, {}, {}, {}, {})
         RETURNING id::text",
        task_id,
        sql_escape(&aid),
        vv_str,
        passed,
        output_sql,
        duration_sql,
        ops_sql,
    ))
    .unwrap()
    .unwrap();

    pgrx::JsonB(serde_json::json!({
        "id": result_id,
        "task_id": task_id.to_string(),
        "agent_name": agent_name,
        "passed": passed,
        "duration_ms": duration_ms,
        "ops_count": ops_count,
    }))
}

/// Per-agent leaderboard for a task: pass/fail counts, rate, average duration.
#[pg_extern]
fn swarm_leaderboard(task_id: pgrx::Uuid) -> pgrx::JsonB {
    let json = Spi::get_one::<pgrx::JsonB>(&format!(
        "SELECT COALESCE(
            jsonb_agg(row_to_json(sub.*) ORDER BY sub.pass_rate DESC, sub.avg_duration_ms ASC),
            '[]'::jsonb
        )
        FROM (
            SELECT
                a.name AS agent_name,
                count(*) FILTER (WHERE tr.passed) AS pass_count,
                count(*) FILTER (WHERE NOT tr.passed) AS fail_count,
                count(*) AS total,
                round(100.0 * count(*) FILTER (WHERE tr.passed) / GREATEST(count(*), 1), 1) AS pass_rate,
                round(avg(tr.duration_ms)::numeric, 0) AS avg_duration_ms
            FROM kerai.test_results tr
            JOIN kerai.agents a ON tr.agent_id = a.id
            WHERE tr.task_id = '{}'::uuid
            GROUP BY a.name
        ) sub",
        task_id,
    ))
    .unwrap()
    .unwrap_or_else(|| pgrx::JsonB(serde_json::json!([])));
    json
}

/// Pass rate over time for a task, bucketed by minute.
#[pg_extern]
fn swarm_progress(task_id: pgrx::Uuid) -> pgrx::JsonB {
    let json = Spi::get_one::<pgrx::JsonB>(&format!(
        "SELECT COALESCE(
            jsonb_agg(row_to_json(sub.*) ORDER BY sub.bucket),
            '[]'::jsonb
        )
        FROM (
            SELECT
                date_trunc('minute', created_at) AS bucket,
                count(*) AS total,
                count(*) FILTER (WHERE passed) AS passed,
                count(*) FILTER (WHERE NOT passed) AS failed,
                round(100.0 * count(*) FILTER (WHERE passed) / GREATEST(count(*), 1), 1) AS pass_rate
            FROM kerai.test_results
            WHERE task_id = '{}'::uuid
            GROUP BY date_trunc('minute', created_at)
        ) sub",
        task_id,
    ))
    .unwrap()
    .unwrap_or_else(|| pgrx::JsonB(serde_json::json!([])));
    json
}

/// Task overview with test result counts. NULL task_id = all tasks.
#[pg_extern]
fn swarm_status(task_id: Option<pgrx::Uuid>) -> pgrx::JsonB {
    let where_clause = match task_id {
        Some(id) => format!("WHERE t.id = '{}'::uuid", id),
        None => String::new(),
    };

    let json = Spi::get_one::<pgrx::JsonB>(&format!(
        "SELECT COALESCE(
            jsonb_agg(row_to_json(sub.*) ORDER BY sub.created_at DESC),
            '[]'::jsonb
        )
        FROM (
            SELECT
                t.id AS task_id,
                t.description,
                t.status,
                t.agent_kind,
                t.agent_count,
                a.name AS swarm_name,
                count(tr.id) AS total_results,
                count(tr.id) FILTER (WHERE tr.passed) AS passed,
                count(tr.id) FILTER (WHERE NOT tr.passed) AS failed,
                t.created_at,
                t.updated_at
            FROM kerai.tasks t
            LEFT JOIN kerai.agents a ON t.swarm_id = a.id
            LEFT JOIN kerai.test_results tr ON tr.task_id = t.id
            {}
            GROUP BY t.id, t.description, t.status, t.agent_kind, t.agent_count,
                     a.name, t.created_at, t.updated_at
        ) sub",
        where_clause,
    ))
    .unwrap()
    .unwrap_or_else(|| pgrx::JsonB(serde_json::json!([])));
    json
}
