# Musings 2: Plain Text, Technical PDFs, and the Knowledge Editor

*2026-02-16 — Exploring how non-code content fits kerai's AST model, and what products that enables.*

---

## 1. How Plain Text (Books) Fit an AST Structure

Prose already has structure — we just flatten it into strings and forget.

### A Book's Natural Tree

A book isn't flat text any more than a Rust program is a flat string. It has a hierarchy:

```
book
├── frontmatter
│   ├── title_page
│   ├── dedication
│   └── preface
├── part: "Part I"
│   ├── chapter: "The Problem"
│   │   ├── section: "Background"
│   │   │   ├── paragraph: "The field of..."
│   │   │   ├── paragraph: "Previous work..."
│   │   │   └── footnote: "See also..."
│   │   └── section: "Our Approach"
│   │       ├── paragraph: "We propose..."
│   │       └── blockquote: "As Knuth observed..."
│   └── chapter: "Prior Art"
│       └── ...
├── part: "Part II"
│   └── ...
└── backmatter
    ├── bibliography
    │   ├── entry: "Knuth1997"
    │   └── entry: "Lamport1978"
    └── index
```

This maps directly to the existing `nodes` table — same columns, just different `kind` values (`chapter`, `paragraph`, `sentence` instead of `function`, `struct`, `impl`). The `path` column (ltree) captures the hierarchy: `book.part_1.ch_2.sec_1.para_3`.

### Edges Make It a Graph, Not Just a Tree

The tree is the skeleton. Edges are where it gets interesting:

| Edge type | Example |
|-----------|---------|
| `cites` | paragraph → bibliography entry |
| `footnote_of` | footnote → paragraph |
| `cross_references` | "as discussed in Chapter 3" → that section node |
| `quotes` | blockquote → source |
| `defines` | first use of a term → glossary entry |
| `contradicts` | paragraph in ch.7 → claim in ch.2 |
| `elaborates` | section → earlier paragraph it expands on |

In plain text, "See Chapter 3" is just a string. In the AST, it's a typed edge you can traverse, query, and validate ("does the target still exist?").

### What This Complexity Buys You

**Structural queries instead of text search.** Plain text gives you grep. The AST gives you:

```sql
-- Which chapters cite Lamport?
SELECT DISTINCT c.content
FROM kerai.edges e
JOIN kerai.nodes ref ON e.target_id = ref.id
JOIN kerai.nodes c ON c.path @> e.source_path AND c.kind = 'chapter'
WHERE ref.kind = 'bib_entry' AND ref.content LIKE 'Lamport%';

-- Sections that were rewritten most between drafts
SELECT n.path, count(*) as revisions
FROM kerai.operations o
JOIN kerai.nodes n ON o.node_id = n.id
WHERE n.kind = 'section'
GROUP BY n.path ORDER BY revisions DESC;

-- Orphaned footnotes (reference target was deleted)
SELECT f.path, f.content
FROM kerai.nodes f
JOIN kerai.edges e ON e.source_id = f.id AND e.kind = 'footnote_of'
LEFT JOIN kerai.nodes target ON e.target_id = target.id
WHERE target.id IS NULL;
```

**Operations are semantic, not textual.** Moving a section from Chapter 3 to Chapter 5 is one `node_move` operation — not a 200-line delete/insert diff. The operation log reads like an editorial history:

- "Moved 'Background' from ch.3 to ch.5"
- "Split paragraph 4.2.7 into two paragraphs"
- "Added 3 citations to section 6.1"

vs git's "142 insertions, 138 deletions across 2 files."

**AI perspectives become precise.** An agent reading your book doesn't just say "chapter 5 is relevant." It weights individual nodes:

- paragraph 5.2.3 → weight 0.95, reasoning: "Core argument for the thesis"
- paragraph 5.2.4 → weight 0.1, reasoning: "Transitional, no unique content"
- section 8.1 → weight -0.3, reasoning: "Contradicts claim in 5.2.3, may need reconciliation"

You can then query: "Show me what two different AI reviewers disagree about" — that's a JOIN across their perspective tables.

**Multi-author collaboration via CRDTs.** Editor restructures Part II while author rewrites Chapter 3. These are operations on different subtrees — CRDTs merge them without conflict.

**The knowledge economy works at node granularity.** A particularly well-crafted explanation of a difficult concept — that specific section node — has independent value. It can be attested, auctioned, and eventually open-sourced through the marketplace.

