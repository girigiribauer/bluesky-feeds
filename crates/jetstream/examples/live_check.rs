use jetstream::event::SkipStats;
use jetstream::ws::{build_subscribe_url, run_session, SessionEnd, SkipCounters};
use jetstream::POST_COLLECTION;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;

const WANTED_POSTS: usize = 20;
const DEFAULT_ENDPOINT: &str = "wss://jetstream1.us-east.bsky.network/subscribe";

#[tokio::main]
async fn main() -> ExitCode {
    let endpoint = std::env::var("JETSTREAM_URL").unwrap_or_else(|_| DEFAULT_ENDPOINT.to_string());
    let url = build_subscribe_url(&endpoint, &[POST_COLLECTION.to_string()], None);

    let (tx, mut rx) = mpsc::channel(64);
    let counters = Arc::new(SkipCounters::default());
    let session = tokio::spawn(run_session(url, tx, Duration::from_secs(15), counters));

    let mut received = 0;
    while received < WANTED_POSTS {
        match tokio::time::timeout(Duration::from_secs(15), rx.recv()).await {
            Ok(Some(_)) => received += 1,
            Ok(None) | Err(_) => break,
        }
    }
    drop(rx);

    let outcome = session.await.expect("receiver task panicked");
    let mut skips = SkipStats::default();
    skips.merge(&outcome.skips);

    println!("接続先: {endpoint}");
    println!("受け取った投稿: {received} 件");
    println!("切断理由: {:?}", outcome.end);
    println!("読み飛ばし: {skips:?}");

    if matches!(outcome.end, SessionEnd::ConnectFailed(_)) {
        println!("結果: 接続できませんでした");
        return ExitCode::FAILURE;
    }
    if received < WANTED_POSTS {
        println!("結果: 接続はできましたが、投稿が {WANTED_POSTS} 件そろいませんでした");
        return ExitCode::FAILURE;
    }
    println!("結果: OK");
    ExitCode::SUCCESS
}
