use postgres::Client;

use crate::output::{print_json, print_rows, OutputFormat};

pub fn create(
    client: &mut Client,
    wallet_type: &str,
    label: Option<&str>,
    format: &OutputFormat,
) -> Result<(), String> {
    let row = client
        .query_one(
            "SELECT kerai.create_wallet($1, $2)::text",
            &[&wallet_type, &label],
        )
        .map_err(|e| format!("create_wallet failed: {e}"))?;

    let text: String = row.get(0);
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("Invalid JSON: {e}"))?;

    let id = value["id"].as_str().unwrap_or("unknown");
    let fp = value["key_fingerprint"].as_str().unwrap_or("");
    println!("Created {wallet_type} wallet {id} (fingerprint: {fp})");
    print_json(&value, format);
    Ok(())
}

pub fn list(
    client: &mut Client,
    type_filter: Option<&str>,
    format: &OutputFormat,
) -> Result<(), String> {
    let row = client
        .query_one(
            "SELECT kerai.list_wallets($1)::text",
            &[&type_filter],
        )
        .map_err(|e| format!("list_wallets failed: {e}"))?;

    let text: String = row.get(0);
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("Invalid JSON: {e}"))?;

    let arr = value.as_array().ok_or("Expected JSON array")?;

    if arr.is_empty() {
        println!("No wallets found.");
        return Ok(());
    }

    let columns = vec![
        "id".into(),
        "type".into(),
        "label".into(),
        "fingerprint".into(),
        "created".into(),
    ];

    let rows: Vec<Vec<String>> = arr
        .iter()
        .map(|w| {
            vec![
                w["id"]
                    .as_str()
                    .unwrap_or("")
                    .chars()
                    .take(8)
                    .collect::<String>(),
                w["wallet_type"].as_str().unwrap_or("").to_string(),
                w["label"].as_str().unwrap_or("-").to_string(),
                w["key_fingerprint"]
                    .as_str()
                    .unwrap_or("")
                    .chars()
                    .take(12)
                    .collect::<String>(),
                w["created_at"].as_str().unwrap_or("").to_string(),
            ]
        })
        .collect();

    print_rows(&columns, &rows, format);
    Ok(())
}

pub fn balance(
    client: &mut Client,
    wallet_id: Option<&str>,
    format: &OutputFormat,
) -> Result<(), String> {
    let row = match wallet_id {
        Some(id) => client
            .query_one(
                "SELECT kerai.get_wallet_balance($1::uuid)::text",
                &[&id],
            )
            .map_err(|e| format!("get_wallet_balance failed: {e}"))?,
        None => client
            .query_one(
                "SELECT kerai.get_wallet_balance(
                    (SELECT w.id FROM kerai.wallets w
                     JOIN kerai.instances i ON w.instance_id = i.id
                     WHERE i.is_self = true AND w.wallet_type = 'instance')
                )::text",
                &[],
            )
            .map_err(|e| format!("get_wallet_balance failed: {e}"))?,
    };

    let text: String = row.get(0);
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("Invalid JSON: {e}"))?;

    let bal = value["balance"].as_i64().unwrap_or(0);
    println!("Balance: {bal} kōi");
    print_json(&value, format);
    Ok(())
}

pub fn transfer(
    client: &mut Client,
    from: &str,
    to: &str,
    amount: i64,
    reason: Option<&str>,
    format: &OutputFormat,
) -> Result<(), String> {
    let row = client
        .query_one(
            "SELECT kerai.transfer_koi($1::uuid, $2::uuid, $3, $4)::text",
            &[&from, &to, &amount, &reason],
        )
        .map_err(|e| format!("transfer_koi failed: {e}"))?;

    let text: String = row.get(0);
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("Invalid JSON: {e}"))?;

    println!("Transferred {amount} kōi");
    print_json(&value, format);
    Ok(())
}

pub fn history(
    client: &mut Client,
    wallet_id: &str,
    limit: i32,
    format: &OutputFormat,
) -> Result<(), String> {
    let row = client
        .query_one(
            "SELECT kerai.wallet_history($1::uuid, $2)::text",
            &[&wallet_id, &limit],
        )
        .map_err(|e| format!("wallet_history failed: {e}"))?;

    let text: String = row.get(0);
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("Invalid JSON: {e}"))?;

    let arr = value.as_array().ok_or("Expected JSON array")?;

    if arr.is_empty() {
        println!("No transaction history.");
        return Ok(());
    }

    let columns = vec![
        "direction".into(),
        "amount".into(),
        "reason".into(),
        "timestamp".into(),
        "created".into(),
    ];

    let rows: Vec<Vec<String>> = arr
        .iter()
        .map(|e| {
            vec![
                e["direction"].as_str().unwrap_or("").to_string(),
                e["amount"]
                    .as_i64()
                    .map(|n| n.to_string())
                    .unwrap_or_default(),
                e["reason"].as_str().unwrap_or("").to_string(),
                e["timestamp"]
                    .as_i64()
                    .map(|n| n.to_string())
                    .unwrap_or_default(),
                e["created_at"].as_str().unwrap_or("").to_string(),
            ]
        })
        .collect();

    print_rows(&columns, &rows, format);
    Ok(())
}
