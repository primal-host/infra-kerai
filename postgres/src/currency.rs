/// Currency — native Koi cryptocurrency: wallet registration, signed transfers, mining rewards.
///
/// All monetary amounts are denominated in nKoi (nano-Koi).
/// 1 Koi = 1,000,000,000 nKoi (10^9). Stored as BIGINT in Postgres.
/// 9 whole digits + implicit decimal + 9 fractional digits.
use pgrx::prelude::*;

use crate::identity;
use crate::sql::sql_escape;

/// 1 Koi = 1,000,000,000 nKoi
pub const NKOI_PER_KOI: i64 = 1_000_000_000;

/// Format bytes as PostgreSQL hex bytea literal: \xABCD...
fn bytes_to_pg_hex(bytes: &[u8]) -> String {
    let hex: String = bytes.iter().map(|b| format!("{:02x}", b)).collect();
    format!("\\x{}", hex)
}

/// Register a wallet with a client-provided Ed25519 public key (hex-encoded, 64 chars).
/// The server never sees the private key. Type must be one of: human, agent, external.
#[pg_extern]
fn register_wallet(
    public_key_hex: &str,
    wallet_type: &str,
    label: Option<&str>,
) -> pgrx::JsonB {
    let valid_types = ["human", "agent", "external"];
    if !valid_types.contains(&wallet_type) {
        error!(
            "Invalid wallet type '{}'. Must be one of: human, agent, external",
            wallet_type
        );
    }

    // Validate hex string: must be exactly 64 hex chars (32 bytes)
    if public_key_hex.len() != 64 {
        error!(
            "Invalid public key: expected 64 hex characters (32 bytes), got {}",
            public_key_hex.len()
        );
    }

    let pk_bytes = match hex::decode(public_key_hex) {
        Ok(b) => b,
        Err(e) => error!("Invalid hex in public key: {}", e),
    };

    if pk_bytes.len() != 32 {
        error!("Public key must be exactly 32 bytes");
    }

    // Verify it's a valid Ed25519 public key
    let pg_hex = bytes_to_pg_hex(&pk_bytes);
    let pk_array: [u8; 32] = pk_bytes.try_into().unwrap();
    let verifying_key = match ed25519_dalek::VerifyingKey::from_bytes(&pk_array) {
        Ok(k) => k,
        Err(e) => error!("Invalid Ed25519 public key: {}", e),
    };

    let fp = identity::fingerprint(&verifying_key);

    let label_sql = match label {
        Some(l) => format!("'{}'", sql_escape(l)),
        None => "NULL".to_string(),
    };

    let row = Spi::get_one::<pgrx::JsonB>(&format!(
        "INSERT INTO kerai.wallets (public_key, key_fingerprint, wallet_type, label)
         VALUES ('{}'::bytea, '{}', '{}', {})
         RETURNING jsonb_build_object(
             'id', id,
             'wallet_type', wallet_type,
             'key_fingerprint', key_fingerprint,
             'label', label,
             'nonce', nonce,
             'created_at', created_at
         )",
        pg_hex,
        sql_escape(&fp),
        sql_escape(wallet_type),
        label_sql,
    ))
    .unwrap()
    .unwrap();
    row
}

