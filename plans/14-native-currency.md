# Plan 14: Native Currency

*Depends on: Plan 01 (Foundation)*
*Enables: Plan 20 (ZK Currency)*

## Overview

The plaintext Koi currency layer — continuous mining (every verifiable work action auto-mints Koi), Ed25519 signed transfers with client-side key custody, nonce-based replay protection, and supply that grows proportionally with work. All amounts are denominated in nKoi (1 Koi = 1,000,000,000 nKoi).

This is the starting point for the Koi economy. It works as a complete currency system on its own. Plan 20 later adds a private ledger alongside it, upgrading transfers from plaintext to zK proofs and replay protection from nonces to nullifiers. Both modes coexist — users choose when to shield.

## What Was Implemented

### Schema (Step 1)
- **`kerai.reward_schedule`** — configurable emission rates per work type, amounts in nKoi:
  - parse_file = 10,000,000,000 (10 Koi)
  - parse_crate = 50,000,000,000 (50 Koi)
  - parse_markdown = 10,000,000,000 (10 Koi)
  - create_version = 5,000,000,000 (5 Koi)
  - bounty_settlement = 20,000,000,000 (20 Koi)
  - peer_sync = 15,000,000,000 (15 Koi)
  - model_training = 25,000,000,000 (25 Koi)
  - mirror_repo = 100,000,000,000 (100 Koi)
- **`kerai.reward_log`** — audit trail for auto-mints with work_type, reward (nKoi), wallet_id, details
- **`kerai.wallets.nonce`** — BIGINT column for replay protection on signed transfers

### Currency Module (Step 2) — `src/currency.rs`
9 `pg_extern` functions:
- **`register_wallet(public_key_hex, wallet_type, label?)`** — Accept Ed25519 public key (hex, 64 chars), compute fingerprint, INSERT wallet. No private key touches the server.
- **`signed_transfer(from, to, amount, nonce, signature_hex, reason?)`** — Verify Ed25519 signature over `"transfer:{from}:{to}:{amount}:{nonce}"`, check nonce = wallet.nonce + 1, validate balance, INSERT ledger, increment nonce. *Plan 20 adds a private alternative: Fuchi generates a zK proof instead, with nullifiers replacing nonces for double-spend prevention. signed_transfer remains for plaintext-mode transfers.*
- **`total_supply()`** — Sum all mints. Returns `{total_supply, total_minted, total_transactions}`. *Unaffected by Plan 20 — mint amounts are always public (tied to verifiable work metrics).*
- **`wallet_share(wallet_id)`** — Returns `{wallet_id, balance, total_supply, share}` where share is a decimal string. *Under Plan 20, this only operates on unshielded balances. Shielded balances are invisible to the server — only Fuchi knows them.*
- **`supply_info()`** — Rich overview: total_supply, wallet_count, top holders, recent mints. *Under Plan 20, "top holders" reveals only plaintext-mode balances. Plan 20 provides aggregate-only alternatives that don't disclose individual holdings.*
- **`mint_reward(work_type, details?)`** — Looks up reward_schedule, mints to self instance wallet, logs to reward_log. *Under Plan 20, minting creates a commitment instead of a plaintext ledger entry. The hook and reward_log stay the same; the ledger target changes.*
- **`evaluate_mining()`** — Periodic evaluation for unrewarded work (retroactive parsing, versions). Bonus amounts use `NKOI_PER_KOI` constant (1 Koi per node, capped at 100 Koi).
- **`get_reward_schedule()`** — List all reward schedule entries.
- **`set_reward(work_type, reward, enabled?)`** — Create or update a reward schedule entry.

### Auto-Mint Hooks (Step 3)
- `parse_crate()` → `mint_reward('parse_crate', ...)`
- `parse_file()` → `mint_reward('parse_file', ...)`
- `parse_source()` → `mint_reward('parse_file', ...)`
- `parse_markdown()` → `mint_reward('parse_markdown', ...)`

*Under Plan 20, these hooks stay the same — the mint target changes from plaintext ledger to commitment, but the trigger mechanism is identical.*

### CRDT Op Types (Step 4)
3 new op types in `src/crdt/operations.rs`:
- `register_wallet` — replicate wallet registration
- `signed_transfer` — replicate signed transfers with signature
- `mint_reward` — replicate mint + reward_log entry

*Under Plan 20, CRDT replication for transfers changes fundamentally. Instead of replicating a plaintext amount and signature, the private ledger replicates a proof, commitment, and nullifier. `mint_reward` replication similarly changes — the commitment and mint proof travel instead of a plaintext amount. `register_wallet` replication is unchanged.*

### Status Enhancement (Step 5)
`status()` now includes `total_supply` and `instance_balance` fields.

*Under Plan 20, `instance_balance` from the plaintext ledger shows only unshielded balance. The shielded balance is known only to the instance's Fuchi client.*

