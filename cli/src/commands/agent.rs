use postgres::Client;

use crate::output::{print_json, print_rows, OutputFormat};

pub fn add(
    client: &mut Client,
    name: &str,
    kind: &str,
    model: Option<&str>,
    format: &OutputFormat,
) -> Result<(), String> {
    let row = client
        .query_one(
            "SELECT kerai.register_agent($1, $2, $3, NULL)::text",
            &[&name, &kind, &model],
        )
        .map_err(|e| format!("register_agent failed: {e}"))?;

    let text: String = row.get(0);
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("Invalid JSON: {e}"))?;

    let is_new = value["is_new"].as_bool().unwrap_or(false);
    if is_new {
        println!("Registered agent '{name}' (kind: {kind})");
    } else {
        println!("Updated agent '{name}' (kind: {kind})");
    }

    print_json(&value, format);
    Ok(())
}

pub fn list(
    client: &mut Client,
    kind: Option<&str>,
    format: &OutputFormat,
) -> Result<(), String> {
    let row = client
        .query_one("SELECT kerai.list_agents($1)::text", &[&kind])
        .map_err(|e| format!("list_agents failed: {e}"))?;

    let text: String = row.get(0);
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("Invalid JSON: {e}"))?;

    let arr = value.as_array().ok_or("Expected JSON array")?;

    if arr.is_empty() {
        println!("No agents registered.");
        return Ok(());
    }

    let columns = vec![
        "name".into(),
        "kind".into(),
        "model".into(),
        "created_at".into(),
    ];

    let rows: Vec<Vec<String>> = arr
        .iter()
        .map(|a| {
            vec![
                a["name"].as_str().unwrap_or("").to_string(),
                a["kind"].as_str().unwrap_or("").to_string(),
                a["model"].as_str().unwrap_or("").to_string(),
                a["created_at"].as_str().unwrap_or("").to_string(),
            ]
        })
        .collect();

    print_rows(&columns, &rows, format);
    Ok(())
}

pub fn remove(client: &mut Client, name: &str) -> Result<(), String> {
    let row = client
        .query_one("SELECT kerai.remove_agent($1)::text", &[&name])
        .map_err(|e| format!("remove_agent failed: {e}"))?;

    let text: String = row.get(0);
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("Invalid JSON: {e}"))?;

    if value["removed"].as_bool().unwrap_or(false) {
        println!("Removed agent '{name}'");
    } else {
        let reason = value["reason"].as_str().unwrap_or("unknown");
        println!("Agent '{name}' not removed: {reason}");
    }
    Ok(())
}

pub fn info(client: &mut Client, name: &str, format: &OutputFormat) -> Result<(), String> {
    let row = client
        .query_one("SELECT kerai.get_agent($1)::text", &[&name])
        .map_err(|e| format!("get_agent failed: {e}"))?;

    let text: String = row.get(0);
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("Invalid JSON: {e}"))?;

    print_json(&value, format);
    Ok(())
}
