# Plan 23 — Stack Machine with Typed Pointers

## Context

Kerai's language module currently parses three notation modes (prefix, infix, postfix) into an `Expr` tree and evaluates it as a calculator. The precursor language tau proposed a stack-based system where the stack holds integer IDs pointing to typed server-side objects, with a web-first Jupyter-like UI.

Kerai already has Postgres as its data store and a growing set of parsers that create both real tables (CSV import) and structural knowledge nodes. The natural evolution: **the stack holds typed pointers, and every pointer refers to something in Postgres**. The workspace stack itself is always persisted to Postgres — push `[1 2 3]` without consuming it, come back tomorrow, it's still there.

**Design principle**: Postgres is the universal backbone. All data — tables, nodes, session state, stack contents — lives in Postgres. Import brings data in, export serializes it out. The stack is a persistent workspace, not ephemeral memory.

**Design principle**: Postgres all the way down. The interpreter state *is* the database. The stack, word definitions, dispatch table, object store — everything starts as Postgres rows and SQL operations. If a specific operation proves too slow (tight arithmetic loops, hot stack manipulation), promote just that piece to Rust. The database is the default; Rust is the escape hatch. Since pgrx runs Rust inside the Postgres process, a word like `+` can start as SQL and become a Rust function without changing its interface to the rest of the machine. Object IDs use Postgres sequences — stable, never renumbered, gaps are permanent. A pointer to object 42 is always object 42.

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

The stack lives in `kerai.stack_items` — one row per item, with a stable rowid from a Postgres sequence (see Authentication & Workspaces section for full schema). Each user has multiple named workspaces, each with its own stack items.

Every stack mutation is a database operation: push = INSERT, pop = DELETE, dup = INSERT with same content. Rowids are permanent — delete row 5, and the next push gets a new higher ID, never 5 again. Push `[1 2 3]` and walk away — reconnect later and it's still there, same rowid, same position.

Since each stack item only holds a pointer (kind + ref_id + meta), rows are small. The actual data behind referenced pointers is already in Postgres tables — it doesn't go anywhere.

## Postgres as Universal Backbone

Every piece of data lives in Postgres:

| What | Where |
|------|-------|
| Raw CSV data | `kaggle.m_teams`, etc. (real tables) |
| Project metadata | `kerai.csv_projects`, `kerai.csv_files` |
| Knowledge graph | `kerai.nodes`, `kerai.edges` |
| Users & workspaces | `kerai.users`, `kerai.workspaces` |
| Stack rows | `kerai.stack_items` (rowid sequence, per-workspace) |
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

## Libraries as Namespaces

The current CLI's top-level command groups (`postgres`, `wallet`, `swarm`, `model`, `csv`, etc.) become **libraries** — namespaces of words. Each library name is itself a word that pushes a library pointer onto the stack.

### Dot Form vs. Space Form

Both forms resolve to the same handler:

```
postgres.ping          # dot form — direct call, single token
postgres ping          # space form — push library, then dispatch "ping" on it
```

The dot form is compact for scripts. The space form reads naturally on the command line and in conversation.

### Argument Placement Rule

**Arguments go before the word that consumes them.** This is the fundamental postfix principle applied consistently:

- **Library configuration** (connection strings, hosts) goes before the library word
- **Action arguments** (patterns, table names, counts) go before the action word

```
"123.12.1.0" postgres ping                  # address → postgres, ping takes nothing
"123.12.1.0" postgres "%parse%" find        # address → postgres, pattern → find
postgres "%parse%" find                      # default postgres, pattern → find
postgres ping                                # default postgres, ping takes nothing
"prod-db" postgres "m_teams" 10 rows        # connect to prod, get 10 rows from m_teams
```

Each word consumes exactly what belongs to it. Library words consume configuration. Action words consume action arguments. Reads left-to-right as "configure, then act."

### Library Pointer Dispatch

When the machine encounters a library name like `postgres`:

1. Check the stack top — if there's an address/connection string, pop it and bind
2. Push `Ptr { kind: "library", ref_id: "postgres", meta: { "host": "..." } }`

When the next word (e.g., `ping`) executes:

1. See `kind: "library"` on stack top → look up `("library:postgres", "ping")` in type_methods
2. Resolves to the same handler as the dot form `postgres.ping`

### Aliases

Libraries can be aliased like any definition:

```
pg: postgres
pg ping                    # same as postgres ping
pg.ping                    # same as postgres.ping
```

### Shell Invocation

Everything after `kerai` on the command line is input to the postfix interpreter:

```bash
kerai postgres ping                         # evaluate "postgres ping"
kerai "123.12.1.0" postgres ping            # evaluate with remote host
kerai "/path/to/dir" csv import             # evaluate csv import
kerai 1 2 +                                 # evaluate arithmetic
kerai                                       # no args → REPL
```

