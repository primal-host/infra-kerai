/// Go CST walker — converts tree-sitter Go parse tree into NodeRow/EdgeRow vectors.
use serde_json::json;
use uuid::Uuid;

use crate::parser::ast_walker::{EdgeRow, NodeRow};
use crate::parser::path_builder::PathContext;
use crate::parser::treesitter::cursor::{node_text, span_end_line, span_start_line};

use super::kinds;
use super::metadata;

/// Walk context accumulator passed through the recursion.
struct GoWalkCtx {
    source: String,
    instance_id: String,
    nodes: Vec<NodeRow>,
    edges: Vec<EdgeRow>,
    path_ctx: PathContext,
}

impl GoWalkCtx {
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
            language: Some("go".to_string()),
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

/// Walk a parsed Go tree and produce NodeRow/EdgeRow vectors.
pub fn walk_go_file(
    tree: &tree_sitter::Tree,
    source: &str,
    file_node_id: &str,
    instance_id: &str,
    path_ctx: PathContext,
) -> (Vec<NodeRow>, Vec<EdgeRow>) {
    let mut ctx = GoWalkCtx {
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
fn walk_children(ctx: &mut GoWalkCtx, node: &tree_sitter::Node, parent_id: &str) {
    let mut cursor = node.walk();
    let children: Vec<_> = node.named_children(&mut cursor).collect();
    for (i, child) in children.iter().enumerate() {
        walk_node(ctx, child, parent_id, i as i32);
    }
}

/// Dispatch a single tree-sitter node to the appropriate walker.
fn walk_node(ctx: &mut GoWalkCtx, node: &tree_sitter::Node, parent_id: &str, position: i32) {
    let source = ctx.source.clone();
    match node.kind() {
        "package_clause" => walk_package(ctx, node, parent_id, position),
        "import_declaration" => walk_import(ctx, node, parent_id, position),
        "function_declaration" => walk_func(ctx, node, parent_id, position, &source),
        "method_declaration" => walk_method(ctx, node, parent_id, position, &source),
        "type_declaration" => walk_type_decl(ctx, node, parent_id, position, &source),
        "var_declaration" => walk_var_decl(ctx, node, parent_id, position),
        "const_declaration" => walk_const_decl(ctx, node, parent_id, position),
        "block" => walk_block(ctx, node, parent_id, position),
        "if_statement" => walk_statement(ctx, node, parent_id, position, kinds::GO_IF),
        "for_statement" => walk_statement(ctx, node, parent_id, position, kinds::GO_FOR),
        "expression_switch_statement" => {
            walk_statement(ctx, node, parent_id, position, kinds::GO_SWITCH)
        }
        "type_switch_statement" => {
            walk_statement(ctx, node, parent_id, position, kinds::GO_TYPE_SWITCH)
        }
        "select_statement" => walk_statement(ctx, node, parent_id, position, kinds::GO_SELECT),
        "return_statement" => walk_leaf(ctx, node, parent_id, position, kinds::GO_RETURN),
        "go_statement" => walk_statement(ctx, node, parent_id, position, kinds::GO_GO),
        "defer_statement" => walk_statement(ctx, node, parent_id, position, kinds::GO_DEFER),
        "short_var_declaration" => walk_leaf(ctx, node, parent_id, position, kinds::GO_SHORT_VAR),
        "assignment_statement" => walk_leaf(ctx, node, parent_id, position, kinds::GO_ASSIGNMENT),
        "expression_statement" => {
            walk_statement(ctx, node, parent_id, position, kinds::GO_EXPRESSION_STMT)
        }
        "send_statement" => walk_leaf(ctx, node, parent_id, position, kinds::GO_SEND_STMT),
        "inc_statement" => walk_leaf(ctx, node, parent_id, position, kinds::GO_INC_STMT),
        "dec_statement" => walk_leaf(ctx, node, parent_id, position, kinds::GO_DEC_STMT),
        "labeled_statement" => {
            walk_statement(ctx, node, parent_id, position, kinds::GO_LABELED_STMT)
        }
        "fallthrough_statement" => {
            walk_leaf(ctx, node, parent_id, position, kinds::GO_FALLTHROUGH)
        }
        "break_statement" => walk_leaf(ctx, node, parent_id, position, kinds::GO_BREAK),
        "continue_statement" => walk_leaf(ctx, node, parent_id, position, kinds::GO_CONTINUE),
        "goto_statement" => walk_leaf(ctx, node, parent_id, position, kinds::GO_GOTO),
        "call_expression" => walk_leaf(ctx, node, parent_id, position, kinds::GO_CALL),
        "selector_expression" => walk_leaf(ctx, node, parent_id, position, kinds::GO_SELECTOR),
        "composite_literal" => walk_leaf(ctx, node, parent_id, position, kinds::GO_COMPOSITE_LIT),
        "func_literal" => walk_func_literal(ctx, node, parent_id, position, &source),
        "expression_case" | "type_case" => {
            walk_case_clause(ctx, node, parent_id, position, kinds::GO_CASE)
        }
        "default_case" => {
            walk_case_clause(ctx, node, parent_id, position, kinds::GO_DEFAULT_CASE)
        }
        "communication_case" => {
            walk_case_clause(ctx, node, parent_id, position, kinds::GO_COMM_CLAUSE)
        }
        "comment" => { /* comments handled separately */ }
        _ if node.is_named() => walk_generic(ctx, node, parent_id, position),
        _ => {}
    }
}

fn walk_package(ctx: &mut GoWalkCtx, node: &tree_sitter::Node, parent_id: &str, position: i32) {
    let source = ctx.source.clone();
    let meta = metadata::package_metadata(node, &source);
    let name = meta
        .get("name")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    ctx.new_node(
        kinds::GO_PACKAGE,
        name,
        Some(parent_id),
        position,
        meta,
        Some(span_start_line(node)),
        Some(span_end_line(node)),
    );
}

fn walk_import(ctx: &mut GoWalkCtx, node: &tree_sitter::Node, parent_id: &str, position: i32) {
    let source = ctx.source.clone();
    let import_id = ctx.new_node(
        kinds::GO_IMPORT,
        None,
        Some(parent_id),
        position,
        json!({"source": node_text(node, &source)}),
        Some(span_start_line(node)),
        Some(span_end_line(node)),
    );

    // import_declaration can contain import_spec directly (single import)
    // or import_spec_list (grouped imports) which contains the import_spec nodes
    let mut spec_idx = 0;
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if child.kind() == "import_spec" {
            add_import_spec(ctx, &child, &import_id, spec_idx);
            spec_idx += 1;
        } else if child.kind() == "import_spec_list" {
            let mut inner_cursor = child.walk();
            for spec in child.named_children(&mut inner_cursor) {
                if spec.kind() == "import_spec" {
                    add_import_spec(ctx, &spec, &import_id, spec_idx);
                    spec_idx += 1;
                }
            }
        }
    }
}

fn add_import_spec(
    ctx: &mut GoWalkCtx,
    node: &tree_sitter::Node,
    parent_id: &str,
    position: i32,
) {
    let src = ctx.source.clone();
    let meta = metadata::import_spec_metadata(node, &src);
    let path = meta
        .get("path")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    ctx.new_node(
        kinds::GO_IMPORT_SPEC,
        path,
        Some(parent_id),
        position,
        meta,
        Some(span_start_line(node)),
        Some(span_end_line(node)),
    );
}

fn walk_func(
    ctx: &mut GoWalkCtx,
    node: &tree_sitter::Node,
    parent_id: &str,
    position: i32,
    source: &str,
) {
    let meta = metadata::func_metadata(node, source);
    let name = node
        .child_by_field_name("name")
        .map(|n| node_text(&n, source).to_string());

    if let Some(ref n) = name {
        ctx.path_ctx.push(n);
    }

    let func_id = ctx.new_node(
        kinds::GO_FUNC,
        name.clone(),
        Some(parent_id),
        position,
        meta,
        Some(span_start_line(node)),
        Some(span_end_line(node)),
    );

    // Walk body block if present
    if let Some(body) = node.child_by_field_name("body") {
        walk_block(ctx, &body, &func_id, 0);
    }

    if name.is_some() {
        ctx.path_ctx.pop();
    }
}

fn walk_method(
    ctx: &mut GoWalkCtx,
    node: &tree_sitter::Node,
    parent_id: &str,
    position: i32,
    source: &str,
) {
    let meta = metadata::method_metadata(node, source);
    let name = node
        .child_by_field_name("name")
        .map(|n| node_text(&n, source).to_string());

    if let Some(ref n) = name {
        ctx.path_ctx.push(n);
    }

    let method_id = ctx.new_node(
        kinds::GO_METHOD,
        name.clone(),
        Some(parent_id),
        position,
        meta,
        Some(span_start_line(node)),
        Some(span_end_line(node)),
    );

    if let Some(body) = node.child_by_field_name("body") {
        walk_block(ctx, &body, &method_id, 0);
    }

    if name.is_some() {
        ctx.path_ctx.pop();
    }
}

fn walk_type_decl(
    ctx: &mut GoWalkCtx,
    node: &tree_sitter::Node,
    parent_id: &str,
    position: i32,
    source: &str,
) {
    let type_decl_id = ctx.new_node(
        kinds::GO_TYPE_DECL,
        None,
        Some(parent_id),
        position,
        json!({"source": node_text(node, source)}),
        Some(span_start_line(node)),
        Some(span_end_line(node)),
    );

    // type_declaration can contain type_spec directly (single) or via type_spec_list (grouped)
    let mut spec_idx = 0;
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if child.kind() == "type_spec" {
            walk_type_spec(ctx, &child, &type_decl_id, spec_idx, source);
            spec_idx += 1;
        } else if child.kind() == "type_spec_list" {
            let mut inner_cursor = child.walk();
            for spec in child.named_children(&mut inner_cursor) {
                if spec.kind() == "type_spec" {
                    walk_type_spec(ctx, &spec, &type_decl_id, spec_idx, source);
                    spec_idx += 1;
                }
            }
        }
    }
}

fn walk_type_spec(
    ctx: &mut GoWalkCtx,
    node: &tree_sitter::Node,
    parent_id: &str,
    position: i32,
    source: &str,
) {
    let meta = metadata::type_spec_metadata(node, source);
    let name = node
        .child_by_field_name("name")
        .map(|n| node_text(&n, source).to_string());

    if let Some(ref n) = name {
        ctx.path_ctx.push(n);
    }

    let spec_id = ctx.new_node(
        kinds::GO_TYPE_SPEC,
        name.clone(),
        Some(parent_id),
        position,
        meta,
        Some(span_start_line(node)),
        Some(span_end_line(node)),
    );

    // Walk the underlying type (struct_type, interface_type, etc.)
    if let Some(type_node) = node.child_by_field_name("type") {
        match type_node.kind() {
            "struct_type" => walk_struct(ctx, &type_node, &spec_id, 0, source),
            "interface_type" => walk_interface(ctx, &type_node, &spec_id, 0, source),
            _ => {
                // Other type definitions (alias, etc.) — store as-is
            }
        }
    }

    if name.is_some() {
        ctx.path_ctx.pop();
    }
}

fn walk_struct(
    ctx: &mut GoWalkCtx,
    node: &tree_sitter::Node,
    parent_id: &str,
    position: i32,
    source: &str,
) {
    let struct_id = ctx.new_node(
        kinds::GO_STRUCT,
        None,
        Some(parent_id),
        position,
        json!({"source": node_text(node, source)}),
        Some(span_start_line(node)),
        Some(span_end_line(node)),
    );

    // struct_type contains field_declaration_list which contains field_declaration nodes
    let mut field_idx = 0;
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if child.kind() == "field_declaration" {
            add_field(ctx, &child, &struct_id, field_idx, source);
            field_idx += 1;
        } else if child.kind() == "field_declaration_list" {
            let mut inner_cursor = child.walk();
            for field in child.named_children(&mut inner_cursor) {
                if field.kind() == "field_declaration" {
                    add_field(ctx, &field, &struct_id, field_idx, source);
                    field_idx += 1;
                }
            }
        }
    }
}

