use axum::extract::{Path, State};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

use crate::db::Pool;

#[derive(Deserialize)]
pub struct ApplyOpRequest {
    pub op_type: String,
    pub node_id: Option<String>,
    pub payload: Value,
}

#[derive(Deserialize)]
pub struct UpdateContentRequest {
    pub content: String,
}

/// POST /api/nodes — apply a CRDT operation
pub async fn create_node(
    State(pool): State<Arc<Pool>>,
    Json(req): Json<ApplyOpRequest>,
) -> Result<Json<Value>, (axum::http::StatusCode, String)> {
    let client = pool.get().await.map_err(|e| {
        (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
    })?;

    let node_id_param = req.node_id
        .map(|id| format!("'{}'::uuid", id.replace('\'', "''")))
        .unwrap_or_else(|| "NULL".to_string());

    let sql = format!(
        "SELECT kerai.apply_op('{}', {}, '{}'::jsonb)",
        req.op_type.replace('\'', "''"),
        node_id_param,
        req.payload.to_string().replace('\'', "''"),
    );

    let row = client.query_one(&sql, &[]).await.map_err(|e| {
        (axum::http::StatusCode::BAD_REQUEST, e.to_string())
    })?;

    let result: Value = row.get(0);
    Ok(Json(result))
}

/// PATCH /api/nodes/:id/content — update node content
pub async fn update_content(
    State(pool): State<Arc<Pool>>,
    Path(node_id): Path<String>,
    Json(req): Json<UpdateContentRequest>,
) -> Result<Json<Value>, (axum::http::StatusCode, String)> {
    let client = pool.get().await.map_err(|e| {
        (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
    })?;

    let payload = json!({"new_content": req.content});
    let sql = format!(
        "SELECT kerai.apply_op('update_content', '{}'::uuid, '{}'::jsonb)",
        node_id.replace('\'', "''"),
        payload.to_string().replace('\'', "''"),
    );

    let row = client.query_one(&sql, &[]).await.map_err(|e| {
        (axum::http::StatusCode::BAD_REQUEST, e.to_string())
    })?;

    let result: Value = row.get(0);
    Ok(Json(result))
}

/// POST /api/nodes/:id/move — move a node
pub async fn move_node(
    State(pool): State<Arc<Pool>>,
    Path(node_id): Path<String>,
    Json(payload): Json<Value>,
) -> Result<Json<Value>, (axum::http::StatusCode, String)> {
    let client = pool.get().await.map_err(|e| {
        (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
    })?;

    let sql = format!(
        "SELECT kerai.apply_op('move_node', '{}'::uuid, '{}'::jsonb)",
        node_id.replace('\'', "''"),
        payload.to_string().replace('\'', "''"),
    );

    let row = client.query_one(&sql, &[]).await.map_err(|e| {
        (axum::http::StatusCode::BAD_REQUEST, e.to_string())
    })?;

    let result: Value = row.get(0);
    Ok(Json(result))
}

/// DELETE /api/nodes/:id — delete a node
pub async fn delete_node(
    State(pool): State<Arc<Pool>>,
    Path(node_id): Path<String>,
) -> Result<Json<Value>, (axum::http::StatusCode, String)> {
    let client = pool.get().await.map_err(|e| {
        (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
    })?;

    let sql = format!(
        "SELECT kerai.apply_op('delete_node', '{}'::uuid, '{{\"cascade\": false}}'::jsonb)",
        node_id.replace('\'', "''"),
    );

    let row = client.query_one(&sql, &[]).await.map_err(|e| {
        (axum::http::StatusCode::BAD_REQUEST, e.to_string())
    })?;

    let result: Value = row.get(0);
    Ok(Json(result))
}
