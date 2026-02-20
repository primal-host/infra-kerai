use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::lang;

const DEFAULT_ALIASES: &str = "\
# common aliases for kerai libraries
pg: postgres
";

/// Creates `~/.kerai/` if it doesn't exist, returns its path.
pub fn ensure_home_dir() -> Result<PathBuf, String> {
    let home = dirs::home_dir().ok_or("could not determine home directory")?;
    let kerai_home = home.join(".kerai");
    if !kerai_home.exists() {
        fs::create_dir_all(&kerai_home)
            .map_err(|e| format!("failed to create ~/.kerai: {e}"))?;
    }
    Ok(kerai_home)
}

/// Creates `aliases.kerai` with default content if missing, returns its path.
pub fn ensure_aliases_file(home: &Path) -> Result<PathBuf, String> {
    let path = home.join("aliases.kerai");
    if !path.exists() {
        fs::write(&path, DEFAULT_ALIASES)
            .map_err(|e| format!("failed to create aliases.kerai: {e}"))?;
    }
    Ok(path)
}

/// Parses `aliases.kerai` into a map of alias → target namespace.
pub fn load_aliases(home: &Path) -> Result<HashMap<String, String>, String> {
    let path = home.join("aliases.kerai");
    if !path.exists() {
        return Ok(HashMap::new());
    }
    let doc = lang::parse_file(&path)?;
    Ok(lang::definitions(&doc)
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect())
}

/// Loads aliases from `aliases.cache` (tab-separated key\tvalue lines).
/// Falls back to `aliases.kerai` if cache doesn't exist.
pub fn load_aliases_cache(home: &Path) -> Result<HashMap<String, String>, String> {
    let cache_path = home.join("aliases.cache");
    if cache_path.exists() {
        let content = fs::read_to_string(&cache_path)
            .map_err(|e| format!("failed to read aliases.cache: {e}"))?;
        let mut map = HashMap::new();
        for line in content.lines() {
            if let Some((key, value)) = line.split_once('\t') {
                map.insert(key.to_string(), value.to_string());
            }
        }
        return Ok(map);
    }
    // Fallback to aliases.kerai for backward compatibility
    load_aliases(home)
}

/// Writes aliases to `aliases.cache` (tab-separated key\tvalue lines).
pub fn sync_aliases_cache(home: &Path, aliases: &[(String, String)]) -> Result<(), String> {
    let cache_path = home.join("aliases.cache");
    let content: String = aliases
        .iter()
        .map(|(k, v)| format!("{k}\t{v}"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut output = content;
    if !output.is_empty() && !output.ends_with('\n') {
        output.push('\n');
    }
    fs::write(&cache_path, output)
        .map_err(|e| format!("failed to write aliases.cache: {e}"))
}

const KERAI_FILE_HEADER: &str = "\
# kerai-controlled configuration — do not hand-edit
# syntax: name: target (definition) | name arg (function call) | :name expr (type application)
";

/// Creates `kerai.kerai` with header comment if missing, returns its path.
pub fn ensure_kerai_file(home: &Path) -> Result<PathBuf, String> {
    let path = home.join("kerai.kerai");
    if !path.exists() {
        fs::write(&path, KERAI_FILE_HEADER)
            .map_err(|e| format!("failed to create kerai.kerai: {e}"))?;
    }
    Ok(path)
}

/// Parses `kerai.kerai` function-call lines into a key→value map.
pub fn load_kerai_file(home: &Path) -> Result<HashMap<String, String>, String> {
    let path = home.join("kerai.kerai");
    if !path.exists() {
        return Ok(HashMap::new());
    }
    let doc = lang::parse_file(&path)?;
    Ok(lang::calls(&doc)
        .into_iter()
        .map(|(f, args)| (f.to_string(), args.join(" ")))
        .collect())
}

/// Sets a key-value pair in `kerai.kerai`, replacing an existing line or appending.
pub fn set_kerai_value(home: &Path, key: &str, value: &str) -> Result<(), String> {
    let path = home.join("kerai.kerai");
    let content = if path.exists() {
        fs::read_to_string(&path)
            .map_err(|e| format!("failed to read kerai.kerai: {e}"))?
    } else {
        KERAI_FILE_HEADER.to_string()
    };

    let doc = lang::parse(&content);
    let mut found = false;
    let mut output_lines: Vec<String> = Vec::with_capacity(doc.lines.len() + 1);

    for line in &doc.lines {
        match line {
            lang::Line::Call {
                function, args: _, ..
            } if function == key => {
                found = true;
                output_lines.push(format!("{key} {value}"));
            }
            lang::Line::Empty => output_lines.push(String::new()),
            lang::Line::Comment { text } => output_lines.push(text.clone()),
            _ => output_lines.push(lang::render_line(line)),
        }
    }

    if !found {
        output_lines.push(format!("{key} {value}"));
    }

    let mut output = output_lines.join("\n");
    if !output.ends_with('\n') {
        output.push('\n');
    }

    fs::write(&path, output)
        .map_err(|e| format!("failed to write kerai.kerai: {e}"))?;
    Ok(())
}