fn add_field(
    ctx: &mut GoWalkCtx,
    node: &tree_sitter::Node,
    parent_id: &str,
    position: i32,
    source: &str,
) {
    let meta = metadata::field_metadata(node, source);
    let name = node
        .child_by_field_name("name")
        .map(|n| node_text(&n, source).to_string());

    ctx.new_node(
        kinds::GO_FIELD,
        name,
        Some(parent_id),
        position,
        meta,
        Some(span_start_line(node)),
        Some(span_end_line(node)),
    );
}

fn walk_interface(
    ctx: &mut GoWalkCtx,
    node: &tree_sitter::Node,
    parent_id: &str,
    position: i32,
    source: &str,
) {
    let iface_id = ctx.new_node(
        kinds::GO_INTERFACE,
        None,
        Some(parent_id),
        position,
        json!({"source": node_text(node, source)}),
        Some(span_start_line(node)),
        Some(span_end_line(node)),
    );

    let mut cursor = node.walk();
    for (i, child) in node.named_children(&mut cursor).enumerate() {
        if child.kind() == "method_spec" {
            let meta = metadata::method_spec_metadata(&child, source);
            let name = child
                .child_by_field_name("name")
                .map(|n| node_text(&n, source).to_string());

            ctx.new_node(
                kinds::GO_METHOD_SPEC,
                name,
                Some(&iface_id),
                i as i32,
                meta,
                Some(span_start_line(&child)),
                Some(span_end_line(&child)),
            );
        } else if child.is_named() {
            // Embedded interface constraints
            walk_generic(ctx, &child, &iface_id, i as i32);
        }
    }
}

