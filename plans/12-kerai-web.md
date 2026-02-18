# Plan 12: kerai_web — Knowledge Editor

*Depends on: Plan 01 (Foundation), Plan 02 (Rust Parser), Plan 04 (CRDT Operations), Plan 06 (Distribution), Plan 08 (AI Perspectives)*
*Enables: Plan 10 (ZK Marketplace) adoption, Plan 11 (External Economy) participation*

## Goal

Build a web-based knowledge editor that lets humans write, research, and produce new knowledge — backed by kerai's structured corpus, AI perspectives, and CRDT collaboration. The editor is the first product that makes kerai's infrastructure tangible to people who aren't thinking about AST-based version control. They're thinking about writing and research. The database machinery is invisible.

## Design Principle

The editor is thin. All intelligence lives in the kerai extension — parsing, CRDT operations, perspective queries, knowledge economy. The web layer is a protocol translator: HTTP/WebSocket on one side, SQL on the other. If something can be a `pg_extern` function call, it should be.

This principle has a concrete architectural consequence: the editor can be rebuilt in any framework (React, Svelte, native app) without reimplementing logic. The API surface is SQL. Any Postgres client is a kerai client; the editor just makes it visual and ergonomic.

## The Bridge

### Why Not Serve HTTP From the Extension?

pgrx background workers can run arbitrary Rust code, including binding a TCP socket. But this fights Postgres's design:

- Connection lifecycle managed by Postgres, not the HTTP server
- A crash in HTTP handling takes down a background worker
- TLS, WebSocket upgrades, static file serving — all reimplemented inside the database process
- Postgres's shared memory model wasn't designed for web traffic patterns

Possible but adversarial to the host.

### The Translation Layer

```
Browser (ProseMirror/TipTap rich text editor)
    |
    |  WebSocket + HTTP
    |
Traefik (TLS termination, static files, routing)
    |
    |-- /api/*   -->  kerai_web bridge (HTTP/WS <-> SQL)
    |                     |
    |                     +-- Postgres + kerai extension
    |
    +-- /*       -->  static files (editor JS/CSS/HTML)
```

**The bridge handles:**
- HTTP request → SQL function call translation
- WebSocket ↔ LISTEN/NOTIFY relay for real-time collaboration
- Session management and auth (map browser session to kerai wallet/identity). *Under Plan 20, the session maps to a wallet whose balance is shielded. The bridge never sees the user's balance or commitment inventory — Fuchi runs client-side (browser or native app) and generates proofs locally. The bridge relays proofs to the extension, not amounts.*
- Request validation and rate limiting

**The bridge does NOT handle:**
- Document parsing (extension)
- CRDT operations (extension)
- Perspective computation (extension)
- Knowledge economy transactions (extension)
- Any business logic whatsoever

### Phased Implementation

**Phase 1: Separate binary.** A Rust binary using axum + tokio-postgres. Conventional deployment, easiest to debug, runs as a Docker container behind Traefik. Connects to Postgres over a socket like any other client.

**Phase 2: pgrx extension.** Collapse the bridge into a `kerai_web` extension. Background worker runs the HTTP/WebSocket server. Gains zero-hop SPI access to kerai functions — no network round-trip, no SQL parsing, direct function calls. Latency drops from milliseconds to microseconds for perspective queries.

Phase 2 is viable because:
- The bridge is already Rust (same language as pgrx)
- Connection count is bounded (editor sessions, not public internet)
- Traefik handles TLS and static files, not the extension
- The WebSocket server is purpose-built and minimal

## Deliverables

### 12.1 Document Parser Module

Add document parsing alongside the Rust parser. Same `nodes` and `edges` tables, different `kind` vocabulary.

**Supported input formats (in priority order):**

1. **Markdown** — CommonMark AST is well-defined. Use `pulldown-cmark` (Rust) for parsing. This is the native editing format — what the editor produces.
2. **LaTeX** — explicit structure (`\section`, `\begin{theorem}`, `\ref{fig:1}`). Richest source for technical papers. Parser produces clean AST directly.
3. **PDF extraction** — hardest case. Use GROBID, Nougat, or Marker for structure extraction. AI classification to assign node kinds and edge types where extraction is uncertain. Confidence scores stored in node metadata.
4. **HTML** — many papers have HTML versions (arxiv HTML, PubMed Central). Structure maps relatively cleanly to nodes.

Each parser is a module: `src/parser/markdown/`, `src/parser/latex/`, `src/parser/pdf/`. The `nodes` and `edges` tables don't change. The `kind` vocabulary grows:

```
-- Document node kinds
paper, book, chapter, part, section, subsection, paragraph, sentence,
title, abstract, keyword, author, affiliation,
theorem, lemma, corollary, definition, proof,
equation, algorithm, code_listing,
figure, table, caption,
footnote, endnote, appendix,
bib_entry, glossary_entry, index_entry,
blockquote, list, list_item
```

### 12.2 Edge Types for Technical Content

Typed edges that make cross-document queries possible:

