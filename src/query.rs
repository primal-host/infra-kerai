/// Query & Navigation — find, refs, tree, children, ancestors.
use pgrx::prelude::*;

/// Escape a string for use in a SQL literal (double single quotes).
fn sql_escape(s: &str) -> String {
    s.replace('\'', "''")
}

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
