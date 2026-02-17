/// Reconstruct module — kerai.nodes → source text.
use pgrx::prelude::*;
use serde_json::json;

mod assembler;
mod derive_orderer;
mod formatter;
mod go;
mod c;
mod import_sorter;
mod markdown;

use assembler::{AssemblyOptions, query_file_flags};

/// Parse reconstruction options from a JSONB parameter.
fn parse_options(options: Option<pgrx::JsonB>) -> AssemblyOptions {
    let mut opts = AssemblyOptions::default();
    if let Some(pgrx::JsonB(ref val)) = options {
        if let Some(v) = val.get("sort_imports").and_then(|v| v.as_bool()) {
            opts.sort_imports = v;
        }
        if let Some(v) = val.get("order_derives").and_then(|v| v.as_bool()) {
            opts.order_derives = v;
        }
        if let Some(v) = val.get("suggestions").and_then(|v| v.as_bool()) {
            opts.suggestions = v;
        }
    }
    opts
}

/// Reconstruct a Rust source file from its stored AST nodes.
/// Takes the UUID of a file-kind node and returns formatted Rust source.
#[pg_extern]
fn reconstruct_file(file_node_id: pgrx::Uuid) -> String {
    reconstruct_file_with_options(file_node_id, None)
}

/// Reconstruct a Rust source file with explicit options.
///
/// Options JSON keys (all boolean, default true):
/// - sort_imports: canonical import ordering (std → external → crate)
/// - order_derives: alphabetical #[derive(...)] normalization
/// - suggestions: emit // kerai: advisory comments
#[pg_extern]
fn reconstruct_file_with_options(
    file_node_id: pgrx::Uuid,
    options: Option<pgrx::JsonB>,
) -> String {
    let id_str = file_node_id.to_string();
    let opts = parse_options(options);

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

    let flags = query_file_flags(&id_str);
    let raw = assembler::assemble_file_with_options(&id_str, &opts);
    let formatted = formatter::format_source(&raw);

    // Apply derive ordering after formatting (quote::ToTokens uses spaced syntax
    // that doesn't match #[derive(...)], so we must order after prettyplease normalizes)
    let order = opts.order_derives && !flags.skip_order_derives && !flags.skip_all;
    if order {
        derive_orderer::order_derives(&formatted)
    } else {
        formatted
    }
}

/// Reconstruct all files in a crate, returning a JSON map of {filename: source}.
#[pg_extern]
fn reconstruct_crate(crate_name: &str) -> pgrx::JsonB {
    reconstruct_crate_with_options(crate_name, None)
}

/// Reconstruct all files in a crate with explicit options.
#[pg_extern]
fn reconstruct_crate_with_options(
    crate_name: &str,
    options: Option<pgrx::JsonB>,
) -> pgrx::JsonB {
    let opts = parse_options(options);

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

        let result = client.select(&query, None, &[]).unwrap();
        for row in result {
            let file_id: String = row.get_by_name::<String, _>("id").unwrap().unwrap_or_default();
            let filename: String = row.get_by_name::<String, _>("content").unwrap().unwrap_or_default();

            let file_flags = query_file_flags(&file_id);
            let raw = assembler::assemble_file_with_options(&file_id, &opts);
            let formatted = formatter::format_source(&raw);
            let order = opts.order_derives && !file_flags.skip_order_derives && !file_flags.skip_all;
            let final_source = if order {
                derive_orderer::order_derives(&formatted)
            } else {
                formatted
            };
            files.insert(filename, json!(final_source));
        }
    });

    pgrx::JsonB(serde_json::Value::Object(files))
}
