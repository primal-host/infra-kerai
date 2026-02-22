use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse};
use axum::Json;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::lang::handlers;
use crate::lang::machine::Machine;
use crate::lang::ptr::Ptr;
use crate::serve::auth;
use crate::serve::db::Pool;
use crate::serve::oauth::{self, OAuthConfig};

#[derive(Deserialize)]
pub struct EvalRequest {
    input: String,
    #[serde(default)]
    session_token: String,
}

#[derive(Serialize)]
pub struct EvalResponse {
    stack: Vec<Ptr>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

pub async fn eval(
    State(pool): State<Arc<Pool>>,
    Json(req): Json<EvalRequest>,
) -> (StatusCode, Json<EvalResponse>) {
    // Resolve session
    let (user_id, workspace_id) = match auth::resolve_session(&pool, &req.session_token).await {
        Ok(ids) => ids,
        Err(e) => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(EvalResponse {
                    stack: vec![],
                    error: Some(e),
                }),
            );
        }
    };

    // Build machine with handlers
    let (handler_map, type_methods, help) = handlers::register_all();
    let mut machine = Machine::new(workspace_id, user_id, handler_map, type_methods, help);

    // Load current stack from DB
    match load_stack(&pool, workspace_id).await {
        Ok(stack) => machine.stack = stack,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(EvalResponse {
                    stack: vec![],
                    error: Some(format!("failed to load stack: {e}")),
                }),
            );
        }
    }

    // Execute input
    let exec_error = match machine.execute(&req.input) {
        Ok(()) => None,
        Err(e) => Some(e),
    };

    // Process any request markers left on the stack by handlers
    resolve_requests(&mut machine, &pool).await;

    // Save stack back to DB
    if let Err(e) = save_stack(&pool, machine.workspace_id, &machine.stack).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(EvalResponse {
                stack: machine.stack,
                error: Some(format!("failed to save stack: {e}")),
            }),
        );
    }

    // Reload to get stable rowids
    match load_stack(&pool, machine.workspace_id).await {
        Ok(stack) => machine.stack = stack,
        Err(_) => {} // Best effort
    }

    (
        StatusCode::OK,
        Json(EvalResponse {
            stack: machine.stack,
            error: exec_error,
        }),
    )
}

/// Load stack items from the database for a workspace.
async fn load_stack(pool: &Pool, workspace_id: uuid::Uuid) -> Result<Vec<Ptr>, String> {
    let client = pool.get().await.map_err(|e| e.to_string())?;

    let rows = client
        .query(
            "SELECT id, position, kind, ref_id, meta \
             FROM kerai.stack_items \
             WHERE workspace_id = $1 \
             ORDER BY position ASC",
            &[&workspace_id],
        )
        .await
        .map_err(|e| e.to_string())?;

    Ok(rows
        .iter()
        .map(|row| Ptr {
            id: row.get::<_, i64>(0),
            kind: row.get::<_, String>(2),
            ref_id: row.get::<_, String>(3),
            meta: row.get::<_, serde_json::Value>(4),
        })
        .collect())
}

/// Save stack items to the database (full replacement).
async fn save_stack(pool: &Pool, workspace_id: uuid::Uuid, stack: &[Ptr]) -> Result<(), String> {
    let client = pool.get().await.map_err(|e| e.to_string())?;

    // Delete all existing items for this workspace
    client
        .execute(
            "DELETE FROM kerai.stack_items WHERE workspace_id = $1",
            &[&workspace_id],
        )
        .await
        .map_err(|e| e.to_string())?;

    // Re-insert with correct positions
    for (pos, ptr) in stack.iter().enumerate() {
        let pos_i32 = pos as i32;
        client
            .execute(
                "INSERT INTO kerai.stack_items (workspace_id, position, kind, ref_id, meta) \
                 VALUES ($1, $2, $3, $4, $5)",
                &[
                    &workspace_id,
                    &pos_i32,
                    &ptr.kind,
                    &ptr.ref_id,
                    &ptr.meta,
                ],
            )
            .await
            .map_err(|e| e.to_string())?;
    }

    Ok(())
}

