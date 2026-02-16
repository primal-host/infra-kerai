/// Markdown walker — pulldown-cmark events → NodeRow/EdgeRow.
use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd, CodeBlockKind};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::parser::ast_walker::{EdgeRow, NodeRow};
use crate::parser::path_builder::{PathContext, sanitize_label};
use super::kinds;

/// Stack entry tracking open container elements.
struct StackEntry {
    node_id: String,
    kind: String,
    heading_level: Option<u8>,
}

/// Walk markdown source and produce NodeRow/EdgeRow vectors.
pub fn walk_markdown(
    source: &str,
    filename: &str,
    instance_id: &str,
    document_node_id: &str,
) -> (Vec<NodeRow>, Vec<EdgeRow>) {
    let opts = Options::ENABLE_TABLES
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_HEADING_ATTRIBUTES;
    let parser = Parser::new_ext(source, opts);

    let mut nodes: Vec<NodeRow> = Vec::new();
    let mut edges: Vec<EdgeRow> = Vec::new();
    let mut path_ctx = PathContext::with_root(&sanitize_label(filename));

    // Stack for container elements (headings, blockquotes, lists, etc.)
    let mut stack: Vec<StackEntry> = Vec::new();

    // Heading hierarchy stack — tracks the current section nesting.
    // Each entry: (heading_level, node_id)
    let mut heading_stack: Vec<(u8, String)> = Vec::new();

    let mut position: i32 = 0;
    let mut text_accum = String::new();
    let mut current_link: Option<(String, String)> = None; // (url, title)

    for event in parser {
        match event {
            Event::Start(tag) => {
                let (kind, metadata, name) = tag_to_kind_meta(&tag);

                let node_id = Uuid::new_v4().to_string();

                // Determine parent based on heading hierarchy or stack
                let parent_id = if kind == kinds::HEADING {
                    let level = heading_level_from_tag(&tag);
                    // Pop heading stack to find appropriate parent
                    while let Some((hl, _)) = heading_stack.last() {
                        if *hl >= level {
                            heading_stack.pop();
                            path_ctx.pop();
                        } else {
                            break;
                        }
                    }
                    let parent = heading_stack.last()
                        .map(|(_, id)| id.clone())
                        .unwrap_or_else(|| document_node_id.to_string());

                    heading_stack.push((level, node_id.clone()));
                    if let Some(n) = &name {
                        path_ctx.push(n);
                    } else {
                        path_ctx.push(&format!("h{}", level));
                    }

                    parent
                } else {
                    // Non-heading containers: parent is the innermost heading or stack item
                    stack.last()
                        .map(|e| e.node_id.clone())
                        .or_else(|| heading_stack.last().map(|(_, id)| id.clone()))
                        .unwrap_or_else(|| document_node_id.to_string())
                };

                if let Tag::Link { dest_url, title, .. } = &tag {
                    current_link = Some((dest_url.to_string(), title.to_string()));
                }
                if let Tag::Image { dest_url, title, .. } = &tag {
                    current_link = Some((dest_url.to_string(), title.to_string()));
                }

                nodes.push(NodeRow {
                    id: node_id.clone(),
                    instance_id: instance_id.to_string(),
                    kind: kind.to_string(),
                    language: Some("markdown".to_string()),
                    content: None, // filled on End or text
                    parent_id: Some(parent_id),
                    position,
                    path: path_ctx.path(),
                    metadata,
                    span_start: None,
                    span_end: None,
                });
                position += 1;

                stack.push(StackEntry {
                    node_id,
                    kind: kind.to_string(),
                    heading_level: if kind == kinds::HEADING {
                        Some(heading_level_from_tag(&tag))
                    } else {
                        None
                    },
                });

                text_accum.clear();
            }

            Event::End(tag_end) => {
                if let Some(entry) = stack.pop() {
                    // Set content from accumulated text
                    if !text_accum.is_empty() {
                        if let Some(node) = nodes.iter_mut().rev().find(|n| n.id == entry.node_id) {
                            node.content = Some(text_accum.clone());

                            // For links/images, add URL to metadata
                            if entry.kind == kinds::LINK || entry.kind == kinds::IMAGE {
                                if let Some((url, title)) = current_link.take() {
                                    let mut meta = node.metadata.clone();
                                    if let Value::Object(ref mut map) = meta {
                                        map.insert("url".to_string(), json!(url));
                                        if !title.is_empty() {
                                            map.insert("title".to_string(), json!(title));
                                        }
                                    }
                                    node.metadata = meta;
                                }
                            }
                        }
                    }

                    // Create edges for links to track cross-references
                    if entry.kind == kinds::LINK {
                        if let Some(node) = nodes.iter().rev().find(|n| n.id == entry.node_id) {
                            if let Some(url) = node.metadata.get("url").and_then(|v| v.as_str()) {
                                // If it's an internal link (no scheme), try to create edge
                                if !url.contains("://") && !url.starts_with('#') {
                                    edges.push(EdgeRow {
                                        id: Uuid::new_v4().to_string(),
                                        source_id: entry.node_id.clone(),
                                        target_id: document_node_id.to_string(),
                                        relation: "links_to".to_string(),
                                        metadata: json!({"url": url}),
                                    });
                                }
                            }
                        }
                    }

                    // Pop path context for non-heading containers
                    if entry.heading_level.is_none() {
                        // Only pop if we pushed for non-heading container types
                        match tag_end {
                            TagEnd::BlockQuote(_) | TagEnd::List(_) | TagEnd::Item
                            | TagEnd::Table | TagEnd::TableHead | TagEnd::TableRow
                            | TagEnd::TableCell | TagEnd::FootnoteDefinition => {}
                            _ => {}
                        }
                    }
                }
                text_accum.clear();
            }

            Event::Text(text) => {
                text_accum.push_str(&text);
            }

            Event::Code(code) => {
                // Inline code — add as text to parent
                text_accum.push('`');
                text_accum.push_str(&code);
                text_accum.push('`');
            }

            Event::SoftBreak => {
                text_accum.push(' ');
            }

            Event::HardBreak => {
                text_accum.push('\n');
            }

            Event::Html(html) => {
                // Raw HTML block
                let parent_id = stack.last()
                    .map(|e| e.node_id.clone())
                    .or_else(|| heading_stack.last().map(|(_, id)| id.clone()))
                    .unwrap_or_else(|| document_node_id.to_string());

                nodes.push(NodeRow {
                    id: Uuid::new_v4().to_string(),
                    instance_id: instance_id.to_string(),
                    kind: kinds::HTML_BLOCK.to_string(),
                    language: Some("markdown".to_string()),
                    content: Some(html.to_string()),
                    parent_id: Some(parent_id),
                    position,
                    path: path_ctx.path(),
                    metadata: json!({}),
                    span_start: None,
                    span_end: None,
                });
                position += 1;
            }

            Event::InlineHtml(html) => {
                text_accum.push_str(&html);
            }

            Event::FootnoteReference(name) => {
                text_accum.push_str(&format!("[^{}]", name));
            }

            Event::TaskListMarker(checked) => {
                // Update the current list_item's metadata
                if let Some(entry) = stack.last() {
                    if let Some(node) = nodes.iter_mut().rev().find(|n| n.id == entry.node_id) {
                        if let Value::Object(ref mut map) = node.metadata {
                            map.insert("task".to_string(), json!(checked));
                        }
                    }
                }
            }

            _ => {}
        }
    }

    (nodes, edges)
}

