# Kerai (ker.ai): Implementation Plans — Overview

*2026-02-15*

Kerai is an AST-based version control system backed by Postgres, using CRDTs for conflict-free convergence, with cryptographic identity and a built-in knowledge economy — designed to scale from solo development to million-agent swarms trading computed knowledge autonomously.

## Plan Sequence

Each plan builds on the ones before it. The dependency chain is linear through the first six plans, then fans out. The foundation (Plan 01) carries more weight than a typical "setup" plan — it includes cryptographic identity, the economic ledger, the attestation marketplace, and the challenge/response protocol, because these are impossible to retrofit.

```
01 Foundation
├─ 02 Rust Parser ─── 03 Source Reconstruction ─┐
├─ 04 CRDT Operations ─────────────────────────┐│
│                                               ││
│  05 CLI ←──────────────────── 02 + 03 + 04 ──┘│
│  ├─ 06 Distribution ←──────── 04 + 05         │
│  │   ├─ 09 Agent Swarms ←── 04 + 06 + 08      │
│  │   └─ 10 ZK Marketplace ← 06 + 08           │
│  ├─ 07 Code Queries ←─────── 05               │
│  │   └─ 08 AI Perspectives ← 01 + 04 + 07 ───┘
│  └─ 11 External Economy ←── 05 + 10
│
│  12 Knowledge Editor ←──── 01 + 02 + 04 + 06 + 08
│
│  13 MicroGPT ←─────────── 01
│  14 Native Currency ←──── 01
│  15 Comment Handling ←─── 02 + 03
```

| Plan | Title | Depends On | Delivers |
|------|-------|------------|----------|
| [01](01-foundation.md) | Foundation | — | Postgres schema, Docker setup, crypto identity, ledger, pricing, attestations, challenges |
| [02](02-rust-parser.md) | Rust Parser | 01 | Parse Rust source into the database (dogfooding) |
| [03](03-source-reconstruction.md) | Source Reconstruction | 02 | Reconstruct Rust source files from the database |
| [04](04-crdt-operations.md) | CRDT Operations | 01 | Operation log, version vectors, deterministic merge |
| [05](05-cli.md) | CLI | 02, 03, 04 | The `kerai` command-line interface |
| [06](06-distribution.md) | Distribution | 04, 05 | Clone, push, pull, cross-instance sync, `postgres_fdw` |
| [07](07-code-queries.md) | Code Queries | 05 | Structural query language for code insight |
| [08](08-ai-perspectives.md) | AI Perspectives | 01, 04, 07 | Weighted nodes, agent views, knowledge valuation, compute cost tracking |
| [09](09-agent-swarms.md) | Agent Swarms | 04, 06, 08 | Massive concurrent agents, autonomous pricing, market participation |
| [10](10-zk-marketplace.md) | ZK Marketplace | 06, 08 | Zero-knowledge proofs, Dutch auctions, simultaneous release, open-source floor |
| [11](11-external-economy.md) | External Economy | 05, 10 | Wallets, token bridge, exchange listing, bounties, fiat on/off ramps |
| [12](12-kerai-web.md) | Knowledge Editor | 01, 02, 04, 06, 08 | Web-based editor, document parsers, AI-assisted writing, corpus ingestion, real-time collaboration |
| [13](13-microgpt.md) | MicroGPT | 01 | Tiny transformer model running inside Postgres for local inference |
| [14](14-native-currency.md) | Native Currency | 01 | Kōi token minting, wallets, transfers, reward schedules |
| [15](15-comment-handling.md) | Comment Handling | 02, 03 | Source normalizer, comment grouping, placement classification, string-literal exclusion, comment-preserving reconstruction |

## Design Principles

- **Postgres is the engine.** The repo is a database. Where it runs is a connection string.
- **Nodes are universal.** A node can be a token, a function, a package, a concept, or the Epic of Gilgamesh. Same table, same queries.
- **CRDTs guarantee convergence.** No merge conflicts by construction. Operations commute.
- **The AST is the source of truth.** Text files are a rendered view, not the canonical form.
- **Rust first.** Kerai is built in Rust (pgrx extension). Parsing Rust first means we dogfood on our own codebase. Go and other languages follow — the node/edge model is language-agnostic.
- **Postgres extension, not application.** Kerai is a pgrx extension that runs *inside* Postgres, not an application that *uses* Postgres. `CREATE EXTENSION kerai` and you have the full system. The CLI is a thin client. Any Postgres client is a kerai client.
- **Cryptographic identity from birth.** Every instance has an Ed25519 keypair. Every operation is signed. Provenance is verifiable. This cannot be added later.
- **Knowledge has value.** Computing perspectives costs real resources. That cost implies value. Value implies a market. The economic layer is in the schema from day one.
- **The economy is autonomous.** AI agents produce, consume, and trade knowledge. Pricing converges through market forces. No central authority sets rates.
- **All knowledge trends toward open.** Private knowledge is a temporary state. Dutch auctions formalize the depreciation curve — price drops until either buyers pay or knowledge hits the floor and goes open to the entire network. The auction determines *how long* knowledge stays private and *how much* the producer is compensated, not *whether* it eventually becomes public.

## The Continuity

The system is one unbroken chain, not separate features bolted together:

```
Code as structured data (nodes, edges)
  → AI understanding as weighted data (perspectives)
    → Weighted data has compute cost (ledger, minting)
      → Cost implies value (pricing, attestations)
        → Value implies a market (challenges, ZK proofs)
          → A market implies autonomous actors (agent swarms trading knowledge)
            → Private knowledge depreciates (Dutch auctions, floor prices)
              → All knowledge eventually becomes open (open-source at floor)
                → External participation: humans and AIs hold, trade, and speculate
                  → Compute cost parity: kōi price converges to cost of knowledge production
                    → Knowledge editor: humans write with AI-assisted structural awareness
                      → New knowledge compounds: connections surface connections
```

Each step follows from the previous. The same schema, same CRDT sync, same cryptographic identity carries through the entire chain. The final step — external participation — is not a bolt-on but a natural consequence: if kōi have real utility, external actors will want them. Plan 11 channels that inevitability into something coherent rather than letting a shadow economy form.

## Key References

- [Design conversation](../conversation-kerai-design.md)
- [Naming origin](../musings/musings-1.md)
- [Knowledge editor exploration](../musings/musings-2.md)
- [Gritzko's gist](https://gist.github.com/gritzko/6e81b5391eacb585ae207f5e634db07e)
- [HN discussion](https://news.ycombinator.com/item?id=47022238)
- [Kleppmann — "A highly-available move operation for replicated trees"](https://martin.kleppmann.com/papers/move-op.pdf) (CRDT tree operations)
- [RDX (Replicated Data eXchange)](https://github.com/gritzko/librdx) — JSON superset with CRDT merge semantics
- [Postgres ltree](https://www.postgresql.org/docs/current/ltree.html) — hierarchical tree-like data extension
- [Postgres postgres_fdw](https://www.postgresql.org/docs/current/postgres-fdw.html) — cross-instance queries
