/// Background task: LISTEN kerai_ops → broadcast to WebSocket clients.
use futures::StreamExt;
use tokio::sync::broadcast;
use tokio_postgres::NoTls;

/// Start the LISTEN background task.
/// Returns a broadcast Sender that WebSocket handlers subscribe to.
pub fn start_listener(database_url: String) -> broadcast::Sender<String> {
    let (tx, _) = broadcast::channel::<String>(256);
    let tx_clone = tx.clone();

    tokio::spawn(async move {
        loop {
            match listen_loop(&database_url, &tx_clone).await {
                Ok(()) => {
                    tracing::info!("LISTEN connection closed, reconnecting...");
                }
                Err(e) => {
                    tracing::error!("LISTEN error: {}, reconnecting in 2s...", e);
                }
            }
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        }
    });

    tx
}

async fn listen_loop(
    database_url: &str,
    tx: &broadcast::Sender<String>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (client, mut connection) = tokio_postgres::connect(database_url, NoTls).await?;

    // Poll the connection for async messages (notifications)
    let stream = futures::stream::poll_fn(move |cx| connection.poll_message(cx));
    let mut stream = std::pin::pin!(stream);

    // Start LISTEN on the kerai_ops channel
    client.execute("LISTEN kerai_ops", &[]).await?;
    tracing::info!("LISTEN kerai_ops started");

    // Forward notifications to the broadcast channel
    while let Some(msg) = stream.next().await {
        match msg? {
            tokio_postgres::AsyncMessage::Notification(n) => {
                let payload = n.payload().to_string();
                tracing::debug!("notification: {}", payload);
                // Ignore send errors (no active receivers)
                let _ = tx.send(payload);
            }
            _ => {}
        }
    }

    Ok(())
}
