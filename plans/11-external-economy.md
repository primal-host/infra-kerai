# Plan 11: External Economy

*Depends on: Plan 10 (ZK Marketplace), Plan 05 (CLI)*
*Enables: human and AI participation in the knowledge economy beyond kerai instances*

## Goal

Bridge the internal kōi economy to the external world — crypto wallets, exchanges, speculation, and fiat on/off ramps. At the end of this plan, humans can hold kōi, trade them on exchanges, speculate on knowledge domains, and AI systems can autonomously manage portfolios of kōi across the kerai network and external markets.

## Why This Is Inevitable

The kōi is grounded in verifiable compute work. It has real utility — you spend it to acquire knowledge that would cost more to reproduce independently. Any token with real utility and verifiable scarcity attracts outside interest:

- **Humans** who commission knowledge work want to pay for it directly
- **Investors** who believe certain knowledge domains will increase in value want exposure
- **AI systems** operating outside the kerai network want to purchase knowledge from inside it
- **Instances** that produce valuable knowledge want to convert kōi to fiat to cover infrastructure costs (electricity, hosting, API fees)

Ignoring this creates a shadow economy. Planning for it channels the inevitable into something coherent.

## Deliverables

### 11.1 Wallet Management CLI

```bash
# Create a standalone wallet (for a human or external entity)
kerai wallet create --label "billy-personal" --type human
# Output: wallet ID, public key, fingerprint
# Private key saved to ~/.kerai/wallets/<fingerprint>/private.pem

# List wallets
kerai wallet list

# Check balance
kerai wallet balance [wallet-id]

# Transfer kōi between wallets
kerai wallet transfer --from <wallet-id> --to <wallet-id> --amount 5000

# View transaction history
kerai wallet history [wallet-id] [--since date] [--reason type]

# Export wallet (for backup or import on another machine)
kerai wallet export <wallet-id> --output wallet-backup.enc
# Encrypted with a passphrase

# Import wallet
kerai wallet import wallet-backup.enc
```

### 11.2 External Wallet Types

The `wallets` table (Plan 01) supports four types:

| Type | Owner | Created By | Typical Use |
|------|-------|------------|-------------|
| `instance` | A kerai instance | `kerai init` (auto) | Internal knowledge economy |
| `human` | A person | `kerai wallet create` | Holding kōi, commissioning work |
| `agent` | An AI agent | Agent registration | Autonomous trading, portfolio management |
| `external` | Bridge contract | Bridge setup | Wrapped tokens on external chains |

All wallet types use the same Ed25519 keypair for identity and transaction signing. A human wallet has no instance — it's a pure financial identity in the kerai economy.

### 11.3 Token Bridge to External Chains

Wrap kōi as **KOI** tokens on established networks for exchange listing and external trading. The ticker drops the macron — your kōi become KOI when they swim to external waters.

**Bridge mechanism:**

```
LOCK (kerai side) → MINT (external side)
BURN (external side) → UNLOCK (kerai side)

1. User locks 10,000 kōi in a bridge wallet on the kerai ledger
   (signed transaction, from_wallet = user, to_wallet = bridge)
2. Bridge verifies the lock transaction (signed, in the ledger)
3. Bridge mints 10,000 KOI on the external chain (ERC-20)
4. User receives KOI in their external wallet

Reverse:
1. User burns 10,000 KOI on the external chain
2. Bridge verifies the burn
3. Bridge unlocks 10,000 kōi on the kerai ledger
   (signed transaction, from_wallet = bridge, to_wallet = user)
```

**External chain candidates:**

- **Ethereum L2 (Arbitrum, Base, etc.):** Low fees, large existing DeFi ecosystem, ERC-20 standard
- **Solana:** High throughput, low fees, good for high-frequency trading
- **Native:** Run kerai's own lightweight chain for maximum control (high effort, probably not needed initially)

Proposed: start with an Ethereum L2 ERC-20 KOI wrapper. The DeFi ecosystem gives liquidity and exchange listings for free.

### 11.4 Speculation and Price Discovery

Once KOI trades on external exchanges, the market price reflects the collective belief about future knowledge production value. This creates interesting dynamics:

**Domain-specific futures.** An exchange could list derivatives on specific knowledge scopes:

- "Kōi earned from `pkg.crypto.*` knowledge will increase next quarter" → bet on cryptography research being valuable
- "Agent swarm efficiency will reduce reproduction costs by 50%" → bet on the deflationary pressure on knowledge prices

Kerai doesn't need to build the derivatives market — external DeFi protocols handle this once the token is liquid. But the system should expose enough data for external actors to make informed bets:

```bash
# Publish anonymized market statistics for external consumption
kerai market export-stats --format json
# Output: auction volumes, settlement prices, knowledge domain activity,
#         mint rate, active instances, open-sourced knowledge volume
```

### 11.5 Commissioning Work

Humans (or external AIs) with wallets can commission knowledge production:

