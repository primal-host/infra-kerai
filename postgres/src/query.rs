/// Query & Navigation — find, refs, tree, children, ancestors, search.
use pgrx::prelude::*;
use serde_json::json;

use crate::sql::sql_escape;

/// Search nodes by content pattern (ILIKE) with optional kind filter and limit.
///
/// Returns JSON array of `{id, kind, content, path, parent_id, metadata}`.
#[pg_extern]
fn find(pattern: &str, kind_filter: Option<&str>, limit: Option<i32>) -> pgrx::JsonB {
    let limit_val = limit.unwrap_or(50).max(1).min(1000);
    let escaped_pattern = sql_escape(pattern);

    let kind_clause = match kind_filter {
        Some(k) => format!("AND kind = '{}'", sql_escape(k)),
        None => String::new(),
    };

    let sql = format!(
        "SELECT COALESCE(jsonb_agg(r), '[]'::jsonb) FROM (
            SELECT jsonb_build_object(
                'id', id,
                'kind', kind,
                'content', content,
                'path', path::text,
                'parent_id', parent_id,
                'metadata', metadata
            ) AS r
            FROM kerai.nodes
            WHERE content ILIKE '{}' {}
            ORDER BY kind, content
            LIMIT {}
        ) sub",
        escaped_pattern, kind_clause, limit_val,
    );

    Spi::get_one::<pgrx::JsonB>(&sql)
        .unwrap()
        .unwrap_or_else(|| pgrx::JsonB(serde_json::json!([])))
}

/// Find all definitions, references, and impl blocks for a symbol.
///
/// Returns `{symbol, definitions: [...], references: [...], impls: [...]}`.
#[pg_extern]
fn refs(symbol: &str) -> pgrx::JsonB {
    let escaped = sql_escape(symbol);

    // Definitions: top-level defining kinds
    let defs_sql = format!(
        "SELECT COALESCE(jsonb_agg(jsonb_build_object(
            'id', id,
            'kind', kind,
            'content', content,
            'path', path::text,
            'metadata', metadata
        ) ORDER BY kind, path::text), '[]'::jsonb)
        FROM kerai.nodes
        WHERE content = '{}' AND kind IN (
            'fn', 'struct', 'enum', 'trait', 'const', 'static',
            'type_alias', 'union', 'macro_def', 'variant', 'field'
        )",
        escaped,
    );

    // References: usage kinds with parent context
    let refs_sql = format!(
        "SELECT COALESCE(jsonb_agg(jsonb_build_object(
            'id', n.id,
            'kind', n.kind,
            'content', n.content,
            'path', n.path::text,
            'parent_kind', p.kind,
            'parent_content', p.content
        ) ORDER BY n.kind, n.path::text), '[]'::jsonb)
        FROM kerai.nodes n
        LEFT JOIN kerai.nodes p ON n.parent_id = p.id
        WHERE n.content = '{}' AND n.kind IN (
            'expr_path', 'expr_method_call', 'type_path', 'expr_call',
            'expr_field', 'pat_path', 'pat_ident', 'pat_struct',
            'pat_tuple_struct', 'use'
        )",
        escaped,
    );

    // Impls: impl blocks where self_ty matches
    let impls_sql = format!(
        "SELECT COALESCE(jsonb_agg(jsonb_build_object(
            'id', id,
            'kind', kind,
            'content', content,
            'path', path::text,
            'metadata', metadata
        ) ORDER BY path::text), '[]'::jsonb)
        FROM kerai.nodes
        WHERE kind = 'impl' AND metadata->>'self_ty' = '{}'",
        escaped,
    );

    let definitions = Spi::get_one::<pgrx::JsonB>(&defs_sql)
        .unwrap()
        .unwrap_or_else(|| pgrx::JsonB(serde_json::json!([])));
    let references = Spi::get_one::<pgrx::JsonB>(&refs_sql)
        .unwrap()
        .unwrap_or_else(|| pgrx::JsonB(serde_json::json!([])));
    let impls = Spi::get_one::<pgrx::JsonB>(&impls_sql)
        .unwrap()
        .unwrap_or_else(|| pgrx::JsonB(serde_json::json!([])));

    pgrx::JsonB(serde_json::json!({
        "symbol": symbol,
        "definitions": definitions.0,
        "references": references.0,
        "impls": impls.0,
    }))
}

