/// Crawler module — autonomous corpus building via citation graph traversal.
///
/// Extracts references from ingested documents, creates reference nodes and
/// citation edges, enabling downstream crawl-and-ingest of the citation frontier.
use pgrx::prelude::*;
use regex::Regex;
use serde_json::json;
use std::collections::HashMap;

use crate::parser::kinds::Kind;
use crate::sql::{sql_jsonb, sql_text, sql_uuid};

/// A reference extracted from document text.
#[derive(Debug, Clone)]
struct ExtractedRef {
    /// Canonical key for deduplication (e.g. "friston-2010" or DOI).
    #[allow(dead_code)]
    key: String,
    /// Display content for the reference node.
    content: String,
    /// Reference type: "bibliography", "citation", "doi", "arxiv", "url".
    ref_type: String,
    /// Structured metadata.
    metadata: serde_json::Value,
    /// Paragraph node IDs that cite this reference.
    citing_paragraphs: Vec<String>,
}

/// Extract references from all documents, or a specific document.
///
/// Scans paragraph nodes for bibliography entries, inline author-year citations,
/// DOIs, arXiv IDs, and URLs. Creates `reference` nodes and `cites` edges.
/// Deduplicates by canonical key (DOI > author-year > URL).
///
/// Returns a JSON summary of what was found.
#[pg_extern]
fn extract_references(document_filter: default!(&str, "'__all__'")) -> pgrx::JsonB {
    let instance_id = crate::parser::get_self_instance_id();

    // Query paragraph nodes from matching documents
    let doc_clause = if document_filter == "__all__" {
        String::new()
    } else {
        format!(
            " AND doc.content = {}",
            sql_text(document_filter)
        )
    };

    let query = format!(
        "SELECT p.id::text, p.content, doc.id::text as doc_id, doc.content as doc_name \
         FROM kerai.nodes p \
         JOIN kerai.nodes heading ON heading.id = p.parent_id \
         JOIN kerai.nodes doc ON doc.id = heading.parent_id \
         WHERE p.kind = 'paragraph' \
         AND doc.kind = 'document' \
         AND length(p.content) > 50 \
         {}
         ORDER BY doc.content, p.position",
        doc_clause
    );

    let mut refs: HashMap<String, ExtractedRef> = HashMap::new();
    let mut total_paragraphs = 0u64;

    Spi::connect(|client| {
        let result = client.select(&query, None, &[]).unwrap();
        for row in result {
            let para_id: String = row.get_by_name("id").unwrap().unwrap_or_default();
            let content: String = row.get_by_name("content").unwrap().unwrap_or_default();
            total_paragraphs += 1;

            // Extract bibliography entries
            extract_bibliography(&content, &para_id, &mut refs);

            // Extract inline citations
            extract_inline_citations(&content, &para_id, &mut refs);

            // Extract DOIs
            extract_dois(&content, &para_id, &mut refs);

            // Extract arXiv IDs
            extract_arxiv(&content, &para_id, &mut refs);

            // Extract URLs
            extract_urls(&content, &para_id, &mut refs);
        }
    });

    // Check which references already exist (idempotent)
    let existing = get_existing_reference_keys(&instance_id);

    // Insert new reference nodes and citation edges
    let mut new_refs = 0u64;
    let mut new_edges = 0u64;
    let mut skipped = 0u64;

    for (key, ext_ref) in &refs {
        if existing.contains_key(key) {
            // Reference exists — just add any new citation edges
            let ref_id = &existing[key];
            new_edges += insert_citation_edges(ref_id, &ext_ref.citing_paragraphs);
            skipped += 1;
            continue;
        }

        let ref_id = uuid::Uuid::new_v4().to_string();

        let meta = json!({
            "ref_type": ext_ref.ref_type,
            "status": "unresolved",
            "cite_count": ext_ref.citing_paragraphs.len(),
            "key": key,
            "details": ext_ref.metadata,
        });

        let insert_sql = format!(
            "INSERT INTO kerai.nodes (id, instance_id, kind, content, metadata, position) \
             VALUES ({}, {}, {}, {}, {}, 0)",
            sql_uuid(&ref_id),
            sql_uuid(&instance_id),
            sql_text(Kind::Reference.as_str()),
            sql_text(&ext_ref.content),
            sql_jsonb(&meta),
        );

        Spi::run(&insert_sql).ok();
        new_refs += 1;

        new_edges += insert_citation_edges(&ref_id, &ext_ref.citing_paragraphs);
    }

    pgrx::JsonB(json!({
        "paragraphs_scanned": total_paragraphs,
        "references_found": refs.len(),
        "new_references": new_refs,
        "new_edges": new_edges,
        "skipped_existing": skipped,
    }))
}

