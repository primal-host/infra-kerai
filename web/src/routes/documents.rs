use axum::extract::{Path, State};
use axum::Json;
use serde::Deserialize;
use serde_json::Value;
use std::sync::Arc;

use crate::db::Pool;

#[derive(Deserialize)]
pub struct ParseMarkdownRequest {
    pub source: String,
    pub filename: String,
}

/// POST /api/documents — parse markdown into kerai nodes
pub async fn create_document(
    State(pool): State<Arc<Pool>>,
    Json(req): Json<ParseMarkdownRequest>,
) -> Result<Json<Value>, (axum::http::StatusCode, String)> {
    let client = pool.get().await.map_err(|e| {
        (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
    })?;

    let sql = format!(
        "SELECT kerai.parse_markdown('{}', '{}')",
        req.source.replace('\'', "''"),
        req.filename.replace('\'', "''"),
    );

    let row = client.query_one(&sql, &[]).await.map_err(|e| {
        (axum::http::StatusCode::BAD_REQUEST, e.to_string())
    })?;

    let result: Value = row.get(0);
    Ok(Json(result))
}

/// GET /api/documents — list document nodes
pub async fn list_documents(
    State(pool): State<Arc<Pool>>,
) -> Result<Json<Value>, (axum::http::StatusCode, String)> {
    let client = pool.get().await.map_err(|e| {
        (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
    })?;

    let sql = "SELECT COALESCE(jsonb_agg(jsonb_build_object(
        'id', id,
        'content', content,
        'metadata', metadata,
        'created_at', created_at
    ) ORDER BY created_at DESC), '[]'::jsonb)
    FROM kerai.nodes WHERE kind = 'document'";

    let row = client.query_one(sql, &[]).await.map_err(|e| {
        (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
    })?;

    let result: Value = row.get(0);
    Ok(Json(result))
}

/// GET /api/documents/:id/tree — get recursive document tree
pub async fn document_tree(
    State(pool): State<Arc<Pool>>,
    Path(doc_id): Path<String>,
) -> Result<Json<Value>, (axum::http::StatusCode, String)> {
    let client = pool.get().await.map_err(|e| {
        (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
    })?;

    // Recursive CTE to get the full tree
    let sql = format!(
        "WITH RECURSIVE tree AS (
            SELECT id, kind, content, parent_id, position, metadata, 0 AS depth
            FROM kerai.nodes WHERE id = '{}'::uuid
            UNION ALL
            SELECT n.id, n.kind, n.content, n.parent_id, n.position, n.metadata, t.depth + 1
            FROM kerai.nodes n
            JOIN tree t ON n.parent_id = t.id
        )
        SELECT COALESCE(jsonb_agg(jsonb_build_object(
            'id', id,
            'kind', kind,
            'content', content,
            'parent_id', parent_id,
            'position', position,
            'metadata', metadata,
            'depth', depth
        ) ORDER BY depth, position), '[]'::jsonb)
        FROM tree",
        doc_id.replace('\'', "''"),
    );

    let row = client.query_one(&sql, &[]).await.map_err(|e| {
        (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
    })?;

    let result: Value = row.get(0);
    Ok(Json(result))
}

/// GET /api/documents/:id/markdown — reconstruct markdown from nodes
pub async fn document_markdown(
    State(pool): State<Arc<Pool>>,
    Path(doc_id): Path<String>,
) -> Result<String, (axum::http::StatusCode, String)> {
    let client = pool.get().await.map_err(|e| {
        (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
    })?;

    let sql = format!(
        "SELECT kerai.reconstruct_markdown('{}'::uuid)",
        doc_id.replace('\'', "''"),
    );

    let row = client.query_one(&sql, &[]).await.map_err(|e| {
        (axum::http::StatusCode::BAD_REQUEST, e.to_string())
    })?;

    let result: String = row.get(0);
    Ok(result)
}
