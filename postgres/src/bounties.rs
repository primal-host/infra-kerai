/// Bounties — task bounty lifecycle management.
use pgrx::prelude::*;

use crate::sql::sql_escape;

/// Create a bounty. Uses the self instance wallet as poster.
/// Validates reward > 0 and poster has sufficient balance.
#[pg_extern]
fn create_bounty(
    scope: &str,
    description: &str,
    reward: i64,
    success_command: Option<&str>,
    expires_at: Option<&str>,
) -> pgrx::JsonB {
    if reward <= 0 {
        error!("Bounty reward must be positive");
    }

    // Get self wallet
    let self_wallet = Spi::get_one::<String>(
        "SELECT w.id::text FROM kerai.wallets w
         JOIN kerai.instances i ON w.instance_id = i.id
         WHERE i.is_self = true AND w.wallet_type = 'instance'",
    )
    .unwrap()
    .unwrap_or_else(|| error!("Self wallet not found"));

    // Check balance
    let balance = Spi::get_one::<i64>(&format!(
        "SELECT COALESCE(
            (SELECT COALESCE(SUM(amount), 0) FROM kerai.ledger WHERE to_wallet = '{0}'::uuid)
            - (SELECT COALESCE(SUM(amount), 0) FROM kerai.ledger WHERE from_wallet = '{0}'::uuid),
            0
        )::bigint",
        sql_escape(&self_wallet),
    ))
    .unwrap()
    .unwrap_or(0);

    if balance < reward {
        error!(
            "Insufficient balance to fund bounty: have {} Koi, need {}",
            balance, reward
        );
    }

    let cmd_sql = match success_command {
        Some(c) => format!("'{}'", sql_escape(c)),
        None => "NULL".to_string(),
    };
    let expires_sql = match expires_at {
        Some(e) => format!("'{}'::timestamptz", sql_escape(e)),
        None => "NULL".to_string(),
    };

    let row = Spi::get_one::<pgrx::JsonB>(&format!(
        "INSERT INTO kerai.bounties (poster_wallet, scope, description, success_command, reward, expires_at)
         VALUES ('{}'::uuid, '{}'::ltree, '{}', {}, {}, {})
         RETURNING jsonb_build_object(
             'id', id,
             'poster_wallet', poster_wallet,
             'scope', scope::text,
             'description', description,
             'success_command', success_command,
             'reward', reward,
             'status', status,
             'created_at', created_at,
             'expires_at', expires_at
         )",
        sql_escape(&self_wallet),
        sql_escape(scope),
        sql_escape(description),
        cmd_sql,
        reward,
        expires_sql,
    ))
    .unwrap()
    .unwrap();
    row
}

/// List bounties with optional status and scope filters.
#[pg_extern]
fn list_bounties(status_filter: Option<&str>, scope_filter: Option<&str>) -> pgrx::JsonB {
    let mut conditions = Vec::new();

    if let Some(s) = status_filter {
        conditions.push(format!("b.status = '{}'", sql_escape(s)));
    }
    if let Some(scope) = scope_filter {
        conditions.push(format!("b.scope <@ '{}'::ltree", sql_escape(scope)));
    }

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };

    let json = Spi::get_one::<pgrx::JsonB>(&format!(
        "SELECT COALESCE(
            jsonb_agg(jsonb_build_object(
                'id', b.id,
                'poster_wallet', b.poster_wallet,
                'scope', b.scope::text,
                'description', b.description,
                'reward', b.reward,
                'status', b.status,
                'claimed_by', b.claimed_by,
                'created_at', b.created_at,
                'expires_at', b.expires_at
            ) ORDER BY b.reward DESC),
            '[]'::jsonb
        ) FROM kerai.bounties b {}",
        where_clause,
    ))
    .unwrap()
    .unwrap_or_else(|| pgrx::JsonB(serde_json::json!([])));
    json
}

/// Get full bounty details including poster info.
#[pg_extern]
fn get_bounty(bounty_id: pgrx::Uuid) -> pgrx::JsonB {
    let bounty = Spi::get_one::<pgrx::JsonB>(&format!(
        "SELECT jsonb_build_object(
            'id', b.id,
            'poster_wallet', b.poster_wallet,
            'scope', b.scope::text,
            'description', b.description,
            'success_command', b.success_command,
            'reward', b.reward,
            'status', b.status,
            'claimed_by', b.claimed_by,
            'verified_at', b.verified_at,
            'created_at', b.created_at,
            'expires_at', b.expires_at
        ) FROM kerai.bounties b WHERE b.id = '{}'::uuid",
        bounty_id,
    ))
    .unwrap_or(None);

    match bounty {
        Some(b) => b,
        None => error!("Bounty not found: {}", bounty_id),
    }
}

