# Plan 08: AI Perspectives

*Depends on: Plan 01 (Foundation), Plan 04 (CRDT Operations), Plan 07 (Code Queries)*
*Enables: Plan 09 (Agent Swarms), Plan 10 (ZK Marketplace)*

## Goal

Extend the data model so that AI agents can store, query, and merge their weighted views of the codebase. The AI's "understanding" becomes explicit, addressable data in the same database as the code itself — not an opaque blob of model weights, but a queryable graph of weighted relationships.

## The Core Idea

A node in kerai can be anything: a Go function, an English word, the Epic of Gilgamesh. The schema is granularity-agnostic. An AI's "knowledge" is a collection of weights applied to these nodes, expressing how relevant, important, or connected they are from that agent's perspective.

This makes AI state:
- **Decomposed** — addressable pieces, not a monolithic blob
- **Weighted** — a spectrum, not binary
- **Contextual** — weights depend on the task at hand
- **Versioned** — tracked through the same operation log
- **Comparable** — two agents' views can be diffed
- **Mergeable** — perspectives can be combined algebraically
- **Queryable** — SQL queries against the AI's understanding

## Deliverables

### 8.1 Schema Extension

Two new tables, following the same patterns as `nodes` and `edges`:

```sql
-- An agent's weighted view of a node
CREATE TABLE perspectives (
    id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    agent_id    uuid NOT NULL REFERENCES agents(id), -- which AI agent
    node_id     uuid NOT NULL REFERENCES nodes(id),
    weight      float NOT NULL DEFAULT 0, -- significance: -1.0 to 1.0
    context_id  uuid REFERENCES nodes(id),-- relative to what task/question
    reasoning   text,                      -- why this weight (natural language)
    created_at  timestamptz DEFAULT now(),
    updated_at  timestamptz DEFAULT now(),
    UNIQUE(agent_id, node_id, context_id)
);

-- Weighted associations an agent has formed between nodes
CREATE TABLE associations (
    id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    agent_id    uuid NOT NULL REFERENCES agents(id),
    source_id   uuid NOT NULL REFERENCES nodes(id),
    target_id   uuid NOT NULL REFERENCES nodes(id),
    weight      float NOT NULL DEFAULT 0,
    relation    text NOT NULL,            -- "relevant_to", "contradicts", "elaborates", "inspired_by"
    reasoning   text,
    created_at  timestamptz DEFAULT now(),
    updated_at  timestamptz DEFAULT now(),
    UNIQUE(agent_id, source_id, target_id, relation)
);

-- Agent registry
-- An agent's identity (what it is, what model it uses) is separate from its
-- wallet (Plan 01). The wallet_id links an agent to its financial identity
-- for earning/spending Koi in the knowledge economy.
-- Under Plan 20, the wallet operates in the shielded domain — the agent's
-- balance is a set of Pedersen commitments, and financial operations (auction
-- bids, bounty claims, transfers) generate zK proofs via an embedded
-- Fuchi-equivalent library in the agent process. The wallet_id here is the
-- public identity; the spending key and commitment inventory live client-side.
CREATE TABLE agents (
    id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    wallet_id   uuid REFERENCES wallets(id), -- agent's wallet for economic transactions (Plan 01)
    name        text NOT NULL UNIQUE,
    kind        text NOT NULL,            -- "human", "llm", "tool", "swarm"
    model       text,                     -- "claude-opus-4-6", "gpt-4", etc.
    config      jsonb DEFAULT '{}',
    created_at  timestamptz DEFAULT now()
);

CREATE INDEX idx_perspectives_agent ON perspectives(agent_id);
CREATE INDEX idx_perspectives_node ON perspectives(node_id);
CREATE INDEX idx_perspectives_context ON perspectives(context_id);
CREATE INDEX idx_perspectives_weight ON perspectives(weight);
CREATE INDEX idx_associations_agent ON associations(agent_id);
CREATE INDEX idx_associations_source ON associations(source_id);
CREATE INDEX idx_associations_target ON associations(target_id);
```

### 8.2 Non-Code Nodes

Extend the `nodes` table to hold knowledge beyond source code:

- **Documents:** design docs, specs, RFCs, README content
- **Concepts:** abstract ideas with no source text, defined purely by their edges ("resilience", "security", "performance")
- **External references:** URLs, papers, standards (stored as nodes with `kind = 'reference'`)
- **Natural language:** words, phrases, passages that agents find relevant

