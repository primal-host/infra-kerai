/// Reconstruct module — kerai.nodes → Rust source text.
use pgrx::prelude::*;
use serde_json::json;

mod assembler;
mod formatter;

/// Reconstruct a Rust source file from its stored AST nodes.
/// Takes the UUID of a file-kind node and returns formatted Rust source.
#[pg_extern]
fn reconstruct_file(file_node_id: pgrx::Uuid) -> String {
    let id_str = file_node_id.to_string();

    // Validate that the node exists and is a file node
    let kind = Spi::get_one::<String>(&format!(
        "SELECT kind FROM kerai.nodes WHERE id = '{}'::uuid",
        id_str.replace('\'', "''")
    ))
    .expect("Failed to query node")
    .unwrap_or_else(|| pgrx::error!("Node not found: {}", id_str));

    if kind != "file" {
        pgrx::error!(
            "Node {} is kind '{}', expected 'file'",
            id_str,
            kind
        );
    }

    let raw = assembler::assemble_file(&id_str);
    formatter::format_source(&raw)
}

/// Reconstruct all files in a crate, returning a JSON map of {filename: source}.
#[pg_extern]
fn reconstruct_crate(crate_name: &str) -> pgrx::JsonB {
    // Find the crate node
    let crate_node_id = Spi::get_one::<String>(&format!(
        "SELECT id::text FROM kerai.nodes \
         WHERE kind = 'crate' AND content = '{}'",
        crate_name.replace('\'', "''")
    ))
    .expect("Failed to query crate node")
    .unwrap_or_else(|| pgrx::error!("Crate not found: {}", crate_name));

    // Find all file nodes under this crate
    let mut files = serde_json::Map::new();

    Spi::connect(|client| {
        let query = format!(
            "SELECT id::text, content FROM kerai.nodes \
             WHERE parent_id = '{}'::uuid AND kind = 'file' \
             ORDER BY position ASC",
            crate_node_id.replace('\'', "''")
        );

        let result = client.select(&query, None, None).unwrap();
        for row in result {
            let file_id: String = row.get_by_name("id").unwrap().unwrap_or_default();
            let filename: String = row.get_by_name("content").unwrap().unwrap_or_default();

            let raw = assembler::assemble_file(&file_id);
            let formatted = formatter::format_source(&raw);
            files.insert(filename, json!(formatted));
        }
    });

    pgrx::JsonB(serde_json::Value::Object(files))
}
