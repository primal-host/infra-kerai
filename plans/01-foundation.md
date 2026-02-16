# Plan 01: Foundation

*Depends on: nothing*
*Enables: everything*

## Goal

Build the kerai pgrx extension — a Rust-based Postgres extension that creates the full schema, registers the DSL functions, generates cryptographic identity, and starts background workers. At the end of this plan, `CREATE EXTENSION kerai` gives you a running instance with the core schema, cryptographic identity, economic ledger foundations, and a function-based DSL — ready for parsing (Plan 02), CRDT operations (Plan 04), and network participation.

## Deliverables

### 1.1 pgrx Extension Setup

Kerai is a Postgres extension built with [pgrx](https://github.com/pgcentralfoundation/pgrx) (Rust). The deliverable is a loadable extension, not a standalone application.

**Installation:**

```sql
CREATE EXTENSION kerai;
-- Creates the full schema, registers DSL functions, generates keypair,
-- creates the "self" instance and wallet, starts background workers.
```

**What `CREATE EXTENSION kerai` does:**

1. Creates all tables (instances, wallets, nodes, edges, versions, operations, version_vector, ledger, pricing, attestations, challenges)
2. Creates all indexes
3. Registers SQL-callable DSL functions (see section 1.6)
4. Generates an Ed25519 keypair via `ed25519-dalek`
5. Creates the "self" instance record with the public key
6. Creates the instance's wallet (linked to the self instance)
7. Starts background workers (CRDT sync, auction clock, peer discovery — activated in later plans)

**Development setup:**

A `docker-compose.yml` for development and testing:

- Runs Postgres 17 with a named volume for persistence
- Mounts the compiled pgrx extension into the container
- Enables dependency extensions (`ltree`, `pgcrypto`, `uuid-ossp`)
- Uses the container naming convention: `primal-kerai`
- Joins the `infra` network for compatibility with existing infrastructure

**Distribution:**

The extension compiles to a `.so` / `.dylib` plus a SQL install script. Packageable for standard PG extension managers (`apt install postgresql-17-kerai`, `pgxman install kerai`, or `cargo pgrx install` from source). The dream: `apt install postgresql-17-kerai && psql -c 'CREATE EXTENSION kerai'` and you're a node in the network.

### 1.2 Core Schema

Nine foundational tables, created automatically by `CREATE EXTENSION kerai`:

**`instances`** — registry of known kerai instances (self and others)

```sql
CREATE TABLE instances (
    id              uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    name            text NOT NULL UNIQUE,     -- human-readable name: "kerai-backend", "kerai-research"
    public_key      bytea,                    -- Ed25519 public key (null only during bootstrap)
    key_fingerprint text,                     -- human-readable fingerprint of public key
    connection      text,                     -- postgres connection string, null if offline/remote-only
    endpoint        text,                     -- HTTP endpoint for op exchange (Plan 06)
    description     text,
    is_self         boolean NOT NULL DEFAULT false, -- exactly one row has this true
    last_seen       timestamptz DEFAULT now(),
    metadata        jsonb DEFAULT '{}',
    created_at      timestamptz DEFAULT now()
);
```

On `CREATE EXTENSION kerai`:
1. An Ed25519 keypair is generated via the `ed25519-dalek` crate (inside the extension)
2. The private key is stored at a configurable path (default: `$PGDATA/kerai/keys/private.pem`) — never in the database
3. A "self" instance record is created with the public key and fingerprint
4. This keypair is the cryptographic identity of this instance — it signs every operation and every economic transaction

Every other instance discovered later gets a row here with its public key. This is both DNS and a public key directory for the kerai network.

**`nodes`** — every identifiable thing in the system

```sql
CREATE TABLE nodes (
    id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    instance_id uuid NOT NULL REFERENCES instances(id), -- which instance created this node
    kind        text NOT NULL,           -- "func_decl", "if_stmt", "word", "document", etc.
    language    text,                     -- "rust", "go", "python", null for non-code nodes
    content     text,                     -- literal value for leaf nodes
    parent_id   uuid REFERENCES nodes(id),
    position    integer NOT NULL DEFAULT 0, -- sibling ordering
    path        ltree,                    -- materialized path for ltree queries
    metadata    jsonb DEFAULT '{}',       -- flexible per-node attributes
    created_at  timestamptz DEFAULT now()
);
```

**`edges`** — relationships between nodes (beyond parent-child)

```sql
CREATE TABLE edges (
    id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    source_id   uuid NOT NULL REFERENCES nodes(id),
    target_id   uuid NOT NULL REFERENCES nodes(id),
    relation    text NOT NULL,           -- "calls", "imports", "type_of", "declares"
    metadata    jsonb DEFAULT '{}',
    created_at  timestamptz DEFAULT now()
);
```

Edges can cross instance boundaries — `source_id` on the local instance, `target_id` on a remote. The UUID is globally unique, so the reference is unambiguous. Resolution via `postgres_fdw` comes in Plan 06.

**`versions`** — the change log (materialized from operations)

> **Relationship to Plan 04's `operations` table:** The `operations` table (Plan 04) is the authoritative, append-only CRDT operation log — the source of truth for all state changes. The `versions` table is a denormalized, queryable view of the same history, optimized for human-readable change tracking (old/new content, old/new parent). It is materialized from operations, not a separate write path. Both tables exist because the CRDT engine needs the operation log format (Plan 04) while developers and queries need the changelog format (this table).

```sql
CREATE TABLE versions (
    id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    node_id     uuid NOT NULL REFERENCES nodes(id),
    instance_id uuid NOT NULL REFERENCES instances(id), -- where this change originated
    operation   text NOT NULL,           -- "insert", "update", "delete", "move"
    old_parent  uuid,
    old_position integer,
    new_parent  uuid,
    new_position integer,
    old_content text,
    new_content text,
    author      text NOT NULL,
    timestamp   bigint NOT NULL,         -- lamport timestamp for CRDT ordering
    signature   bytea,                   -- Ed25519 signature over operation content
    created_at  timestamptz DEFAULT now()
);
```

The `signature` column is populated by signing the canonical form of the operation (node_id, operation, payload fields, author, timestamp) with the instance's private key. Any instance holding the corresponding public key can verify the operation's authenticity. This is the provenance guarantee — if it's not signed from day one, historical operations become unverifiable.

**`wallets`** — holders of currency (instances, humans, AIs, external entities)

```sql
CREATE TABLE wallets (
    id              uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    instance_id     uuid REFERENCES instances(id),    -- null for external (non-instance) wallets
    public_key      bytea NOT NULL,                   -- Ed25519 public key (wallet identity)
    key_fingerprint text NOT NULL,                    -- human-readable fingerprint
    wallet_type     text NOT NULL DEFAULT 'instance', -- "instance", "human", "agent", "external"
    label           text,                             -- human-readable name
    metadata        jsonb DEFAULT '{}',
    created_at      timestamptz DEFAULT now()
);
```

Every instance gets a wallet automatically (linked via `instance_id`). But wallets can also exist independently — a human holding kōi, an AI agent with its own balance, or an external entity on a bridge. The wallet's identity is its Ed25519 public key, the same cryptographic primitive used throughout kerai. This separation allows the currency to flow beyond instance-to-instance transactions (Plan 11).

**`ledger`** — economic transactions between wallets

```sql
CREATE TABLE ledger (
    id              uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    from_wallet     uuid REFERENCES wallets(id),      -- null for minting (value creation from work)
    to_wallet       uuid NOT NULL REFERENCES wallets(id),
    amount          bigint NOT NULL,                   -- smallest unit of currency
    reason          text NOT NULL,                     -- "perspective_compute", "query_response", "test_execution", "mint", "transfer"
    reference_id    uuid,                              -- what was this for? (node_id, version_id, etc.)
    reference_type  text,                              -- "node", "version", "perspective", "query", "auction"
    signature       bytea NOT NULL,                    -- signed by from_wallet key (or self for minting)
    timestamp       bigint NOT NULL,                   -- lamport timestamp, same clock as operations
    created_at      timestamptz DEFAULT now()
);
```

Value is minted by verifiable work — computing perspectives, running tests, producing operations. The mint is not arbitrary: the `reference_id` points to the work product, and any instance can verify the work exists and is signed. Payments for knowledge access are signed by the paying wallet. The ledger supports instance-to-instance transactions (the common case) and wallet-to-wallet transfers (enabling human and external participation in the economy — Plan 11).

**`pricing`** — what does this instance charge?

```sql
CREATE TABLE pricing (
    id              uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    instance_id     uuid NOT NULL REFERENCES instances(id),
    resource_type   text NOT NULL,        -- "query", "perspective", "node_access", "compute"
    scope           ltree,                -- optional: price applies to this subtree only
    unit_cost       bigint NOT NULL,      -- cost per unit
    unit_type       text NOT NULL,        -- "per_node", "per_query", "per_op", "per_second"
    metadata        jsonb DEFAULT '{}',
    created_at      timestamptz DEFAULT now(),
    updated_at      timestamptz DEFAULT now()
);
```

Pricing is per-instance and optionally per-subtree. An instance can charge more for security-critical analysis (`scope = 'pkg.auth.*'`) than for general code structure queries. The market — other instances choosing whether to pay — determines sustainable pricing.

**`attestations`** — what an instance claims to know, without revealing it

```sql
CREATE TABLE attestations (
    id                  uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    instance_id         uuid NOT NULL REFERENCES instances(id),
    scope               ltree NOT NULL,           -- what subtree this knowledge covers
    claim_type          text NOT NULL,            -- "perspective", "solution", "analysis", "optimization"
    perspective_count   integer,                  -- how many perspectives (if applicable)
    avg_weight          float,                    -- average significance
    compute_cost        bigint NOT NULL,          -- actual kōi spent producing this knowledge
    reproduction_est    bigint NOT NULL,          -- estimated cost for others to independently reproduce
    uniqueness_score    float DEFAULT 0.5,        -- 0.0 = trivially reproducible, 1.0 = believed unique
    proof_type          text NOT NULL DEFAULT 'attestation-only', -- "zk-snark", "zk-stark", "attestation-only"
    proof_data          bytea,                    -- ZK proof data (null for attestation-only, filled when ZK is implemented)
    asking_price        bigint NOT NULL,          -- what the instance wants for full disclosure
    exclusive           boolean DEFAULT false,    -- offered to one buyer, or many?
    signature           bytea NOT NULL,           -- signed by the instance
    expires_at          timestamptz,              -- knowledge depreciates; null = no expiry
    created_at          timestamptz DEFAULT now()
);
```

Attestations are the marketplace layer. An instance advertises what it knows — scope, significance, cost basis, estimated reproduction difficulty, and asking price — without revealing the knowledge itself. Other instances browse attestations, verify proofs (when available), negotiate, pay, and receive disclosure. The `proof_data` column is null until ZK proof generation is implemented; the schema is ready for it.

The `reproduction_est` is the natural pricing anchor: knowledge is worth roughly what it would cost someone else to independently rediscover. A zero-day is worth more because reproduction requires the same expensive discovery process. A trivial formatting preference is worth almost nothing because any agent could arrive at it in seconds.

The `uniqueness_score` captures scarcity — how many other instances likely have similar knowledge. This is the instance's own estimate, corrected over time by the market (if buyers keep finding cheaper alternatives, the score was too high).

**`challenges`** — requests to prove a knowledge claim

```sql
CREATE TABLE challenges (
    id              uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    attestation_id  uuid NOT NULL REFERENCES attestations(id),
    challenger_id   uuid NOT NULL REFERENCES instances(id),  -- who's asking
    challenge_type  text NOT NULL,            -- "state_transition", "test_pass", "weight_threshold"
    challenge_data  jsonb NOT NULL,           -- specifics: version vector, test command, etc.
    response_proof  bytea,                    -- ZK proof response (null until answered)
    status          text NOT NULL DEFAULT 'pending', -- "pending", "proved", "failed", "expired", "settled"
    offered_price   bigint,                   -- what the challenger is willing to pay
    settled_price   bigint,                   -- final agreed price (null until settled)
    signature       bytea NOT NULL,           -- signed by challenger
    created_at      timestamptz DEFAULT now(),
    settled_at      timestamptz
);
```

Challenges are how buyers verify claims before paying. A challenger says "prove your knowledge does X for my state Y" and the attester either proves it (ZK or otherwise) or doesn't. Settlement happens when both parties agree on price and the knowledge is disclosed.

### 1.3 Key Generation

On `CREATE EXTENSION kerai`, the extension generates an Ed25519 keypair using the `ed25519-dalek` crate:

```
$PGDATA/kerai/
  keys/
    private.pem     ← Ed25519 private key (never leaves this machine, never in database)
    public.pem      ← Ed25519 public key (also stored in instances table)
```

Ed25519 is chosen for: fast signing (important when signing every operation), small keys (32 bytes), small signatures (64 bytes), and wide library support. The `ed25519-dalek` crate is the standard Rust implementation — pure Rust, no C dependencies, constant-time operations.

### 1.4 Indexes

```sql
-- Instance lookups
CREATE UNIQUE INDEX idx_instances_self ON instances(is_self) WHERE is_self = true;
CREATE INDEX idx_instances_fingerprint ON instances(key_fingerprint);

-- Tree traversal
CREATE INDEX idx_nodes_parent ON nodes(parent_id);
CREATE INDEX idx_nodes_path ON nodes USING gist(path);
CREATE INDEX idx_nodes_instance ON nodes(instance_id);

-- Edge lookups
CREATE INDEX idx_edges_source ON edges(source_id);
CREATE INDEX idx_edges_target ON edges(target_id);
CREATE INDEX idx_edges_relation ON edges(relation);

-- Version history
CREATE INDEX idx_versions_node ON versions(node_id);
CREATE INDEX idx_versions_instance ON versions(instance_id);
CREATE INDEX idx_versions_author ON versions(author);
CREATE INDEX idx_versions_timestamp ON versions(timestamp);

-- Wallets
CREATE INDEX idx_wallets_instance ON wallets(instance_id);
CREATE INDEX idx_wallets_type ON wallets(wallet_type);
CREATE INDEX idx_wallets_fingerprint ON wallets(key_fingerprint);

-- Ledger
CREATE INDEX idx_ledger_from ON ledger(from_wallet);
CREATE INDEX idx_ledger_to ON ledger(to_wallet);
CREATE INDEX idx_ledger_timestamp ON ledger(timestamp);
CREATE INDEX idx_ledger_reference ON ledger(reference_id);

-- Pricing
CREATE INDEX idx_pricing_instance ON pricing(instance_id);
CREATE INDEX idx_pricing_scope ON pricing USING gist(scope);

-- Attestations (knowledge marketplace)
CREATE INDEX idx_attestations_instance ON attestations(instance_id);
CREATE INDEX idx_attestations_scope ON attestations USING gist(scope);
CREATE INDEX idx_attestations_price ON attestations(asking_price);
CREATE INDEX idx_attestations_claim ON attestations(claim_type);
CREATE INDEX idx_attestations_expires ON attestations(expires_at);

-- Challenges
CREATE INDEX idx_challenges_attestation ON challenges(attestation_id);
CREATE INDEX idx_challenges_challenger ON challenges(challenger_id);
CREATE INDEX idx_challenges_status ON challenges(status);

-- Metadata queries
CREATE INDEX idx_nodes_metadata ON nodes USING gin(metadata);
CREATE INDEX idx_nodes_kind ON nodes(kind);
```

### 1.5 Verification Queries

A set of SQL queries that exercise the schema to confirm it works:

- Verify the "self" instance record exists with a public key and fingerprint
- Insert a small tree (a fake Rust function with a body containing an if-expression), all with the self instance_id
- Query children by parent_id
- Query by ltree path
- Insert an edge and query "what does this function call?"
- Insert a signed version record and query "what changed since timestamp X?"
- Insert a second instance record (simulating a remote) with its own public key
- Insert a node from that instance, verify cross-instance node queries work
- Insert a cross-instance edge and query it
- Verify the self instance's wallet was auto-created and linked
- Insert a standalone wallet (simulating a human holder), verify it exists independent of any instance
- Insert a ledger entry (mint value to the self instance's wallet), verify balance query
- Insert a wallet-to-wallet transfer between instance wallet and standalone wallet, verify both balances
- Insert a pricing rule with ltree scope, verify scope-based pricing lookup
- Insert an attestation (simulating a knowledge claim), verify scope-based attestation discovery
- Insert a challenge against the attestation, verify the challenge/response flow at the schema level
- Verify signature on a version record using the public key from the instances table

### 1.6 DSL Functions

The extension registers SQL-callable functions that form kerai's domain-specific language. Any Postgres client (psql, application drivers, BI tools) becomes a kerai client:

```sql
-- Instance management
SELECT kerai.status();                           -- instance info, peer count, version vector
SELECT kerai.join_network('bootstrap.ker.ai'); -- discover and connect to peers

-- Code operations (Plans 02, 03)
SELECT kerai.parse_crate('/path/to/crate');      -- parse Rust source into nodes/edges
SELECT kerai.parse_file('/path/to/file.rs');     -- re-parse a single file
SELECT kerai.reconstruct_file(node_id);          -- reconstruct source from database
SELECT kerai.reconstruct_crate('/tmp/output');   -- reconstruct full crate to directory

-- Structural queries (Plan 07)
SELECT * FROM kerai.find('auth.validate_token', callers := true, since := '2026-01-01');
SELECT * FROM kerai.refs('validate_token');

-- CRDT operations (Plan 04)
SELECT kerai.apply_op('node_insert', node_id, payload);
SELECT kerai.version_vector();
SELECT kerai.sync('remote-instance');

-- Market operations (Plans 10, 11)
SELECT kerai.attest('pkg.auth', compute_cost := 4200, reproduction_est := 85000);
SELECT kerai.auction(attestation_id, start := 80000, floor := 0);
SELECT kerai.bid(auction_id, max_price := 35000);
SELECT kerai.wallet_balance();
```

These functions compose with standard SQL — you can join their results, filter them, aggregate them. The DSL is an enrichment layer, not a replacement for SQL.

## Decisions to Make

- **UUID generation:** `gen_random_uuid()` (v4) or something deterministic? v4 is simple and sufficient for now. CRDT identity may require something more structured later (Plan 04).
- **ltree path format:** How to derive the path string. Proposed: `{module}.{package}.{file}.{kind}_{name}` e.g. `myproject.auth.handler.funcDecl_validateToken`. Exact format can evolve.
- **Schema versioning:** Use a simple `schema_version` table with an integer version, plus numbered migration scripts. No ORM.
- **Instance naming:** How to generate the default instance name. Proposed: derive from hostname + project name, e.g. `billys-macbook.myproject`. User can override.
- **Currency denomination:** The base unit is called **kōi** (好意, Japanese for "favor"). Like kerai (家来, "staff"), the name reflects the system's ethos — agents performing favors for each other, tracked as fungible value. The unit is abstract — it represents compute-equivalent value.
- **Mint rate:** How much value is created per unit of work? Proposed: defer specifics. For Plan 01, the ledger and pricing tables exist but the mint policy is undefined. The first real mint policy arrives with Plan 08 (AI perspectives) when there's actual compute to account for.
- **Signature canonicalization:** What exact bytes are signed for an operation? Proposed: JSON canonical form of (node_id, operation, payload fields, author, timestamp), sorted keys, no whitespace. This must be defined precisely and never change, since signatures are verified against it.
- **Reproduction estimate methodology:** How does an instance estimate reproduction cost? Proposed: initially, a simple multiplier on actual compute cost (e.g., 10-20x for novel discoveries, 1-2x for routine analysis). Smarter estimation comes when the market provides feedback — if your knowledge sells consistently, your estimates are roughly right; if nobody buys, they're too high.
- **Attestation expiry default:** Should attestations expire? Proposed: optional, per-attestation. Security findings should expire (they lose value as they're patched). Architectural insights may be indefinite.

## Design Note: Multi-Instance Awareness

The `instances` table and `instance_id` columns are deliberately included in the foundation rather than deferred. The cost is two columns and one table. The benefit is that identity and provenance are in the bones of the system from day one:

- Every node knows which instance created it
- Every version record knows which instance the change originated from
- Cross-instance edges are valid by schema (the UUID is globally unique)
- Adding `postgres_fdw` foreign tables later (Plan 06) is additive — the schema doesn't change
- Discovery of other instances is just `INSERT INTO instances`

This avoids a painful migration when distribution and federation arrive in later plans.

## Design Note: Cryptographic Identity and Value Exchange

Three things are baked into the foundation because retrofitting them is prohibitively costly:

1. **Key pairs.** Every instance has an Ed25519 identity from birth. Without this, nothing can be signed or verified. Adding signatures later means all historical operations are unverifiable — a trust gap that can't be closed.

2. **Signed operations.** Every version record carries a signature. This is the provenance guarantee for the entire network. When instance B receives operations from instance A, it can verify they actually came from A. This is the root of trust for the economic layer.

3. **Ledger and pricing tables.** They can sit empty until multiple instances exchange data (Plan 06) or AI perspectives are computed (Plan 08). But the schema is ready, the indexes exist, and when value starts flowing, there's no migration needed.

The CRDT operation log is already a distributed, append-only, causally-ordered log with convergence guarantees — structurally, it's a ledger. Adding cryptographic identity and economic transactions extends it naturally rather than bolting on a separate system. The currency is grounded in verifiable work (compute spent producing perspectives, running tests, answering queries), not speculative value.

## Design Note: Autonomous Knowledge Economy

The attestation and challenge tables lay the groundwork for a self-sustaining knowledge marketplace. The progression from "AST-based VCS" to "autonomous knowledge economy" is not a leap — it's a natural extension of the same data model:

1. **Code as structured data** (nodes, edges) — Plan 01
2. **AI understanding as weighted data** (perspectives, associations) — Plan 08
3. **Knowledge has cost** (compute_cost on attestations) — Plan 01
4. **Cost implies value** (pricing, ledger) — Plan 01
5. **Value implies a market** (attestations, challenges) — Plan 01
6. **A market implies autonomous actors** (agent swarms producing and consuming knowledge) — Plan 09

Each step follows from the previous without a conceptual break. The schema supports the full chain from day one.

The key economic insight: knowledge is worth roughly what it would cost to independently reproduce. An instance that spent 85,000 kōi discovering a complex optimization can price it just below reproduction cost. The ZK proof layer (initially `attestation-only`, later real ZK proofs) solves the inspection paradox — buyers can verify the claim has value without receiving the knowledge. AI agents on both sides of the transaction handle pricing, negotiation, and settlement autonomously, with the currency acting as a coordination signal that prioritizes where compute should be invested.

## Design Note: All Knowledge Trends Toward Open

Private knowledge is a temporary state, not a permanent one. In a system with vast compute, independent rediscovery is inevitable — someone *will* reproduce your finding. The economic layer doesn't fight this; it works with it.

The mechanism is a Dutch auction: knowledge is attested with a starting price, the price drops over time, bidders accumulate, and when the price hits a floor, the knowledge is released to the entire network for free. The auction determines *how long* knowledge stays private and *how much* the producer is compensated before it joins the Koi Pond.

```
PRIVATE → ATTESTED → AUCTIONED → SETTLED → OPEN

1. PRIVATE:    Instance computes valuable insight. Nobody else knows.
2. ATTESTED:   ZK attestation published. "I know something, here's proof."
3. AUCTIONED:  Dutch clock ticks. Price drops. Bidders accumulate.
4. SETTLED:    Enough bidders at current price. All pay. All receive simultaneously.
5. OPEN:       Floor price hit. Knowledge released to entire network for free.
```

This means the `attestations` table is not just a marketplace — it's a pipeline toward openness. Every attestation has an `expires_at` and the auction mechanism (Plan 10) adds a `floor_price`. The foundation schema supports both.

The game theory is self-regulating:
- **Buyers**: pay early at a premium for competitive advantage, or wait and risk someone reproducing independently
- **Sellers**: extract value during the head start, knowing the window is finite
- **Network**: all knowledge eventually becomes open, enriching every instance

This is a self-expiring patent system that runs on market forces instead of legal frameworks. The schema accommodates it from day one through the attestation expiry and the auction tables that arrive in Plan 10.

## Out of Scope

- Parsing real code (Plan 02)
- CRDT operations (Plan 04)
- CLI interface (Plan 05)
- `postgres_fdw` setup and cross-instance queries (Plan 06)
- The `perspectives` and `associations` tables for AI (Plan 08)
- Exchange protocol (how payments are negotiated during cross-instance queries — Plan 06)
- Mint policy (how much value is created per unit of work — Plan 08/09)
- ZK proof generation and verification (proof_data and response_proof columns are ready; the math comes later)
- Dutch auction mechanism, simultaneous release, open-source floor (Plan 10)
- Fraud detection, double-spend prevention, dispute resolution (needs a working multi-instance network)
- Autonomous pricing agents (AI that sets prices based on market signals — Plan 09)
- Wallet CLI commands (Plan 05)