/// Signed transfer: verify Ed25519 signature over canonical message, validate nonce and balance.
/// Message format: "transfer:{from}:{to}:{amount}:{nonce}"
#[pg_extern]
fn signed_transfer(
    from_wallet_id: pgrx::Uuid,
    to_wallet_id: pgrx::Uuid,
    amount: i64,
    nonce: i64,
    signature_hex: &str,
    reason: Option<&str>,
) -> pgrx::JsonB {
    if amount <= 0 {
        error!("Transfer amount must be positive");
    }

    // Get from_wallet public key and current nonce
    let wallet_row = Spi::get_one::<pgrx::JsonB>(&format!(
        "SELECT jsonb_build_object(
            'public_key', encode(public_key, 'hex'),
            'nonce', nonce
        ) FROM kerai.wallets WHERE id = '{}'::uuid",
        from_wallet_id,
    ))
    .unwrap_or(None);

    let wallet_info = match wallet_row {
        Some(w) => w,
        None => error!("Source wallet not found: {}", from_wallet_id),
    };

    let current_nonce = wallet_info.0["nonce"].as_i64().unwrap_or(0);
    let pk_hex = wallet_info.0["public_key"]
        .as_str()
        .unwrap_or_else(|| error!("Wallet has no public key"));

    // Verify nonce = current + 1
    if nonce != current_nonce + 1 {
        error!(
            "Invalid nonce: expected {}, got {}",
            current_nonce + 1,
            nonce
        );
    }

    // Verify destination wallet exists
    let to_exists = Spi::get_one::<bool>(&format!(
        "SELECT EXISTS(SELECT 1 FROM kerai.wallets WHERE id = '{}'::uuid)",
        to_wallet_id,
    ))
    .unwrap()
    .unwrap_or(false);
    if !to_exists {
        error!("Destination wallet not found: {}", to_wallet_id);
    }

    // Construct canonical message
    let message = format!(
        "transfer:{}:{}:{}:{}",
        from_wallet_id, to_wallet_id, amount, nonce
    );

    // Decode and verify signature
    let sig_bytes = match hex::decode(signature_hex) {
        Ok(b) => b,
        Err(e) => error!("Invalid hex in signature: {}", e),
    };

    let pk_bytes = match hex::decode(pk_hex) {
        Ok(b) => b,
        Err(e) => error!("Invalid hex in stored public key: {}", e),
    };

    let pk_array: [u8; 32] = pk_bytes
        .try_into()
        .unwrap_or_else(|_| error!("Stored public key is not 32 bytes"));
    let verifying_key = match ed25519_dalek::VerifyingKey::from_bytes(&pk_array) {
        Ok(k) => k,
        Err(e) => error!("Invalid stored public key: {}", e),
    };

    if !identity::verify_signature(&verifying_key, message.as_bytes(), &sig_bytes) {
        error!("Invalid signature for transfer");
    }

    // Check balance
    let balance = Spi::get_one::<i64>(&format!(
        "SELECT COALESCE(
            (SELECT COALESCE(SUM(amount), 0) FROM kerai.ledger WHERE to_wallet = '{0}'::uuid)
            - (SELECT COALESCE(SUM(amount), 0) FROM kerai.ledger WHERE from_wallet = '{0}'::uuid),
            0
        )::bigint",
        from_wallet_id,
    ))
    .unwrap()
    .unwrap_or(0);

    if balance < amount {
        error!(
            "Insufficient balance: wallet {} has {} nKoi but transfer requires {}",
            from_wallet_id, balance, amount
        );
    }

    // Get lamport timestamp
    let lamport = Spi::get_one::<i64>(
        "SELECT COALESCE(max(timestamp), 0) + 1 FROM kerai.ledger",
    )
    .unwrap()
    .unwrap_or(1);

    let reason_str = reason.unwrap_or("signed_transfer");
    let sig_pg = bytes_to_pg_hex(&sig_bytes);

    // Insert ledger entry
    let row = Spi::get_one::<pgrx::JsonB>(&format!(
        "INSERT INTO kerai.ledger (from_wallet, to_wallet, amount, reason, signature, timestamp)
         VALUES ('{}'::uuid, '{}'::uuid, {}, '{}', '{}'::bytea, {})
         RETURNING jsonb_build_object(
             'id', id,
             'from_wallet', from_wallet,
             'to_wallet', to_wallet,
             'amount', amount,
             'reason', reason,
             'timestamp', timestamp
         )",
        from_wallet_id,
        to_wallet_id,
        amount,
        sql_escape(reason_str),
        sig_pg,
        lamport,
    ))
    .unwrap()
    .unwrap();

    // Increment wallet nonce
    Spi::run(&format!(
        "UPDATE kerai.wallets SET nonce = {} WHERE id = '{}'::uuid",
        nonce, from_wallet_id,
    ))
    .unwrap();

    row
}

/// Total supply: sum all mints (ledger WHERE from_wallet IS NULL).
#[pg_extern]
fn total_supply() -> pgrx::JsonB {
    let total_minted = Spi::get_one::<i64>(
        "SELECT COALESCE(SUM(amount), 0)::bigint FROM kerai.ledger WHERE from_wallet IS NULL",
    )
    .unwrap()
    .unwrap_or(0);

    let total_transactions = Spi::get_one::<i64>(
        "SELECT count(*)::bigint FROM kerai.ledger",
    )
    .unwrap()
    .unwrap_or(0);

    pgrx::JsonB(serde_json::json!({
        "total_supply": total_minted,
        "total_minted": total_minted,
        "total_transactions": total_transactions,
    }))
}

