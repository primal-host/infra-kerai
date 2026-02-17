/// Extract comments from Rust source text.
///
/// syn does not preserve regular comments (only doc comments via attributes).
/// This module scans source text line-by-line to find // and /* */ comments,
/// groups consecutive line comments into blocks, classifies placement, and
/// excludes false positives inside string literals.

use syn::visit::Visit;

/// Raw comment info extracted from a single line/block.
#[derive(Debug, Clone)]
pub struct CommentInfo {
    pub line: usize,
    pub col: usize,
    pub text: String,
    pub is_doc: bool,
    pub is_inner: bool,
    pub is_block_style: bool,
}

/// Where a comment block sits relative to AST nodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommentPlacement {
    Above,
    Trailing,
    Between,
    Eof,
}

/// A group of consecutive line comments or a single block comment.
#[derive(Debug, Clone)]
pub struct CommentBlock {
    pub start_line: usize,
    pub end_line: usize,
    pub col: usize,
    pub lines: Vec<String>,
    pub is_doc: bool,
    pub is_inner: bool,
    pub is_block_style: bool,
    pub placement: CommentPlacement,
}

/// Collect (start_line, end_line) spans for all string literals in a parsed file.
/// These are exclusion zones — any comment on a line within a string literal span
/// is a false positive (e.g. `"// not a comment"`).
pub fn collect_string_spans(file: &syn::File) -> Vec<(usize, usize)> {
    struct StringVisitor {
        spans: Vec<(usize, usize)>,
    }

    impl<'ast> Visit<'ast> for StringVisitor {
        fn visit_lit(&mut self, lit: &'ast syn::Lit) {
            match lit {
                syn::Lit::Str(s) => {
                    let span = s.span();
                    self.spans.push((span.start().line, span.end().line));
                }
                syn::Lit::ByteStr(s) => {
                    let span = s.span();
                    self.spans.push((span.start().line, span.end().line));
                }
                syn::Lit::CStr(s) => {
                    let span = s.span();
                    self.spans.push((span.start().line, span.end().line));
                }
                _ => {}
            }
            syn::visit::visit_lit(self, lit);
        }
    }

    let mut visitor = StringVisitor { spans: Vec::new() };
    visitor.visit_file(file);
    visitor.spans
}

/// Check if a line falls within any exclusion zone.
fn is_excluded(line: usize, exclusions: &[(usize, usize)]) -> bool {
    exclusions.iter().any(|&(start, end)| {
        // Only exclude if the string literal spans multiple lines
        // and this line is strictly inside (not the start line, which has the opening quote)
        start != end && line > start && line <= end
    })
}

/// Extract all comments from source text, skipping lines in exclusion zones.
pub fn extract_comments(source: &str, exclusions: &[(usize, usize)]) -> Vec<CommentInfo> {
    let mut comments = Vec::new();
    let mut in_block_comment = false;
    let mut block_start_line = 0;
    let mut block_start_col = 0;
    let mut block_text = String::new();
    let mut block_is_doc = false;
    let mut block_is_inner = false;

    for (line_idx, line) in source.lines().enumerate() {
        let line_num = line_idx + 1;

        if in_block_comment {
            if let Some(end_pos) = line.find("*/") {
                block_text.push('\n');
                block_text.push_str(&line[..end_pos]);
                if !is_excluded(block_start_line, exclusions) {
                    comments.push(CommentInfo {
                        line: block_start_line,
                        col: block_start_col,
                        text: block_text.clone(),
                        is_doc: block_is_doc,
                        is_inner: block_is_inner,
                        is_block_style: true,
                    });
                }
                block_text.clear();
                in_block_comment = false;
            } else {
                block_text.push('\n');
                block_text.push_str(line);
            }
            continue;
        }

        if is_excluded(line_num, exclusions) {
            continue;
        }

        let trimmed = line.trim_start();
        let col = line.len() - trimmed.len() + 1;

        if trimmed.starts_with("///") && !trimmed.starts_with("////") {
            let text = trimmed.strip_prefix("///").unwrap_or("").trim_start();
            comments.push(CommentInfo {
                line: line_num,
                col,
                text: text.to_string(),
                is_doc: true,
                is_inner: false,
                is_block_style: false,
            });
        } else if trimmed.starts_with("//!") {
            let text = trimmed.strip_prefix("//!").unwrap_or("").trim_start();
            comments.push(CommentInfo {
                line: line_num,
                col,
                text: text.to_string(),
                is_doc: true,
                is_inner: true,
                is_block_style: false,
            });
        } else if trimmed.starts_with("//") {
            let text = trimmed.strip_prefix("//").unwrap_or("").trim_start();
            comments.push(CommentInfo {
                line: line_num,
                col,
                text: text.to_string(),
                is_doc: false,
                is_inner: false,
                is_block_style: false,
            });
        } else if let Some(pos) = trimmed.find("/*") {
            let after_open = &trimmed[pos + 2..];
            block_is_doc = after_open.starts_with('*') && !after_open.starts_with("**");
            block_is_inner = after_open.starts_with('!');

            if let Some(end_pos) = after_open.find("*/") {
                let text_start = if block_is_doc || block_is_inner { 1 } else { 0 };
                let text = &after_open[text_start..end_pos].trim();
                comments.push(CommentInfo {
                    line: line_num,
                    col: col + pos,
                    text: text.to_string(),
                    is_doc: block_is_doc,
                    is_inner: block_is_inner,
                    is_block_style: true,
                });
            } else {
                in_block_comment = true;
                block_start_line = line_num;
                block_start_col = col + pos;
                let text_start = if block_is_doc || block_is_inner {
                    pos + 3
                } else {
                    pos + 2
                };
                block_text = trimmed[text_start..].to_string();
            }
        }
    }

    comments
}

