# Plan 13: Micro-GPT — In-Database Learned Intelligence

*Depends on: Plan 01 (Foundation), Plan 04 (CRDT), Plan 08 (Perspectives), Plan 11 (Economy)*

## Context

Kerai stores knowledge as a graph of nodes, edges, perspectives, and associations. Currently, search ranking (`context_search`) uses hand-tuned heuristics: FTS `ts_rank` multiplied by `(1.0 + avg_perspective_weight)`. This works but cannot learn from the graph's structure or user behavior.

Inspired by Karpathy's MicroGPT — a complete GPT in ~150 lines of pure Python ("this file is the complete algorithm, everything else is just efficiency") — we embed a tiny transformer directly in the pgrx extension. Pure Rust, zero external ML deps. The vocabulary is not characters — it's **kerai nodes**. The model learns to predict the next relevant node given a walk through the graph, making it a neural ranker aware of edge topology, perspective weights, and hierarchical structure.

## Design Decisions

### Vocabulary: Nodes as Tokens

Each node in `kerai.nodes` maps to a dense integer index via `kerai.model_vocab`. The model works in index-space internally, mapping back to UUIDs at the API boundary. Dynamic — grows as nodes are added; new tokens get zero-initialized embeddings until trained.

### Training Sequences: Graph Walks

Training data is generated from the graph itself:

- **tree**: Depth-first parent→child traversal ordered by `position`. Natural for code ASTs.
- **edge**: Follow edges from each node, producing `(source, target, ...)` chains.
- **perspective**: Random walk where transition probability is weighted by an agent's perspective weight on the neighbor. High-perspective nodes visited more.
- **random**: Uniform random walk over edges (baseline).

### Architecture (following Karpathy)

- Embedding dimension: 32 (configurable 16–128)
- Attention heads: 4 (configurable 1–8)
- Layers: 1–2 (configurable)
- Context length: 16 nodes (configurable 8–64)
- RMSNorm, ReLU, residual connections, learned positional embeddings
- Weight tying between token embeddings and output head
- ~64K params at (vocab=1000, dim=32, heads=4, layers=1) ≈ 256KB

### Forward with Tensors, Backward by Hand

No tensor-level autograd. Forward pass uses efficient `Vec<f32>` tensor ops (matmul, softmax, RMSNorm). Backward pass uses manually-derived gradient formulas per layer — straightforward at this scale and far more efficient than scalar autograd for matrix operations.

### Weights as BYTEA

Stored in `kerai.model_weights`, one row per named tensor per agent. BYTEA of little-endian f32 — 4 bytes/float vs ~12 for JSONB. Serialization roundtrips via `to_bytes`/`from_bytes`.

### Training as CRDT Operations

Each gradient update produces an `update_model_weights` CRDT op with a base64-encoded f32 delta. Deltas are additive — two instances training independently produce weight updates that can be summed (federated averaging). The CRDT merge applies the delta to local weights.

### Economy Integration

Training mints Koi (produces value). Inference costs Koi (consumes value). Bounties can commission model training on specific ltree scopes. All amounts are denominated in nKoi (1 Koi = 1,000,000,000 nKoi).

*Under Plan 20, training rewards are minted as Pedersen commitments rather than plaintext ledger entries. The reward amount remains public (tied to the verifiable training run in `training_runs`), but the recipient's accumulated balance is private. Inference costs are deducted via private transfers — the agent's Fuchi-equivalent generates a proof that the agent can afford the inference, without revealing total holdings. The `cost_koi` field in `inference_log` records the public cost (needed for market pricing); the actual payment is a nullified commitment.*

## Module Structure

```
src/microgpt/
    mod.rs          — pg_extern functions
    tensor.rs       — Vec<f32> tensor ops (matmul, softmax, rms_norm, cross_entropy)
    model.rs        — MicroGPT struct, forward, backward, serialize/deserialize
    optimizer.rs    — Adam optimizer
    walks.rs        — Graph walk sequence generator (SPI queries)
```

