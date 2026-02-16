use axum::extract::{State, WebSocketUpgrade};
use axum::extract::ws::{Message, WebSocket};
use axum::response::IntoResponse;
use futures::{SinkExt, StreamExt};
use std::sync::Arc;
use tokio::sync::broadcast;

use crate::db::Pool;

/// Shared state for WebSocket handlers.
pub struct WsState {
    pub pool: Arc<Pool>,
    pub notify_tx: broadcast::Sender<String>,
}

/// GET /api/ws — WebSocket upgrade
pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<WsState>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(socket: WebSocket, state: Arc<WsState>) {
    let (mut sender, mut receiver) = socket.split();

    // Subscribe to NOTIFY broadcast
    let mut notify_rx = state.notify_tx.subscribe();

    // Forward notifications to WebSocket client
    let send_task = tokio::spawn(async move {
        while let Ok(payload) = notify_rx.recv().await {
            if sender.send(Message::Text(payload.into())).await.is_err() {
                break;
            }
        }
    });

    // Receive messages from WebSocket client (operations)
    let pool = state.pool.clone();
    let recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            match msg {
                Message::Text(text) => {
                    // Parse as operation and execute
                    if let Err(e) = handle_client_op(&pool, &text).await {
                        tracing::warn!("client op error: {}", e);
                    }
                }
                Message::Close(_) => break,
                _ => {}
            }
        }
    });

    // Wait for either task to finish
    tokio::select! {
        _ = send_task => {},
        _ = recv_task => {},
    }
}

async fn handle_client_op(pool: &Pool, text: &str) -> Result<(), String> {
    let op: serde_json::Value = serde_json::from_str(text)
        .map_err(|e| format!("invalid JSON: {}", e))?;

    let op_type = op["op_type"].as_str()
        .ok_or_else(|| "missing op_type".to_string())?;

    let node_id_param = op.get("node_id")
        .and_then(|v| v.as_str())
        .map(|id| format!("'{}'::uuid", id.replace('\'', "''")))
        .unwrap_or_else(|| "NULL".to_string());

    let payload = op.get("payload")
        .unwrap_or(&serde_json::json!({}));

    let sql = format!(
        "SELECT kerai.apply_op('{}', {}, '{}'::jsonb)",
        op_type.replace('\'', "''"),
        node_id_param,
        payload.to_string().replace('\'', "''"),
    );

    let client = pool.get().await.map_err(|e| e.to_string())?;
    client.execute(&sql, &[]).await.map_err(|e| e.to_string())?;

    Ok(())
}
