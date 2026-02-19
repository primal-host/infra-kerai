/// Generic tree-sitter node helpers shared across language walkers.

/// Extract the source text for a tree-sitter node.
pub fn node_text<'a>(node: &tree_sitter::Node, source: &'a str) -> &'a str {
    &source[node.byte_range()]
}

/// 1-based start line for a tree-sitter node (kerai convention).
pub fn span_start_line(node: &tree_sitter::Node) -> i32 {
    (node.start_position().row + 1) as i32
}

/// 1-based end line for a tree-sitter node (kerai convention).
pub fn span_end_line(node: &tree_sitter::Node) -> i32 {
    (node.end_position().row + 1) as i32
}