### CLI Commands (Steps 8-9)
`kerai currency <subcommand>`:
- `register` — --pubkey, --type, --label
- `transfer` — --from, --to, --amount, --nonce, --signature, --reason
- `supply` — show total supply info
- `share` — wallet_id positional
- `schedule` — list reward schedule
- `set-reward` — --work-type, --reward, --enabled

*Under Plan 20, Fuchi provides private equivalents: `fuchi transfer` (generates zK proof), `fuchi wallet balance` (scans commitments locally). The `kerai currency` commands remain for plaintext-mode operations and supply auditing.*

## Tests Added (17 new, 140 total)
- `test_register_wallet_currency` — register with valid hex pubkey
- `test_register_wallet_invalid_key` — #[should_panic] on bad hex
- `test_register_wallet_duplicate_key` — #[should_panic] on same pubkey
- `test_signed_transfer` — register, mint, sign, transfer, verify balances
- `test_signed_transfer_bad_signature` — #[should_panic]
- `test_signed_transfer_bad_nonce` — #[should_panic] on replay
- `test_signed_transfer_insufficient_balance` — #[should_panic]
- `test_total_supply` — mint, verify total
- `test_wallet_share` — mint, verify share calculation
- `test_supply_info` — verify rich supply overview
- `test_mint_reward` — call mint_reward, verify ledger + reward_log
- `test_mint_reward_disabled` — disable work_type, verify null return
- `test_evaluate_mining` — verify periodic evaluation
- `test_get_reward_schedule` — verify 8 seed entries
- `test_set_reward` — create/update reward entry
- `test_auto_mint_on_parse` — parse_source triggers supply increase
- `test_status_includes_supply` — status JSON has total_supply + instance_balance

## Key Design Decisions
1. **Client-side key custody**: `register_wallet` accepts a public key hex string. The server never sees or stores private keys. *This aligns directly with Plan 20's Fuchi model — the private key stays client-side, extended with viewing keys and commitment inventory.*
2. **Signed transfers**: Message format `"transfer:{from}:{to}:{amount}:{nonce}"` — deterministic, nonce provides replay protection. *Plan 20's private transfers use nullifiers instead of nonces — a fundamentally different replay protection mechanism suited to the commitment model.*
3. **Proportional supply**: Total supply grows continuously with work. No inflation schedule or halving. *This property is preserved under Plan 20 — mint proofs still tie supply growth to verifiable work. The difference is that individual mint amounts become commitments while aggregate supply remains publicly auditable.*
4. **Configurable reward schedule**: Instance owners tune emission rates per work type. Defaults seeded at extension creation. All amounts in nKoi.
5. **Curve25519 alignment**: Ed25519 signed transfers operate on Curve25519 — the same curve used by Plan 20's Bulletproofs for range proofs and balance conservation. The cryptographic foundation is shared.

## Relationship to Plan 20

Plan 14 and Plan 20 are the plaintext and private layers of the same currency:

| Aspect | Plan 14 (Plaintext) | Plan 20 (Private) |
|--------|--------------------|--------------------|
| Amounts | Visible in ledger | Hidden in commitments |
| Transfers | Ed25519 signature | zK proof |
| Replay protection | Nonce (sequential) | Nullifier (one-time) |
| Balance query | `SUM(amount)` from ledger | Fuchi scans with viewing key |
| Minting | Plaintext ledger entry | Commitment with mint proof |
| CRDT replication | Amount + signature | Proof + commitment + nullifier |
| Supply audit | `SUM(amount) WHERE from_wallet IS NULL` | Sum of mint proof amounts (public) |

Users shield Koi by transferring from the plaintext ledger to a commitment (Plan 20.8). They unshield by revealing a commitment back. Both layers share the same denomination (nKoi), the same reward schedule, and the same curve (Curve25519).

## Files Changed
| File | Action | Description |
|---|---|---|
| `src/schema.rs` | Modified | reward_schedule, reward_log tables + seed data (nKoi); wallets nonce column |
| `src/currency.rs` | Created | 9 pg_extern functions, `NKOI_PER_KOI` constant |
| `src/parser/mod.rs` | Modified | Auto-mint hooks in parse_crate/parse_file/parse_source |
| `src/parser/markdown/mod.rs` | Modified | Auto-mint hook in parse_markdown |
| `src/crdt/operations.rs` | Modified | 3 new op types + apply handlers |
| `src/functions/status.rs` | Modified | total_supply + instance_balance in status JSON |
| `src/lib.rs` | Modified | mod currency + 17 tests |
| `cli/src/commands/currency.rs` | Created | CLI currency subcommands |
| `cli/src/commands/mod.rs` | Modified | Currency module + Command variants |
| `cli/src/main.rs` | Modified | CurrencyAction enum + dispatch |