**Format-agnostic reconstruction.** The AST doesn't care about output format. The same tree reconstructs to Markdown, HTML, LaTeX, or EPUB — just different renderers.

### The Parser Question

For Rust you have `syn`. For prose you'd need a document parser — but these exist: CommonMark has a well-defined AST, Pandoc defines an internal AST that bridges dozens of formats, and even LaTeX has parsable structure. The parser is a module, and you'd add `src/parser/markdown/` or `src/parser/latex/` alongside the Rust parser.

The `nodes` and `edges` tables don't change. The `kind` vocabulary grows. That's it.

### The Short Answer

The "complexity" is recognizing structure that was always there. A book has chapters, sections, paragraphs, citations, and cross-references — those are real things with real relationships. Storing them as a flat string throws away that structure and forces every consumer to re-parse it. Storing them as nodes and edges makes the structure queryable, versionable, and addressable — the same gains code gets from AST representation.

---

## 2. Extending to Technical PDFs

Technical PDFs are where structure goes to die — and recovering it is enormously valuable.

### The Problem With Technical PDFs

A PDF is a *rendering format*. It stores instructions like "draw glyph 'A' at coordinates (72.4, 841.2)." The structure — sections, equations, figure-caption relationships, citation links — existed in the LaTeX or Word source but was flattened into paint commands during compilation. Every consumer of that PDF has to reverse-engineer the structure, and most don't bother.

A research paper with 50 citations, 12 figures, 8 equations, and 4 tables is, to most tools, just a long string.

### The AST of a Technical PDF

```
paper
├── metadata
│   ├── title: "Efficient CRDT Sync for Large-Scale Systems"
│   ├── authors: [node per author, with affiliation edges]
│   ├── abstract: paragraph node
│   ├── keywords: [node per keyword]
│   └── doi: "10.1145/..."
├── section: "1. Introduction"
│   ├── paragraph: "Conflict-free replicated..."
│   ├── paragraph: "Prior approaches [1,2,3]..."
│   │   ├── ── edge:cites → bib.entry_1
│   │   ├── ── edge:cites → bib.entry_2
│   │   └── ── edge:cites → bib.entry_3
│   └── paragraph: "Our contribution is..."
├── section: "2. Background"
│   ├── subsection: "2.1 System Model"
│   │   └── definition: "Definition 1 (Operation)"
│   │       └── equation: "op = (author, seq, lamport_ts, payload)"
│   │           metadata: { format: "latex", raw: "op = \\langle..." }
│   └── subsection: "2.2 Convergence"
│       ├── theorem: "Theorem 1"
│       │   ├── statement: paragraph
│       │   └── proof: [paragraph nodes]
│       └── equation: "forall o1, o2: apply(o1, apply(o2, S)) = apply(o2, apply(o1, S))"
│           ├── ── edge:formalizes → theorem_1
│           └── metadata: { label: "eq:commutativity" }
├── section: "3. Algorithm"
│   ├── algorithm: "Algorithm 1: Sync Protocol"
│   │   ├── input: "Version vectors V_a, V_b"
│   │   ├── step: "compute delta = V_b - V_a"
│   │   ├── step: "for each op in delta..."
│   │   └── output: "Updated state S'"
│   └── code_listing: "Listing 1: Rust implementation"
│       ├── content: "fn sync(local: &VV, remote: &VV)..."
│       └── metadata: { language: "rust", lines: 24 }
├── section: "4. Evaluation"
│   ├── figure: "Figure 1: Throughput vs. agent count"
│   │   ├── image: [binary reference or extracted data]
│   │   ├── caption: "Throughput scales linearly..."
│   │   ├── ── edge:depicts_data → table_1
│   │   └── metadata: { x_axis: "agents", y_axis: "ops/sec", type: "line_chart" }
│   ├── table: "Table 1: Benchmark Results"
│   │   ├── columns: ["Agents", "Ops/sec", "P99 latency", "Convergence time"]
│   │   ├── row: [10, 12400, "2.3ms", "0.8s"]
│   │   ├── row: [100, 11800, "4.1ms", "1.2s"]
│   │   ├── row: [1000, 10200, "12ms", "3.4s"]
│   │   └── row: [10000, 9800, "34ms", "8.1s"]
│   └── paragraph: "As shown in Figure 1 and Table 1..."
│       ├── ── edge:references → figure_1
│       └── ── edge:references → table_1
├── section: "5. Related Work"
│   └── paragraph: "Shapiro et al. [4] introduced..."
│       └── ── edge:compares_to → bib.entry_4
└── bibliography
    ├── entry_1: { authors, title, venue, year, doi }
    ├── entry_2: ...
    └── entry_4: ...
```