## Schema Additions (`src/schema.rs`)

```sql
-- Node UUID ↔ dense integer index per model
CREATE TABLE kerai.model_vocab (
    model_id    UUID NOT NULL REFERENCES kerai.agents(id),
    node_id     UUID NOT NULL REFERENCES kerai.nodes(id),
    token_idx   INTEGER NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (model_id, token_idx),
    UNIQUE (model_id, node_id)
);

-- Weight tensors: one row per named tensor per agent
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

-- Training run audit log
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

-- Inference log for feedback learning (UNLOGGED for perf)
CREATE UNLOGGED TABLE kerai.inference_log (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    agent_id      UUID NOT NULL,
    context_nodes UUID[] NOT NULL,
    predicted     UUID NOT NULL,
    score         DOUBLE PRECISION NOT NULL,
    selected      BOOLEAN DEFAULT false,
    cost_koi      BIGINT DEFAULT 0,           -- nKoi (public cost for pricing; actual payment is a private commitment under Plan 20)
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);
```

## Key Data Structures

### Tensor (`src/microgpt/tensor.rs`)

```rust
pub struct Tensor {
    pub data: Vec<f32>,
    pub shape: Vec<usize>,
}
// Methods: zeros, randn, matmul, add, mul_scalar, relu, softmax,
//          rms_norm, cross_entropy_loss, from_bytes, to_bytes
```

### Model (`src/microgpt/model.rs`)

```rust
pub struct ModelConfig {
    pub vocab_size: usize,
    pub dim: usize,        // default 32
    pub n_heads: usize,    // default 4
    pub n_layers: usize,   // default 1
    pub context_len: usize,// default 16
}

pub struct TransformerLayer {
    pub q_proj: Tensor,   // [dim, dim]
    pub k_proj: Tensor,
    pub v_proj: Tensor,
    pub o_proj: Tensor,
    pub ff_up: Tensor,    // [dim, 4*dim]
    pub ff_down: Tensor,  // [4*dim, dim]
    pub norm1: Tensor,    // [dim]
    pub norm2: Tensor,    // [dim]
}

pub struct MicroGPT {
    pub config: ModelConfig,
    pub token_emb: Tensor,    // [vocab_size, dim]
    pub pos_emb: Tensor,      // [context_len, dim]
    pub layers: Vec<TransformerLayer>,
    pub final_norm: Tensor,   // [dim]
    // lm_head tied to token_emb (transposed)
}
// Methods: new, forward, backward, predict_next,
//          to_weight_map, from_weight_map
```

### Adam (`src/microgpt/optimizer.rs`)

```rust
pub struct Adam {
    pub lr: f32, pub beta1: f32, pub beta2: f32, pub eps: f32,
    pub step: usize,
    pub m: Vec<f32>,  // first moment
    pub v: Vec<f32>,  // second moment
}
```

## pg_extern Functions (`src/microgpt/mod.rs`)

| Function | Signature | Description |
|----------|-----------|-------------|
| `create_model` | `(agent_name, dim?, n_heads?, n_layers?, context_len?, scope?) -> JsonB` | Init random weights, build vocab from graph |
| `train_model` | `(agent_name, walk_type?, n_sequences?, n_steps?, lr?, scope?, perspective_agent?) -> JsonB` | Train on graph walks, return loss history |
| `predict_next` | `(agent_name, context: JsonB, top_k?) -> JsonB` | Given node sequence, predict next nodes |
| `neural_search` | `(agent_name, query_text, context_nodes?, limit?) -> JsonB` | FTS candidates re-ranked by model |
| `ensemble_predict` | `(agent_names: JsonB, context: JsonB, top_k?) -> JsonB` | Average logits from multiple models |
| `model_info` | `(agent_name) -> JsonB` | Architecture, param count, training history |
| `delete_model` | `(agent_name) -> JsonB` | Remove weights and vocab |
| `record_selection` | `(inference_id: Uuid) -> JsonB` | Mark inference log entry as selected |

