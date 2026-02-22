use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Redirect};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;
use tracing::{error, info};
use uuid::Uuid;

use super::db::Pool;
use super::oauth::{self, OAuthConfig};

#[derive(Serialize)]
pub struct SessionInfo {
    pub user_id: String,
    pub workspace_id: String,
    pub workspace_name: String,
    pub handle: Option<String>,
    pub auth_provider: String,
    pub is_admin: bool,
    pub token: String,
    pub pg_host: String,
}

/// GET /auth/session — Return current session info or create anonymous session.
pub async fn get_session(
    State(pool): State<Arc<Pool>>,
    headers: HeaderMap,
) -> Result<Json<SessionInfo>, (StatusCode, String)> {
    let client = pool.get().await.map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
    })?;

    // Check for existing session cookie
    if let Some(token) = extract_session_token(&headers) {
        if let Some(info) = lookup_session(&client, &token, pool.pg_host()).await? {
            return Ok(Json(info));
        }
    }

    // No valid session — create anonymous user + workspace + session
    let user_id: Uuid = client
        .query_one(
            "INSERT INTO kerai.users (auth_provider) VALUES ('anonymous') RETURNING id",
            &[],
        )
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .get(0);

    // Generate a short anonymous workspace name from user_id
    let ws_name = format!("anon-{}", &user_id.to_string()[..8]);

    let workspace_id: Uuid = client
        .query_one(
            "INSERT INTO kerai.workspaces (user_id, name, is_active, is_anonymous) \
             VALUES ($1, $2, true, true) RETURNING id",
            &[&user_id, &ws_name],
        )
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .get(0);

    // Generate session token
    let token = generate_token();

    client
        .execute(
            "INSERT INTO kerai.sessions (user_id, workspace_id, token) VALUES ($1, $2, $3)",
            &[&user_id, &workspace_id, &token],
        )
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(SessionInfo {
        user_id: user_id.to_string(),
        workspace_id: workspace_id.to_string(),
        workspace_name: ws_name,
        handle: None,
        auth_provider: "anonymous".into(),
        is_admin: false,
        token,
        pg_host: pool.pg_host().to_string(),
    }))
}

#[derive(Deserialize)]
pub struct BskyStartRequest {
    pub handle: Option<String>,
}

/// POST /auth/bsky/start — Begin AT Protocol OAuth. Returns authorize URL.
pub async fn bsky_start(
    State(pool): State<Arc<Pool>>,
    headers: HeaderMap,
    Json(req): Json<BskyStartRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let client = pool.get().await.map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
    })?;

    let session_token = extract_session_token(&headers)
        .ok_or_else(|| (StatusCode::UNAUTHORIZED, "no session".to_string()))?;

    // Load OAuth config
    let config = load_oauth_config(&client).await.map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, e)
    })?;

    // Discover auth server — resolve handle if provided, otherwise go direct to bsky.social
    let (handle, auth_meta) = if let Some(h) = req.handle.filter(|h| !h.is_empty()) {
        let did = oauth::resolve_handle(&h).await.map_err(|e| {
            (StatusCode::BAD_REQUEST, format!("handle resolution failed: {e}"))
        })?;
        let meta = oauth::discover_auth_server(&did).await.map_err(|e| {
            (StatusCode::BAD_GATEWAY, format!("auth server discovery failed: {e}"))
        })?;
        (Some(h), meta)
    } else {
        let meta = oauth::discover_auth_server_from_pds("https://bsky.social").await.map_err(|e| {
            (StatusCode::BAD_GATEWAY, format!("auth server discovery failed: {e}"))
        })?;
        (None, meta)
    };

    // Generate PKCE + ephemeral DPoP key (must be distinct from client JWKS key)
    let (code_verifier, code_challenge) = oauth::generate_pkce();
    let state = oauth::generate_state();
    let (dpop_key, dpop_key_b64) = oauth::generate_dpop_key();

    // PAR → authorize URL
    let authorize_url = oauth::pushed_auth_request(&config, &auth_meta, &code_challenge, &state, &dpop_key)
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, format!("PAR failed: {e}")))?;

    // Store state
    let handle_ref: Option<&str> = handle.as_deref();
    let did_ref: Option<&str> = None;
    client
        .execute(
            "INSERT INTO kerai.oauth_state (state, code_verifier, session_token, handle, did, token_endpoint, issuer, dpop_key) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
            &[
                &state,
                &code_verifier,
                &session_token,
                &handle_ref,
                &did_ref,
                &auth_meta.token_endpoint,
                &auth_meta.issuer,
                &dpop_key_b64,
            ],
        )
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("state storage failed: {e}")))?;

    Ok(Json(json!({ "url": authorize_url })))
}

