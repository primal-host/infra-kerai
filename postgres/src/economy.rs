/// Economy — wallet management, Koi transfers, and minting.
///
/// All monetary amounts are denominated in nKoi (nano-Koi).
/// 1 Koi = 1,000,000,000 nKoi (10^9). See currency::NKOI_PER_KOI.
use pgrx::prelude::*;

use crate::identity;
use crate::sql::sql_escape;

/// Format bytes as PostgreSQL hex bytea literal: \xABCD...
fn bytes_to_pg_hex(bytes: &[u8]) -> String {
    let hex: String = bytes.iter().map(|b| format!("{:02x}", b)).collect();
    format!("\\x{}", hex)
}

/// Create a new wallet with a fresh Ed25519 keypair.
/// Type must be one of: human, agent, external.
#[pg_extern]
fn create_wallet(wallet_type: &str, label: Option<&str>) -> pgrx::JsonB {
    let valid_types = ["human", "agent", "external"];
    if !valid_types.contains(&wallet_type) {
        error!(
            "Invalid wallet type '{}'. Must be one of: human, agent, external (instance wallets are created at bootstrap)",
            wallet_type
        );
    }

    // Generate a new Ed25519 keypair for this wallet
    let mut rng = rand::rngs::OsRng;
    let signing_key = ed25519_dalek::SigningKey::generate(&mut rng);
    let verifying_key = signing_key.verifying_key();
    let pk_hex = bytes_to_pg_hex(verifying_key.as_bytes());
    let fp = identity::fingerprint(&verifying_key);

    // Get self instance_id for linking
    let instance_id = Spi::get_one::<String>(
        "SELECT id::text FROM kerai.instances WHERE is_self = true",
    )
    .unwrap()
    .unwrap_or_else(|| error!("Self instance not found"));

    let label_sql = match label {
        Some(l) => format!("'{}'", sql_escape(l)),
        None => "NULL".to_string(),
    };

    let row = Spi::get_one::<pgrx::JsonB>(&format!(
        "INSERT INTO kerai.wallets (instance_id, public_key, key_fingerprint, wallet_type, label)
         VALUES ('{}'::uuid, '{}'::bytea, '{}', '{}', {})
         RETURNING jsonb_build_object(
             'id', id,
             'wallet_type', wallet_type,
             'key_fingerprint', key_fingerprint,
             'label', label,
             'created_at', created_at
         )",
        sql_escape(&instance_id),
        pk_hex,
        sql_escape(&fp),
        sql_escape(wallet_type),
        label_sql,
    ))
    .unwrap()
    .unwrap();
    row
}

/// List all wallets, optionally filtered by type.
#[pg_extern]
fn list_wallets(type_filter: Option<&str>) -> pgrx::JsonB {
    let where_clause = match type_filter {
        Some(t) => format!("WHERE w.wallet_type = '{}'", sql_escape(t)),
        None => String::new(),
    };

    let json = Spi::get_one::<pgrx::JsonB>(&format!(
        "SELECT COALESCE(
            jsonb_agg(jsonb_build_object(
                'id', w.id,
                'wallet_type', w.wallet_type,
                'key_fingerprint', w.key_fingerprint,
                'label', w.label,
                'instance_id', w.instance_id,
                'created_at', w.created_at
            ) ORDER BY w.created_at),
            '[]'::jsonb
        ) FROM kerai.wallets w {}",
        where_clause,
    ))
    .unwrap()
    .unwrap_or_else(|| pgrx::JsonB(serde_json::json!([])));
    json
}

/// Get wallet details including balance.
#[pg_extern]
fn get_wallet(wallet_id: pgrx::Uuid) -> pgrx::JsonB {
    let wallet = Spi::get_one::<pgrx::JsonB>(&format!(
        "SELECT jsonb_build_object(
            'id', w.id,
            'wallet_type', w.wallet_type,
            'key_fingerprint', w.key_fingerprint,
            'label', w.label,
            'instance_id', w.instance_id,
            'created_at', w.created_at,
            'balance', COALESCE(
                (SELECT COALESCE(SUM(amount), 0) FROM kerai.ledger WHERE to_wallet = w.id)
                - (SELECT COALESCE(SUM(amount), 0) FROM kerai.ledger WHERE from_wallet = w.id),
                0
            )
        ) FROM kerai.wallets w WHERE w.id = '{}'::uuid",
        wallet_id,
    ))
    .unwrap_or(None);

    match wallet {
        Some(w) => w,
        None => error!("Wallet not found: {}", wallet_id),
    }
}

/// Compute balance from ledger for any wallet by ID.
#[pg_extern]
fn get_wallet_balance(wallet_id: pgrx::Uuid) -> pgrx::JsonB {
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

    let received = Spi::get_one::<i64>(&format!(
        "SELECT COALESCE(SUM(amount), 0)::bigint FROM kerai.ledger WHERE to_wallet = '{}'::uuid",
        wallet_id,
    ))
    .unwrap()
    .unwrap_or(0);

    let sent = Spi::get_one::<i64>(&format!(
        "SELECT COALESCE(SUM(amount), 0)::bigint FROM kerai.ledger WHERE from_wallet = '{}'::uuid",
        wallet_id,
    ))
    .unwrap()
    .unwrap_or(0);

    pgrx::JsonB(serde_json::json!({
        "wallet_id": wallet_id.to_string(),
        "balance": received - sent,
        "total_received": received,
        "total_sent": sent,
    }))
}

