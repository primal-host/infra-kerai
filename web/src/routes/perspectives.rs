use axum::extract::{Query, State};
use axum::Json;
use serde::Deserialize;
use serde_json::Value;
use std::sync::Arc;

use crate::db::Pool;

#[derive(Deserialize)]
pub struct PerspectiveParams {
    pub agent: String,
    pub context_id: Option<String>,
    pub min_weight: Option<f64>,
}

#[derive(Deserialize)]
pub struct ConsensusParams {
    pub context_id: Option<String>,
    pub min_agents: Option<i32>,
    pub min_weight: Option<f64>,
}

/// GET /api/perspectives — get agent perspectives
pub async fn get_perspectives(
    State(pool): State<Arc<Pool>>,
    Query(params): Query<PerspectiveParams>,
) -> Result<Json<Value>, (axum::http::StatusCode, String)> {
    let client = pool.get().await.map_err(|e| {
        (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
    })?;

    let ctx_param = params.context_id
        .map(|c| format!("'{}'::uuid", c.replace('\'', "''")))
        .unwrap_or_else(|| "NULL".to_string());

    let weight_param = params.min_weight
        .map(|w| w.to_string())
        .unwrap_or_else(|| "NULL".to_string());

    let sql = format!(
        "SELECT kerai.get_perspectives('{}', {}, {})",
        params.agent.replace('\'', "''"),
        ctx_param,
        weight_param,
    );

    let row = client.query_one(&sql, &[]).await.map_err(|e| {
        (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
    })?;

    let result: Value = row.get(0);
    Ok(Json(result))
}

/// GET /api/consensus — get multi-agent consensus
pub async fn consensus(
    State(pool): State<Arc<Pool>>,
    Query(params): Query<ConsensusParams>,
) -> Result<Json<Value>, (axum::http::StatusCode, String)> {
    let client = pool.get().await.map_err(|e| {
        (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
    })?;

    let ctx_param = params.context_id
        .map(|c| format!("'{}'::uuid", c.replace('\'', "''")))
        .unwrap_or_else(|| "NULL".to_string());

    let agents_param = params.min_agents
        .map(|a| a.to_string())
        .unwrap_or_else(|| "NULL".to_string());

    let weight_param = params.min_weight
        .map(|w| w.to_string())
        .unwrap_or_else(|| "NULL".to_string());

    let sql = format!(
        "SELECT kerai.consensus({}, {}, {})",
        ctx_param,
        agents_param,
        weight_param,
    );

    let row = client.query_one(&sql, &[]).await.map_err(|e| {
        (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
    })?;

    let result: Value = row.get(0);
    Ok(Json(result))
}
