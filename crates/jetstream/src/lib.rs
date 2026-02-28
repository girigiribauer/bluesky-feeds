use anyhow::Result;
use atrium_api::types::string::Nsid;
use chrono::{DateTime, Utc};
use jetstream_oxide::{
    events::{commit::CommitEvent, JetstreamEvent},
    JetstreamCompression, JetstreamConfig, JetstreamConnector,
};

const JETSTREAM_URL: &str = "wss://jetstream2.us-west.bsky.network/subscribe";

pub async fn connect_and_run<F, Fut>(cursor: Option<DateTime<Utc>>, callback: F) -> Result<()>
where
    F: Fn(CommitEvent) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = ()> + Send,
{
    tracing::info!(
        "Connecting to Jetstream at {} (cursor: {:?})",
        JETSTREAM_URL,
        cursor
    );

    let config = JetstreamConfig {
        endpoint: JETSTREAM_URL.to_string(),
        wanted_collections: vec![Nsid::new("app.bsky.feed.post".to_string()).unwrap()],
        wanted_dids: vec![],
        compression: JetstreamCompression::Zstd,
        cursor,
        base_delay_ms: 5000,  // 5秒からスタート
        max_delay_ms: 600000, // 最大 10 分まで
        max_retries: 300,     // 合計で約 50 時間リトライを継続
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
