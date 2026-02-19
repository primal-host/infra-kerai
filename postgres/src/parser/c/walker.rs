/// C CST walker — converts tree-sitter C parse tree into NodeRow/EdgeRow vectors.
use serde_json::json;
use uuid::Uuid;

use crate::parser::ast_walker::{EdgeRow, NodeRow};
use crate::parser::path_builder::PathContext;
use crate::parser::treesitter::cursor::{node_text, span_end_line, span_start_line};

use super::kinds;
use super::metadata;

/// Walk context accumulator passed through the recursion.
struct CWalkCtx {
    source: String,
    instance_id: String,
    nodes: Vec<NodeRow>,
    edges: Vec<EdgeRow>,
    path_ctx: PathContext,
}

impl CWalkCtx {
    fn new_node(
        &mut self,
        kind: &str,
        content: Option<String>,
        parent_id: Option<&str>,
        position: i32,
        meta: serde_json::Value,
        span_start: Option<i32>,
        span_end: Option<i32>,
    ) -> String {
        let id = Uuid::new_v4().to_string();
        self.nodes.push(NodeRow {
            id: id.clone(),
            instance_id: self.instance_id.clone(),
            kind: kind.to_string(),
            language: Some("c".to_string()),
            content,
            parent_id: parent_id.map(|s| s.to_string()),
            position,
            path: self.path_ctx.path(),
            metadata: meta,
            span_start,
            span_end,
        });
        id
    }

    #[allow(dead_code)]
    fn new_edge(&mut self, source_id: &str, target_id: &str, relation: &str) {
        self.edges.push(EdgeRow {
            id: Uuid::new_v4().to_string(),
            source_id: source_id.to_string(),
            target_id: target_id.to_string(),
            relation: relation.to_string(),
            metadata: json!({}),
        });
    }
}

/// Recursively extract the identifier name from a C declarator chain.
///
/// C declarators can be deeply nested: `int *(*fp)(int)` produces
/// `pointer_declarator → parenthesized_declarator → pointer_declarator →
/// function_declarator → identifier`. This walks inward to find the name.
fn unwrap_declarator_name(node: &tree_sitter::Node, source: &str) -> Option<String> {
    match node.kind() {
        "identifier" | "field_identifier" | "type_identifier" => {
            Some(node_text(node, source).to_string())
        }
        "pointer_declarator" | "array_declarator" | "function_declarator"
        | "parenthesized_declarator" | "attributed_declarator" => node
            .child_by_field_name("declarator")
            .and_then(|d| unwrap_declarator_name(&d, source)),
        _ => {
            // Try walking named children as fallback
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                if let Some(name) = unwrap_declarator_name(&child, source) {
                    return Some(name);
                }
            }
            None
        }
    }
}

/// Check if a declaration has a specific storage class specifier.
fn has_storage_class(node: &tree_sitter::Node, source: &str, class: &str) -> bool {
    let mut cursor = node.walk();
    let children: Vec<_> = node.named_children(&mut cursor).collect();
    children
        .iter()
        .any(|c| c.kind() == "storage_class_specifier" && node_text(c, source) == class)
}

/// Walk a parsed C tree and produce NodeRow/EdgeRow vectors.
pub fn walk_c_file(
    tree: &tree_sitter::Tree,
    source: &str,
    file_node_id: &str,
    instance_id: &str,
    path_ctx: PathContext,
) -> (Vec<NodeRow>, Vec<EdgeRow>) {
    let mut ctx = CWalkCtx {
        source: source.to_string(),
        instance_id: instance_id.to_string(),
        nodes: Vec::new(),
        edges: Vec::new(),
        path_ctx,
    };

    let root = tree.root_node();
    walk_children(&mut ctx, &root, file_node_id);

    (ctx.nodes, ctx.edges)
}

/// Walk all named children of a node.
fn walk_children(ctx: &mut CWalkCtx, node: &tree_sitter::Node, parent_id: &str) {
    let mut cursor = node.walk();
    let children: Vec<_> = node.named_children(&mut cursor).collect();
    for (i, child) in children.iter().enumerate() {
        walk_node(ctx, child, parent_id, i as i32);
    }
}

