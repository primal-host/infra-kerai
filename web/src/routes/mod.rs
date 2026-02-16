pub mod documents;
pub mod health;
pub mod models;
pub mod nodes;
pub mod perspectives;
pub mod search;
pub mod ws;

use axum::routing::{delete, get, patch, post};
use axum::Router;
use std::sync::Arc;
use tokio::sync::broadcast;

use crate::db::Pool;
use ws::WsState;

/// Build the application router with all API routes.
pub fn build_router(pool: Arc<Pool>, notify_tx: broadcast::Sender<String>) -> Router {
    let ws_state = Arc::new(WsState {
        pool: pool.clone(),
        notify_tx,
    });

    let api = Router::new()
        // Health
        .route("/health", get(health::health))
        // Nodes CRUD
        .route("/nodes", post(nodes::create_node))
        .route("/nodes/{id}/content", patch(nodes::update_content))
        .route("/nodes/{id}/move", post(nodes::move_node))
        .route("/nodes/{id}", delete(nodes::delete_node))
        // Documents
        .route("/documents", post(documents::create_document))
        .route("/documents", get(documents::list_documents))
        .route("/documents/{id}/tree", get(documents::document_tree))
        .route("/documents/{id}/markdown", get(documents::document_markdown))
        // Search
        .route("/search", get(search::search))
        .route("/suggest", get(search::suggest))
        // Perspectives
        .route("/perspectives", get(perspectives::get_perspectives))
        .route("/consensus", get(perspectives::consensus))
        // Models
        .route("/models", post(models::create_model))
        .route("/models/train", post(models::train_model))
        .route("/models/predict", post(models::predict_next))
        .route("/models/search", get(models::neural_search))
        .route("/models/ensemble", post(models::ensemble_predict))
        .route("/models/{agent}/info", get(models::model_info))
        .route("/models/{agent}", delete(models::delete_model))
        .route("/models/feedback", post(models::record_selection))
        .with_state(pool);

    // WebSocket needs its own state
    let ws_router = Router::new()
        .route("/ws", get(ws::ws_handler))
        .with_state(ws_state);

    Router::new()
        .nest("/api", api)
        .nest("/api", ws_router)
}
