use axum::extract::{Query, State};
use axum::Json;
use serde::Deserialize;
use serde_json::Value;
use std::sync::Arc;

use crate::db::Pool;

#[derive(Deserialize)]
pub struct SearchParams {
    pub q: String,
    pub kind: Option<String>,
    pub limit: Option<i32>,
}

#[derive(Deserialize)]
pub struct ContextSearchParams {
    pub text: String,
    pub agents: Option<String>, // comma-separated agent names
    pub limit: Option<i32>,
}

/// GET /api/search — full-text search
pub async fn search(
    State(pool): State<Arc<Pool>>,
    Query(params): Query<SearchParams>,
) -> Result<Json<Value>, (axum::http::StatusCode, String)> {
    let client = pool.get().await.map_err(|e| {
        (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
    })?;

    let kind_param = params.kind
        .map(|k| format!("'{}'", k.replace('\'', "''")))
        .unwrap_or_else(|| "NULL".to_string());

    let limit_param = params.limit
        .map(|l| l.to_string())
        .unwrap_or_else(|| "NULL".to_string());

    let sql = format!(
        "SELECT kerai.search('{}', {}, {})",
        params.q.replace('\'', "''"),
        kind_param,
        limit_param,
    );

    let row = client.query_one(&sql, &[]).await.map_err(|e| {
        (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
    })?;

    let result: Value = row.get(0);
    Ok(Json(result))
}

/// GET /api/suggest — context-aware search for AI suggestions
pub async fn suggest(
    State(pool): State<Arc<Pool>>,
    Query(params): Query<ContextSearchParams>,
) -> Result<Json<Value>, (axum::http::StatusCode, String)> {
    let client = pool.get().await.map_err(|e| {
        (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
    })?;

    let agents_param = params.agents
        .map(|a| {
            let names: Vec<String> = a.split(',')
                .map(|s| format!("\"{}\"", s.trim()))
                .collect();
            format!("'[{}]'::jsonb", names.join(","))
        })
        .unwrap_or_else(|| "NULL".to_string());

    let limit_param = params.limit
        .map(|l| l.to_string())
        .unwrap_or_else(|| "NULL".to_string());

    let sql = format!(
        "SELECT kerai.context_search('{}', {}, {})",
        params.text.replace('\'', "''"),
        agents_param,
        limit_param,
    );

    let row = client.query_one(&sql, &[]).await.map_err(|e| {
        (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
    })?;

    let result: Value = row.get(0);
    Ok(Json(result))
}
