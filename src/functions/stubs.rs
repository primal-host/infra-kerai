use pgrx::prelude::*;

// Plan 02: Parsing
#[pg_extern(schema = "kerai")]
fn parse_crate(path: &str) -> String {
    format!("STUB: parse_crate('{}') — implemented in Plan 02", path)
}

#[pg_extern(schema = "kerai")]
fn parse_file(path: &str) -> String {
    format!("STUB: parse_file('{}') — implemented in Plan 02", path)
}

#[pg_extern(schema = "kerai")]
fn parse_source(language: &str, source: &str) -> String {
    format!(
        "STUB: parse_source('{}', '{}...') — implemented in Plan 02",
        language,
        &source[..source.len().min(40)]
    )
}

// Plan 03: Reconstruction
#[pg_extern(schema = "kerai")]
fn reconstruct_file(node_id: pgrx::Uuid) -> String {
    format!(
        "STUB: reconstruct_file('{}') — implemented in Plan 03",
        node_id
    )
}

#[pg_extern(schema = "kerai")]
fn reconstruct_crate(crate_name: &str) -> String {
    format!(
        "STUB: reconstruct_crate('{}') — implemented in Plan 03",
        crate_name
    )
}

// Plan 04: CRDT Operations
#[pg_extern(schema = "kerai")]
fn apply_op(op_type: &str, node_id: pgrx::Uuid, _payload: pgrx::JsonB) -> String {
    format!(
        "STUB: apply_op('{}', '{}', ...) — implemented in Plan 04",
        op_type, node_id
    )
}

#[pg_extern(schema = "kerai")]
fn version_vector() -> String {
    "STUB: version_vector() — implemented in Plan 04".to_string()
}

// Plan 06: Sync
#[pg_extern(schema = "kerai")]
fn sync(peer: &str) -> String {
    format!("STUB: sync('{}') — implemented in Plan 06", peer)
}

#[pg_extern(schema = "kerai")]
fn join_network(endpoint: &str) -> String {
    format!(
        "STUB: join_network('{}') — implemented in Plan 06",
        endpoint
    )
}

// Plan 07: Query / Navigation
#[pg_extern(schema = "kerai")]
fn find(pattern: &str) -> String {
    format!("STUB: find('{}') — implemented in Plan 07", pattern)
}

#[pg_extern(schema = "kerai")]
fn refs(symbol: &str) -> String {
    format!("STUB: refs('{}') — implemented in Plan 07", symbol)
}

// Plan 10/11: Marketplace
#[pg_extern(schema = "kerai")]
fn attest(scope: &str, asking_price: i64, compute_cost: i64) -> String {
    format!(
        "STUB: attest('{}', {}, {}) — implemented in Plan 10",
        scope, asking_price, compute_cost
    )
}

#[pg_extern(schema = "kerai")]
fn auction(attestation_id: pgrx::Uuid, min_price: i64, duration_secs: i64) -> String {
    format!(
        "STUB: auction('{}', {}, {}) — implemented in Plan 11",
        attestation_id, min_price, duration_secs
    )
}

#[pg_extern(schema = "kerai")]
fn bid(auction_id: pgrx::Uuid, amount: i64) -> String {
    format!(
        "STUB: bid('{}', {}) — implemented in Plan 11",
        auction_id, amount
    )
}