/// Extract bibliography entries: `Author, F. N. (YYYY). "Title." Journal ...`
fn extract_bibliography(text: &str, para_id: &str, refs: &mut HashMap<String, ExtractedRef>) {
    // Bibliography paragraphs contain multiple entries concatenated.
    // Pattern: starts with author surname, contains (year) and quoted title.
    let re = Regex::new(
        r#"([A-Z][a-z\x{00AD}\x{00C0}-\x{024F}]+(?:,\s*(?:[A-Z]\.\s*)+(?:and\s+)?)*(?:,?\s*(?:et\s+al\.))?)\s*\((\d{4}[a-z]?)\)\.\s*["\x{201C}]([^"\x{201D}]+)["\x{201D}]"#
    ).unwrap();

    for cap in re.captures_iter(text) {
        let authors = cap[1].trim().to_string();
        let year = cap[2].to_string();
        let title = cap[3].trim().to_string();

        // Extract DOI if present in this entry
        let doi = extract_doi_from_segment(text, cap.get(0).unwrap().start());

        let key = if let Some(ref d) = doi {
            d.clone()
        } else {
            make_author_year_key(&authors, &year)
        };

        let content = format!("{} ({}). \"{}\"", authors, year, title);

        let entry = refs.entry(key.clone()).or_insert_with(|| ExtractedRef {
            key,
            content,
            ref_type: "bibliography".to_string(),
            metadata: json!({
                "authors": authors,
                "year": year,
                "title": title,
                "doi": doi,
            }),
            citing_paragraphs: Vec::new(),
        });

        if !entry.citing_paragraphs.contains(&para_id.to_string()) {
            entry.citing_paragraphs.push(para_id.to_string());
        }
    }
}

/// Extract inline citations: `(Author YYYY)`, `(Author & Author, YYYY)`, `(Author et al. YYYY)`
fn extract_inline_citations(
    text: &str,
    para_id: &str,
    refs: &mut HashMap<String, ExtractedRef>,
) {
    // Match patterns like (Friston 2010), (Parr & Friston, 2019), (Da Costa et al., 2020)
    // Also handles (Friston, FitzGerald et al. 2016)
    let re = Regex::new(
        r"\(([A-Z][a-z\x{00AD}\x{00C0}-\x{024F}]+(?:(?:,?\s*(?:and|&)\s*)?[A-Z][a-z\x{00AD}\x{00C0}-\x{024F}]+)*(?:,?\s*et\s+al\.?)?),?\s*(\d{4}[a-z]?)\)"
    ).unwrap();

    for cap in re.captures_iter(text) {
        let authors = cap[1].trim().to_string();
        let year = cap[2].to_string();
        let key = make_author_year_key(&authors, &year);

        let entry = refs.entry(key.clone()).or_insert_with(|| ExtractedRef {
            key,
            content: format!("{} ({})", authors, year),
            ref_type: "citation".to_string(),
            metadata: json!({
                "authors": authors,
                "year": year,
            }),
            citing_paragraphs: Vec::new(),
        });

        if !entry.citing_paragraphs.contains(&para_id.to_string()) {
            entry.citing_paragraphs.push(para_id.to_string());
        }
    }
}

/// Extract DOI references.
fn extract_dois(text: &str, para_id: &str, refs: &mut HashMap<String, ExtractedRef>) {
    let re = Regex::new(r"doi:?(10\.\d{4,}/[^\s,;]+)").unwrap();

    for cap in re.captures_iter(text) {
        let doi = cap[1].trim_end_matches('.').to_string();
        let key = doi.clone();

        let entry = refs.entry(key.clone()).or_insert_with(|| ExtractedRef {
            key,
            content: format!("doi:{}", doi),
            ref_type: "doi".to_string(),
            metadata: json!({ "doi": doi }),
            citing_paragraphs: Vec::new(),
        });

        if !entry.citing_paragraphs.contains(&para_id.to_string()) {
            entry.citing_paragraphs.push(para_id.to_string());
        }
    }
}

