/// Format Rust source through prettyplease for canonical output,
/// preserving regular comments that syn would otherwise strip.

/// Parse and reformat Rust source. Preserves // and /* */ comments
/// by extracting them before formatting, then re-inserting them.
/// Falls back to raw input on parse failure.
pub fn format_source(raw: &str) -> String {
    let lines: Vec<&str> = raw.lines().collect();
    if lines.is_empty() {
        return String::new();
    }

    // Identify which lines are standalone comment lines
    // (lines where the trimmed content starts with // or /*)
    let mut comment_map: Vec<(usize, String)> = Vec::new();
    let mut code_lines: Vec<String> = Vec::new();

    for (idx, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with("//") && !trimmed.starts_with("///") && !trimmed.starts_with("//!") {
            // Regular line comment — save and replace with blank
            comment_map.push((idx, line.to_string()));
            code_lines.push(String::new());
        } else if trimmed.starts_with("/*") && !trimmed.starts_with("/**") && !trimmed.starts_with("/*!") {
            // Regular block comment — save and replace with blank
            comment_map.push((idx, line.to_string()));
            code_lines.push(String::new());
        } else {
            code_lines.push(line.to_string());
        }
    }

    if comment_map.is_empty() {
        // No regular comments — format normally
        match syn::parse_file(raw) {
            Ok(parsed) => prettyplease::unparse(&parsed),
            Err(_) => raw.to_string(),
        }
    } else {
        // Format the code portion (with comments replaced by blanks)
        let code_only = code_lines.join("\n");
        let formatted = match syn::parse_file(&code_only) {
            Ok(parsed) => prettyplease::unparse(&parsed),
            Err(_) => code_only,
        };

        // Re-insert comments. Strategy: put comments before the item
        // they're associated with by building the output from formatted
        // code lines and injecting saved comments.
        //
        // Since prettyplease may reformat and change line counts, we use
        // a simpler approach: collect comment groups and code items from
        // the original, format code items individually, reassemble.
        reassemble_with_comments(raw, &formatted, &comment_map)
    }
}

/// Reassemble formatted code with preserved comments.
///
/// Takes the original source (for structure reference), the formatted
/// code-only output, and the comment map. Returns the final source
/// with comments in their original relative positions.
fn reassemble_with_comments(
    raw: &str,
    _formatted: &str,
    _comment_map: &[(usize, String)],
) -> String {
    // For correctness, we use a segment-based approach:
    // Split the raw input into alternating comment/code segments,
    // format each code segment independently, then reassemble.
    let lines: Vec<&str> = raw.lines().collect();
    let mut segments: Vec<Segment> = Vec::new();
    let mut current_comments: Vec<String> = Vec::new();
    let mut current_code: Vec<String> = Vec::new();

    for line in &lines {
        let trimmed = line.trim();
        let is_comment = (trimmed.starts_with("//")
            && !trimmed.starts_with("///")
            && !trimmed.starts_with("//!"))
            || (trimmed.starts_with("/*")
                && !trimmed.starts_with("/**")
                && !trimmed.starts_with("/*!"));

        if is_comment {
            if !current_code.is_empty() {
                segments.push(Segment::Code(current_code.join("\n")));
                current_code.clear();
            }
            current_comments.push(line.to_string());
        } else {
            if !current_comments.is_empty() {
                segments.push(Segment::Comments(current_comments.clone()));
                current_comments.clear();
            }
            current_code.push(line.to_string());
        }
    }

    // Flush remaining
    if !current_comments.is_empty() {
        segments.push(Segment::Comments(current_comments));
    }
    if !current_code.is_empty() {
        segments.push(Segment::Code(current_code.join("\n")));
    }

    // Format code segments, keep comments as-is
    let mut result = String::new();
    for segment in &segments {
        match segment {
            Segment::Comments(lines) => {
                for line in lines {
                    result.push_str(line);
                    result.push('\n');
                }
            }
            Segment::Code(code) => {
                let trimmed_code = code.trim();
                if trimmed_code.is_empty() {
                    continue;
                }
                match syn::parse_file(trimmed_code) {
                    Ok(parsed) => {
                        let formatted = prettyplease::unparse(&parsed);
                        result.push_str(&formatted);
                    }
                    Err(_) => {
                        result.push_str(code);
                        result.push('\n');
                    }
                }
            }
        }
    }

    result
}

enum Segment {
    Comments(Vec<String>),
    Code(String),
}
