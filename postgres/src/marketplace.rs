/// Marketplace — Dutch auction engine and market observability.
use pgrx::prelude::*;

use crate::sql::sql_escape;

/// Create a Dutch auction for an attestation. The seller must be the self instance.
#[pg_extern]
fn create_auction(
    attestation_id: pgrx::Uuid,
    starting_price: i64,
    price_decrement: i64,
    decrement_interval_secs: i64,
    floor_price: default!(i64, 0),
    min_bidders: default!(i32, 1),
    open_delay_hours: default!(i32, 24),
) -> pgrx::JsonB {
    if starting_price <= 0 {
        error!("starting_price must be positive");
    }
    if floor_price < 0 {
        error!("floor_price cannot be negative");
    }
    if floor_price >= starting_price {
        error!("floor_price must be less than starting_price");
    }
    if price_decrement <= 0 {
        error!("price_decrement must be positive");
    }
    if decrement_interval_secs <= 0 {
        error!("decrement_interval_secs must be positive");
    }

    // Verify attestation exists and belongs to self instance
    let att_exists = Spi::get_one::<bool>(&format!(
        "SELECT EXISTS(
            SELECT 1 FROM kerai.attestations a
            JOIN kerai.instances i ON a.instance_id = i.id
            WHERE a.id = '{}'::uuid AND i.is_self = true
        )",
        attestation_id,
    ))
    .unwrap()
    .unwrap_or(false);

    if !att_exists {
        error!("Attestation not found or not owned by this instance: {}", attestation_id);
    }

    // Check no active auction exists for this attestation
    let active_exists = Spi::get_one::<bool>(&format!(
        "SELECT EXISTS(
            SELECT 1 FROM kerai.auctions
            WHERE attestation_id = '{}'::uuid AND status = 'active'
        )",
        attestation_id,
    ))
    .unwrap()
    .unwrap_or(false);

    if active_exists {
        error!("An active auction already exists for attestation {}", attestation_id);
    }

    // Get self wallet
    let seller_wallet = Spi::get_one::<String>(
        "SELECT w.id::text FROM kerai.wallets w
         JOIN kerai.instances i ON w.instance_id = i.id
         WHERE i.is_self = true AND w.wallet_type = 'instance'",
    )
    .unwrap()
    .unwrap_or_else(|| error!("Self wallet not found"));

    let row = Spi::get_one::<pgrx::JsonB>(&format!(
        "INSERT INTO kerai.auctions (
            attestation_id, seller_wallet, starting_price, floor_price,
            current_price, price_decrement, decrement_interval,
            min_bidders, open_delay_hours
        ) VALUES (
            '{}'::uuid, '{}'::uuid, {}, {},
            {}, {}, '{} seconds'::interval,
            {}, {}
        ) RETURNING jsonb_build_object(
            'id', id,
            'attestation_id', attestation_id,
            'starting_price', starting_price,
            'floor_price', floor_price,
            'current_price', current_price,
            'price_decrement', price_decrement,
            'min_bidders', min_bidders,
            'status', status,
            'created_at', created_at
        )",
        attestation_id,
        sql_escape(&seller_wallet),
        starting_price,
        floor_price,
        starting_price, // current_price starts at starting_price
        price_decrement,
        decrement_interval_secs,
        min_bidders,
        open_delay_hours,
    ))
    .unwrap()
    .unwrap();
    row
}