/// Wallet share: balance / total_supply as a decimal string.
#[pg_extern]
fn wallet_share(wallet_id: pgrx::Uuid) -> pgrx::JsonB {
    // Verify wallet exists
    let exists = Spi::get_one::<bool>(&format!(
        "SELECT EXISTS(SELECT 1 FROM kerai.wallets WHERE id = '{}'::uuid)",
        wallet_id,
    ))
    .unwrap()
    .unwrap_or(false);
    if !exists {
        error!("Wallet not found: {}", wallet_id);
    }

    let balance = Spi::get_one::<i64>(&format!(
        "SELECT COALESCE(
            (SELECT COALESCE(SUM(amount), 0) FROM kerai.ledger WHERE to_wallet = '{0}'::uuid)
            - (SELECT COALESCE(SUM(amount), 0) FROM kerai.ledger WHERE from_wallet = '{0}'::uuid),
            0
        )::bigint",
        wallet_id,
    ))
    .unwrap()
    .unwrap_or(0);

    let total = Spi::get_one::<i64>(
        "SELECT COALESCE(SUM(amount), 0)::bigint FROM kerai.ledger WHERE from_wallet IS NULL",
    )
    .unwrap()
    .unwrap_or(0);

    let share = if total > 0 {
        format!("{:.18}", balance as f64 / total as f64)
    } else {
        "0".to_string()
    };

    pgrx::JsonB(serde_json::json!({
        "wallet_id": wallet_id.to_string(),
        "balance": balance,
        "total_supply": total,
        "share": share,
    }))
}

/// Rich supply overview: total_supply, wallet_count, top holders, recent mints.
#[pg_extern]
fn supply_info() -> pgrx::JsonB {
    let total = Spi::get_one::<i64>(
        "SELECT COALESCE(SUM(amount), 0)::bigint FROM kerai.ledger WHERE from_wallet IS NULL",
    )
    .unwrap()
    .unwrap_or(0);

    let wallet_count = Spi::get_one::<i64>(
        "SELECT count(*)::bigint FROM kerai.wallets",
    )
    .unwrap()
    .unwrap_or(0);

    let top_holders = Spi::get_one::<pgrx::JsonB>(
        "SELECT COALESCE(jsonb_agg(row_to_json(t)), '[]'::jsonb) FROM (
            SELECT w.id, w.wallet_type, w.label,
                COALESCE(
                    (SELECT COALESCE(SUM(amount), 0) FROM kerai.ledger WHERE to_wallet = w.id)
                    - (SELECT COALESCE(SUM(amount), 0) FROM kerai.ledger WHERE from_wallet = w.id),
                    0
                ) AS balance
            FROM kerai.wallets w
            ORDER BY balance DESC
            LIMIT 10
        ) t",
    )
    .unwrap()
    .unwrap_or_else(|| pgrx::JsonB(serde_json::json!([])));

    let recent_mints = Spi::get_one::<pgrx::JsonB>(
        "SELECT COALESCE(jsonb_agg(row_to_json(t)), '[]'::jsonb) FROM (
            SELECT id, to_wallet, amount, reason, created_at
            FROM kerai.ledger
            WHERE from_wallet IS NULL
            ORDER BY created_at DESC
            LIMIT 10
        ) t",
    )
    .unwrap()
    .unwrap_or_else(|| pgrx::JsonB(serde_json::json!([])));

    pgrx::JsonB(serde_json::json!({
        "total_supply": total,
        "wallet_count": wallet_count,
        "top_holders": top_holders.0,
        "recent_mints": recent_mints.0,
    }))
}