/// Extract arXiv IDs.
fn extract_arxiv(text: &str, para_id: &str, refs: &mut HashMap<String, ExtractedRef>) {
    let re = Regex::new(r"arXiv[:\s]+([\d.]+(?:v\d+)?(?:\s*\[[a-z.,\s]+\])?)").unwrap();

    for cap in re.captures_iter(text) {
        let arxiv_id = cap[1].trim().to_string();
        // Clean up: take just the numeric ID part
        let clean_id = arxiv_id
            .split_whitespace()
            .next()
            .unwrap_or(&arxiv_id)
            .trim_end_matches('.')
            .to_string();
        let key = format!("arxiv:{}", clean_id);

        let entry = refs.entry(key.clone()).or_insert_with(|| ExtractedRef {
            key,
            content: format!("arXiv:{}", clean_id),
            ref_type: "arxiv".to_string(),
            metadata: json!({ "arxiv_id": clean_id }),
            citing_paragraphs: Vec::new(),
        });

        if !entry.citing_paragraphs.contains(&para_id.to_string()) {
            entry.citing_paragraphs.push(para_id.to_string());
        }
    }
}

/// Extract URLs.
fn extract_urls(text: &str, para_id: &str, refs: &mut HashMap<String, ExtractedRef>) {
    let re = Regex::new(r"https?://[^\s,;)\]]+").unwrap();

    for mat in re.find_iter(text) {
        let url = mat.as_str().trim_end_matches('.').to_string();
        let key = url.clone();

        let entry = refs.entry(key.clone()).or_insert_with(|| ExtractedRef {
            key,
            content: url.clone(),
            ref_type: "url".to_string(),
            metadata: json!({ "url": url }),
            citing_paragraphs: Vec::new(),
        });

        if !entry.citing_paragraphs.contains(&para_id.to_string()) {
            entry.citing_paragraphs.push(para_id.to_string());
        }
    }
}

/// Try to find a DOI near a position in the text (for bibliography entries).
fn extract_doi_from_segment(text: &str, start: usize) -> Option<String> {
    // Look ahead up to 500 chars from the match start for a doi.
    // Must find a valid char boundary to avoid panicking on multi-byte UTF-8.
    let end = {
        let target = (start + 500).min(text.len());
        // Walk backward to find a char boundary
        let mut pos = target;
        while pos > start && !text.is_char_boundary(pos) {
            pos -= 1;
        }
        pos
    };
    if end <= start {
        return None;
    }
    let segment = &text[start..end];
    let re = Regex::new(r"doi:?(10\.\d{4,}/[^\s,;]+)").unwrap();
    re.captures(segment)
        .map(|c| c[1].trim_end_matches('.').to_string())
}

/// Create a canonical author-year key for deduplication.
fn make_author_year_key(authors: &str, year: &str) -> String {
    // Extract first surname, lowercase, strip diacritics/hyphens
    let first_author = authors
        .split(',')
        .next()
        .unwrap_or(authors)
        .split_whitespace()
        .next()
        .unwrap_or(authors)
        .to_lowercase()
        .replace('\u{00AD}', "") // soft hyphen
        .replace('\u{00A0}', "") // nbsp
        .replace('-', "");

    format!("{}-{}", first_author, year)
}

/// Get existing reference nodes by key to avoid duplicates.
fn get_existing_reference_keys(instance_id: &str) -> HashMap<String, String> {
    let mut existing = HashMap::new();

    Spi::connect(|client| {
        let query = format!(
            "SELECT id::text, metadata->>'key' as key \
             FROM kerai.nodes \
             WHERE instance_id = {} AND kind = 'reference' \
             AND metadata->>'key' IS NOT NULL",
            sql_uuid(instance_id)
        );
        let result = client.select(&query, None, &[]).unwrap();
        for row in result {
            let id: String = row.get_by_name("id").unwrap().unwrap_or_default();
            let key: String = row.get_by_name("key").unwrap().unwrap_or_default();
            if !key.is_empty() {
                existing.insert(key, id);
            }
        }
    });

    existing
}