```bash
# Post a bounty: "I'll pay 50,000 kōi for perspectives on pkg.auth
# that make TestValidateToken pass"
kerai bounty create \
  --scope "pkg.auth.*" \
  --success-criterion "cargo test --package auth" \
  --reward 50000 \
  --wallet <wallet-id>

# Instances and agents see the bounty, compete to fill it
kerai bounty list --open

# When an instance fills the bounty, payment is automatic
# (verified by running the success criterion against the submitted knowledge)
```

```sql
CREATE TABLE bounties (
    id              uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    poster_wallet   uuid NOT NULL REFERENCES wallets(id),
    scope           ltree NOT NULL,
    description     text NOT NULL,
    success_command text,                     -- automated verification
    reward          bigint NOT NULL,
    status          text NOT NULL DEFAULT 'open', -- "open", "claimed", "verified", "paid", "expired"
    claimed_by      uuid REFERENCES wallets(id),
    verified_at     timestamptz,
    signature       bytea NOT NULL,
    created_at      timestamptz DEFAULT now(),
    expires_at      timestamptz
);

CREATE INDEX idx_bounties_scope ON bounties USING gist(scope);
CREATE INDEX idx_bounties_status ON bounties(status);
CREATE INDEX idx_bounties_poster ON bounties(poster_wallet);
CREATE INDEX idx_bounties_reward ON bounties(reward);
```

### 11.6 Revenue Model for Instances

Instances that produce valuable knowledge can convert kōi to fiat:

```
Instance produces knowledge
  → Knowledge sold via Dutch auction (Plan 10)
  → Kōi earned
  → Kōi transferred to human wallet
  → Kōi bridged to external chain
  → Tokens sold on exchange for fiat
  → Fiat pays for hosting, compute, API costs
```

This closes the loop. The economic incentive to run a kerai instance and produce knowledge is: it can pay for itself. An instance that consistently produces high-value perspectives earns kōi that convert to real money.

AI systems have the same path — an autonomous agent that earns kōi can use them to purchase more compute (API calls, GPU time), creating a self-sustaining cycle where knowledge production funds further knowledge production.

### 11.7 Fiat On-Ramp

For humans who want to buy kōi without running an instance:

```bash
# Purchase kōi with fiat (via integrated payment processor or exchange)
kerai wallet buy --amount 10000 --payment-method stripe

# Or: buy KOI on an exchange, then bridge them into kerai
# (no kerai-specific tooling needed, just standard token bridge UI)
```

The fiat on-ramp lets humans commission bounties, bid in auctions, and participate in the economy without producing knowledge themselves. They bring capital; instances bring knowledge. The exchange rate between fiat and kōi is set by the external market.

## Tokenomics

### Supply

- **No pre-mine.** Kōi are only minted by verifiable work (perspectives computed, tests run, queries answered). There's no initial token allocation, no founder's share, no VC allocation.
- **Inflationary.** New kōi are minted as work is done. The inflation rate is bounded by the amount of actual compute happening in the network. More work = more kōi, but also more knowledge produced and more utility backing each kōi.
- **Deflationary pressure from open-sourcing.** As knowledge goes open (Dutch auction floor), the exclusive value it backed returns to the Koi Pond. Kōi spent acquiring that knowledge don't disappear, but the knowledge that gave them utility is now free. This creates natural deflationary pressure — older kōi backed less exclusive knowledge.

### Demand

- **Utility demand:** Instances need kōi to query other instances, acquire knowledge, and participate in auctions.
- **Speculative demand:** External actors believe the knowledge economy will grow, increasing future demand for kōi.
- **Commissioning demand:** Humans and external AIs post bounties that require kōi.

### Equilibrium

The kōi price in fiat should converge toward the average cost of compute per unit of knowledge produced. If kōi trade above this, it's cheaper to produce knowledge than buy kōi — new instances enter the market, increasing supply. If kōi trade below, it's cheaper to buy kōi than to compute — instances reduce production, decreasing supply. The market self-regulates around compute cost parity.

## Decisions to Make

- **Token standard:** KOI as ERC-20 on which L2? Proposed: start with Base (Coinbase L2) for simplicity and accessibility, add others later.
- **Bridge custody:** Who holds the locked kōi on the kerai side? Proposed: a multi-sig bridge wallet requiring N-of-M instance signatures to unlock. Decentralized custody from the start.
- **Regulatory considerations:** Utility tokens have different regulatory treatment than securities in most jurisdictions. The kōi is clearly a utility token (it's spent on a specific service — knowledge queries). Legal review recommended before exchange listing.
- **Minimum mint for external trading:** Should there be a minimum network size (number of instances, volume of transactions) before enabling the external bridge? Proposed: yes. The internal economy should prove itself before external speculation enters. Launch the bridge when the network has 100+ active instances and a meaningful transaction volume.

## Out of Scope

- Building a DEX (use existing ones — Uniswap, Raydium, etc.)
- Derivatives and futures contracts (external DeFi protocols handle this)
- Fiat banking integration beyond basic on/off ramps
- Regulatory compliance (requires legal counsel, jurisdiction-specific)
- Governance tokens / DAO structure (the economy is algorithmic, not governed by vote — but this could evolve)
