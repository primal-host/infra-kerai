use axum::extract::{Path, Query, State};
use axum::Json;
use serde::Deserialize;
use serde_json::Value;
use std::sync::Arc;

use crate::db::Pool;

type ApiResult = Result<Json<Value>, (axum::http::StatusCode, String)>;

fn internal_err(e: impl std::fmt::Display) -> (axum::http::StatusCode, String) {
    (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

#[derive(Deserialize)]
pub struct CreateModelBody {
    pub agent: String,
    pub dim: Option<i32>,
    pub n_heads: Option<i32>,
    pub n_layers: Option<i32>,
    pub context_len: Option<i32>,
    pub scope: Option<String>,
}

/// POST /api/models — create a new model
pub async fn create_model(
    State(pool): State<Arc<Pool>>,
    Json(body): Json<CreateModelBody>,
) -> ApiResult {
    let client = pool.get().await.map_err(internal_err)?;
    let sql = format!(
        "SELECT kerai.create_model('{}', {}, {}, {}, {}, {})::text",
        body.agent.replace('\'', "''"),
        body.dim.map(|v| v.to_string()).unwrap_or("NULL".into()),
        body.n_heads.map(|v| v.to_string()).unwrap_or("NULL".into()),
        body.n_layers.map(|v| v.to_string()).unwrap_or("NULL".into()),
        body.context_len.map(|v| v.to_string()).unwrap_or("NULL".into()),
        body.scope.as_ref().map(|s| format!("'{}'", s.replace('\'', "''"))).unwrap_or("NULL".into()),
    );
    let row = client.query_one(&sql, &[]).await.map_err(internal_err)?;
    let text: String = row.get(0);
    let value: Value = serde_json::from_str(&text).map_err(internal_err)?;
    Ok(Json(value))
}

#[derive(Deserialize)]
pub struct TrainModelBody {
    pub agent: String,
    pub walk_type: Option<String>,
    pub n_sequences: Option<i32>,
    pub n_steps: Option<i32>,
    pub lr: Option<f64>,
    pub scope: Option<String>,
    pub perspective_agent: Option<String>,
}

/// POST /api/models/train — train a model
pub async fn train_model(
    State(pool): State<Arc<Pool>>,
    Json(body): Json<TrainModelBody>,
) -> ApiResult {
    let client = pool.get().await.map_err(internal_err)?;
    let sql = format!(
        "SELECT kerai.train_model('{}', {}, {}, {}, {}, {}, {})::text",
        body.agent.replace('\'', "''"),
        body.walk_type.as_ref().map(|s| format!("'{}'", s.replace('\'', "''"))).unwrap_or("NULL".into()),
        body.n_sequences.map(|v| v.to_string()).unwrap_or("NULL".into()),
        body.n_steps.map(|v| v.to_string()).unwrap_or("NULL".into()),
        body.lr.map(|v| v.to_string()).unwrap_or("NULL".into()),
        body.scope.as_ref().map(|s| format!("'{}'", s.replace('\'', "''"))).unwrap_or("NULL".into()),
        body.perspective_agent.as_ref().map(|s| format!("'{}'", s.replace('\'', "''"))).unwrap_or("NULL".into()),
    );
    let row = client.query_one(&sql, &[]).await.map_err(internal_err)?;
    let text: String = row.get(0);
    let value: Value = serde_json::from_str(&text).map_err(internal_err)?;
    Ok(Json(value))
}

#[derive(Deserialize)]
pub struct PredictBody {
    pub agent: String,
    pub context: Vec<String>,
    pub top_k: Option<i32>,
}

/// POST /api/models/predict — predict next nodes
pub async fn predict_next(
    State(pool): State<Arc<Pool>>,
    Json(body): Json<PredictBody>,
) -> ApiResult {
    let client = pool.get().await.map_err(internal_err)?;
    let context_json = serde_json::json!(body.context).to_string();
    let sql = format!(
        "SELECT kerai.predict_next('{}', '{}'::jsonb, {})::text",
        body.agent.replace('\'', "''"),
        context_json.replace('\'', "''"),
        body.top_k.map(|v| v.to_string()).unwrap_or("NULL".into()),
    );
    let row = client.query_one(&sql, &[]).await.map_err(internal_err)?;
    let text: String = row.get(0);
    let value: Value = serde_json::from_str(&text).map_err(internal_err)?;
    Ok(Json(value))
}

#[derive(Deserialize)]
pub struct NeuralSearchParams {
    pub agent: String,
    pub q: String,
    pub limit: Option<i32>,
}

/// GET /api/models/search — neural-enhanced search
pub async fn neural_search(
    State(pool): State<Arc<Pool>>,
    Query(params): Query<NeuralSearchParams>,
) -> ApiResult {
    let client = pool.get().await.map_err(internal_err)?;
    let sql = format!(
        "SELECT kerai.neural_search('{}', '{}', NULL, {})::text",
        params.agent.replace('\'', "''"),
        params.q.replace('\'', "''"),
        params.limit.map(|v| v.to_string()).unwrap_or("NULL".into()),
    );
    let row = client.query_one(&sql, &[]).await.map_err(internal_err)?;
    let text: String = row.get(0);
    let value: Value = serde_json::from_str(&text).map_err(internal_err)?;
    Ok(Json(value))
}

#[derive(Deserialize)]
pub struct EnsembleBody {
    pub agents: Vec<String>,
    pub context: Vec<String>,
    pub top_k: Option<i32>,
}

/// POST /api/models/ensemble — ensemble prediction
pub async fn ensemble_predict(
    State(pool): State<Arc<Pool>>,
    Json(body): Json<EnsembleBody>,
) -> ApiResult {
    let client = pool.get().await.map_err(internal_err)?;
    let agents_json = serde_json::json!(body.agents).to_string();
    let context_json = serde_json::json!(body.context).to_string();
    let sql = format!(
        "SELECT kerai.ensemble_predict('{}'::jsonb, '{}'::jsonb, {})::text",
        agents_json.replace('\'', "''"),
        context_json.replace('\'', "''"),
        body.top_k.map(|v| v.to_string()).unwrap_or("NULL".into()),
    );
    let row = client.query_one(&sql, &[]).await.map_err(internal_err)?;
    let text: String = row.get(0);
    let value: Value = serde_json::from_str(&text).map_err(internal_err)?;
    Ok(Json(value))
}

/// GET /api/models/:agent/info — model info
pub async fn model_info(
    State(pool): State<Arc<Pool>>,
    Path(agent): Path<String>,
) -> ApiResult {
    let client = pool.get().await.map_err(internal_err)?;
    let sql = format!(
        "SELECT kerai.model_info('{}')::text",
        agent.replace('\'', "''"),
    );
    let row = client.query_one(&sql, &[]).await.map_err(internal_err)?;
    let text: String = row.get(0);
    let value: Value = serde_json::from_str(&text).map_err(internal_err)?;
    Ok(Json(value))
}

/// DELETE /api/models/:agent — delete model
pub async fn delete_model(
    State(pool): State<Arc<Pool>>,
    Path(agent): Path<String>,
) -> ApiResult {
    let client = pool.get().await.map_err(internal_err)?;
    let sql = format!(
        "SELECT kerai.delete_model('{}')::text",
        agent.replace('\'', "''"),
    );
    let row = client.query_one(&sql, &[]).await.map_err(internal_err)?;
    let text: String = row.get(0);
    let value: Value = serde_json::from_str(&text).map_err(internal_err)?;
    Ok(Json(value))
}

#[derive(Deserialize)]
pub struct FeedbackBody {
    pub inference_id: String,
}

/// POST /api/models/feedback — record selection
pub async fn record_selection(
    State(pool): State<Arc<Pool>>,
    Json(body): Json<FeedbackBody>,
) -> ApiResult {
    let client = pool.get().await.map_err(internal_err)?;
    let sql = format!(
        "SELECT kerai.record_selection('{}'::uuid)::text",
        body.inference_id.replace('\'', "''"),
    );
    let row = client.query_one(&sql, &[]).await.map_err(internal_err)?;
    let text: String = row.get(0);
    let value: Value = serde_json::from_str(&text).map_err(internal_err)?;
    Ok(Json(value))
}
