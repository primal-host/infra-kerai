use postgres::Client;

use crate::output::{print_json, OutputFormat};

pub fn create(
    client: &mut Client,
    agent: &str,
    dim: Option<i32>,
    heads: Option<i32>,
    layers: Option<i32>,
    context_len: Option<i32>,
    scope: Option<&str>,
    format: &OutputFormat,
) -> Result<(), String> {
    let row = client
        .query_one(
            "SELECT kerai.create_model($1, $2, $3, $4, $5, $6)::text",
            &[&agent, &dim, &heads, &layers, &context_len, &scope],
        )
        .map_err(|e| format!("create_model failed: {e}"))?;

    let text: String = row.get(0);
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("Invalid JSON: {e}"))?;

    let vocab = value["vocab_size"].as_u64().unwrap_or(0);
    let params = value["param_count"].as_u64().unwrap_or(0);
    println!("Created model for '{}' (vocab={}, params={})", agent, vocab, params);
    print_json(&value, format);
    Ok(())
}

pub fn train(
    client: &mut Client,
    agent: &str,
    walks: Option<&str>,
    sequences: Option<i32>,
    steps: Option<i32>,
    lr: Option<f64>,
    scope: Option<&str>,
    perspective_agent: Option<&str>,
    format: &OutputFormat,
) -> Result<(), String> {
    let row = client
        .query_one(
            "SELECT kerai.train_model($1, $2, $3, $4, $5, $6, $7)::text",
            &[&agent, &walks, &sequences, &steps, &lr, &scope, &perspective_agent],
        )
        .map_err(|e| format!("train_model failed: {e}"))?;

    let text: String = row.get(0);
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("Invalid JSON: {e}"))?;

    let init_loss = value["initial_loss"].as_f64().unwrap_or(0.0);
    let final_loss = value["final_loss"].as_f64().unwrap_or(0.0);
    let dur = value["duration_ms"].as_i64().unwrap_or(0);
    println!(
        "Training complete: loss {:.4} → {:.4} ({}ms)",
        init_loss, final_loss, dur
    );
    print_json(&value, format);
    Ok(())
}

pub fn predict(
    client: &mut Client,
    agent: &str,
    context: &str,
    top_k: Option<i32>,
    format: &OutputFormat,
) -> Result<(), String> {
    // Parse comma-separated UUIDs into JSON array
    let uuids: Vec<&str> = context.split(',').map(|s| s.trim()).collect();
    let json_array = serde_json::json!(uuids);
    let json_str = json_array.to_string();

    let row = client
        .query_one(
            "SELECT kerai.predict_next($1, $2::jsonb, $3)::text",
            &[&agent, &json_str, &top_k],
        )
        .map_err(|e| format!("predict_next failed: {e}"))?;

    let text: String = row.get(0);
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("Invalid JSON: {e}"))?;

    print_json(&value, format);
    Ok(())
}

pub fn search(
    client: &mut Client,
    agent: &str,
    query: &str,
    top_k: Option<i32>,
    format: &OutputFormat,
) -> Result<(), String> {
    let row = client
        .query_one(
            "SELECT kerai.neural_search($1, $2, NULL, $3)::text",
            &[&agent, &query, &top_k],
        )
        .map_err(|e| format!("neural_search failed: {e}"))?;

    let text: String = row.get(0);
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("Invalid JSON: {e}"))?;

    let results = value["results"].as_array().map(|a| a.len()).unwrap_or(0);
    println!("{} results", results);
    print_json(&value, format);
    Ok(())
}

pub fn ensemble(
    client: &mut Client,
    agents: &str,
    context: &str,
    top_k: Option<i32>,
    format: &OutputFormat,
) -> Result<(), String> {
    let agent_names: Vec<&str> = agents.split(',').map(|s| s.trim()).collect();
    let agents_json = serde_json::json!(agent_names).to_string();

    let uuids: Vec<&str> = context.split(',').map(|s| s.trim()).collect();
    let context_json = serde_json::json!(uuids).to_string();

    let row = client
        .query_one(
            "SELECT kerai.ensemble_predict($1::jsonb, $2::jsonb, $3)::text",
            &[&agents_json, &context_json, &top_k],
        )
        .map_err(|e| format!("ensemble_predict failed: {e}"))?;

    let text: String = row.get(0);
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("Invalid JSON: {e}"))?;

    print_json(&value, format);
    Ok(())
}

pub fn info(
    client: &mut Client,
    agent: &str,
    format: &OutputFormat,
) -> Result<(), String> {
    let row = client
        .query_one(
            "SELECT kerai.model_info($1)::text",
            &[&agent],
        )
        .map_err(|e| format!("model_info failed: {e}"))?;

    let text: String = row.get(0);
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("Invalid JSON: {e}"))?;

    print_json(&value, format);
    Ok(())
}

pub fn delete(
    client: &mut Client,
    agent: &str,
    format: &OutputFormat,
) -> Result<(), String> {
    let row = client
        .query_one(
            "SELECT kerai.delete_model($1)::text",
            &[&agent],
        )
        .map_err(|e| format!("delete_model failed: {e}"))?;

    let text: String = row.get(0);
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("Invalid JSON: {e}"))?;

    println!("Model deleted for '{}'", agent);
    print_json(&value, format);
    Ok(())
}
