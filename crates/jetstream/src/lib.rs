use anyhow::Result;
use atrium_api::types::string::Nsid;
use chrono::Utc;
use jetstream_oxide::{
    events::{commit::CommitEvent, JetstreamEvent},
    JetstreamCompression, JetstreamConfig, JetstreamConnector,
};
use std::time::Duration;

const JETSTREAM_URL: &str = "wss://jetstream1.us-east.bsky.network/subscribe";

/// Jetstream のイベントを受信し続けるループ。
///
/// - `initial_cursor`: 前回処理した最後のイベントの `time_us`（マイクロ秒）。
///   `Some` の場合はその時刻から再生（バックフィル）が行われる。
///   `None` の場合はライブテール（最新から）で開始する。
///
/// - `callback`: イベントを受け取る非同期関数。処理したイベントの `time_us` を返す。
///   この値がカーソルとして保存され、次回の再接続に使われる。
///
/// この関数はゾンビ接続（Ping 失敗後に接続が固まる問題）を防ぐため、
/// 60秒間メッセージが届かない場合に強制的に再接続を行う。
pub async fn connect_and_run<F, Fut>(callback: F, initial_cursor: Option<i64>) -> Result<()>
where
    F: Fn(CommitEvent) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = Option<i64>> + Send,
{
    tracing::info!("Connecting to Jetstream at {}", JETSTREAM_URL);

    let mut cursor_us: Option<i64> = initial_cursor;

    loop {
        let cursor_for_connect = cursor_us.map(|us| {
            // time_us はマイクロ秒なので chrono::DateTime に変換
            chrono::DateTime::from_timestamp_micros(us).unwrap_or_else(Utc::now)
        });

        let config = JetstreamConfig {
            endpoint: JETSTREAM_URL.to_string(),
            wanted_collections: vec![Nsid::new("app.bsky.feed.post".to_string()).unwrap()],
            wanted_dids: vec![],
            compression: JetstreamCompression::Zstd,
            cursor: cursor_for_connect,
            base_delay_ms: 5000, // 5秒からスタート
            max_delay_ms: 30000, // 最大 30 秒（元の設定を戻す）
            max_retries: 300,    // 合計で約 50 時間リトライを継続
            reset_retries_min_ms: 60000,
        };

        let connector = match JetstreamConnector::new(config) {
            Ok(c) => c,
            Err(e) => {
                tracing::error!("Failed to create Jetstream connector: {}", e);
                tokio::time::sleep(Duration::from_secs(5)).await;
                continue;
            }
        };

        let receiver = match connector.connect().await {
            Ok(r) => r,
            Err(e) => {
                tracing::error!("Failed to connect to Jetstream: {}", e);
                tokio::time::sleep(Duration::from_secs(5)).await;
                continue;
            }
        };

        tracing::info!("Jetstream connected. cursor={:?}", cursor_us);

        // ゾンビ接続対策: 60秒以内にメッセージが届かなければ強制再接続
        let timeout_duration = Duration::from_secs(60);

        loop {
            match tokio::time::timeout(timeout_duration, receiver.recv_async()).await {
                Ok(Ok(JetstreamEvent::Commit(event))) => {
                    if let Some(new_cursor) = callback(event).await {
                        cursor_us = Some(new_cursor);
                    }
                }
                Ok(Ok(_)) => {
                    // Commit 以外のイベント（Identity, Account など）は無視
                }
                Ok(Err(_)) => {
                    // チャネルが閉じた = ライブラリが再接続ループを終了した
                    tracing::warn!("Jetstream channel closed. Reconnecting...");
                    break;
                }
                Err(_) => {
                    // タイムアウト = ゾンビ接続の可能性が高い
                    tracing::warn!(
                        "Jetstream receive timeout ({}s). Suspected zombie connection. Reconnecting...",
                        timeout_duration.as_secs()
                    );
                    break;
                }
            }
        }

        // 再接続前に少し待つ
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}
