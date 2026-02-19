/// C-specific metadata extraction from tree-sitter nodes.
use serde_json::{json, Value};

use crate::parser::treesitter::cursor::node_text;

use super::walker;

/// Extract metadata for a `preproc_include` node.
pub fn include_metadata(node: &tree_sitter::Node, source: &str) -> Value {
    let mut meta = serde_json::Map::new();

    // The path child is either `system_lib_string` (<stdio.h>) or `string_literal` ("foo.h")
    if let Some(path_node) = node.child_by_field_name("path") {
        let path_text = node_text(&path_node, source);
        meta.insert("path".into(), json!(path_text));
        meta.insert("system".into(), json!(path_node.kind() == "system_lib_string"));
    }

    meta.insert("source".into(), json!(node_text(node, source)));
    Value::Object(meta)
}

/// Extract metadata for a `preproc_def` node (#define NAME value).
pub fn define_metadata(node: &tree_sitter::Node, source: &str) -> Value {
    let mut meta = serde_json::Map::new();

    if let Some(name_node) = node.child_by_field_name("name") {
        meta.insert("name".into(), json!(node_text(&name_node, source)));
    }

    if let Some(value_node) = node.child_by_field_name("value") {
        meta.insert("value".into(), json!(node_text(&value_node, source).trim()));
    }

    meta.insert("source".into(), json!(node_text(node, source)));
    Value::Object(meta)
}

/// Extract metadata for a `preproc_function_def` node (#define NAME(args) body).
pub fn macro_metadata(node: &tree_sitter::Node, source: &str) -> Value {
    let mut meta = serde_json::Map::new();

    if let Some(name_node) = node.child_by_field_name("name") {
        meta.insert("name".into(), json!(node_text(&name_node, source)));
    }

    if let Some(params_node) = node.child_by_field_name("parameters") {
        meta.insert("parameters".into(), json!(node_text(&params_node, source)));
    }

    if let Some(value_node) = node.child_by_field_name("value") {
        meta.insert("value".into(), json!(node_text(&value_node, source).trim()));
    }

    meta.insert("source".into(), json!(node_text(node, source)));
    Value::Object(meta)
}

/// Extract metadata for a `function_definition` node.
pub fn function_metadata(node: &tree_sitter::Node, source: &str) -> Value {
    let mut meta = serde_json::Map::new();

    // Return type from the `type` field
    if let Some(type_node) = node.child_by_field_name("type") {
        meta.insert("return_type".into(), json!(node_text(&type_node, source)));
    }

    // Extract name from declarator chain
    if let Some(decl) = node.child_by_field_name("declarator") {
        if let Some(name) = walker::unwrap_declarator_name_pub(&decl, source) {
            meta.insert("name".into(), json!(name));
        }

        // Extract parameter list from function_declarator
        if let Some(params) = extract_func_params(&decl, source) {
            meta.insert("params".into(), json!(params));
        }
    }

    // Check for static storage class
    meta.insert("static".into(), json!(walker::has_storage_class_pub(node, source, "static")));

    meta.insert("source".into(), json!(node_text(node, source)));
    Value::Object(meta)
}

/// Extract metadata for a `declaration` node (variable/function declaration).
pub fn declaration_metadata(node: &tree_sitter::Node, source: &str) -> Value {
    let mut meta = serde_json::Map::new();

    if let Some(type_node) = node.child_by_field_name("type") {
        meta.insert("type".into(), json!(node_text(&type_node, source)));
    }

    // Try to get name from declarator
    if let Some(decl) = node.child_by_field_name("declarator") {
        if let Some(name) = walker::unwrap_declarator_name_pub(&decl, source) {
            meta.insert("name".into(), json!(name));
        }
    }

    // Check storage class
    if walker::has_storage_class_pub(node, source, "static") {
        meta.insert("storage_class".into(), json!("static"));
    } else if walker::has_storage_class_pub(node, source, "extern") {
        meta.insert("storage_class".into(), json!("extern"));
    }

    meta.insert("source".into(), json!(node_text(node, source)));
    Value::Object(meta)
}

