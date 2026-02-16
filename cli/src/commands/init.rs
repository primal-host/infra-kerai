use postgres::Client;
use std::path::Path;

use crate::output::{print_json, OutputFormat};

pub fn run(
    client: &mut Client,
    path: Option<&str>,
    db_conn: &str,
    format: &OutputFormat,
) -> Result<(), String> {
    // Resolve project path
    let project_path = match path {
        Some(p) => std::fs::canonicalize(p).map_err(|e| format!("Invalid path '{p}': {e}"))?,
        None => std::env::current_dir().map_err(|e| format!("Cannot get cwd: {e}"))?,
    };

    let project_str = project_path.to_string_lossy();

    // Derive crate name from directory name
    let crate_name = project_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown".into());

    // Create .kerai/config.toml
    let kerai_dir = project_path.join(".kerai");
    if !kerai_dir.exists() {
        std::fs::create_dir_all(&kerai_dir)
            .map_err(|e| format!("Failed to create .kerai/: {e}"))?;
    }

    let config_path = kerai_dir.join("config.toml");
    if !config_path.exists() {
        let config_content = format!(
            "[default]\nconnection = \"{db_conn}\"\ncrate_name = \"{crate_name}\"\n"
        );
        std::fs::write(&config_path, config_content)
            .map_err(|e| format!("Failed to write config: {e}"))?;
        println!("Created {}", config_path.display());
    }

    // Add .kerai/ to .gitignore if not already present
    add_to_gitignore(&project_path);

    // Ensure extension is loaded
    crate::db::ensure_extension(client)?;

    // Parse the crate
    let row = client
        .query_one(
            "SELECT kerai.parse_crate($1)::text",
            &[&project_str.as_ref()],
        )
        .map_err(|e| format!("parse_crate failed: {e}"))?;

    let text: String = row.get(0);
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("Invalid JSON: {e}"))?;

    print_json(&value, format);
    Ok(())
}

fn add_to_gitignore(project_path: &Path) {
    let gitignore = project_path.join(".gitignore");
    if gitignore.exists() {
        if let Ok(content) = std::fs::read_to_string(&gitignore) {
            if content.lines().any(|l| l.trim() == ".kerai/") {
                return;
            }
            // Append
            let suffix = if content.ends_with('\n') { "" } else { "\n" };
            let _ = std::fs::write(&gitignore, format!("{content}{suffix}.kerai/\n"));
        }
    } else {
        let _ = std::fs::write(&gitignore, ".kerai/\n");
    }
}
