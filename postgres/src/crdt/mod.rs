/// CRDT operation layer — signed operation log with Lamport clock and version vector.
mod clock;
mod operations;
mod signer;

use pgrx::prelude::*;
use serde_json::Value;

use crate::identity;
use crate::sql::sql_escape;

/// Format bytes as PostgreSQL hex bytea literal: \xABCD...
fn bytes_to_pg_hex(bytes: &[u8]) -> String {
    let hex: String = bytes.iter().map(|b| format!("{:02x}", b)).collect();
    format!("\\x{}", hex)
}

/// Get the self instance's (instance_id, key_fingerprint).
fn get_self_identity() -> (String, String) {
    let row = Spi::get_two::<String, String>(
        "SELECT id::text, key_fingerprint FROM kerai.instances WHERE is_self = true",
    )
    .unwrap();
    match row {
        (Some(id), Some(fp)) => (id, fp),
        _ => error!("Self instance not found — run kerai.bootstrap_instance() first"),
    }
}

/// Resolve instance_id for a remote author by fingerprint + public key hex.
/// If the peer exists, update last_seen and return the id.
/// If not found, auto-register as a new peer and return the new id.
fn resolve_author_instance(author_fingerprint: &str, public_key_hex: &str) -> String {
    let escaped_fp = sql_escape(author_fingerprint);

    // Try to find existing instance by fingerprint
    let existing = Spi::get_one::<String>(&format!(
        "SELECT id::text FROM kerai.instances WHERE key_fingerprint = '{}'",
        escaped_fp,
    ))
    .unwrap();

    if let Some(id) = existing {
        // Update last_seen
        Spi::run(&format!(
            "UPDATE kerai.instances SET last_seen = now() WHERE id = '{}'::uuid",
            sql_escape(&id),
        ))
        .unwrap();
        return id;
    }

    // Auto-register new peer
    let prefix = if author_fingerprint.len() >= 8 {
        &author_fingerprint[..8]
    } else {
        author_fingerprint
    };
    let peer_name = format!("peer-{}", prefix);

    let new_id = Spi::get_one::<String>(&format!(
        "INSERT INTO kerai.instances (name, public_key, key_fingerprint, is_self, last_seen)
         VALUES ('{}', '\\x{}'::bytea, '{}', false, now())
         RETURNING id::text",
        sql_escape(&peer_name),
        sql_escape(public_key_hex),
        escaped_fp,
    ))
    .unwrap()
    .unwrap();

    new_id
}

/// Insert an operation record into the operations table.
fn insert_operation(
    instance_id: &str,
    op_type: &str,
    node_id: Option<&str>,
    author: &str,
    lamport_ts: i64,
    author_seq: i64,
    payload: &Value,
    signature: &[u8],
) {
    let node_sql = match node_id {
        Some(nid) => format!("'{}'::uuid", sql_escape(nid)),
        None => "NULL".to_string(),
    };
    let payload_str = sql_escape(&payload.to_string());
    let sig_hex = bytes_to_pg_hex(signature);

    Spi::run(&format!(
        "INSERT INTO kerai.operations (instance_id, op_type, node_id, author, lamport_ts, author_seq, payload, signature)
         VALUES ('{}'::uuid, '{}', {}, '{}', {}, {}, '{}'::jsonb, '{}'::bytea)",
        sql_escape(instance_id),
        sql_escape(op_type),
        node_sql,
        sql_escape(author),
        lamport_ts,
        author_seq,
        payload_str,
        sig_hex,
    ))
    .unwrap();
}

/// Apply a local CRDT operation. Validates, applies to materialized state,
/// signs with the local Ed25519 key, and records in the operation log.
///
/// Returns JSON: {op_type, node_id, lamport_ts, author_seq, author}
#[pg_extern]
fn apply_op(op_type: &str, node_id: Option<pgrx::Uuid>, payload: pgrx::JsonB) -> pgrx::JsonB {
    let (instance_id, fingerprint) = get_self_identity();
    let nid_str = node_id.map(|u| u.to_string());
    let nid_ref = nid_str.as_deref();

    // Validate
    operations::validate_op(op_type, nid_ref, &payload.0);

    // Apply to materialized state
    let affected_id = operations::apply(op_type, nid_ref, &payload.0, &instance_id);

    // Clock
    let lamport_ts = clock::next_lamport_ts();
    let author_seq = clock::next_author_seq(&fingerprint);

    // Sign
    let signing_key = identity::load_signing_key()
        .unwrap_or_else(|| error!("No signing key found — identity not initialized"));
    let signable = signer::build_signable(op_type, Some(&affected_id), author_seq, &payload.0.to_string());
    let signature = identity::sign_data(&signing_key, &signable);

    // Record
    insert_operation(
        &instance_id,
        op_type,
        Some(&affected_id),
        &fingerprint,
        lamport_ts,
        author_seq,
        &payload.0,
        &signature,
    );

    // Notify connected listeners
    let notify_payload = serde_json::json!({
        "op_type": op_type,
        "node_id": affected_id,
        "lamport_ts": lamport_ts,
        "author": fingerprint,
    });
    Spi::run(&format!(
        "NOTIFY kerai_ops, '{}'",
        sql_escape(&notify_payload.to_string()),
    ))
    .ok();

    pgrx::JsonB(serde_json::json!({
        "op_type": op_type,
        "node_id": affected_id,
        "lamport_ts": lamport_ts,
        "author_seq": author_seq,
        "author": fingerprint,
    }))
}

