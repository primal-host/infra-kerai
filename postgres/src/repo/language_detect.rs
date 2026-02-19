// Extension-based language classification for repository files.

/// Languages that have a kerai parser.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseableLanguage {
    Rust,
    Go,
    C,
    Markdown,
}

impl ParseableLanguage {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Rust => "rust",
            Self::Go => "go",
            Self::C => "c",
            Self::Markdown => "markdown",
        }
    }
}

/// Classification result for a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LanguageClass {
    /// Has a kerai parser — will be fully parsed into AST nodes.
    Parseable(ParseableLanguage),
    /// Known text language without a parser — source stored in metadata.
    OpaqueText(String),
    /// Binary file — only hash and size stored.
    Binary,
}

/// Classify a file by its extension and (optionally) filename.
///
/// For unknown extensions, `content_sample` is checked for null bytes
/// (git's heuristic): if null bytes are found in the first 8KB, it's binary.
pub fn classify(filename: &str, content_sample: Option<&[u8]>) -> LanguageClass {
    // Check special filenames first
    let basename = filename.rsplit('/').next().unwrap_or(filename);
    if let Some(class) = classify_special_filename(basename) {
        return class;
    }

    // Extract extension
    let ext = match basename.rsplit_once('.') {
        Some((_, ext)) => ext.to_lowercase(),
        None => {
            // No extension — check content for binary
            return if is_binary(content_sample) {
                LanguageClass::Binary
            } else {
                LanguageClass::OpaqueText("text".to_string())
            };
        }
    };

    classify_extension(&ext, content_sample)
}

/// Special filenames that don't rely on extension.
fn classify_special_filename(basename: &str) -> Option<LanguageClass> {
    match basename {
        "Makefile" | "GNUmakefile" => Some(LanguageClass::OpaqueText("make".to_string())),
        "Dockerfile" => Some(LanguageClass::OpaqueText("dockerfile".to_string())),
        "Vagrantfile" => Some(LanguageClass::OpaqueText("ruby".to_string())),
        "Rakefile" | "Gemfile" => Some(LanguageClass::OpaqueText("ruby".to_string())),
        "CMakeLists.txt" => Some(LanguageClass::OpaqueText("cmake".to_string())),
        ".gitignore" | ".gitattributes" | ".gitmodules" => {
            Some(LanguageClass::OpaqueText("gitconfig".to_string()))
        }
        ".editorconfig" => Some(LanguageClass::OpaqueText("ini".to_string())),
        _ => None,
    }
}

