/// SQL-based language census for a repository.
use pgrx::prelude::*;
use serde_json::{json, Value};

use crate::sql::sql_uuid;

/// Aggregate files by language under a repo root node.
///
/// Returns JSON: `{repo_id, total_files, total_lines, languages: {lang: {files, lines}}}`.
pub fn repo_census(repo_node_id: &str) -> Value {
    let node_id = sql_uuid(repo_node_id);

    let mut languages: serde_json::Map<String, Value> = serde_json::Map::new();
    let mut total_files: i64 = 0;
    let mut total_lines: i64 = 0;

    Spi::connect(|client| {
        // Count parsed files (kind='file') grouped by language
        let query = format!(
            "WITH RECURSIVE descendants AS (
                SELECT id, kind, language, metadata FROM kerai.nodes
                WHERE parent_id = {node_id}
                UNION ALL
                SELECT n.id, n.kind, n.language, n.metadata FROM kerai.nodes n
                JOIN descendants d ON n.parent_id = d.id
            )
            SELECT
                COALESCE(language, 'unknown') AS lang,
                COUNT(*)::bigint AS file_count,
                COALESCE(SUM(COALESCE((metadata->>'line_count')::bigint, 0)), 0)::bigint AS line_count
            FROM descendants
            WHERE kind IN ('file', 'repo_opaque_text')
            GROUP BY language
            ORDER BY file_count DESC",
        );

        let result = client.select(&query, None, &[]).unwrap();
        for row in result {
            let lang: String = row
                .get_by_name::<String, _>("lang")
                .unwrap()
                .unwrap_or_else(|| "unknown".to_string());
            let files: i64 = row
                .get_by_name::<i64, _>("file_count")
                .unwrap()
                .unwrap_or(0);
            let lines: i64 = row
                .get_by_name::<i64, _>("line_count")
                .unwrap()
                .unwrap_or(0);

            total_files += files;
            total_lines += lines;

            languages.insert(
                lang,
                json!({
                    "files": files,
                    "lines": lines,
                }),
            );
        }

        // Count binary files separately
        let binary_query = format!(
            "WITH RECURSIVE descendants AS (
                SELECT id, kind FROM kerai.nodes
                WHERE parent_id = {node_id}
                UNION ALL
                SELECT n.id, n.kind FROM kerai.nodes n
                JOIN descendants d ON n.parent_id = d.id
            )
            SELECT COUNT(*)::bigint AS cnt FROM descendants
            WHERE kind = 'repo_opaque_binary'",
        );

        let binary_result = client.select(&binary_query, None, &[]).unwrap();
        for row in binary_result {
            let count: i64 = row
                .get_by_name::<i64, _>("cnt")
                .unwrap()
                .unwrap_or(0);
            if count > 0 {
                total_files += count;
                languages.insert(
                    "binary".to_string(),
                    json!({"files": count, "lines": 0}),
                );
            }
        }
    });

    json!({
        "repo_id": repo_node_id,
        "total_files": total_files,
        "total_lines": total_lines,
        "languages": languages,
    })
}
