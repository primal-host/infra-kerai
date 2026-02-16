use postgres::Client;

use crate::output::{print_json, print_rows, OutputFormat};

pub fn create(
    client: &mut Client,
    scope: &str,
    description: &str,
    reward: i64,
    success_command: Option<&str>,
    expires: Option<&str>,
    format: &OutputFormat,
) -> Result<(), String> {
    let row = client
        .query_one(
            "SELECT kerai.create_bounty($1, $2, $3, $4, $5)::text",
            &[&scope, &description, &reward, &success_command, &expires],
        )
        .map_err(|e| format!("create_bounty failed: {e}"))?;

    let text: String = row.get(0);
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("Invalid JSON: {e}"))?;

    let id = value["id"].as_str().unwrap_or("unknown");
    println!("Created bounty {id} ({reward} kōi)");
    print_json(&value, format);
    Ok(())
}

pub fn list(
    client: &mut Client,
    status: Option<&str>,
    scope: Option<&str>,
    format: &OutputFormat,
) -> Result<(), String> {
    let row = client
        .query_one(
            "SELECT kerai.list_bounties($1, $2)::text",
            &[&status, &scope],
        )
        .map_err(|e| format!("list_bounties failed: {e}"))?;

    let text: String = row.get(0);
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("Invalid JSON: {e}"))?;

    let arr = value.as_array().ok_or("Expected JSON array")?;

    if arr.is_empty() {
        println!("No bounties found.");
        return Ok(());
    }

    let columns = vec![
        "id".into(),
        "scope".into(),
        "description".into(),
        "reward".into(),
        "status".into(),
    ];

    let rows: Vec<Vec<String>> = arr
        .iter()
        .map(|b| {
            vec![
                b["id"]
                    .as_str()
                    .unwrap_or("")
                    .chars()
                    .take(8)
                    .collect::<String>(),
                b["scope"].as_str().unwrap_or("").to_string(),
                b["description"]
                    .as_str()
                    .unwrap_or("")
                    .chars()
                    .take(40)
                    .collect::<String>(),
                b["reward"]
                    .as_i64()
                    .map(|n| n.to_string())
                    .unwrap_or_default(),
                b["status"].as_str().unwrap_or("").to_string(),
            ]
        })
        .collect();

    print_rows(&columns, &rows, format);
    Ok(())
}

pub fn show(
    client: &mut Client,
    bounty_id: &str,
    format: &OutputFormat,
) -> Result<(), String> {
    let row = client
        .query_one(
            "SELECT kerai.get_bounty($1::uuid)::text",
            &[&bounty_id],
        )
        .map_err(|e| format!("get_bounty failed: {e}"))?;

    let text: String = row.get(0);
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("Invalid JSON: {e}"))?;

    print_json(&value, format);
    Ok(())
}

pub fn claim(
    client: &mut Client,
    bounty_id: &str,
    wallet_id: &str,
    format: &OutputFormat,
) -> Result<(), String> {
    let row = client
        .query_one(
            "SELECT kerai.claim_bounty($1::uuid, $2::uuid)::text",
            &[&bounty_id, &wallet_id],
        )
        .map_err(|e| format!("claim_bounty failed: {e}"))?;

    let text: String = row.get(0);
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("Invalid JSON: {e}"))?;

    println!("Bounty {bounty_id} claimed");
    print_json(&value, format);
    Ok(())
}

pub fn settle(
    client: &mut Client,
    bounty_id: &str,
    format: &OutputFormat,
) -> Result<(), String> {
    let row = client
        .query_one(
            "SELECT kerai.settle_bounty($1::uuid)::text",
            &[&bounty_id],
        )
        .map_err(|e| format!("settle_bounty failed: {e}"))?;

    let text: String = row.get(0);
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("Invalid JSON: {e}"))?;

    let reward = value["reward"].as_i64().unwrap_or(0);
    println!("Bounty {bounty_id} settled ({reward} kōi transferred)");
    print_json(&value, format);
    Ok(())
}
