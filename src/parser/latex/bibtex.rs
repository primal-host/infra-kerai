/// BibTeX parser — parses .bib files into kerai.nodes using the biblatex crate.
use serde_json::{json, Value};
use uuid::Uuid;

use crate::parser::ast_walker::{EdgeRow, NodeRow};
use crate::parser::path_builder::PathContext;

use super::kinds;

/// Parse BibTeX source into NodeRow/EdgeRow vectors.
///
/// Each bibliography entry becomes a `bib_entry` node with structured metadata.
/// Entry fields are stored as child `bib_field` nodes.
pub fn parse_bibtex(
    source: &str,
    file_node_id: &str,
    instance_id: &str,
    path_ctx: &mut PathContext,
) -> (Vec<NodeRow>, Vec<EdgeRow>) {
    let mut nodes = Vec::new();
    let edges = Vec::new();

    let bibliography = match biblatex::Bibliography::parse(source) {
        Ok(bib) => bib,
        Err(e) => {
            pgrx::warning!("Failed to parse BibTeX: {:?}", e);
            return (nodes, edges);
        }
    };

    for (position, entry) in bibliography.iter().enumerate() {
        let cite_key = &entry.key;

        path_ctx.push(cite_key);

        // Build structured metadata for the entry
        let meta = entry_metadata(entry);

        let entry_id = Uuid::new_v4().to_string();
        nodes.push(NodeRow {
            id: entry_id.clone(),
            instance_id: instance_id.to_string(),
            kind: kinds::BIB_ENTRY.to_string(),
            language: Some("bibtex".to_string()),
            content: Some(cite_key.to_string()),
            parent_id: Some(file_node_id.to_string()),
            position: position as i32,
            path: path_ctx.path(),
            metadata: meta,
            span_start: None,
            span_end: None,
        });

        // Create child nodes for each field
        let fields = extract_fields(entry);
        for (field_pos, (field_name, field_value)) in fields.iter().enumerate() {
            let field_id = Uuid::new_v4().to_string();
            nodes.push(NodeRow {
                id: field_id,
                instance_id: instance_id.to_string(),
                kind: kinds::BIB_FIELD.to_string(),
                language: Some("bibtex".to_string()),
                content: Some(field_value.clone()),
                parent_id: Some(entry_id.clone()),
                position: field_pos as i32,
                path: None,
                metadata: json!({"field": field_name}),
                span_start: None,
                span_end: None,
            });
        }

        path_ctx.pop();
    }

    (nodes, edges)
}

/// Extract structured metadata from a bibliography entry.
fn entry_metadata(entry: &biblatex::Entry) -> Value {
    let mut meta = serde_json::Map::new();

    // Entry type
    meta.insert("entry_type".into(), json!(format!("{:?}", entry.entry_type)));

    // Authors
    if let Ok(authors) = entry.author() {
        let author_names: Vec<String> = authors
            .iter()
            .map(format_person)
            .collect();
        meta.insert("authors".into(), json!(author_names));
    }

    // Title
    if let Ok(title) = entry.title() {
        meta.insert("title".into(), json!(chunks_to_string(title)));
    }

    // Date/year
    if let Ok(date) = entry.date() {
        match date {
            biblatex::PermissiveType::Typed(d) => {
                let year = match &d.value {
                        biblatex::DateValue::At(dt) | biblatex::DateValue::After(dt) | biblatex::DateValue::Before(dt) => dt.year,
                        biblatex::DateValue::Between(dt, _) => dt.year,
                    };
                    meta.insert("year".into(), json!(year));
            }
            biblatex::PermissiveType::Chunks(chunks) => {
                meta.insert("year_raw".into(), json!(owned_chunks_to_string(&chunks)));
            }
        }
    }

    // Journal
    if let Ok(journal) = entry.journal() {
        meta.insert("journal".into(), json!(chunks_to_string(journal)));
    }

    // DOI
    if let Ok(doi) = entry.doi() {
        meta.insert("doi".into(), json!(doi));
    }

    // URL
    if let Ok(url) = entry.url() {
        meta.insert("url".into(), json!(url));
    }

    // Publisher
    if let Ok(publishers) = entry.publisher() {
        let pub_names: Vec<String> = publishers
            .iter()
            .map(|chunks| owned_chunks_to_string(chunks))
            .collect();
        if !pub_names.is_empty() {
            meta.insert("publisher".into(), json!(pub_names.join("; ")));
        }
    }

    // ISBN
    if let Ok(isbn) = entry.isbn() {
        meta.insert("isbn".into(), json!(chunks_to_string(isbn)));
    }

    // Volume
    if let Ok(vol) = entry.volume() {
        match vol {
            biblatex::PermissiveType::Typed(v) => {
                meta.insert("volume".into(), json!(v));
            }
            biblatex::PermissiveType::Chunks(chunks) => {
                meta.insert("volume".into(), json!(owned_chunks_to_string(&chunks)));
            }
        }
    }

    // Editors
    if let Ok(editors) = entry.editors() {
        let editor_names: Vec<String> = editors
            .into_iter()
            .flat_map(|(persons, _editor_type)| {
                persons.into_iter().map(|p| format_person(&p))
            })
            .collect();
        if !editor_names.is_empty() {
            meta.insert("editors".into(), json!(editor_names));
        }
    }

    Value::Object(meta)
}

