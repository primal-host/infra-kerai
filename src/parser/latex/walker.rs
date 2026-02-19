/// LaTeX CST walker — converts tree-sitter LaTeX parse tree into NodeRow/EdgeRow vectors.
use std::collections::HashMap;

use serde_json::json;
use uuid::Uuid;

use crate::parser::ast_walker::{EdgeRow, NodeRow};
use crate::parser::path_builder::PathContext;
use crate::parser::treesitter::cursor::{node_text, span_end_line, span_start_line};

use super::kinds;
use super::metadata;

/// Walk context accumulator passed through the recursion.
struct LatexWalkCtx {
    source: String,
    instance_id: String,
    nodes: Vec<NodeRow>,
    edges: Vec<EdgeRow>,
    path_ctx: PathContext,
    /// Map from \label key → node_id of the labeled element
    label_map: HashMap<String, String>,
    /// Pending \ref references: (ref_node_id, label_key)
    pending_refs: Vec<(String, String)>,
    /// Pending \cite references: (cite_node_id, Vec<cite_key>)
    pending_cites: Vec<(String, Vec<String>)>,
    /// Section stack for path building: (depth, node_id, name)
    section_stack: Vec<(u8, String, String)>,
}

impl LatexWalkCtx {
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
            language: Some("latex".to_string()),
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

    /// Determine the parent for a new section node based on depth.
    /// Returns the parent_id by popping sections of equal or greater depth.
    fn section_parent(&mut self, depth: u8, file_node_id: &str) -> String {
        // Pop sections that are at the same or deeper level
        while let Some(&(d, _, _)) = self.section_stack.last() {
            if d >= depth {
                let (_, _, ref name) = self.section_stack.pop().expect("stack not empty");
                // Pop the corresponding path segment
                let _ = name;
                self.path_ctx.pop();
            } else {
                break;
            }
        }

        // Parent is the top of the stack (if any), otherwise the file node
        self.section_stack
            .last()
            .map(|(_, ref id, _)| id.clone())
            .unwrap_or_else(|| file_node_id.to_string())
    }
}

/// Walk a parsed LaTeX tree and produce NodeRow/EdgeRow vectors.
///
/// After walking the CST, resolves intra-file \label/\ref cross-references
/// into edges.
pub fn walk_latex_file(
    tree: &tree_sitter::Tree,
    source: &str,
    file_node_id: &str,
    instance_id: &str,
    path_ctx: PathContext,
) -> (Vec<NodeRow>, Vec<EdgeRow>, Vec<(String, Vec<String>)>) {
    let mut ctx = LatexWalkCtx {
        source: source.to_string(),
        instance_id: instance_id.to_string(),
        nodes: Vec::new(),
        edges: Vec::new(),
        path_ctx,
        label_map: HashMap::new(),
        pending_refs: Vec::new(),
        pending_cites: Vec::new(),
        section_stack: Vec::new(),
    };

    let root = tree.root_node();
    walk_children(&mut ctx, &root, file_node_id);

    // Post-walk: resolve \ref → \label edges
    for (ref_node_id, label_key) in &ctx.pending_refs {
        if let Some(target_id) = ctx.label_map.get(label_key) {
            ctx.edges.push(EdgeRow {
                id: Uuid::new_v4().to_string(),
                source_id: ref_node_id.clone(),
                target_id: target_id.clone(),
                relation: "references".to_string(),
                metadata: json!({"label": label_key}),
            });
        }
    }

    let pending_cites = ctx.pending_cites.clone();
    (ctx.nodes, ctx.edges, pending_cites)
}

/// Walk all children of a node.
fn walk_children(ctx: &mut LatexWalkCtx, node: &tree_sitter::Node, parent_id: &str) {
    let mut cursor = node.walk();
    let children: Vec<_> = node.children(&mut cursor).collect();
    for (i, child) in children.iter().enumerate() {
        walk_node(ctx, child, parent_id, i as i32);
    }
}

