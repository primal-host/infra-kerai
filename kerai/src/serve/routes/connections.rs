use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

use crate::lang::ptr::Ptr;
use crate::serve::auth;
use crate::serve::db::Pool;

#[derive(Serialize)]
pub struct WorkspaceEntry {
    id: String,
    name: String,
    item_count: i32,
    is_active: bool,
}

#[derive(Serialize)]
pub struct ConnectionsResponse {
    pg_host: String,
    handle: String,
    workspaces: Vec<WorkspaceEntry>,
    current_workspace_id: String,
}

/// GET /api/connections — returns tree data for the connections panel.
pub async fn connections(
    State(pool): State<Arc<Pool>>,
    headers: HeaderMap,
) -> Result<Json<ConnectionsResponse>, (StatusCode, String)> {
    let token = auth::extract_session_token(&headers)
        .ok_or_else(|| (StatusCode::UNAUTHORIZED, "no session".into()))?;

    let (user_id, workspace_id) = auth::resolve_session(&pool, &token)
        .await
        .map_err(|e| (StatusCode::UNAUTHORIZED, e))?;

    let client = pool.get().await.map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
    })?;

    // Get handle
    let handle: String = client
        .query_one("SELECT COALESCE(handle, 'anonymous') FROM kerai.users WHERE id = $1", &[&user_id])
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .get(0);

    // Get workspaces with item counts (same query as eval.rs workspace_list_request)
    let rows = client
        .query(
            "SELECT w.id, w.name, w.is_active, \
             COALESCE((SELECT COUNT(*)::int FROM kerai.stack_items si WHERE si.workspace_id = w.id), 0) AS item_count \
             FROM kerai.workspaces w \
             WHERE w.user_id = $1 \
             ORDER BY w.updated_at DESC",
            &[&user_id],
        )
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let workspaces: Vec<WorkspaceEntry> = rows
        .iter()
        .map(|r| WorkspaceEntry {
            id: r.get::<_, Uuid>(0).to_string(),
            name: r.get::<_, String>(1),
            is_active: r.get::<_, bool>(2),
            item_count: r.get::<_, i32>(3),
        })
        .collect();

    Ok(Json(ConnectionsResponse {
        pg_host: pool.pg_host().to_string(),
        handle,
        workspaces,
        current_workspace_id: workspace_id.to_string(),
    }))
}

#[derive(Deserialize)]
pub struct SwitchRequest {
    workspace_id: String,
    session_token: String,
}

#[derive(Serialize)]
pub struct SwitchResponse {
    workspace_name: String,
    stack: Vec<Ptr>,
}

/// POST /api/workspace/switch — switches active workspace by UUID.
pub async fn switch_workspace(
    State(pool): State<Arc<Pool>>,
    Json(req): Json<SwitchRequest>,
) -> Result<Json<SwitchResponse>, (StatusCode, String)> {
    let (user_id, old_workspace_id) = auth::resolve_session(&pool, &req.session_token)
        .await
        .map_err(|e| (StatusCode::UNAUTHORIZED, e))?;

    let ws_id: Uuid = req.workspace_id.parse().map_err(|_| {
        (StatusCode::BAD_REQUEST, "invalid workspace_id".into())
    })?;

    let client = pool.get().await.map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
    })?;

    // Verify workspace belongs to user
    let ws_row = client
        .query_opt(
            "SELECT name FROM kerai.workspaces WHERE id = $1 AND user_id = $2",
            &[&ws_id, &user_id],
        )
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "workspace not found".into()))?;

    let workspace_name: String = ws_row.get(0);

    // Deactivate all → activate target
    client
        .execute(
            "UPDATE kerai.workspaces SET is_active = false WHERE user_id = $1 AND is_active = true",
            &[&user_id],
        )
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    client
        .execute(
            "UPDATE kerai.workspaces SET is_active = true, updated_at = now() WHERE id = $1",
            &[&ws_id],
        )
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // Update session to point to new workspace
    client
        .execute(
            "UPDATE kerai.sessions SET workspace_id = $1 WHERE user_id = $2 AND workspace_id = $3",
            &[&ws_id, &user_id, &old_workspace_id],
        )
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // Load new stack
    let rows = client
        .query(
            "SELECT id, position, kind, ref_id, meta \
             FROM kerai.stack_items \
             WHERE workspace_id = $1 \
             ORDER BY position ASC",
            &[&ws_id],
        )
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let stack: Vec<Ptr> = rows
        .iter()
        .map(|r| Ptr {
            id: r.get::<_, i64>(0),
            kind: r.get::<_, String>(2),
            ref_id: r.get::<_, String>(3),
            meta: r.get::<_, serde_json::Value>(4),
        })
        .collect();

    Ok(Json(SwitchResponse {
        workspace_name,
        stack,
    }))
}