The shell's word splitting tokenizes for free. Quoted strings pass through as single tokens. This **replaces the clap subcommand tree entirely** — the kerai binary joins `argv[1..]` into a postfix expression, evaluates it, prints the stack, and exits.

### Current CLI Groups → Libraries

| Current CLI | Library | Example Words |
|------------|---------|---------------|
| `kerai postgres <action>` | `postgres` | `ping`, `find`, `query`, `tree`, `refs`, `import`, `export` |
| `kerai postgres import-csv` | `csv` | `import`, `export` |
| `kerai wallet <action>` | `wallet` | `create`, `balance`, `transfer`, `history` |
| `kerai currency <action>` | `currency` | `register`, `transfer`, `supply`, `schedule` |
| `kerai market <action>` | `market` | `create`, `bid`, `settle`, `browse`, `stats` |
| `kerai bounty <action>` | `bounty` | `create`, `list`, `show`, `claim`, `settle` |
| `kerai swarm <action>` | `swarm` | `launch`, `status`, `stop`, `leaderboard` |
| `kerai model <action>` | `model` | `create`, `train`, `predict`, `search`, `info` |
| `kerai agent <action>` | `agent` | `add`, `list`, `remove`, `info` |
| `kerai peer <action>` | `peer` | `add`, `list`, `remove`, `info` |
| `kerai config <action>` | `config` | `get`, `set`, `list`, `delete` |
| `kerai alias <action>` | `alias` | `get`, `set`, `list`, `delete` |

Generic words that dispatch on stack type rather than belonging to a namespace: `edit`, `export`, `import`, `info`, `list`, `show`, `delete`.

## Composable Command Chains

```
kerai> "data.zip" csv.import                          # push csv_project
kerai> "m_teams" table.select                         # pop project, push csv_table for m_teams
kerai> [season teamid] columns.pick                   # pop table, push filtered table view
kerai> edit                                           # open grid editor for the filtered view
kerai> "SELECT * FROM kaggle.m_teams WHERE teamid > 1400" sql  # push query_result
kerai> dup csv.export                                 # export without consuming the result
```

Stack manipulation words: `dup`, `swap`, `drop`, `over`, `rot`, `clear` — standard Forth vocabulary. All accept an optional rowid prefix for direct access: `123 dup` copies row 123 to the top (see Web UI Integration for rowid display).

## Web UI Integration

The existing Axum server adds:

- **`POST /api/eval`** — sends input text, receives stack as JSON array of `Ptr` objects
- **`GET /api/object/:kind/:ref_id`** — fetches renderable content for a pointer
- **WebSocket `/api/ws`** — live updates (import progress, stack changes)

### Stack Display

Each stack row has a stable rowid (Postgres sequence — never renumbered, gaps are permanent). The UI displays the stack as a vertical list with rowids on the left:

```
   1  "hello world"
   2  42
   3  [1 2 3 4 5 6 7 8 9 10 11 12 13 14 15...
   4  kaggle.m_teams (381 rows, 4 cols)
   5  march-machine-learning-mania-2026 (35 tables, 7.2M rows)
```

**Layout:**
- Rowid: right-aligned in a ~100px left column, dim/muted color (e.g., `color: #666`)
- Space separator between rowid and content
- Content: rendered representation of the pointer, NOT raw JSON — each `kind` has a display formatter
- Content clipped to viewport width with ellipsis for overflow

**Display formatters by kind:**
- `text` → character string, clipped to width
- `int`/`float` → literal value
- `list` → `[1 2 3 4 5...]` with truncation
- `csv_project` → `project-name (N tables, M rows)`
- `csv_table` → `schema.table (N rows, M cols)`
- `query_result` → `query result (N rows)`
- `workspace_list` → numbered selection list (see Workspace Management)
- `library` → `postgres` / `csv` / etc.
- `error` → red text

### Direct Stack Manipulation by Rowid

The rowid is not just a display element — it's a direct manipulation handle. Users can reference any stack item by its rowid:

```
kerai> 123 dup          # copy row 123 to the top of the stack
kerai> 123 drop         # remove row 123 from the stack
kerai> 123 view         # expand row 123 — show full content, no truncation
kerai> 123 edit         # open row 123 in the appropriate editor
kerai> 123 45 swap      # swap rows 123 and 45
```

This extends the standard Forth stack vocabulary (which only operates on the top few items) to allow random access by stable ID. Traditional `dup`/`swap`/`drop` with no rowid prefix operate on the top of the stack as usual.

### View Expansion

