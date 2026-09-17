use crate::event::{parse_event, Event, SkipStats};
use futures_util::StreamExt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc::{error::TrySendError, Sender};
use tokio_tungstenite::tungstenite::Message;

#[derive(Debug, Default)]
pub struct SkipCounters {
    json_error: AtomicU64,
    non_commit: AtomicU64,
    other_collection: AtomicU64,
    unknown_operation: AtomicU64,
    missing_record: AtomicU64,
    loose_record: AtomicU64,
    binary_frame: AtomicU64,
}

impl SkipCounters {
    pub fn add(&self, delta: &SkipStats) {
        add_to(&self.json_error, delta.json_error);
        add_to(&self.non_commit, delta.non_commit);
        add_to(&self.other_collection, delta.other_collection);
        add_to(&self.unknown_operation, delta.unknown_operation);
        add_to(&self.missing_record, delta.missing_record);
        add_to(&self.loose_record, delta.loose_record);
        add_to(&self.binary_frame, delta.binary_frame);
    }

    pub fn snapshot(&self) -> SkipStats {
        SkipStats {
            json_error: self.json_error.load(Ordering::Relaxed),
            non_commit: self.non_commit.load(Ordering::Relaxed),
            other_collection: self.other_collection.load(Ordering::Relaxed),
            unknown_operation: self.unknown_operation.load(Ordering::Relaxed),
            missing_record: self.missing_record.load(Ordering::Relaxed),
            loose_record: self.loose_record.load(Ordering::Relaxed),
            binary_frame: self.binary_frame.load(Ordering::Relaxed),
        }
    }
}

fn add_to(counter: &AtomicU64, delta: u64) {
    if delta > 0 {
        counter.fetch_add(delta, Ordering::Relaxed);
    }
}

#[derive(Debug)]
pub enum SessionEnd {
    ConnectFailed(String),
    ServerClosed { code: Option<u16>, reason: String },
    StreamEnded,
    IdleTimeout { secs: u64 },
    WebSocketError(String),
    ConsumerGone,
}

#[derive(Debug)]
pub struct SessionOutcome {
    pub end: SessionEnd,
    pub sent: u64,
    pub skips: SkipStats,
    pub first_time_us: Option<i64>,
    pub last_time_us: Option<i64>,
    pub send_blocked_ms: u64,
}

pub fn build_subscribe_url(
    endpoint: &str,
    collections: &[String],
    cursor_us: Option<i64>,
) -> String {
    let mut url = endpoint.to_string();
    let mut separator = if endpoint.contains('?') { '&' } else { '?' };

    for collection in collections {
        url.push(separator);
        url.push_str("wantedCollections=");
        url.push_str(collection);
        separator = '&';
    }

    url.push(separator);
    url.push_str("compress=false");

    if let Some(cursor) = cursor_us {
        url.push_str("&cursor=");
        url.push_str(&cursor.to_string());
    }

    url
}