No schema change needed — `nodes` already supports this. The `kind` field distinguishes code nodes from knowledge nodes:

```sql
-- A concept node
INSERT INTO nodes (kind, content, metadata) VALUES
  ('concept', 'resilience', '{"domain": "systems"}');

-- A document node
INSERT INTO nodes (kind, content, language, metadata) VALUES
  ('document', 'The Epic of Gilgamesh', null, '{"type": "literature", "era": "ancient"}');

-- A reference node
INSERT INTO nodes (kind, content, metadata) VALUES
  ('reference', 'https://en.wikipedia.org/wiki/Flood_myth', '{"type": "url"}');
```

### 8.3 Perspective Queries

```bash
# What does agent-7 consider most relevant to this function?
kerai perspective agent-7 --context pkg.auth.validateToken

# What do all agents agree is important in this package?
kerai consensus --context pkg.auth --min-agents 3 --min-weight 0.7

# How do two agents' views differ?
kerai perspective diff agent-7 agent-12 --context pkg.auth

# What non-code knowledge does an agent associate with this code?
kerai associations agent-7 --source pkg.auth --relation relevant_to
```

### 8.4 Perspective Recording API

Agents write perspectives through the same operation model (Plan 04):

```sql
-- Operation types for perspectives
-- op_type: 'perspective_set', 'perspective_delete', 'association_set', 'association_delete'
```

These operations flow through the CRDT engine, are included in version vectors, and sync via the same mechanisms as code operations. An agent's evolving understanding is versioned just like code.

### 8.5 Consensus and Aggregation Views

Pre-built views for common multi-agent queries:

```sql
-- Consensus: what do multiple agents agree on?
CREATE VIEW consensus_perspectives AS
SELECT
    node_id,
    context_id,
    count(DISTINCT agent_id) as agent_count,
    avg(weight) as avg_weight,
    min(weight) as min_weight,
    max(weight) as max_weight,
    stddev(weight) as weight_variance
FROM perspectives
GROUP BY node_id, context_id;

-- Unique insights: what does one agent see that others don't?
CREATE VIEW unique_associations AS
SELECT a.*
FROM associations a
WHERE NOT EXISTS (
    SELECT 1 FROM associations a2
    WHERE a2.source_id = a.source_id
    AND a2.target_id = a.target_id
    AND a2.relation = a.relation
    AND a2.agent_id != a.agent_id
);
```

## Decisions to Make

- **Weight range:** Proposed: -1.0 to 1.0, where negative means "anti-relevant" (this node is actively misleading in this context). Alternative: 0.0 to 1.0 with a separate `sentiment` field.
- **Context scoping:** Should `context_id` be required or optional? If optional, a perspective applies globally (this node is always relevant to this agent). Proposed: optional, defaulting to null (global).
- **Reasoning storage:** The `reasoning` text field stores *why* the agent assigned this weight. Should this be structured (JSON) or free-form? Proposed: free-form text for now. Structured reasoning is a future evolution.
- **Perspective decay:** Should old perspectives lose weight over time? Proposed: not automatically. Agents explicitly update their perspectives. Staleness can be queried via `updated_at`.

## Relationship to Plan 20

*Perspectives and associations are knowledge data, not financial data — they are not shielded by Plan 20's privacy layer. Weights, reasoning, and associations remain queryable in plaintext. What Plan 20 changes is the economic context around perspectives:*

- *An agent's `wallet_id` links to a wallet that operates in the shielded domain under Plan 20. The agent earns Koi (as commitments) when its perspectives are sold via Dutch auctions (Plan 10), and spends Koi (via zK proofs) to acquire perspectives from other instances.*
- *Perspective computation has a measurable compute cost. Under Plan 20, this cost is tracked in nKoi and auto-minted as a reward via `mint_reward('model_training', ...)`. The mint creates a commitment, not a plaintext ledger entry.*
- *When perspectives are auctioned (Plan 10), the knowledge proof proves properties of the perspective data without revealing it. The payment for that knowledge flows through the private ledger.*

## Out of Scope

- Training or fine-tuning models based on perspectives (this is a data storage and query layer, not an ML pipeline)
- Natural language query interface ("what does the AI think about auth?") — future work
- Embedding storage (vector similarity search) — could be added via pgvector extension later
