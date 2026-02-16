# Plan 10: ZK Marketplace

*Depends on: Plan 06 (Distribution), Plan 08 (AI Perspectives)*
*Enables: Plan 11 (External Economy)*

## Goal

Implement the zero-knowledge proof layer and Dutch auction mechanism that allow instances to prove they possess valuable knowledge, auction it to interested buyers, release it simultaneously to all successful bidders, and open-source it when the price hits the floor. At the end of this plan, the kerai network has a self-regulating knowledge economy where all knowledge trends toward open.

## The Core Principle: All Knowledge Trends Toward Open

Private knowledge is a temporary state. In a system with vast compute, independent rediscovery is inevitable. The marketplace doesn't fight this — it formalizes the depreciation curve:

1. Knowledge starts private (valuable, scarce)
2. It's attested and auctioned (provably real, price dropping)
3. Buyers pay for early access (competitive advantage)
4. The price hits floor → knowledge goes open to the entire network (joins the commons)

The auction determines *how long* knowledge stays private and *how much* the producer is compensated. Not *whether* it eventually becomes public — that's a given.

## Deliverables

### 10.1 Dutch Auction Engine

```sql
CREATE TABLE auctions (
    id                  uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    attestation_id      uuid NOT NULL REFERENCES attestations(id),
    seller_wallet       uuid NOT NULL REFERENCES wallets(id), -- seller's wallet (receives payment)
    auction_type        text NOT NULL DEFAULT 'dutch',
    starting_price      bigint NOT NULL,          -- initial asking price
    floor_price         bigint NOT NULL DEFAULT 0, -- open-source trigger (0 = always goes open)
    current_price       bigint NOT NULL,          -- drops over time
    price_decrement     bigint NOT NULL,          -- kōi per interval
    decrement_interval  interval NOT NULL,        -- how often price drops
    min_bidders         integer DEFAULT 1,        -- minimum bidders to trigger settlement
    release_type        text NOT NULL DEFAULT 'simultaneous', -- all bidders receive at once
    status              text NOT NULL DEFAULT 'active', -- "active", "settled", "open_sourced", "expired"
    settled_price       bigint,                   -- what bidders actually paid
    open_sourced        boolean DEFAULT false,     -- true when floor hit or post-settlement release
    open_sourced_at     timestamptz,
    signature           bytea NOT NULL,           -- signed by seller
    created_at          timestamptz DEFAULT now(),
    settled_at          timestamptz
);

CREATE TABLE bids (
    id              uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    auction_id      uuid NOT NULL REFERENCES auctions(id),
    bidder_wallet   uuid NOT NULL REFERENCES wallets(id), -- bidder's wallet (pays on settlement)
    max_price       bigint NOT NULL,        -- highest price this bidder will pay
    signature       bytea NOT NULL,         -- signed commitment to pay
    created_at      timestamptz DEFAULT now()
);

CREATE INDEX idx_auctions_attestation ON auctions(attestation_id);
CREATE INDEX idx_auctions_status ON auctions(status);
CREATE INDEX idx_auctions_seller ON auctions(seller_wallet);
CREATE INDEX idx_auctions_floor ON auctions(floor_price);
CREATE INDEX idx_bids_auction ON bids(auction_id);
CREATE INDEX idx_bids_bidder ON bids(bidder_wallet);
```

**How the Dutch clock works:**

```
starting_price = 80,000 kōi
floor_price = 0
price_decrement = 1,000 kōi
decrement_interval = 1 hour

Hour 0:  current_price = 80,000  (no bidders)
Hour 10: current_price = 70,000  (2 bidders at max_price >= 70,000)
Hour 30: current_price = 50,000  (5 bidders)
Hour 55: current_price = 25,000  (12 bidders, min_bidders met → SETTLE)
  → All 12 bidders pay 25,000. Knowledge released simultaneously.
  → Seller receives 12 × 25,000 = 300,000 kōi.
  → 24 hours after settlement, knowledge goes open to the network.

OR:

Hour 80: current_price = 0 (floor hit, no settlement)
  → Knowledge released to entire network for free.
  → Seller receives 0 kōi but knowledge enters the commons.
```