#[derive(Deserialize)]
pub struct CallbackParams {
    pub code: String,
    pub state: String,
    #[serde(default)]
    pub iss: Option<String>,
}

/// GET /auth/bsky/callback — Handle OAuth callback.
pub async fn bsky_callback(
    State(pool): State<Arc<Pool>>,
    Query(params): Query<CallbackParams>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    info!("OAuth callback: state={}", &params.state[..8.min(params.state.len())]);

    let client = pool.get().await.map_err(|e| {
        error!("callback: pool error: {e}");
        (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
    })?;

    // Look up oauth_state
    let state_row = client
        .query_opt(
            "SELECT code_verifier, session_token, handle, did, token_endpoint, dpop_nonce, issuer, dpop_key \
             FROM kerai.oauth_state WHERE state = $1 AND expires_at > now()",
            &[&params.state],
        )
        .await
        .map_err(|e| {
            error!("callback: state lookup error: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
        })?
        .ok_or_else(|| {
            error!("callback: state not found or expired");
            (StatusCode::BAD_REQUEST, "invalid or expired state".to_string())
        })?;

    let code_verifier: String = state_row.get(0);
    let session_token: String = state_row.get(1);
    let handle: Option<String> = state_row.get(2);
    let did: Option<String> = state_row.get(3);
    let token_endpoint: String = state_row.get(4);
    let dpop_nonce: Option<String> = state_row.get(5);
    let issuer: String = state_row.get(6);
    let dpop_key_b64: String = state_row.get(7);

    info!("callback: state found, session_token_len={}, issuer={}", session_token.len(), issuer);

    // Restore ephemeral DPoP key
    let dpop_key = oauth::dpop_key_from_b64(&dpop_key_b64).map_err(|e| {
        error!("callback: dpop key restore failed: {e}");
        (StatusCode::INTERNAL_SERVER_ERROR, format!("dpop key error: {e}"))
    })?;

    // Load OAuth config
    let config = load_oauth_config(&client).await.map_err(|e| {
        error!("callback: config load failed: {e}");
        (StatusCode::INTERNAL_SERVER_ERROR, e)
    })?;

    // Exchange code for tokens
    info!("callback: exchanging code for tokens at {token_endpoint}");
    let token_resp = oauth::exchange_code(
        &config,
        &token_endpoint,
        &issuer,
        &params.code,
        &code_verifier,
        dpop_nonce.as_deref(),
        &dpop_key,
    )
    .await
    .map_err(|e| {
        error!("callback: token exchange failed: {e}");
        (StatusCode::BAD_GATEWAY, format!("token exchange failed: {e}"))
    })?;

    // Extract DID from token response (sub field) or use stored DID
    let user_did = token_resp
        .sub
        .as_deref()
        .or(did.as_deref())
        .ok_or_else(|| {
            error!("callback: no DID in token response");
            (StatusCode::INTERNAL_SERVER_ERROR, "no DID in token response".to_string())
        })?
        .to_string();

    info!("callback: authenticated as DID {user_did}");

    // Resolve handle from DID if not already known
    let user_handle = if let Some(h) = handle.filter(|h| !h.is_empty()) {
        h
    } else {
        match oauth::resolve_did_to_handle(&user_did).await {
            Ok(h) => {
                info!("callback: resolved handle {h}");
                h
            }
            Err(e) => {
                info!("callback: handle resolution failed (non-fatal): {e}");
                String::new()
            }
        }
    };

    // Find session
    let session_row = client
        .query_opt(
            "SELECT user_id, workspace_id FROM kerai.sessions \
             WHERE token = $1 AND expires_at > now()",
            &[&session_token],
        )
        .await
        .map_err(|e| {
            error!("callback: session lookup error: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
        })?
        .ok_or_else(|| {
            error!("callback: session expired for token_len={}", session_token.len());
            (StatusCode::BAD_REQUEST, "session expired".to_string())
        })?;

    let current_user_id: Uuid = session_row.get(0);
    info!("callback: upgrading user {current_user_id}");

    // Check if DID already has an account
    let existing_user = client
        .query_opt(
            "SELECT id, is_allowed FROM kerai.users WHERE did = $1",
            &[&user_did],
        )
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    if let Some(existing_row) = existing_user {
        let existing_user_id: Uuid = existing_row.get(0);
        let is_allowed: bool = existing_row.get(1);

        // Access gate: existing user must be allowed
        if !is_allowed {
            info!("callback: user {existing_user_id} not allowed, rejecting");
            // Clean up oauth state
            let _ = client
                .execute("DELETE FROM kerai.oauth_state WHERE state = $1", &[&params.state])
                .await;
            return Ok(Redirect::to("/?error=not_allowed"));
        }

        if existing_user_id != current_user_id {
            info!("callback: DID already linked to user {existing_user_id}, merging session");
            // DID already has an account — point session to existing user
            client
                .execute(
                    "UPDATE kerai.sessions SET user_id = $1 \
                     WHERE token = $2",
                    &[&existing_user_id, &session_token],
                )
                .await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

            // Update handle in case it changed
            client
                .execute(
                    "UPDATE kerai.users SET handle = $1, last_login = now() WHERE id = $2",
                    &[&user_handle, &existing_user_id],
                )
                .await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        } else {
            info!("callback: DID already linked to this user, updating handle");
            client
                .execute(
                    "UPDATE kerai.users SET handle = $1, last_login = now() WHERE id = $2",
                    &[&user_handle, &current_user_id],
                )
                .await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        }
    } else {
        // New user — check if any admin exists
        let has_admin: bool = client
            .query_one(
                "SELECT EXISTS(SELECT 1 FROM kerai.users WHERE is_admin = true)",
                &[],
            )
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
            .get(0);

        if !has_admin {
            info!("callback: no admin exists — first user becomes admin");
            // First user ever: auto-admin + auto-allow
            client
                .execute(
                    "UPDATE kerai.users SET did = $1, handle = $2, auth_provider = 'bsky', \
                     is_admin = true, is_allowed = true, last_login = now() \
                     WHERE id = $3",
                    &[&user_did, &user_handle, &current_user_id],
                )
                .await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        } else {
            // Admin exists — check if handle was pre-allowlisted (placeholder row)
            let placeholder = client
                .query_opt(
                    "SELECT id FROM kerai.users WHERE handle = $1 AND is_allowed = true AND did IS NULL",
                    &[&user_handle],
                )
                .await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

            if let Some(placeholder_row) = placeholder {
                let placeholder_id: Uuid = placeholder_row.get(0);
                info!("callback: found allowlisted placeholder {placeholder_id}, upgrading");
                // Delete the placeholder, upgrade the current anonymous user
                client
                    .execute("DELETE FROM kerai.users WHERE id = $1", &[&placeholder_id])
                    .await
                    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
                client
                    .execute(
                        "UPDATE kerai.users SET did = $1, handle = $2, auth_provider = 'bsky', \
                         is_allowed = true, last_login = now() \
                         WHERE id = $3",
                        &[&user_did, &user_handle, &current_user_id],
                    )
                    .await
                    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
            } else {
                info!("callback: user {} not allowlisted, rejecting", user_handle);
                // Not allowlisted — reject
                let _ = client
                    .execute("DELETE FROM kerai.oauth_state WHERE state = $1", &[&params.state])
                    .await;
                return Ok(Redirect::to("/?error=not_allowed"));
            }
        }
    }

    // Rename anonymous workspace if applicable
    if !user_handle.is_empty() {
        let _ = client
            .execute(
                "UPDATE kerai.workspaces SET name = $1 \
                 WHERE user_id = $2 AND is_anonymous = true AND name LIKE 'anon-%'",
                &[&user_handle, &current_user_id],
            )
            .await;
    }

    // Clean up oauth state
    client
        .execute(
            "DELETE FROM kerai.oauth_state WHERE state = $1",
            &[&params.state],
        )
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    info!("callback: success, redirecting to /");
    // Redirect to home
    Ok(Redirect::to("/"))
}

/// GET /.well-known/oauth-client-metadata — AT Protocol client metadata.
pub async fn client_metadata(
    State(pool): State<Arc<Pool>>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let client = pool.get().await.map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
    })?;

    let public_url = get_config_value(&client, "public_url")
        .await
        .unwrap_or_else(|_| "https://ker.ai".to_string());

    Ok(Json(json!({
        "client_id": format!("{}/.well-known/oauth-client-metadata", public_url),
        "client_name": "ker.ai",
        "redirect_uris": [format!("{}/auth/bsky/callback", public_url)],
        "grant_types": ["authorization_code"],
        "response_types": ["code"],
        "scope": "atproto",
        "token_endpoint_auth_method": "private_key_jwt",
        "token_endpoint_auth_signing_alg": "ES256",
        "dpop_bound_access_tokens": true,
        "jwks_uri": format!("{}/oauth/jwks.json", public_url),
    })))
}

/// GET /oauth/jwks.json — ES256 public key in JWKS format.
pub async fn jwks(
    State(pool): State<Arc<Pool>>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let client = pool.get().await.map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
    })?;

    let jwk_str = get_config_value(&client, "oauth.bsky.public_jwk").await.map_err(|e| {
        (StatusCode::NOT_FOUND, format!("JWKS not configured: {e}"))
    })?;

    let jwk: serde_json::Value = serde_json::from_str(&jwk_str).map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, format!("invalid JWK: {e}"))
    })?;

    Ok(Json(json!({ "keys": [jwk] })))
}