/// Dispatch a single tree-sitter node to the appropriate walker.
fn walk_node(ctx: &mut CWalkCtx, node: &tree_sitter::Node, parent_id: &str, position: i32) {
    let source = ctx.source.clone();
    match node.kind() {
        // Preprocessor
        "preproc_include" => walk_include(ctx, node, parent_id, position),
        "preproc_def" => walk_define(ctx, node, parent_id, position),
        "preproc_function_def" => walk_macro_def(ctx, node, parent_id, position),
        "preproc_ifdef" => walk_ifdef(ctx, node, parent_id, position, kinds::C_IFDEF),
        "preproc_if" => walk_ifdef(ctx, node, parent_id, position, kinds::C_IF_DIRECTIVE),
        "preproc_call" => walk_leaf(ctx, node, parent_id, position, kinds::C_PRAGMA),
        "preproc_else" | "preproc_elif" => {
            // Children of ifdef/if blocks — walk their contents
            walk_children(ctx, node, parent_id);
        }
        // Declarations
        "function_definition" => walk_function(ctx, node, parent_id, position, &source),
        "declaration" => walk_declaration(ctx, node, parent_id, position, &source),
        "type_definition" => walk_typedef(ctx, node, parent_id, position, &source),
        "struct_specifier" => walk_struct(ctx, node, parent_id, position, &source),
        "union_specifier" => walk_union(ctx, node, parent_id, position, &source),
        "enum_specifier" => walk_enum(ctx, node, parent_id, position, &source),
        // Statements
        "compound_statement" => walk_block(ctx, node, parent_id, position),
        "if_statement" => walk_statement(ctx, node, parent_id, position, kinds::C_IF),
        "for_statement" => walk_statement(ctx, node, parent_id, position, kinds::C_FOR),
        "while_statement" => walk_statement(ctx, node, parent_id, position, kinds::C_WHILE),
        "do_statement" => walk_statement(ctx, node, parent_id, position, kinds::C_DO_WHILE),
        "switch_statement" => walk_statement(ctx, node, parent_id, position, kinds::C_SWITCH),
        "case_statement" => walk_case(ctx, node, parent_id, position),
        "return_statement" => walk_leaf(ctx, node, parent_id, position, kinds::C_RETURN),
        "break_statement" => walk_leaf(ctx, node, parent_id, position, kinds::C_BREAK),
        "continue_statement" => walk_leaf(ctx, node, parent_id, position, kinds::C_CONTINUE),
        "goto_statement" => walk_leaf(ctx, node, parent_id, position, kinds::C_GOTO),
        "labeled_statement" => walk_statement(ctx, node, parent_id, position, kinds::C_LABEL),
        "expression_statement" => walk_leaf(ctx, node, parent_id, position, kinds::C_EXPR_STMT),
        // Comments handled separately
        "comment" => {}
        // Skip unnamed or unimportant nodes
        _ if node.is_named() => walk_generic(ctx, node, parent_id, position),
        _ => {}
    }
}

fn walk_include(ctx: &mut CWalkCtx, node: &tree_sitter::Node, parent_id: &str, position: i32) {
    let source = ctx.source.clone();
    let meta = metadata::include_metadata(node, &source);
    let path = meta
        .get("path")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    ctx.new_node(
        kinds::C_INCLUDE,
        path,
        Some(parent_id),
        position,
        meta,
        Some(span_start_line(node)),
        Some(span_end_line(node)),
    );
}

fn walk_define(ctx: &mut CWalkCtx, node: &tree_sitter::Node, parent_id: &str, position: i32) {
    let source = ctx.source.clone();
    let meta = metadata::define_metadata(node, &source);
    let name = meta
        .get("name")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    ctx.new_node(
        kinds::C_DEFINE,
        name,
        Some(parent_id),
        position,
        meta,
        Some(span_start_line(node)),
        Some(span_end_line(node)),
    );
}

fn walk_macro_def(ctx: &mut CWalkCtx, node: &tree_sitter::Node, parent_id: &str, position: i32) {
    let source = ctx.source.clone();
    let meta = metadata::macro_metadata(node, &source);
    let name = meta
        .get("name")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    ctx.new_node(
        kinds::C_MACRO,
        name,
        Some(parent_id),
        position,
        meta,
        Some(span_start_line(node)),
        Some(span_end_line(node)),
    );
}

fn walk_ifdef(
    ctx: &mut CWalkCtx,
    node: &tree_sitter::Node,
    parent_id: &str,
    position: i32,
    kind: &str,
) {
    let source = ctx.source.clone();
    let ifdef_id = ctx.new_node(
        kind,
        None,
        Some(parent_id),
        position,
        json!({"source": node_text(node, &source)}),
        Some(span_start_line(node)),
        Some(span_end_line(node)),
    );

    walk_children(ctx, node, &ifdef_id);
}

fn walk_function(
    ctx: &mut CWalkCtx,
    node: &tree_sitter::Node,
    parent_id: &str,
    position: i32,
    source: &str,
) {
    let meta = metadata::function_metadata(node, source);
    let name = node
        .child_by_field_name("declarator")
        .and_then(|d| unwrap_declarator_name(&d, source));

    if let Some(ref n) = name {
        ctx.path_ctx.push(n);
    }

    let func_id = ctx.new_node(
        kinds::C_FUNCTION,
        name.clone(),
        Some(parent_id),
        position,
        meta,
        Some(span_start_line(node)),
        Some(span_end_line(node)),
    );

    // Walk body (compound_statement) if present
    if let Some(body) = node.child_by_field_name("body") {
        walk_block(ctx, &body, &func_id, 0);
    }

    if name.is_some() {
        ctx.path_ctx.pop();
    }
}

