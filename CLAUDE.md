# Kerai — Project Instructions

## Overview
pgrx Postgres extension (Rust) for AST-based version control with CRDT sync and knowledge economy.

## Environment
- **pgrx**: 0.17.0 (must match cargo-pgrx CLI exactly)
- **Postgres**: 17.x (Homebrew standalone, NOT Docker)
- **Rust**: stable (1.93+)

## Build & Test

```bash
# Run all tests (required flags for macOS + PG17)
LC_ALL=C CARGO_TARGET_DIR="$(pwd)/tgt" cargo pgrx test pg17

# Interactive REPL
cargo pgrx run pg17
```

- `LC_ALL=C` — fixes PG17 "postmaster became multithreaded during startup" on macOS
- Absolute `CARGO_TARGET_DIR` — fixes pgrx-tests relative path bug with initdb
- `#[should_panic]` needed for constraint violation tests (PG errors propagate as panics)

### tree-sitter-latex build prerequisite

The `tree-sitter-latex` crate is a git dependency (not on crates.io). Its repo gitignores the generated `src/parser.c` file, so a fresh clone won't compile. Before the first build, generate it:

```bash
# Install tree-sitter CLI (one-time)
cargo install tree-sitter-cli

# Generate parser.c in the cargo git checkout
CHECKOUT=$(find ~/.cargo/git/checkouts -name "tree-sitter-latex-*" -type d -maxdepth 1)/$(ls $(find ~/.cargo/git/checkouts -name "tree-sitter-latex-*" -type d -maxdepth 1))
cd "$CHECKOUT" && tree-sitter generate && cd -
```

This only needs to be done once per checkout (or after `cargo clean` clears the git cache).

## Architecture

### Schema
- Extension schema: `kerai` (set in `.control` file's `schema = kerai`)
- **DO NOT** use `schema = "kerai"` on `#[pg_extern]` — causes "schema did not exist" error
- Schema is auto-created by Postgres from the `.control` file
- **DO NOT** include `CREATE SCHEMA` in `extension_sql!` — conflicts with `.control` auto-create
- `.control` requires `superuser = true` and `trusted = false` (pgrx 0.17 mandates these)
- `requires = 'ltree'` in `.control` — ltree must be created before kerai

### Module Layout
```
src/
├── lib.rs              # Root: module declarations, pg_module_magic, tests
├── schema.rs           # All DDL (extension_sql! with tables, indexes, triggers)
├── bootstrap.rs        # Bootstrap/identity initialization
├── identity.rs         # Ed25519 identity management
├── workers.rs          # Background worker stubs
├── functions/
│   ├── mod.rs          # Module declarations
│   └── stubs.rs        # Stub functions for future plans
├── parser/
│   ├── mod.rs          # Public API: parse_crate(), parse_file(), parse_source(), parallel_parse()
│   ├── kinds.rs        # syn type → kind string constants
│   ├── path_builder.rs # ltree path builder
│   ├── metadata.rs     # JSONB metadata extraction from syn items
│   ├── comment_extractor.rs  # Comment scanner
│   ├── cargo_parser.rs # Cargo.toml parser
│   ├── crate_walker.rs # .rs file discovery via walkdir
│   ├── ast_walker.rs   # Recursive AST walker (syn → NodeRow/EdgeRow)
│   ├── inserter.rs     # Batch SPI INSERT (500 rows/batch)
│   ├── go/             # Go parser (tree-sitter-go)
│   ├── c/              # C parser (tree-sitter-c)
│   └── latex/          # LaTeX/BibTeX parser (tree-sitter-latex + biblatex)
│       ├── mod.rs      # pg_extern: parse_latex_{source,file}, parse_bibtex_{source,file}, link_citations
│       ├── kinds.rs    # latex_* and bib_* kind constants
│       ├── metadata.rs # Metadata extractors for LaTeX tree-sitter nodes
│       ├── walker.rs   # Tree-sitter CST walker with section hierarchy + label/ref resolution
│       └── bibtex.rs   # BibTeX parser via biblatex crate
└── bin/
    └── pgrx_embed.rs   # pgrx binary entrypoint
```

### Key Tables
- `kerai.instances` — self-identity (Ed25519 keypair, instance_id)
- `kerai.nodes` — AST nodes (id, instance_id, kind, path, content, metadata, span)
- `kerai.edges` — relationships between nodes
- `kerai.changes` / `kerai.clock` — CRDT change tracking (future)

## Conventions
- All `#[pg_extern]` functions go in their respective module (parser, functions, etc.)
- SQL DDL lives exclusively in `src/schema.rs` via `extension_sql!`
- Tests use `#[pg_test]` and live in `src/lib.rs`