/// Dispatch a single tree-sitter node to the appropriate handler.
fn walk_node(ctx: &mut LatexWalkCtx, node: &tree_sitter::Node, parent_id: &str, position: i32) {
    match node.kind() {
        // Generic commands (\command_name{args})
        "generic_command" | "new_command_definition" | "title_declaration" => {
            walk_generic_command(ctx, node, parent_id, position);
        }

        // Environments (\begin{...}...\end{...})
        "generic_environment" | "math_environment" => {
            walk_environment(ctx, node, parent_id, position);
        }

        // Inline math $...$
        "inline_formula" => {
            walk_inline_math(ctx, node, parent_id, position);
        }

        // Display math \[...\] or $$...$$
        "displayed_equation" => {
            walk_display_math(ctx, node, parent_id, position);
        }

        // Package inclusion
        "package_include" => {
            walk_usepackage(ctx, node, parent_id, position);
        }

        // Document class
        "class_include" => {
            walk_documentclass(ctx, node, parent_id, position);
        }

        // Import commands (\input, \include)
        "import" | "latex_include" => {
            walk_input(ctx, node, parent_id, position);
        }

        // Text blocks and paragraphs
        "text" => {
            // Skip pure text nodes — they're just content between commands
        }

        // Recurse into structural nodes
        "document" | "preamble" | "curly_group" | "curly_group_text"
        | "curly_group_text_list" | "curly_group_command" => {
            walk_children(ctx, node, parent_id);
        }

        // Skip anonymous/whitespace/comment nodes
        "comment" | "line_comment" | "block_comment"
        | "ERROR" | "MISSING" => {}

        // For all other named nodes, recurse into their children
        _ if node.is_named() => {
            walk_children(ctx, node, parent_id);
        }

        _ => {}
    }
}

/// Walk a generic command node.
///
/// Dispatches to specialized handlers based on the command name
/// (section, cite, label, ref, footnote, caption, input/include).
fn walk_generic_command(
    ctx: &mut LatexWalkCtx,
    node: &tree_sitter::Node,
    parent_id: &str,
    position: i32,
) {
    let source = ctx.source.clone();
    let cmd_name = extract_command_name(node, &source);

    match cmd_name.as_str() {
        // Sectioning commands
        "\\part" | "\\part*" | "\\chapter" | "\\chapter*"
        | "\\section" | "\\section*" | "\\subsection" | "\\subsection*"
        | "\\subsubsection" | "\\subsubsection*"
        | "\\paragraph" | "\\paragraph*" => {
            walk_section(ctx, node, parent_id, position, &cmd_name);
        }

        // Citations
        "\\cite" | "\\citep" | "\\citet" | "\\citealt" | "\\citealp"
        | "\\citeauthor" | "\\citeyear" | "\\citetext"
        | "\\autocite" | "\\textcite" | "\\parencite"
        | "\\Cite" | "\\Citep" | "\\Citet" => {
            walk_citation(ctx, node, parent_id, position, &cmd_name);
        }

        // Labels
        "\\label" => {
            walk_label(ctx, node, parent_id, position);
        }

        // References
        "\\ref" | "\\eqref" | "\\pageref" | "\\nameref"
        | "\\autoref" | "\\cref" | "\\Cref" | "\\vref" => {
            walk_ref(ctx, node, parent_id, position, &cmd_name);
        }

        // Captions
        "\\caption" => {
            walk_caption(ctx, node, parent_id, position);
        }

        // Footnotes
        "\\footnote" => {
            walk_footnote(ctx, node, parent_id, position);
        }

        // File inclusion
        "\\input" | "\\include" => {
            walk_input_cmd(ctx, node, parent_id, position, &cmd_name);
        }

        // Other commands — create a generic node only if interesting
        _ => {
            // Skip common formatting commands to avoid noise
            if !is_formatting_command(&cmd_name) {
                let meta = metadata::command_metadata(node, &source, &cmd_name);
                ctx.new_node(
                    kinds::LATEX_COMMAND,
                    Some(cmd_name),
                    Some(parent_id),
                    position,
                    meta,
                    Some(span_start_line(node)),
                    Some(span_end_line(node)),
                );
            }
        }
    }
}

