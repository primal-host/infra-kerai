/// Database connection pool using tokio-postgres.
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_postgres::{Client, NoTls};

use super::config::Config;

/// Simple connection pool wrapper.
pub struct Pool {
    config: Config,
    client: Mutex<Option<Client>>,
}

impl Pool {
    pub fn new(config: Config) -> Arc<Self> {
        Arc::new(Self {
            config,
            client: Mutex::new(None),
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

    /// Extract postgres host from connection string.
    pub fn pg_host(&self) -> &str {
        let url = &self.config.database_url;
        // Key=value format: "host=/tmp dbname=kerai"
        if let Some(pos) = url.find("host=") {
            let rest = &url[pos + 5..];
            rest.split_whitespace().next().unwrap_or("?")
        // URI format: "postgresql://user:pass@host:port/db"
        } else if let Some(at) = url.find('@') {
            let rest = &url[at + 1..];
            rest.split(&[':', '/'][..]).next().unwrap_or("?")
        } else {
            "localhost"
        }
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
