/// Node kind string constants for repository ingestion.

/// Root node for an ingested repository.
pub const REPO_REPOSITORY: &str = "repo_repository";

/// Git commit node.
pub const REPO_COMMIT: &str = "repo_commit";

/// Directory in the file tree.
pub const REPO_DIRECTORY: &str = "repo_directory";

/// Git tag reference.
pub const REPO_TAG: &str = "repo_tag";

/// Git branch reference.
pub const REPO_BRANCH: &str = "repo_branch";

/// Unparsed text file (source stored in metadata).
pub const REPO_OPAQUE_TEXT: &str = "repo_opaque_text";

/// Binary file (hash + size only in metadata).
pub const REPO_OPAQUE_BINARY: &str = "repo_opaque_binary";