fn walk_var_decl(
    ctx: &mut GoWalkCtx,
    node: &tree_sitter::Node,
    parent_id: &str,
    position: i32,
) {
    let source = ctx.source.clone();
    let decl_id = ctx.new_node(
        kinds::GO_VAR_DECL,
        None,
        Some(parent_id),
        position,
        json!({"source": node_text(node, &source)}),
        Some(span_start_line(node)),
        Some(span_end_line(node)),
    );

    let mut spec_idx = 0;
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if child.kind() == "var_spec" {
            add_var_spec(ctx, &child, &decl_id, spec_idx, kinds::GO_VAR_SPEC);
            spec_idx += 1;
        } else if child.kind() == "var_spec_list" {
            let mut inner_cursor = child.walk();
            for spec in child.named_children(&mut inner_cursor) {
                if spec.kind() == "var_spec" {
                    add_var_spec(ctx, &spec, &decl_id, spec_idx, kinds::GO_VAR_SPEC);
                    spec_idx += 1;
                }
            }
        }
    }
}

fn walk_const_decl(
    ctx: &mut GoWalkCtx,
    node: &tree_sitter::Node,
    parent_id: &str,
    position: i32,
) {
    let source = ctx.source.clone();
    let decl_id = ctx.new_node(
        kinds::GO_CONST_DECL,
        None,
        Some(parent_id),
        position,
        json!({"source": node_text(node, &source)}),
        Some(span_start_line(node)),
        Some(span_end_line(node)),
    );

    let mut spec_idx = 0;
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if child.kind() == "const_spec" {
            add_var_spec(ctx, &child, &decl_id, spec_idx, kinds::GO_CONST_SPEC);
            spec_idx += 1;
        } else if child.kind() == "const_spec_list" {
            let mut inner_cursor = child.walk();
            for spec in child.named_children(&mut inner_cursor) {
                if spec.kind() == "const_spec" {
                    add_var_spec(ctx, &spec, &decl_id, spec_idx, kinds::GO_CONST_SPEC);
                    spec_idx += 1;
                }
            }
        }
    }
}

