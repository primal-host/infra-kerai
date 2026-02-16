use postgres::Client;

pub fn run(client: &mut Client) -> Result<(), String> {
    // Basic connectivity check
    client
        .query_one("SELECT 1", &[])
        .map_err(|e| format!("Ping failed: {e}"))?;

    // Extension check
    let row = client
        .query_one("SELECT kerai.status()::text", &[])
        .map_err(|e| format!("Extension not loaded: {e}"))?;

    let status_text: String = row.get(0);
    let status: serde_json::Value =
        serde_json::from_str(&status_text).unwrap_or(serde_json::Value::Null);

    let name = status["instance_name"]
        .as_str()
        .unwrap_or("unknown");
    let fingerprint = status["fingerprint"]
        .as_str()
        .unwrap_or("unknown");

    println!("Connected to {name} ({fingerprint})");
    Ok(())
}
