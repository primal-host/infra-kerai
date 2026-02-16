use postgres::Client;

use crate::output::{print_rows, OutputFormat};

pub fn run(client: &mut Client, sql: &str, format: &OutputFormat) -> Result<(), String> {
    let rows = client
        .query(sql, &[])
        .map_err(|e| format!("Query failed: {e}"))?;

    if rows.is_empty() {
        println!("(0 rows)");
        return Ok(());
    }

    let columns: Vec<String> = rows[0]
        .columns()
        .iter()
        .map(|c| c.name().to_string())
        .collect();

    let data: Vec<Vec<String>> = rows
        .iter()
        .map(|row| {
            (0..columns.len())
                .map(|i| row_value_to_string(row, i))
                .collect()
        })
        .collect();

    print_rows(&columns, &data, format);
    Ok(())
}

fn row_value_to_string(row: &postgres::Row, idx: usize) -> String {
    use postgres::types::Type;

    let col_type = row.columns()[idx].type_();

    match *col_type {
        Type::BOOL => row
            .try_get::<_, bool>(idx)
            .map(|v| v.to_string())
            .unwrap_or_default(),
        Type::INT2 => row
            .try_get::<_, i16>(idx)
            .map(|v| v.to_string())
            .unwrap_or_default(),
        Type::INT4 => row
            .try_get::<_, i32>(idx)
            .map(|v| v.to_string())
            .unwrap_or_default(),
        Type::INT8 => row
            .try_get::<_, i64>(idx)
            .map(|v| v.to_string())
            .unwrap_or_default(),
        Type::FLOAT4 => row
            .try_get::<_, f32>(idx)
            .map(|v| v.to_string())
            .unwrap_or_default(),
        Type::FLOAT8 => row
            .try_get::<_, f64>(idx)
            .map(|v| v.to_string())
            .unwrap_or_default(),
        Type::UUID => row
            .try_get::<_, uuid::Uuid>(idx)
            .map(|v| v.to_string())
            .unwrap_or_default(),
        Type::JSON | Type::JSONB => row
            .try_get::<_, serde_json::Value>(idx)
            .map(|v| v.to_string())
            .unwrap_or_default(),
        _ => row
            .try_get::<_, String>(idx)
            .unwrap_or_default(),
    }
}
