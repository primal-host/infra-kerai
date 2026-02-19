/// Peer management — register, list, get, remove peer instances.
use pgrx::prelude::*;

use crate::identity;
use crate::sql::sql_escape;

/// Register a peer instance. Decodes hex public key, computes fingerprint,
/// UPSERTs into kerai.instances. Returns JSON with peer info.
#[pg_extern]
fn register_peer(
    name: &str,
    public_key_hex: &str,
    endpoint: Option<&str>,
    connection: Option<&str>,
) -> pgrx::JsonB {
    let pk_bytes = hex::decode(public_key_hex)
        .unwrap_or_else(|_| error!("Invalid hex public_key"));
    if pk_bytes.len() != 32 {
        error!("Public key must be 32 bytes (got {})", pk_bytes.len());
    }
    let pk_hex_pg: String = pk_bytes.iter().map(|b| format!("{:02x}", b)).collect();
    let pk_array: [u8; 32] = pk_bytes.try_into().unwrap();
    let verifying_key = ed25519_dalek::VerifyingKey::from_bytes(&pk_array)
        .unwrap_or_else(|_| error!("Invalid Ed25519 public key"));
    let fp = identity::fingerprint(&verifying_key);
    let endpoint_sql = match endpoint {
        Some(e) => format!("'{}'", sql_escape(e)),
        None => "NULL".to_string(),
    };
    let connection_sql = match connection {
        Some(c) => format!("'{}'", sql_escape(c)),
        None => "NULL".to_string(),
    };

    // Check if already exists by fingerprint (unwrap_or: 0 rows → None)
    let existing = Spi::get_one::<String>(&format!(
        "SELECT id::text FROM kerai.instances WHERE key_fingerprint = '{}'",
        sql_escape(&fp),
    ))
    .unwrap_or(None);

    let is_new;
    let instance_id;

    if let Some(eid) = existing {
        // Update name, endpoint, connection, last_seen
        Spi::run(&format!(
            "UPDATE kerai.instances SET name = '{}', endpoint = {}, connection = {}, last_seen = now()
             WHERE key_fingerprint = '{}'",
            sql_escape(name),
            endpoint_sql,
            connection_sql,
            sql_escape(&fp),
        ))
        .unwrap();
        is_new = false;
        instance_id = eid;
    } else {
        // Insert new peer
        let new_id = Spi::get_one::<String>(&format!(
            "INSERT INTO kerai.instances (name, public_key, key_fingerprint, endpoint, connection, is_self, last_seen)
             VALUES ('{}', '\\x{}'::bytea, '{}', {}, {}, false, now())
             RETURNING id::text",
            sql_escape(name),
            pk_hex_pg,
            sql_escape(&fp),
            endpoint_sql,
            connection_sql,
        ))
        .unwrap()
        .unwrap();
        is_new = true;
        instance_id = new_id;
    }

    pgrx::JsonB(serde_json::json!({
        "id": instance_id,
        "name": name,
        "key_fingerprint": fp,
        "endpoint": endpoint,
        "connection": connection,
        "is_new": is_new,
    }))
}

/// List all non-self peer instances as a JSON array.
#[pg_extern]
fn list_peers() -> pgrx::JsonB {
    let json = Spi::get_one::<pgrx::JsonB>(
        "SELECT COALESCE(
            jsonb_agg(jsonb_build_object(
                'id', id,
                'name', name,
                'key_fingerprint', key_fingerprint,
                'endpoint', endpoint,
                'connection', connection,
                'last_seen', last_seen,
                'public_key', encode(public_key, 'hex')
            ) ORDER BY name),
            '[]'::jsonb
        ) FROM kerai.instances WHERE is_self = false",
    )
    .unwrap()
    .unwrap_or_else(|| pgrx::JsonB(serde_json::json!([])));
    json
}

/// Get a single peer by fingerprint.
#[pg_extern]
fn get_peer(fingerprint: &str) -> pgrx::JsonB {
    let row = Spi::get_one::<pgrx::JsonB>(&format!(
        "SELECT jsonb_build_object(
            'id', id,
            'name', name,
            'key_fingerprint', key_fingerprint,
            'endpoint', endpoint,
            'connection', connection,
            'last_seen', last_seen,
            'public_key', encode(public_key, 'hex'),
            'is_self', is_self
        ) FROM kerai.instances WHERE key_fingerprint = '{}'",
        sql_escape(fingerprint),
    ))
    .unwrap_or(None);

    match row {
        Some(j) => j,
        None => error!("Peer not found: {}", fingerprint),
    }
}

/// Remove a non-self peer by name. Returns JSON with removal status.
#[pg_extern]
fn remove_peer(name: &str) -> pgrx::JsonB {
    // Check it's not self (unwrap_or: 0 rows → None)
    let is_self = Spi::get_one::<bool>(&format!(
        "SELECT is_self FROM kerai.instances WHERE name = '{}'",
        sql_escape(name),
    ))
    .unwrap_or(None);

    match is_self {
        Some(true) => error!("Cannot remove self instance"),
        None => {
            return pgrx::JsonB(serde_json::json!({
                "removed": false,
                "name": name,
                "reason": "not found",
            }));
        }
        _ => {}
    }

    Spi::run(&format!(
        "DELETE FROM kerai.instances WHERE name = '{}' AND is_self = false",
        sql_escape(name),
    ))
    .unwrap();

    pgrx::JsonB(serde_json::json!({
        "removed": true,
        "name": name,
    }))
}

/// Return the self instance's public key as a hex string.
#[pg_extern]
fn self_public_key_hex() -> String {
    Spi::get_one::<String>(
        "SELECT encode(public_key, 'hex') FROM kerai.instances WHERE is_self = true",
    )
    .unwrap()
    .unwrap_or_else(|| error!("Self instance not found"))
}