/// Mint reward for work. Looks up reward_schedule, mints to self instance wallet, logs to reward_log.
/// Returns the mint result or null JSON if work_type is disabled/not found.
#[pg_extern]
fn mint_reward(work_type: &str, details: Option<pgrx::JsonB>) -> pgrx::JsonB {
    // Look up reward schedule
    let schedule = Spi::get_one::<pgrx::JsonB>(&format!(
        "SELECT jsonb_build_object('reward', reward, 'enabled', enabled)
         FROM kerai.reward_schedule WHERE work_type = '{}'",
        sql_escape(work_type),
    ))
    .unwrap_or(None);

    let schedule_info = match schedule {
        Some(s) => s,
        None => return pgrx::JsonB(serde_json::json!(null)),
    };

    let enabled = schedule_info.0["enabled"].as_bool().unwrap_or(false);
    if !enabled {
        return pgrx::JsonB(serde_json::json!(null));
    }

    let reward = schedule_info.0["reward"]
        .as_i64()
        .unwrap_or_else(|| error!("Invalid reward value in schedule"));

    // Get self instance wallet
    let wallet_id = Spi::get_one::<String>(
        "SELECT w.id::text FROM kerai.wallets w
         JOIN kerai.instances i ON w.instance_id = i.id
         WHERE i.is_self = true AND w.wallet_type = 'instance'",
    )
    .unwrap()
    .unwrap_or_else(|| error!("Self instance wallet not found"));

    // Get lamport timestamp
    let lamport = Spi::get_one::<i64>(
        "SELECT COALESCE(max(timestamp), 0) + 1 FROM kerai.ledger",
    )
    .unwrap()
    .unwrap_or(1);

    // Insert ledger entry (mint)
    let ledger_id = Spi::get_one::<String>(&format!(
        "INSERT INTO kerai.ledger (from_wallet, to_wallet, amount, reason, timestamp)
         VALUES (NULL, '{}'::uuid, {}, '{}', {})
         RETURNING id::text",
        sql_escape(&wallet_id),
        reward,
        sql_escape(&format!("reward:{}", work_type)),
        lamport,
    ))
    .unwrap()
    .unwrap();

    // Log to reward_log
    let details_str = match &details {
        Some(d) => sql_escape(&d.0.to_string()),
        None => "{}".to_string(),
    };

    Spi::run(&format!(
        "INSERT INTO kerai.reward_log (work_type, reward, wallet_id, details)
         VALUES ('{}', {}, '{}'::uuid, '{}'::jsonb)",
        sql_escape(work_type),
        reward,
        sql_escape(&wallet_id),
        details_str,
    ))
    .unwrap();

    pgrx::JsonB(serde_json::json!({
        "ledger_id": ledger_id,
        "work_type": work_type,
        "reward": reward,
        "wallet_id": wallet_id,
    }))
}