/// Place a bid on an active auction. Bidder is the self instance wallet.
#[pg_extern]
fn place_bid(auction_id: pgrx::Uuid, max_price: i64) -> pgrx::JsonB {
    if max_price <= 0 {
        error!("max_price must be positive");
    }

    // Verify auction is active
    let status = Spi::get_one::<String>(&format!(
        "SELECT status FROM kerai.auctions WHERE id = '{}'::uuid",
        auction_id,
    ))
    .unwrap_or(None);

    match status.as_deref() {
        None => error!("Auction not found: {}", auction_id),
        Some("active") => {}
        Some(s) => error!("Auction is not active, currently '{}'", s),
    }

    // Get self wallet
    let bidder_wallet = Spi::get_one::<String>(
        "SELECT w.id::text FROM kerai.wallets w
         JOIN kerai.instances i ON w.instance_id = i.id
         WHERE i.is_self = true AND w.wallet_type = 'instance'",
    )
    .unwrap()
    .unwrap_or_else(|| error!("Self wallet not found"));

    let row = Spi::get_one::<pgrx::JsonB>(&format!(
        "INSERT INTO kerai.bids (auction_id, bidder_wallet, max_price)
         VALUES ('{}'::uuid, '{}'::uuid, {})
         RETURNING jsonb_build_object(
             'id', id,
             'auction_id', auction_id,
             'max_price', max_price,
             'created_at', created_at
         )",
        auction_id,
        sql_escape(&bidder_wallet),
        max_price,
    ))
    .unwrap()
    .unwrap();
    row
}

/// Advance the auction clock: decrement price, check floor hit, check settlement conditions.
#[pg_extern]
fn tick_auction(auction_id: pgrx::Uuid) -> pgrx::JsonB {
    // Get auction details
    let auction = Spi::get_one::<pgrx::JsonB>(&format!(
        "SELECT jsonb_build_object(
            'id', id,
            'current_price', current_price,
            'floor_price', floor_price,
            'price_decrement', price_decrement,
            'min_bidders', min_bidders,
            'status', status
        ) FROM kerai.auctions WHERE id = '{}'::uuid",
        auction_id,
    ))
    .unwrap_or(None);

    let auction = match auction {
        Some(a) => a,
        None => error!("Auction not found: {}", auction_id),
    };

    let obj = auction.0.as_object().unwrap();
    let status = obj["status"].as_str().unwrap();
    if status != "active" {
        error!("Auction is not active, currently '{}'", status);
    }

    let current_price = obj["current_price"].as_i64().unwrap();
    let floor_price = obj["floor_price"].as_i64().unwrap();
    let decrement = obj["price_decrement"].as_i64().unwrap();
    let min_bidders = obj["min_bidders"].as_i64().unwrap();

    let new_price = (current_price - decrement).max(floor_price);

    // Check if floor is hit
    if new_price <= floor_price {
        // Open-source immediately
        Spi::run(&format!(
            "UPDATE kerai.auctions
             SET current_price = {}, status = 'open_sourced',
                 open_sourced = true, open_sourced_at = now()
             WHERE id = '{}'::uuid",
            floor_price, auction_id,
        ))
        .unwrap();

        return pgrx::JsonB(serde_json::json!({
            "auction_id": auction_id.to_string(),
            "action": "open_sourced",
            "current_price": floor_price,
            "reason": "floor_price_hit",
        }));
    }

    // Update price
    Spi::run(&format!(
        "UPDATE kerai.auctions SET current_price = {} WHERE id = '{}'::uuid",
        new_price, auction_id,
    ))
    .unwrap();

    // Check settlement conditions: enough qualifying bidders?
    let qualifying = Spi::get_one::<i64>(&format!(
        "SELECT count(*)::bigint FROM kerai.bids
         WHERE auction_id = '{}'::uuid AND max_price >= {}",
        auction_id, new_price,
    ))
    .unwrap()
    .unwrap_or(0);

    if qualifying >= min_bidders {
        return pgrx::JsonB(serde_json::json!({
            "auction_id": auction_id.to_string(),
            "action": "settlement_ready",
            "current_price": new_price,
            "qualifying_bidders": qualifying,
        }));
    }

    pgrx::JsonB(serde_json::json!({
        "auction_id": auction_id.to_string(),
        "action": "price_decremented",
        "current_price": new_price,
        "qualifying_bidders": qualifying,
    }))
}

