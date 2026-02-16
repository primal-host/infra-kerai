use postgres::{Client, NoTls};

/// Sync protocol: pull-then-push between local and peer databases.
///
/// 1. Look up peer's connection string from kerai.instances
/// 2. Connect to peer's Postgres
/// 3. Get both version vectors
/// 4. Pull: for each author where peer is ahead, fetch ops and apply locally
/// 5. Push: for each author where local is ahead, fetch ops and apply on peer
/// 6. Print summary
pub fn run(client: &mut Client, peer_name: &str) -> Result<(), String> {
    // Look up peer's connection string
    let peer_row = client
        .query_opt(
            "SELECT connection FROM kerai.instances WHERE name = $1 AND is_self = false",
            &[&peer_name],
        )
        .map_err(|e| format!("Failed to look up peer: {e}"))?
        .ok_or_else(|| format!("Peer '{peer_name}' not found"))?;

    let peer_conn: Option<String> = peer_row.get(0);
    let peer_conn = peer_conn.ok_or_else(|| {
        format!("Peer '{peer_name}' has no connection string. Use: kerai peer add {peer_name} --public-key <hex> --connection <pg_url>")
    })?;

    // Connect to peer
    let mut peer_client =
        Client::connect(&peer_conn, NoTls).map_err(|e| format!("Cannot connect to peer: {e}"))?;

    // Get both version vectors
    let local_vv = get_version_vector(client)?;
    let peer_vv = get_version_vector(&mut peer_client)?;

    let mut pulled = 0u64;
    let mut pushed = 0u64;

    // Pull: for each author in peer's VV where peer is ahead
    for (author, peer_seq) in &peer_vv {
        let local_seq = local_vv.get(author).copied().unwrap_or(0);
        if *peer_seq > local_seq {
            let ops = get_ops_since(&mut peer_client, author, local_seq)?;
            for op in &ops {
                apply_remote_op(client, op)?;
                pulled += 1;
            }
        }
    }

    // Push: for each author in local VV where local is ahead
    for (author, local_seq) in &local_vv {
        let peer_seq = peer_vv.get(author).copied().unwrap_or(0);
        if *local_seq > peer_seq {
            let ops = get_ops_since(client, author, peer_seq)?;
            for op in &ops {
                apply_remote_op(&mut peer_client, op)?;
                pushed += 1;
            }
        }
    }

    // Update last_seen
    client
        .execute(
            "UPDATE kerai.instances SET last_seen = now() WHERE name = $1",
            &[&peer_name],
        )
        .map_err(|e| format!("Failed to update last_seen: {e}"))?;

    println!("Synced with '{peer_name}': pulled {pulled}, pushed {pushed}");

    Ok(())
}

/// Get the version vector from a database as a map of author -> max_seq.
fn get_version_vector(
    client: &mut Client,
) -> Result<std::collections::HashMap<String, i64>, String> {
    let row = client
        .query_one("SELECT kerai.version_vector()::text", &[])
        .map_err(|e| format!("version_vector failed: {e}"))?;

    let text: String = row.get(0);
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("Invalid JSON: {e}"))?;

    let mut map = std::collections::HashMap::new();
    if let Some(obj) = value.as_object() {
        for (k, v) in obj {
            if let Some(seq) = v.as_i64() {
                map.insert(k.clone(), seq);
            }
        }
    }
    Ok(map)
}

/// Get operations from a database for a given author since a sequence number.
fn get_ops_since(
    client: &mut Client,
    author: &str,
    since_seq: i64,
) -> Result<Vec<serde_json::Value>, String> {
    let row = client
        .query_one(
            "SELECT kerai.ops_since($1, $2)::text",
            &[&author, &since_seq],
        )
        .map_err(|e| format!("ops_since failed: {e}"))?;

    let text: String = row.get(0);
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("Invalid JSON: {e}"))?;

    value
        .as_array()
        .cloned()
        .ok_or_else(|| "Expected JSON array from ops_since".to_string())
}

/// Apply a remote operation on a target database.
fn apply_remote_op(client: &mut Client, op: &serde_json::Value) -> Result<(), String> {
    let op_json = serde_json::to_string(op).map_err(|e| format!("JSON encode failed: {e}"))?;

    let row = client
        .query_one(
            "SELECT kerai.apply_remote_op($1::jsonb)::text",
            &[&op_json],
        )
        .map_err(|e| format!("apply_remote_op failed: {e}"))?;

    let text: String = row.get(0);
    let result: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("Invalid JSON: {e}"))?;

    let status = result["status"].as_str().unwrap_or("unknown");
    if status == "duplicate" {
        // Skip silently — idempotent
    }

    Ok(())
}