`view` expands a stack item to show its full content — the untruncated text, the full list, the complete table schema, etc. The web UI replaces the clipped single-line display with an expanded card view. `view` without a rowid expands the top of the stack.

### Anonymous Sessions

Users who haven't logged in get a random-named workspace (e.g., `anon-7f3a2b`) marked for discard. The workspace functions normally — the stack persists, rowids are stable — but the workspace is cleaned up when the browser session ends or the user logs in and switches to a real workspace. If the user logs in during an anonymous session, they can `workspace save "my-project"` to adopt it before it's discarded.

### Card Rendering

For complex types, the web UI renders richer cards instead of single-line summaries:
- `csv_project` → table list with row counts (expandable)
- `csv_table` → data preview (first 20 rows)
- `query_result` → full result grid
- `workspace_list` → numbered selection list

`edit` tells the browser to open an interactive editor for the top card.

## Authentication & Workspaces (First Implementation Target)

This is the first end-to-end implementation — a top-down UX flow that exercises the stack machine, web UI, typed pointer rendering, and Postgres persistence. It guides all subsequent design decisions by making the system real and interactive.

### Login Flow

```
kerai> login bsky
```

1. `login` pushes `Ptr { kind: "library", ref_id: "login" }`
2. `bsky` dispatches — initiates AT Protocol OAuth (DPoP-based), browser redirects to Bluesky authorize page
3. Callback returns with user's DID, session binds to authenticated user
4. Pushes `Ptr { kind: "session", ref_id: "did:plc:abc123", meta: { "handle": "you.bsky.social", "provider": "bsky" } }`
5. Web UI sees `kind: "session"` and renders a user card (handle, avatar, provider)

Auth providers are pluggable — `login bsky`, `login github`, etc. Each is a word in the `login` library. The session table tracks which provider authenticated the user.

### Workspace Management

Once authenticated, the user has workspaces — named, persistent stacks:

```
kerai> workspace list
```

1. `workspace` pushes the library pointer
2. `list` queries `kerai.sessions WHERE user_id = current_did`, pushes `Ptr { kind: "workspace_list", meta: { items: [...] } }`
3. Web UI sees `kind: "workspace_list"` and renders a numbered selection card:
   ```
   1. march-madness-2026    (3 items, last used 2h ago)
   2. kerai-dev             (12 items, last used yesterday)
   3. tax-analysis          (empty, created last week)
   ```

```
kerai> 5 workspace load
```

1. `5` pushes `Ptr { kind: "int", ref_id: "5" }`
2. `workspace` sees an int on the stack, holds it, pushes bound library
3. `load` pops the library (with bound index), pops the workspace list, selects item 5, replaces the current stack with that workspace's saved stack

Other workspace words:
- `workspace new "project-name"` — create a fresh workspace
- `workspace save` — persist the current stack (auto-saves on every mutation anyway)
- `workspace delete` — delete a workspace
- `workspace rename "new-name"` — rename the current workspace

### Numbered List Selection Pattern

The `workspace_list` kind establishes a general pattern: any list-type pointer can be rendered as a numbered list by the web UI, and items are selected by pushing an int before the action word. This pattern reuses everywhere:

```
kerai> postgres "nodes" 10 list     # push a numbered list of 10 nodes
kerai> 3 show                       # show details of item 3
```

### Schema

```sql
CREATE TABLE kerai.users (
    id             UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    did            TEXT UNIQUE,                  -- "did:plc:abc123"
    handle         TEXT,                         -- "you.bsky.social"
    auth_provider  TEXT NOT NULL,                -- "bsky", "github"
    auth_token     TEXT,                         -- encrypted refresh token
    created_at     TIMESTAMPTZ DEFAULT now(),
    last_login     TIMESTAMPTZ DEFAULT now()
);

CREATE TABLE kerai.workspaces (
    id             UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id        UUID NOT NULL REFERENCES kerai.users(id),
    name           TEXT NOT NULL,
    is_active      BOOLEAN DEFAULT false,
    is_anonymous   BOOLEAN DEFAULT false,        -- true for not-yet-logged-in sessions
    created_at     TIMESTAMPTZ DEFAULT now(),
    updated_at     TIMESTAMPTZ DEFAULT now(),
    UNIQUE (user_id, name)
);

CREATE SEQUENCE kerai.stack_item_id_seq;

CREATE TABLE kerai.stack_items (
    id             BIGINT PRIMARY KEY DEFAULT nextval('kerai.stack_item_id_seq'),
    workspace_id   UUID NOT NULL REFERENCES kerai.workspaces(id) ON DELETE CASCADE,
    position       INTEGER NOT NULL,             -- ordering within the stack (top = highest)
    kind           TEXT NOT NULL,                 -- "text", "int", "csv_project", etc.
    ref_id         TEXT NOT NULL DEFAULT '',      -- UUID, qualified name, or literal value
    meta           JSONB NOT NULL DEFAULT '{}',
    created_at     TIMESTAMPTZ DEFAULT now()
);

CREATE INDEX ON kerai.stack_items (workspace_id, position);
```