/// Periodic evaluation: check for unrewarded work and mint bonus rewards.
#[pg_extern]
fn evaluate_mining() -> pgrx::JsonB {
    let wallet_id = Spi::get_one::<String>(
        "SELECT w.id::text FROM kerai.wallets w
         JOIN kerai.instances i ON w.instance_id = i.id
         WHERE i.is_self = true AND w.wallet_type = 'instance'",
    )
    .unwrap()
    .unwrap_or_else(|| error!("Self instance wallet not found"));

    let mut mints = Vec::new();

    // Check total nodes vs rewarded parse count
    let node_count = Spi::get_one::<i64>(
        "SELECT count(*)::bigint FROM kerai.nodes",
    )
    .unwrap()
    .unwrap_or(0);

    let rewarded_parses = Spi::get_one::<i64>(
        "SELECT count(*)::bigint FROM kerai.reward_log WHERE work_type IN ('parse_file', 'parse_crate', 'parse_markdown')",
    )
    .unwrap()
    .unwrap_or(0);

    // If there are many nodes but few rewards, issue a bonus
    if node_count > 0 && rewarded_parses == 0 {
        let bonus = std::cmp::min(node_count, 100) * NKOI_PER_KOI; // 1 Koi per node, cap 100 Koi
        let lamport = Spi::get_one::<i64>(
            "SELECT COALESCE(max(timestamp), 0) + 1 FROM kerai.ledger",
        )
        .unwrap()
        .unwrap_or(1);

        Spi::run(&format!(
            "INSERT INTO kerai.ledger (from_wallet, to_wallet, amount, reason, timestamp)
             VALUES (NULL, '{}'::uuid, {}, 'reward:retroactive_parsing', {})",
            sql_escape(&wallet_id),
            bonus,
            lamport,
        ))
        .unwrap();

        Spi::run(&format!(
            "INSERT INTO kerai.reward_log (work_type, reward, wallet_id, details)
             VALUES ('retroactive_parsing', {}, '{}'::uuid, '{}'::jsonb)",
            bonus,
            sql_escape(&wallet_id),
            sql_escape(&format!("{{\"node_count\": {}}}", node_count)),
        ))
        .unwrap();

        mints.push(serde_json::json!({
            "work_type": "retroactive_parsing",
            "reward": bonus,
            "node_count": node_count,
        }));
    }

    // Check version count
    let version_count = Spi::get_one::<i64>(
        "SELECT count(*)::bigint FROM kerai.versions",
    )
    .unwrap()
    .unwrap_or(0);

    let rewarded_versions = Spi::get_one::<i64>(
        "SELECT count(*)::bigint FROM kerai.reward_log WHERE work_type = 'create_version'",
    )
    .unwrap()
    .unwrap_or(0);

    if version_count > rewarded_versions {
        let unrewarded = version_count - rewarded_versions;
        let reward_per = Spi::get_one::<i64>(
            "SELECT reward FROM kerai.reward_schedule WHERE work_type = 'create_version' AND enabled = true",
        )
        .unwrap_or(None);

        if let Some(rate) = reward_per {
            let bonus = unrewarded * rate;
            let lamport = Spi::get_one::<i64>(
                "SELECT COALESCE(max(timestamp), 0) + 1 FROM kerai.ledger",
            )
            .unwrap()
            .unwrap_or(1);

            Spi::run(&format!(
                "INSERT INTO kerai.ledger (from_wallet, to_wallet, amount, reason, timestamp)
                 VALUES (NULL, '{}'::uuid, {}, 'reward:retroactive_versions', {})",
                sql_escape(&wallet_id),
                bonus,
                lamport,
            ))
            .unwrap();

            Spi::run(&format!(
                "INSERT INTO kerai.reward_log (work_type, reward, wallet_id, details)
                 VALUES ('retroactive_versions', {}, '{}'::uuid, '{}'::jsonb)",
                bonus,
                sql_escape(&wallet_id),
                sql_escape(&format!("{{\"version_count\": {}, \"unrewarded\": {}}}", version_count, unrewarded)),
            ))
            .unwrap();

            mints.push(serde_json::json!({
                "work_type": "retroactive_versions",
                "reward": bonus,
                "unrewarded": unrewarded,
            }));
        }
    }

    pgrx::JsonB(serde_json::json!({
        "evaluated": true,
        "mints": mints,
    }))
}

/// List all reward schedule entries.
#[pg_extern]
fn get_reward_schedule() -> pgrx::JsonB {
    let json = Spi::get_one::<pgrx::JsonB>(
        "SELECT COALESCE(
            jsonb_agg(jsonb_build_object(
                'id', id,
                'work_type', work_type,
                'reward', reward,
                'enabled', enabled,
                'updated_at', updated_at
            ) ORDER BY work_type),
            '[]'::jsonb
        ) FROM kerai.reward_schedule",
    )
    .unwrap()
    .unwrap_or_else(|| pgrx::JsonB(serde_json::json!([])));
    json
}

/// Create or update a reward schedule entry.
#[pg_extern]
fn set_reward(work_type: &str, reward: i64, enabled: Option<bool>) -> pgrx::JsonB {
    if reward <= 0 {
        error!("Reward must be positive");
    }

    let enabled_val = enabled.unwrap_or(true);

    let row = Spi::get_one::<pgrx::JsonB>(&format!(
        "INSERT INTO kerai.reward_schedule (work_type, reward, enabled)
         VALUES ('{}', {}, {})
         ON CONFLICT (work_type) DO UPDATE SET reward = EXCLUDED.reward, enabled = EXCLUDED.enabled, updated_at = now()
         RETURNING jsonb_build_object(
             'id', id,
             'work_type', work_type,
             'reward', reward,
             'enabled', enabled,
             'updated_at', updated_at
         )",
        sql_escape(work_type),
        reward,
        enabled_val,
    ))
    .unwrap()
    .unwrap();
    row
}