/// Extract metadata for a `type_definition` node.
pub fn typedef_metadata(node: &tree_sitter::Node, source: &str) -> Value {
    let mut meta = serde_json::Map::new();

    // The typedef name is in the `declarator` field
    if let Some(decl) = node.child_by_field_name("declarator") {
        if let Some(name) = walker::unwrap_declarator_name_pub(&decl, source) {
            meta.insert("name".into(), json!(name));
        }
    }

    // The underlying type
    if let Some(type_node) = node.child_by_field_name("type") {
        meta.insert("type".into(), json!(node_text(&type_node, source)));
    }

    meta.insert("source".into(), json!(node_text(node, source)));
    Value::Object(meta)
}

/// Extract metadata for a `struct_specifier` node.
pub fn struct_metadata(node: &tree_sitter::Node, source: &str) -> Value {
    let mut meta = serde_json::Map::new();

    if let Some(name_node) = node.child_by_field_name("name") {
        meta.insert("name".into(), json!(node_text(&name_node, source)));
    }

    meta.insert("source".into(), json!(node_text(node, source)));
    Value::Object(meta)
}

/// Extract metadata for a `union_specifier` node.
pub fn union_metadata(node: &tree_sitter::Node, source: &str) -> Value {
    let mut meta = serde_json::Map::new();

    if let Some(name_node) = node.child_by_field_name("name") {
        meta.insert("name".into(), json!(node_text(&name_node, source)));
    }

    meta.insert("source".into(), json!(node_text(node, source)));
    Value::Object(meta)
}

/// Extract metadata for an `enum_specifier` node.
pub fn enum_metadata(node: &tree_sitter::Node, source: &str) -> Value {
    let mut meta = serde_json::Map::new();

    if let Some(name_node) = node.child_by_field_name("name") {
        meta.insert("name".into(), json!(node_text(&name_node, source)));
    }

    meta.insert("source".into(), json!(node_text(node, source)));
    Value::Object(meta)
}

/// Extract metadata for a `field_declaration` node.
pub fn field_metadata(node: &tree_sitter::Node, source: &str) -> Value {
    let mut meta = serde_json::Map::new();

    if let Some(type_node) = node.child_by_field_name("type") {
        meta.insert("type".into(), json!(node_text(&type_node, source)));
    }

    if let Some(decl) = node.child_by_field_name("declarator") {
        if let Some(name) = walker::unwrap_declarator_name_pub(&decl, source) {
            meta.insert("name".into(), json!(name));
        }
    }

    // Check for bitfield
    if let Some(bitfield) = node.child_by_field_name("size") {
        meta.insert("bitfield".into(), json!(node_text(&bitfield, source)));
    }

    meta.insert("source".into(), json!(node_text(node, source)));
    Value::Object(meta)
}

/// Extract metadata for an `enumerator` node.
pub fn enumerator_metadata(node: &tree_sitter::Node, source: &str) -> Value {
    let mut meta = serde_json::Map::new();

    if let Some(name_node) = node.child_by_field_name("name") {
        meta.insert("name".into(), json!(node_text(&name_node, source)));
    }

    if let Some(value_node) = node.child_by_field_name("value") {
        meta.insert("value".into(), json!(node_text(&value_node, source)));
    }

    meta.insert("source".into(), json!(node_text(node, source)));
    Value::Object(meta)
}

/// Generic metadata for any node: stores source text and tree-sitter kind.
pub fn generic_metadata(node: &tree_sitter::Node, source: &str, ts_kind: &str) -> Value {
    json!({
        "source": node_text(node, source),
        "ts_kind": ts_kind,
    })
}

/// Extract parameter text from a function declarator chain.
fn extract_func_params(node: &tree_sitter::Node, source: &str) -> Option<String> {
    match node.kind() {
        "function_declarator" => node
            .child_by_field_name("parameters")
            .map(|p| node_text(&p, source).to_string()),
        "pointer_declarator" | "parenthesized_declarator" | "attributed_declarator" => node
            .child_by_field_name("declarator")
            .and_then(|d| extract_func_params(&d, source)),
        _ => None,
    }
}
