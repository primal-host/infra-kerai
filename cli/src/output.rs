use clap::ValueEnum;
use comfy_table::{presets::UTF8_FULL_CONDENSED, Table};

#[derive(Debug, Clone, ValueEnum)]
pub enum OutputFormat {
    Table,
    Json,
    Csv,
}

/// Print a JSON value in the requested format.
pub fn print_json(value: &serde_json::Value, format: &OutputFormat) {
    match format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(value).unwrap());
        }
        OutputFormat::Table | OutputFormat::Csv => {
            // For non-JSON formats, just pretty-print the JSON
            println!("{}", serde_json::to_string_pretty(value).unwrap());
        }
    }
}

/// Print tabular data in the requested format.
pub fn print_rows(columns: &[String], rows: &[Vec<String>], format: &OutputFormat) {
    match format {
        OutputFormat::Table => {
            let mut table = Table::new();
            table.load_preset(UTF8_FULL_CONDENSED);
            table.set_header(columns);
            for row in rows {
                table.add_row(row);
            }
            println!("{table}");
        }
        OutputFormat::Json => {
            let json_rows: Vec<serde_json::Value> = rows
                .iter()
                .map(|row| {
                    let mut map = serde_json::Map::new();
                    for (i, col) in columns.iter().enumerate() {
                        map.insert(
                            col.clone(),
                            serde_json::Value::String(row.get(i).cloned().unwrap_or_default()),
                        );
                    }
                    serde_json::Value::Object(map)
                })
                .collect();
            println!("{}", serde_json::to_string_pretty(&json_rows).unwrap());
        }
        OutputFormat::Csv => {
            println!("{}", columns.join(","));
            for row in rows {
                println!("{}", row.join(","));
            }
        }
    }
}
