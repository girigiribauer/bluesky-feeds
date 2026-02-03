use anyhow::Result;
use atrium_api::types::string::Nsid;
use jetstream_oxide::{
    events::{commit::CommitEvent, JetstreamEvent},
    JetstreamCompression, JetstreamConfig, JetstreamConnector,
};

const JETSTREAM_URL: &str = "wss://jetstream1.us-east.bsky.network/subscribe";

pub async fn connect_and_run<F, Fut>(callback: F) -> Result<()>
where
    F: Fn(CommitEvent) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = ()> + Send,
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

    while let Ok(event) = receiver.recv_async().await {
        if let JetstreamEvent::Commit(event) = event {
            callback(event).await;
        }
    }

    Ok(())
}