/// Walk a sectioning command.
fn walk_section(
    ctx: &mut LatexWalkCtx,
    node: &tree_sitter::Node,
    file_node_id: &str,
    position: i32,
    cmd_name: &str,
) {
    let source = ctx.source.clone();
    let base_cmd = cmd_name.trim_end_matches('*');
    let kind = kinds::section_cmd_to_kind(base_cmd).unwrap_or(kinds::LATEX_COMMAND);
    let depth = kinds::section_depth(base_cmd).unwrap_or(5);

    let meta = metadata::section_metadata(node, &source, cmd_name);
    let title = meta
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    // Determine parent based on section hierarchy
    let parent_id = ctx.section_parent(depth, file_node_id);

    // Sanitize title for path building
    let path_segment = sanitize_path_segment(&title);
    ctx.path_ctx.push(&path_segment);

    let section_id = ctx.new_node(
        kind,
        Some(title.clone()),
        Some(&parent_id),
        position,
        meta,
        Some(span_start_line(node)),
        Some(span_end_line(node)),
    );

    ctx.section_stack
        .push((depth, section_id, path_segment));
}

/// Walk a citation command.
fn walk_citation(
    ctx: &mut LatexWalkCtx,
    node: &tree_sitter::Node,
    parent_id: &str,
    position: i32,
    cmd_name: &str,
) {
    let source = ctx.source.clone();
    let meta = metadata::citation_metadata(node, &source, cmd_name);

    let keys: Vec<String> = meta
        .get("keys")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    let keys_str = keys.join(", ");

    let cite_id = ctx.new_node(
        kinds::LATEX_CITATION,
        Some(keys_str),
        Some(parent_id),
        position,
        meta,
        Some(span_start_line(node)),
        Some(span_end_line(node)),
    );

    if !keys.is_empty() {
        ctx.pending_cites.push((cite_id, keys));
    }
}

/// Walk a \label command.
fn walk_label(
    ctx: &mut LatexWalkCtx,
    node: &tree_sitter::Node,
    parent_id: &str,
    position: i32,
) {
    let source = ctx.source.clone();
    let meta = metadata::label_metadata(node, &source);

    let key = meta
        .get("key")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let label_id = ctx.new_node(
        kinds::LATEX_LABEL,
        Some(key.clone()),
        Some(parent_id),
        position,
        meta,
        Some(span_start_line(node)),
        Some(span_end_line(node)),
    );

    // Register in label_map: the label is attached to the parent element
    if !key.is_empty() {
        ctx.label_map.insert(key, label_id);
    }
}

/// Walk a \ref command.
fn walk_ref(
    ctx: &mut LatexWalkCtx,
    node: &tree_sitter::Node,
    parent_id: &str,
    position: i32,
    cmd_name: &str,
) {
    let source = ctx.source.clone();
    let meta = metadata::ref_metadata(node, &source, cmd_name);

    let key = meta
        .get("key")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let ref_id = ctx.new_node(
        kinds::LATEX_REF,
        Some(key.clone()),
        Some(parent_id),
        position,
        meta,
        Some(span_start_line(node)),
        Some(span_end_line(node)),
    );

    if !key.is_empty() {
        ctx.pending_refs.push((ref_id, key));
    }
}

/// Walk a \caption command.
fn walk_caption(
    ctx: &mut LatexWalkCtx,
    node: &tree_sitter::Node,
    parent_id: &str,
    position: i32,
) {
    let source = ctx.source.clone();
    let meta = metadata::caption_metadata(node, &source);

    let text = meta
        .get("text")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    ctx.new_node(
        kinds::LATEX_CAPTION,
        text,
        Some(parent_id),
        position,
        meta,
        Some(span_start_line(node)),
        Some(span_end_line(node)),
    );
}

