# Plan 23 — Stack Machine with Typed Pointers

## Context

Kerai's language module currently parses three notation modes (prefix, infix, postfix) into an `Expr` tree and evaluates it as a calculator. The precursor language tau proposed a stack-based system where the stack holds integer IDs pointing to typed server-side objects, with a web-first Jupyter-like UI.

Kerai already has Postgres as its data store and a growing set of parsers that create both real tables (CSV import) and structural knowledge nodes. The natural evolution: **the stack holds typed pointers, and every pointer refers to something in Postgres**. The workspace stack itself is always persisted to Postgres — push `[1 2 3]` without consuming it, come back tomorrow, it's still there.

**Design principle**: Postgres is the universal backbone. All data — tables, nodes, session state, stack contents — lives in Postgres. Import brings data in, export serializes it out. The stack is a persistent workspace, not ephemeral memory.

## Stack Pointer Structure

```rust
struct Ptr {
    kind: &'static str,    // "csv_project", "csv_table", "query_result", "text", "int", "list", ...
    ref_id: String,         // UUID, qualified table name, or literal value — depends on kind
    meta: serde_json::Value, // kind-specific metadata (row counts, column info, etc.)
}
```

The stack is `Vec<Ptr>`, persisted to Postgres as JSONB in the session table. Every mutation to the stack writes through to the session row.

**Inline vs. referenced**: Primitives (int, float, string, list) carry their value directly in `ref_id`/`meta`. Large/persistent data (tables, projects, query results, nodes) store a UUID or qualified name in `ref_id` that points to a Postgres table or row. The `kind` field tells the machine which is which. Either way, the pointer itself is always persisted in the session's stack column — nothing is lost on disconnect.

## How It Works — The csv.import Example

```
kerai> "/path/to/march-machine-learning-mania-2026" csv.import
```

Step by step:

1. **`"/path/to/march-machine-learning-mania-2026"`** — pushes `Ptr { kind: "path", ref_id: "/path/to/...", meta: {} }`.

2. **`csv.import`** — pops the path pointer, inspects kind:
   - `"path"` + directory → runs `parse_csv_dir()`, derives project name from basename
   - `"path"` + `.zip` → extracts to temp dir, runs `parse_csv_dir()` on contents
   - `"path"` + `.csv` → runs `parse_csv_file()`
   - Pushes result: `Ptr { kind: "csv_project", ref_id: "<project-uuid>", meta: { "schema": "kaggle", "tables": 35, "rows": 7213256 } }`

3. **`edit`** — pops the top pointer, dispatches on `kind`:
   - `"csv_project"` → opens project explorer in web UI (table list, row counts, column types)
   - `"csv_table"` → opens data grid editor
   - `"text"` → opens text editor
   - `"query_result"` → opens result table view

Dispatch is fully late-bound. `edit` doesn't know about CSV — it looks up a handler registry keyed by `kind`.

## Stack Persistence

```sql
CREATE TABLE kerai.sessions (
    id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id    TEXT NOT NULL,
    stack      JSONB NOT NULL DEFAULT '[]',  -- serialized Vec<Ptr>
    created_at TIMESTAMPTZ DEFAULT now(),
    updated_at TIMESTAMPTZ DEFAULT now()
);
```

Every stack mutation writes through to this row. Push `[1 2 3]` and walk away — reconnect later and it's still on top of the stack. Since the stack only holds pointers (not bulk data), serialization is trivial. The actual data behind referenced pointers is already in Postgres tables — it doesn't go anywhere.

## Postgres as Universal Backbone

Every piece of data lives in Postgres:

| What | Where |
|------|-------|
| Raw CSV data | `kaggle.m_teams`, etc. (real tables) |
| Project metadata | `kerai.csv_projects`, `kerai.csv_files` |
| Knowledge graph | `kerai.nodes`, `kerai.edges` |
| Session/stack state | `kerai.sessions` |
| User definitions | `kerai.definitions` (new) |
| Named objects | `kerai.objects` (new, generic typed store) |

No external file storage, no in-memory-only state. Crash → restart → reload session → all data still there.

## Import / Export Symmetry

```
kerai> "/data/competition.zip" csv.import       # zip → project in Postgres
kerai> csv.export                                # top of stack (csv_project) → zip file
kerai> "kaggle.m_teams" csv.export               # single table → CSV file
kerai> project.export                            # project → zip with CSVs + node metadata
```

`csv.export` dispatches on what's on the stack:
- `csv_project` → exports all tables as CSVs in a zip
- `csv_table` → exports one table as a CSV
- `query_result` → exports query results as a CSV

`project.export` is broader — includes kerai node/edge metadata alongside raw data, so someone can `project.import` it on their kerai instance and get the full knowledge graph.

## Type Dispatch Table

```rust
type Handler = fn(stack: &mut Vec<Ptr>, db: &PgPool) -> Result<(), Error>;

struct Machine {
    stack: Vec<Ptr>,
    db: PgPool,
    session_id: Uuid,
    handlers: HashMap<String, Handler>,           // "csv.import" → handler_csv_import
    type_methods: HashMap<(String, String), Handler>, // ("csv_project", "edit") → handler_edit_project
}
```

Word resolution order:
1. Check `handlers` for exact match (e.g., `csv.import` always does the same thing)
2. Check `type_methods` keyed by `(top_of_stack.kind, word)` (e.g., `("csv_project", "edit")`)
3. Try parsing as literal (int, float, string, list) and push
4. Error: unknown word

This gives both explicit commands and type-dispatched methods.

## Composable Command Chains

