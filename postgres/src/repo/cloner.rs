/// Git operations via libgit2: clone, fetch, HEAD resolution.
use git2::{FetchOptions, Repository};
use std::path::{Path, PathBuf};

/// Clone a repository into `dest`. Returns the opened repo.
pub fn clone_repo(url: &str, dest: &Path) -> Result<Repository, String> {
    Repository::clone(url, dest).map_err(|e| format!("git clone failed: {}", e))
}

/// Fetch updates for an existing repository.
pub fn fetch_repo(repo: &Repository) -> Result<(), String> {
    let mut remote = repo
        .find_remote("origin")
        .map_err(|e| format!("no remote 'origin': {}", e))?;

    let mut opts = FetchOptions::new();
    remote
        .fetch(&[] as &[&str], Some(&mut opts), None)
        .map_err(|e| format!("fetch failed: {}", e))?;

    // Fast-forward HEAD to FETCH_HEAD if possible
    let fetch_head = repo
        .find_reference("FETCH_HEAD")
        .map_err(|e| format!("no FETCH_HEAD: {}", e))?;
    let fetch_commit = repo
        .reference_to_annotated_commit(&fetch_head)
        .map_err(|e| format!("FETCH_HEAD resolve failed: {}", e))?;

    let head_ref = match repo.head() {
        Ok(h) => h,
        Err(_) => return Ok(()), // detached or empty — skip ff
    };

    if let Some(refname) = head_ref.name() {
        let mut reference = repo
            .find_reference(refname)
            .map_err(|e| format!("ref lookup failed: {}", e))?;
        reference
            .set_target(fetch_commit.id(), "kerai: fast-forward after fetch")
            .map_err(|e| format!("fast-forward failed: {}", e))?;
        repo.set_head(refname)
            .map_err(|e| format!("set_head failed: {}", e))?;
        repo.checkout_head(Some(git2::build::CheckoutBuilder::default().force()))
            .map_err(|e| format!("checkout failed: {}", e))?;
    }

    Ok(())
}

/// Get the SHA of HEAD.
pub fn head_sha(repo: &Repository) -> Result<String, String> {
    let head = repo
        .head()
        .map_err(|e| format!("no HEAD: {}", e))?;
    let commit = head
        .peel_to_commit()
        .map_err(|e| format!("HEAD is not a commit: {}", e))?;
    Ok(commit.id().to_string())
}

/// Open an existing repository at `path`.
pub fn open_repo(path: &Path) -> Result<Repository, String> {
    Repository::open(path).map_err(|e| format!("failed to open repo at {}: {}", path.display(), e))
}

/// Compute the clone destination path. Uses `$PGDATA/kerai_repos/{name}_{short_uuid}/`,
/// falling back to a temp directory.
pub fn clone_path(name: &str, short_id: &str) -> PathBuf {
    let dirname = format!("{}_{}", sanitize_name(name), short_id);

    // Try $PGDATA first
    if let Ok(pgdata) = std::env::var("PGDATA") {
        let base = Path::new(&pgdata).join("kerai_repos");
        let dest = base.join(&dirname);
        if std::fs::create_dir_all(&base).is_ok() {
            return dest;
        }
    }

    // Fallback to temp dir
    std::env::temp_dir().join("kerai_repos").join(dirname)
}

/// Sanitize a repository name for filesystem use.
fn sanitize_name(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect()
}

/// Extract a repository name from a URL.
///
/// Examples:
/// - `https://github.com/user/repo.git` → `repo`
/// - `file:///tmp/test-repo` → `test-repo`
/// - `git@github.com:user/repo.git` → `repo`
pub fn repo_name_from_url(url: &str) -> String {
    let path = url
        .rsplit('/')
        .next()
        .or_else(|| url.rsplit(':').next())
        .unwrap_or(url);

    path.trim_end_matches(".git").to_string()
}
