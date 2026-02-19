/// Centralized SQL string helpers for SPI queries.
///
/// pgrx 0.17 supports parameterized queries via `SpiClient::select`
/// and `Spi::run_with_args` ($1-style positional parameters), but this
/// codebase predates those and uses string interpolation throughout.
/// These helpers centralize escaping to reduce duplication and bug risk.
///
/// TODO: Migrate high-traffic queries (especially `inserter.rs` batch
/// inserts) to use $1-style parameterized queries for proper type safety
/// instead of string interpolation.

/// Escape a string for use in a SQL literal (double single quotes).
pub fn sql_escape(s: &str) -> String {
    s.replace('\'', "''")
}

/// Format a string as a SQL text literal: `'escaped_value'`
pub fn sql_text(s: &str) -> String {
    format!("'{}'", sql_escape(s))
}

/// Format a UUID string as a SQL UUID literal: `'value'::uuid`
pub fn sql_uuid(id: &str) -> String {
    format!("'{}'::uuid", sql_escape(id))
}

/// Format an `Option<String>` as a SQL value (text literal or NULL).
pub fn sql_opt_text(val: &Option<String>) -> String {
    match val {
        Some(s) => sql_text(s),
        None => "NULL".to_string(),
    }
}

/// Format an `Option<i32>` as a SQL value (integer or NULL).
pub fn sql_opt_int(val: Option<i32>) -> String {
    match val {
        Some(i) => i.to_string(),
        None => "NULL".to_string(),
    }
}

/// Format a JSONB value as a SQL literal: `'escaped_json'::jsonb`
pub fn sql_jsonb(val: &serde_json::Value) -> String {
    format!("'{}'::jsonb", sql_escape(&val.to_string()))
}

/// Format an ltree path as a SQL literal: `'path'::ltree`
pub fn sql_ltree(path: &str) -> String {
    format!("'{}'::ltree", sql_escape(path))
}