```
-- Citation edges
cites, extends, contradicts, reproduces, uses_dataset_from, compares_to

-- Internal reference edges
cross_references, footnote_of, caption_of, depicts_data, label_ref

-- Semantic edges
formalizes, proves, defines, elaborates, quotes, exemplifies

-- Structural edges (implicit from tree, explicit when cross-cutting)
belongs_to, precedes, follows
```

### 12.3 Bridge API

The HTTP/WebSocket API that the editor frontend talks to. Every endpoint maps to one or more `pg_extern` calls.

**Document operations:**
```
POST   /api/nodes              -- insert_node(kind, path, content, metadata)
PATCH  /api/nodes/:id          -- update_node(id, content, metadata)
DELETE /api/nodes/:id          -- delete_node(id)
POST   /api/nodes/:id/move     -- move_node(id, new_parent, position)
POST   /api/edges              -- create_edge(kind, source_id, target_id)
DELETE /api/edges/:id          -- delete_edge(id)
```

**Query operations:**
```
GET    /api/nodes?path=...     -- query nodes by ltree path
GET    /api/nodes/:id/edges    -- edges for a node
GET    /api/search?q=...       -- full-text search (GIN index)
GET    /api/perspectives       -- perspective queries
GET    /api/corpus/search      -- cross-document structural search
```

**Collaboration:**
```
WS     /api/ws                 -- WebSocket for real-time ops relay
GET    /api/version            -- current version vector
GET    /api/history/:node_id   -- operation history for a node
```

**Corpus management:**
```
POST   /api/ingest             -- parse and ingest a document (PDF, LaTeX, MD)
GET    /api/corpus             -- list ingested documents
GET    /api/corpus/:id/tree    -- document's node tree
```

### 12.4 Real-Time Collaboration

Uses Postgres LISTEN/NOTIFY, relayed through the bridge's WebSocket:

1. User A types a paragraph → editor sends `POST /api/nodes` → bridge calls `kerai.insert_node()` → extension applies CRDT operation → triggers `NOTIFY kerai_ops, '{op_json}'`
2. Bridge subscribes to `kerai_ops` → receives notification → relays via WebSocket to all connected editors
3. User B's editor receives the operation → applies it to local document state

The CRDT guarantee means operations commute. The bridge doesn't resolve conflicts — there are none. It just relays.

For the Phase 2 extension approach, LISTEN/NOTIFY is replaced by direct shared memory communication between the HTTP worker and the kerai extension — even lower latency.

### 12.5 AI-Assisted Writing

The editor's distinguishing feature. As the user writes, the system surfaces relevant structure from the corpus. This is NOT autocomplete — it's structural awareness.

**How it works:**

1. User writes a paragraph. The editor sends the content to the bridge.
2. Bridge calls `kerai.identify_context(content)` — the extension extracts key concepts, terms, and claims from the text.
3. Bridge calls `kerai.query_perspectives(context, agent_ids)` — returns pre-computed perspective weights for relevant nodes across the corpus.
4. Results streamed back to the editor as sidebar suggestions:

```
┌──────────────────────────────────────┬─────────────────────────────────┐
│                                      │ CONNECTIONS                     │
│  Your document                       │                                 │
│                                      │ ▸ Theorem 3 in Kleppmann 2019  │
│  ## 2. System Model                  │   proves this for trees, but    │
│                                      │   your claim is about DAGs.     │
│  We consider a network of N nodes    │   Gap identified.               │
│  under partial synchrony, where      │   [technique_matcher: 0.92]     │
│  messages are delivered within a     │                                 │
│  known but unbounded time...█        │ ▸ Lemma 4 in Attiya 2015       │
│                                      │   proves equivalent property    │
│                                      │   under partial synchrony.      │
│                                      │   Unused in CRDT literature.    │
│                                      │   [gap_finder: 0.87]           │
│                                      │                                 │
│                                      │ ▸ 3 papers test only to 1K     │
│                                      │   nodes. Your 100K regime is   │
│                                      │   uncharted.                    │
│                                      │   [data_synthesizer: 0.78]     │
│                                      │                                 │
│                                      │ DATA                            │
│                                      │ ▸ Table 2 (Kleppmann 2019)     │
│                                      │ ▸ Table 4 (Shapiro 2011)       │
│                                      │ ▸ Figure 7 (Preguiça 2018)    │
└──────────────────────────────────────┴─────────────────────────────────┘
```

**Key architectural points:**
- Suggestions are database queries, not LLM inference. The perspectives are pre-computed (Plan 08). Latency is milliseconds.
- When the user accepts a suggestion (clicks to cite, incorporates a connection), the system records provenance edges: new_node → edge:enabled_by → source_node, edge:suggested_by → agent.
- LLM inference is used for two things: (a) initial context extraction from the user's text, and (b) periodic perspective recomputation by agents over the corpus. Neither is in the real-time editing path.

### 12.6 Corpus Ingestion Pipeline

Bulk import of technical literature into the kerai database:

