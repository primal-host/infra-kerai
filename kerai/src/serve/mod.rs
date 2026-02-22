pub mod auth;
pub mod config;
pub mod db;
pub mod notify;
pub mod oauth;
pub mod routes;

use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;

use config::Config;

/// Run the web server.
pub async fn run(addr: &str, db_url: &str) {
    tracing_subscriber::fmt::init();

    let config = Config {
        database_url: db_url.to_string(),
        listen_addr: addr.to_string(),
        static_dir: std::env::var("STATIC_DIR").ok(),
    };

    tracing::info!("Starting kerai serve on {}", config.listen_addr);
    tracing::info!("Database: {}", config.database_url);

    // Database pool
    let pool = db::Pool::new(config.clone());

    // Start LISTEN/NOTIFY background task
    let notify_tx = notify::start_listener(config.database_url.clone());

    // Build router
    let mut app = routes::build_router(pool, notify_tx)
        .layer(CorsLayer::permissive());

    // Serve static files if configured
    if let Some(ref static_dir) = config.static_dir {
        tracing::info!("Serving static files from {}", static_dir);
        app = app.fallback_service(ServeDir::new(static_dir));
    }

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("Failed to bind");

    tracing::info!("Listening on {}", addr);
    axum::serve(listener, app).await.expect("Server error");
}
