/// Parse Cargo.toml into nodes.
use serde_json::{json, Value};
use std::path::Path;
use uuid::Uuid;

use super::kinds::Kind;
use super::path_builder::PathContext;

/// A row to be inserted into kerai.nodes.
use super::ast_walker::NodeRow;

/// Parse a Cargo.toml file and return nodes for the crate and its dependencies.
pub fn parse_cargo_toml(
    cargo_path: &Path,
    instance_id: &str,
) -> Result<(Vec<NodeRow>, String, String), String> {
    let content =
        std::fs::read_to_string(cargo_path).map_err(|e| format!("Failed to read Cargo.toml: {}", e))?;

    let parsed: toml::Table =
        content.parse().map_err(|e| format!("Failed to parse Cargo.toml: {}", e))?;

    let package = parsed
        .get("package")
        .and_then(|p| p.as_table())
        .ok_or("Cargo.toml missing [package] section")?;

    let crate_name = package
        .get("name")
        .and_then(|n| n.as_str())
        .ok_or("Cargo.toml missing package.name")?
        .to_string();

    let version = package
        .get("version")
        .and_then(|v| v.as_str())
        .unwrap_or("0.0.0")
        .to_string();

    let edition = package
        .get("edition")
        .and_then(|e| e.as_str())
        .unwrap_or("2021");

    let mut nodes = Vec::new();
    let path_ctx = PathContext::with_root(&crate_name);

    // Crate root node
    let crate_id = Uuid::new_v4().to_string();
    let mut crate_meta = serde_json::Map::new();
    crate_meta.insert("version".into(), json!(version));
    crate_meta.insert("edition".into(), json!(edition));
    if let Some(desc) = package.get("description").and_then(|d| d.as_str()) {
        crate_meta.insert("description".into(), json!(desc));
    }

    nodes.push(NodeRow {
        id: crate_id.clone(),
        instance_id: instance_id.to_string(),
        kind: Kind::Crate.as_str().to_string(),
        language: Some("rust".to_string()),
        content: Some(crate_name.clone()),
        parent_id: None,
        position: 0,
        path: path_ctx.path(),
        metadata: Value::Object(crate_meta),
        span_start: None,
        span_end: None,
    });

    // Cargo.toml metadata node
    let cargo_node_id = Uuid::new_v4().to_string();
    nodes.push(NodeRow {
        id: cargo_node_id.clone(),
        instance_id: instance_id.to_string(),
        kind: Kind::CargoToml.as_str().to_string(),
        language: Some("toml".to_string()),
        content: Some("Cargo.toml".to_string()),
        parent_id: Some(crate_id.clone()),
        position: 0,
        path: Some(path_ctx.child_path("Cargo_toml")),
        metadata: json!({}),
        span_start: None,
        span_end: None,
    });

    // Dependencies
    let mut dep_position = 0;
    if let Some(deps) = parsed.get("dependencies").and_then(|d| d.as_table()) {
        for (dep_name, dep_value) in deps {
            let dep_id = Uuid::new_v4().to_string();
            let mut dep_meta = serde_json::Map::new();

            match dep_value {
                toml::Value::String(ver) => {
                    dep_meta.insert("version".into(), json!(ver));
                }
                toml::Value::Table(t) => {
                    if let Some(ver) = t.get("version").and_then(|v| v.as_str()) {
                        dep_meta.insert("version".into(), json!(ver));
                    }
                    if let Some(features) = t.get("features").and_then(|f| f.as_array()) {
                        let feat_list: Vec<&str> =
                            features.iter().filter_map(|f| f.as_str()).collect();
                        dep_meta.insert("features".into(), json!(feat_list));
                    }
                    if let Some(optional) = t.get("optional").and_then(|o| o.as_bool()) {
                        dep_meta.insert("optional".into(), json!(optional));
                    }
                    if let Some(path) = t.get("path").and_then(|p| p.as_str()) {
                        dep_meta.insert("path".into(), json!(path));
                    }
                    if let Some(git) = t.get("git").and_then(|g| g.as_str()) {
                        dep_meta.insert("git".into(), json!(git));
                    }
                }
                _ => {}
            }
            dep_meta.insert("dep_type".into(), json!("normal"));

            nodes.push(NodeRow {
                id: dep_id,
                instance_id: instance_id.to_string(),
                kind: Kind::Dependency.as_str().to_string(),
                language: None,
                content: Some(dep_name.clone()),
                parent_id: Some(cargo_node_id.clone()),
                position: dep_position,
                path: Some(path_ctx.child_path(&format!("Cargo_toml.{}", dep_name))),
                metadata: Value::Object(dep_meta),
                span_start: None,
                span_end: None,
            });
            dep_position += 1;
        }
    }

    // Dev dependencies
    if let Some(deps) = parsed.get("dev-dependencies").and_then(|d| d.as_table()) {
        for (dep_name, dep_value) in deps {
            let dep_id = Uuid::new_v4().to_string();
            let mut dep_meta = serde_json::Map::new();

            match dep_value {
                toml::Value::String(ver) => {
                    dep_meta.insert("version".into(), json!(ver));
                }
                toml::Value::Table(t) => {
                    if let Some(ver) = t.get("version").and_then(|v| v.as_str()) {
                        dep_meta.insert("version".into(), json!(ver));
                    }
                }
                _ => {}
            }
            dep_meta.insert("dep_type".into(), json!("dev"));

            nodes.push(NodeRow {
                id: dep_id,
                instance_id: instance_id.to_string(),
                kind: Kind::Dependency.as_str().to_string(),
                language: None,
                content: Some(dep_name.clone()),
                parent_id: Some(cargo_node_id.clone()),
                position: dep_position,
                path: Some(path_ctx.child_path(&format!("Cargo_toml.{}", dep_name))),
                metadata: Value::Object(dep_meta),
                span_start: None,
                span_end: None,
            });
            dep_position += 1;
        }
    }

    Ok((nodes, crate_id, crate_name))
}