/// Resolve request markers left on the stack by handlers.
async fn resolve_requests(machine: &mut Machine, pool: &Pool) {
    let client = match pool.get().await {
        Ok(c) => c,
        Err(_) => return,
    };

    let mut i = 0;
    while i < machine.stack.len() {
        match machine.stack[i].kind.as_str() {
            "workspace_list_request" => {
                let user_id = machine.user_id;
                match client
                    .query(
                        "SELECT w.id, w.name, w.is_active, \
                         COALESCE((SELECT COUNT(*)::int FROM kerai.stack_items si WHERE si.workspace_id = w.id), 0) AS item_count, \
                         w.updated_at::text \
                         FROM kerai.workspaces w \
                         WHERE w.user_id = $1 \
                         ORDER BY w.updated_at DESC",
                        &[&user_id],
                    )
                    .await
                {
                    Ok(rows) => {
                        let items: Vec<serde_json::Value> = rows
                            .iter()
                            .map(|r| {
                                serde_json::json!({
                                    "id": r.get::<_, uuid::Uuid>(0).to_string(),
                                    "name": r.get::<_, String>(1),
                                    "is_active": r.get::<_, bool>(2),
                                    "item_count": r.get::<_, i32>(3),
                                    "updated_at": r.get::<_, String>(4),
                                })
                            })
                            .collect();

                        machine.stack[i] = Ptr {
                            kind: "workspace_list".into(),
                            ref_id: String::new(),
                            meta: serde_json::json!({"items": items}),
                            id: 0,
                        };
                    }
                    Err(e) => {
                        machine.stack[i] = Ptr::error(&format!("workspace list failed: {e}"));
                    }
                }
            }
            "workspace_new_request" => {
                let name = machine.stack[i].ref_id.clone();
                let user_id = machine.user_id;
                match client
                    .query_one(
                        "INSERT INTO kerai.workspaces (user_id, name) VALUES ($1, $2) RETURNING id",
                        &[&user_id, &name],
                    )
                    .await
                {
                    Ok(row) => {
                        let ws_id: uuid::Uuid = row.get(0);
                        machine.stack[i] = Ptr {
                            kind: "text".into(),
                            ref_id: format!("workspace '{}' created ({})", name, &ws_id.to_string()[..8]),
                            meta: serde_json::Value::Null,
                            id: 0,
                        };
                    }
                    Err(e) => {
                        machine.stack[i] = Ptr::error(&format!("workspace new failed: {e}"));
                    }
                }
            }
            "workspace_save_request" => {
                let name = machine.stack[i].ref_id.clone();
                match client
                    .execute(
                        "UPDATE kerai.workspaces SET name = $1, is_anonymous = false, updated_at = now() \
                         WHERE id = $2",
                        &[&name, &machine.workspace_id],
                    )
                    .await
                {
                    Ok(_) => {
                        machine.stack[i] = Ptr {
                            kind: "text".into(),
                            ref_id: format!("workspace saved as '{}'", name),
                            meta: serde_json::Value::Null,
                            id: 0,
                        };
                    }
                    Err(e) => {
                        machine.stack[i] = Ptr::error(&format!("workspace save failed: {e}"));
                    }
                }
            }
            "workspace_load_request" => {
                let selection: i64 = machine.stack[i].ref_id.parse().unwrap_or(0);

                // Find the most recent workspace_list on the stack
                let mut target_ws_id: Option<String> = None;
                for j in (0..i).rev() {
                    if machine.stack[j].kind == "workspace_list" {
                        if let Some(items) = machine.stack[j].meta.get("items").and_then(|v| v.as_array()) {
                            let idx = (selection - 1) as usize;
                            if let Some(item) = items.get(idx) {
                                target_ws_id = item.get("id").and_then(|v| v.as_str()).map(|s| s.to_string());
                            }
                        }
                        break;
                    }
                }

                match target_ws_id {
                    Some(ws_id_str) => {
                        if let Ok(ws_id) = ws_id_str.parse::<uuid::Uuid>() {
                            // Activate the workspace
                            let _ = client
                                .execute(
                                    "UPDATE kerai.workspaces SET is_active = false \
                                     WHERE user_id = $1 AND is_active = true",
                                    &[&machine.user_id],
                                )
                                .await;
                            let _ = client
                                .execute(
                                    "UPDATE kerai.workspaces SET is_active = true, updated_at = now() \
                                     WHERE id = $1",
                                    &[&ws_id],
                                )
                                .await;

                            // Update session to point to new workspace
                            let _ = client
                                .execute(
                                    "UPDATE kerai.sessions SET workspace_id = $1 \
                                     WHERE user_id = $2 AND workspace_id = $3",
                                    &[&ws_id, &machine.user_id, &machine.workspace_id],
                                )
                                .await;

                            // Switch the machine's workspace and reload stack
                            machine.workspace_id = ws_id;
                            machine.stack.clear();
                            if let Ok(rows) = client
                                .query(
                                    "SELECT id, position, kind, ref_id, meta \
                                     FROM kerai.stack_items \
                                     WHERE workspace_id = $1 \
                                     ORDER BY position ASC",
                                    &[&ws_id],
                                )
                                .await
                            {
                                machine.stack = rows
                                    .iter()
                                    .map(|r| Ptr {
                                        id: r.get::<_, i64>(0),
                                        kind: r.get::<_, String>(2),
                                        ref_id: r.get::<_, String>(3),
                                        meta: r.get::<_, serde_json::Value>(4),
                                    })
                                    .collect();
                            }
                            return; // Stack is replaced, no more processing needed
                        } else {
                            machine.stack[i] = Ptr::error("workspace load: invalid workspace id");
                        }
                    }
                    None => {
                        machine.stack[i] = Ptr::error(&format!(
                            "workspace load: selection {} not found (run 'workspace list' first)",
                            selection
                        ));
                    }
                }
            }
            "auth_pending_request" => {
                // Load OAuth config from DB
                let config_rows = client
                    .query(
                        "SELECT key, value FROM kerai.config WHERE key LIKE 'oauth.bsky.%' OR key = 'public_url'",
                        &[],
                    )
                    .await;

                match config_rows {
                    Ok(rows) if !rows.is_empty() => {
                        let pairs: Vec<(String, String)> = rows
                            .iter()
                            .map(|r| (r.get::<_, String>(0), r.get::<_, String>(1)))
                            .collect();

                        match OAuthConfig::from_config_rows(&pairs) {
                            Ok(config) => {
                                // Go directly to bsky.social auth server (no handle needed)
                                match oauth::discover_auth_server_from_pds("https://bsky.social").await {
                                    Ok(auth_meta) => {
                                        let (code_verifier, code_challenge) = oauth::generate_pkce();
                                        let state = oauth::generate_state();
                                        let (dpop_key, dpop_key_b64) = oauth::generate_dpop_key();

                                        match oauth::pushed_auth_request(&config, &auth_meta, &code_challenge, &state, &dpop_key).await {
                                            Ok(authorize_url) => {
                                                // Find the actual session token
                                                let token_row = client
                                                    .query_opt(
                                                        "SELECT token FROM kerai.sessions WHERE user_id = $1 AND workspace_id = $2",
                                                        &[&machine.user_id, &machine.workspace_id],
                                                    )
                                                    .await;

                                                let sess_token = match token_row {
                                                    Ok(Some(r)) => r.get::<_, String>(0),
                                                    _ => String::new(),
                                                };

                                                // Store oauth state with ephemeral DPoP key
                                                let _ = client
                                                    .execute(
                                                        "INSERT INTO kerai.oauth_state (state, code_verifier, session_token, token_endpoint, issuer, dpop_key) \
                                                         VALUES ($1, $2, $3, $4, $5, $6)",
                                                        &[&state, &code_verifier, &sess_token, &auth_meta.token_endpoint, &auth_meta.issuer, &dpop_key_b64],
                                                    )
                                                    .await;

                                                machine.stack[i] = Ptr {
                                                    kind: "auth_pending".into(),
                                                    ref_id: "bsky".into(),
                                                    meta: serde_json::json!({
                                                        "url": authorize_url,
                                                    }),
                                                    id: 0,
                                                };
                                            }
                                            Err(e) => {
                                                machine.stack[i] = Ptr::error(&format!("PAR failed: {e}"));
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        machine.stack[i] = Ptr::error(&format!("auth server discovery failed: {e}"));
                                    }
                                }
                            }
                            Err(e) => {
                                machine.stack[i] = Ptr::error(&format!("OAuth config invalid: {e}"));
                            }
                        }
                    }
                    _ => {
                        machine.stack[i] = Ptr::error("OAuth not configured. Run 'admin oauth setup bsky' first.");
                    }
                }
            }
            "admin_user_allow_request" => {
                let handle = machine.stack[i].ref_id.clone();
                let user_id = machine.user_id;

                // Check if requesting user is admin
                let is_admin: bool = match client
                    .query_one(
                        "SELECT is_admin FROM kerai.users WHERE id = $1",
                        &[&user_id],
                    )
                    .await
                {
                    Ok(row) => row.get(0),
                    Err(_) => false,
                };

                if !is_admin {
                    machine.stack[i] = Ptr::error("permission denied: admin only");
                } else {
                    // Check if user already exists
                    match client
                        .query_opt(
                            "SELECT id, is_allowed, did FROM kerai.users WHERE handle = $1",
                            &[&handle],
                        )
                        .await
                    {
                        Ok(Some(row)) => {
                            let is_allowed: bool = row.get(1);
                            let did: Option<String> = row.get(2);
                            if is_allowed {
                                machine.stack[i] = Ptr {
                                    kind: "text".into(),
                                    ref_id: format!("{} is already allowed", handle),
                                    meta: serde_json::Value::Null,
                                    id: 0,
                                };
                            } else {
                                let target_id: uuid::Uuid = row.get(0);
                                match client
                                    .execute(
                                        "UPDATE kerai.users SET is_allowed = true WHERE id = $1",
                                        &[&target_id],
                                    )
                                    .await
                                {
                                    Ok(_) => {
                                        let status = if did.is_some() { "allowed" } else { "allowlisted (pending login)" };
                                        machine.stack[i] = Ptr {
                                            kind: "text".into(),
                                            ref_id: format!("{} {}", handle, status),
                                            meta: serde_json::Value::Null,
                                            id: 0,
                                        };
                                    }
                                    Err(e) => {
                                        machine.stack[i] = Ptr::error(&format!("allow failed: {e}"));
                                    }
                                }
                            }
                        }
                        Ok(None) => {
                            // Insert placeholder row with handle + is_allowed=true
                            match client
                                .execute(
                                    "INSERT INTO kerai.users (handle, auth_provider, is_allowed) \
                                     VALUES ($1, 'bsky', true)",
                                    &[&handle],
                                )
                                .await
                            {
                                Ok(_) => {
                                    machine.stack[i] = Ptr {
                                        kind: "text".into(),
                                        ref_id: format!("{} allowlisted (pending login)", handle),
                                        meta: serde_json::Value::Null,
                                        id: 0,
                                    };
                                }
                                Err(e) => {
                                    machine.stack[i] = Ptr::error(&format!("allow failed: {e}"));
                                }
                            }
                        }
                        Err(e) => {
                            machine.stack[i] = Ptr::error(&format!("allow lookup failed: {e}"));
                        }
                    }
                }
            }
            "admin_oauth_setup_request" => {
                // Admin guard: allow if user is admin or no admin exists yet (bootstrap)
                let user_id = machine.user_id;
                let is_admin: bool = match client
                    .query_one(
                        "SELECT is_admin FROM kerai.users WHERE id = $1",
                        &[&user_id],
                    )
                    .await
                {
                    Ok(row) => row.get(0),
                    Err(_) => false,
                };
                let has_any_admin: bool = match client
                    .query_one(
                        "SELECT EXISTS(SELECT 1 FROM kerai.users WHERE is_admin = true)",
                        &[],
                    )
                    .await
                {
                    Ok(row) => row.get(0),
                    Err(_) => false,
                };

                if !is_admin && has_any_admin {
                    machine.stack[i] = Ptr::error("permission denied: admin only");
                    i += 1;
                    continue;
                }

                // Generate ES256 keypair
                let (private_key_pem, public_jwk_json) = OAuthConfig::generate_keypair();

                // Get or default public_url
                let public_url = match client
                    .query_opt("SELECT value FROM kerai.config WHERE key = 'public_url'", &[])
                    .await
                {
                    Ok(Some(row)) => row.get::<_, String>(0),
                    _ => "https://ker.ai".to_string(),
                };

                // Store in kerai.config (upsert)
                let upsert = "INSERT INTO kerai.config (key, value, updated_at) VALUES ($1, $2, now()) \
                              ON CONFLICT (key) DO UPDATE SET value = $2, updated_at = now()";

                let results = futures::future::join3(
                    client.execute(upsert, &[&"oauth.bsky.private_key", &private_key_pem]),
                    client.execute(upsert, &[&"oauth.bsky.public_jwk", &public_jwk_json]),
                    client.execute(upsert, &[&"public_url", &public_url]),
                )
                .await;

                if results.0.is_err() || results.1.is_err() || results.2.is_err() {
                    machine.stack[i] = Ptr::error("failed to store OAuth config");
                } else {
                    let client_id = format!("{}/.well-known/oauth-client-metadata", public_url);
                    machine.stack[i] = Ptr {
                        kind: "text".into(),
                        ref_id: format!("OAuth configured. client_id: {}", client_id),
                        meta: serde_json::Value::Null,
                        id: 0,
                    };
                }
            }
            _ => {}
        }
        i += 1;
    }
}

pub async fn terminal_page() -> impl IntoResponse {
    Html(include_str!("../../../terminal.html"))
}