/// Score a single reference by expected free energy.
///
/// Components (weights sum to 1.0):
/// - Citation frequency: `ln(1 + cite_count)`, 40% weight
/// - Source diversity: `ln(1 + unique_doc_count)`, 30% weight
/// - Author novelty: 1.0 if new author, 0.1 if already resolved, 20% weight
/// - Resolvability: type-based likelihood of successful fetch, 10% weight
///
/// Higher scores = higher priority for the crawler to chase.
#[pg_extern]
fn score_reference(ref_id: &str) -> f64 {
    let query = format!(
        "SELECT \
            (SELECT count(*) FROM kerai.edges \
             WHERE target_id = r.id AND relation = 'cites') as cite_count, \
            (SELECT count(DISTINCT doc.id) \
             FROM kerai.edges e \
             JOIN kerai.nodes p ON e.source_id = p.id \
             JOIN kerai.nodes h ON h.id = p.parent_id \
             JOIN kerai.nodes doc ON doc.id = h.parent_id AND doc.kind = 'document' \
             WHERE e.target_id = r.id AND e.relation = 'cites') as doc_count, \
            r.metadata->>'ref_type' as ref_type, \
            r.metadata->'details'->>'authors' as authors \
         FROM kerai.nodes r WHERE r.id = {}",
        sql_uuid(ref_id)
    );

    let mut cite_count = 0i64;
    let mut doc_count = 0i64;
    let mut ref_type = String::new();
    let mut authors = String::new();

    Spi::connect(|client| {
        let result = client.select(&query, None, &[]).unwrap();
        if let Some(row) = result.into_iter().next() {
            cite_count = row.get_by_name("cite_count").unwrap().unwrap_or(0);
            doc_count = row.get_by_name("doc_count").unwrap().unwrap_or(0);
            ref_type = row.get_by_name("ref_type").unwrap().unwrap_or_default();
            authors = row.get_by_name("authors").unwrap().unwrap_or_default();
        }
    });

    let novelty = author_novelty(&authors);
    efe_score(cite_count, doc_count, &ref_type, novelty)
}

/// Score all unresolved references and return a ranked JSON list.
///
/// Efficient batch version — gathers all data in two queries (reference stats +
/// resolved authors) and computes scores in Rust without per-row SPI overhead.
///
/// Returns: `{ total, references: [{ ref_id, content, ref_type, key, cite_count, doc_count, novelty, score }] }`
#[pg_extern]
fn score_references() -> pgrx::JsonB {
    let query = "\
        SELECT r.id::text as ref_id, r.content, \
            r.metadata->>'ref_type' as ref_type, \
            r.metadata->'details'->>'authors' as authors, \
            r.metadata->>'key' as ref_key, \
            (SELECT count(*) FROM kerai.edges \
             WHERE target_id = r.id AND relation = 'cites') as cite_count, \
            (SELECT count(DISTINCT doc.id) \
             FROM kerai.edges e \
             JOIN kerai.nodes p ON e.source_id = p.id \
             JOIN kerai.nodes h ON h.id = p.parent_id \
             JOIN kerai.nodes doc ON doc.id = h.parent_id AND doc.kind = 'document' \
             WHERE e.target_id = r.id AND e.relation = 'cites') as doc_count \
        FROM kerai.nodes r \
        WHERE r.kind = 'reference' AND r.metadata->>'status' = 'unresolved' \
        ORDER BY r.created_at";

    // Pre-fetch resolved authors for batch novelty calculation
    let known_authors = get_resolved_authors();

    let mut scored: Vec<serde_json::Value> = Vec::new();

    Spi::connect(|client| {
        let result = client.select(query, None, &[]).unwrap();
        for row in result {
            let ref_id: String = row.get_by_name("ref_id").unwrap().unwrap_or_default();
            let content: String = row.get_by_name("content").unwrap().unwrap_or_default();
            let ref_type: String = row.get_by_name("ref_type").unwrap().unwrap_or_default();
            let authors: String = row.get_by_name("authors").unwrap().unwrap_or_default();
            let ref_key: String = row.get_by_name("ref_key").unwrap().unwrap_or_default();
            let cite_count: i64 = row.get_by_name("cite_count").unwrap().unwrap_or(0);
            let doc_count: i64 = row.get_by_name("doc_count").unwrap().unwrap_or(0);

            let novelty = author_novelty_cached(&authors, &known_authors);
            let score = efe_score(cite_count, doc_count, &ref_type, novelty);

            scored.push(json!({
                "ref_id": ref_id,
                "content": content,
                "ref_type": ref_type,
                "key": ref_key,
                "cite_count": cite_count,
                "doc_count": doc_count,
                "novelty": novelty,
                "score": (score * 1000.0).round() / 1000.0,
            }));
        }
    });

    // Sort by score descending
    scored.sort_by(|a, b| {
        let sa = a["score"].as_f64().unwrap_or(0.0);
        let sb = b["score"].as_f64().unwrap_or(0.0);
        sb.partial_cmp(&sa).unwrap_or(std::cmp::Ordering::Equal)
    });

    pgrx::JsonB(json!({
        "total": scored.len(),
        "references": scored,
    }))
}