## CRDT Operation Types

Added to `VALID_OP_TYPES` in `src/crdt/operations.rs`:

- `create_model` — Initialize model weights for an agent
- `update_model_weights` — Apply weight delta (payload: `{agent_id, tensor_name, delta: base64 f32, step, lr}`)
- `delete_model` — Remove model weights
- `train_step` — Audit log entry for a training step

Merge strategy for `update_model_weights`: decode delta, element-wise add to local tensor. Federated averaging emerges naturally.

## CLI Commands (`cli/src/commands/model.rs`)

```
kerai model create  --agent NAME --dim 32 --heads 4 --layers 1 --scope "crate.kerai"
kerai model train   --agent NAME --walks tree --sequences 200 --steps 100 --lr 0.001
kerai model predict --agent NAME --context "uuid1,uuid2,uuid3" --top-k 10
kerai model search  --agent NAME --query "authentication" --top-k 20
kerai model ensemble --agents "a,b,c" --context "uuid1,uuid2" --top-k 10
kerai model info    --agent NAME
kerai model delete  --agent NAME
```

## Web Routes (`web/src/routes/models.rs`)

```
POST   /api/models              -> create_model
POST   /api/models/train        -> train_model
POST   /api/models/predict      -> predict_next
GET    /api/models/search       -> neural_search
POST   /api/models/ensemble     -> ensemble_predict
GET    /api/models/:agent/info  -> model_info
DELETE /api/models/:agent       -> delete_model
POST   /api/models/feedback     -> record_selection
```

## Implementation Steps

### Step 1: Tensor library

**Created** `src/microgpt/tensor.rs`

`Tensor` struct with `Vec<f32>` data and `Vec<usize>` shape. Methods: `zeros`, `randn_xavier` (Xavier init using `rand`), `matmul` (2D), `batched_matmul`, `add`, `add_inplace`, `mul_scalar`, `mul_elementwise`, `relu`, `relu_mask`, `softmax` (numerically stable with max subtraction), `rms_norm`, `cross_entropy_loss`, `from_bytes`/`to_bytes` (little-endian f32), `transpose`, `slice_row`, `embed_lookup`, `reshape`, `as_2d`.

### Step 2: Model architecture

**Created** `src/microgpt/model.rs`, `src/microgpt/optimizer.rs`

`MicroGPT::new()` — Xavier-initialized weights. `forward()` — token embed + pos embed -> RMSNorm -> for each layer: multi-head attention (Q/K/V projections, causal mask, softmax, output projection, residual) -> MLP (up-project, ReLU, down-project, residual) -> final norm -> logits via transposed token embeddings.

`backward()` — manual gradient computation for each layer in reverse. `backward_with_tokens()` adds proper embedding gradient scatter.

`Adam::step()` — standard Adam update with bias correction.

`predict_next()` — forward pass, softmax on final logits, return top-k (index, probability) pairs.

`train_step()` — forward + backward + Adam on a batch of sequences.

### Step 3: Schema + storage

**Modified** `src/schema.rs` — added `model_vocab`, `model_weights`, `training_runs`, `inference_log` tables.

**Created** `src/microgpt/mod.rs` — wired module, implemented all 8 pg_extern functions with economy integration (mint reward after training, deduct cost for inference).

**Modified** `src/lib.rs` — added `mod microgpt;`

### Step 4: Graph walk generator

**Created** `src/microgpt/walks.rs`

`build_vocab(scope?)` — SELECT id FROM kerai.nodes, assign dense indices, INSERT into model_vocab.

`generate_walks(walk_type, ...)` — SPI queries to produce `Vec<Vec<usize>>` sequences:
- **tree**: parent->children adjacency from vocab, DFS from random roots
- **edge**: follow explicit edges N hops deep
- **perspective**: join perspectives table, weighted random selection of next node
- **random**: combine parent-child + edge adjacency, bidirectional uniform random