/// Walk a \footnote command.
fn walk_footnote(
    ctx: &mut LatexWalkCtx,
    node: &tree_sitter::Node,
    parent_id: &str,
    position: i32,
) {
    let source = ctx.source.clone();
    let meta = metadata::command_metadata(node, &source, "\\footnote");

    let text = meta
        .get("arg")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    ctx.new_node(
        kinds::LATEX_FOOTNOTE,
        text,
        Some(parent_id),
        position,
        meta,
        Some(span_start_line(node)),
        Some(span_end_line(node)),
    );
}

/// Walk an \input or \include command.
fn walk_input_cmd(
    ctx: &mut LatexWalkCtx,
    node: &tree_sitter::Node,
    parent_id: &str,
    position: i32,
    cmd_name: &str,
) {
    let source = ctx.source.clone();
    let kind = if cmd_name == "\\include" {
        kinds::LATEX_INCLUDE
    } else {
        kinds::LATEX_INPUT
    };

    let meta = metadata::input_metadata(node, &source, cmd_name);
    let path = meta
        .get("path")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    ctx.new_node(
        kind,
        path,
        Some(parent_id),
        position,
        meta,
        Some(span_start_line(node)),
        Some(span_end_line(node)),
    );
}

/// Walk a \usepackage command.
fn walk_usepackage(
    ctx: &mut LatexWalkCtx,
    node: &tree_sitter::Node,
    parent_id: &str,
    position: i32,
) {
    let source = ctx.source.clone();
    let meta = metadata::usepackage_metadata(node, &source);

    let pkg = meta
        .get("package")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    ctx.new_node(
        kinds::LATEX_USEPACKAGE,
        pkg,
        Some(parent_id),
        position,
        meta,
        Some(span_start_line(node)),
        Some(span_end_line(node)),
    );
}

/// Walk a \documentclass command.
fn walk_documentclass(
    ctx: &mut LatexWalkCtx,
    node: &tree_sitter::Node,
    parent_id: &str,
    position: i32,
) {
    let source = ctx.source.clone();
    let meta = metadata::documentclass_metadata(node, &source);

    let class = meta
        .get("class")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    ctx.new_node(
        kinds::LATEX_DOCUMENTCLASS,
        class,
        Some(parent_id),
        position,
        meta,
        Some(span_start_line(node)),
        Some(span_end_line(node)),
    );
}

/// Walk an environment (\begin{env}...\end{env}).
fn walk_environment(
    ctx: &mut LatexWalkCtx,
    node: &tree_sitter::Node,
    parent_id: &str,
    position: i32,
) {
    let source = ctx.source.clone();

    // Extract environment name from the \begin{name} child
    let env_name = extract_env_name(node, &source).unwrap_or_default();
    let kind = kinds::env_name_to_kind(&env_name);

    // Special case: document environment represents the document body
    if env_name == "document" {
        walk_children(ctx, node, parent_id);
        return;
    }

    let meta = metadata::environment_metadata(&env_name, node, &source);

    let env_id = ctx.new_node(
        kind,
        Some(env_name),
        Some(parent_id),
        position,
        meta,
        Some(span_start_line(node)),
        Some(span_end_line(node)),
    );

    // Recurse into environment body
    walk_children(ctx, node, &env_id);
}

/// Walk inline math ($..$ or \(...\)).
fn walk_inline_math(
    ctx: &mut LatexWalkCtx,
    node: &tree_sitter::Node,
    parent_id: &str,
    position: i32,
) {
    let source = ctx.source.clone();
    let text = node_text(node, &source).to_string();

    ctx.new_node(
        kinds::LATEX_INLINE_MATH,
        Some(text),
        Some(parent_id),
        position,
        json!({}),
        Some(span_start_line(node)),
        Some(span_end_line(node)),
    );
}