fn add_var_spec(
    ctx: &mut GoWalkCtx,
    node: &tree_sitter::Node,
    parent_id: &str,
    position: i32,
    kind: &str,
) {
    let source = ctx.source.clone();
    let meta = metadata::var_const_spec_metadata(node, &source);
    let name = node
        .child_by_field_name("name")
        .map(|n| node_text(&n, &source).to_string());

    ctx.new_node(
        kind,
        name,
        Some(parent_id),
        position,
        meta,
        Some(span_start_line(node)),
        Some(span_end_line(node)),
    );
}

fn walk_block(ctx: &mut GoWalkCtx, node: &tree_sitter::Node, parent_id: &str, position: i32) {
    let source = ctx.source.clone();
    let block_id = ctx.new_node(
        kinds::GO_BLOCK,
        None,
        Some(parent_id),
        position,
        json!({}),
        Some(span_start_line(node)),
        Some(span_end_line(node)),
    );

    let _ = source; // used implicitly via walk_children
    walk_children(ctx, node, &block_id);
}

fn walk_statement(
    ctx: &mut GoWalkCtx,
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

/// Walk a leaf node — no recursion into children.
fn walk_leaf(
    ctx: &mut GoWalkCtx,
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

fn walk_func_literal(
    ctx: &mut GoWalkCtx,
    node: &tree_sitter::Node,
    parent_id: &str,
    position: i32,
    source: &str,
) {
    let func_id = ctx.new_node(
        kinds::GO_FUNC_LIT,
        None,
        Some(parent_id),
        position,
        json!({"source": node_text(node, source)}),
        Some(span_start_line(node)),
        Some(span_end_line(node)),
    );

    if let Some(body) = node.child_by_field_name("body") {
        walk_block(ctx, &body, &func_id, 0);
    }
}

fn walk_case_clause(
    ctx: &mut GoWalkCtx,
    node: &tree_sitter::Node,
    parent_id: &str,
    position: i32,
    kind: &str,
) {
    let source = ctx.source.clone();
    let case_id = ctx.new_node(
        kind,
        None,
        Some(parent_id),
        position,
        json!({"source": node_text(node, &source)}),
        Some(span_start_line(node)),
        Some(span_end_line(node)),
    );

    walk_children(ctx, node, &case_id);
}

/// Fallback: creates a go_other node, preserves ts_kind in metadata, recurses.
fn walk_generic(
    ctx: &mut GoWalkCtx,
    node: &tree_sitter::Node,
    parent_id: &str,
    position: i32,
) {
    let source = ctx.source.clone();
    let ts_kind = node.kind();
    let kerai_kind = kinds::ts_kind_to_go_kind(ts_kind);

    // If we have a specific mapping, use it as a leaf
    if kerai_kind != kinds::GO_OTHER {
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
        kinds::GO_OTHER,
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

/// Collect byte ranges of string literals for comment exclusion.
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
        "interpreted_string_literal" | "raw_string_literal" => {
            let start_line = (node.start_position().row + 1) as usize;
            let end_line = (node.end_position().row + 1) as usize;
            spans.push((start_line, end_line));
        }
        _ => {}
    }

    let _ = source; // available for future use
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_string_spans_recursive(&child, source, spans);
    }
}
