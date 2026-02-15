use pgrx::prelude::*;

/// Returns JSON status of the Kerai instance
#[pg_extern]
fn status() -> pgrx::JsonB {
    let instance_id = Spi::get_one::<String>(
        "SELECT id::text FROM kerai.instances WHERE is_self = true",
    )
    .unwrap_or(None)
    .unwrap_or_else(|| "unknown".to_string());

    let name = Spi::get_one::<String>(
        "SELECT name FROM kerai.instances WHERE is_self = true",
    )
    .unwrap_or(None)
    .unwrap_or_else(|| "unknown".to_string());

    let fingerprint = Spi::get_one::<String>(
        "SELECT key_fingerprint FROM kerai.instances WHERE is_self = true",
    )
    .unwrap_or(None)
    .unwrap_or_else(|| "unknown".to_string());

    let peer_count = Spi::get_one::<i64>(
        "SELECT count(*)::bigint FROM kerai.instances WHERE is_self = false",
    )
    .unwrap_or(None)
    .unwrap_or(0);

    let node_count = Spi::get_one::<i64>(
        "SELECT count(*)::bigint FROM kerai.nodes",
    )
    .unwrap_or(None)
    .unwrap_or(0);

    let version_count = Spi::get_one::<i64>(
        "SELECT count(*)::bigint FROM kerai.versions",
    )
    .unwrap_or(None)
    .unwrap_or(0);

    let status = serde_json::json!({
        "instance_id": instance_id,
        "name": name,
        "fingerprint": fingerprint,
        "peer_count": peer_count,
        "node_count": node_count,
        "version_count": version_count,
        "version": "0.1.0"
    });

    pgrx::JsonB(status)
}
