use postgres::Client;

use crate::output::{print_json, OutputFormat};

pub fn run(
    client: &mut Client,
    context_id: Option<&str>,
    min_agents: Option<i32>,
    min_weight: Option<f64>,
    format: &OutputFormat,
) -> Result<(), String> {
    let row = client
        .query_one(
            "SELECT kerai.consensus($1::uuid, $2::integer, $3::double precision)::text",
            &[&context_id, &min_agents, &min_weight],
        )
        .map_err(|e| format!("consensus failed: {e}"))?;

    let text: String = row.get(0);
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("Invalid JSON: {e}"))?;

    let arr = value.as_array().ok_or("Expected JSON array")?;

    if arr.is_empty() {
        println!("No consensus found matching criteria.");
        return Ok(());
    }

    println!("{} node(s) with multi-agent consensus:", arr.len());
    print_json(&value, format);
    Ok(())
}