fn walk_declaration(
    ctx: &mut CWalkCtx,
    node: &tree_sitter::Node,
    parent_id: &str,
    position: i32,
    source: &str,
) {
    let meta = metadata::declaration_metadata(node, source);
    let name = meta
        .get("name")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    ctx.new_node(
        kinds::C_DECLARATION,
        name,
        Some(parent_id),
        position,
        meta,
        Some(span_start_line(node)),
        Some(span_end_line(node)),
    );
}

fn walk_typedef(
    ctx: &mut CWalkCtx,
    node: &tree_sitter::Node,
    parent_id: &str,
    position: i32,
    source: &str,
) {
    let meta = metadata::typedef_metadata(node, source);
    let name = meta
        .get("name")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    if let Some(ref n) = name {
        ctx.path_ctx.push(n);
    }

    let typedef_id = ctx.new_node(
        kinds::C_TYPEDEF,
        name.clone(),
        Some(parent_id),
        position,
        meta,
        Some(span_start_line(node)),
        Some(span_end_line(node)),
    );

    // Walk inner struct/union/enum if present
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        match child.kind() {
            "struct_specifier" => walk_struct(ctx, &child, &typedef_id, 0, source),
            "union_specifier" => walk_union(ctx, &child, &typedef_id, 0, source),
            "enum_specifier" => walk_enum(ctx, &child, &typedef_id, 0, source),
            _ => {}
        }
    }

    if name.is_some() {
        ctx.path_ctx.pop();
    }
}

fn walk_struct(
    ctx: &mut CWalkCtx,
    node: &tree_sitter::Node,
    parent_id: &str,
    position: i32,
    source: &str,
) {
    let meta = metadata::struct_metadata(node, source);
    let name = node
        .child_by_field_name("name")
        .map(|n| node_text(&n, source).to_string());

    let struct_id = ctx.new_node(
        kinds::C_STRUCT,
        name,
        Some(parent_id),
        position,
        meta,
        Some(span_start_line(node)),
        Some(span_end_line(node)),
    );

    // Walk field_declaration_list → field_declaration nodes
    if let Some(body) = node.child_by_field_name("body") {
        let mut field_idx = 0;
        let mut cursor = body.walk();
        for child in body.named_children(&mut cursor) {
            if child.kind() == "field_declaration" {
                add_field(ctx, &child, &struct_id, field_idx, source);
                field_idx += 1;
            }
        }
    }
}

fn walk_union(
    ctx: &mut CWalkCtx,
    node: &tree_sitter::Node,
    parent_id: &str,
    position: i32,
    source: &str,
) {
    let meta = metadata::union_metadata(node, source);
    let name = node
        .child_by_field_name("name")
        .map(|n| node_text(&n, source).to_string());

    let union_id = ctx.new_node(
        kinds::C_UNION,
        name,
        Some(parent_id),
        position,
        meta,
        Some(span_start_line(node)),
        Some(span_end_line(node)),
    );

    if let Some(body) = node.child_by_field_name("body") {
        let mut field_idx = 0;
        let mut cursor = body.walk();
        for child in body.named_children(&mut cursor) {
            if child.kind() == "field_declaration" {
                add_field(ctx, &child, &union_id, field_idx, source);
                field_idx += 1;
            }
        }
    }
}

fn walk_enum(
    ctx: &mut CWalkCtx,
    node: &tree_sitter::Node,
    parent_id: &str,
    position: i32,
    source: &str,
) {
    let meta = metadata::enum_metadata(node, source);
    let name = node
        .child_by_field_name("name")
        .map(|n| node_text(&n, source).to_string());

    let enum_id = ctx.new_node(
        kinds::C_ENUM,
        name,
        Some(parent_id),
        position,
        meta,
        Some(span_start_line(node)),
        Some(span_end_line(node)),
    );

    // Walk enumerator_list → enumerator nodes
    if let Some(body) = node.child_by_field_name("body") {
        let mut enum_idx = 0;
        let mut cursor = body.walk();
        for child in body.named_children(&mut cursor) {
            if child.kind() == "enumerator" {
                add_enumerator(ctx, &child, &enum_id, enum_idx, source);
                enum_idx += 1;
            }
        }
    }
}

fn add_field(
    ctx: &mut CWalkCtx,
    node: &tree_sitter::Node,
    parent_id: &str,
    position: i32,
    source: &str,
) {
    let meta = metadata::field_metadata(node, source);
    let name = meta
        .get("name")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    ctx.new_node(
        kinds::C_FIELD,
        name,
        Some(parent_id),
        position,
        meta,
        Some(span_start_line(node)),
        Some(span_end_line(node)),
    );
}