```bash
# Ingest a single PDF
curl -X POST http://editor.primal.host/api/ingest \
  -F "file=@paper.pdf" -F "format=pdf"

# Ingest a directory of LaTeX sources
curl -X POST http://editor.primal.host/api/ingest \
  -F "path=/papers/crdt-survey/" -F "format=latex"

# Ingest from arxiv (fetch + parse)
curl -X POST http://editor.primal.host/api/ingest \
  -F "url=https://arxiv.org/abs/2012.00472" -F "format=pdf"
```

After ingestion:
1. Document is parsed into nodes/edges
2. Perspective agents are triggered to compute weights over the new content
3. New nodes become available to all editors querying the corpus

### 12.7 Provenance Tracking

Every piece of knowledge in the editor has traceable origins:

- **Human-authored nodes** — created_by edge to the author's wallet/identity
- **AI-suggested connections** — suggested_by edge to the agent, with the perspective weight and reasoning that triggered the suggestion
- **Accepted suggestions** — enabled_by edge from the new content to the source nodes the AI surfaced
- **Corpus-derived claims** — cites edges to the specific nodes (theorem, table, figure) in the source papers

This provenance chain is what makes the knowledge economy (Plans 10-11) work at the editor level. A novel connection between two papers — identified by an AI agent, validated and incorporated by a human — has traceable, auditable origins. Its reproduction cost is measurable: how long would it take another agent to independently find this connection?

*Under Plan 20, provenance edges are public data (they're structural, not financial). But the economic transactions triggered by provenance — bounty payouts for novel connections, auction earnings for valuable perspectives — flow through the private ledger. The editor shows provenance; Fuchi manages the economics.*

## Frontend

### Editor Framework

The browser-side editor needs:
- **Rich text editing** with structural awareness (not just formatting, but node-level operations)
- **Real-time collaboration** via CRDT operations over WebSocket
- **Sidebar panels** for AI suggestions, corpus search, provenance view
- **Multiple output formats** — the same document reconstructs to Markdown, HTML, LaTeX, PDF

**Proposed: ProseMirror** (or TipTap, which wraps ProseMirror).

ProseMirror is document-model-first — it defines a schema of node types and their nesting rules, then renders them. This maps directly to kerai's node kinds. A ProseMirror "transaction" (insert node, delete node, replace content) maps 1:1 to kerai CRDT operations. The editor's internal document model and kerai's database model are structurally isomorphic.

### Deployment

```yaml
# docker-compose.yml (Phase 1)
services:
  kerai-web:
    build: .
    image: primal-host-kerai-web
    container_name: kerai-web
    environment:
      DATABASE_URL: postgres://postgres:password@host.docker.internal:5432/kerai
    networks:
      - infra
    labels:
      - "traefik.enable=true"
      - "traefik.http.routers.kerai-web.rule=Host(`editor.primal.host`)"
      - "traefik.http.routers.kerai-web.tls.certresolver=letsencrypt"
```

Follows the naming conventions from CLAUDE.md: image `primal-host-kerai-web`, container `kerai-web`.

## Decisions to Make

- **Editor framework:** ProseMirror vs Slate vs CodeMirror 6. ProseMirror's document model maps most naturally to kerai's node structure. Slate is more React-native. CodeMirror is better for code-heavy editing. Proposed: ProseMirror (via TipTap for ergonomics).
- **Phase 1 bridge language:** Rust (axum) vs Go. Rust keeps the entire stack in one language and simplifies Phase 2 migration to pgrx. Go is faster to prototype. Proposed: Rust, since the Phase 2 target is a pgrx extension anyway.
- **PDF parser:** GROBID (Java, mature, academic standard) vs Nougat (Python, Meta, neural) vs Marker (Python, newer). Proposed: start with Marker for speed, validate against GROBID for accuracy, run as a sidecar service called by the bridge.
- **Perspective trigger:** Should AI agents recompute perspectives on every new paragraph, or on save/commit? Real-time is more responsive but more expensive. Proposed: debounced — recompute 2 seconds after the user stops typing, using only fast perspective queries (pre-computed weights). Full agent recomputation runs on save.
- **Auth model:** Kerai wallets have Ed25519 keypairs. Browser sessions need something more conventional. Proposed: standard session cookies mapping to wallet IDs. The bridge handles session ↔ wallet translation. WebAuthn is a future option (hardware key signs operations directly).
- **Corpus access control:** Should all editors see all ingested papers? Or per-user/per-team corpora? Proposed: shared corpus by default (knowledge wants to be open). Private annotations via perspectives (your weights are yours, the nodes are shared).

## Out of Scope

- Mobile-native editor (web-first, mobile can use responsive design)
- Offline editing (requires local Postgres or a client-side CRDT engine — future work)
- Video/audio content parsing (stick to text, equations, figures, tables)
- Building the AI agents themselves (Plan 08 defines the perspective infrastructure; agent implementation is separate work)
- Marketplace UI (Plan 10 has its own interface needs)
