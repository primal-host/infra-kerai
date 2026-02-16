use postgres::Client;
use std::path::Path;

use crate::config;

pub fn run(client: &mut Client, message: Option<&str>) -> Result<(), String> {
    let project_root = config::find_project_root()
        .ok_or("No .kerai/config.toml found. Run 'kerai init' first.")?;

    let _ = message; // Reserved for future commit message tracking

    // Walk for .rs files, skipping target/ and tgt/ and .kerai/
    let mut rs_files: Vec<String> = Vec::new();
    walk_rs_files(&project_root, &project_root, &mut rs_files)?;

    if rs_files.is_empty() {
        println!("No .rs files found.");
        return Ok(());
    }

    println!("Parsing {} files...", rs_files.len());

    let mut total_nodes = 0u64;
    let mut total_edges = 0u64;

    for file_path in &rs_files {
        let row = client
            .query_one("SELECT kerai.parse_file($1)::text", &[file_path])
            .map_err(|e| format!("parse_file failed for {file_path}: {e}"))?;

        let text: String = row.get(0);
        let value: serde_json::Value =
            serde_json::from_str(&text).unwrap_or(serde_json::Value::Null);

        let nodes = value["nodes_inserted"].as_u64().unwrap_or(0);
        let edges = value["edges_inserted"].as_u64().unwrap_or(0);
        total_nodes += nodes;
        total_edges += edges;

        // Show relative path for cleaner output
        let rel = file_path
            .strip_prefix(&project_root.to_string_lossy().as_ref())
            .unwrap_or(file_path)
            .trim_start_matches('/');
        println!("  {rel}: {nodes} nodes, {edges} edges");
    }

    println!(
        "Committed {} files: {total_nodes} nodes, {total_edges} edges",
        rs_files.len()
    );
    Ok(())
}

fn walk_rs_files(root: &Path, dir: &Path, out: &mut Vec<String>) -> Result<(), String> {
    let entries =
        std::fs::read_dir(dir).map_err(|e| format!("Cannot read {}: {e}", dir.display()))?;

    for entry in entries {
        let entry = entry.map_err(|e| format!("Read dir entry: {e}"))?;
        let path = entry.path();
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        if path.is_dir() {
            // Skip build/hidden directories
            if name_str == "target"
                || name_str == "tgt"
                || name_str == ".kerai"
                || name_str.starts_with('.')
            {
                continue;
            }
            walk_rs_files(root, &path, out)?;
        } else if name_str.ends_with(".rs") {
            out.push(path.to_string_lossy().to_string());
        }
    }
    Ok(())
}
