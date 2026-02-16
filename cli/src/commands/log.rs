use postgres::Client;

use crate::output::{print_rows, OutputFormat};

pub fn run(
    client: &mut Client,
    author: Option<&str>,
    limit: i64,
    format: &OutputFormat,
) -> Result<(), String> {
    let (sql, rows) = if let Some(author_val) = author {
        let sql = "SELECT lamport_ts, author_seq, op_type, node_id::text, author, created_at::text \
                    FROM kerai.operations WHERE author = $1 ORDER BY lamport_ts DESC LIMIT $2";
        let rows = client
            .query(sql, &[&author_val, &limit])
            .map_err(|e| format!("Query failed: {e}"))?;
        (sql, rows)
    } else {
        let sql = "SELECT lamport_ts, author_seq, op_type, node_id::text, author, created_at::text \
                    FROM kerai.operations ORDER BY lamport_ts DESC LIMIT $1";
        let rows = client
            .query(sql, &[&limit])
            .map_err(|e| format!("Query failed: {e}"))?;
        (sql, rows)
    };
    let _ = sql;

    if rows.is_empty() {
        println!("No operations found.");
        return Ok(());
    }

    let columns = vec![
        "lamport_ts".into(),
        "author_seq".into(),
        "op_type".into(),
        "node_id".into(),
        "author".into(),
        "created_at".into(),
    ];

    let data: Vec<Vec<String>> = rows
        .iter()
        .map(|row| {
            vec![
                row.try_get::<_, i64>(0)
                    .map(|v| v.to_string())
                    .unwrap_or_default(),
                row.try_get::<_, i64>(1)
                    .map(|v| v.to_string())
                    .unwrap_or_default(),
                row.try_get::<_, String>(2).unwrap_or_default(),
                row.try_get::<_, String>(3).unwrap_or_default(),
                row.try_get::<_, String>(4).unwrap_or_default(),
                row.try_get::<_, String>(5).unwrap_or_default(),
            ]
        })
        .collect();

    print_rows(&columns, &data, format);
    Ok(())
}