/// Settle an active auction: all qualifying bidders pay current_price.
#[pg_extern]
fn settle_auction(auction_id: pgrx::Uuid) -> pgrx::JsonB {
    let auction = Spi::get_one::<pgrx::JsonB>(&format!(
        "SELECT jsonb_build_object(
            'id', id,
            'current_price', current_price,
            'seller_wallet', seller_wallet,
            'min_bidders', min_bidders,
            'status', status
        ) FROM kerai.auctions WHERE id = '{}'::uuid",
        auction_id,
    ))
    .unwrap_or(None);

    let auction = match auction {
        Some(a) => a,
        None => error!("Auction not found: {}", auction_id),
    };

    let obj = auction.0.as_object().unwrap();
    let status = obj["status"].as_str().unwrap();
    if status != "active" {
        error!("Auction must be 'active' to settle, currently '{}'", status);
    }

    let current_price = obj["current_price"].as_i64().unwrap();
    let seller_wallet = obj["seller_wallet"].as_str().unwrap();
    let min_bidders = obj["min_bidders"].as_i64().unwrap();

    // Get qualifying bidders
    let bidders_json = Spi::get_one::<pgrx::JsonB>(&format!(
        "SELECT COALESCE(jsonb_agg(jsonb_build_object(
            'bid_id', id,
            'bidder_wallet', bidder_wallet,
            'max_price', max_price
        )), '[]'::jsonb)
        FROM kerai.bids
        WHERE auction_id = '{}'::uuid AND max_price >= {}",
        auction_id, current_price,
    ))
    .unwrap()
    .unwrap_or_else(|| pgrx::JsonB(serde_json::json!([])));

    let bidders = bidders_json.0.as_array().unwrap();
    let bidder_count = bidders.len() as i64;

    if bidder_count < min_bidders {
        error!(
            "Not enough qualifying bidders: {} < {} (min_bidders)",
            bidder_count, min_bidders
        );
    }

    // Get current lamport_ts for ledger entries
    let lamport = Spi::get_one::<i64>(
        "SELECT COALESCE(max(lamport_ts), 0) + 1 FROM kerai.operations",
    )
    .unwrap()
    .unwrap_or(1);

    // Create ledger entries for each winning bidder
    let mut total_revenue: i64 = 0;
    for bidder in bidders {
        let bidder_wallet_id = bidder["bidder_wallet"].as_str().unwrap();
        Spi::run(&format!(
            "INSERT INTO kerai.ledger (from_wallet, to_wallet, amount, reason, reference_id, reference_type, timestamp)
             VALUES ('{}'::uuid, '{}'::uuid, {}, 'auction_settlement', '{}'::uuid, 'auction', {})",
            sql_escape(bidder_wallet_id),
            sql_escape(seller_wallet),
            current_price,
            auction_id,
            lamport + total_revenue, // unique timestamp per entry
        ))
        .unwrap();
        total_revenue += current_price;
    }

    // Update auction status
    Spi::run(&format!(
        "UPDATE kerai.auctions
         SET status = 'settled', settled_price = {}, settled_at = now()
         WHERE id = '{}'::uuid",
        current_price, auction_id,
    ))
    .unwrap();

    pgrx::JsonB(serde_json::json!({
        "auction_id": auction_id.to_string(),
        "status": "settled",
        "settled_price": current_price,
        "bidder_count": bidder_count,
        "total_revenue": total_revenue,
    }))
}

/// Mark a settled auction as open-sourced (post-settlement release).
#[pg_extern]
fn open_source_auction(auction_id: pgrx::Uuid) -> pgrx::JsonB {
    let status = Spi::get_one::<String>(&format!(
        "SELECT status FROM kerai.auctions WHERE id = '{}'::uuid",
        auction_id,
    ))
    .unwrap_or(None);

    match status.as_deref() {
        None => error!("Auction not found: {}", auction_id),
        Some("settled") | Some("open_sourced") => {}
        Some(s) => error!("Auction must be 'settled' or already 'open_sourced' to open-source, currently '{}'", s),
    }

    Spi::run(&format!(
        "UPDATE kerai.auctions
         SET status = 'open_sourced', open_sourced = true, open_sourced_at = now()
         WHERE id = '{}'::uuid",
        auction_id,
    ))
    .unwrap();

    pgrx::JsonB(serde_json::json!({
        "auction_id": auction_id.to_string(),
        "status": "open_sourced",
    }))
}