pub async fn run_session(
    url: String,
    tx: Sender<Event>,
    idle_timeout: Duration,
    counters: Arc<SkipCounters>,
) -> SessionOutcome {
    let mut skips = SkipStats::default();
    let mut sent = 0u64;
    let mut first_time_us = None;
    let mut last_time_us = None;
    let mut send_blocked = Duration::ZERO;

    let mut socket = match tokio_tungstenite::connect_async(&url).await {
        Ok((socket, _)) => socket,
        Err(e) => {
            return SessionOutcome {
                end: SessionEnd::ConnectFailed(e.to_string()),
                sent,
                skips,
                first_time_us,
                last_time_us,
                send_blocked_ms: 0,
            }
        }
    };

    let end = loop {
        let message = match tokio::time::timeout(idle_timeout, socket.next()).await {
            Err(_) => {
                break SessionEnd::IdleTimeout {
                    secs: idle_timeout.as_secs(),
                }
            }
            Ok(None) => break SessionEnd::StreamEnded,
            Ok(Some(Err(e))) => break SessionEnd::WebSocketError(e.to_string()),
            Ok(Some(Ok(message))) => message,
        };

        let json = match message {
            Message::Text(json) => json,
            Message::Binary(_) => {
                let delta = SkipStats {
                    binary_frame: 1,
                    ..SkipStats::default()
                };
                skips.merge(&delta);
                counters.add(&delta);
                continue;
            }
            Message::Close(frame) => {
                break SessionEnd::ServerClosed {
                    code: frame.as_ref().map(|f| u16::from(f.code)),
                    reason: frame.map(|f| f.reason.to_string()).unwrap_or_default(),
                }
            }
            Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => continue,
        };

        let mut delta = SkipStats::default();
        let parsed = parse_event(&json, &mut delta);
        skips.merge(&delta);
        counters.add(&delta);

        let event = match parsed {
            Ok(Some(event)) => event,
            Ok(None) => continue,
            Err(e) => {
                tracing::debug!("Skipped unreadable Jetstream message: {}", e);
                continue;
            }
        };

        let time_us = event.time_us();
        match tx.try_send(event) {
            Ok(()) => {}
            Err(TrySendError::Full(event)) => {
                let blocked_at = Instant::now();
                if tx.send(event).await.is_err() {
                    break SessionEnd::ConsumerGone;
                }
                send_blocked += blocked_at.elapsed();
            }
            Err(TrySendError::Closed(_)) => break SessionEnd::ConsumerGone,
        }

        sent += 1;
        first_time_us.get_or_insert(time_us);
        last_time_us = Some(time_us);
    };

    SessionOutcome {
        end,
        sent,
        skips,
        first_time_us,
        last_time_us,
        send_blocked_ms: send_blocked.as_millis() as u64,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::POST_COLLECTION;
    use crate::local_jetstream::{
        poison_account_json, poison_identity_json, post_json, LocalJetstream, Script,
    };
    use tokio::sync::mpsc;

    fn collections() -> Vec<String> {
        vec![POST_COLLECTION.to_string()]
    }

    fn text_of(event: &Event) -> String {
        match event {
            Event::Post(post) => post.record.text.clone(),
            Event::PostDelete(_) => String::new(),
        }
    }

    async fn drain(rx: &mut mpsc::Receiver<Event>) -> Vec<String> {
        let mut texts = Vec::new();
        while let Some(event) = rx.recv().await {
            texts.push(text_of(&event));
        }
        texts
    }

    /// 接続 URL に投稿コレクションと compress=false が入ること
    #[test]
    fn test_build_subscribe_url_without_cursor() {
        assert_eq!(
            build_subscribe_url("wss://example.test/subscribe", &collections(), None),
            "wss://example.test/subscribe?wantedCollections=app.bsky.feed.post&compress=false"
        );
    }

    /// カーソルを指定したとき、接続 URL に cursor が入ること
    #[test]
    fn test_build_subscribe_url_with_cursor() {
        assert_eq!(
            build_subscribe_url("wss://example.test/subscribe", &collections(), Some(1700000000000000)),
            "wss://example.test/subscribe?wantedCollections=app.bsky.feed.post&compress=false&cursor=1700000000000000"
        );
    }

    /// 接続先に既にクエリがあるとき、それを壊さずに追記すること
    #[test]
    fn test_build_subscribe_url_keeps_existing_query() {
        assert_eq!(
            build_subscribe_url("wss://example.test/subscribe?foo=1", &collections(), None),
            "wss://example.test/subscribe?foo=1&wantedCollections=app.bsky.feed.post&compress=false"
        );
    }

    /// コレクションを複数指定したとき、すべて並ぶこと
    #[test]
    fn test_build_subscribe_url_with_multiple_collections() {
        let collections = vec![
            "app.bsky.feed.post".to_string(),
            "app.bsky.feed.like".to_string(),
        ];

        assert_eq!(
            build_subscribe_url("wss://example.test/subscribe", &collections, None),
            "wss://example.test/subscribe?wantedCollections=app.bsky.feed.post&wantedCollections=app.bsky.feed.like&compress=false"
        );
    }

    /// 毒データや壊れた JSON を挟んでも接続が続き、後続の投稿が届くこと
    #[tokio::test]
    async fn test_session_survives_poison_payloads() {
        let server = LocalJetstream::start(vec![
            Script::Text(post_json("before", 1700000000000000)),
            Script::Text(poison_account_json()),
            Script::Text(poison_identity_json()),
            Script::Text("{\"did\":".to_string()),
            Script::Text(post_json("after", 1700000000000003)),
            Script::Close(1000, "done".to_string()),
        ])
        .await;

        let (tx, mut rx) = mpsc::channel(16);
        let counters = Arc::new(SkipCounters::default());
        let outcome = run_session(server.url(), tx, Duration::from_secs(5), counters).await;
        let texts = drain(&mut rx).await;

        assert_eq!(texts, vec!["before".to_string(), "after".to_string()]);
        assert_eq!(outcome.sent, 2);
        assert_eq!(outcome.skips.non_commit, 2);
        assert_eq!(outcome.skips.json_error, 1);
        assert_eq!(outcome.first_time_us, Some(1700000000000000));
        assert_eq!(outcome.last_time_us, Some(1700000000000003));
    }

    /// サーバに閉じられたとき、コードと理由が残ること
    #[tokio::test]
    async fn test_session_reports_server_close() {
        let server =
            LocalJetstream::start(vec![Script::Close(1011, "slow consumer".to_string())]).await;

        let (tx, _rx) = mpsc::channel(16);
        let counters = Arc::new(SkipCounters::default());
        let outcome = run_session(server.url(), tx, Duration::from_secs(5), counters).await;

        match outcome.end {
            SessionEnd::ServerClosed { code, reason } => {
                assert_eq!(code, Some(1011));
                assert_eq!(reason, "slow consumer");
            }
            other => panic!("切断理由が残っていない: {:?}", other),
        }
    }

    /// 無通信が続いたときに切り、その理由が残ること
    #[tokio::test]
    async fn test_session_reports_idle_timeout() {
        let server = LocalJetstream::start(vec![Script::Sleep(Duration::from_secs(5))]).await;

        let (tx, _rx) = mpsc::channel(16);
        let counters = Arc::new(SkipCounters::default());
        let outcome = run_session(server.url(), tx, Duration::from_millis(200), counters).await;

        assert!(matches!(outcome.end, SessionEnd::IdleTimeout { .. }));
        assert_eq!(outcome.sent, 0);
    }

    /// サーバからの生存確認に応答すること
    #[tokio::test]
    async fn test_session_answers_ping_with_pong() {
        let server = LocalJetstream::start(vec![
            Script::Ping,
            Script::Sleep(Duration::from_millis(50)),
            Script::Text(post_json("after ping", 1700000000000000)),
            Script::Close(1000, String::new()),
        ])
        .await;

        let (tx, mut rx) = mpsc::channel(16);
        let counters = Arc::new(SkipCounters::default());
        let outcome = run_session(server.url(), tx, Duration::from_secs(5), counters).await;
        let texts = drain(&mut rx).await;

        assert_eq!(texts, vec!["after ping".to_string()]);
        assert_eq!(outcome.sent, 1);
        assert_eq!(server.pongs(), 1);
    }

    /// 実際の接続要求に、カーソルとコレクションが入っていること
    #[tokio::test]
    async fn test_session_reports_requested_url() {
        let server = LocalJetstream::start(vec![Script::Close(1000, String::new())]).await;
        let url = build_subscribe_url(&server.url(), &collections(), Some(1700000000000000));

        let (tx, _rx) = mpsc::channel(16);
        let counters = Arc::new(SkipCounters::default());
        run_session(url, tx, Duration::from_secs(5), counters).await;

        let requested = server.requested_uri().expect("接続要求が記録されていない");
        assert!(
            requested.contains("wantedCollections=app.bsky.feed.post"),
            "{requested}"
        );
        assert!(requested.contains("compress=false"), "{requested}");
        assert!(requested.contains("cursor=1700000000000000"), "{requested}");
    }

    /// 溜め場所が一杯でもイベントを捨てないこと（背圧）
    #[tokio::test]
    async fn test_session_does_not_drop_events_when_queue_is_full() {
        let mut script: Vec<Script> = (0..50)
            .map(|i| Script::Text(post_json(&format!("post{i}"), 1700000000000000 + i)))
            .collect();
        script.push(Script::Close(1000, String::new()));
        let server = LocalJetstream::start(script).await;

        let (tx, mut rx) = mpsc::channel(1);
        let counters = Arc::new(SkipCounters::default());
        let session = tokio::spawn(run_session(
            server.url(),
            tx,
            Duration::from_secs(5),
            counters,
        ));

        let mut texts = Vec::new();
        while let Some(event) = rx.recv().await {
            tokio::time::sleep(Duration::from_millis(1)).await;
            texts.push(text_of(&event));
        }
        let outcome = session.await.unwrap();

        assert_eq!(texts.len(), 50);
        assert_eq!(texts[49], "post49");
        assert_eq!(outcome.sent, 50);
    }

    /// 圧縮フレームが来ても数えるだけで、接続を続けること
    #[tokio::test]
    async fn test_session_counts_binary_frame_without_dying() {
        let server = LocalJetstream::start(vec![
            Script::Binary(vec![40, 181, 47, 253]),
            Script::Text(post_json("after binary", 1700000000000000)),
            Script::Close(1000, String::new()),
        ])
        .await;

        let (tx, mut rx) = mpsc::channel(16);
        let counters = Arc::new(SkipCounters::default());
        let outcome = run_session(server.url(), tx, Duration::from_secs(5), counters.clone()).await;
        let texts = drain(&mut rx).await;

        assert_eq!(texts, vec!["after binary".to_string()]);
        assert_eq!(outcome.skips.binary_frame, 1);
        assert_eq!(counters.snapshot().binary_frame, 1);
    }

    /// 接続できないとき、その理由が残ること
    #[tokio::test]
    async fn test_session_reports_connect_failure() {
        let (tx, _rx) = mpsc::channel(16);
        let counters = Arc::new(SkipCounters::default());

        let outcome = run_session(
            "ws://127.0.0.1:1/subscribe".to_string(),
            tx,
            Duration::from_secs(1),
            counters,
        )
        .await;

        assert!(matches!(outcome.end, SessionEnd::ConnectFailed(_)));
    }
}