/// Classify by file extension.
fn classify_extension(ext: &str, content_sample: Option<&[u8]>) -> LanguageClass {
    match ext {
        // Parseable languages
        "rs" => LanguageClass::Parseable(ParseableLanguage::Rust),
        "go" => LanguageClass::Parseable(ParseableLanguage::Go),
        "c" | "h" => LanguageClass::Parseable(ParseableLanguage::C),
        "md" | "markdown" => LanguageClass::Parseable(ParseableLanguage::Markdown),

        // Opaque text — scripting
        "py" | "pyw" | "pyi" => LanguageClass::OpaqueText("python".to_string()),
        "js" | "mjs" | "cjs" => LanguageClass::OpaqueText("javascript".to_string()),
        "ts" | "mts" | "cts" => LanguageClass::OpaqueText("typescript".to_string()),
        "jsx" => LanguageClass::OpaqueText("jsx".to_string()),
        "tsx" => LanguageClass::OpaqueText("tsx".to_string()),
        "rb" => LanguageClass::OpaqueText("ruby".to_string()),
        "php" => LanguageClass::OpaqueText("php".to_string()),
        "pl" | "pm" => LanguageClass::OpaqueText("perl".to_string()),
        "lua" => LanguageClass::OpaqueText("lua".to_string()),
        "r" => LanguageClass::OpaqueText("r".to_string()),
        "jl" => LanguageClass::OpaqueText("julia".to_string()),
        "ex" | "exs" => LanguageClass::OpaqueText("elixir".to_string()),
        "erl" | "hrl" => LanguageClass::OpaqueText("erlang".to_string()),

        // Opaque text — compiled
        "java" => LanguageClass::OpaqueText("java".to_string()),
        "kt" | "kts" => LanguageClass::OpaqueText("kotlin".to_string()),
        "scala" | "sc" => LanguageClass::OpaqueText("scala".to_string()),
        "swift" => LanguageClass::OpaqueText("swift".to_string()),
        "cs" => LanguageClass::OpaqueText("csharp".to_string()),
        "cpp" | "cc" | "cxx" | "hpp" | "hxx" | "hh" => {
            LanguageClass::OpaqueText("cpp".to_string())
        }
        "zig" => LanguageClass::OpaqueText("zig".to_string()),
        "nim" => LanguageClass::OpaqueText("nim".to_string()),
        "d" => LanguageClass::OpaqueText("d".to_string()),
        "hs" | "lhs" => LanguageClass::OpaqueText("haskell".to_string()),
        "ml" | "mli" => LanguageClass::OpaqueText("ocaml".to_string()),
        "fs" | "fsx" | "fsi" => LanguageClass::OpaqueText("fsharp".to_string()),
        "clj" | "cljs" | "cljc" => LanguageClass::OpaqueText("clojure".to_string()),
        "dart" => LanguageClass::OpaqueText("dart".to_string()),

        // Opaque text — shell / scripting
        "sh" | "bash" | "zsh" => LanguageClass::OpaqueText("shell".to_string()),
        "fish" => LanguageClass::OpaqueText("fish".to_string()),
        "ps1" | "psm1" | "psd1" => LanguageClass::OpaqueText("powershell".to_string()),
        "bat" | "cmd" => LanguageClass::OpaqueText("batch".to_string()),

        // Opaque text — data / config
        "yaml" | "yml" => LanguageClass::OpaqueText("yaml".to_string()),
        "json" | "jsonc" => LanguageClass::OpaqueText("json".to_string()),
        "toml" => LanguageClass::OpaqueText("toml".to_string()),
        "xml" | "xsl" | "xsd" => LanguageClass::OpaqueText("xml".to_string()),
        "ini" | "cfg" => LanguageClass::OpaqueText("ini".to_string()),
        "csv" | "tsv" => LanguageClass::OpaqueText("csv".to_string()),
        "sql" => LanguageClass::OpaqueText("sql".to_string()),
        "graphql" | "gql" => LanguageClass::OpaqueText("graphql".to_string()),
        "proto" => LanguageClass::OpaqueText("protobuf".to_string()),

        // Opaque text — web
        "html" | "htm" | "xhtml" => LanguageClass::OpaqueText("html".to_string()),
        "css" => LanguageClass::OpaqueText("css".to_string()),
        "scss" | "sass" => LanguageClass::OpaqueText("scss".to_string()),
        "less" => LanguageClass::OpaqueText("less".to_string()),
        "vue" => LanguageClass::OpaqueText("vue".to_string()),
        "svelte" => LanguageClass::OpaqueText("svelte".to_string()),

        // Opaque text — docs / plain text
        "txt" | "text" => LanguageClass::OpaqueText("text".to_string()),
        "rst" => LanguageClass::OpaqueText("rst".to_string()),
        "adoc" | "asciidoc" => LanguageClass::OpaqueText("asciidoc".to_string()),
        "tex" | "latex" => LanguageClass::OpaqueText("latex".to_string()),
        "org" => LanguageClass::OpaqueText("org".to_string()),
        "log" => LanguageClass::OpaqueText("log".to_string()),

        // Opaque text — build / CI
        "cmake" => LanguageClass::OpaqueText("cmake".to_string()),
        "gradle" => LanguageClass::OpaqueText("gradle".to_string()),

        // Opaque text — misc
        "lock" => LanguageClass::OpaqueText("lockfile".to_string()),
        "env" => LanguageClass::OpaqueText("dotenv".to_string()),
        "diff" | "patch" => LanguageClass::OpaqueText("diff".to_string()),

        // Binary formats
        "png" | "jpg" | "jpeg" | "gif" | "bmp" | "ico" | "webp" | "svg" | "tiff" | "tif" => {
            LanguageClass::Binary
        }
        "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "odt" => {
            LanguageClass::Binary
        }
        "zip" | "tar" | "gz" | "bz2" | "xz" | "7z" | "rar" | "zst" => LanguageClass::Binary,
        "exe" | "dll" | "so" | "dylib" | "a" | "lib" => LanguageClass::Binary,
        "o" | "obj" | "pyc" | "pyo" | "class" => LanguageClass::Binary,
        "wasm" => LanguageClass::Binary,
        "bin" | "dat" => LanguageClass::Binary,
        "ttf" | "otf" | "woff" | "woff2" | "eot" => LanguageClass::Binary,
        "mp3" | "mp4" | "wav" | "ogg" | "flac" | "avi" | "mkv" | "mov" | "webm" => {
            LanguageClass::Binary
        }
        "db" | "sqlite" | "sqlite3" => LanguageClass::Binary,

        // Unknown extension — use content heuristic
        _ => {
            if is_binary(content_sample) {
                LanguageClass::Binary
            } else {
                LanguageClass::OpaqueText("text".to_string())
            }
        }
    }
}