/// Transfer Koi between wallets. Validates sufficient balance.
#[pg_extern]
fn transfer_koi(
    from_wallet_id: pgrx::Uuid,
    to_wallet_id: pgrx::Uuid,
    amount: i64,
    reason: Option<&str>,
) -> pgrx::JsonB {
    if amount <= 0 {
        error!("Transfer amount must be positive");
    }

    // Verify both wallets exist
    let from_exists = Spi::get_one::<bool>(&format!(
        "SELECT EXISTS(SELECT 1 FROM kerai.wallets WHERE id = '{}'::uuid)",
        from_wallet_id,
    ))
    .unwrap()
    .unwrap_or(false);
    if !from_exists {
        error!("Source wallet not found: {}", from_wallet_id);
    }

    let to_exists = Spi::get_one::<bool>(&format!(
        "SELECT EXISTS(SELECT 1 FROM kerai.wallets WHERE id = '{}'::uuid)",
        to_wallet_id,
    ))
    .unwrap()
    .unwrap_or(false);
    if !to_exists {
        error!("Destination wallet not found: {}", to_wallet_id);
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

    let reason_str = reason.unwrap_or("transfer");

    let row = Spi::get_one::<pgrx::JsonB>(&format!(
        "INSERT INTO kerai.ledger (from_wallet, to_wallet, amount, reason, timestamp)
         VALUES ('{}'::uuid, '{}'::uuid, {}, '{}', {})
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
        lamport,
    ))
    .unwrap()
    .unwrap();
    row
}

/// Mint Koi from verifiable work. from_wallet is NULL (creation).
/// Only the self instance can mint.
#[pg_extern]
fn mint_koi(
    to_wallet_id: pgrx::Uuid,
    amount: i64,
    reason: &str,
    reference_id: Option<pgrx::Uuid>,
    reference_type: Option<&str>,
) -> pgrx::JsonB {
    if amount <= 0 {
        error!("Mint amount must be positive");
    }

    // Verify target wallet exists
    let exists = Spi::get_one::<bool>(&format!(
        "SELECT EXISTS(SELECT 1 FROM kerai.wallets WHERE id = '{}'::uuid)",
        to_wallet_id,
    ))
    .unwrap()
    .unwrap_or(false);
    if !exists {
        error!("Target wallet not found: {}", to_wallet_id);
    }

    let lamport = Spi::get_one::<i64>(
        "SELECT COALESCE(max(timestamp), 0) + 1 FROM kerai.ledger",
    )
    .unwrap()
    .unwrap_or(1);

    let ref_id_sql = match reference_id {
        Some(r) => format!("'{}'::uuid", r),
        None => "NULL".to_string(),
    };
    let ref_type_sql = match reference_type {
        Some(r) => format!("'{}'", sql_escape(r)),
        None => "NULL".to_string(),
    };

    let row = Spi::get_one::<pgrx::JsonB>(&format!(
        "INSERT INTO kerai.ledger (from_wallet, to_wallet, amount, reason, reference_id, reference_type, timestamp)
         VALUES (NULL, '{}'::uuid, {}, '{}', {}, {}, {})
         RETURNING jsonb_build_object(
             'id', id,
             'to_wallet', to_wallet,
             'amount', amount,
             'reason', reason,
             'reference_id', reference_id,
             'reference_type', reference_type,
             'timestamp', timestamp
         )",
        to_wallet_id,
        amount,
        sql_escape(reason),
        ref_id_sql,
        ref_type_sql,
        lamport,
    ))
    .unwrap()
    .unwrap();
    row
}

/// Return recent ledger entries for a wallet (sent + received).
#[pg_extern]
fn wallet_history(wallet_id: pgrx::Uuid, limit: default!(i32, 50)) -> pgrx::JsonB {
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

    let json = Spi::get_one::<pgrx::JsonB>(&format!(
        "SELECT COALESCE(
            jsonb_agg(jsonb_build_object(
                'id', l.id,
                'from_wallet', l.from_wallet,
                'to_wallet', l.to_wallet,
                'amount', l.amount,
                'reason', l.reason,
                'reference_id', l.reference_id,
                'reference_type', l.reference_type,
                'timestamp', l.timestamp,
                'direction', CASE
                    WHEN l.to_wallet = '{0}'::uuid THEN 'received'
                    ELSE 'sent'
                END,
                'created_at', l.created_at
            ) ORDER BY l.timestamp DESC),
            '[]'::jsonb
        ) FROM (
            SELECT * FROM kerai.ledger
            WHERE to_wallet = '{0}'::uuid OR from_wallet = '{0}'::uuid
            ORDER BY timestamp DESC
            LIMIT {1}
        ) l",
        wallet_id,
        limit,
    ))
    .unwrap()
    .unwrap_or_else(|| pgrx::JsonB(serde_json::json!([])));
    json
}