/// Claim an open bounty. Sets status='claimed' and records claimer wallet.
#[pg_extern]
fn claim_bounty(bounty_id: pgrx::Uuid, claimer_wallet_id: pgrx::Uuid) -> pgrx::JsonB {
    // Verify claimer wallet exists
    let claimer_exists = Spi::get_one::<bool>(&format!(
        "SELECT EXISTS(SELECT 1 FROM kerai.wallets WHERE id = '{}'::uuid)",
        claimer_wallet_id,
    ))
    .unwrap()
    .unwrap_or(false);
    if !claimer_exists {
        error!("Claimer wallet not found: {}", claimer_wallet_id);
    }

    // Get current bounty status
    let status = Spi::get_one::<String>(&format!(
        "SELECT status FROM kerai.bounties WHERE id = '{}'::uuid",
        bounty_id,
    ))
    .unwrap_or(None);

    match status.as_deref() {
        None => error!("Bounty not found: {}", bounty_id),
        Some("open") => {}
        Some(s) => error!("Bounty cannot be claimed, currently '{}' (must be 'open')", s),
    }

    let row = Spi::get_one::<pgrx::JsonB>(&format!(
        "UPDATE kerai.bounties
         SET status = 'claimed', claimed_by = '{}'::uuid
         WHERE id = '{}'::uuid
         RETURNING jsonb_build_object(
             'id', id,
             'status', status,
             'claimed_by', claimed_by,
             'reward', reward,
             'scope', scope::text,
             'description', description
         )",
        claimer_wallet_id,
        bounty_id,
    ))
    .unwrap()
    .unwrap();
    row
}

/// Settle a claimed bounty: transfer reward from poster to claimer.
#[pg_extern]
fn settle_bounty(bounty_id: pgrx::Uuid) -> pgrx::JsonB {
    // Get bounty details
    let bounty = Spi::get_one::<pgrx::JsonB>(&format!(
        "SELECT jsonb_build_object(
            'id', id,
            'poster_wallet', poster_wallet,
            'claimed_by', claimed_by,
            'reward', reward,
            'status', status
        ) FROM kerai.bounties WHERE id = '{}'::uuid",
        bounty_id,
    ))
    .unwrap_or(None);

    let bounty = match bounty {
        Some(b) => b,
        None => error!("Bounty not found: {}", bounty_id),
    };

    let obj = bounty.0.as_object().unwrap();
    let status = obj["status"].as_str().unwrap();
    if status != "claimed" {
        error!(
            "Bounty must be 'claimed' to settle, currently '{}'",
            status
        );
    }

    let poster_wallet = obj["poster_wallet"].as_str().unwrap();
    let claimed_by = obj["claimed_by"]
        .as_str()
        .unwrap_or_else(|| error!("Bounty has no claimer"));
    let reward = obj["reward"].as_i64().unwrap();

    // Verify poster has sufficient balance
    let balance = Spi::get_one::<i64>(&format!(
        "SELECT COALESCE(
            (SELECT COALESCE(SUM(amount), 0) FROM kerai.ledger WHERE to_wallet = '{0}'::uuid)
            - (SELECT COALESCE(SUM(amount), 0) FROM kerai.ledger WHERE from_wallet = '{0}'::uuid),
            0
        )::bigint",
        sql_escape(poster_wallet),
    ))
    .unwrap()
    .unwrap_or(0);

    if balance < reward {
        error!(
            "Poster wallet has insufficient balance: {} Koi, needs {}",
            balance, reward
        );
    }

    // Get lamport timestamp
    let lamport = Spi::get_one::<i64>(
        "SELECT COALESCE(max(timestamp), 0) + 1 FROM kerai.ledger",
    )
    .unwrap()
    .unwrap_or(1);

    // Transfer reward
    Spi::run(&format!(
        "INSERT INTO kerai.ledger (from_wallet, to_wallet, amount, reason, reference_id, reference_type, timestamp)
         VALUES ('{}'::uuid, '{}'::uuid, {}, 'bounty_settlement', '{}'::uuid, 'bounty', {})",
        sql_escape(poster_wallet),
        sql_escape(claimed_by),
        reward,
        bounty_id,
        lamport,
    ))
    .unwrap();

    // Update bounty status
    Spi::run(&format!(
        "UPDATE kerai.bounties SET status = 'paid', verified_at = now() WHERE id = '{}'::uuid",
        bounty_id,
    ))
    .unwrap();

    pgrx::JsonB(serde_json::json!({
        "bounty_id": bounty_id.to_string(),
        "status": "paid",
        "reward": reward,
        "poster_wallet": poster_wallet,
        "claimed_by": claimed_by,
    }))
}
