pub mod event;
pub mod ws;

#[cfg(test)]
mod local_jetstream;

pub use event::{Event, Operation, PostDelete, PostEvent, PostRecord, SkipStats, POST_COLLECTION};
pub use ws::{SessionEnd, SessionOutcome, SkipCounters};

use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

const BACKOFF_MIN_SECS: u64 = 5;
const BACKOFF_MAX_SECS: u64 = 300;
const SESSION_MIN_SUCCESS_EVENTS: u64 = 100;
const SESSION_MIN_SUCCESS_SECS: u64 = 10;
const ETA_MIN_LAG_SECS: f64 = 60.0;

#[derive(Debug, Clone)]
pub struct ConsumerConfig {
    pub endpoint: String,
    pub wanted_collections: Vec<String>,
    pub queue_capacity: usize,
    pub idle_timeout: Duration,
    pub replay_buffer_us: i64,
    pub cursor_save_interval: Duration,
    pub metrics_interval: Duration,
}

impl Default for ConsumerConfig {
    fn default() -> Self {
        ConsumerConfig {
            endpoint: String::new(),
            wanted_collections: vec![POST_COLLECTION.to_string()],
            queue_capacity: 2048,
            idle_timeout: Duration::from_secs(30),
            replay_buffer_us: 1_000_000,
            cursor_save_interval: Duration::from_secs(5),
            metrics_interval: Duration::from_secs(60),
        }
    }
}

impl ConsumerConfig {
    pub fn from_env() -> Self {
        ConsumerConfig {
            endpoint: std::env::var("JETSTREAM_URL")
                .expect("JETSTREAM_URL environment variable must be set"),
            ..ConsumerConfig::default()
        }
    }
}

fn next_backoff(current_secs: u64, was_successful: bool) -> u64 {
    if was_successful {
        BACKOFF_MIN_SECS
    } else {
        (current_secs * 2).min(BACKOFF_MAX_SECS)
    }
}

fn was_session_successful(processed: u64, session_secs: u64) -> bool {
    processed >= SESSION_MIN_SUCCESS_EVENTS
        || (processed > 0 && session_secs >= SESSION_MIN_SUCCESS_SECS)
}

fn reconnect_cursor_us(latest_us: i64, replay_buffer_us: i64) -> Option<i64> {
    if latest_us <= 0 {
        None
    } else {
        Some((latest_us - replay_buffer_us).max(1))
    }
}

fn catch_up_rate(first_us: i64, last_us: i64, elapsed: Duration) -> Option<f64> {
    let elapsed_us = elapsed.as_micros() as f64;
    if elapsed_us <= 0.0 || last_us <= first_us {
        return None;
    }
    Some((last_us - first_us) as f64 / elapsed_us)
}

fn lag_secs(last_time_us: Option<i64>, now_us: i64) -> f64 {
    last_time_us
        .map(|last| (now_us - last) as f64 / 1_000_000.0)
        .unwrap_or(0.0)
}

fn eta_secs(lag_secs: f64, catch_up_rate: Option<f64>) -> Option<f64> {
    match catch_up_rate {
        Some(rate) if rate > 1.0 && lag_secs >= ETA_MIN_LAG_SECS => Some(lag_secs / (rate - 1.0)),
        _ => None,
    }
}

struct Metrics {
    interval: Duration,
    window_start: Instant,
    processed: u64,
    first_time_us: Option<i64>,
    last_time_us: Option<i64>,
    reported_skips: SkipStats,
}

impl Metrics {
    fn new(interval: Duration) -> Self {
        Metrics {
            interval,
            window_start: Instant::now(),
            processed: 0,
            first_time_us: None,
            last_time_us: None,
            reported_skips: SkipStats::default(),
        }
    }

    fn record(&mut self, time_us: i64) {
        self.processed += 1;
        self.first_time_us.get_or_insert(time_us);
        self.last_time_us = Some(time_us);
    }

