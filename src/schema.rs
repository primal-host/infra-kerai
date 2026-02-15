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
extension_sql!(
    r#"
CREATE TABLE kerai.ledger (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    from_wallet     UUID REFERENCES kerai.wallets(id),
    to_wallet       UUID NOT NULL REFERENCES kerai.wallets(id),
    amount          BIGINT NOT NULL CHECK (amount > 0),
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
    unit_cost     BIGINT NOT NULL,
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
