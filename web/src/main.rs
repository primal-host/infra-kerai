mod config;
mod db;
mod notify;
mod routes;

use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let config = config::Config::from_env();
    tracing::info!("Starting kerai-web on {}", config.listen_addr);
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

    let listener = tokio::net::TcpListener::bind(&config.listen_addr)
        .await
        .expect("Failed to bind");

    tracing::info!("Listening on {}", config.listen_addr);
    axum::serve(listener, app).await.expect("Server error");
}