/// Check if content is likely binary by looking for null bytes in the sample.
/// This mirrors git's own heuristic.
fn is_binary(sample: Option<&[u8]>) -> bool {
    match sample {
        Some(data) => data.iter().take(8192).any(|&b| b == 0),
        None => false, // assume text if no sample available
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parseable_languages() {
        assert_eq!(
            classify("main.rs", None),
            LanguageClass::Parseable(ParseableLanguage::Rust)
        );
        assert_eq!(
            classify("main.go", None),
            LanguageClass::Parseable(ParseableLanguage::Go)
        );
        assert_eq!(
            classify("main.c", None),
            LanguageClass::Parseable(ParseableLanguage::C)
        );
        assert_eq!(
            classify("header.h", None),
            LanguageClass::Parseable(ParseableLanguage::C)
        );
        assert_eq!(
            classify("README.md", None),
            LanguageClass::Parseable(ParseableLanguage::Markdown)
        );
        assert_eq!(
            classify("doc.markdown", None),
            LanguageClass::Parseable(ParseableLanguage::Markdown)
        );
    }

    #[test]
    fn test_opaque_text_languages() {
        assert_eq!(
            classify("script.py", None),
            LanguageClass::OpaqueText("python".to_string())
        );
        assert_eq!(
            classify("app.js", None),
            LanguageClass::OpaqueText("javascript".to_string())
        );
        assert_eq!(
            classify("types.ts", None),
            LanguageClass::OpaqueText("typescript".to_string())
        );
        assert_eq!(
            classify("Main.java", None),
            LanguageClass::OpaqueText("java".to_string())
        );
        assert_eq!(
            classify("deploy.sh", None),
            LanguageClass::OpaqueText("shell".to_string())
        );
        assert_eq!(
            classify("config.yaml", None),
            LanguageClass::OpaqueText("yaml".to_string())
        );
        assert_eq!(
            classify("data.json", None),
            LanguageClass::OpaqueText("json".to_string())
        );
        assert_eq!(
            classify("settings.toml", None),
            LanguageClass::OpaqueText("toml".to_string())
        );
        assert_eq!(
            classify("index.html", None),
            LanguageClass::OpaqueText("html".to_string())
        );
        assert_eq!(
            classify("style.css", None),
            LanguageClass::OpaqueText("css".to_string())
        );
        assert_eq!(
            classify("query.sql", None),
            LanguageClass::OpaqueText("sql".to_string())
        );
    }

    #[test]
    fn test_binary_files() {
        assert_eq!(classify("image.png", None), LanguageClass::Binary);
        assert_eq!(classify("photo.jpg", None), LanguageClass::Binary);
        assert_eq!(classify("archive.zip", None), LanguageClass::Binary);
        assert_eq!(classify("program.exe", None), LanguageClass::Binary);
        assert_eq!(classify("module.wasm", None), LanguageClass::Binary);
        assert_eq!(classify("lib.so", None), LanguageClass::Binary);
        assert_eq!(classify("doc.pdf", None), LanguageClass::Binary);
    }

    #[test]
    fn test_special_filenames() {
        assert_eq!(
            classify("Makefile", None),
            LanguageClass::OpaqueText("make".to_string())
        );
        assert_eq!(
            classify("Dockerfile", None),
            LanguageClass::OpaqueText("dockerfile".to_string())
        );
        assert_eq!(
            classify("Gemfile", None),
            LanguageClass::OpaqueText("ruby".to_string())
        );
        assert_eq!(
            classify("CMakeLists.txt", None),
            LanguageClass::OpaqueText("cmake".to_string())
        );
        assert_eq!(
            classify(".gitignore", None),
            LanguageClass::OpaqueText("gitconfig".to_string())
        );
    }

    #[test]
    fn test_unknown_extension_text() {
        // No null bytes → text
        let sample = b"Hello world\nThis is text\n";
        assert_eq!(
            classify("readme.unknown", Some(sample)),
            LanguageClass::OpaqueText("text".to_string())
        );
    }

    #[test]
    fn test_unknown_extension_binary() {
        // Null byte → binary
        let sample = b"Hello\x00world";
        assert_eq!(
            classify("data.unknown", Some(sample)),
            LanguageClass::Binary
        );
    }

    #[test]
    fn test_no_extension_no_sample() {
        // No extension, no sample → assume text
        assert_eq!(
            classify("LICENSE", None),
            LanguageClass::OpaqueText("text".to_string())
        );
    }

    #[test]
    fn test_path_with_directories() {
        assert_eq!(
            classify("src/main.rs", None),
            LanguageClass::Parseable(ParseableLanguage::Rust)
        );
        assert_eq!(
            classify("deep/path/to/file.py", None),
            LanguageClass::OpaqueText("python".to_string())
        );
    }

    #[test]
    fn test_case_insensitive_extension() {
        assert_eq!(
            classify("README.MD", None),
            LanguageClass::Parseable(ParseableLanguage::Markdown)
        );
        assert_eq!(
            classify("image.PNG", None),
            LanguageClass::Binary
        );
    }
}
