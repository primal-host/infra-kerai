use pgrx::prelude::*;

/// Returns the token balance for the self instance's wallet
#[pg_extern]
fn wallet_balance() -> i64 {
    let balance = Spi::get_one::<i64>(
        "SELECT COALESCE(
            (SELECT COALESCE(SUM(amount), 0) FROM kerai.ledger
             WHERE to_wallet = (SELECT id FROM kerai.wallets WHERE instance_id = (SELECT id FROM kerai.instances WHERE is_self = true)))
            -
            (SELECT COALESCE(SUM(amount), 0) FROM kerai.ledger
             WHERE from_wallet = (SELECT id FROM kerai.wallets WHERE instance_id = (SELECT id FROM kerai.instances WHERE is_self = true))),
            0
        )::bigint",
    )
    .unwrap_or(None)
    .unwrap_or(0);

    balance
}
