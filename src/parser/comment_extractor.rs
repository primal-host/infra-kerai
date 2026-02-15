/// Extract comments from Rust source text.
///
/// syn does not preserve regular comments (only doc comments via attributes).
/// This module scans source text line-by-line to find // and /* */ comments.

#[derive(Debug)]
pub struct CommentInfo {
    pub line: usize,
    pub col: usize,
    pub text: String,
    pub is_doc: bool,
    pub is_inner: bool,
}

/// Extract all comments from source text.
pub fn extract_comments(source: &str) -> Vec<CommentInfo> {
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
                comments.push(CommentInfo {
                    line: block_start_line,
                    col: block_start_col,
                    text: block_text.clone(),
                    is_doc: block_is_doc,
                    is_inner: block_is_inner,
                });
                block_text.clear();
                in_block_comment = false;
            } else {
                block_text.push('\n');
                block_text.push_str(line);
            }
            continue;
        }

        let trimmed = line.trim_start();
        let col = line.len() - trimmed.len() + 1;

        if trimmed.starts_with("///") && !trimmed.starts_with("////") {
            // Doc comment (outer)
            let text = trimmed.strip_prefix("///").unwrap_or("").trim_start();
            comments.push(CommentInfo {
                line: line_num,
                col,
                text: text.to_string(),
                is_doc: true,
                is_inner: false,
            });
        } else if trimmed.starts_with("//!") {
            // Inner doc comment
            let text = trimmed.strip_prefix("//!").unwrap_or("").trim_start();
            comments.push(CommentInfo {
                line: line_num,
                col,
                text: text.to_string(),
                is_doc: true,
                is_inner: true,
            });
        } else if trimmed.starts_with("//") {
            // Regular line comment
            let text = trimmed.strip_prefix("//").unwrap_or("").trim_start();
            comments.push(CommentInfo {
                line: line_num,
                col,
                text: text.to_string(),
                is_doc: false,
                is_inner: false,
            });
        } else if let Some(pos) = trimmed.find("/*") {
            let after_open = &trimmed[pos + 2..];
            block_is_doc = after_open.starts_with('*') && !after_open.starts_with("**");
            block_is_inner = after_open.starts_with('!');

            if let Some(end_pos) = after_open.find("*/") {
                // Single-line block comment
                let text_start = if block_is_doc || block_is_inner { 1 } else { 0 };
                let text = &after_open[text_start..end_pos].trim();
                comments.push(CommentInfo {
                    line: line_num,
                    col: col + pos,
                    text: text.to_string(),
                    is_doc: block_is_doc,
                    is_inner: block_is_inner,
                });
            } else {
                // Multi-line block comment starts
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
