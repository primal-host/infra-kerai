use pgrx::prelude::*;

use crate::identity;

/// Get the system hostname
fn get_hostname() -> String {
    std::process::Command::new("hostname")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "kerai-instance".to_string())
}

/// Format bytes as PostgreSQL hex bytea literal: \xABCD...
fn bytes_to_pg_hex(bytes: &[u8]) -> String {
    let hex: String = bytes.iter().map(|b| format!("{:02x}", b)).collect();
    format!("\\x{}", hex)
}

use crate::sql::sql_escape;

/// Bootstrap the self instance and wallet. Idempotent — skips if already exists.
#[pg_extern]
fn bootstrap_instance() -> &'static str {
    // Check if self instance already exists
    let exists = Spi::get_one::<bool>(
        "SELECT EXISTS(SELECT 1 FROM kerai.instances WHERE is_self = true)",
    )
    .unwrap_or(Some(false))
    .unwrap_or(false);

    if exists {
        info!("Self instance already exists, skipping bootstrap");
        return "already_bootstrapped";
    }

    // Generate keypair
    let (_, verifying_key) = identity::generate_keypair();
    let pk_hex = bytes_to_pg_hex(verifying_key.as_bytes());
    let fp = identity::fingerprint(&verifying_key);
    let hostname = get_hostname();

    // Insert self instance
    Spi::run(&format!(
        "INSERT INTO kerai.instances (name, public_key, key_fingerprint, is_self) \
         VALUES ('{}', '{}'::bytea, '{}', true)",
        sql_escape(&hostname),
        pk_hex,
        sql_escape(&fp)
    ))
    .expect("Failed to insert self instance");

    // Insert self wallet using subquery for instance_id
    Spi::run(&format!(
        "INSERT INTO kerai.wallets (instance_id, public_key, key_fingerprint, wallet_type, label) \
         SELECT id, '{}'::bytea, '{}', 'instance', 'Self Wallet' \
         FROM kerai.instances WHERE is_self = true",
        pk_hex,
        sql_escape(&fp)
    ))
    .expect("Failed to insert self wallet");

    info!("Bootstrapped self instance '{}' with wallet", hostname);
    "bootstrapped"
}

// Auto-run bootstrap after all tables are created
extension_sql!(
    r#"
SELECT kerai.bootstrap_instance();
"#,
    name = "run_bootstrap",
    finalize
);