/// Compute expected free energy score from components.
fn efe_score(cite_count: i64, doc_count: i64, ref_type: &str, novelty: f64) -> f64 {
    // Epistemic: citation frequency (log-scaled)
    let w_citations = (1.0 + cite_count as f64).ln() * 0.4;

    // Epistemic: source diversity (log-scaled unique document count)
    let w_diversity = (1.0 + doc_count as f64).ln() * 0.3;

    // Epistemic: author novelty
    let w_novelty = novelty * 0.2;

    // Pragmatic: resolvability by reference type
    let resolvability = match ref_type {
        "doi" => 1.0,
        "arxiv" => 0.9,
        "url" => 0.8,
        "bibliography" => 0.6,
        "citation" => 0.4,
        _ => 0.2,
    };
    let w_resolvability = resolvability * 0.1;

    w_citations + w_diversity + w_novelty + w_resolvability
}

/// Compute author novelty via SPI (for single-reference scoring).
fn author_novelty(authors: &str) -> f64 {
    let key = first_author_key(authors);
    if key.is_empty() {
        return 0.5; // unknown — neutral score
    }

    let count = Spi::get_one::<i64>(&format!(
        "SELECT count(*) FROM kerai.nodes \
         WHERE kind = 'reference' AND metadata->>'status' = 'resolved' \
         AND lower(metadata->'details'->>'authors') LIKE {}",
        sql_text(&format!("%{}%", key))
    ))
    .unwrap_or(Some(0))
    .unwrap_or(0);

    if count == 0 { 1.0 } else { 0.1 }
}

/// Compute author novelty using pre-fetched resolved author set (for batch scoring).
fn author_novelty_cached(authors: &str, known_authors: &[String]) -> f64 {
    let key = first_author_key(authors);
    if key.is_empty() {
        return 0.5;
    }
    if known_authors.iter().any(|a| a.contains(&key)) {
        0.1
    } else {
        1.0
    }
}

/// Get all resolved reference author strings for batch novelty check.
fn get_resolved_authors() -> Vec<String> {
    let mut authors = Vec::new();
    Spi::connect(|client| {
        let result = client
            .select(
                "SELECT lower(metadata->'details'->>'authors') as authors \
                 FROM kerai.nodes \
                 WHERE kind = 'reference' AND metadata->>'status' = 'resolved' \
                 AND metadata->'details'->>'authors' IS NOT NULL",
                None,
                &[],
            )
            .unwrap();
        for row in result {
            let a: Option<String> = row.get_by_name("authors").unwrap();
            if let Some(a) = a {
                authors.push(a);
            }
        }
    });
    authors
}

/// Extract normalized first-author key for novelty comparison.
fn first_author_key(authors: &str) -> String {
    authors
        .split(',')
        .next()
        .unwrap_or("")
        .trim()
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_lowercase()
        .replace('\u{00AD}', "")
        .replace('\u{00A0}', "")
        .replace('-', "")
}

/// Insert citation edges from paragraphs to a reference node.
/// Returns the number of new edges created.
fn insert_citation_edges(ref_id: &str, para_ids: &[String]) -> u64 {
    let mut count = 0u64;

    for para_id in para_ids {
        // Check if edge already exists
        let exists_sql = format!(
            "SELECT 1 FROM kerai.edges \
             WHERE source_id = {} AND target_id = {} AND relation = 'cites'",
            sql_uuid(para_id),
            sql_uuid(ref_id)
        );

        let exists = Spi::get_one::<i32>(&exists_sql)
            .unwrap_or(None)
            .is_some();

        if !exists {
            let edge_id = uuid::Uuid::new_v4().to_string();
            let insert_sql = format!(
                "INSERT INTO kerai.edges (id, source_id, target_id, relation, metadata) \
                 VALUES ({}, {}, {}, 'cites', '{{}}'::jsonb)",
                sql_uuid(&edge_id),
                sql_uuid(para_id),
                sql_uuid(ref_id),
            );
            Spi::run(&insert_sql).ok();
            count += 1;
        }
    }

    count
}
