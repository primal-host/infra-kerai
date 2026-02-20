use postgres::Client;
use std::env;

use crate::output::OutputFormat;

pub fn pull(client: &mut Client, format: &OutputFormat) -> Result<(), String> {
    let row = client
        .query_one("SELECT kerai.pull_init()", &[])
        .map_err(|e| format!("Failed to pull init: {e}"))?;

    let result: String = row.get(0);
    match format {
        OutputFormat::Json => println!(r#"{{"status":"{result}"}}"#),
        _ => println!("{result}"),
    }
    Ok(())
}

pub fn push(client: &mut Client, format: &OutputFormat) -> Result<(), String> {
    let row = client
        .query_one("SELECT kerai.push_init()", &[])
        .map_err(|e| format!("Failed to push init: {e}"))?;

    let result: String = row.get(0);
    match format {
        OutputFormat::Json => println!("{result}"),
        _ => {
            // Parse the JSON summary for human-readable output
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&result) {
                if let Some(err) = v.get("error") {
                    println!("error: {}", err.as_str().unwrap_or("unknown"));
                } else {
                    let added = v.get("added").and_then(|v| v.as_i64()).unwrap_or(0);
                    let updated = v.get("updated").and_then(|v| v.as_i64()).unwrap_or(0);
                    let deleted = v.get("deleted").and_then(|v| v.as_i64()).unwrap_or(0);
                    println!("applied: +{added} ~{updated} -{deleted}");
                }
            } else {
                println!("{result}");
            }
        }
    }
    Ok(())
}

pub fn diff(client: &mut Client, format: &OutputFormat) -> Result<(), String> {
    let row = client
        .query_one("SELECT kerai.diff_init()", &[])
        .map_err(|e| format!("Failed to diff init: {e}"))?;

    let result: String = row.get(0);
    match format {
        OutputFormat::Json => println!("{result}"),
        _ => {
            if let Ok(changes) = serde_json::from_str::<Vec<serde_json::Value>>(&result) {
                if changes.is_empty() {
                    println!("no changes");
                } else {
                    for change in &changes {
                        let op = change.get("op").and_then(|v| v.as_str()).unwrap_or("?");
                        let cat = change.get("category").and_then(|v| v.as_str()).unwrap_or("?");
                        let key = change.get("key").and_then(|v| v.as_str()).unwrap_or("?");
                        match op {
                            "add" => {
                                let val = change.get("value").and_then(|v| v.as_str()).unwrap_or("");
                                println!("+ {cat} {key} = {val}");
                            }
                            "update" => {
                                let old = change.get("old").and_then(|v| v.as_str()).unwrap_or("");
                                let new = change.get("new").and_then(|v| v.as_str()).unwrap_or("");
                                println!("~ {cat} {key}: {old} -> {new}");
                            }
                            "delete" => {
                                let val = change.get("value").and_then(|v| v.as_str()).unwrap_or("");
                                println!("- {cat} {key} = {val}");
                            }
                            _ => {
                                println!("? {cat} {key}");
                            }
                        }
                    }
                }
            } else {
                println!("{result}");
            }
        }
    }
    Ok(())
}

pub fn show(client: &mut Client, format: &OutputFormat) -> Result<(), String> {
    // Reuse stack show — init show just displays the stack top
    super::stack_cmd::show(client, format)
}

pub fn edit(client: &mut Client, format: &OutputFormat) -> Result<(), String> {
    // Get current stack top
    let row = client
        .query_opt("SELECT kerai.stack_peek()", &[])
        .map_err(|e| format!("Failed to peek stack: {e}"))?;

    let content = match row.and_then(|r| r.get::<_, Option<String>>(0)) {
        Some(c) => c,
        None => {
            return Err("stack is empty — run 'kerai init pull' first".to_string());
        }
    };

    // Write to temp file
    let tmp_dir = env::temp_dir();
    let tmp_path = tmp_dir.join("kerai-init.kerai");
    std::fs::write(&tmp_path, &content)
        .map_err(|e| format!("Failed to write temp file: {e}"))?;

    // Open in editor
    let editor = env::var("EDITOR").unwrap_or_else(|_| "vi".to_string());
    let status = std::process::Command::new(&editor)
        .arg(&tmp_path)
        .status()
        .map_err(|e| format!("Failed to open editor '{editor}': {e}"))?;

    if !status.success() {
        return Err(format!("Editor exited with status: {status}"));
    }

    // Read back
    let new_content = std::fs::read_to_string(&tmp_path)
        .map_err(|e| format!("Failed to read temp file: {e}"))?;

    // Clean up
    let _ = std::fs::remove_file(&tmp_path);

    // Only replace if changed
    if new_content == content {
        match format {
            OutputFormat::Json => println!(r#"{{"status":"unchanged"}}"#),
            _ => println!("no changes"),
        }
        return Ok(());
    }

    client
        .execute("SELECT kerai.stack_replace($1)", &[&new_content])
        .map_err(|e| format!("Failed to replace stack: {e}"))?;

    match format {
        OutputFormat::Json => println!(r#"{{"status":"replaced"}}"#),
        _ => println!("updated"),
    }
    Ok(())
}
