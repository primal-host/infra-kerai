use pgrx::prelude::*;

// Schema bootstrap — marker for dependency ordering.
// The kerai schema is created automatically by PostgreSQL
// because of `schema = kerai` in kerai.control.
extension_sql!(
    r#"
-- schema kerai is auto-created by PostgreSQL via .control file
"#,
    name = "schema_bootstrap"
);

// Table: instances — peer identity registry
extension_sql!(
    r#"
CREATE TABLE kerai.instances (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name            TEXT NOT NULL,
    public_key      BYTEA NOT NULL,
    key_fingerprint TEXT NOT NULL UNIQUE,
    connection      TEXT,
    endpoint        TEXT,
    description     TEXT,
    is_self         BOOLEAN NOT NULL DEFAULT false,
    last_seen       TIMESTAMPTZ,
    metadata        JSONB DEFAULT '{}'::jsonb,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Only one self instance allowed
CREATE UNIQUE INDEX idx_instances_is_self
    ON kerai.instances (is_self) WHERE is_self = true;

CREATE INDEX idx_instances_name ON kerai.instances (name);
CREATE INDEX idx_instances_last_seen ON kerai.instances (last_seen);
"#,
    name = "table_instances",
    requires = ["schema_bootstrap"]
);

// Table: nodes — AST node storage
extension_sql!(
    r#"
CREATE TABLE kerai.nodes (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    instance_id UUID NOT NULL REFERENCES kerai.instances(id),
    kind        TEXT NOT NULL,
    language    TEXT,
    content     TEXT,
    parent_id   UUID REFERENCES kerai.nodes(id),
    position    INTEGER NOT NULL DEFAULT 0,
    path        ltree,
    metadata    JSONB DEFAULT '{}'::jsonb,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_nodes_instance ON kerai.nodes (instance_id);
CREATE INDEX idx_nodes_kind ON kerai.nodes (kind);
CREATE INDEX idx_nodes_parent ON kerai.nodes (parent_id);
CREATE INDEX idx_nodes_path ON kerai.nodes USING gist (path);
CREATE INDEX idx_nodes_language ON kerai.nodes (language) WHERE language IS NOT NULL;
CREATE INDEX idx_nodes_parent_position ON kerai.nodes (parent_id, position);
CREATE INDEX idx_nodes_content_fts ON kerai.nodes
    USING gin (to_tsvector('english', COALESCE(content, '')));
"#,
    name = "table_nodes",
    requires = ["table_instances"]
);

// Table: edges — relationships between nodes
extension_sql!(
    r#"
CREATE TABLE kerai.edges (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    source_id   UUID NOT NULL REFERENCES kerai.nodes(id),
    target_id   UUID NOT NULL REFERENCES kerai.nodes(id),
    relation    TEXT NOT NULL,
    metadata    JSONB DEFAULT '{}'::jsonb,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_edges_source ON kerai.edges (source_id);
CREATE INDEX idx_edges_target ON kerai.edges (target_id);
CREATE INDEX idx_edges_relation ON kerai.edges (relation);
CREATE UNIQUE INDEX idx_edges_unique_rel
    ON kerai.edges (source_id, target_id, relation);
"#,
    name = "table_edges",
    requires = ["table_nodes"]
);

// Table: versions — edit history with Lamport timestamps
extension_sql!(
    r#"
CREATE TABLE kerai.versions (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    node_id     UUID NOT NULL REFERENCES kerai.nodes(id),
    instance_id UUID NOT NULL REFERENCES kerai.instances(id),
    operation   TEXT NOT NULL,
    old_parent  UUID,
    new_parent  UUID,
    old_position INTEGER,
    new_position INTEGER,
    old_content TEXT,
    new_content TEXT,
    author      TEXT NOT NULL,
    timestamp   BIGINT NOT NULL,
    signature   BYTEA,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_versions_node ON kerai.versions (node_id);
CREATE INDEX idx_versions_instance ON kerai.versions (instance_id);
CREATE INDEX idx_versions_timestamp ON kerai.versions (timestamp);
CREATE INDEX idx_versions_author ON kerai.versions (author);
CREATE INDEX idx_versions_node_timestamp
    ON kerai.versions (node_id, timestamp);
"#,
    name = "table_versions",
    requires = ["table_nodes"]
);

// Table: wallets — token wallets for instances and system
extension_sql!(
    r#"
CREATE TABLE kerai.wallets (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    instance_id     UUID REFERENCES kerai.instances(id),
    public_key      BYTEA NOT NULL,
    key_fingerprint TEXT NOT NULL UNIQUE,
    wallet_type     TEXT NOT NULL DEFAULT 'instance',
    label           TEXT,
    metadata        JSONB DEFAULT '{}'::jsonb,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_wallets_instance ON kerai.wallets (instance_id);
CREATE INDEX idx_wallets_type ON kerai.wallets (wallet_type);
"#,
    name = "table_wallets",
    requires = ["table_instances"]
);

// Table: ledger — immutable transaction log
// All amounts are in nKoi (nano-Koi): 1 Koi = 1,000,000,000 nKoi (10^9).
// 9 whole digits + implicit decimal + 9 fractional digits, stored as BIGINT.
extension_sql!(
    r#"
CREATE TABLE kerai.ledger (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    from_wallet     UUID REFERENCES kerai.wallets(id),
    to_wallet       UUID NOT NULL REFERENCES kerai.wallets(id),
    amount          BIGINT NOT NULL CHECK (amount > 0),  -- nKoi
    reason          TEXT NOT NULL,
    reference_id    UUID,
    reference_type  TEXT,
    signature       BYTEA,
    timestamp       BIGINT NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_ledger_from ON kerai.ledger (from_wallet);
CREATE INDEX idx_ledger_to ON kerai.ledger (to_wallet);
CREATE INDEX idx_ledger_reason ON kerai.ledger (reason);
CREATE INDEX idx_ledger_timestamp ON kerai.ledger (timestamp);
CREATE INDEX idx_ledger_reference
    ON kerai.ledger (reference_type, reference_id)
    WHERE reference_id IS NOT NULL;
"#,
    name = "table_ledger",
    requires = ["table_wallets"]
);

// Table: pricing — per-instance resource pricing
extension_sql!(
    r#"
CREATE TABLE kerai.pricing (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    instance_id   UUID NOT NULL REFERENCES kerai.instances(id),
    resource_type TEXT NOT NULL,
    scope         ltree,
    unit_cost     BIGINT NOT NULL,  -- nKoi
    unit_type     TEXT NOT NULL DEFAULT 'token',
    metadata      JSONB DEFAULT '{}'::jsonb,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_pricing_instance ON kerai.pricing (instance_id);
CREATE INDEX idx_pricing_resource ON kerai.pricing (resource_type);
CREATE INDEX idx_pricing_scope ON kerai.pricing USING gist (scope);
CREATE UNIQUE INDEX idx_pricing_unique
    ON kerai.pricing (instance_id, resource_type, scope);
"#,
    name = "table_pricing",
    requires = ["table_instances"]
);

// Table: attestations — knowledge claims
extension_sql!(
    r#"
CREATE TABLE kerai.attestations (
    id                UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    instance_id       UUID NOT NULL REFERENCES kerai.instances(id),
    scope             ltree NOT NULL,
    claim_type        TEXT NOT NULL,
    perspective_count INTEGER NOT NULL DEFAULT 0,
    avg_weight        DOUBLE PRECISION NOT NULL DEFAULT 0.0,
    compute_cost      BIGINT NOT NULL DEFAULT 0,
    reproduction_est  BIGINT NOT NULL DEFAULT 0,
    uniqueness_score  DOUBLE PRECISION NOT NULL DEFAULT 0.0,
    proof_type        TEXT,
    proof_data        BYTEA,
    asking_price      BIGINT,
    exclusive         BOOLEAN NOT NULL DEFAULT false,
    signature         BYTEA,
    expires_at        TIMESTAMPTZ,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_attestations_instance ON kerai.attestations (instance_id);
CREATE INDEX idx_attestations_scope ON kerai.attestations USING gist (scope);
CREATE INDEX idx_attestations_claim ON kerai.attestations (claim_type);
CREATE INDEX idx_attestations_expires
    ON kerai.attestations (expires_at)
    WHERE expires_at IS NOT NULL;
"#,
    name = "table_attestations",
    requires = ["table_instances"]
);

// Table: challenges — dispute resolution for attestations
extension_sql!(
    r#"
CREATE TABLE kerai.challenges (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    attestation_id  UUID NOT NULL REFERENCES kerai.attestations(id),
    challenger_id   UUID NOT NULL REFERENCES kerai.instances(id),
    challenge_type  TEXT NOT NULL,
    challenge_data  JSONB,
    response_proof  BYTEA,
    status          TEXT NOT NULL DEFAULT 'pending',
    offered_price   BIGINT,
    settled_price   BIGINT,
    signature       BYTEA,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    settled_at      TIMESTAMPTZ
);

CREATE INDEX idx_challenges_attestation ON kerai.challenges (attestation_id);
CREATE INDEX idx_challenges_challenger ON kerai.challenges (challenger_id);
CREATE INDEX idx_challenges_status ON kerai.challenges (status);
"#,
    name = "table_challenges",
    requires = ["table_attestations"]
);

// Table: agents — AI agent registry
extension_sql!(
    r#"
CREATE TABLE kerai.agents (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    wallet_id   UUID REFERENCES kerai.wallets(id),
    name        TEXT NOT NULL UNIQUE,
    kind        TEXT NOT NULL,
    model       TEXT,
    config      JSONB DEFAULT '{}'::jsonb,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_agents_kind ON kerai.agents(kind);
"#,
    name = "table_agents",
    requires = ["table_wallets"]
);

// Table: perspectives — weighted agent views of nodes
extension_sql!(
    r#"
CREATE TABLE kerai.perspectives (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    agent_id    UUID NOT NULL REFERENCES kerai.agents(id),
    node_id     UUID NOT NULL REFERENCES kerai.nodes(id),
    weight      DOUBLE PRECISION NOT NULL DEFAULT 0,
    context_id  UUID REFERENCES kerai.nodes(id),
    reasoning   TEXT,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(agent_id, node_id, context_id)
);

CREATE INDEX idx_perspectives_agent ON kerai.perspectives(agent_id);
CREATE INDEX idx_perspectives_node ON kerai.perspectives(node_id);
CREATE INDEX idx_perspectives_context ON kerai.perspectives(context_id) WHERE context_id IS NOT NULL;
CREATE INDEX idx_perspectives_weight ON kerai.perspectives(weight);
"#,
    name = "table_perspectives",
    requires = ["table_agents", "table_nodes"]
);

// Table: associations — weighted relationships between nodes from an agent's view
extension_sql!(
    r#"
CREATE TABLE kerai.associations (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    agent_id    UUID NOT NULL REFERENCES kerai.agents(id),
    source_id   UUID NOT NULL REFERENCES kerai.nodes(id),
    target_id   UUID NOT NULL REFERENCES kerai.nodes(id),
    weight      DOUBLE PRECISION NOT NULL DEFAULT 0,
    relation    TEXT NOT NULL,
    reasoning   TEXT,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(agent_id, source_id, target_id, relation)
);

CREATE INDEX idx_associations_agent ON kerai.associations(agent_id);
CREATE INDEX idx_associations_source ON kerai.associations(source_id);
CREATE INDEX idx_associations_target ON kerai.associations(target_id);
"#,
    name = "table_associations",
    requires = ["table_agents", "table_nodes"]
);

// View: consensus_perspectives — aggregated weight stats per node across agents
extension_sql!(
    r#"
CREATE VIEW kerai.consensus_perspectives AS
SELECT
    node_id,
    context_id,
    count(DISTINCT agent_id) AS agent_count,
    avg(weight) AS avg_weight,
    min(weight) AS min_weight,
    max(weight) AS max_weight,
    stddev(weight) AS stddev_weight
FROM kerai.perspectives
GROUP BY node_id, context_id;
"#,
    name = "view_consensus_perspectives",
    requires = ["table_perspectives"]
);

// View: unique_associations — associations held by only one agent
extension_sql!(
    r#"
CREATE VIEW kerai.unique_associations AS
SELECT a.*
FROM kerai.associations a
WHERE NOT EXISTS (
    SELECT 1 FROM kerai.associations a2
    WHERE a2.source_id = a.source_id
      AND a2.target_id = a.target_id
      AND a2.relation = a.relation
      AND a2.agent_id != a.agent_id
);
"#,
    name = "view_unique_associations",
    requires = ["table_associations"]
);

// Table: auctions — Dutch auction for knowledge attestations
extension_sql!(
    r#"
CREATE TABLE kerai.auctions (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    attestation_id      UUID NOT NULL REFERENCES kerai.attestations(id),
    seller_wallet       UUID NOT NULL REFERENCES kerai.wallets(id),
    auction_type        TEXT NOT NULL DEFAULT 'dutch',
    starting_price      BIGINT NOT NULL,
    floor_price         BIGINT NOT NULL DEFAULT 0,
    current_price       BIGINT NOT NULL,
    price_decrement     BIGINT NOT NULL,
    decrement_interval  INTERVAL NOT NULL,
    min_bidders         INTEGER DEFAULT 1,
    release_type        TEXT NOT NULL DEFAULT 'simultaneous',
    status              TEXT NOT NULL DEFAULT 'active',
    settled_price       BIGINT,
    open_sourced        BOOLEAN DEFAULT false,
    open_sourced_at     TIMESTAMPTZ,
    open_delay_hours    INTEGER NOT NULL DEFAULT 24,
    signature           BYTEA,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    settled_at          TIMESTAMPTZ
);

CREATE INDEX idx_auctions_attestation ON kerai.auctions(attestation_id);
CREATE INDEX idx_auctions_status ON kerai.auctions(status);
CREATE INDEX idx_auctions_seller ON kerai.auctions(seller_wallet);
CREATE INDEX idx_auctions_floor ON kerai.auctions(floor_price);
"#,
    name = "table_auctions",
    requires = ["table_attestations", "table_wallets"]
);

// Table: bids — buyer commitments to auctions
extension_sql!(
    r#"
CREATE TABLE kerai.bids (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    auction_id      UUID NOT NULL REFERENCES kerai.auctions(id),
    bidder_wallet   UUID NOT NULL REFERENCES kerai.wallets(id),
    max_price       BIGINT NOT NULL,
    signature       BYTEA,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_bids_auction ON kerai.bids(auction_id);
CREATE INDEX idx_bids_bidder ON kerai.bids(bidder_wallet);
"#,
    name = "table_bids",
    requires = ["table_auctions", "table_wallets"]
);

// Alter challenges — add auction_id for marketplace integration
extension_sql!(
    r#"
ALTER TABLE kerai.challenges ADD COLUMN IF NOT EXISTS auction_id UUID REFERENCES kerai.auctions(id);
CREATE INDEX IF NOT EXISTS idx_challenges_auction ON kerai.challenges(auction_id) WHERE auction_id IS NOT NULL;
"#,
    name = "alter_challenges_auction",
    requires = ["table_challenges", "table_auctions"]
);

// Table: tasks — swarm task definitions
extension_sql!(
    r#"
CREATE TABLE kerai.tasks (
    id               UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    description      TEXT NOT NULL,
    scope_node_id    UUID REFERENCES kerai.nodes(id),
    success_command  TEXT NOT NULL,
    budget_ops       INTEGER,
    budget_seconds   INTEGER,
    status           TEXT NOT NULL DEFAULT 'pending',
    agent_kind       TEXT,
    agent_model      TEXT,
    agent_count      INTEGER,
    swarm_id         UUID REFERENCES kerai.agents(id),
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_tasks_status ON kerai.tasks (status);
CREATE INDEX idx_tasks_scope ON kerai.tasks (scope_node_id) WHERE scope_node_id IS NOT NULL;
CREATE INDEX idx_tasks_swarm ON kerai.tasks (swarm_id) WHERE swarm_id IS NOT NULL;
"#,
    name = "table_tasks",
    requires = ["table_nodes", "table_agents"]
);

// Table: test_results — UNLOGGED for write performance
extension_sql!(
    r#"
CREATE UNLOGGED TABLE kerai.test_results (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    task_id         UUID NOT NULL REFERENCES kerai.tasks(id),
    agent_id        UUID NOT NULL REFERENCES kerai.agents(id),
    version_vector  JSONB NOT NULL DEFAULT '{}'::jsonb,
    passed          BOOLEAN NOT NULL,
    output          TEXT,
    duration_ms     INTEGER,
    ops_count       INTEGER,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_test_results_task ON kerai.test_results (task_id);
CREATE INDEX idx_test_results_agent ON kerai.test_results (agent_id);
CREATE INDEX idx_test_results_task_passed ON kerai.test_results (task_id, passed);
CREATE INDEX idx_test_results_task_created ON kerai.test_results (task_id, created_at);
"#,
    name = "table_test_results",
    requires = ["table_tasks", "table_agents"]
);

// Table: bounties — task bounties funded by wallets
extension_sql!(
    r#"
CREATE TABLE kerai.bounties (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    poster_wallet   UUID NOT NULL REFERENCES kerai.wallets(id),
    scope           ltree NOT NULL,
    description     TEXT NOT NULL,
    success_command TEXT,
    reward          BIGINT NOT NULL CHECK (reward > 0),
    status          TEXT NOT NULL DEFAULT 'open',
    claimed_by      UUID REFERENCES kerai.wallets(id),
    verified_at     TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at      TIMESTAMPTZ
);

CREATE INDEX idx_bounties_scope ON kerai.bounties USING gist (scope);
CREATE INDEX idx_bounties_status ON kerai.bounties (status);
CREATE INDEX idx_bounties_poster ON kerai.bounties (poster_wallet);
CREATE INDEX idx_bounties_reward ON kerai.bounties (reward);
"#,
    name = "table_bounties",
    requires = ["table_wallets"]
);

// Table: operations — CRDT operation log (stub for Plan 04)
extension_sql!(
    r#"
CREATE TABLE kerai.operations (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    instance_id UUID NOT NULL REFERENCES kerai.instances(id),
    op_type     TEXT NOT NULL,
    node_id     UUID,
    author      TEXT NOT NULL,
    lamport_ts  BIGINT NOT NULL,
    author_seq  BIGINT NOT NULL,
    payload     JSONB NOT NULL DEFAULT '{}'::jsonb,
    signature   BYTEA,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_operations_instance ON kerai.operations (instance_id);
CREATE INDEX idx_operations_node ON kerai.operations (node_id) WHERE node_id IS NOT NULL;
CREATE INDEX idx_operations_author ON kerai.operations (author);
CREATE INDEX idx_operations_lamport ON kerai.operations (lamport_ts);
CREATE UNIQUE INDEX idx_operations_author_seq
    ON kerai.operations (author, author_seq);
"#,
    name = "table_operations",
    requires = ["table_instances"]
);

// Table: version_vector — CRDT version tracking (stub for Plan 04)
extension_sql!(
    r#"
CREATE TABLE kerai.version_vector (
    author  TEXT PRIMARY KEY,
    max_seq BIGINT NOT NULL DEFAULT 0
);
"#,
    name = "table_version_vector",
    requires = ["schema_bootstrap"]
);

// Table: reward_schedule — configurable emission rates per work type
extension_sql!(
    r#"
CREATE TABLE kerai.reward_schedule (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    work_type   TEXT NOT NULL UNIQUE,
    reward      BIGINT NOT NULL CHECK (reward > 0),  -- nKoi
    enabled     BOOLEAN NOT NULL DEFAULT true,
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
"#,
    name = "table_reward_schedule",
    requires = ["schema_bootstrap"]
);

// Table: reward_log — audit trail for auto-mints
extension_sql!(
    r#"
CREATE TABLE kerai.reward_log (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    work_type   TEXT NOT NULL,
    reward      BIGINT NOT NULL,  -- nKoi
    wallet_id   UUID NOT NULL REFERENCES kerai.wallets(id),
    details     JSONB DEFAULT '{}'::jsonb,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
"#,
    name = "table_reward_log",
    requires = ["table_wallets"]
);

// Table: repositories — ingested git repositories
extension_sql!(
    r#"
CREATE TABLE kerai.repositories (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    instance_id UUID NOT NULL REFERENCES kerai.instances(id),
    url         TEXT NOT NULL,
    name        TEXT NOT NULL,
    local_path  TEXT NOT NULL,
    head_commit TEXT,
    last_sync   TIMESTAMPTZ,
    node_id     UUID REFERENCES kerai.nodes(id),
    metadata    JSONB DEFAULT '{}'::jsonb,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX idx_repositories_url ON kerai.repositories (instance_id, url);
CREATE INDEX idx_repositories_name ON kerai.repositories (name);
"#,
    name = "table_repositories",
    requires = ["table_instances", "table_nodes"]
);

// Seed data: default reward schedule (amounts in nKoi; 1 Koi = 1,000,000,000 nKoi)
extension_sql!(
    r#"
INSERT INTO kerai.reward_schedule (work_type, reward) VALUES
    ('parse_file',        10000000000),  -- 10 Koi
    ('parse_crate',       50000000000),  -- 50 Koi
    ('parse_markdown',    10000000000),  -- 10 Koi
    ('create_version',     5000000000),  --  5 Koi
    ('bounty_settlement', 20000000000),  -- 20 Koi
    ('peer_sync',         15000000000),  -- 15 Koi
    ('model_training',    25000000000),  -- 25 Koi
    ('mirror_repo',      100000000000);  -- 100 Koi
"#,
    name = "seed_reward_schedule",
    requires = ["table_reward_schedule"]
);

// Alter wallets: add nonce column for replay protection
extension_sql!(
    r#"
ALTER TABLE kerai.wallets ADD COLUMN nonce BIGINT NOT NULL DEFAULT 0;
"#,
    name = "alter_wallets_nonce",
    requires = ["table_wallets"]
);

// Table: model_vocab — node UUID ↔ dense integer index per model
extension_sql!(
    r#"
CREATE TABLE kerai.model_vocab (
    model_id    UUID NOT NULL REFERENCES kerai.agents(id),
    node_id     UUID NOT NULL REFERENCES kerai.nodes(id),
    token_idx   INTEGER NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (model_id, token_idx),
    UNIQUE (model_id, node_id)
);

CREATE INDEX idx_model_vocab_node ON kerai.model_vocab (node_id);
"#,
    name = "table_model_vocab",
    requires = ["table_agents", "table_nodes"]
);

// Table: model_weights — one row per named tensor per agent
extension_sql!(
    r#"
CREATE TABLE kerai.model_weights (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    agent_id    UUID NOT NULL REFERENCES kerai.agents(id),
    tensor_name TEXT NOT NULL,
    tensor_data BYTEA NOT NULL,
    shape       INTEGER[] NOT NULL,
    version     BIGINT NOT NULL DEFAULT 1,
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (agent_id, tensor_name)
);

CREATE INDEX idx_model_weights_agent ON kerai.model_weights (agent_id);
"#,
    name = "table_model_weights",
    requires = ["table_agents"]
);

// Table: training_runs — audit log
extension_sql!(
    r#"
CREATE TABLE kerai.training_runs (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    agent_id    UUID NOT NULL REFERENCES kerai.agents(id),
    config      JSONB NOT NULL,
    walk_type   TEXT NOT NULL,
    scope       ltree,
    n_sequences INTEGER NOT NULL,
    n_steps     INTEGER NOT NULL,
    final_loss  DOUBLE PRECISION,
    duration_ms INTEGER,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_training_runs_agent ON kerai.training_runs (agent_id);
"#,
    name = "table_training_runs",
    requires = ["table_agents"]
);

// Table: inference_log — UNLOGGED for perf
extension_sql!(
    r#"
CREATE UNLOGGED TABLE kerai.inference_log (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    agent_id      UUID NOT NULL,
    context_nodes UUID[] NOT NULL,
    predicted     UUID NOT NULL,
    score         DOUBLE PRECISION NOT NULL,
    selected      BOOLEAN DEFAULT false,
    cost_koi      BIGINT DEFAULT 0,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_inference_log_agent ON kerai.inference_log (agent_id);
"#,
    name = "table_inference_log",
    requires = ["table_agents"]
);
