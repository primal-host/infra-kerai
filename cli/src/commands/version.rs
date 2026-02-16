use postgres::Client;

use crate::output::{print_json, OutputFormat};

pub fn run(client: &mut Client, format: &OutputFormat) -> Result<(), String> {
    let row = client
        .query_one("SELECT kerai.version_vector()::text", &[])
        .map_err(|e| format!("Failed to get version vector: {e}"))?;

    let text: String = row.get(0);
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("Invalid JSON: {e}"))?;

    print_json(&value, format);
    Ok(())
}
