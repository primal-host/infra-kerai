/// Discover .rs files in a Rust crate directory.
use std::path::{Path, PathBuf};

/// Discover all .rs files in a crate, skipping target/ and hidden directories.
pub fn discover_rs_files(crate_root: &Path) -> Result<Vec<PathBuf>, String> {
    let src_dir = crate_root.join("src");
    if !src_dir.exists() {
        return Err(format!("No src/ directory found in {}", crate_root.display()));
    }

    let mut files = Vec::new();

    for entry in walkdir::WalkDir::new(&src_dir)
        .follow_links(true)
        .into_iter()
        .filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            // Skip hidden dirs and target/
            !name.starts_with('.') && name != "target"
        })
    {
        let entry = entry.map_err(|e| format!("walkdir error: {}", e))?;
        if entry.file_type().is_file() {
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "rs") {
                files.push(path.to_path_buf());
            }
        }
    }

    // Sort for deterministic ordering
    files.sort();
    Ok(files)
}
