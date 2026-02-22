/// Database connection pool using tokio-postgres.
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_postgres::{Client, NoTls};

use super::config::Config;

/// Simple connection pool wrapper.
pub struct Pool {
    config: Config,
    client: Mutex<Option<Client>>,
    pg_host: String,
}

impl Pool {
    pub fn new(config: Config) -> Arc<Self> {
        let raw_host = parse_host(&config.database_url);
        let pg_host = if raw_host.starts_with('/') {
            // Unix socket — resolve to system hostname
            std::process::Command::new("hostname")
                .output()
                .ok()
                .and_then(|o| String::from_utf8(o.stdout).ok())
                .map(|s| s.trim().to_string())
                .unwrap_or_else(|| "localhost".into())
        } else {
            raw_host
        };
        Arc::new(Self {
            config,
            client: Mutex::new(None),
            pg_host,
        })
    }

    /// Get a database client, reconnecting if needed.
    pub async fn get(&self) -> Result<tokio_postgres::Client, tokio_postgres::Error> {
        let mut guard = self.client.lock().await;

        // Check if existing connection is still alive
        if let Some(ref client) = *guard {
            if !client.is_closed() {
                // Return a new connection since Client isn't Clone
                // In production, use bb8 or deadpool for proper pooling
                drop(guard);
                return self.connect().await;
            }
        }

        let client = self.connect().await?;
        *guard = Some(client);
        // Return a fresh connection for the caller
        drop(guard);
        self.connect().await
    }

    /// Postgres host name (resolved at startup).
    pub fn pg_host(&self) -> &str {
        &self.pg_host
    }

    async fn connect(&self) -> Result<tokio_postgres::Client, tokio_postgres::Error> {
        let (client, connection) = tokio_postgres::connect(&self.config.database_url, NoTls).await?;

        // Spawn the connection handler
        tokio::spawn(async move {
            if let Err(e) = connection.await {
                tracing::error!("database connection error: {}", e);
            }
        });

        Ok(client)
    }
}

fn parse_host(url: &str) -> String {
    // Key=value format: "host=/tmp dbname=kerai"
    if let Some(pos) = url.find("host=") {
        let rest = &url[pos + 5..];
        return rest.split_whitespace().next().unwrap_or("localhost").to_string();
    }
    // URI format: "postgresql://user:pass@host:port/db"
    if let Some(at) = url.find('@') {
        let rest = &url[at + 1..];
        return rest.split(&[':', '/'][..]).next().unwrap_or("localhost").to_string();
    }
    "localhost".into()
}