### What's Different From a Book

A book's structure is mostly hierarchical with light cross-referencing. A technical PDF has **dense, typed relationships** between structurally different kinds of content:

| Content type | What's special |
|---|---|
| **Equations** | Have LaTeX source, label, and semantic links (formalizes a theorem, defines a variable used elsewhere) |
| **Figures** | Image data + caption + what data they depict + what claims they support |
| **Tables** | Actual structured data — rows, columns, types — not just rendered pixels |
| **Theorems/Proofs** | Formal claim-evidence pairs with dependency chains |
| **Algorithms** | Pseudocode with inputs/outputs/steps — executable structure |
| **Code listings** | Actual parseable code (which feeds back into the Rust parser) |
| **Citations** | Not just "[4]" but a typed edge: cites, contradicts, extends, reproduces |
| **Data** | Numbers in tables and figures that are currently trapped as rendered text |

### The Gains

**Cross-paper queries.** Once multiple papers are parsed into the same database:

```sql
-- Papers that cite Lamport 1978 AND report benchmarks over 1000 nodes
SELECT DISTINCT p.content AS title
FROM kerai.nodes p
JOIN kerai.nodes bib ON bib.kind = 'bib_entry'
  AND bib.metadata->>'author' LIKE '%Lamport%'
  AND bib.metadata->>'year' = '1978'
JOIN kerai.edges cite ON cite.source_id = p.id AND cite.target_id = bib.id
JOIN kerai.nodes tbl ON tbl.kind = 'table'
  AND tbl.path <@ p.path
  AND tbl.metadata @> '{"has_column": "nodes"}'
WHERE p.kind = 'paper';
```

```sql
-- Find all theorems across papers that claim commutativity
SELECT t.content AS theorem, pr.content AS proof,
       paper.content AS paper_title
FROM kerai.nodes t
JOIN kerai.nodes pr ON pr.kind = 'proof'
JOIN kerai.edges e ON e.source_id = pr.id
  AND e.target_id = t.id AND e.kind = 'proves'
JOIN kerai.nodes paper ON paper.kind = 'paper'
  AND t.path <@ paper.path
WHERE t.kind = 'theorem'
  AND t.content ILIKE '%commut%';
```

**Data liberation.** Table 1 in the example contains real benchmark data — agent counts, throughput, latency. In a PDF, those numbers are pixels. In the AST, they're queryable rows:

```sql
-- Aggregate P99 latency across all CRDT papers' benchmarks
SELECT paper.content AS paper,
       avg((row_data->>'p99_latency_ms')::float) AS avg_p99
FROM kerai.nodes tbl
JOIN kerai.nodes paper ON paper.kind = 'paper' AND tbl.path <@ paper.path
CROSS JOIN LATERAL jsonb_array_elements(tbl.metadata->'rows') AS row_data
WHERE tbl.kind = 'table'
  AND tbl.metadata->'columns' ? 'p99_latency'
GROUP BY paper.content;
```

The data locked in rendered tables becomes part of a queryable corpus. Meta-analyses that currently require manual extraction from dozens of PDFs become SQL queries.

**Equation-claim-evidence chains.** Technical writing has a structure that plain text obscures:

- A **claim** ("our protocol converges in O(n) time")
- is **formalized** by an equation
- is **proven** by a theorem/proof block
- is **supported** by experimental data in a table
- which is **visualized** in a figure
- which is **compared against** prior work via citations

These are all edges in the graph. You can trace the full evidence chain for any claim, or find claims that lack formal proof, or find figures that don't correspond to any table data.

**AI perspectives on technical content.** An AI agent reviewing a paper can weight individual components:

- Theorem 1 → weight 0.95, "Novel contribution, not in prior work"
- Section 2.1 → weight 0.2, "Standard definitions, available in textbooks"
- Table 1 → weight 0.8, "Unique experimental data"
- Equation 3 → weight -0.4, "Appears to assume synchronous network, contradicts system model in 2.1"