/// Walk display math (\[..\] or $$..$$).
fn walk_display_math(
    ctx: &mut LatexWalkCtx,
    node: &tree_sitter::Node,
    parent_id: &str,
    position: i32,
) {
    let source = ctx.source.clone();
    let text = node_text(node, &source).to_string();

    ctx.new_node(
        kinds::LATEX_DISPLAY_MATH,
        Some(text),
        Some(parent_id),
        position,
        json!({}),
        Some(span_start_line(node)),
        Some(span_end_line(node)),
    );
}

/// Walk an \input or \include tree-sitter node (the import/latex_include kind).
fn walk_input(
    ctx: &mut LatexWalkCtx,
    node: &tree_sitter::Node,
    parent_id: &str,
    position: i32,
) {
    let source = ctx.source.clone();
    let text = node_text(node, &source);
    let cmd_name = if text.starts_with("\\include") {
        "\\include"
    } else {
        "\\input"
    };

    walk_input_cmd(ctx, node, parent_id, position, cmd_name);
}

/// Extract the command name from a generic_command node.
///
/// Looks for the first child whose kind is "command_name" and returns its text.
fn extract_command_name(node: &tree_sitter::Node, source: &str) -> String {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "command_name" {
            return node_text(&child, source).to_string();
        }
    }
    // Fallback: try the node text itself if it starts with \
    let text = node_text(node, source);
    if text.starts_with('\\') {
        text.split_whitespace()
            .next()
            .unwrap_or(text)
            .split('{')
            .next()
            .unwrap_or(text)
            .to_string()
    } else {
        String::new()
    }
}

/// Extract the environment name from a generic_environment node.
///
/// Looks for the \begin{name} part.
fn extract_env_name(node: &tree_sitter::Node, source: &str) -> Option<String> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "begin" {
            // The begin node contains the environment name in a curly_group child
            let mut inner_cursor = child.walk();
            for inner in child.children(&mut inner_cursor) {
                let k = inner.kind();
                if k == "curly_group" || k == "curly_group_text" || k == "curly_group_text_list"
                    || k == "name"
                {
                    let text = node_text(&inner, source);
                    let stripped = text
                        .strip_prefix('{')
                        .unwrap_or(text)
                        .strip_suffix('}')
                        .unwrap_or(text);
                    return Some(stripped.to_string());
                }
            }
        }
    }
    None
}

/// Sanitize a string for use as an ltree path segment.
fn sanitize_path_segment(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .take(64) // limit segment length
        .collect::<String>()
        .trim_matches('_')
        .to_string()
}

/// Returns true for common formatting commands that should be skipped
/// to reduce noise in the AST.
fn is_formatting_command(cmd: &str) -> bool {
    matches!(
        cmd,
        "\\textbf" | "\\textit" | "\\emph" | "\\textrm" | "\\textsf" | "\\texttt"
        | "\\textsc" | "\\textsl" | "\\textup" | "\\textnormal"
        | "\\bf" | "\\it" | "\\em" | "\\rm" | "\\sf" | "\\tt" | "\\sc" | "\\sl"
        | "\\small" | "\\large" | "\\Large" | "\\LARGE" | "\\huge" | "\\Huge"
        | "\\tiny" | "\\scriptsize" | "\\footnotesize" | "\\normalsize"
        | "\\centering" | "\\raggedright" | "\\raggedleft"
        | "\\noindent" | "\\indent" | "\\par" | "\\newline" | "\\linebreak"
        | "\\hspace" | "\\vspace" | "\\hfill" | "\\vfill"
        | "\\maketitle" | "\\tableofcontents" | "\\listoffigures" | "\\listoftables"
        | "\\clearpage" | "\\newpage" | "\\pagebreak"
        | "\\medskip" | "\\bigskip" | "\\smallskip"
        | "\\item"
    )
}
