use pgrx::prelude::*;

// Plan 02: Parsing — moved to src/parser/mod.rs
// Plan 03: Reconstruction — moved to src/reconstruct/mod.rs
// Plan 04: CRDT Operations — moved to src/crdt/mod.rs
// Plan 06: Peer Sync — moved to src/peers.rs + cli/src/commands/sync.rs
// Plan 07: Query / Navigation — moved to src/query.rs

// Plan 10/11: Marketplace
#[pg_extern]
fn attest(scope: &str, asking_price: i64, compute_cost: i64) -> String {
    format!(
        "STUB: attest('{}', {}, {}) — implemented in Plan 10",
        scope, asking_price, compute_cost
    )
}

#[pg_extern]
fn auction(attestation_id: pgrx::Uuid, min_price: i64, duration_secs: i64) -> String {
    format!(
        "STUB: auction('{}', {}, {}) — implemented in Plan 11",
        attestation_id, min_price, duration_secs
    )
}

#[pg_extern]
fn bid(auction_id: pgrx::Uuid, amount: i64) -> String {
    format!(
        "STUB: bid('{}', {}) — implemented in Plan 11",
        auction_id, amount
    )
}