That last one — flagging an internal contradiction — is possible because the AI's perspective is over *individual nodes* with *typed edges* between them. It's not just "this paper is 7/10." It's a structured review.

**Citation graph as first-class data.** Citations stop being opaque "[4]" markers and become typed, traversable edges. A citation might mean "we extend this," "we contradict this," "we reproduce this," or "we use their dataset."

**The marketplace applies naturally.** A novel proof technique, a uniquely comprehensive benchmark dataset, an algorithm with better complexity — these are nodes with measurable reproduction cost.

### The Parser Challenge

Options in order of fidelity:

1. **LaTeX source** (if available) — best case. LaTeX has explicit structure. A LaTeX parser produces a clean AST directly.
2. **PDF with structure tags** (PDF/A, tagged PDF) — some PDFs carry structural metadata.
3. **PDF extraction + AI classification** — tools like GROBID, Nougat, or Marker extract structure from untagged PDFs. Imperfect but useful.
4. **Markdown/HTML intermediate** — many modern papers have HTML versions (arxiv HTML, PubMed Central).

The parser is a module, the schema is universal. You add `src/parser/latex/` or `src/parser/pdf/` alongside the existing Rust parser. The downstream infrastructure works unchanged.

### The Bigger Picture

Technical PDFs are arguably where this architecture delivers the most value relative to the status quo. Code already has decent tooling (LSP, git, grep). But the world's technical knowledge is locked in millions of PDFs where the structure has been flattened into paint commands. Recovering that structure and making it queryable, versionable, and tradeable is a genuinely unsolved problem that kerai's architecture is positioned to address.

---

## 3. The Knowledge Editor — A Product Vision

### What We're Describing

An environment where a human writes, and the system — drawing on a corpus of parsed technical literature — actively participates in knowledge production. Not autocomplete. Not "summarize this paper." Something closer to a research collaborator that has read everything, remembers structural relationships, and can compute genuinely new connections.

### Why Kerai Makes This Possible (and Existing Tools Can't)

Tools like Semantic Scholar, Elicit, and Connected Papers work at the **paper level** — titles, abstracts, citation counts. They can tell you "these papers are related" but not *why*, and not at the level of specific claims, equations, or data.

Kerai's parsed corpus works at **node level**. The system doesn't know that Paper A and Paper B are related. It knows that Theorem 3 in Paper A and Equation 7 in Paper B formalize the same property with different bounds, that Table 2 in Paper C has experimental data that neither A nor B tested against, and that the proof technique in Paper D's Lemma 2 could close the gap between A's upper bound and B's lower bound.

That's the difference between a library catalog and a research partner.

### How It Maps to the Architecture

Every piece already has a home in the plans:

**The corpus** — parsed papers stored as nodes/edges. Thousands of papers, each decomposed into theorems, equations, tables, figures, claims, citations — all in one queryable graph.

**The AI agents** (Plan 08) — running perspectives over the corpus. Not one monolithic AI, but specialized agents:

- A *consistency checker* that weights contradictions between papers
- A *gap finder* that identifies claims without supporting data, or data without explanatory theory
- A *technique matcher* that notices when a proof method from one domain could apply to an open problem in another
- A *data synthesizer* that aggregates experimental results across papers into combined views

Each agent's perspective is stored, versioned, queryable.

**The editor** — a new layer, but thin. The human writes, producing nodes. As they write, the system:

1. **Recognizes what you're discussing** — you type a claim, the system identifies related nodes across the corpus
2. **Surfaces relevant structure** — not "here are 10 papers about CRDTs" but "Theorem 3 in Kleppmann 2019 proves this for trees, but your claim is about DAGs — here's the gap, and here's a technique from Shapiro 2011 that might bridge it"
3. **Offers computed contributions** — "If you combine the bounds from these three papers, the resulting bound is tighter than any of them state individually. Here's the derivation." That's a genuinely new result, produced by computation over structured data.
4. **Tracks provenance** — every suggestion is an edge. The new paragraph you write cites sources, but the system also records *which agent suggested the connection*, *which nodes it drew from*, and *what reasoning it applied*. The knowledge production process is auditable.

**The economy** (Plans 10-11) — the new knowledge you produce enters the same graph as the source material. It has measurable value (reproduction cost: how long would it take another agent to independently notice this connection?). It can be attested, auctioned, and eventually open-sourced through the same marketplace.