```
kerai> "data.zip" csv.import                          # push csv_project
kerai> "m_teams" table.select                         # pop project, push csv_table for m_teams
kerai> [season teamid] columns.pick                   # pop table, push filtered table view
kerai> edit                                           # open grid editor for the filtered view
kerai> "SELECT * FROM kaggle.m_teams WHERE teamid > 1400" sql  # push query_result
kerai> dup csv.export                                 # export without consuming the result
```

Stack manipulation words: `dup`, `swap`, `drop`, `over`, `rot`, `clear` — standard Forth vocabulary.

## Web UI Integration

The existing Axum server adds:

- **`POST /api/eval`** — sends input text, receives stack as JSON array of `Ptr` objects
- **`GET /api/object/:kind/:ref_id`** — fetches renderable content for a pointer
- **WebSocket `/api/ws`** — live updates (import progress, stack changes)

The browser renders each stack item as a card. Card appearance is determined by `kind`:
- `csv_project` → table list with row counts
- `csv_table` → data preview (first 20 rows)
- `query_result` → full result grid
- `int`/`float`/`text`/`list` → inline display
- `error` → red card

`edit` tells the browser to open an interactive editor for the top card.

## Migration Path from Current Lang Module

1. **Keep** `token.rs` — already handles whitespace-separated words, quoted strings, brackets
2. **Keep** `Expr` tree and Pratt parser — useful for infix arithmetic subexpressions
3. **Replace** `eval.rs` `Value` enum with `Ptr` — stack type changes from `Value` to `Ptr`
4. **Add** `machine.rs` — dispatch loop that processes postfix word streams
5. **Add** handler modules — `handlers/csv.rs`, `handlers/stack.rs` (dup/swap/drop), `handlers/query.rs` (SQL), `handlers/io.rs` (import/export)
6. **Wire** into web server — `/api/eval` endpoint runs the machine

The existing `Value::Int/Float/Str/List` maps directly to pointer kinds:
- `Value::Int(42)` → `Ptr { kind: "int", ref_id: "42", meta: null }`
- `Value::List([1,2,3])` → `Ptr { kind: "list", ref_id: "", meta: [1,2,3] }`

Arithmetic operations pop ptr values, compute, push result. The difference is that *some* pointers refer to large Postgres-resident objects, and the machine knows how to operate on both.

## Evolution from Tau

| Aspect | Tau (original) | Kerai (proposed) |
|--------|---------------|-----------------|
| Stack values | Integer IDs into a Go map | Typed `Ptr` structs with kind/ref/meta |
| Object store | In-memory `map[int64]*Object` | Postgres (tables, nodes, or inline in stack) |
| Language | Go, standalone binary | Rust, Postgres extension + CLI + web |
| GC | Unspecified | Postgres handles it (DROP TABLE, session cleanup) |
| Persistence | `~/.tau` files | Postgres entirely — stack survives disconnects |
| Type dispatch | Unspecified | `(kind, word) → handler` lookup table |
| File format | `.tau` with comment blocks | `.kerai` with notation directives |

## Implementation

### New Files

| File | Role |
|------|------|
| `kerai/src/lang/ptr.rs` | `Ptr` struct, serialization, inline value helpers |
| `kerai/src/lang/machine.rs` | `Machine` struct, dispatch loop, stack persistence |
| `kerai/src/lang/handlers/mod.rs` | Handler trait, registry |
| `kerai/src/lang/handlers/stack.rs` | dup, swap, drop, over, rot, clear |
| `kerai/src/lang/handlers/csv.rs` | csv.import, csv.export |
| `kerai/src/lang/handlers/query.rs` | sql, table.select, columns.pick |
| `kerai/src/lang/handlers/io.rs` | project.export, project.import |
| `kerai/src/serve/routes/eval.rs` | POST /api/eval endpoint |
| `kerai/src/serve/routes/object.rs` | GET /api/object/:kind/:ref_id endpoint |

### Modified Files

| File | Change |
|------|--------|
| `kerai/src/lang/mod.rs` | Add `pub mod ptr;`, `pub mod machine;`, `pub mod handlers;` |
| `kerai/src/lang/eval.rs` | Adapt Value → Ptr for arithmetic ops |
| `kerai/src/serve/mod.rs` | Wire eval/object routes |
| `postgres/src/schema.rs` | Add `kerai.sessions` DDL |

### Schema Additions

```sql
CREATE TABLE kerai.sessions (
    id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id    TEXT NOT NULL,
    stack      JSONB NOT NULL DEFAULT '[]',
    created_at TIMESTAMPTZ DEFAULT now(),
    updated_at TIMESTAMPTZ DEFAULT now()
);

CREATE TABLE kerai.definitions (
    id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    session_id UUID REFERENCES kerai.sessions(id),
    name       TEXT NOT NULL,
    body       TEXT NOT NULL,           -- stored as kerai source text
    created_at TIMESTAMPTZ DEFAULT now(),
    UNIQUE (session_id, name)
);
```

## What This Gets You

1. **Persistent workspace**: push values, disconnect, reconnect — stack is intact
2. **Shareable**: `project.export` gives someone a zip; `project.import` restores it
3. **Queryable**: everything is SQL — `SELECT * FROM kaggle.m_teams WHERE teamid > 1400`
4. **Composable**: stack operations chain — `"data.zip" csv.import "m_teams" table.select [season teamid] columns.pick`
5. **Extensible**: new kinds just need a handler — `go.import`, `rust.import`, `pdf.import` all follow the same pattern
6. **Recoverable**: crash → restart → reload session → all data still there