/// Group consecutive line comments into CommentBlocks.
///
/// Merges adjacent `//` comments (same column, adjacent lines, all same
/// doc/non-doc type) into one CommentBlock. `/* */` comments become
/// single-entry blocks. Placement defaults to Above (refined later in matching).
pub fn group_comments(comments: Vec<CommentInfo>) -> Vec<CommentBlock> {
    let mut blocks: Vec<CommentBlock> = Vec::new();

    for comment in comments {
        if comment.is_block_style {
            // Block comments are always standalone
            blocks.push(CommentBlock {
                start_line: comment.line,
                end_line: comment.line,
                col: comment.col,
                lines: vec![comment.text],
                is_doc: comment.is_doc,
                is_inner: comment.is_inner,
                is_block_style: true,
                placement: CommentPlacement::Above,
            });
            continue;
        }

        // Try to merge with the previous block
        let can_merge = if let Some(prev) = blocks.last() {
            !prev.is_block_style
                && prev.col == comment.col
                && prev.end_line + 1 == comment.line
                && prev.is_doc == comment.is_doc
                && prev.is_inner == comment.is_inner
        } else {
            false
        };

        if can_merge {
            let prev = blocks.last_mut().unwrap();
            prev.end_line = comment.line;
            prev.lines.push(comment.text);
        } else {
            blocks.push(CommentBlock {
                start_line: comment.line,
                end_line: comment.line,
                col: comment.col,
                lines: vec![comment.text],
                is_doc: comment.is_doc,
                is_inner: comment.is_inner,
                is_block_style: false,
                placement: CommentPlacement::Above,
            });
        }
    }

    blocks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_line_comment() {
        let source = "// hello\nfn main() {}\n";
        let comments = extract_comments(source, &[]);
        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0].text, "hello");
        assert!(!comments[0].is_doc);
        assert!(!comments[0].is_block_style);
    }

    #[test]
    fn test_doc_comments() {
        let source = "/// doc\n//! inner\nfn main() {}\n";
        let comments = extract_comments(source, &[]);
        assert_eq!(comments.len(), 2);
        assert!(comments[0].is_doc);
        assert!(!comments[0].is_inner);
        assert!(comments[1].is_doc);
        assert!(comments[1].is_inner);
    }

    #[test]
    fn test_block_comment() {
        let source = "/* block */\nfn main() {}\n";
        let comments = extract_comments(source, &[]);
        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0].text, "block");
        assert!(comments[0].is_block_style);
    }

    #[test]
    fn test_grouping_consecutive() {
        let source = "// line 1\n// line 2\n// line 3\nfn main() {}\n";
        let comments = extract_comments(source, &[]);
        assert_eq!(comments.len(), 3);

        let blocks = group_comments(comments);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].lines.len(), 3);
        assert_eq!(blocks[0].start_line, 1);
        assert_eq!(blocks[0].end_line, 3);
    }

    #[test]
    fn test_grouping_gap_splits() {
        let source = "// group 1\n\n// group 2\nfn main() {}\n";
        let comments = extract_comments(source, &[]);
        let blocks = group_comments(comments);
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].lines, vec!["group 1"]);
        assert_eq!(blocks[1].lines, vec!["group 2"]);
    }

    #[test]
    fn test_grouping_doc_vs_nondoc_splits() {
        let source = "/// doc\n// regular\nfn main() {}\n";
        let comments = extract_comments(source, &[]);
        let blocks = group_comments(comments);
        assert_eq!(blocks.len(), 2);
        assert!(blocks[0].is_doc);
        assert!(!blocks[1].is_doc);
    }

    #[test]
    fn test_exclusion_zones_multiline_string() {
        // Line 2 of a multiline string should be excluded
        // Simulate: line 1 starts string, line 2 has //, line 3 ends string
        let source = "let s = \"start\n// not a comment\nend\";\nfn main() {}\n";
        let exclusions = vec![(1, 3)]; // String spans lines 1-3
        let comments = extract_comments(source, &exclusions);
        assert!(comments.is_empty(), "Should not extract comments inside string literals");
    }

    #[test]
    fn test_single_line_string_not_excluded() {
        // Single-line strings: the start line equals end line, so no exclusion
        let source = "let s = \"// not a comment\";\n// real comment\nfn main() {}\n";
        // For single-line string on line 1: (1, 1) — is_excluded returns false since start==end
        let exclusions = vec![(1, 1)];
        let comments = extract_comments(source, &exclusions);
        // The // inside the string won't be caught by our line-by-line scanner
        // because it looks for trimmed.starts_with("//") and the line starts with "let"
        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0].text, "real comment");
    }
}
