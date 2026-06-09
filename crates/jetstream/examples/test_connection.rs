/// ライブラリ単体の挙動確認用スクリプト
/// 本番と同じ設定（max_retries: 0, Microcosm, zstd）で接続し、
/// イベント受信のタイムスタンプと接続継続時間を表示する。
///
/// 使い方:
///   cargo run --example test_connection -p jetstream
use atrium_api::types::string::Nsid;
use jetstream_oxide::{
    events::{commit::CommitEvent, JetstreamEvent},
    JetstreamCompression, JetstreamConfig, JetstreamConnector,
};
use std::time::Instant;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let endpoint = std::env::var("JETSTREAM_URL")
        .unwrap_or_else(|_| "wss://jetstream2.fr.hose.cam/subscribe".to_string());

    println!("endpoint: {}", endpoint);
    println!("max_retries: 0 (本番と同じ設定)");
    println!("compression: zstd");
    println!("---");

    let config = JetstreamConfig {
        endpoint,
        wanted_collections: vec![Nsid::new("app.bsky.feed.post".to_string()).unwrap()],
        wanted_dids: vec![],
        compression: JetstreamCompression::Zstd,
        cursor: None,
        base_delay_ms: 5000,
        max_delay_ms: 600000,
        max_retries: 0,
        reset_retries_min_ms: 60000,
    };

    let connector = JetstreamConnector::new(config)?;
    let start = Instant::now();

    println!(
        "{:.1}s: connect() 呼び出し中...",
        start.elapsed().as_secs_f64()
    );
    let receiver = connector.connect().await?;
    println!(
        "{:.1}s: connect() 返却（注意: まだ接続確立されていない可能性あり）",
        start.elapsed().as_secs_f64()
    );

    let timeout = std::time::Duration::from_secs(60);
    let mut count = 0u64;
    loop {
        match tokio::time::timeout(
            timeout.saturating_sub(start.elapsed()),
            receiver.recv_async(),
        )
        .await
        {
            Err(_) => {
                println!(
                    "{:.1}s: タイムアウト（{}秒経過）。受信イベント数={}",
                    start.elapsed().as_secs_f64(),
                    timeout.as_secs(),
                    count
                );
                break;
            }
            Ok(Err(_)) => break,
            Ok(Ok(event)) => {
                count += 1;
                let elapsed = start.elapsed().as_secs_f64();
                if let JetstreamEvent::Commit(commit) = event {
                    let op = match &commit {
                        CommitEvent::Create { .. } => "create",
                        CommitEvent::Delete { .. } => "delete",
                        CommitEvent::Update { .. } => "update",
                    };
                    if count <= 5 || count % 100 == 0 {
                        println!("{:.1}s: イベント#{} op={}", elapsed, count, op);
                    }
                }
            }
        }
    }

    println!(
        "{:.1}s: チャンネル切断。受信イベント数={}",
        start.elapsed().as_secs_f64(),
        count
    );

    Ok(())
}
