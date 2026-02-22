use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Redirect};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;
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
    pub token: String,
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
        if let Some(info) = lookup_session(&client, &token).await? {
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
        token,
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

    // Resolve handle → DID
    let handle = req.handle.unwrap_or_else(|| "bsky.social".into());
    let did = oauth::resolve_handle(&handle).await.map_err(|e| {
        (StatusCode::BAD_REQUEST, format!("handle resolution failed: {e}"))
    })?;

    // Discover auth server
    let auth_meta = oauth::discover_auth_server(&did).await.map_err(|e| {
        (StatusCode::BAD_GATEWAY, format!("auth server discovery failed: {e}"))
    })?;

    // Generate PKCE
    let (code_verifier, code_challenge) = oauth::generate_pkce();
    let state = oauth::generate_state();

    // PAR → authorize URL
    let authorize_url = oauth::pushed_auth_request(&config, &auth_meta, &code_challenge, &state)
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, format!("PAR failed: {e}")))?;

    // Store state
    client
        .execute(
            "INSERT INTO kerai.oauth_state (state, code_verifier, session_token, handle, did, token_endpoint) \
             VALUES ($1, $2, $3, $4, $5, $6)",
            &[
                &state,
                &code_verifier,
                &session_token,
                &handle,
                &did,
                &auth_meta.token_endpoint,
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
    let client = pool.get().await.map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
    })?;

    // Look up oauth_state
    let state_row = client
        .query_opt(
            "SELECT code_verifier, session_token, handle, did, token_endpoint, dpop_nonce \
             FROM kerai.oauth_state WHERE state = $1 AND expires_at > now()",
            &[&params.state],
        )
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "invalid or expired state".to_string()))?;

    let code_verifier: String = state_row.get(0);
    let session_token: String = state_row.get(1);
    let handle: Option<String> = state_row.get(2);
    let did: Option<String> = state_row.get(3);
    let token_endpoint: String = state_row.get(4);
    let dpop_nonce: Option<String> = state_row.get(5);

    // Load OAuth config
    let config = load_oauth_config(&client).await.map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, e)
    })?;

    // Exchange code for tokens
    let token_resp = oauth::exchange_code(
        &config,
        &token_endpoint,
        &params.code,
        &code_verifier,
        dpop_nonce.as_deref(),
    )
    .await
    .map_err(|e| (StatusCode::BAD_GATEWAY, format!("token exchange failed: {e}")))?;

    // Extract DID from token response (sub field) or use stored DID
    let user_did = token_resp
        .sub
        .as_deref()
        .or(did.as_deref())
        .ok_or_else(|| (StatusCode::INTERNAL_SERVER_ERROR, "no DID in token response".to_string()))?
        .to_string();

    let user_handle = handle.unwrap_or_default();

    // Find session
    let session_row = client
        .query_opt(
            "SELECT user_id, workspace_id FROM kerai.sessions \
             WHERE token = $1 AND expires_at > now()",
            &[&session_token],
        )
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "session expired".to_string()))?;

    let current_user_id: Uuid = session_row.get(0);

    // Check if DID already has an account
    let existing_user = client
        .query_opt(
            "SELECT id FROM kerai.users WHERE did = $1",
            &[&user_did],
        )
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    if let Some(existing_row) = existing_user {
        let existing_user_id: Uuid = existing_row.get(0);
        if existing_user_id != current_user_id {
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
        }
    } else {
        // Upgrade anonymous user → bsky user
        client
            .execute(
                "UPDATE kerai.users SET did = $1, handle = $2, auth_provider = 'bsky', last_login = now() \
                 WHERE id = $3",
                &[&user_did, &user_handle, &current_user_id],
            )
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    }

    // Clean up oauth state
    client
        .execute(
            "DELETE FROM kerai.oauth_state WHERE state = $1",
            &[&params.state],
        )
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

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
fn extract_session_token(headers: &HeaderMap) -> Option<String> {
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
) -> Result<Option<SessionInfo>, (StatusCode, String)> {
    let row = client
        .query_opt(
            "SELECT s.user_id, s.workspace_id, w.name, u.handle, u.auth_provider, s.token \
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
