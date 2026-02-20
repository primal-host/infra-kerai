use postgres::Client;

use crate::home;
use crate::output::{print_rows, OutputFormat};

pub fn config_get(client: &mut Client, key: &str, format: &OutputFormat) -> Result<(), String> {
    let row = client
        .query_opt(
            "SELECT kerai.get_preference('config', $1)",
            &[&key],
        )
        .map_err(|e| format!("Failed to get config: {e}"))?;

    match row.and_then(|r| r.get::<_, Option<String>>(0)) {
        Some(value) => {
            let columns = vec!["key".to_string(), "value".to_string()];
            let rows = vec![vec![key.to_string(), value]];
            print_rows(&columns, &rows, format);
        }
        None => {
            match format {
                OutputFormat::Json => println!("null"),
                _ => println!("not found"),
            }
        }
    }
    Ok(())
}

pub fn config_set(
    client: &mut Client,
    key: &str,
    value: &str,
    format: &OutputFormat,
) -> Result<(), String> {
    client
        .execute(
            "SELECT kerai.set_preference('config', $1, $2)",
            &[&key, &value],
        )
        .map_err(|e| format!("Failed to set config: {e}"))?;

    match format {
        OutputFormat::Json => {
            println!(r#"{{"status":"ok","key":"{}","value":"{}"}}"#, key, value);
        }
        _ => println!("set {key} = {value}"),
    }
    Ok(())
}

pub fn config_list(client: &mut Client, format: &OutputFormat) -> Result<(), String> {
    let rows = client
        .query(
            "SELECT key, value, updated_at::text FROM kerai.list_preferences('config')",
            &[],
        )
        .map_err(|e| format!("Failed to list config: {e}"))?;

    let columns = vec![
        "key".to_string(),
        "value".to_string(),
        "updated_at".to_string(),
    ];
    let data: Vec<Vec<String>> = rows
        .iter()
        .map(|r| {
            vec![
                r.get::<_, String>(0),
                r.get::<_, String>(1),
                r.get::<_, String>(2),
            ]
        })
        .collect();

    print_rows(&columns, &data, format);
    Ok(())
}

pub fn config_delete(
    client: &mut Client,
    key: &str,
    format: &OutputFormat,
) -> Result<(), String> {
    let row = client
        .query_one(
            "SELECT kerai.delete_preference('config', $1)",
            &[&key],
        )
        .map_err(|e| format!("Failed to delete config: {e}"))?;

    let result: String = row.get(0);
    match format {
        OutputFormat::Json => {
            println!(r#"{{"status":"{}","key":"{}"}}"#, result, key);
        }
        _ => println!("{result}"),
    }
    Ok(())
}

pub fn alias_get(client: &mut Client, name: &str, format: &OutputFormat) -> Result<(), String> {
    let row = client
        .query_opt(
            "SELECT kerai.get_preference('alias', $1)",
            &[&name],
        )
        .map_err(|e| format!("Failed to get alias: {e}"))?;

    match row.and_then(|r| r.get::<_, Option<String>>(0)) {
        Some(value) => {
            let columns = vec!["name".to_string(), "target".to_string()];
            let rows = vec![vec![name.to_string(), value]];
            print_rows(&columns, &rows, format);
        }
        None => {
            match format {
                OutputFormat::Json => println!("null"),
                _ => println!("not found"),
            }
        }
    }
    Ok(())
}

pub fn alias_set(
    client: &mut Client,
    name: &str,
    target: &str,
    format: &OutputFormat,
) -> Result<(), String> {
    client
        .execute(
            "SELECT kerai.set_preference('alias', $1, $2)",
            &[&name, &target],
        )
        .map_err(|e| format!("Failed to set alias: {e}"))?;

    // Sync aliases cache
    sync_aliases_from_db(client)?;

    match format {
        OutputFormat::Json => {
            println!(r#"{{"status":"ok","name":"{}","target":"{}"}}"#, name, target);
        }
        _ => println!("alias {name}: {target}"),
    }
    Ok(())
}

pub fn alias_list(client: &mut Client, format: &OutputFormat) -> Result<(), String> {
    let rows = client
        .query(
            "SELECT key, value, updated_at::text FROM kerai.list_preferences('alias')",
            &[],
        )
        .map_err(|e| format!("Failed to list aliases: {e}"))?;

    let columns = vec![
        "name".to_string(),
        "target".to_string(),
        "updated_at".to_string(),
    ];
    let data: Vec<Vec<String>> = rows
        .iter()
        .map(|r| {
            vec![
                r.get::<_, String>(0),
                r.get::<_, String>(1),
                r.get::<_, String>(2),
            ]
        })
        .collect();

    print_rows(&columns, &data, format);
    Ok(())
}

pub fn alias_delete(
    client: &mut Client,
    name: &str,
    format: &OutputFormat,
) -> Result<(), String> {
    let row = client
        .query_one(
            "SELECT kerai.delete_preference('alias', $1)",
            &[&name],
        )
        .map_err(|e| format!("Failed to delete alias: {e}"))?;

    let result: String = row.get(0);

    // Sync aliases cache after delete
    if result == "deleted" {
        sync_aliases_from_db(client)?;
    }

    match format {
        OutputFormat::Json => {
            println!(r#"{{"status":"{}","name":"{}"}}"#, result, name);
        }
        _ => println!("{result}"),
    }
    Ok(())
}

/// Query all aliases from postgres and write to ~/.kerai/aliases.cache.
pub fn sync_aliases_from_db(client: &mut Client) -> Result<(), String> {
    let rows = client
        .query(
            "SELECT key, value FROM kerai.list_preferences('alias')",
            &[],
        )
        .map_err(|e| format!("Failed to query aliases: {e}"))?;

    let aliases: Vec<(String, String)> = rows
        .iter()
        .map(|r| (r.get::<_, String>(0), r.get::<_, String>(1)))
        .collect();

    let kerai_home = home::ensure_home_dir()?;
    home::sync_aliases_cache(&kerai_home, &aliases)
}
