use postgres::Client;

use crate::output::{print_json, print_rows, OutputFormat};

pub fn create(
    client: &mut Client,
    attestation_id: &str,
    starting_price: i64,
    floor_price: i64,
    price_decrement: i64,
    decrement_interval: i64,
    min_bidders: i32,
    open_delay_hours: i32,
    format: &OutputFormat,
) -> Result<(), String> {
    let row = client
        .query_one(
            "SELECT kerai.create_auction($1::uuid, $2, $3, $4, $5, $6, $7)::text",
            &[
                &attestation_id,
                &starting_price,
                &floor_price,
                &price_decrement,
                &decrement_interval,
                &min_bidders,
                &open_delay_hours,
            ],
        )
        .map_err(|e| format!("create_auction failed: {e}"))?;

    let text: String = row.get(0);
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("Invalid JSON: {e}"))?;

    let id = value["id"].as_str().unwrap_or("unknown");
    println!("Created auction {id} (status: active)");
    print_json(&value, format);
    Ok(())
}

pub fn bid(
    client: &mut Client,
    auction_id: &str,
    max_price: i64,
    format: &OutputFormat,
) -> Result<(), String> {
    let row = client
        .query_one(
            "SELECT kerai.place_bid($1::uuid, $2)::text",
            &[&auction_id, &max_price],
        )
        .map_err(|e| format!("place_bid failed: {e}"))?;

    let text: String = row.get(0);
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("Invalid JSON: {e}"))?;

    println!("Bid placed on auction (max_price: {max_price})");
    print_json(&value, format);
    Ok(())
}

pub fn settle(
    client: &mut Client,
    auction_id: &str,
    format: &OutputFormat,
) -> Result<(), String> {
    let row = client
        .query_one(
            "SELECT kerai.settle_auction($1::uuid)::text",
            &[&auction_id],
        )
        .map_err(|e| format!("settle_auction failed: {e}"))?;

    let text: String = row.get(0);
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("Invalid JSON: {e}"))?;

    let price = value["settled_price"].as_i64().unwrap_or(0);
    let bidders = value["bidder_count"].as_i64().unwrap_or(0);
    println!("Auction settled at {price} credits with {bidders} bidder(s)");
    print_json(&value, format);
    Ok(())
}

pub fn open_source(
    client: &mut Client,
    auction_id: &str,
) -> Result<(), String> {
    let row = client
        .query_one(
            "SELECT kerai.open_source_auction($1::uuid)::text",
            &[&auction_id],
        )
        .map_err(|e| format!("open_source_auction failed: {e}"))?;

    let text: String = row.get(0);
    let _value: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("Invalid JSON: {e}"))?;

    println!("Auction {auction_id} open-sourced");
    Ok(())
}

pub fn browse(
    client: &mut Client,
    scope: Option<&str>,
    max_price: Option<i64>,
    status: Option<&str>,
    format: &OutputFormat,
) -> Result<(), String> {
    let row = client
        .query_one(
            "SELECT kerai.market_browse($1, $2, $3)::text",
            &[&scope, &max_price, &status],
        )
        .map_err(|e| format!("market_browse failed: {e}"))?;

    let text: String = row.get(0);
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("Invalid JSON: {e}"))?;

    let arr = value.as_array().ok_or("Expected JSON array")?;

    if arr.is_empty() {
        println!("No auctions found.");
        return Ok(());
    }

    let columns = vec![
        "auction_id".into(),
        "scope".into(),
        "claim".into(),
        "price".into(),
        "floor".into(),
        "bids".into(),
        "status".into(),
    ];

    let rows: Vec<Vec<String>> = arr
        .iter()
        .map(|a| {
            vec![
                a["auction_id"]
                    .as_str()
                    .unwrap_or("")
                    .chars()
                    .take(8)
                    .collect::<String>(),
                a["scope"].as_str().unwrap_or("").to_string(),
                a["claim_type"].as_str().unwrap_or("").to_string(),
                a["current_price"]
                    .as_i64()
                    .map(|n| n.to_string())
                    .unwrap_or_default(),
                a["floor_price"]
                    .as_i64()
                    .map(|n| n.to_string())
                    .unwrap_or_default(),
                a["bid_count"]
                    .as_i64()
                    .map(|n| n.to_string())
                    .unwrap_or("0".into()),
                a["status"].as_str().unwrap_or("").to_string(),
            ]
        })
        .collect();

    print_rows(&columns, &rows, format);
    Ok(())
}

pub fn status(
    client: &mut Client,
    auction_id: &str,
    format: &OutputFormat,
) -> Result<(), String> {
    let row = client
        .query_one(
            "SELECT kerai.market_status($1::uuid)::text",
            &[&auction_id],
        )
        .map_err(|e| format!("market_status failed: {e}"))?;

    let text: String = row.get(0);
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("Invalid JSON: {e}"))?;

    print_json(&value, format);
    Ok(())
}

pub fn balance(client: &mut Client, format: &OutputFormat) -> Result<(), String> {
    let row = client
        .query_one("SELECT kerai.market_balance()::text", &[])
        .map_err(|e| format!("market_balance failed: {e}"))?;

    let text: String = row.get(0);
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("Invalid JSON: {e}"))?;

    print_json(&value, format);
    Ok(())
}

pub fn commons(
    client: &mut Client,
    scope: Option<&str>,
    since: Option<&str>,
    format: &OutputFormat,
) -> Result<(), String> {
    let row = client
        .query_one(
            "SELECT kerai.market_commons($1, $2)::text",
            &[&scope, &since],
        )
        .map_err(|e| format!("market_commons failed: {e}"))?;

    let text: String = row.get(0);
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("Invalid JSON: {e}"))?;

    let arr = value.as_array().ok_or("Expected JSON array")?;

    if arr.is_empty() {
        println!("No open-sourced knowledge found.");
        return Ok(());
    }

    let columns = vec![
        "auction_id".into(),
        "scope".into(),
        "claim".into(),
        "settled_price".into(),
        "open_sourced_at".into(),
    ];

    let rows: Vec<Vec<String>> = arr
        .iter()
        .map(|a| {
            vec![
                a["auction_id"]
                    .as_str()
                    .unwrap_or("")
                    .chars()
                    .take(8)
                    .collect::<String>(),
                a["scope"].as_str().unwrap_or("").to_string(),
                a["claim_type"].as_str().unwrap_or("").to_string(),
                a["settled_price"]
                    .as_i64()
                    .map(|n| n.to_string())
                    .unwrap_or("-".into()),
                a["open_sourced_at"].as_str().unwrap_or("").to_string(),
            ]
        })
        .collect();

    print_rows(&columns, &rows, format);
    Ok(())
}

pub fn stats(client: &mut Client, format: &OutputFormat) -> Result<(), String> {
    let row = client
        .query_one("SELECT kerai.market_stats()::text", &[])
        .map_err(|e| format!("market_stats failed: {e}"))?;

    let text: String = row.get(0);
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("Invalid JSON: {e}"))?;

    print_json(&value, format);
    Ok(())
}
