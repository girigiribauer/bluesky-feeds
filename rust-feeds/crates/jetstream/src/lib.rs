use anyhow::Result;
use jetstream_oxide::{
    events::{commit::CommitEvent, JetstreamEvent},
    JetstreamCompression, JetstreamConfig, JetstreamConnector,
};
use atrium_api::types::string::Nsid;
use tracing;

const JETSTREAM_URL: &str = "wss://jetstream1.us-east.bsky.network/subscribe";

pub async fn connect_and_run<F>(callback: F) -> Result<()>
where
    F: Fn(&CommitEvent) + Send + Sync + 'static,
{
    tracing::info!("Connecting to Jetstream at {}", JETSTREAM_URL);

    let config = JetstreamConfig {
        endpoint: JETSTREAM_URL.to_string(),
        wanted_collections: vec![Nsid::new("app.bsky.feed.post".to_string()).unwrap()],
        wanted_dids: vec![],
        compression: JetstreamCompression::Zstd,
        cursor: None,
        base_delay_ms: 100,
        max_delay_ms: 3000,
        max_retries: 5,
        reset_retries_min_ms: 60000,
    };

    let connector = JetstreamConnector::new(config)?;
    let receiver = connector.connect().await?;
    tracing::info!("Jetstream connected successfully");

    let mut count = 0;
    while let Ok(event) = receiver.recv_async().await {
        count += 1;
        if count % 1000 == 0 {
            tracing::info!("Jetstream heartbeat: received {} events", count);
        }
        if let JetstreamEvent::Commit(event) = event {
            callback(&event);
        }
    }
    tracing::warn!("Jetstream receiver loop exited (connection closed?)");


    Ok(())
}