/// Browse active auctions with optional filters.
#[pg_extern]
fn market_browse(
    scope_filter: Option<&str>,
    max_price: Option<i64>,
    status_filter: Option<&str>,
) -> pgrx::JsonB {
    let mut conditions = Vec::new();

    match status_filter {
        Some(s) => conditions.push(format!("au.status = '{}'", sql_escape(s))),
        None => conditions.push("au.status = 'active'".to_string()),
    }

    if let Some(scope) = scope_filter {
        conditions.push(format!("at.scope <@ '{}'::ltree", sql_escape(scope)));
    }
    if let Some(price) = max_price {
        conditions.push(format!("au.current_price <= {}", price));
    }

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };

    let json = Spi::get_one::<pgrx::JsonB>(&format!(
        "SELECT COALESCE(
            jsonb_agg(jsonb_build_object(
                'auction_id', au.id,
                'attestation_id', au.attestation_id,
                'scope', at.scope::text,
                'claim_type', at.claim_type,
                'current_price', au.current_price,
                'floor_price', au.floor_price,
                'starting_price', au.starting_price,
                'status', au.status,
                'min_bidders', au.min_bidders,
                'bid_count', (SELECT count(*) FROM kerai.bids b WHERE b.auction_id = au.id),
                'created_at', au.created_at
            ) ORDER BY au.current_price ASC),
            '[]'::jsonb
        )
        FROM kerai.auctions au
        JOIN kerai.attestations at ON au.attestation_id = at.id
        {}",
        where_clause,
    ))
    .unwrap()
    .unwrap_or_else(|| pgrx::JsonB(serde_json::json!([])));
    json
}

/// Get detailed auction status including bid history.
#[pg_extern]
fn market_status(auction_id: pgrx::Uuid) -> pgrx::JsonB {
    let auction = Spi::get_one::<pgrx::JsonB>(&format!(
        "SELECT jsonb_build_object(
            'id', au.id,
            'attestation_id', au.attestation_id,
            'scope', at.scope::text,
            'claim_type', at.claim_type,
            'starting_price', au.starting_price,
            'current_price', au.current_price,
            'floor_price', au.floor_price,
            'price_decrement', au.price_decrement,
            'min_bidders', au.min_bidders,
            'status', au.status,
            'settled_price', au.settled_price,
            'open_sourced', au.open_sourced,
            'open_sourced_at', au.open_sourced_at,
            'open_delay_hours', au.open_delay_hours,
            'created_at', au.created_at,
            'settled_at', au.settled_at
        )
        FROM kerai.auctions au
        JOIN kerai.attestations at ON au.attestation_id = at.id
        WHERE au.id = '{}'::uuid",
        auction_id,
    ))
    .unwrap_or(None);

    let mut auction = match auction {
        Some(a) => a,
        None => error!("Auction not found: {}", auction_id),
    };

    // Attach bids
    let bids = Spi::get_one::<pgrx::JsonB>(&format!(
        "SELECT COALESCE(
            jsonb_agg(jsonb_build_object(
                'id', b.id,
                'max_price', b.max_price,
                'created_at', b.created_at
            ) ORDER BY b.created_at),
            '[]'::jsonb
        ) FROM kerai.bids b WHERE b.auction_id = '{}'::uuid",
        auction_id,
    ))
    .unwrap()
    .unwrap_or_else(|| pgrx::JsonB(serde_json::json!([])));

    if let Some(obj) = auction.0.as_object_mut() {
        obj.insert("bids".to_string(), bids.0);
    }

    auction
}

