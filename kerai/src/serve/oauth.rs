use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use p256::ecdsa::SigningKey;
use p256::elliptic_curve::rand_core::OsRng;
use p256::elliptic_curve::sec1::ToEncodedPoint;
use p256::SecretKey;
use rand::Rng;
use serde::Deserialize;
use sha2::{Digest, Sha256};

/// OAuth config loaded from kerai.config table.
pub struct OAuthConfig {
    pub public_url: String,
    pub client_id: String,
    pub private_key: SecretKey,
    pub public_jwk: serde_json::Value,
    pub jwks: serde_json::Value,
}

/// Authorization server metadata.
#[derive(Debug, Deserialize)]
pub struct AuthServerMeta {
    pub issuer: String,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    #[serde(default)]
    pub pushed_authorization_request_endpoint: Option<String>,
    #[serde(default)]
    pub dpop_signing_alg_values_supported: Vec<String>,
}

/// Token response from the authorization server.
#[derive(Debug, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    #[serde(default)]
    pub token_type: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub expires_in: Option<u64>,
    #[serde(default)]
    pub sub: Option<String>,
    #[serde(default)]
    pub scope: Option<String>,
}

/// PAR response.
#[derive(Debug, Deserialize)]
struct ParResponse {
    request_uri: String,
    #[allow(dead_code)]
    #[serde(default)]
    expires_in: Option<u64>,
}

impl OAuthConfig {
    /// Load OAuth config from kerai.config rows.
    pub fn from_config_rows(rows: &[(String, String)]) -> Result<Self, String> {
        let mut private_key_b64 = None;
        let mut public_jwk_str = None;
        let mut public_url = None;

        for (key, value) in rows {
            match key.as_str() {
                "oauth.bsky.private_key" => private_key_b64 = Some(value.clone()),
                "oauth.bsky.public_jwk" => public_jwk_str = Some(value.clone()),
                "public_url" => public_url = Some(value.clone()),
                _ => {}
            }
        }

        let public_url =
            public_url.ok_or_else(|| "missing config key: public_url".to_string())?;
        let key_b64 = private_key_b64
            .ok_or_else(|| "missing config key: oauth.bsky.private_key".to_string())?;
        let jwk_str = public_jwk_str
            .ok_or_else(|| "missing config key: oauth.bsky.public_jwk".to_string())?;

        let key_bytes = URL_SAFE_NO_PAD
            .decode(&key_b64)
            .map_err(|e| format!("invalid private key encoding: {e}"))?;
        let private_key = SecretKey::from_slice(&key_bytes)
            .map_err(|e| format!("invalid private key: {e}"))?;

        let public_jwk: serde_json::Value =
            serde_json::from_str(&jwk_str).map_err(|e| format!("invalid public JWK: {e}"))?;

        let client_id = format!("{}/.well-known/oauth-client-metadata", public_url);

        let jwks = serde_json::json!({ "keys": [public_jwk.clone()] });

        Ok(Self {
            public_url,
            client_id,
            private_key,
            public_jwk,
            jwks,
        })
    }

    /// Generate a new ES256 keypair and return (private_key_b64, public_jwk_json).
    pub fn generate_keypair() -> (String, String) {
        let secret = SecretKey::random(&mut OsRng);
        let public = secret.public_key();

        // Base64url-encode the raw 32-byte private key
        let key_b64 = URL_SAFE_NO_PAD.encode(secret.to_bytes());

        // Build JWK from public key
        let point = public.to_encoded_point(false);
        let x = URL_SAFE_NO_PAD.encode(point.x().expect("x coordinate"));
        let y = URL_SAFE_NO_PAD.encode(point.y().expect("y coordinate"));

        // Generate a key ID from public key hash
        let kid = {
            let mut hasher = Sha256::new();
            hasher.update(point.as_bytes());
            let hash = hasher.finalize();
            URL_SAFE_NO_PAD.encode(&hash[..8])
        };

        let jwk = serde_json::json!({
            "kty": "EC",
            "crv": "P-256",
            "x": x,
            "y": y,
            "kid": kid,
            "use": "sig",
            "alg": "ES256",
        });

        (key_b64, serde_json::to_string(&jwk).unwrap())
    }
}