The stack is no longer a JSONB column — each item is a row with a stable `id` from a Postgres sequence. The `id` is the rowid displayed in the UI and used for direct manipulation (`123 dup`, `123 view`). IDs are never reused or renumbered. The `position` column determines stack order (top = highest position). Pushing appends at the highest position; popping removes the highest.

Separated `users` from `workspaces`. One user has many workspaces. One workspace is active at a time per user. The active workspace's stack items are what the web UI displays and the interpreter operates on.

### Auth Endpoints

- **`GET /auth/bsky`** — initiate AT Protocol OAuth, redirect to Bluesky
- **`GET /auth/callback/bsky`** — handle OAuth callback, create/update user, set session cookie, redirect to app
- **`GET /auth/me`** — return current user info (or 401)
- **`POST /auth/logout`** — clear session

### Web UI Flow

1. User visits `kerai.primal.host` — if no session cookie, show login screen with "Sign in with Bluesky" button
2. OAuth flow completes → redirect back to app with session cookie
3. App loads, calls `GET /auth/me` to get user info
4. Calls `POST /api/eval` with `workspace list` to show workspaces
5. User types `1 workspace load` (or clicks workspace #1) → stack loads
6. From here, normal stack machine interaction — type words, see results as cards

### Implementation Files

| File | Role |
|------|------|
| `kerai/src/serve/auth/mod.rs` | Auth routes, session middleware |
| `kerai/src/serve/auth/bsky.rs` | AT Protocol OAuth flow (DPoP) |
| `kerai/src/lang/handlers/login.rs` | `login` library words |
| `kerai/src/lang/handlers/workspace.rs` | `workspace` library words |
| `kerai/src/serve/static/index.html` | Login screen + main app shell |
| `kerai/src/serve/static/app.js` | Stack rendering, card display, input handling |

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
| `kerai/src/lang/handlers/login.rs` | `login` library — bsky, github auth words |
| `kerai/src/lang/handlers/workspace.rs` | `workspace` library — list, load, new, save, delete |
| `kerai/src/lang/handlers/csv.rs` | csv.import, csv.export |
| `kerai/src/lang/handlers/query.rs` | sql, table.select, columns.pick |
| `kerai/src/lang/handlers/io.rs` | project.export, project.import |
| `kerai/src/serve/auth/mod.rs` | Auth routes, session cookie middleware |
| `kerai/src/serve/auth/bsky.rs` | AT Protocol OAuth flow (DPoP) |
| `kerai/src/serve/routes/eval.rs` | POST /api/eval endpoint |
| `kerai/src/serve/routes/object.rs` | GET /api/object/:kind/:ref_id endpoint |
| `kerai/src/serve/static/index.html` | Login screen + main app shell |
| `kerai/src/serve/static/app.js` | Stack rendering, card display, input handling |

### Modified Files

| File | Change |
|------|--------|
| `kerai/src/lang/mod.rs` | Add `pub mod ptr;`, `pub mod machine;`, `pub mod handlers;` |
| `kerai/src/lang/eval.rs` | Adapt Value → Ptr for arithmetic ops |
| `kerai/src/serve/mod.rs` | Wire eval/object routes |
| `postgres/src/schema.rs` | Add `kerai.users`, `kerai.workspaces`, `kerai.definitions` DDL |

### Schema Additions

```sql
-- See "Authentication & Workspaces" section for kerai.users and kerai.workspaces

CREATE TABLE kerai.definitions (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id UUID REFERENCES kerai.workspaces(id),
    name         TEXT NOT NULL,
    body         TEXT NOT NULL,           -- stored as kerai source text
    created_at   TIMESTAMPTZ DEFAULT now(),
    UNIQUE (workspace_id, name)
);
```

## What This Gets You

1. **Persistent workspace**: push values, disconnect, reconnect — stack is intact
2. **Shareable**: `project.export` gives someone a zip; `project.import` restores it
3. **Queryable**: everything is SQL — `SELECT * FROM kaggle.m_teams WHERE teamid > 1400`
4. **Composable**: stack operations chain — `"data.zip" csv.import "m_teams" table.select [season teamid] columns.pick`
5. **Extensible**: new kinds just need a handler — `go.import`, `rust.import`, `pdf.import` all follow the same pattern
6. **Recoverable**: crash → restart → reload session → all data still there