### What's Genuinely Novel

The editor, the corpus, the AI, and the economy are all operating on the same graph. There's no impedance mismatch between "the tool I write in" and "the database of knowledge" and "the AI's understanding" and "the marketplace." They're all nodes, edges, perspectives, and operations in one Postgres database.

**Writing is graph manipulation.** When you write a paragraph claiming X, you're inserting a node. When the AI suggests a connection to Theorem Y, it's proposing an edge. When you accept that suggestion, the edge is created with provenance. When you refine the claim based on the connection, that's a node update with an operation record.

**Reading is querying.** The AI doesn't re-read papers each time. The perspectives are already computed and stored. When you start writing about topic X, the system queries existing perspectives: "which agents have weighted which nodes highly for topic X?" The response is instant because it's a database query, not a language model inference.

**New results are first-class.** The system can produce *new computed knowledge* — combined datasets, tightened bounds, identified contradictions, novel applications of existing techniques — and that knowledge immediately enters the corpus as nodes with full provenance, available to the next person (or agent) working on a related problem.

### A Concrete Scenario

You're writing a paper on scaling CRDTs to 100K nodes. You open the editor and start your introduction.

As you write, the system shows you:

> **3 papers claim O(n) convergence but test only up to 1K nodes.**
> Specifically: Kleppmann 2019 Table 2, Shapiro 2011 Table 4, Preguica 2018 Figure 7.
> None test beyond 1,024 nodes. Your proposed 100K regime is uncharted.

You write your system model section. The system notices:

> **Your network assumption (partial synchrony) differs from Kleppmann's (asynchronous).**
> Theorem 1 in Kleppmann 2019 requires asynchrony.
> If you want to cite it, you need to re-prove under partial synchrony — or weaken your claim.
> However: Lemma 4 in Attiya 2015 proves an equivalent property under partial synchrony. It's unused in the CRDT literature.

That Attiya connection — a technique from distributed computing applied to CRDTs — is a genuinely new contribution that the system identified by traversing edges across subfields. You didn't know about Attiya 2015. The system found it because an agent's perspective had weighted Lemma 4 highly for "convergence proof technique" and your current writing context matched.

You incorporate the connection. The system records the full provenance chain: your claim node → edge:enabled_by → Attiya Lemma 4 node → edge:discovered_by → technique_matcher agent → perspective weight 0.92 with reasoning.

Later, another researcher writing about a different CRDT property gets surfaced this same connection — because your published use of Attiya's technique created new edges in the graph that the gap-finder weights highly for their problem too. Knowledge compounds.

### As a Product

This would be the first application that makes kerai's infrastructure tangible to people who aren't thinking about AST-based version control. They're thinking about writing, research, and producing knowledge. The version control, CRDTs, and marketplace are invisible infrastructure — what they see is an editor that genuinely understands their field and helps them produce new results.

The development path:

1. **Corpus ingestion** — PDF/LaTeX parser module (adjacent to the Rust parser)
2. **Perspective agents** — specialized agents running over the corpus (Plan 08 infrastructure)
3. **Editor interface** — could be a web app, a VS Code extension, or even a terminal UI. It's thin — most logic is SQL queries against the kerai database.
4. **The feedback loop** — new writing enters the graph, agents re-evaluate, connections compound

The hard parts are the parser (extracting structure from PDFs) and tuning the perspective agents to surface genuinely useful connections rather than noise. The infrastructure — storage, versioning, querying, collaboration, economy — is what we're already building.

---

## 4. The Bridge Architecture — kerai_web

### Can a pgrx Extension Serve HTTP?

Technically, yes. pgrx background workers run arbitrary Rust code, so a worker *could* bind a TCP socket and run an HTTP server (axum, hyper, etc.) inside the Postgres process. But this fights the design of Postgres:

- Postgres manages its own process lifecycle — a background worker handling hundreds of HTTP connections creates resource contention
- TLS termination, static file serving, WebSocket upgrades, connection pooling would all need reimplementation inside the extension
- A crash in HTTP handling takes down a Postgres background worker, potentially affecting database stability
- Postgres's shared memory model wasn't designed for web traffic patterns

Possible but adversarial to the host. Not where you want to be.

### What the Extension Already Does Well

