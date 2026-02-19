/// Consensus queries — multi-agent agreement, diffs, and unique insights.
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

/// Multi-agent consensus on nodes. Returns aggregated weight stats
/// for nodes rated by multiple agents, optionally filtered.
#[pg_extern]
fn consensus(
    context_id: Option<pgrx::Uuid>,
    min_agents: Option<i32>,
    min_weight: Option<f64>,
) -> pgrx::JsonB {
    let min_a = min_agents.unwrap_or(2);
    let min_w = min_weight.unwrap_or(-1.0);

    let mut conditions = vec![
        format!("c.agent_count >= {}", min_a),
        format!("c.avg_weight >= {}", min_w),
    ];

    if let Some(ctx) = context_id {
        conditions.push(format!("c.context_id = '{}'::uuid", ctx));
    }

    let where_clause = conditions.join(" AND ");

    let json = Spi::get_one::<pgrx::JsonB>(&format!(
        "SELECT COALESCE(
            jsonb_agg(jsonb_build_object(
                'node_id', c.node_id,
                'context_id', c.context_id,
                'agent_count', c.agent_count,
                'avg_weight', c.avg_weight,
                'min_weight', c.min_weight,
                'max_weight', c.max_weight,
                'stddev_weight', c.stddev_weight,
                'node_kind', n.kind,
                'node_content', n.content
            ) ORDER BY c.avg_weight DESC),
            '[]'::jsonb
        ) FROM kerai.consensus_perspectives c
        JOIN kerai.nodes n ON n.id = c.node_id
        WHERE {}",
        where_clause,
    ))
    .unwrap()
    .unwrap_or_else(|| pgrx::JsonB(serde_json::json!([])));
    json
}

/// Compare two agents' perspectives. Returns nodes only in agent1,
/// only in agent2, and disagreements (same node, different weights).
#[pg_extern]
fn perspective_diff(
    agent1_name: &str,
    agent2_name: &str,
    context_id: Option<pgrx::Uuid>,
) -> pgrx::JsonB {
    let a1_id = resolve_agent(agent1_name);
    let a2_id = resolve_agent(agent2_name);

    let ctx_clause = match context_id {
        Some(c) => format!("AND context_id = '{}'::uuid", c),
        None => String::new(),
    };

    // Nodes only in agent1
    let only_a = Spi::get_one::<pgrx::JsonB>(&format!(
        "SELECT COALESCE(
            jsonb_agg(jsonb_build_object(
                'node_id', p.node_id,
                'weight', p.weight,
                'node_kind', n.kind,
                'node_content', n.content
            )),
            '[]'::jsonb
        ) FROM kerai.perspectives p
        JOIN kerai.nodes n ON n.id = p.node_id
        WHERE p.agent_id = '{}'::uuid {}
          AND NOT EXISTS (
            SELECT 1 FROM kerai.perspectives p2
            WHERE p2.agent_id = '{}'::uuid AND p2.node_id = p.node_id
              AND p2.context_id IS NOT DISTINCT FROM p.context_id
          )",
        sql_escape(&a1_id),
        ctx_clause,
        sql_escape(&a2_id),
    ))
    .unwrap()
    .unwrap_or_else(|| pgrx::JsonB(serde_json::json!([])));

    // Nodes only in agent2
    let only_b = Spi::get_one::<pgrx::JsonB>(&format!(
        "SELECT COALESCE(
            jsonb_agg(jsonb_build_object(
                'node_id', p.node_id,
                'weight', p.weight,
                'node_kind', n.kind,
                'node_content', n.content
            )),
            '[]'::jsonb
        ) FROM kerai.perspectives p
        JOIN kerai.nodes n ON n.id = p.node_id
        WHERE p.agent_id = '{}'::uuid {}
          AND NOT EXISTS (
            SELECT 1 FROM kerai.perspectives p2
            WHERE p2.agent_id = '{}'::uuid AND p2.node_id = p.node_id
              AND p2.context_id IS NOT DISTINCT FROM p.context_id
          )",
        sql_escape(&a2_id),
        ctx_clause,
        sql_escape(&a1_id),
    ))
    .unwrap()
    .unwrap_or_else(|| pgrx::JsonB(serde_json::json!([])));

    // Disagreements — same node, different weights
    let disagreements = Spi::get_one::<pgrx::JsonB>(&format!(
        "SELECT COALESCE(
            jsonb_agg(jsonb_build_object(
                'node_id', p1.node_id,
                'weight_a', p1.weight,
                'weight_b', p2.weight,
                'diff', abs(p1.weight - p2.weight),
                'node_kind', n.kind,
                'node_content', n.content
            ) ORDER BY abs(p1.weight - p2.weight) DESC),
            '[]'::jsonb
        ) FROM kerai.perspectives p1
        JOIN kerai.perspectives p2 ON p1.node_id = p2.node_id
          AND p1.context_id IS NOT DISTINCT FROM p2.context_id
        JOIN kerai.nodes n ON n.id = p1.node_id
        WHERE p1.agent_id = '{}'::uuid AND p2.agent_id = '{}'::uuid {}
          AND p1.weight != p2.weight",
        sql_escape(&a1_id),
        sql_escape(&a2_id),
        ctx_clause,
    ))
    .unwrap()
    .unwrap_or_else(|| pgrx::JsonB(serde_json::json!([])));

    pgrx::JsonB(serde_json::json!({
        "agent_a": agent1_name,
        "agent_b": agent2_name,
        "only_in_a": only_a.0,
        "only_in_b": only_b.0,
        "disagreements": disagreements.0,
    }))
}

/// Associations this agent has that no other agent does.
#[pg_extern]
fn unique_insights(agent_name: &str) -> pgrx::JsonB {
    let agent_id = resolve_agent(agent_name);

    let json = Spi::get_one::<pgrx::JsonB>(&format!(
        "SELECT COALESCE(
            jsonb_agg(jsonb_build_object(
                'id', a.id,
                'source_id', a.source_id,
                'target_id', a.target_id,
                'weight', a.weight,
                'relation', a.relation,
                'reasoning', a.reasoning,
                'source_kind', ns.kind,
                'source_content', ns.content,
                'target_kind', nt.kind,
                'target_content', nt.content
            ) ORDER BY a.weight DESC),
            '[]'::jsonb
        ) FROM kerai.unique_associations a
        JOIN kerai.nodes ns ON ns.id = a.source_id
        JOIN kerai.nodes nt ON nt.id = a.target_id
        WHERE a.agent_id = '{}'::uuid",
        sql_escape(&agent_id),
    ))
    .unwrap()
    .unwrap_or_else(|| pgrx::JsonB(serde_json::json!([])));
    json
}
