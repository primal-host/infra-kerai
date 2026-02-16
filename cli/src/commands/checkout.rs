use postgres::Client;

use crate::config;

pub fn run(client: &mut Client, file: Option<&str>) -> Result<(), String> {
    match file {
        Some(filename) => checkout_file(client, filename),
        None => checkout_crate(client),
    }
}

fn checkout_file(client: &mut Client, filename: &str) -> Result<(), String> {
    // Find the file node by name
    let row = client
        .query_opt(
            "SELECT id FROM kerai.nodes WHERE kind = 'file' AND metadata->>'filename' = $1",
            &[&filename],
        )
        .map_err(|e| format!("Query failed: {e}"))?
        .ok_or_else(|| format!("File node not found: {filename}"))?;

    let file_id: uuid::Uuid = row.get(0);

    let row = client
        .query_one(
            "SELECT kerai.reconstruct_file($1)",
            &[&file_id],
        )
        .map_err(|e| format!("reconstruct_file failed: {e}"))?;

    let content: String = row.get(0);

    // Write to the filename in the current directory
    let out_path = std::path::Path::new(filename);
    if let Some(parent) = out_path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create directories: {e}"))?;
        }
    }
    std::fs::write(out_path, &content)
        .map_err(|e| format!("Failed to write {filename}: {e}"))?;

    println!("Wrote {filename} ({} bytes)", content.len());
    Ok(())
}

fn checkout_crate(client: &mut Client) -> Result<(), String> {
    let project_root = config::find_project_root()
        .ok_or("No .kerai/config.toml found. Run 'kerai init' first.")?;

    let config_path = project_root.join(".kerai").join("config.toml");
    let content = std::fs::read_to_string(&config_path)
        .map_err(|e| format!("Failed to read config: {e}"))?;
    let cfg: config::ConfigFile = toml::from_str(&content)
        .map_err(|e| format!("Invalid config: {e}"))?;

    let crate_name = cfg
        .default
        .as_ref()
        .and_then(|d| d.crate_name.as_deref())
        .ok_or("No crate_name in project config")?;

    let row = client
        .query_one(
            "SELECT kerai.reconstruct_crate($1)::text",
            &[&crate_name],
        )
        .map_err(|e| format!("reconstruct_crate failed: {e}"))?;

    let text: String = row.get(0);
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("Invalid JSON: {e}"))?;

    let files = value["files"]
        .as_array()
        .ok_or("Expected 'files' array in response")?;

    let mut total_bytes = 0usize;
    for file_obj in files {
        let filename = file_obj["filename"]
            .as_str()
            .ok_or("Missing filename in response")?;
        let file_content = file_obj["content"]
            .as_str()
            .ok_or("Missing content in response")?;

        let out_path = project_root.join(filename);
        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create dirs for {filename}: {e}"))?;
        }
        std::fs::write(&out_path, file_content)
            .map_err(|e| format!("Failed to write {filename}: {e}"))?;

        total_bytes += file_content.len();
        println!("  {filename} ({} bytes)", file_content.len());
    }

    println!(
        "Checked out {} files ({total_bytes} bytes total)",
        files.len()
    );
    Ok(())
}