/// Resolve a Bluesky handle to a DID.
pub async fn resolve_handle(handle: &str) -> Result<String, String> {
    let url = format!(
        "https://bsky.social/xrpc/com.atproto.identity.resolveHandle?handle={}",
        handle
    );

    let resp = reqwest::get(&url)
        .await
        .map_err(|e| format!("resolve handle request failed: {e}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("resolve handle failed ({status}): {body}"));
    }

    #[derive(Deserialize)]
    struct ResolveResponse {
        did: String,
    }

    let data: ResolveResponse = resp
        .json()
        .await
        .map_err(|e| format!("resolve handle parse failed: {e}"))?;

    Ok(data.did)
}

/// Fetch auth server metadata directly from a PDS endpoint URL.
/// Used when no handle is provided — goes straight to bsky.social.
pub async fn discover_auth_server_from_pds(pds_url: &str) -> Result<AuthServerMeta, String> {
    let meta_url = format!(
        "{}/.well-known/oauth-authorization-server",
        pds_url.trim_end_matches('/')
    );
    let resp = reqwest::get(&meta_url)
        .await
        .map_err(|e| format!("auth server metadata request failed: {e}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("auth server metadata failed ({status}): {body}"));
    }

    resp.json()
        .await
        .map_err(|e| format!("auth server metadata parse failed: {e}"))
}

/// Discover the authorization server for a DID.
pub async fn discover_auth_server(did: &str) -> Result<AuthServerMeta, String> {
    // Step 1: Get DID document from PLC directory
    let plc_url = format!("https://plc.directory/{}", did);
    let resp = reqwest::get(&plc_url)
        .await
        .map_err(|e| format!("PLC directory request failed: {e}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("PLC directory failed ({status}): {body}"));
    }

    let doc: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("PLC document parse failed: {e}"))?;

    // Step 2: Extract PDS endpoint
    let pds_endpoint = doc
        .get("service")
        .and_then(|s| s.as_array())
        .and_then(|services| {
            services.iter().find_map(|svc| {
                let svc_type = svc.get("type")?.as_str()?;
                if svc_type == "AtprotoPersonalDataServer" {
                    svc.get("serviceEndpoint")?.as_str().map(|s| s.to_string())
                } else {
                    None
                }
            })
        })
        .ok_or_else(|| "no PDS endpoint in DID document".to_string())?;

    // Step 3: Fetch authorization server metadata
    let meta_url = format!(
        "{}/.well-known/oauth-authorization-server",
        pds_endpoint.trim_end_matches('/')
    );
    let resp = reqwest::get(&meta_url)
        .await
        .map_err(|e| format!("auth server metadata request failed: {e}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("auth server metadata failed ({status}): {body}"));
    }

    let meta: AuthServerMeta = resp
        .json()
        .await
        .map_err(|e| format!("auth server metadata parse failed: {e}"))?;

    Ok(meta)
}

/// Generate a PKCE code_verifier and code_challenge (S256).
pub fn generate_pkce() -> (String, String) {
    let mut rng = rand::thread_rng();
    let bytes: Vec<u8> = (0..32).map(|_| rng.gen()).collect();
    let code_verifier = URL_SAFE_NO_PAD.encode(&bytes);

    let mut hasher = Sha256::new();
    hasher.update(code_verifier.as_bytes());
    let hash = hasher.finalize();
    let code_challenge = URL_SAFE_NO_PAD.encode(&hash);

    (code_verifier, code_challenge)
}

/// Generate a random state parameter.
pub fn generate_state() -> String {
    let mut rng = rand::thread_rng();
    let bytes: Vec<u8> = (0..16).map(|_| rng.gen()).collect();
    URL_SAFE_NO_PAD.encode(&bytes)
}

/// Build a DPoP proof JWT.
pub fn build_dpop_proof(
    key: &SecretKey,
    htm: &str,
    htu: &str,
    nonce: Option<&str>,
) -> Result<String, String> {
    use p256::ecdsa::signature::Signer;
    let signing_key = SigningKey::from(key);
    let public = key.public_key();
    let point = public.to_encoded_point(false);
    let x = URL_SAFE_NO_PAD.encode(point.x().expect("x coordinate"));
    let y = URL_SAFE_NO_PAD.encode(point.y().expect("y coordinate"));

    let jwk = serde_json::json!({
        "kty": "EC",
        "crv": "P-256",
        "x": x,
        "y": y,
    });

    let jti = generate_state();
    let iat = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let mut claims = serde_json::json!({
        "jti": jti,
        "htm": htm,
        "htu": htu,
        "iat": iat,
    });
    if let Some(n) = nonce {
        claims["nonce"] = serde_json::Value::String(n.to_string());
    }

    let header_json = serde_json::json!({
        "typ": "dpop+jwt",
        "alg": "ES256",
        "jwk": jwk,
    });
    let header_b64 = URL_SAFE_NO_PAD.encode(header_json.to_string().as_bytes());
    let claims_b64 = URL_SAFE_NO_PAD.encode(claims.to_string().as_bytes());
    let message = format!("{}.{}", header_b64, claims_b64);

    let sig: p256::ecdsa::Signature = signing_key.sign(message.as_bytes());
    let sig_b64 = URL_SAFE_NO_PAD.encode(sig.to_bytes());

    Ok(format!("{}.{}", message, sig_b64))
}

/// Build a client_assertion JWT (private_key_jwt).
/// `audience` should be the authorization server's issuer URL.
pub fn build_client_assertion(
    key: &SecretKey,
    client_id: &str,
    audience: &str,
) -> Result<String, String> {
    use p256::ecdsa::signature::Signer;
    let signing_key = SigningKey::from(key);
    let jti = generate_state();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    // Derive kid from public key
    let public = key.public_key();
    let point = public.to_encoded_point(false);
    let kid = {
        let mut hasher = Sha256::new();
        hasher.update(point.as_bytes());
        let hash = hasher.finalize();
        URL_SAFE_NO_PAD.encode(&hash[..8])
    };

    let header_json = serde_json::json!({
        "alg": "ES256",
        "kid": kid,
    });

    let claims = serde_json::json!({
        "iss": client_id,
        "sub": client_id,
        "aud": audience,
        "jti": jti,
        "iat": now,
        "exp": now + 300,
    });

    let header_b64 = URL_SAFE_NO_PAD.encode(header_json.to_string().as_bytes());
    let claims_b64 = URL_SAFE_NO_PAD.encode(claims.to_string().as_bytes());
    let message = format!("{}.{}", header_b64, claims_b64);

    let sig: p256::ecdsa::Signature = signing_key.sign(message.as_bytes());
    let sig_b64 = URL_SAFE_NO_PAD.encode(sig.to_bytes());

    Ok(format!("{}.{}", message, sig_b64))
}

/// Perform Pushed Authorization Request (PAR).
/// Returns the authorize URL to redirect the user to.
pub async fn pushed_auth_request(
    config: &OAuthConfig,
    auth_meta: &AuthServerMeta,
    code_challenge: &str,
    state: &str,
) -> Result<String, String> {
    let par_endpoint = auth_meta
        .pushed_authorization_request_endpoint
        .as_deref()
        .ok_or_else(|| "authorization server does not support PAR".to_string())?;

    let redirect_uri = format!("{}/auth/bsky/callback", config.public_url);

    let par_params = [
        ("client_id", config.client_id.as_str()),
        ("redirect_uri", redirect_uri.as_str()),
        ("code_challenge", code_challenge),
        ("code_challenge_method", "S256"),
        ("state", state),
        ("scope", "atproto"),
        ("response_type", "code"),
        (
            "client_assertion_type",
            "urn:ietf:params:oauth:client-assertion-type:jwt-bearer",
        ),
    ];

    // First attempt
    let client_assertion =
        build_client_assertion(&config.private_key, &config.client_id, &auth_meta.issuer)?;
    let dpop_proof = build_dpop_proof(&config.private_key, "POST", par_endpoint, None)?;

    let http = reqwest::Client::new();
    let mut form: Vec<(&str, &str)> = par_params.to_vec();
    form.push(("client_assertion", &client_assertion));

    let resp = http
        .post(par_endpoint)
        .header("DPoP", &dpop_proof)
        .form(&form)
        .send()
        .await
        .map_err(|e| format!("PAR request failed: {e}"))?;

    // Handle DPoP nonce requirement
    if resp.status() == reqwest::StatusCode::BAD_REQUEST {
        if let Some(nonce) = resp.headers().get("dpop-nonce").and_then(|v| v.to_str().ok()) {
            let nonce = nonce.to_string();
            let _ = resp.text().await; // consume body

            let client_assertion2 =
                build_client_assertion(&config.private_key, &config.client_id, &auth_meta.issuer)?;
            let dpop_proof2 =
                build_dpop_proof(&config.private_key, "POST", par_endpoint, Some(&nonce))?;
            let mut form2: Vec<(&str, &str)> = par_params.to_vec();
            form2.push(("client_assertion", &client_assertion2));

            let resp2 = http
                .post(par_endpoint)
                .header("DPoP", &dpop_proof2)
                .form(&form2)
                .send()
                .await
                .map_err(|e| format!("PAR retry failed: {e}"))?;

            if !resp2.status().is_success() {
                let status = resp2.status();
                let body = resp2.text().await.unwrap_or_default();
                return Err(format!("PAR retry failed ({status}): {body}"));
            }

            let par: ParResponse = resp2
                .json()
                .await
                .map_err(|e| format!("PAR retry parse failed: {e}"))?;

            return Ok(format!(
                "{}?client_id={}&request_uri={}",
                auth_meta.authorization_endpoint,
                urlencoding(&config.client_id),
                urlencoding(&par.request_uri),
            ));
        }
    }

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("PAR failed ({status}): {body}"));
    }

    let par: ParResponse = resp
        .json()
        .await
        .map_err(|e| format!("PAR response parse failed: {e}"))?;

    Ok(format!(
        "{}?client_id={}&request_uri={}",
        auth_meta.authorization_endpoint,
        urlencoding(&config.client_id),
        urlencoding(&par.request_uri),
    ))
}

/// Exchange authorization code for tokens.
pub async fn exchange_code(
    config: &OAuthConfig,
    token_endpoint: &str,
    issuer: &str,
    code: &str,
    code_verifier: &str,
    dpop_nonce: Option<&str>,
) -> Result<TokenResponse, String> {
    let redirect_uri = format!("{}/auth/bsky/callback", config.public_url);
    let http = reqwest::Client::new();

    // First attempt
    let dpop_proof =
        build_dpop_proof(&config.private_key, "POST", token_endpoint, dpop_nonce)?;
    let client_assertion =
        build_client_assertion(&config.private_key, &config.client_id, issuer)?;

    let resp = http
        .post(token_endpoint)
        .header("DPoP", &dpop_proof)
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", redirect_uri.as_str()),
            ("code_verifier", code_verifier),
            ("client_id", config.client_id.as_str()),
            (
                "client_assertion_type",
                "urn:ietf:params:oauth:client-assertion-type:jwt-bearer",
            ),
            ("client_assertion", client_assertion.as_str()),
        ])
        .send()
        .await
        .map_err(|e| format!("token exchange failed: {e}"))?;

    // Handle use_dpop_nonce error — retry with server-provided nonce
    if resp.status() == reqwest::StatusCode::BAD_REQUEST {
        if let Some(nonce) = resp.headers().get("dpop-nonce").and_then(|v| v.to_str().ok()) {
            let nonce = nonce.to_string();
            let _ = resp.text().await; // consume body

            let dpop_proof2 =
                build_dpop_proof(&config.private_key, "POST", token_endpoint, Some(&nonce))?;
            let client_assertion2 =
                build_client_assertion(&config.private_key, &config.client_id, issuer)?;

            let resp2 = http
                .post(token_endpoint)
                .header("DPoP", &dpop_proof2)
                .form(&[
                    ("grant_type", "authorization_code"),
                    ("code", code),
                    ("redirect_uri", redirect_uri.as_str()),
                    ("code_verifier", code_verifier),
                    ("client_id", config.client_id.as_str()),
                    (
                        "client_assertion_type",
                        "urn:ietf:params:oauth:client-assertion-type:jwt-bearer",
                    ),
                    ("client_assertion", client_assertion2.as_str()),
                ])
                .send()
                .await
                .map_err(|e| format!("token exchange retry failed: {e}"))?;

            let status2 = resp2.status();
            if !status2.is_success() {
                let body = resp2.text().await.unwrap_or_default();
                return Err(format!(
                    "token exchange retry failed ({status2}): {body}",
                ));
            }

            return resp2
                .json::<TokenResponse>()
                .await
                .map_err(|e| format!("token response parse failed: {e}"));
        }
    }

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("token exchange failed ({status}): {body}"));
    }

    resp.json::<TokenResponse>()
        .await
        .map_err(|e| format!("token response parse failed: {e}"))
}

/// Simple percent-encoding for URL query parameters.
fn urlencoding(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                result.push(b as char);
            }
            _ => {
                result.push('%');
                result.push_str(&format!("{:02X}", b));
            }
        }
    }
    result
}