/// Extract all fields from an entry as (name, value) pairs.
fn extract_fields(entry: &biblatex::Entry) -> Vec<(String, String)> {
    let mut fields = Vec::new();

    if let Ok(title) = entry.title() {
        fields.push(("title".into(), chunks_to_string(title)));
    }

    if let Ok(authors) = entry.author() {
        let names: Vec<String> = authors.iter().map(format_person).collect();
        fields.push(("author".into(), names.join(" and ")));
    }

    if let Ok(date) = entry.date() {
        match date {
            biblatex::PermissiveType::Typed(d) => {
                let year = match &d.value {
                    biblatex::DateValue::At(dt)
                    | biblatex::DateValue::After(dt)
                    | biblatex::DateValue::Before(dt) => dt.year,
                    biblatex::DateValue::Between(dt, _) => dt.year,
                };
                fields.push(("year".into(), year.to_string()));
            }
            biblatex::PermissiveType::Chunks(chunks) => {
                fields.push(("year".into(), owned_chunks_to_string(&chunks)));
            }
        }
    }

    if let Ok(journal) = entry.journal() {
        fields.push(("journal".into(), chunks_to_string(journal)));
    }

    if let Ok(doi) = entry.doi() {
        fields.push(("doi".into(), doi));
    }

    if let Ok(url) = entry.url() {
        fields.push(("url".into(), url));
    }

    if let Ok(isbn) = entry.isbn() {
        fields.push(("isbn".into(), chunks_to_string(isbn)));
    }

    fields
}

/// Format a Person as "Given Prefix Name Suffix", trimmed.
fn format_person(p: &biblatex::Person) -> String {
    let mut parts = Vec::new();
    if !p.given_name.is_empty() {
        parts.push(p.given_name.as_str());
    }
    if !p.prefix.is_empty() {
        parts.push(p.prefix.as_str());
    }
    if !p.name.is_empty() {
        parts.push(p.name.as_str());
    }
    if !p.suffix.is_empty() {
        parts.push(p.suffix.as_str());
    }
    parts.join(" ")
}

/// Convert a ChunksRef (slice of Spanned<Chunk>) to a plain string.
fn chunks_to_string(chunks: &[biblatex::Spanned<biblatex::Chunk>]) -> String {
    chunks
        .iter()
        .map(|spanned| spanned.v.get())
        .collect::<Vec<_>>()
        .join("")
}

/// Convert owned Chunks (Vec<Spanned<Chunk>>) to a plain string.
fn owned_chunks_to_string(chunks: &[biblatex::Spanned<biblatex::Chunk>]) -> String {
    chunks_to_string(chunks)
}