    fn maybe_report(&mut self, queue_len: usize, queue_capacity: usize, counters: &SkipCounters) {
        let elapsed = self.window_start.elapsed();
        if elapsed < self.interval {
            return;
        }

        let snapshot = counters.snapshot();
        let skips = snapshot.since(&self.reported_skips);
        let rate = self.processed as f64 / elapsed.as_secs_f64();
        let lag_secs = lag_secs(self.last_time_us, chrono::Utc::now().timestamp_micros());
        let rate_of_catch_up = match (self.first_time_us, self.last_time_us) {
            (Some(first), Some(last)) => catch_up_rate(first, last, elapsed),
            _ => None,
        };

        tracing::info!(
            "METRICS [{}s]: processed={} rate={:.1}/s queue={}/{} lag={:.1}s advance={} eta={} skip{{json={},non_commit={},other_coll={},unknown_op={},no_record={},loose={},binary={}}}",
            elapsed.as_secs(),
            self.processed,
            rate,
            queue_len,
            queue_capacity,
            lag_secs,
            rate_of_catch_up
                .map(|rate| format!("{:.1}x", rate))
                .unwrap_or_else(|| "-".to_string()),
            eta_secs(lag_secs, rate_of_catch_up)
                .map(|secs| format!("{:.1}m", secs / 60.0))
                .unwrap_or_else(|| "-".to_string()),
            skips.json_error,
            skips.non_commit,
            skips.other_collection,
            skips.unknown_operation,
            skips.missing_record,
            skips.loose_record,
            skips.binary_frame,
        );

        self.window_start = Instant::now();
        self.processed = 0;
        self.first_time_us = None;
        self.last_time_us = None;
        self.reported_skips = snapshot;
    }
}

pub async fn start_consumer<F, Fut>(
    realfakebluesky_db: sqlx::SqlitePool,
    config: ConsumerConfig,
    callback: F,
) where
    F: Fn(Event) -> Fut + Send + Sync + 'static + Clone,
    Fut: std::future::Future<Output = ()> + Send,
{
    let initial_cursor_us: Option<i64> =
        sqlx::query_scalar("SELECT cursor_us FROM jetstream_cursor WHERE id = 1")
            .fetch_optional(&realfakebluesky_db)
            .await
            .unwrap_or(None);
    tracing::info!(
        "Jetstream initial cursor from DB: {:?} us",
        initial_cursor_us
    );

    let latest_cursor = Arc::new(AtomicI64::new(initial_cursor_us.unwrap_or(0)));

    {
        let cursor_for_save = latest_cursor.clone();
        let pool_for_save = realfakebluesky_db.clone();
        let interval = config.cursor_save_interval;
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(interval).await;
                save_cursor(&pool_for_save, &cursor_for_save).await;
            }
        });
    }

    let counters = Arc::new(SkipCounters::default());
    let mut metrics = Metrics::new(config.metrics_interval);
    let mut backoff_secs = BACKOFF_MIN_SECS;

    loop {
        let cursor_us = reconnect_cursor_us(
            latest_cursor.load(Ordering::Relaxed),
            config.replay_buffer_us,
        );
        let url = ws::build_subscribe_url(&config.endpoint, &config.wanted_collections, cursor_us);
        tracing::info!("Starting Jetstream connection with cursor: {:?}", cursor_us);

        let (tx, mut rx) = mpsc::channel(config.queue_capacity);
        let session = tokio::spawn(ws::run_session(
            url,
            tx,
            config.idle_timeout,
            counters.clone(),
        ));

        let session_start = Instant::now();
        let mut processed = 0u64;
        while let Some(event) = rx.recv().await {
            let time_us = event.time_us();
            callback(event).await;
            latest_cursor.fetch_max(time_us, Ordering::Relaxed);
            processed += 1;
            metrics.record(time_us);
            metrics.maybe_report(rx.len(), config.queue_capacity, &counters);
        }

        let outcome = match session.await {
            Ok(outcome) => outcome,
            Err(e) => {
                tracing::error!("Jetstream reader task failed: {}", e);
                tokio::time::sleep(Duration::from_secs(backoff_secs)).await;
                continue;
            }
        };
        save_cursor(&realfakebluesky_db, &latest_cursor).await;

        let session_secs = session_start.elapsed().as_secs();
        let successful = was_session_successful(processed, session_secs);
        backoff_secs = next_backoff(backoff_secs, successful);

        tracing::warn!(
            "Jetstream disconnected: {:?}. processed={}, duration={}s, successful={}, blocked={}ms, skips={:?}. Reconnecting in {} seconds...",
            outcome.end,
            processed,
            session_secs,
            successful,
            outcome.send_blocked_ms,
            outcome.skips,
            backoff_secs
        );
        tokio::time::sleep(Duration::from_secs(backoff_secs)).await;
    }
}