/// View self instance's marketplace earnings and spending.
#[pg_extern]
fn market_balance() -> pgrx::JsonB {
    let self_wallet = Spi::get_one::<String>(
        "SELECT w.id::text FROM kerai.wallets w
         JOIN kerai.instances i ON w.instance_id = i.id
         WHERE i.is_self = true AND w.wallet_type = 'instance'",
    )
    .unwrap()
    .unwrap_or_else(|| error!("Self wallet not found"));

    let earnings = Spi::get_one::<i64>(&format!(
        "SELECT COALESCE(sum(amount), 0)::bigint FROM kerai.ledger
         WHERE to_wallet = '{}'::uuid AND reference_type = 'auction'",
        sql_escape(&self_wallet),
    ))
    .unwrap()
    .unwrap_or(0);

    let spending = Spi::get_one::<i64>(&format!(
        "SELECT COALESCE(sum(amount), 0)::bigint FROM kerai.ledger
         WHERE from_wallet = '{}'::uuid AND reference_type = 'auction'",
        sql_escape(&self_wallet),
    ))
    .unwrap()
    .unwrap_or(0);

    let active_auctions = Spi::get_one::<i64>(&format!(
        "SELECT count(*)::bigint FROM kerai.auctions
         WHERE seller_wallet = '{}'::uuid AND status = 'active'",
        sql_escape(&self_wallet),
    ))
    .unwrap()
    .unwrap_or(0);

    let active_bids = Spi::get_one::<i64>(&format!(
        "SELECT count(*)::bigint FROM kerai.bids
         WHERE bidder_wallet = '{}'::uuid
           AND auction_id IN (SELECT id FROM kerai.auctions WHERE status = 'active')",
        sql_escape(&self_wallet),
    ))
    .unwrap()
    .unwrap_or(0);

    pgrx::JsonB(serde_json::json!({
        "earnings": earnings,
        "spending": spending,
        "net": earnings - spending,
        "active_auctions": active_auctions,
        "active_bids": active_bids,
    }))
}

/// Browse open-sourced knowledge in the Koi Pond.
#[pg_extern]
fn market_commons(scope_filter: Option<&str>, since: Option<&str>) -> pgrx::JsonB {
    let mut conditions = vec!["au.open_sourced = true".to_string()];

    if let Some(scope) = scope_filter {
        conditions.push(format!("at.scope <@ '{}'::ltree", sql_escape(scope)));
    }
    if let Some(since_ts) = since {
        conditions.push(format!("au.open_sourced_at >= '{}'::timestamptz", sql_escape(since_ts)));
    }

    let where_clause = format!("WHERE {}", conditions.join(" AND "));

    let json = Spi::get_one::<pgrx::JsonB>(&format!(
        "SELECT COALESCE(
            jsonb_agg(jsonb_build_object(
                'auction_id', au.id,
                'attestation_id', au.attestation_id,
                'scope', at.scope::text,
                'claim_type', at.claim_type,
                'settled_price', au.settled_price,
                'open_sourced_at', au.open_sourced_at
            ) ORDER BY au.open_sourced_at DESC),
            '[]'::jsonb
        )
        FROM kerai.auctions au
        JOIN kerai.attestations at ON au.attestation_id = at.id
        {}",
        where_clause,
    ))
    .unwrap()
    .unwrap_or_else(|| pgrx::JsonB(serde_json::json!([])));
    json
}

/// Market-wide statistics.
#[pg_extern]
fn market_stats() -> pgrx::JsonB {
    let stats = Spi::get_one::<pgrx::JsonB>(
        "SELECT jsonb_build_object(
            'active_auctions', (SELECT count(*) FROM kerai.auctions WHERE status = 'active'),
            'settled_auctions', (SELECT count(*) FROM kerai.auctions WHERE status = 'settled'),
            'open_sourced', (SELECT count(*) FROM kerai.auctions WHERE open_sourced = true),
            'total_bids', (SELECT count(*) FROM kerai.bids),
            'total_settlement_value', (SELECT COALESCE(sum(amount), 0) FROM kerai.ledger WHERE reference_type = 'auction'),
            'avg_settlement_price', (
                SELECT COALESCE(round(avg(settled_price)), 0)
                FROM kerai.auctions WHERE settled_price IS NOT NULL
            )
        )",
    )
    .unwrap()
    .unwrap_or_else(|| pgrx::JsonB(serde_json::json!({})));
    stats
}
