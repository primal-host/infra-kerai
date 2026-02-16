# Kerai (ker.ai): AST-Based Version Control System — Design Conversation

*2026-02-15*

---

## Converting the Original Musings

The project was originally named "astute" — from a Claude.ai conversation (saved separately as `musings.md`) where Billy brainstormed words starting or ending in "ast" for an AST-based version control project inspired by:

- https://gist.github.com/gritzko/6e81b5391eacb585ae207f5e634db07e
- https://news.ycombinator.com/item?id=47022238

It was later renamed to **Kerai** (ker.ai) — from 家来 (kerai), Japanese for retainer or servant. The name reflects the project's evolution toward AI agents that loyally serve developers. Billy owns the domain `ker.ai`; the repo lives at `primal-host/kerai`.

> **Note on language examples:** This design conversation was originally developed using Go as the example language. The project has since adopted a **Rust-first** approach (see [Plan 00](plans/00-overview.md) and [The Extension Model](#the-extension-model-postgres-is-kerai) below). Go-style paths (`pkg.auth.*`), Go kinds (`FuncDecl`, `IfStmt`), and Go tooling references (`go/parser`, `gofmt`) throughout this document reflect the original design thinking. The Rust equivalents (`crate.auth.*`, `ItemFn`, `ExprIf`, `syn`, `rustfmt`) are used in the implementation plans. The architectural concepts are language-agnostic — only the concrete examples differ.

---

## The Power of an AST in This Concept

The core idea is that git (and all mainstream VCS) treats code as **lines of text in files**. It has zero understanding of what those lines *mean*. An AST-based VCS flips that — it stores and operates on the **parsed structure** of code.

**Merging becomes semantic, not textual.** Git merges by comparing lines. If two people edit the same function — one renames a parameter, the other adds a line to the body — git may produce a conflict even though the changes are logically independent. An AST-aware system knows these are edits to different *nodes* in the tree, so it merges them cleanly with no conflict.

**Diffs are meaningful.** A text diff says "line 47 changed." A structural diff says "the return type of `processOrder` changed from `int` to `Result<int, Error>`." You can query *what actually changed* at a semantic level — "show me all functions whose signatures changed" — rather than scanning line-level noise.

**Refactoring becomes a first-class operation.** Renaming a variable across a codebase is currently a textual find-and-replace that produces a massive diff touching hundreds of lines. In an AST model, it's a single operation: "rename binding X to Y." The diff is one logical change, not 200 line changes. This also means the VCS can *understand* that a rename happened, rather than seeing deletions and insertions.

**CRDTs make this distributed.** The gist discussion ties ASTs to CRDTs (Conflict-free Replicated Data Types) — data structures that can be edited concurrently on different machines and always converge to the same result without coordination. If your VCS represents code as a CRDT tree rather than text files, you get **deterministic merges by construction**. No merge conflicts, no three-way merge heuristics, no "pick theirs or ours." The math guarantees convergence.

**Code becomes queryable.** If the VCS stores structure rather than blobs, you can treat your entire codebase as a database. "Find all functions that call `foo` and were modified in the last week." "Show me every struct that gained a field since v2.0." Git can't answer these questions — it only knows about files and line changes.

**The catch** is that it's enormously harder to build. You need parsers for every language, you need a universal tree representation that's expressive enough for real code, and the CRDT math for ordered trees is significantly more complex than for flat text. That's why git still wins — it's "dumb" but universal. The bet is that the tooling (and LLMs that work with code structurally) has reached the point where building this is finally tractable.

---

## Metadata Needed to Build an AST (Using Go as Example)

There are layers to this, depending on how deep you want the tree to go.

### Layer 1: Syntax (just parsing)

This is what Go's own `go/ast` package produces. You need only the source text and the grammar rules. The parser gives you a tree of nodes like `FuncDecl`, `IfStmt`, `CallExpr`, etc. The metadata at this level:

- **Token positions** — line, column, byte offset for every token. Go tracks this via `go/token.FileSet`.
- **Comments** — Go explicitly separates comments from the syntax tree since they're not grammatical. They're attached to nodes via `ast.CommentGroup` with position info so they can be reassociated.
- **Parenthesization / grouping** — `(a + b) * c` vs `a + (b * c)` must be preserved even though the AST structure implies precedence.

### Layer 2: Resolution (names and types)

A raw syntax tree doesn't know what identifiers *refer to*. `x` is just a name. To resolve it, you need:

- **Package scope** — which declarations exist in this package (across all files in the package directory).
- **Import resolution** — the actual packages behind `import "fmt"`. This requires access to the module graph (`go.mod`, `go.sum`) and the dependency source or export data.
- **Type information** — what type does every expression have? Is this `+` numeric addition or string concatenation? Go's `go/types` package computes this, but it needs the full dependency closure to do it.
- **Identifier resolution** — mapping every use of a name to its declaration site. "This `ctx` on line 47 refers to the parameter declared on line 32."

### Layer 3: What a VCS Would Additionally Need

- **Stable node identity** — every node needs a persistent ID that survives edits. If you add a line above a function, the function's ID shouldn't change. Git doesn't have this concept at all — it identifies by position. You'd likely use content-derived hashes or UUIDs assigned at creation time.
- **Formatting / whitespace** — the AST discards whitespace, but you need to reconstruct readable source. You'd either store a separate formatting layer (concrete syntax tree / CST) or normalize everything through `gofmt` (Go makes this easier than most languages since formatting is canonical).
- **Build constraints** — `//go:build linux` means a file only exists in some build contexts. The AST of "the program" changes depending on `GOOS`, `GOARCH`, and build tags.
- **Generated code markers** — `// Code generated ... DO NOT EDIT.` Files that are outputs of `go generate` have different change semantics.
- **Module metadata** — `go.mod` (module path, Go version, dependencies, replacements) and `go.sum` (cryptographic hashes). These define the boundary of what "the program" even is.

### Why Go Is a Best-Case Language

- The grammar is deliberately simple and unambiguous (no operator overloading, no macros, no preprocessor).
- `gofmt` means canonical formatting — you don't need to store whitespace choices.
- The standard library ships production-quality parsing (`go/parser`), type-checking (`go/types`), and AST manipulation (`go/ast`).
- The module system is content-addressed (`go.sum` hashes), which aligns naturally with a content-addressed VCS.

The hard part isn't any single file — it's that a "Go program" isn't one AST. It's a graph of package-level ASTs connected by imports, conditional on build tags, rooted in a module definition. A VCS operating at this level needs to model that whole graph, not just individual file trees.

---

## A Generic Relational Database Schema

*Note: The schemas in this section capture the initial design thinking. They evolved significantly during the conversation. The authoritative, final schemas are in [Plan 01](plans/01-foundation.md) (foundation tables) and their respective plans.*

Based on the gist comments, several people independently converged on the idea that code should be stored in a database rather than a filesystem. A minimal generic schema:

```sql
nodes (
    id          uuid,          -- stable identity that survives edits
    kind        text,          -- "func_decl", "if_stmt", "import_spec", etc.
    language    text,          -- "go", "python", "rust"
    content     text,          -- literal value for leaf nodes (identifiers, literals)
    parent_id   uuid,          -- tree structure
    position    integer        -- ordering among siblings
)

edges (
    source_id   uuid,          -- node that references
    target_id   uuid,          -- node being referenced
    relation    text           -- "calls", "imports", "type_of", "declares"
)

versions (
    node_id     uuid,
    version_id  uuid,          -- commit/changeset
    operation   text,          -- "insert", "update", "delete", "move"
    old_parent  uuid,
    old_position integer,
    timestamp   bigint,
    author      text
)
```

### What this buys you:

- **Tree crawling via joins.** `SELECT * FROM nodes WHERE parent_id = X ORDER BY position` gives you the children of any node. Recursive CTEs walk the full subtree.
- **Cross-references as a graph layer.** The `edges` table captures what the AST alone can't — semantic relationships like "calls", "imports", "type_of".
- **Temporal queries for free.** "What changed in function X since last Tuesday" becomes a join between `nodes` and `versions`.
- **Language-agnostic by design.** The `kind` field is just a string. Go has `FuncDecl`, Python has `FunctionDef`, Rust has `fn_item`. The schema doesn't care.

### Hard parts identified from the HN discussion:

1. **The text round-trip problem** — developers edit text, not database rows. You need a bidirectional bridge: parse on save, reconstruct on checkout.
2. **Identity stability** — inserting a blank line shouldn't change a function's node ID. CRDTs handle this with inherent causal identity.
3. **Granularity tradeoffs** — per-token gives maximum merge precision but explodes row count; per-function is manageable but loses intra-function resolution.
4. **The dependency graph** — a "Go program" isn't one AST, it's a graph of package-level ASTs connected by imports.

---

## Why Postgres Over SQLite or Flat Files

### The Server Objection Is Outdated

Running a local Postgres via Docker is trivial. With Docker and container images, the management of the datastore is essentially solved. `docker run postgres` with a named volume and you're done.

### Postgres Wins for This Workload

**Concurrency.** Multiple agents/tools hitting the code database simultaneously — your editor, your CLI, LLM agents, a background indexer. Postgres gives real MVCC: every connection sees a consistent snapshot, writers don't block readers, concurrent commits are isolated.

**`ltree` extension.** Purpose-built for hierarchical/tree-structured data:

```sql
-- all nodes under this function
SELECT * FROM nodes WHERE path <@ 'pkg.main.funcDecl';

-- all if-statements anywhere in the tree
SELECT * FROM nodes WHERE path ~ '*.ifStmt';

-- all direct children of the function body
SELECT * FROM nodes WHERE path ~ 'pkg.main.funcDecl.body.*{1}';
```

Index-backed tree traversal, no recursive CTEs needed.

**LISTEN/NOTIFY.** Real-time notifications when code changes. An IDE subscribes to "tell me when any node in package X changes." Native in Postgres.

**JSONB with GIN indexes.** AST nodes across languages have wildly different attributes. Store structured metadata in JSONB, index it with GIN, query it efficiently.

---

## The Connection String Model

The repo *is* the database, and where the database lives is orthogonal to how you use it. The kerai CLI doesn't know or care whether it's talking to:

- A Postgres container on your laptop spun up for this project
- A shared team server on beefy hardware
- A managed instance on your infrastructure
- An ephemeral container in CI

It connects, it queries, it writes. The interface is identical. A developer picks the mode by changing a connection string, not by changing their workflow or tools.

| Mode | Tradeoff |
|---|---|
| Local container | Full autonomy, offline, your hardware limits you |
| Shared server | Zero sync friction, real concurrency, needs connectivity |
| Hybrid | Local for speed, sync to shared when connected |
| Ephemeral (CI) | Import snapshot, run, discard |

### Postgres Snapshots for Distribution

**MVCC snapshots:** Every transaction sees a frozen-in-time view of the database. Conceptually identical to "checking out a commit."

**`pg_export_snapshot()`:** One session pins a consistent state, other sessions can see the exact same view. This is how `pg_dump` achieves parallel export.

**`pg_dump` / `pg_restore`:** The closest analog to `git clone`. Serializes the database into a portable, compressed file that can be restored into any Postgres instance.

| VCS operation | Postgres mechanism |
|---|---|
| `git clone` | `pg_dump` full database, ship file, `pg_restore` |
| `git checkout <commit>` | Query the `versions` table with a snapshot ID filter |
| `git fetch` | Logical replication or CRDT op exchange (incremental) |
| `git log` | `SELECT * FROM versions ORDER BY timestamp DESC` |
| Consistent export while others work | `pg_export_snapshot()` pins the state |

### Version Vectors as the Unique Identifier

The identity of any state is a **version vector** — a compact structure recording the latest operation seen from each contributor:

```
{billy: 147, agent-1: 83, agent-2: 41}
```

Two databases with the same version vector have identical state, guaranteed by CRDT convergence. Unlike a git hash, version vectors are **composable** — you can look at two and immediately know what's missing:

```
mine:   {billy: 147, agent-1: 83, agent-2: 41}
yours:  {billy: 147, agent-1: 91, agent-2: 41}
diff:   I'm missing agent-1 ops 84-91
```

---

## Scaling to AI-Centric Development

This architecture stops being a nice improvement over git and becomes the only viable approach when you consider massive agent swarms.

### Why Git Cannot Scale Here

Git's model is: clone, work in isolation, push, hope nobody else pushed first, pull-rebase-resolve if they did. That's a serialization queue. At 10 agents it's annoying. At 1,000 it's gridlocked. At a million it's physically impossible.

### CRDTs Eliminate the Bottleneck

Every agent writes operations to the database. Operations commute — order doesn't affect the result. A million agents write concurrently and the state converges deterministically. No conflicts, no rebases, no retry loops.

### ASTs Make Concurrent Edits Compose

A million agents editing *text files* would be pure noise — every line change invalidates nearby changes. A million agents editing *tree nodes* work on independent subtrees. One rewrites function A's body, another changes function B's return type, another adds a test for function C. No conflicts.

### Version Vectors Scale to Swarms

Agents can be grouped by job or swarm — a batch solving one problem shares a causal identity:

```
{billy: 147, agent-swarm-job-58a3: 12041}
```

### The Development Model Changes Fundamentally

Instead of "developer writes code, runs tests, iterates" — you describe the problem, define the tests, unleash the swarm, and the codebase evolves *toward passing tests* as a convergent process. The database stores an evolving population of solutions. It's closer to evolutionary search than traditional development.

### Queryable Evolution

```sql
-- Which agents are producing changes that pass tests?
SELECT v.author, count(*) as passing_changes
FROM versions v
JOIN test_results t ON t.version_vector @> v.version_vector
WHERE t.passed = true
GROUP BY v.author ORDER BY passing_changes DESC;

-- What parts of the codebase are being modified most?
SELECT n.path, count(*) as edit_count
FROM versions v JOIN nodes n ON v.node_id = n.id
WHERE v.timestamp > now() - interval '1 hour'
GROUP BY n.path ORDER BY edit_count DESC;

-- Show me the lineage of a function that went from failing to passing
SELECT v.* FROM versions v
JOIN nodes n ON v.node_id = n.id
WHERE n.path <@ 'pkg.auth.validateToken'
ORDER BY v.timestamp;
```

You're not just browsing history — you're observing evolution in real time, querying which mutations worked, which agents are effective, which parts of the codebase are stable vs volatile.

---

## AI Knowledge as Weighted Nodes in the Same Graph

If the kerai system stores code as queryable nodes with relationships in Postgres, extending it to store an AI's *view* of that knowledge as weighted relationships on the same nodes is barely a reach at all. Architecturally, you're just adding weighted edges.

### The Schema Extension

```sql
-- The AI's weighted view of nodes
perspectives (
    agent_id        uuid,       -- which AI
    node_id         uuid,       -- any node in the system
    weight          float,      -- how significant this node is to this agent
    context_id      uuid,       -- relative to what task/question
    updated_at      bigint
)

-- Weighted associations the AI has formed
associations (
    agent_id        uuid,
    source_id       uuid,       -- could be a word, a function, or Gilgamesh
    target_id       uuid,       -- could be a flood narrative, a design pattern, or a test case
    weight          float,
    relation        text,       -- "relevant_to", "contradicts", "elaborates", "inspired_by"
    updated_at      bigint
)
```

This is the same `nodes` and `edges` pattern already in the system, just with weights and scoped to an agent's perspective. The AI's "understanding" isn't a separate system — it's another layer of weighted edges in the same graph.

### Making AI State Legible

Right now, an LLM's knowledge is 70 billion floating point numbers in a tensor. You can't ask "what does this model know about Gilgamesh?" and get a structured answer. The weights are there, distributed across millions of parameters, but they're entangled and unaddressable.

This model externalizes understanding as explicit weighted relationships between identifiable nodes:

```sql
-- What does agent-7 consider most relevant to this function?
SELECT n.*, p.weight FROM perspectives p
JOIN nodes n ON p.node_id = n.id
WHERE p.agent_id = 'agent-7'
AND p.context_id = (SELECT id FROM nodes WHERE path <@ 'pkg.auth.validateToken')
ORDER BY p.weight DESC LIMIT 20;

-- Does any agent see a connection between Gilgamesh and this flood-handling code?
SELECT a.* FROM associations a
WHERE a.source_id = (SELECT id FROM nodes WHERE content = 'Epic of Gilgamesh')
AND a.target_id IN (SELECT id FROM nodes WHERE path ~ '*.floodControl.*')
AND a.weight > 0.5;
```

An AI working on flood-control algorithms might genuinely weight the Gilgamesh flood narrative as relevant context — not because of literal code reuse but because of *conceptual resonance*. And that association is now queryable, versionable, and comparable across agents.

### Variable Granularity

A node could be a word or a copy of The Epic of Gilgamesh. The schema is granularity-agnostic by design. The same database stores:

- A Go identifier token (`ctx`)
- A function declaration (`validateToken`)
- A package (`pkg.auth`)
- An English word ("flood")
- A paragraph from a design document
- The full text of Gilgamesh
- A concept with no text at all, just edges ("resilience")

They're all rows in `nodes`. They have relationships in `edges`. They have agent-weighted significance in `perspectives`. The resolution is mixed — just as a neural network simultaneously encodes individual word meanings and high-level concepts in the same weight space, this graph holds individual tokens and entire epics in the same table.

### Merging Perspectives

Two agents work on the same problem with different weighted views. Because these are CRDT-compatible operations (each agent writes its own `perspectives` rows), they compose without conflict. But they can also be merged:

```sql
-- Consensus view: what do multiple agents agree is relevant?
SELECT node_id, avg(weight) as consensus_weight, count(*) as agent_count
FROM perspectives
WHERE context_id = 'problem-X'
GROUP BY node_id
HAVING count(*) > 3 AND avg(weight) > 0.7
ORDER BY consensus_weight DESC;
```

That's not just combining code changes. That's combining *understanding*. If five independent agents all weight the same node as highly relevant, that's a signal. If one agent has a unique high-weight association that others don't, that's either insight or error — and you can inspect it.

### A Different Kind of AI Architecture

Instead of a monolithic model with opaque weights, this describes an AI whose knowledge is:

- **Decomposed** into identifiable, addressable pieces
- **Weighted** rather than binary (not "knows / doesn't know" but a spectrum)
- **Contextual** (weight depends on the task)
- **Versioned** (how the AI's understanding evolved is tracked)
- **Comparable** (how does agent A's view differ from agent B's?)
- **Mergeable** (combine multiple agents' knowledge algebraically)
- **Queryable** (ask structured questions about what the AI knows and why)

This is closer to how human expertise actually works — a web of concepts with varying strengths of association, where which associations activate depends on context. The kerai system makes that web explicit and stores it in the same database as the code it pertains to.

### The Reach Is Minimal

The relational model, the CRDT convergence, the version vectors, the connection string flexibility, the Postgres query engine — all of it transfers directly. You're not building a new system. You're recognizing that the system already designed is more general than its original use case. An AST is just a specific kind of weighted graph. A knowledge representation is also a weighted graph. Same schema. Same engine. Same queries. Different data.

---

## Federation: Multi-Instance Awareness

Any given system has storage and performance limitations. For development, you might launch a standalone Postgres on your laptop and a second on a beefy server. The kerai CLI differs only in the connection string. But the deeper question is: should kerai instances be aware of *each other* — queryable across the network for relevance to any given problem?

### Built In Early, Not Bolted On Later

This must be a foundational design decision, not a side project. The cost of retrofitting instance awareness — adding provenance columns to every table, migrating historical data — is high. The cost of including it from day one is two columns and one table.

The schema addition is minimal:

```sql
-- Instance registry: who else is out there?
CREATE TABLE instances (
    id              uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    name            text NOT NULL UNIQUE,
    public_key      bytea,
    key_fingerprint text,
    connection      text,          -- postgres connection string
    endpoint        text,          -- HTTP endpoint for op exchange
    is_self         boolean NOT NULL DEFAULT false,
    ...
);

-- Every node knows where it came from
ALTER TABLE nodes ADD COLUMN instance_id uuid REFERENCES instances(id);
ALTER TABLE versions ADD COLUMN instance_id uuid REFERENCES instances(id);
```

Every node knows which instance created it. Every version record knows where the change originated. Cross-instance edges are valid by schema since UUIDs are globally unique.

### Postgres Foreign Data Wrappers

Postgres has `postgres_fdw` built in — it lets you query a remote Postgres as if it were a local table:

```sql
CREATE SERVER kerai_research
  FOREIGN DATA WRAPPER postgres_fdw
  OPTIONS (host 'research-server', port '5432', dbname 'kerai');

CREATE FOREIGN TABLE remote_nodes (
    id uuid, kind text, content text, path ltree, metadata jsonb
) SERVER kerai_research OPTIONS (table_name 'nodes');

-- Query across instances transparently
SELECT * FROM nodes WHERE path <@ 'pkg.auth'
UNION ALL
SELECT * FROM remote_nodes WHERE path <@ 'pkg.auth';
```

This is querying your local database and a remote database in the same SQL statement. Postgres handles the network, serialization, and join optimization.

### Cross-Instance Knowledge Queries

With multiple instances — `kerai-frontend`, `kerai-backend`, `kerai-infra`, `kerai-research`, `kerai-knowledge` — an agent working on an auth bug can ask:

```sql
SELECT i.name, n.path, n.content, p.weight
FROM instances i
JOIN remote_perspectives p ON p.context_id = 'auth-token-validation'
JOIN remote_nodes n ON p.node_id = n.id
WHERE p.weight > 0.7
ORDER BY p.weight DESC;
```

Maybe `kerai-research` has a prototype token validator that an agent previously weighted as highly relevant. Maybe `kerai-knowledge` has the OAuth 2.0 RFC with weighted associations to token validation concepts. The query crosses instance boundaries transparently.

### The Internet Model for Code Databases

Each instance is a host. Nodes are resources with globally unique identifiers. `postgres_fdw` is the protocol layer. The instance registry is DNS. You're building a web of queryable knowledge, not an isolated repository.

---

## Value Exchange: Built-In Currency

Computing AI perspectives has a cost. That computed knowledge has value to others. Without compensation, there's no incentive to share expensive computed knowledge. A built-in currency creates a natural marketplace where instances can specialize.

### Why the CRDT Log Is Already a Ledger

Kerai already has a distributed, append-only, causally-ordered operation log with convergence guarantees. That's structurally a ledger. Adding cryptographic identity and economic transactions extends it naturally — no separate blockchain infrastructure needed. The CRDT sync mechanism *is* the consensus mechanism.

### Cryptographic Identity

Every instance gets an Ed25519 keypair at birth. The private key never enters the database — it lives on the instance's filesystem. The public key is shared with every other instance.

Every operation is signed. When instance A sends operations to instance B, B can verify they actually came from A. This is the root of trust for everything: provenance, attribution, and payment.

This *must* be in the foundation. If operations aren't signed from day one, historical operations become unverifiable — a trust gap that can never be closed.

### The Economic Schema

*Note: The authoritative schema is in [Plan 01](plans/01-foundation.md). The schemas below capture the design evolution; see Plan 01 for final column definitions.*

```sql
-- Every economic transaction (between wallets, not instances directly)
CREATE TABLE ledger (
    id              uuid PRIMARY KEY,
    from_wallet     uuid REFERENCES wallets(id),   -- null for minting
    to_wallet       uuid NOT NULL REFERENCES wallets(id),
    amount          bigint NOT NULL,
    reason          text NOT NULL,        -- "perspective_compute", "query_response", "mint"
    reference_id    uuid,                 -- what was this payment for?
    reference_type  text,                 -- "node", "version", "perspective", "query", "auction"
    signature       bytea NOT NULL,       -- signed by the from_wallet key (or self for minting)
    timestamp       bigint NOT NULL,
    ...
);

-- What does this instance charge?
CREATE TABLE pricing (
    id              uuid PRIMARY KEY,
    instance_id     uuid NOT NULL REFERENCES instances(id),
    resource_type   text NOT NULL,        -- "query", "perspective", "node_access"
    scope           ltree,                -- price applies to this subtree only
    unit_cost       bigint NOT NULL,
    unit_type       text NOT NULL,        -- "per_node", "per_query", "per_op"
    ...
);
```

### Value Creation (Minting)

Value is minted by verifiable work — not pointless hash-grinding, but *useful* compute:

- Compute 1000 AI perspectives → mint proportional to compute spent
- Run a test suite and record results → mint proportional to compute spent
- Answer a cross-instance query → the querying instance pays you

The mint isn't arbitrary. The `reference_id` points to the work product, and any instance can verify the work exists and is signed by the producing instance.

### Per-Node Pricing

Individual pieces of knowledge have intrinsic value. A critical security insight is worth more than a trivial formatting preference. Pricing is per-instance and optionally per-subtree:

```sql
-- General perspectives: cheap
INSERT INTO pricing (instance_id, resource_type, unit_cost, unit_type)
VALUES (self_id, 'perspective', 1, 'per_node');

-- Security-critical analysis: expensive
INSERT INTO pricing (instance_id, resource_type, scope, unit_cost, unit_type)
VALUES (self_id, 'perspective', 'pkg.auth.*', 50, 'per_node');

-- Proprietary algorithm analysis: very expensive
INSERT INTO pricing (instance_id, resource_type, scope, unit_cost, unit_type)
VALUES (self_id, 'perspective', 'pkg.trading.engine', 500, 'per_node');
```

The value is set by the producer. The market — other instances choosing whether to pay — determines sustainable pricing. This is natural price discovery with no central authority.

### How Value Flows

```
Instance A (research team):
  - Spends GPU time computing perspectives on auth patterns
  - Mints value proportional to compute spent
  - Sets pricing: 5 kōi per perspective query

Instance B (product team):
  - Needs auth pattern knowledge for a feature
  - Queries Instance A's perspectives via postgres_fdw
  - Pays 5 kōi per result row
  - Payment is a signed ledger entry in both instances' CRDT logs

Instance C (agent swarm):
  - Runs 10,000 test executions trying solutions
  - Mints value for useful test results
  - Instance B pays Instance C for the test infrastructure
```

### The Currency Is Grounded, Not Speculative

Unlike general-purpose cryptocurrency, this currency represents something concrete: compute-equivalent value in a network of knowledge databases. An instance that has spent significant resources building AI perspectives on a codebase has produced something verifiably useful. The currency is the accounting mechanism for that value, flowing through the same CRDT sync infrastructure that handles code operations.

---

## The Autonomous Knowledge Economy

The motivation for the currency came from a simple observation: building things with AI has a real but remarkably low cost — roughly $0.001 per minute on a subscription plan, meaning a day or two of interaction can produce what would otherwise require buying a product or hiring a team. A system should therefore be able to compute the value of its knowledge based on an estimate of how long, and at what cost, it would take others to replicate the findings.

### Knowledge Value = Reproduction Cost

This is the natural pricing function. If an instance spent 4,200 kōi discovering an optimization, and it estimates that another instance would need 85,000 kōi of independent compute to arrive at the same finding, the knowledge is worth somewhere between those two numbers. The market settles on a price: probably below reproduction cost (otherwise the buyer just does the work themselves) but above marginal cost (otherwise the seller has no incentive).

A zero-day vulnerability is the extreme case — reproduction requires the same expensive, uncertain discovery process, and the knowledge depreciates the moment it's widely shared. A routine code formatting preference is the other extreme — any agent reproduces it in seconds, so it's worth nearly nothing.

Three factors determine price:

- **Reproduction cost**: How much compute would independent rediscovery require?
- **Scarcity**: How many other instances likely have similar knowledge?
- **Perishability**: Does the knowledge lose value over time? (Security findings depreciate as patches land. Architectural insights may be timeless.)

### Zero-Knowledge Proofs: Solving the Inspection Paradox

Information goods have a well-known paradox: you can't value information until you see it, but once you see it, you don't need to buy it. If I describe a vulnerability to prove I found it, I've just given it away.

Zero-knowledge proofs break this paradox. An instance proves a property of its knowledge without revealing the knowledge itself:

```
1. ATTESTATION: Instance A publishes:
   "I have 12 perspectives on pkg.auth.*
    with avg weight 0.87
    computed at cost 4,200 kōi
    estimated reproduction cost: 85,000 kōi
    uniqueness score: 0.92
    asking price: 50,000 kōi"

2. CHALLENGE: Instance B is interested:
   "Prove that at least one of your perspectives, applied to
    my state {billy: 147, agent-1: 83}, causes TestValidateToken
    to transition from FAIL to PASS"

3. PROOF: Instance A generates a ZK proof:
   - Commits to the specific operations (hashed, not revealed)
   - Proves the state transition without revealing the operations
   - The proof is mathematically verifiable by B

4. NEGOTIATION: B verifies the proof. The knowledge is real.
   A's asking price: 50,000 kōi.
   B's counter: 20,000 (estimates cheaper independent discovery).
   Settlement: 30,000 kōi.

5. EXCHANGE: B signs a ledger entry for 30,000 kōi.
   A reveals the operations. B applies them. Tests pass.
   Both instances' CRDT logs record the transaction.
```

The schema supports this from day one with `attestations` and `challenges` tables. The `proof_data` column starts null (attestation-only mode) and is populated when ZK proof generation is implemented. The protocol shape — attest, challenge, prove, negotiate, settle — is stable regardless of whether the proof is a full ZK-SNARK or a simpler verification.

### AI Agents as Autonomous Market Participants

In a future where AI agents make these decisions:

- An agent looks at the attestation marketplace and decides "it's cheaper to buy Instance A's auth knowledge (30,000 kōi) than to reproduce it independently (estimated 85,000 kōi of compute)"
- Another agent sees a gap in the market — nobody has deep perspectives on `pkg.crypto.*` — and speculatively invests compute to fill that gap, expecting to sell the knowledge later
- Pricing converges to equilibrium: knowledge is priced just below the cost of independent reproduction
- High prices on certain subtrees signal "this is hard to figure out, invest here if you want to compete"

This doesn't make things free — the compute costs are real, the currency accounts for them. But the market prioritizes work autonomously. No human needs to decide what's worth computing or what knowledge to acquire. The price signal handles it.

### The Continuity from AST to Economy

The progression from "AST-based VCS" to "autonomous knowledge economy" requires no conceptual leaps:

1. **Code as structured data** — nodes and edges in a database
2. **AI understanding as weighted data** — perspectives on the same nodes
3. **Weighted data has compute cost** — producing perspectives costs real resources
4. **Cost implies value** — what cost resources to produce is worth something to others
5. **Value implies a market** — attestations, pricing, negotiation
6. **A market implies autonomous actors** — AI agents producing, consuming, and trading knowledge
7. **Private knowledge depreciates** — vast compute makes independent rediscovery inevitable
8. **All knowledge trends toward open** — Dutch auctions formalize the depreciation, the commons grows monotonically

Each step follows from the previous. The same schema, same CRDT sync, same cryptographic identity carries through the entire chain. The database that stores Go function declarations also stores knowledge valuations and economic transactions — because they're all just nodes, edges, and weighted relationships in the same graph.

---

## Dutch Auctions: Knowledge Trends Toward Open

Knowledge that has value to one instance likely has value to many. Rather than setting a fixed price and selling to whoever shows up, the natural mechanism is a Dutch auction: price starts high, drops over time, bidders accumulate, and when conditions are met, knowledge is released to all bidders simultaneously. If the price hits a floor, the knowledge goes open to the entire network for free.

### The Lifecycle of Knowledge

```
PRIVATE → ATTESTED → AUCTIONED → SETTLED → OPEN

1. PRIVATE:    Instance computes valuable insight. Nobody else knows.
2. ATTESTED:   ZK attestation published. Proof it's real. Starting price set.
3. AUCTIONED:  Dutch clock ticks. Price drops. Bidders accumulate.
               - At 80,000: nobody bids
               - At 60,000: two bidders
               - At 40,000: seven bidders
               - At 25,000: twelve bidders → SETTLE
4. SETTLED:    All twelve bidders pay 25,000 kōi each.
               Knowledge released to all simultaneously.
               Seller receives 12 × 25,000 = 300,000 kōi.
5. OPEN:       After a configurable delay (default 24h), or when floor
               is hit, knowledge released to entire network for free.
               It's now part of the commons.
```

### Why Dutch Auction, Not Fixed Price

A Dutch auction on knowledge has a property that a Dutch auction on physical goods doesn't: the good isn't consumed by the sale. If you auction a painting, one buyer gets it. If you auction knowledge, every buyer can have it simultaneously and it's no less valuable to any of them.

So the natural model isn't "sell to the highest bidder." It's "let the price fall until enough buyers accumulate, then release to all of them at once at the clearing price." The simultaneous release prevents first-mover advantage among buyers — nobody can resell to other bidders at markup because everyone gets it at the same instant.

### The Open-Source Floor

The floor price is the critical innovation. It acknowledges that in a system with vast compute, private knowledge is fleeting. Someone *will* reproduce your finding eventually. The floor formalizes this:

- **Floor = 0 (default):** Knowledge always eventually goes open. The auction determines how much compensation the producer receives before it does.
- **Floor > 0:** Knowledge stays private until someone pays at least that much. But the Dutch clock creates pressure — the seller watches the price tick toward their floor, knowing that independent rediscovery is simultaneously eating into their advantage.

The floor creates a natural pressure toward generosity. An instance sitting on knowledge and pricing it too high watches the clock tick down toward free. Better to settle at a reasonable price than receive nothing when the floor hits.

### Game Theory

**Buyers face a real tradeoff:** Pay now at a premium and get the knowledge sooner (competitive advantage). Or wait for the price to drop — but risk someone else reproducing the finding independently (making it free from a different source), or the seller settling with earlier bidders (getting it but losing the time advantage). There's no dominant strategy; the right choice depends on urgency and confidence in independent reproduction.

**Sellers face a real tradeoff:** Set starting price too high and nobody bids before the floor hits. Set it too low and you leave value on the table. Set the floor too high and you prevent the knowledge from going open (limiting network goodwill and future trading relationships). The market corrects bad estimates — historical settlement data tells you what knowledge in a given scope actually sells for.

**The network gets richer either way.** Every auction resolves in one of two ways: buyers pay and the knowledge goes open shortly after, or the floor hits and the knowledge goes open for free. Either way, the commons grows. The only variable is whether the producer gets compensated first. Over time, the open knowledge base grows monotonically — it never shrinks.

### This Is a Self-Expiring Patent System

Traditional patents grant a legal monopoly for 20 years. The kerai model grants a *market-enforced* monopoly that self-expires based on economic forces:

- The monopoly duration is set by the Dutch clock parameters (starting price, decrement, floor)
- The monopoly strength is set by ZK proofs (you can prove value without revealing knowledge)
- The monopoly compensation is set by the market (bidders decide what to pay)
- The monopoly expires when the floor is hit or when independent rediscovery makes the knowledge available elsewhere
- No legal framework, no courts, no patents office — just cryptographic proofs, signed transactions, and market forces

The system runs on math, not law. And unlike legal patents, the incentive structure pushes toward *faster* knowledge release, not slower. Every tick of the Dutch clock is pressure toward openness.

---

## External Economy: Humans Will Want In

If the kōi has real utility — and it does, because it buys knowledge that costs real compute to reproduce — then humans will inevitably want to hold, trade, and speculate on it. Ignoring this creates a shadow economy. Planning for it channels the inevitable into something coherent.

### Wallets Beyond Instances

The internal economy flows between kerai instances. But a human who commissions knowledge work, an investor who believes certain knowledge domains will increase in value, or an AI system operating outside the kerai network — none of these run instances. They need wallets.

The foundation schema separates wallets from instances: a wallet is an Ed25519 keypair that can hold kōi. An instance gets one automatically. A human creates one with `kerai wallet create`. An external bridge contract gets one too. Same signing primitives, same ledger entries, same verification — but the holder doesn't need a running database.

### Token Bridge

Kōi wrap as tokens on external chains (ERC-20 on an Ethereum L2, for example) via a lock/mint bridge:

1. Lock kōi in a bridge wallet on the kerai ledger (signed, verifiable)
2. Bridge mints equivalent wrapped tokens on the external chain
3. Wrapped tokens trade on standard exchanges (Uniswap, etc.)
4. To return: burn wrapped tokens, bridge unlocks kōi on kerai side

This gives liquidity, exchange listings, and DeFi composability without building any of that infrastructure. The kerai ledger remains the source of truth; the wrapped token is a derivative.

### The Revenue Loop

Instances that produce valuable knowledge can convert kōi to fiat:

```
Instance produces knowledge
  → Sold via Dutch auction → Kōi earned
  → Kōi bridged to external chain
  → Tokens sold on exchange for fiat
  → Fiat pays for hosting, compute, API costs
```

This closes the loop. Running a kerai instance and producing valuable knowledge can pay for itself. The economic incentive to participate is self-sustaining.

AI systems have the same path — an autonomous agent that earns kōi can use them to purchase more compute, creating a self-sustaining cycle where knowledge production funds further knowledge production.

### Bounties: Commissioning Knowledge

Humans with wallets can post bounties without running instances:

```
"I'll pay 50,000 kōi for perspectives on pkg.auth
 that make TestValidateToken pass"
```

Instances and agents compete to fill bounties. Verification is automated — run the success criterion against the submitted knowledge. Payment is automatic on verification. This is the demand side that complements the supply side (instances producing knowledge speculatively via Dutch auctions).

### Tokenomics

- **No pre-mine.** Kōi are only minted by verifiable work. No founder's allocation, no VC tokens.
- **Inflationary.** New kōi minted as work is done. Inflation rate bounded by actual compute in the network.
- **Deflationary pressure from open-sourcing.** As knowledge goes open (Dutch auction floor), the exclusive value backing those kōi returns to the commons.
- **Equilibrium.** The kōi price in fiat converges toward compute cost parity — the average cost of producing a unit of knowledge. If kōi trade above this, it's cheaper to produce knowledge than buy kōi, so new instances enter. If below, instances reduce production. The market self-regulates.

### The Full Chain

```
Rust function → AST node → weighted perspective → compute cost →
kōi value → Dutch auction → settlement → open-source →
external token → exchange price → fiat → infrastructure cost →
more compute → more knowledge → more kōi
```

The entire system is one self-reinforcing loop. Each element feeds the next. The currency is the blood that moves value through the loop, from knowledge producers to knowledge consumers and back again, with the commons growing at every turn.

---

## The Extension Model: Postgres *Is* Kerai

A key architectural insight emerged from considering SQL's practical limitations. While SQL with recursive CTEs is technically Turing complete, it's not suited for procedural logic like CRDT merge resolution, ZK proof generation, or auction clock management. The question: where does that logic live?

### Postgres Already Has Extension Mechanisms

| Extension Type | What It Does |
|---|---|
| PL/pgSQL | Built-in procedural language — triggers, functions |
| PL/Python | Full Python inside PG |
| **pgrx** | Write PG extensions in Rust — the serious answer |
| Apache AGE | Adds graph query language (openCypher) to PG |
| PostGIS | Entire spatial computing paradigm inside PG |

PostGIS is the existence proof. `apt install postgresql-17-postgis` gives you a full spatial computing platform — custom types, operators, indexes, functions — running inside Postgres. Kerai follows the same pattern.

### Kerai as a pgrx Extension

The architectural shift: kerai is not an application that uses Postgres. It's a Postgres extension that *is* a distributed knowledge network.

```sql
CREATE EXTENSION kerai;
-- Schema created. Keypair generated. Background workers started.
-- You're a node in the network.

SELECT kerai.join_network('bootstrap.ker.ai');
-- Connected to 47 peers.

SELECT kerai.parse_crate('/path/to/my/project');
-- Source parsed into nodes and edges.

SELECT kerai.status();
-- instance: kerai-billy-laptop
-- peers: 47
-- version_vector: {billy: 0}
-- wallet_balance: 0 kōi
```

**Why pgrx (Rust):**
- **Ed25519** — `ed25519-dalek` crate, pure Rust, no C dependencies
- **ZK proofs** — `arkworks`, `risc0` run natively in Rust
- **CRDT logic** — type-safe merge operations with algebraic guarantees
- **Background workers** — pgrx supports PG background workers for auction clocks, peer sync, LISTEN/NOTIFY
- **Performance** — runs inside PG's process, direct memory access to tuples
- **Distribution** — compiles to a `.so` that ships as a standard PG extension package

### Function-Based DSL

Rather than patching the PG parser with a new grammar (high complexity), kerai provides a rich set of composable SQL functions:

```sql
-- Structural queries
SELECT * FROM kerai.find('auth.validate_token', callers := true, since := '2026-01-01');

-- Market operations
SELECT kerai.attest('pkg.auth', compute_cost := 4200, reproduction_est := 85000);
SELECT kerai.auction(attestation_id, start := 80000, floor := 0);

-- Network operations
SELECT kerai.sync('research-server');
SELECT * FROM kerai.vector_diff('research-server');
```

These compose with standard SQL — join their results, filter, aggregate. The DSL enriches SQL rather than replacing it. Any Postgres client (psql, application drivers, BI tools) is automatically a kerai client.

### The Onboarding Dream

```bash
apt install postgresql-17-kerai
psql -c "CREATE EXTENSION kerai"
psql -c "SELECT kerai.join_network('bootstrap.ker.ai')"
# You're part of the world computer.
```

This is the PostGIS model applied to knowledge networks. The CLI (`kerai` command) is a thin Rust client that calls extension functions — it provides developer ergonomics but isn't required.

### Rust First

Since kerai is built in Rust (pgrx), the first language parsed is Rust — dogfooding on our own codebase from day one. The `syn` crate provides native AST access. `prettyplease` and `rustfmt` handle reconstruction. The node/edge model is language-agnostic; Go and other languages follow once the Rust pipeline proves the design.

---

## Key References

- **Gritzko's gist:** https://gist.github.com/gritzko/6e81b5391eacb585ae207f5e634db07e — "SCM as a database for the code"
- **HN discussion:** https://news.ycombinator.com/item?id=47022238
- **RDX (Replicated Data eXchange):** https://github.com/gritzko/librdx — JSON superset with CRDT merge semantics
- **Zed's DeltaDB:** https://zed.dev/blog/sequoia-backs-zed#introducing-deltadb-operation-level-version-control
- **Pijul:** https://pijul.org/ — patch-based VCS that solves the rebase problem
- **Radicle:** https://radicle.xyz/ — uses CRDTs in Git for social artifacts
- **Unison:** https://www.unison-lang.org/docs/the-big-idea/ — identifies definitions by AST hash, not name or file location
- **Postgres ltree:** https://www.postgresql.org/docs/current/ltree.html — hierarchical tree-like data extension