kerai pushes all logic into SQL-callable functions. Parsing, CRDT operations, queries, perspectives — they're all `#[pg_extern]` functions. From the database's perspective, the editor's operations are just:

```sql
SELECT kerai.insert_node('paragraph', 'ch3.sec1.para4', 'The claim is...');
SELECT kerai.create_edge('cites', new_para_id, bib_entry_id);
SELECT kerai.get_perspectives('technique_matcher', current_context_id);
SELECT kerai.apply_operation(op_json);
```

The web layer's only job is translating between HTTP/WebSocket and these SQL calls. Genuinely thin.

### The Bridge Architecture

A translation layer, not an application server:

```
Browser (ProseMirror/TipTap editor)
    |
    |  WebSocket + HTTP
    |
Bridge (thin Rust or Go binary)
    |
    |  SQL (libpq / tokio-postgres)
    |
Postgres + kerai extension
```

**The bridge handles:**
- HTTP → SQL translation (editor action → `kerai.insert_node()`)
- WebSocket → LISTEN/NOTIFY relay (real-time collaboration)
- Static file serving (the editor's HTML/JS/CSS)
- TLS termination (or sit behind Traefik)
- Auth (validate session, map to kerai wallet/identity)

**The bridge does NOT handle:**
- Parsing logic (extension)
- CRDT operations (extension)
- Perspective queries (extension)
- Knowledge economy (extension)

A few hundred lines of Rust. Closer to a protocol adapter than an application.

### Real-Time Collaboration via Postgres Primitives

Postgres already has the mechanism — `LISTEN/NOTIFY`:

1. User A inserts a paragraph → bridge calls `kerai.insert_node()` → extension applies CRDT operation → triggers `NOTIFY kerai_ops, '{op_json}'`
2. Bridge subscribes to `kerai_ops` channel → receives notification → relays via WebSocket to all connected editors
3. User B's editor receives the operation → applies it locally

The CRDT guarantee means operations commute — no conflict resolution in the bridge. It just relays.

### Could It Be a Second Extension?

Instead of a separate binary, the bridge *could* be a second pgrx extension — `kerai_web` — that:

- Runs a background worker with a lightweight HTTP server
- Serves only WebSocket/API endpoints (static files served by Traefik)
- Has direct SPI access to kerai's functions (no network round-trip — it's *in* the database)
- Uses Postgres's LISTEN/NOTIFY internally for event relay

This avoids the "fighting Postgres" problems because:
- Not serving arbitrary web traffic — just API calls from the editor
- Connection count is bounded (editor sessions, not public internet)
- WebSocket server is purpose-built and minimal
- Static assets served by Traefik, not the extension

The advantage: **zero-hop access to kerai**. The bridge doesn't connect to Postgres over a socket — it calls kerai functions directly via SPI. Latency drops from "HTTP → SQL parse → execute → serialize → HTTP" to "direct function call."

### Recommended Path

Given existing infrastructure (Traefik, Docker, Postgres on Homebrew):

```
Browser
    |
Traefik (TLS, static files, routing)
    |
    |-- /api/*  -->  kerai_web extension (background worker, HTTP/WS)
    |                    |
    |                    +-- SPI calls to kerai extension (zero hop)
    |
    +-- /*      -->  static files (editor JS/CSS/HTML)
```

**Phase 1:** Separate bridge binary. Fastest to develop, easiest to debug, conventional deployment. A few hundred lines of Rust with axum + tokio-postgres.

**Phase 2:** Collapse the bridge into `kerai_web` extension once the API surface stabilizes. The bridge logic is already Rust, so porting into a pgrx background worker is straightforward. Gain the zero-hop SPI advantage.

Either way, the editor's intelligence lives entirely in the kerai extension. The bridge — whether binary or second extension — is just translating protocols.

---

## Meta-Observation

This conversation itself is the kind of thought chain that should be a first-class object in kerai. Each section builds on the previous, cross-references the plans, introduces new concepts, and produces conclusions that weren't in any single input. The provenance is traceable: plans → question about books → extension to PDFs → product vision. Storing this as a flat markdown file is exactly the problem we're describing — the structure and relationships exist but are flattened into text.

Soon, this musing *would be* nodes in the graph, with edges back to the plan documents it draws from, weighted by the AI perspectives that helped shape it, versioned through the CRDT operations that recorded its creation.