### Step 5: Training loop

**Implemented** in `src/microgpt/mod.rs` — `train_model` pg_extern: load weights from DB, generate walks, train loop with forward/backward/Adam, save weights back, log training run, mint reward.

### Step 6: Inference functions

**Implemented** in `src/microgpt/mod.rs` — `predict_next` (forward pass + top-k), `neural_search` (FTS candidates re-ranked by model score: `fts_rank * (1 + neural_score)`), `ensemble_predict` (average logits from multiple models), `model_info`, `record_selection`.

### Step 7: CRDT integration

**Modified** `src/crdt/operations.rs` — added 4 new op types and their apply handlers. `update_model_weights` decodes base64 delta, adds element-wise to local tensor BYTEA.

### Step 8: Economy integration

**Modified** `src/microgpt/mod.rs` — mint reward after training, deduct cost before inference.

**Modified** `src/schema.rs` — seeded `model_training` in reward_schedule (25 koi).

### Step 9: CLI commands

**Created** `cli/src/commands/model.rs` — 7 subcommands: create, train, predict, search, ensemble, info, delete.

**Modified** `cli/src/commands/mod.rs` — added `pub mod model;` and 7 Command variants.

**Modified** `cli/src/main.rs` — added `ModelAction` enum and dispatch.

### Step 10: Web routes

**Created** `web/src/routes/models.rs` — 8 route handlers following existing patterns.

**Modified** `web/src/routes/mod.rs` — added model routes to router.

### Step 11: Tests

**Modified** `src/lib.rs` — added 10 pg_test functions:

- `test_tensor_matmul` — known 2x2 matrix multiply result
- `test_tensor_softmax` — probabilities sum to 1.0 per row
- `test_forward_pass_shape` — model produces logits of correct shape [seq, vocab]
- `test_weight_roundtrip` — serialize -> deserialize -> identical forward pass
- `test_train_loss_decreases` — train 50 steps on repeating sequence, verify loss drops
- `test_predict_next_returns_results` — predictions exist and probabilities <= 1.0
- `test_create_model` — parse source, create model, verify weights and vocab in DB
- `test_model_info` — verify info returns expected fields
- `test_delete_model` — delete model, verify weights removed from DB
- `test_tensor_byte_roundtrip` — serialize -> deserialize -> bit-exact floats

## Key Files Modified

| File | Change |
|------|--------|
| `src/lib.rs` | Add `mod microgpt;` + 10 tests |
| `src/schema.rs` | Add 4 tables + seed reward_schedule |
| `src/crdt/operations.rs` | Add 4 op types + apply handlers |
| `cli/src/main.rs` | Add Model command variant |
| `cli/src/commands/mod.rs` | Add `pub mod model;` + 7 variants |
| `web/src/routes/mod.rs` | Add model routes |

## Key Files Created

| File | Purpose |
|------|---------|
| `src/microgpt/mod.rs` | 8 pg_extern functions |
| `src/microgpt/tensor.rs` | Minimal tensor library |
| `src/microgpt/model.rs` | MicroGPT transformer |
| `src/microgpt/optimizer.rs` | Adam optimizer |
| `src/microgpt/walks.rs` | Graph walk sequence generator |
| `cli/src/commands/model.rs` | 7 CLI subcommands |
| `web/src/routes/models.rs` | 8 REST endpoints |

## Verification

1. `LC_ALL=C CARGO_TARGET_DIR="$(pwd)/tgt" cargo pgrx test pg17` — all 156 tests pass (146 existing + 10 new)
2. `CARGO_TARGET_DIR="$(pwd)/tgt" cargo build -p kerai-web` — web crate compiles
3. `CARGO_TARGET_DIR="$(pwd)/tgt" cargo build -p kerai-cli` — CLI compiles
