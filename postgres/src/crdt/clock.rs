/// Lamport clock and version vector management for CRDT operations.
use pgrx::prelude::*;

/// Get the current maximum Lamport timestamp from the operations table.
pub fn current_lamport_ts() -> i64 {
    Spi::get_one::<i64>("SELECT COALESCE(MAX(lamport_ts), 0)::bigint FROM kerai.operations")
        .unwrap()
        .unwrap_or(0)
}

/// Get the next Lamport timestamp (current + 1).
pub fn next_lamport_ts() -> i64 {
    current_lamport_ts() + 1
}

/// Increment and return the next author sequence number.
/// Uses UPSERT to atomically advance the version_vector entry.
pub fn next_author_seq(author: &str) -> i64 {
    let escaped = author.replace('\'', "''");
    Spi::get_one::<i64>(&format!(
        "INSERT INTO kerai.version_vector (author, max_seq) VALUES ('{}', 1)
         ON CONFLICT (author) DO UPDATE SET max_seq = kerai.version_vector.max_seq + 1
         RETURNING max_seq",
        escaped
    ))
    .unwrap()
    .unwrap()
}

/// Advance the version_vector entry for a remote author to at least `seq`.
/// Uses GREATEST semantics — never goes backwards.
pub fn advance_author_seq(author: &str, seq: i64) {
    let escaped = author.replace('\'', "''");
    Spi::run(&format!(
        "INSERT INTO kerai.version_vector (author, max_seq) VALUES ('{}', {})
         ON CONFLICT (author) DO UPDATE SET max_seq = GREATEST(kerai.version_vector.max_seq, {})",
        escaped, seq, seq
    ))
    .unwrap();
}

/// Get the full version vector as a JSON object: {"author": max_seq, ...}
pub fn get_version_vector() -> pgrx::JsonB {
    let json = Spi::get_one::<pgrx::JsonB>(
        "SELECT COALESCE(
            jsonb_object_agg(author, max_seq),
            '{}'::jsonb
        ) FROM kerai.version_vector",
    )
    .unwrap()
    .unwrap_or_else(|| pgrx::JsonB(serde_json::json!({})));
    json
}
