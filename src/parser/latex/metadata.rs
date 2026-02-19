/// LaTeX-specific metadata extraction from tree-sitter nodes.
use serde_json::{json, Value};

use crate::parser::treesitter::cursor::node_text;

/// Extract metadata for a sectioning command (\section, \chapter, etc.).
///
/// Metadata: title, starred, label (if a \label immediately follows).
pub fn section_metadata(node: &tree_sitter::Node, source: &str, cmd_name: &str) -> Value {
    let mut meta = serde_json::Map::new();
    meta.insert("command".into(), json!(cmd_name));
    meta.insert("starred".into(), json!(cmd_name.ends_with('*')));

    // Extract the title from the first curly_group child
    if let Some(title) = find_curly_group_text(node, source, 0) {
        meta.insert("title".into(), json!(title));
    }

    // Check for optional short title in square brackets
    if let Some(short) = find_brack_group_text(node, source) {
        meta.insert("short_title".into(), json!(short));
    }

    meta.insert("source".into(), json!(node_text(node, source)));
    Value::Object(meta)
}

/// Extract metadata for an environment node.
///
/// Metadata: env_name, optional args.
pub fn environment_metadata(env_name: &str, node: &tree_sitter::Node, source: &str) -> Value {
    let mut meta = serde_json::Map::new();
    meta.insert("env_name".into(), json!(env_name));

    // Check for optional arguments (e.g., \begin{theorem}[Name])
    if let Some(opt_arg) = find_brack_group_text(node, source) {
        meta.insert("args".into(), json!(opt_arg));
    }

    meta.insert("source".into(), json!(node_text(node, source)));
    Value::Object(meta)
}

/// Extract metadata for a \cite-family command.
///
/// Metadata: keys (array), command variant, prenote, postnote.
pub fn citation_metadata(node: &tree_sitter::Node, source: &str, cmd_name: &str) -> Value {
    let mut meta = serde_json::Map::new();
    meta.insert("command".into(), json!(cmd_name));

    // The citation keys are in the curly group: \cite{key1,key2}
    if let Some(keys_text) = find_curly_group_text(node, source, 0) {
        let keys: Vec<&str> = keys_text.split(',').map(str::trim).collect();
        meta.insert("keys".into(), json!(keys));
    }

    // Some citation commands have prenote/postnote in brackets
    if let Some(note) = find_brack_group_text(node, source) {
        meta.insert("note".into(), json!(note));
    }

    meta.insert("source".into(), json!(node_text(node, source)));
    Value::Object(meta)
}

/// Extract metadata for a \label command.
pub fn label_metadata(node: &tree_sitter::Node, source: &str) -> Value {
    let mut meta = serde_json::Map::new();

    if let Some(key) = find_curly_group_text(node, source, 0) {
        meta.insert("key".into(), json!(key));
    }

    meta.insert("source".into(), json!(node_text(node, source)));
    Value::Object(meta)
}

/// Extract metadata for a \ref or \eqref command.
pub fn ref_metadata(node: &tree_sitter::Node, source: &str, cmd_name: &str) -> Value {
    let mut meta = serde_json::Map::new();
    meta.insert("command".into(), json!(cmd_name));

    if let Some(key) = find_curly_group_text(node, source, 0) {
        meta.insert("key".into(), json!(key));
    }

    meta.insert("source".into(), json!(node_text(node, source)));
    Value::Object(meta)
}

/// Extract metadata for a \caption command.
pub fn caption_metadata(node: &tree_sitter::Node, source: &str) -> Value {
    let mut meta = serde_json::Map::new();

    if let Some(text) = find_curly_group_text(node, source, 0) {
        meta.insert("text".into(), json!(text));
    }

    if let Some(short) = find_brack_group_text(node, source) {
        meta.insert("short_caption".into(), json!(short));
    }

    meta.insert("source".into(), json!(node_text(node, source)));
    Value::Object(meta)
}

/// Extract metadata for a \documentclass command.
pub fn documentclass_metadata(node: &tree_sitter::Node, source: &str) -> Value {
    let mut meta = serde_json::Map::new();

    if let Some(class) = find_curly_group_text(node, source, 0) {
        meta.insert("class".into(), json!(class));
    }

    if let Some(options) = find_brack_group_text(node, source) {
        let opts: Vec<&str> = options.split(',').map(str::trim).collect();
        meta.insert("options".into(), json!(opts));
    }

    meta.insert("source".into(), json!(node_text(node, source)));
    Value::Object(meta)
}

/// Extract metadata for a \usepackage command.
pub fn usepackage_metadata(node: &tree_sitter::Node, source: &str) -> Value {
    let mut meta = serde_json::Map::new();

    if let Some(pkg) = find_curly_group_text(node, source, 0) {
        meta.insert("package".into(), json!(pkg));
    }

    if let Some(options) = find_brack_group_text(node, source) {
        let opts: Vec<&str> = options.split(',').map(str::trim).collect();
        meta.insert("options".into(), json!(opts));
    }

    meta.insert("source".into(), json!(node_text(node, source)));
    Value::Object(meta)
}

/// Extract metadata for a generic command.
pub fn command_metadata(node: &tree_sitter::Node, source: &str, cmd_name: &str) -> Value {
    let mut meta = serde_json::Map::new();
    meta.insert("command".into(), json!(cmd_name));

    if let Some(arg) = find_curly_group_text(node, source, 0) {
        meta.insert("arg".into(), json!(arg));
    }

    meta.insert("source".into(), json!(node_text(node, source)));
    Value::Object(meta)
}

/// Extract metadata for a \input or \include command.
pub fn input_metadata(node: &tree_sitter::Node, source: &str, cmd_name: &str) -> Value {
    let mut meta = serde_json::Map::new();
    meta.insert("command".into(), json!(cmd_name));

    if let Some(path) = find_curly_group_text(node, source, 0) {
        meta.insert("path".into(), json!(path));
    }

    meta.insert("source".into(), json!(node_text(node, source)));
    Value::Object(meta)
}

/// Find the text content of the Nth curly_group child, stripping the braces.
fn find_curly_group_text<'a>(
    node: &tree_sitter::Node,
    source: &'a str,
    nth: usize,
) -> Option<&'a str> {
    let mut count = 0;
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        let k = child.kind();
        if k == "curly_group" || k == "curly_group_text" || k == "curly_group_text_list"
            || k == "curly_group_command"
        {
            if count == nth {
                let text = node_text(&child, source);
                // Strip surrounding braces
                return Some(text.strip_prefix('{')
                    .unwrap_or(text)
                    .strip_suffix('}')
                    .unwrap_or(text));
            }
            count += 1;
        }
    }
    None
}

/// Find the text content of the first bracket (optional) group child.
fn find_brack_group_text<'a>(node: &tree_sitter::Node, source: &'a str) -> Option<&'a str> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        let k = child.kind();
        if k == "brack_group" || k == "brack_group_text" {
            let text = node_text(&child, source);
            return Some(text.strip_prefix('[')
                .unwrap_or(text)
                .strip_suffix(']')
                .unwrap_or(text));
        }
    }
    None
}