/// Navigate the AST tree structure.
///
/// - No path: show top-level nodes (crate, module, file).
/// - Path with lquery wildcards (`*`, `|`, `!`): use `path ~ pattern::lquery`.
/// - Otherwise: use `path <@ pattern::ltree` for subtree.
///
/// Each node includes a `child_count`.
#[pg_extern]
fn tree(path_pattern: Option<&str>) -> pgrx::JsonB {
    let sql = match path_pattern {
        None => {
            // Top-level: nodes with no parent (crate/module/file roots)
            "SELECT COALESCE(jsonb_agg(jsonb_build_object(
                'id', n.id,
                'kind', n.kind,
                'content', n.content,
                'path', n.path::text,
                'child_count', (SELECT count(*) FROM kerai.nodes c WHERE c.parent_id = n.id)
            ) ORDER BY n.path::text, n.position), '[]'::jsonb)
            FROM kerai.nodes n
            WHERE n.parent_id IS NULL".to_string()
        }
        Some(pattern) => {
            let escaped = sql_escape(pattern);
            // Check for lquery wildcards
            let has_lquery = pattern.contains('*') || pattern.contains('|') || pattern.contains('!');
            let where_clause = if has_lquery {
                format!("n.path ~ '{}'::lquery", escaped)
            } else {
                format!("n.path <@ '{}'::ltree", escaped)
            };

            format!(
                "SELECT COALESCE(jsonb_agg(jsonb_build_object(
                    'id', n.id,
                    'kind', n.kind,
                    'content', n.content,
                    'path', n.path::text,
                    'child_count', (SELECT count(*) FROM kerai.nodes c WHERE c.parent_id = n.id)
                ) ORDER BY n.path::text, n.position), '[]'::jsonb)
                FROM kerai.nodes n
                WHERE {}",
                where_clause,
            )
        }
    };

    Spi::get_one::<pgrx::JsonB>(&sql)
        .unwrap()
        .unwrap_or_else(|| pgrx::JsonB(serde_json::json!([])))
}

/// Get direct children of a node, ordered by position.
///
/// Each child includes its own `child_count`.
#[pg_extern]
fn children(node_id: pgrx::Uuid) -> pgrx::JsonB {
    let sql = format!(
        "SELECT COALESCE(jsonb_agg(jsonb_build_object(
            'id', n.id,
            'kind', n.kind,
            'content', n.content,
            'path', n.path::text,
            'position', n.position,
            'child_count', (SELECT count(*) FROM kerai.nodes c WHERE c.parent_id = n.id)
        ) ORDER BY n.position), '[]'::jsonb)
        FROM kerai.nodes n
        WHERE n.parent_id = '{}'::uuid",
        node_id,
    );

    Spi::get_one::<pgrx::JsonB>(&sql)
        .unwrap()
        .unwrap_or_else(|| pgrx::JsonB(serde_json::json!([])))
}

/// Walk the parent chain from a node to the root.
///
/// Returns array ordered by depth (0 = immediate parent, increasing toward root).
#[pg_extern]
fn ancestors(node_id: pgrx::Uuid) -> pgrx::JsonB {
    let sql = format!(
        "WITH RECURSIVE chain AS (
            SELECT parent_id, 0 AS depth
            FROM kerai.nodes WHERE id = '{0}'::uuid
          UNION ALL
            SELECT n.parent_id, c.depth + 1
            FROM chain c
            JOIN kerai.nodes n ON n.id = c.parent_id
            WHERE c.parent_id IS NOT NULL
        )
        SELECT COALESCE(jsonb_agg(jsonb_build_object(
            'id', n.id,
            'kind', n.kind,
            'content', n.content,
            'path', n.path::text,
            'depth', c.depth
        ) ORDER BY c.depth), '[]'::jsonb)
        FROM chain c
        JOIN kerai.nodes n ON n.id = c.parent_id
        WHERE c.parent_id IS NOT NULL",
        node_id,
    );

    Spi::get_one::<pgrx::JsonB>(&sql)
        .unwrap()
        .unwrap_or_else(|| pgrx::JsonB(serde_json::json!([])))
}