/// POST /auth/logout — Clear session.
pub async fn logout(
    State(pool): State<Arc<Pool>>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let client = pool.get().await.map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
    })?;

    if let Some(token) = extract_session_token(&headers) {
        client
            .execute(
                "DELETE FROM kerai.sessions WHERE token = $1",
                &[&token],
            )
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    }

    Ok(Json(json!({"status": "logged_out"})))
}

/// Extract session token from Cookie header.
pub(crate) fn extract_session_token(headers: &HeaderMap) -> Option<String> {
    let cookie_header = headers.get("cookie")?.to_str().ok()?;
    for pair in cookie_header.split(';') {
        let pair = pair.trim();
        if let Some(value) = pair.strip_prefix("kerai_session=") {
            let token = value.trim();
            if !token.is_empty() {
                return Some(token.to_string());
            }
        }
    }
    None
}

/// Look up a session by token and return session info.
async fn lookup_session(
    client: &tokio_postgres::Client,
    token: &str,
    pg_host: &str,
) -> Result<Option<SessionInfo>, (StatusCode, String)> {
    let row = client
        .query_opt(
            "SELECT s.user_id, s.workspace_id, w.name, u.handle, u.auth_provider, s.token, u.is_admin \
             FROM kerai.sessions s \
             JOIN kerai.users u ON u.id = s.user_id \
             JOIN kerai.workspaces w ON w.id = s.workspace_id \
             WHERE s.token = $1 AND s.expires_at > now()",
            &[&token],
        )
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(row.map(|r| SessionInfo {
        user_id: r.get::<_, Uuid>(0).to_string(),
        workspace_id: r.get::<_, Uuid>(1).to_string(),
        workspace_name: r.get::<_, String>(2),
        handle: r.get::<_, Option<String>>(3),
        auth_provider: r.get::<_, String>(4),
        token: r.get::<_, String>(5),
        is_admin: r.get::<_, bool>(6),
        pg_host: pg_host.to_string(),
    }))
}