fn add_enumerator(
    ctx: &mut CWalkCtx,
    node: &tree_sitter::Node,
    parent_id: &str,
    position: i32,
    source: &str,
) {
    let meta = metadata::enumerator_metadata(node, source);
    let name = node
        .child_by_field_name("name")
        .map(|n| node_text(&n, source).to_string());

    ctx.new_node(
        kinds::C_ENUMERATOR,
        name,
        Some(parent_id),
        position,
        meta,
        Some(span_start_line(node)),
        Some(span_end_line(node)),
    );
}

fn walk_block(ctx: &mut CWalkCtx, node: &tree_sitter::Node, parent_id: &str, position: i32) {
    let source = ctx.source.clone();
    let block_id = ctx.new_node(
        kinds::C_BLOCK,
        None,
        Some(parent_id),
        position,
        json!({}),
        Some(span_start_line(node)),
        Some(span_end_line(node)),
    );

    let _ = source;
    walk_children(ctx, node, &block_id);
}

fn walk_statement(
    ctx: &mut CWalkCtx,
    node: &tree_sitter::Node,
    parent_id: &str,
    position: i32,
    kind: &str,
) {
    let source = ctx.source.clone();
    let stmt_id = ctx.new_node(
        kind,
        None,
        Some(parent_id),
        position,
        json!({"source": node_text(node, &source)}),
        Some(span_start_line(node)),
        Some(span_end_line(node)),
    );

    walk_children(ctx, node, &stmt_id);
}

fn walk_case(ctx: &mut CWalkCtx, node: &tree_sitter::Node, parent_id: &str, position: i32) {
    let source = ctx.source.clone();
    let case_id = ctx.new_node(
        kinds::C_CASE,
        None,
        Some(parent_id),
        position,
        json!({"source": node_text(node, &source)}),
        Some(span_start_line(node)),
        Some(span_end_line(node)),
    );

    walk_children(ctx, node, &case_id);
}

/// Walk a leaf node — no recursion into children.
fn walk_leaf(
    ctx: &mut CWalkCtx,
    node: &tree_sitter::Node,
    parent_id: &str,
    position: i32,
    kind: &str,
) {
    let source = ctx.source.clone();
    ctx.new_node(
        kind,
        Some(node_text(node, &source).to_string()),
        Some(parent_id),
        position,
        json!({"source": node_text(node, &source)}),
        Some(span_start_line(node)),
        Some(span_end_line(node)),
    );
}

/// Fallback: creates a c_other node, preserves ts_kind in metadata, recurses.
fn walk_generic(
    ctx: &mut CWalkCtx,
    node: &tree_sitter::Node,
    parent_id: &str,
    position: i32,
) {
    let source = ctx.source.clone();
    let ts_kind = node.kind();
    let kerai_kind = kinds::ts_kind_to_c_kind(ts_kind);

    // If we have a specific mapping, use it as a leaf
    if kerai_kind != kinds::C_OTHER {
        ctx.new_node(
            kerai_kind,
            Some(node_text(node, &source).to_string()),
            Some(parent_id),
            position,
            json!({"source": node_text(node, &source)}),
            Some(span_start_line(node)),
            Some(span_end_line(node)),
        );
        return;
    }

    let node_id = ctx.new_node(
        kinds::C_OTHER,
        Some(node_text(node, &source).to_string()),
        Some(parent_id),
        position,
        metadata::generic_metadata(node, &source, ts_kind),
        Some(span_start_line(node)),
        Some(span_end_line(node)),
    );

    // Recurse into named children
    walk_children(ctx, node, &node_id);
}

/// Collect byte ranges of string/char literals for comment exclusion.
pub fn collect_string_spans(tree: &tree_sitter::Tree, source: &str) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    collect_string_spans_recursive(&tree.root_node(), source, &mut spans);
    spans
}

fn collect_string_spans_recursive(
    node: &tree_sitter::Node,
    source: &str,
    spans: &mut Vec<(usize, usize)>,
) {
    match node.kind() {
        "string_literal" | "char_literal" | "concatenated_string" => {
            let start_line = node.start_position().row + 1;
            let end_line = node.end_position().row + 1;
            spans.push((start_line, end_line));
        }
        _ => {}
    }

    let _ = source;
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_string_spans_recursive(&child, source, spans);
    }
}

// Public wrappers for use by the metadata module.

/// Public wrapper for `unwrap_declarator_name`.
pub fn unwrap_declarator_name_pub(node: &tree_sitter::Node, source: &str) -> Option<String> {
    unwrap_declarator_name(node, source)
}

/// Public wrapper for `has_storage_class`.
pub fn has_storage_class_pub(node: &tree_sitter::Node, source: &str, class: &str) -> bool {
    has_storage_class(node, source, class)
}
