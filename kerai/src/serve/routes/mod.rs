pub mod connections;
pub mod documents;
pub mod eval;
pub mod health;
pub mod models;
pub mod nodes;
pub mod perspectives;
pub mod search;
pub mod stack;
pub mod ws;

use axum::routing::{delete, get, patch, post, put};
use axum::Router;
use std::sync::Arc;
use tokio::sync::broadcast;

use super::auth;
use super::db::Pool;
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
        // Stack (legacy instance-scoped)
        .route("/stack", get(stack::stack_peek))
        .route("/stack", put(stack::stack_replace))
        .route("/stack", delete(stack::stack_drop))
        .route("/stack/list", get(stack::stack_list))
        .route("/stack/push", post(stack::stack_push))
        .route("/stack/all", delete(stack::stack_clear))
        // Init
        .route("/init/pull", post(stack::init_pull))
        .route("/init/push", post(stack::init_push))
        .route("/init/diff", get(stack::init_diff))
        // Connections
        .route("/connections", get(connections::connections))
        .route("/workspace/switch", post(connections::switch_workspace))
        .with_state(pool.clone());

    // WebSocket needs its own state
    let ws_router = Router::new()
        .route("/ws", get(ws::ws_handler))
        .with_state(ws_state);

    // Eval route (stack machine)
    let eval_router = Router::new()
        .route("/eval", post(eval::eval))
        .with_state(pool.clone());

    // OAuth metadata routes (top-level, not nested)
    let oauth_meta_router = Router::new()
        .route("/.well-known/oauth-client-metadata", get(auth::client_metadata))
        .route("/oauth/jwks.json", get(auth::jwks))
        .with_state(pool.clone());

    // Auth routes
    let auth_router = Router::new()
        .route("/session", get(auth::get_session))
        .route("/bsky/start", post(auth::bsky_start))
        .route("/bsky/callback", get(auth::bsky_callback))
        .route("/logout", post(auth::logout))
        .with_state(pool);

    Router::new()
        .route("/", get(eval::terminal_page))
        .merge(oauth_meta_router)
        .nest("/api", api)
        .nest("/api", ws_router)
        .nest("/api", eval_router)
        .nest("/auth", auth_router)
}