/// Generate a random session token (hex-encoded 32 bytes).
fn generate_token() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let bytes: Vec<u8> = (0..32).map(|_| rng.gen()).collect();
    hex_encode(&bytes)
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Look up the session for a given token string. Used by eval route.
pub async fn resolve_session(
    pool: &Pool,
    token: &str,
) -> Result<(Uuid, Uuid), String> {
    let client = pool.get().await.map_err(|e| e.to_string())?;

    let row = client
        .query_opt(
            "SELECT user_id, workspace_id FROM kerai.sessions \
             WHERE token = $1 AND expires_at > now()",
            &[&token],
        )
        .await
        .map_err(|e| e.to_string())?
        .ok_or("invalid or expired session")?;

    let user_id: Uuid = row.get(0);
    let workspace_id: Uuid = row.get(1);
    Ok((user_id, workspace_id))
}

/// Load OAuth config from kerai.config table.
async fn load_oauth_config(
    client: &tokio_postgres::Client,
) -> Result<OAuthConfig, String> {
    let rows = client
        .query(
            "SELECT key, value FROM kerai.config WHERE key LIKE 'oauth.bsky.%' OR key = 'public_url'",
            &[],
        )
        .await
        .map_err(|e| format!("config query failed: {e}"))?;

    if rows.is_empty() {
        return Err("OAuth not configured. Run 'admin oauth setup bsky' first.".to_string());
    }

    let config_rows: Vec<(String, String)> = rows
        .iter()
        .map(|r| (r.get::<_, String>(0), r.get::<_, String>(1)))
        .collect();

    OAuthConfig::from_config_rows(&config_rows)
}

/// Get a single config value.
async fn get_config_value(
    client: &tokio_postgres::Client,
    key: &str,
) -> Result<String, String> {
    let row = client
        .query_opt(
            "SELECT value FROM kerai.config WHERE key = $1",
            &[&key],
        )
        .await
        .map_err(|e| format!("config query failed: {e}"))?
        .ok_or_else(|| format!("config key not found: {key}"))?;

    Ok(row.get(0))
}