async fn save_cursor(pool: &sqlx::SqlitePool, latest_cursor: &AtomicI64) {
    let cursor = latest_cursor.load(Ordering::Relaxed);
    if cursor <= 0 {
        return;
    }
    if let Err(e) =
        sqlx::query("INSERT OR REPLACE INTO jetstream_cursor (id, cursor_us) VALUES (1, ?)")
            .bind(cursor)
            .execute(pool)
            .await
    {
        tracing::warn!("Failed to save Jetstream cursor: {}", e);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local_jetstream::{post_json, LocalJetstream, Script};
    use sqlx::sqlite::SqlitePoolOptions;
    use sqlx::SqlitePool;

    async fn cursor_pool(initial_us: Option<i64>) -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS jetstream_cursor (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                cursor_us INTEGER NOT NULL
            );
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();
        if let Some(cursor) = initial_us {
            sqlx::query("INSERT INTO jetstream_cursor (id, cursor_us) VALUES (1, ?)")
                .bind(cursor)
                .execute(&pool)
                .await
                .unwrap();
        }
        pool
    }

    async fn stored_cursor(pool: &SqlitePool) -> Option<i64> {
        sqlx::query_scalar("SELECT cursor_us FROM jetstream_cursor WHERE id = 1")
            .fetch_optional(pool)
            .await
            .unwrap()
    }

    fn test_config(endpoint: String) -> ConsumerConfig {
        ConsumerConfig {
            endpoint,
            queue_capacity: 8,
            idle_timeout: Duration::from_millis(300),
            cursor_save_interval: Duration::from_millis(20),
            metrics_interval: Duration::from_millis(50),
            ..ConsumerConfig::default()
        }
    }

    /// 失敗した接続のあとは、待ち時間が2倍になること
    #[test]
    fn test_next_backoff_doubles_when_session_failed() {
        assert_eq!(next_backoff(5, false), 10);
        assert_eq!(next_backoff(10, false), 20);
        assert_eq!(next_backoff(160, false), 300);
    }

    /// 待ち時間が上限（300秒）を超えないこと
    #[test]
    fn test_next_backoff_capped_at_max() {
        assert_eq!(next_backoff(300, false), 300);
    }

    /// 成功した接続のあとは、待ち時間が最小に戻ること
    #[test]
    fn test_next_backoff_resets_after_successful_session() {
        assert_eq!(next_backoff(300, true), BACKOFF_MIN_SECS);
        assert_eq!(next_backoff(5, true), BACKOFF_MIN_SECS);
    }

    /// 短くてもたくさん処理した接続は、成功とみなすこと
    #[test]
    fn test_short_session_with_many_events_is_successful() {
        assert!(was_session_successful(SESSION_MIN_SUCCESS_EVENTS, 1));
        assert!(was_session_successful(20_000, 2));
    }

    /// 件数が少なくても長く続いた接続は、成功とみなすこと
    #[test]
    fn test_long_session_with_few_events_is_successful() {
        assert!(was_session_successful(1, SESSION_MIN_SUCCESS_SECS));
        assert!(was_session_successful(3, 3600));
    }

    /// 短くて件数も少ない接続は、失敗とみなすこと
    #[test]
    fn test_short_session_with_few_events_is_not_successful() {
        assert!(!was_session_successful(1, 1));
        assert!(!was_session_successful(SESSION_MIN_SUCCESS_EVENTS - 1, 9));
    }

    /// 1件も処理していない接続は、成功とみなさないこと
    #[test]
    fn test_session_without_events_is_never_successful() {
        assert!(!was_session_successful(0, 0));
        assert!(!was_session_successful(0, 3600));
    }

    /// 再接続時は、カーソルを巻き戻してから繋ぐこと
    #[test]
    fn test_reconnect_cursor_rewinds_by_replay_buffer() {
        assert_eq!(
            reconnect_cursor_us(1_700_000_000_000_000, 1_000_000),
            Some(1_699_999_999_000_000)
        );
    }

    /// カーソルが未取得のときは、指定なし（ライブ追従）で繋ぐこと
    #[test]
    fn test_reconnect_cursor_is_none_when_unknown() {
        assert_eq!(reconnect_cursor_us(0, 1_000_000), None);
        assert_eq!(reconnect_cursor_us(-1, 1_000_000), None);
    }

    /// 巻き戻してもカーソルが1を下回らないこと
    #[test]
    fn test_reconnect_cursor_never_goes_below_one() {
        assert_eq!(reconnect_cursor_us(10, 1_000_000), Some(1));
    }

    /// 追いつき倍率が「進んだ時間 ÷ 実時間」になること
    #[test]
    fn test_catch_up_rate_is_ratio_of_stream_time_to_real_time() {
        let rate = catch_up_rate(0, 10_000_000, Duration::from_secs(1)).unwrap();
        assert!((rate - 10.0).abs() < f64::EPSILON);

        assert_eq!(catch_up_rate(10, 10, Duration::from_secs(1)), None);
        assert_eq!(catch_up_rate(0, 10, Duration::ZERO), None);
    }

    /// 遅れが十分大きく、かつ追いついているときだけ、完了見込みを出すこと
    #[test]
    fn test_eta_is_only_known_while_catching_up() {
        assert_eq!(eta_secs(100.0, Some(11.0)), Some(10.0));
        assert_eq!(eta_secs(100.0, Some(1.0)), None);
        assert_eq!(eta_secs(100.0, None), None);
        assert_eq!(eta_secs(0.0, Some(10.0)), None);
        assert_eq!(eta_secs(0.1, Some(1.000001)), None);
        assert_eq!(eta_secs(59.9, Some(2.0)), None);
    }

    /// 遅れが「今の時刻 − 最後に処理したイベントの時刻」になること
    #[test]
    fn test_lag_is_distance_from_now_to_last_event() {
        let now = 1_700_000_000_000_000;

        assert_eq!(lag_secs(Some(now - 5_000_000), now), 5.0);
        assert_eq!(lag_secs(Some(now), now), 0.0);
        assert_eq!(lag_secs(None, now), 0.0);
    }

    /// 出力の間隔が来るまでは、集計を持ち越すこと
    #[test]
    fn test_metrics_keeps_counting_until_interval_passes() {
        let mut metrics = Metrics::new(Duration::from_secs(3600));
        let counters = SkipCounters::default();

        metrics.record(1_700_000_000_000_000);
        metrics.record(1_700_000_000_000_500);
        metrics.maybe_report(0, 2048, &counters);

        assert_eq!(metrics.processed, 2);
        assert_eq!(metrics.first_time_us, Some(1_700_000_000_000_000));
        assert_eq!(metrics.last_time_us, Some(1_700_000_000_000_500));
    }

    /// 出力したら集計をリセットすること
    #[test]
    fn test_metrics_resets_counting_after_report() {
        let mut metrics = Metrics::new(Duration::ZERO);
        let counters = SkipCounters::default();

        metrics.record(1_700_000_000_000_000);
        metrics.maybe_report(0, 2048, &counters);

        assert_eq!(metrics.processed, 0);
        assert_eq!(metrics.first_time_us, None);
        assert_eq!(metrics.last_time_us, None);
    }

    /// 読み飛ばし件数は、前回の出力からの差分で数えること
    #[test]
    fn test_metrics_reports_skips_since_last_report() {
        let mut metrics = Metrics::new(Duration::ZERO);
        let counters = SkipCounters::default();

        counters.add(&SkipStats {
            non_commit: 3,
            ..SkipStats::default()
        });
        metrics.maybe_report(0, 2048, &counters);
        assert_eq!(metrics.reported_skips.non_commit, 3);

        counters.add(&SkipStats {
            non_commit: 2,
            ..SkipStats::default()
        });
        let before = metrics.reported_skips;
        metrics.maybe_report(0, 2048, &counters);

        assert_eq!(counters.snapshot().since(&before).non_commit, 2);
        assert_eq!(metrics.reported_skips.non_commit, 5);
    }

    /// 処理した分だけカーソルが進み、DB に保存されること
    #[tokio::test]
    async fn test_start_consumer_advances_cursor_after_processing() {
        let server = LocalJetstream::start(vec![
            Script::Text(post_json("first", 1_700_000_000_000_000)),
            Script::Text(post_json("second", 1_700_000_000_000_500)),
            Script::Sleep(Duration::from_secs(5)),
        ])
        .await;
        let pool = cursor_pool(None).await;

        let processed = Arc::new(AtomicI64::new(0));
        let processed_for_callback = processed.clone();
        let consumer = tokio::spawn(start_consumer(
            pool.clone(),
            test_config(server.url()),
            move |_event| {
                let processed = processed_for_callback.clone();
                async move {
                    processed.fetch_add(1, Ordering::Relaxed);
                }
            },
        ));

        let mut stored = None;
        for _ in 0..100 {
            tokio::time::sleep(Duration::from_millis(20)).await;
            stored = stored_cursor(&pool).await;
            if stored == Some(1_700_000_000_000_500) {
                break;
            }
        }
        consumer.abort();

        assert_eq!(stored, Some(1_700_000_000_000_500));
        assert_eq!(processed.load(Ordering::Relaxed), 2);
    }

    /// 処理が終わるまでカーソルを進めないこと（途中で落ちても取りこぼさない）
    #[tokio::test]
    async fn test_start_consumer_holds_cursor_until_processing_finishes() {
        let server = LocalJetstream::start(vec![
            Script::Text(post_json("first", 1_700_000_000_000_000)),
            Script::Text(post_json("second", 1_700_000_000_000_500)),
            Script::Sleep(Duration::from_secs(5)),
        ])
        .await;
        let pool = cursor_pool(Some(1_000_000)).await;

        let gate = Arc::new(tokio::sync::Semaphore::new(0));
        let gate_for_callback = gate.clone();
        let consumer = tokio::spawn(start_consumer(
            pool.clone(),
            test_config(server.url()),
            move |_event| {
                let gate = gate_for_callback.clone();
                async move {
                    gate.acquire().await.unwrap().forget();
                }
            },
        ));

        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(stored_cursor(&pool).await, Some(1_000_000));

        gate.add_permits(2);
        let mut stored = None;
        for _ in 0..100 {
            tokio::time::sleep(Duration::from_millis(20)).await;
            stored = stored_cursor(&pool).await;
            if stored == Some(1_700_000_000_000_500) {
                break;
            }
        }
        consumer.abort();

        assert_eq!(stored, Some(1_700_000_000_000_500));
    }

    /// 接続に失敗しても、保存済みのカーソルを壊さないこと
    #[tokio::test]
    async fn test_start_consumer_keeps_cursor_when_connection_fails() {
        let pool = cursor_pool(Some(1_700_000_000_000_000)).await;
        let config = test_config("ws://127.0.0.1:1/subscribe".to_string());

        let consumer = tokio::spawn(start_consumer(pool.clone(), config, |_event| async {}));
        tokio::time::sleep(Duration::from_millis(200)).await;
        consumer.abort();

        assert_eq!(stored_cursor(&pool).await, Some(1_700_000_000_000_000));
    }

    /// DB に保存されたカーソルから遡って接続すること
    #[tokio::test]
    async fn test_start_consumer_reconnects_from_stored_cursor() {
        let server = LocalJetstream::start(vec![Script::Sleep(Duration::from_secs(5))]).await;
        let pool = cursor_pool(Some(1_700_000_000_000_000)).await;

        let consumer = tokio::spawn(start_consumer(
            pool.clone(),
            test_config(server.url()),
            |_event| async {},
        ));
        tokio::time::sleep(Duration::from_millis(200)).await;
        consumer.abort();

        let requested = server.requested_uri().expect("接続要求が記録されていない");
        assert!(requested.contains("cursor=1699999999000000"), "{requested}");
    }
}
