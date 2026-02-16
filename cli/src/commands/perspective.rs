use postgres::Client;

use crate::output::{print_json, OutputFormat};

pub fn run(
    client: &mut Client,
    agent: &str,
    context_id: Option<&str>,
    min_weight: Option<f64>,
    format: &OutputFormat,
) -> Result<(), String> {
    let row = client
        .query_one(
            "SELECT kerai.get_perspectives($1, $2::uuid, $3::double precision)::text",
            &[&agent, &context_id, &min_weight],
        )
        .map_err(|e| format!("get_perspectives failed: {e}"))?;

    let text: String = row.get(0);
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("Invalid JSON: {e}"))?;

    let arr = value.as_array().ok_or("Expected JSON array")?;

    if arr.is_empty() {
        println!("No perspectives found for agent '{agent}'.");
        return Ok(());
    }

    println!("{} perspective(s) for agent '{agent}':", arr.len());
    print_json(&value, format);
    Ok(())
}