/// Apply a remote CRDT operation received from a peer.
/// Verifies the signature, checks causality, applies to materialized state.
///
/// Input JSON: {op_type, node_id?, author, author_seq, lamport_ts, payload, signature (hex), public_key (hex)}
/// Returns JSON: {status: "applied"|"duplicate", ...}
#[pg_extern]
fn apply_remote_op(op_json: pgrx::JsonB) -> pgrx::JsonB {
    let obj = op_json.0.as_object()
        .unwrap_or_else(|| error!("apply_remote_op expects a JSON object"));

    let op_type = obj["op_type"].as_str()
        .unwrap_or_else(|| error!("Missing 'op_type'"));
    let author = obj["author"].as_str()
        .unwrap_or_else(|| error!("Missing 'author'"));
    let author_seq = obj["author_seq"].as_i64()
        .unwrap_or_else(|| error!("Missing 'author_seq'"));
    let lamport_ts = obj["lamport_ts"].as_i64()
        .unwrap_or_else(|| error!("Missing 'lamport_ts'"));
    let payload = obj.get("payload")
        .unwrap_or_else(|| error!("Missing 'payload'"));
    let sig_hex = obj["signature"].as_str()
        .unwrap_or_else(|| error!("Missing 'signature'"));
    let pk_hex = obj["public_key"].as_str()
        .unwrap_or_else(|| error!("Missing 'public_key'"));

    let node_id = obj.get("node_id").and_then(|v| v.as_str());

    // Decode hex signature and public key
    let signature = hex::decode(sig_hex)
        .unwrap_or_else(|_| error!("Invalid hex signature"));
    let public_key = hex::decode(pk_hex)
        .unwrap_or_else(|_| error!("Invalid hex public_key"));

    // Verify signature
    if !signer::verify_op_signature(
        &public_key,
        op_type,
        node_id,
        author_seq,
        &payload.to_string(),
        &signature,
    ) {
        error!("Signature verification failed for remote op");
    }

    // Check for duplicate (idempotency)
    let exists = Spi::get_one::<bool>(&format!(
        "SELECT EXISTS(SELECT 1 FROM kerai.operations WHERE author = '{}' AND author_seq = {})",
        sql_escape(author),
        author_seq,
    ))
    .unwrap()
    .unwrap_or(false);

    if exists {
        return pgrx::JsonB(serde_json::json!({
            "status": "duplicate",
            "author": author,
            "author_seq": author_seq,
        }));
    }

    // Resolve instance_id for the remote author (auto-registers unknown peers)
    let instance_id = resolve_author_instance(author, pk_hex);

    // Validate and apply
    operations::validate_op(op_type, node_id, payload);
    let affected_id = operations::apply(op_type, node_id, payload, &instance_id);

    // Advance clocks
    clock::advance_author_seq(author, author_seq);

    // Record operation
    insert_operation(
        &instance_id,
        op_type,
        Some(&affected_id),
        author,
        lamport_ts,
        author_seq,
        payload,
        &signature,
    );

    // Notify connected listeners
    let notify_payload = serde_json::json!({
        "op_type": op_type,
        "node_id": affected_id,
        "lamport_ts": lamport_ts,
        "author": author,
    });
    Spi::run(&format!(
        "NOTIFY kerai_ops, '{}'",
        sql_escape(&notify_payload.to_string()),
    ))
    .ok();

    pgrx::JsonB(serde_json::json!({
        "status": "applied",
        "op_type": op_type,
        "node_id": affected_id,
        "lamport_ts": lamport_ts,
        "author_seq": author_seq,
        "author": author,
    }))
}

/// Get the current version vector as JSON: {"author_fingerprint": max_seq, ...}
#[pg_extern]
fn version_vector() -> pgrx::JsonB {
    clock::get_version_vector()
}

/// Get the current Lamport clock value.
#[pg_extern]
fn lamport_clock() -> i64 {
    clock::current_lamport_ts()
}

/// Get operations for a given author since a sequence number (exclusive).
/// Returns a JSON array of operation objects, including the author's public_key.
#[pg_extern]
fn ops_since(author: &str, since_seq: i64) -> pgrx::JsonB {
    let escaped = sql_escape(author);
    let json = Spi::get_one::<pgrx::JsonB>(&format!(
        "SELECT COALESCE(
            jsonb_agg(jsonb_build_object(
                'op_type', o.op_type,
                'node_id', o.node_id,
                'author', o.author,
                'author_seq', o.author_seq,
                'lamport_ts', o.lamport_ts,
                'payload', o.payload,
                'signature', encode(o.signature, 'hex'),
                'public_key', encode(i.public_key, 'hex')
            ) ORDER BY o.author_seq),
            '[]'::jsonb
        ) FROM kerai.operations o
        JOIN kerai.instances i ON i.key_fingerprint = o.author
        WHERE o.author = '{}' AND o.author_seq > {}",
        escaped,
        since_seq,
    ))
    .unwrap()
    .unwrap_or_else(|| pgrx::JsonB(serde_json::json!([])));
    json
}