### 10.2 Settlement and Simultaneous Release

When settlement triggers (enough bidders at current price, or seller manually triggers):

1. All bidders with `max_price >= current_price` are identified
2. Each pays `current_price` (uniform price — everyone pays the same, regardless of their max bid)
3. Signed ledger entries are created for each payment
4. The knowledge (operations, perspectives, associations) is disclosed to all winning bidders simultaneously
5. A configurable delay (default 24 hours) after settlement, the knowledge goes open

The simultaneous release is critical — no first-mover advantage among buyers. Everyone who bid gets the knowledge at the same instant. This prevents a buyer from reselling to other bidders at markup.

### 10.3 Open-Source Release

When knowledge goes open (floor hit or post-settlement delay):

1. The attestation's underlying data (perspectives, operations, associations) is published to the network
2. Any instance can sync the data via the normal CRDT operation exchange (Plan 06)
3. The attestation is marked `open_sourced = true`
4. Ledger records the event as a mint with `reason = 'open_source_release'` and `amount = 0`
5. The data is now part of the commons — it syncs freely via normal distribution mechanisms

```bash
# Query the open knowledge commons
kerai market browse --open-sourced --scope "pkg.auth.*"

# Sync open-sourced knowledge from the network
kerai market sync --open-only
```

### 10.4 Zero-Knowledge Proof Generation

Implement ZK proof generation to replace `attestation-only` mode. An instance can prove:

- **Existence**: "I have a perspective on a node matching pattern X with weight > Y"
- **State transition**: "Applying my operations to your state S produces test result R"
- **Aggregate properties**: "I have N perspectives with average weight > Y covering scope S"

Without revealing: the specific operations, the specific nodes, the actual weights, or the perspective content.

**Proposed approach:** ZK-STARKs over the Postgres query results, implemented in Rust inside the pgrx extension.

1. Seller runs the relevant query locally (e.g., `SELECT count(*), avg(weight) FROM perspectives p JOIN nodes n ON p.node_id = n.id WHERE n.path <@ 'pkg.auth'`)
2. The query execution trace is captured
3. A STARK proof is generated over the trace using Rust ZK libraries (`arkworks`, `risc0`, or `winterfell`), proving the query returned the claimed result on the actual data
4. The proof is stored in `attestations.proof_data` and `challenges.response_proof`
5. Buyers verify the proof without access to the underlying data

Because the extension runs inside Postgres, it has direct access to the data for proof generation — no serialization overhead. Proof verification is also a SQL-callable function:

```sql
-- Generate a ZK proof for an attestation
SELECT kerai.generate_proof(attestation_id);

-- Verify a proof received from another instance
SELECT kerai.verify_proof(attestation_id, proof_data);
```

The **auction clock** runs as a pgrx background worker — it periodically scans active auctions, decrements prices, checks for settlement conditions, and triggers open-source release when the floor is hit.

This is the hardest part of Plan 10. The schema is ready (`proof_data bytea`); the math is the deliverable. Rust's ZK ecosystem (`arkworks` for STARKs, `bellman` for SNARKs, `risc0` for general-purpose ZK-VM) is the most mature outside of specialized C++ libraries.

### 10.5 Challenge Protocol

Extend the `challenges` table from Plan 01 with auction integration:

```sql
ALTER TABLE challenges ADD COLUMN auction_id uuid REFERENCES auctions(id);
```

The full protocol:

```bash
# Buyer browses attestations
kerai market browse --scope "pkg.auth.*" --max-price 50000

# Buyer challenges a specific attestation
kerai market challenge <attestation-id> \
  --type state_transition \
  --test "cargo test --package auth" \
  --my-state '{"billy": 147}' \
  --offer 30000

# Seller responds with ZK proof
kerai market prove <challenge-id>

# Buyer verifies and bids
kerai market bid <auction-id> --max-price 35000

# Auction settles when conditions are met
# Knowledge is released simultaneously to all winning bidders
```

### 10.6 Market Observability

```bash
# Browse active auctions
kerai market browse [--scope path] [--max-price N] [--min-weight N]

# View auction status and bid history
kerai market status <auction-id>

# View your instance's earnings and spending
kerai market balance

# View the knowledge commons (open-sourced knowledge)
kerai market commons [--scope path] [--since date]

# Market statistics
kerai market stats
# Output:
#   Active auctions: 47
#   Settled this week: 12 (total value: 450,000 kōi)
#   Open-sourced this week: 8
#   Average settlement price: 37,500 kōi
#   Most active scope: pkg.auth.* (15 auctions)
```

### 10.7 Pricing Strategy Agents

AI agents that help instances set optimal auction parameters:

- Analyze historical settlement data to estimate demand
- Monitor reproduction cost trends (is independent discovery getting cheaper?)
- Recommend starting price, floor price, decrement rate based on knowledge scope and uniqueness
- Track market signals: if nobody bids on `pkg.auth.*` knowledge above 20,000 kōi, stop pricing it at 80,000

This is the point where AI agents participate on both sides of the market — producing knowledge, pricing it, bidding for knowledge they need, and deciding whether to buy or reproduce independently.

## Game Theory

**For buyers:** Bid early at a premium and get the knowledge sooner (competitive advantage over other instances that don't have it). Or wait for the price to drop — but risk someone else reproducing the knowledge independently, or the seller settling with earlier bidders. There's no dominant strategy; the tradeoff depends on how urgently you need the knowledge and how confident you are in independent reproduction.

**For sellers:** Set starting price based on reproduction cost estimate. Set floor based on how quickly you think independent discovery will happen. The market corrects bad estimates — if nobody bids, your reproduction estimate was too optimistic and you should lower prices. If the auction settles quickly at a high price, you underestimated the value.

**For the network:** Every auction resolves in one of two ways — either buyers pay and receive the knowledge (and it goes open shortly after), or the floor is hit and it goes open for free. Either way, the knowledge enters the commons. The only question is whether the producer gets compensated first. Over time, the commons grows monotonically. The network gets richer with every auction, whether it settles or not.

**The open-source floor creates a natural pressure toward generosity.** An instance sitting on knowledge and pricing it too high watches the clock tick down toward free. Better to settle at a reasonable price than receive nothing when the floor hits. This keeps the market efficient and prevents knowledge hoarding.

## Decisions to Make

- **ZK library:** Which ZK proof system to use? ZK-STARKs are transparent (no trusted setup) and post-quantum secure but produce larger proofs. ZK-SNARKs are more compact but require a trusted setup. Proposed: start with STARKs for transparency, optimize later if proof size is a problem.
- **Post-settlement open delay:** How long after settlement before knowledge goes open to the network? Proposed: configurable per-auction, default 24 hours. This gives buyers a brief exclusive window. Set to 0 for immediate open-sourcing after settlement.
- **Floor price = 0 as default:** Should all auctions eventually go open? Proposed: yes, default `floor_price = 0`. Sellers can set a non-zero floor if they want minimum compensation, but the design ethos is that all knowledge trends toward open.
- **Multi-auction for same knowledge:** Can an instance run multiple auctions for the same attestation (different buyer groups, different price schedules)? Proposed: no, one auction per attestation. Prevents seller from double-dipping. If the auction expires, a new one can be created.

## Out of Scope

- Cross-chain interoperability (connecting the kerai economy to external cryptocurrencies)
- Legal framework for knowledge ownership (the system operates on cryptographic proof, not legal IP)
- Reputation systems beyond ledger history (trust scores, reliability ratings — future work)