/// Extract kind, metadata, and optional name from a pulldown-cmark Tag.
fn tag_to_kind_meta<'a>(tag: &'a Tag<'a>) -> (&'a str, Value, Option<String>) {
    match tag {
        Tag::Heading { level, id, .. } => {
            let lvl = heading_level_num(level);
            let mut meta = json!({"level": lvl});
            if let Some(id_str) = id {
                if let Value::Object(ref mut map) = meta {
                    map.insert("id".to_string(), json!(id_str.to_string()));
                }
            }
            (kinds::HEADING, meta, None)
        }
        Tag::Paragraph => (kinds::PARAGRAPH, json!({}), None),
        Tag::BlockQuote(_) => (kinds::BLOCKQUOTE, json!({}), None),
        Tag::List(first_item) => {
            let ordered = first_item.is_some();
            let mut meta = json!({"ordered": ordered});
            if let Some(start) = first_item {
                if let Value::Object(ref mut map) = meta {
                    map.insert("start".to_string(), json!(start));
                }
            }
            (kinds::LIST, meta, None)
        }
        Tag::Item => (kinds::LIST_ITEM, json!({}), None),
        Tag::CodeBlock(cb_kind) => {
            let meta = match cb_kind {
                CodeBlockKind::Fenced(lang) if !lang.is_empty() => {
                    json!({"language": lang.to_string()})
                }
                CodeBlockKind::Fenced(_) => json!({"language": ""}),
                CodeBlockKind::Indented => json!({"indented": true}),
            };
            (kinds::CODE_BLOCK, meta, None)
        }
        Tag::Table(alignments) => {
            let aligns: Vec<&str> = alignments.iter().map(|a| match a {
                pulldown_cmark::Alignment::None => "none",
                pulldown_cmark::Alignment::Left => "left",
                pulldown_cmark::Alignment::Center => "center",
                pulldown_cmark::Alignment::Right => "right",
            }).collect();
            (kinds::TABLE, json!({"alignments": aligns}), None)
        }
        Tag::TableHead => (kinds::TABLE_HEAD, json!({}), None),
        Tag::TableRow => (kinds::TABLE_ROW, json!({}), None),
        Tag::TableCell => (kinds::TABLE_CELL, json!({}), None),
        Tag::Emphasis => (kinds::EMPHASIS, json!({}), None),
        Tag::Strong => (kinds::STRONG, json!({}), None),
        Tag::Strikethrough => (kinds::STRIKETHROUGH, json!({}), None),
        Tag::Link { dest_url, title, .. } => {
            let meta = json!({
                "url": dest_url.to_string(),
                "title": title.to_string(),
            });
            (kinds::LINK, meta, None)
        }
        Tag::Image { dest_url, title, .. } => {
            let meta = json!({
                "url": dest_url.to_string(),
                "title": title.to_string(),
            });
            (kinds::IMAGE, meta, None)
        }
        Tag::FootnoteDefinition(name) => {
            (kinds::FOOTNOTE, json!({"name": name.to_string()}), Some(name.to_string()))
        }
        Tag::HtmlBlock => (kinds::HTML_BLOCK, json!({}), None),
        _ => (kinds::PARAGRAPH, json!({}), None),
    }
}

fn heading_level_from_tag(tag: &Tag) -> u8 {
    match tag {
        Tag::Heading { level, .. } => heading_level_num(level),
        _ => 0,
    }
}

fn heading_level_num(level: &HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}