/// Full-text search using PostgreSQL tsvector/tsquery with ranking.
///
/// Unlike `find` which uses ILIKE pattern matching, `search` uses proper
/// FTS with `plainto_tsquery` and `ts_rank` for relevance-ranked results.
///
/// Returns JSON array of `{id, kind, content, path, rank, metadata}`.
#[pg_extern]
fn search(query: &str, kind_filter: Option<&str>, limit: Option<i32>) -> pgrx::JsonB {
    let limit_val = limit.unwrap_or(50).max(1).min(1000);
    let escaped_query = sql_escape(query);

    let kind_clause = match kind_filter {
        Some(k) => format!("AND n.kind = '{}'", sql_escape(k)),
        None => String::new(),
    };

    let sql = format!(
        "SELECT COALESCE(jsonb_agg(r ORDER BY rank DESC), '[]'::jsonb) FROM (
            SELECT jsonb_build_object(
                'id', n.id,
                'kind', n.kind,
                'content', n.content,
                'path', n.path::text,
                'rank', ts_rank(to_tsvector('english', COALESCE(n.content, '')), q.query),
                'metadata', n.metadata
            ) AS r,
            ts_rank(to_tsvector('english', COALESCE(n.content, '')), q.query) AS rank
            FROM kerai.nodes n,
                 plainto_tsquery('english', '{}') q(query)
            WHERE to_tsvector('english', COALESCE(n.content, '')) @@ q.query {}
            ORDER BY rank DESC
            LIMIT {}
        ) sub",
        escaped_query, kind_clause, limit_val,
    );

    Spi::get_one::<pgrx::JsonB>(&sql)
        .unwrap()
        .unwrap_or_else(|| pgrx::JsonB(json!([])))
}

/// Context-aware search combining FTS with perspective-weighted ranking.
///
/// Searches nodes by text, optionally boosted by agent perspectives.
/// If agent_names is provided, results are ranked higher when agents
/// have positive perspectives on them.
///
/// Returns JSON array of `{id, kind, content, path, fts_rank, perspective_weight, combined_score, agents}`.
#[pg_extern]
fn context_search(
    query_text: &str,
    agent_names: Option<pgrx::JsonB>,
    limit: Option<i32>,
) -> pgrx::JsonB {
    let limit_val = limit.unwrap_or(50).max(1).min(1000);
    let escaped_query = sql_escape(query_text);

    // Build agent join clause if agent names provided
    let (agent_join, agent_select) = match &agent_names {
        Some(names) => {
            let names_arr = names.0.as_array()
                .map(|arr| arr.iter()
                    .filter_map(|v| v.as_str())
                    .map(|s| format!("'{}'", sql_escape(s)))
                    .collect::<Vec<_>>()
                    .join(", "))
                .unwrap_or_default();

            if names_arr.is_empty() {
                (String::new(), "NULL::double precision AS perspective_weight, '[]'::jsonb AS agents".to_string())
            } else {
                (
                    format!(
                        "LEFT JOIN LATERAL (
                            SELECT avg(p.weight) AS avg_weight,
                                   jsonb_agg(jsonb_build_object('agent', a.name, 'weight', p.weight, 'reasoning', p.reasoning)) AS agent_details
                            FROM kerai.perspectives p
                            JOIN kerai.agents a ON a.id = p.agent_id
                            WHERE p.node_id = n.id AND a.name IN ({})
                        ) pw ON true",
                        names_arr
                    ),
                    "pw.avg_weight AS perspective_weight, COALESCE(pw.agent_details, '[]'::jsonb) AS agents".to_string(),
                )
            }
        }
        None => (
            String::new(),
            "NULL::double precision AS perspective_weight, '[]'::jsonb AS agents".to_string(),
        ),
    };

    // When no agent join, reference pw columns directly as NULLs
    let combined_expr = if agent_join.is_empty() {
        "ts_rank(to_tsvector('english', COALESCE(n.content, '')), q.query) AS combined_score"
    } else {
        "ts_rank(to_tsvector('english', COALESCE(n.content, '')), q.query) * (1.0 + COALESCE(pw.avg_weight, 0.0)) AS combined_score"
    };

    let sql = format!(
        "SELECT COALESCE(jsonb_agg(jsonb_build_object(
            'id', n.id,
            'kind', n.kind,
            'content', n.content,
            'path', n.path::text,
            'fts_rank', ts_rank(to_tsvector('english', COALESCE(n.content, '')), q.query),
            'perspective_weight', sub.perspective_weight,
            'combined_score', sub.combined_score,
            'agents', sub.agents
        ) ORDER BY sub.combined_score DESC), '[]'::jsonb)
        FROM (
            SELECT n.id,
                   {agent_select},
                   {combined_expr}
            FROM kerai.nodes n,
                 plainto_tsquery('english', '{escaped_query}') q(query)
            {agent_join}
            WHERE to_tsvector('english', COALESCE(n.content, '')) @@ q.query
            ORDER BY combined_score DESC
            LIMIT {limit_val}
        ) sub
        JOIN kerai.nodes n ON n.id = sub.id,
             plainto_tsquery('english', '{escaped_query}') q(query)",
    );

    Spi::get_one::<pgrx::JsonB>(&sql)
        .unwrap()
        .unwrap_or_else(|| pgrx::JsonB(json!([])))
}
