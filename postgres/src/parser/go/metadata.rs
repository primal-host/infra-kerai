/// Go-specific metadata extraction from tree-sitter nodes.
use serde_json::{json, Value};

use crate::parser::treesitter::cursor::node_text;

/// Whether a Go identifier is exported (starts with uppercase).
pub fn is_exported(name: &str) -> bool {
    name.starts_with(|c: char| c.is_uppercase())
}

/// Extract metadata for a function_declaration node.
pub fn func_metadata(node: &tree_sitter::Node, source: &str) -> Value {
    let mut meta = serde_json::Map::new();

    if let Some(name_node) = node.child_by_field_name("name") {
        let name = node_text(&name_node, source);
        meta.insert("exported".into(), json!(is_exported(name)));
    }

    if let Some(params) = node.child_by_field_name("parameters") {
        meta.insert("params".into(), json!(node_text(&params, source)));
    }

    if let Some(result) = node.child_by_field_name("result") {
        meta.insert("returns".into(), json!(node_text(&result, source)));
    }

    if let Some(type_params) = node.child_by_field_name("type_parameters") {
        meta.insert("type_parameters".into(), json!(node_text(&type_params, source)));
    }

    meta.insert("source".into(), json!(node_text(node, source)));
    Value::Object(meta)
}

/// Extract metadata for a method_declaration node.
pub fn method_metadata(node: &tree_sitter::Node, source: &str) -> Value {
    let mut meta = serde_json::Map::new();

    if let Some(name_node) = node.child_by_field_name("name") {
        let name = node_text(&name_node, source);
        meta.insert("exported".into(), json!(is_exported(name)));
    }

    if let Some(recv) = node.child_by_field_name("receiver") {
        let recv_text = node_text(&recv, source);
        meta.insert("receiver".into(), json!(recv_text));
        meta.insert("pointer_receiver".into(), json!(recv_text.contains('*')));
    }

    if let Some(params) = node.child_by_field_name("parameters") {
        meta.insert("params".into(), json!(node_text(&params, source)));
    }

    if let Some(result) = node.child_by_field_name("result") {
        meta.insert("returns".into(), json!(node_text(&result, source)));
    }

    meta.insert("source".into(), json!(node_text(node, source)));
    Value::Object(meta)
}

/// Extract metadata for a type_spec node.
pub fn type_spec_metadata(node: &tree_sitter::Node, source: &str) -> Value {
    let mut meta = serde_json::Map::new();

    if let Some(name_node) = node.child_by_field_name("name") {
        let name = node_text(&name_node, source);
        meta.insert("exported".into(), json!(is_exported(name)));
    }

    if let Some(type_params) = node.child_by_field_name("type_parameters") {
        meta.insert("type_parameters".into(), json!(node_text(&type_params, source)));
    }

    meta.insert("source".into(), json!(node_text(node, source)));
    Value::Object(meta)
}

/// Extract metadata for a field_declaration node.
pub fn field_metadata(node: &tree_sitter::Node, source: &str) -> Value {
    let mut meta = serde_json::Map::new();

    if let Some(name_node) = node.child_by_field_name("name") {
        let name = node_text(&name_node, source);
        meta.insert("exported".into(), json!(is_exported(name)));
    }

    if let Some(type_node) = node.child_by_field_name("type") {
        meta.insert("type".into(), json!(node_text(&type_node, source)));
    }

    if let Some(tag_node) = node.child_by_field_name("tag") {
        meta.insert("tag".into(), json!(node_text(&tag_node, source)));
    }

    meta.insert("source".into(), json!(node_text(node, source)));
    Value::Object(meta)
}

/// Extract metadata for an import_spec node.
pub fn import_spec_metadata(node: &tree_sitter::Node, source: &str) -> Value {
    let mut meta = serde_json::Map::new();

    if let Some(path_node) = node.child_by_field_name("path") {
        meta.insert("path".into(), json!(node_text(&path_node, source)));
    }

    if let Some(name_node) = node.child_by_field_name("name") {
        meta.insert("alias".into(), json!(node_text(&name_node, source)));
    }

    meta.insert("source".into(), json!(node_text(node, source)));
    Value::Object(meta)
}

/// Extract metadata for a var_spec or const_spec node.
pub fn var_const_spec_metadata(node: &tree_sitter::Node, source: &str) -> Value {
    let mut meta = serde_json::Map::new();

    if let Some(name_node) = node.child_by_field_name("name") {
        let name = node_text(&name_node, source);
        meta.insert("exported".into(), json!(is_exported(name)));
    }

    if let Some(type_node) = node.child_by_field_name("type") {
        meta.insert("type".into(), json!(node_text(&type_node, source)));
    }

    if let Some(value_node) = node.child_by_field_name("value") {
        meta.insert("value".into(), json!(node_text(&value_node, source)));
    }

    meta.insert("source".into(), json!(node_text(node, source)));
    Value::Object(meta)
}

/// Extract metadata for a package_clause node.
pub fn package_metadata(node: &tree_sitter::Node, source: &str) -> Value {
    let mut meta = serde_json::Map::new();

    // The package name is the second child (after "package" keyword)
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            if child.kind() == "package_identifier" {
                meta.insert("name".into(), json!(node_text(&child, source)));
                break;
            }
        }
    }

    meta.insert("source".into(), json!(node_text(node, source)));
    Value::Object(meta)
}

/// Extract metadata for a method_spec node (interface method).
pub fn method_spec_metadata(node: &tree_sitter::Node, source: &str) -> Value {
    let mut meta = serde_json::Map::new();

    if let Some(name_node) = node.child_by_field_name("name") {
        let name = node_text(&name_node, source);
        meta.insert("exported".into(), json!(is_exported(name)));
    }

    if let Some(params) = node.child_by_field_name("parameters") {
        meta.insert("params".into(), json!(node_text(&params, source)));
    }

    if let Some(result) = node.child_by_field_name("result") {
        meta.insert("returns".into(), json!(node_text(&result, source)));
    }

    meta.insert("source".into(), json!(node_text(node, source)));
    Value::Object(meta)
}

/// Generic metadata for any node: just stores source text.
pub fn generic_metadata(node: &tree_sitter::Node, source: &str, ts_kind: &str) -> Value {
    json!({
        "source": node_text(node, source),
        "ts_kind": ts_kind,
    })
}
