use postgres::Client;

use crate::output::{print_json, print_rows, OutputFormat};

pub fn add(
    client: &mut Client,
    name: &str,
    public_key: &str,
    endpoint: Option<&str>,
    connection: Option<&str>,
    format: &OutputFormat,
) -> Result<(), String> {
    let row = client
        .query_one(
            "SELECT kerai.register_peer($1, $2, $3, $4)::text",
            &[&name, &public_key, &endpoint, &connection],
        )
        .map_err(|e| format!("register_peer failed: {e}"))?;

    let text: String = row.get(0);
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("Invalid JSON: {e}"))?;

    let is_new = value["is_new"].as_bool().unwrap_or(false);
    let fp = value["key_fingerprint"].as_str().unwrap_or("unknown");

    if is_new {
        println!("Registered peer '{name}' ({fp})");
    } else {
        println!("Updated peer '{name}' ({fp})");
    }

    print_json(&value, format);
    Ok(())
}

pub fn list(client: &mut Client, format: &OutputFormat) -> Result<(), String> {
    let row = client
        .query_one("SELECT kerai.list_peers()::text", &[])
        .map_err(|e| format!("list_peers failed: {e}"))?;

    let text: String = row.get(0);
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("Invalid JSON: {e}"))?;

    let arr = value.as_array().ok_or("Expected JSON array")?;

    if arr.is_empty() {
        println!("No peers registered.");
        return Ok(());
    }

    let columns = vec![
        "name".into(),
        "key_fingerprint".into(),
        "endpoint".into(),
        "connection".into(),
        "last_seen".into(),
    ];

    let rows: Vec<Vec<String>> = arr
        .iter()
        .map(|p| {
            vec![
                p["name"].as_str().unwrap_or("").to_string(),
                p["key_fingerprint"].as_str().unwrap_or("").to_string(),
                p["endpoint"].as_str().unwrap_or("").to_string(),
                p["connection"].as_str().unwrap_or("").to_string(),
                p["last_seen"].as_str().unwrap_or("").to_string(),
            ]
        })
        .collect();

    print_rows(&columns, &rows, format);
    Ok(())
}

pub fn remove(client: &mut Client, name: &str) -> Result<(), String> {
    let row = client
        .query_one(
            "SELECT kerai.remove_peer($1)::text",
            &[&name],
        )
        .map_err(|e| format!("remove_peer failed: {e}"))?;

    let text: String = row.get(0);
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("Invalid JSON: {e}"))?;

    if value["removed"].as_bool().unwrap_or(false) {
        println!("Removed peer '{name}'");
    } else {
        let reason = value["reason"].as_str().unwrap_or("unknown");
        println!("Peer '{name}' not removed: {reason}");
    }
    Ok(())
}

pub fn info(client: &mut Client, name: &str, format: &OutputFormat) -> Result<(), String> {
    // Look up fingerprint by name first
    let fp_row = client
        .query_opt(
            "SELECT key_fingerprint FROM kerai.instances WHERE name = $1",
            &[&name],
        )
        .map_err(|e| format!("Query failed: {e}"))?
        .ok_or_else(|| format!("Peer '{name}' not found"))?;

    let fp: String = fp_row.get(0);

    let row = client
        .query_one(
            "SELECT kerai.get_peer($1)::text",
            &[&fp],
        )
        .map_err(|e| format!("get_peer failed: {e}"))?;

    let text: String = row.get(0);
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("Invalid JSON: {e}"))?;

    print_json(&value, format);
    Ok(())
}
