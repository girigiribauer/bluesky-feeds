use anyhow::Result;
use atrium_api::types::string::Nsid;
use chrono::{DateTime, Utc};
use jetstream_oxide::{
    events::{commit::CommitEvent, JetstreamEvent},
    JetstreamCompression, JetstreamConfig, JetstreamConnector,
};

const CURSOR_RESET_THRESHOLD_SECS: i64 = 300; // 5分以上古いカーソルは切り捨てる
const BACKOFF_MIN_SECS: u64 = 5;
const BACKOFF_MAX_SECS: u64 = 300;
// 短命な接続（ブリップ接続）でバックオフがリセットされないよう、
// この秒数以上続いたセッションのみ「成功」とみなす
const SESSION_MIN_SUCCESS_SECS: u64 = 10;

/// 与えられたカーソルの時刻と現在時刻を比較し、
/// 閾値（CURSOR_RESET_THRESHOLD_SECS）以上古ければ true を返す純粋な判定関数。
fn should_reset_cursor(cursor_dt: Option<DateTime<Utc>>, now: DateTime<Utc>) -> bool {
    if let Some(cursor) = cursor_dt {
        let diff_secs = now.timestamp() - cursor.timestamp();
        diff_secs >= CURSOR_RESET_THRESHOLD_SECS
    } else {
        false
    }
}

/// 次回の再接続待ち時間を計算する純粋な判定関数。
/// - セッションが成功（イベント受信あり かつ 十分な継続時間）の場合は最小値にリセット
/// - それ以外（即切断・短命接続など）の場合は現在値を2倍にして上限でクランプ
fn next_backoff(current_secs: u64, was_successful: bool) -> u64 {
    if was_successful {
        BACKOFF_MIN_SECS
    } else {
        (current_secs * 2).min(BACKOFF_MAX_SECS)
    }
}

/// セッションが「成功」とみなせるかを判定する純粋な判定関数。
/// イベントを受信しており、かつ SESSION_MIN_SUCCESS_SECS 以上継続したセッションを成功とみなす。
fn was_session_successful(had_events: bool, session_secs: u64) -> bool {
    had_events && session_secs >= SESSION_MIN_SUCCESS_SECS
}

pub async fn start_consumer<F, Fut>(realfakebluesky_db: sqlx::SqlitePool, callback: F)
where
    F: Fn(CommitEvent) -> Fut + Send + Sync + 'static + Clone,
    Fut: std::future::Future<Output = ()> + Send,
{
    // 起動時に DB からカーソルを読み込む（マイクロ秒 i64）
    let initial_cursor_us: Option<i64> =
        sqlx::query_scalar("SELECT cursor_us FROM jetstream_cursor WHERE id = 1")
            .fetch_optional(&realfakebluesky_db)
            .await
            .unwrap_or(None);
    tracing::info!(
        "Jetstream initial cursor from DB: {:?} us",
        initial_cursor_us
    );

    // コールバック内でカーソルを更新するための共有 AtomicI64（マイクロ秒）
    let latest_cursor = std::sync::Arc::new(std::sync::atomic::AtomicI64::new(
        initial_cursor_us.unwrap_or(0),
    ));

    // 5秒ごとにカーソルを DB に保存するタスク
    {
        let cursor_for_save = latest_cursor.clone();
        let pool_for_save = realfakebluesky_db.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                let cursor = cursor_for_save.load(std::sync::atomic::Ordering::Relaxed);
                if cursor > 0 {
                    if let Err(e) = sqlx::query(
                        "INSERT OR REPLACE INTO jetstream_cursor (id, cursor_us) VALUES (1, ?)",
                    )
                    .bind(cursor)
                    .execute(&pool_for_save)
                    .await
                    {
                        tracing::warn!("Failed to save Jetstream cursor: {}", e);
                    }
                }
            }
        });
    }

    // jetstreamの再接続ループ
    let recv_count = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
    let last_report = std::sync::Arc::new(std::sync::Mutex::new(std::time::Instant::now()));

    // セッション単位でイベント受信があったかを追跡するフラグ
    // recv_count はMETRICSで60秒ごとにリセットされるため、バックオフ判定には使えない
    let session_had_events = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));

    let mut backoff_secs = BACKOFF_MIN_SECS;

    loop {
        // セッション開始時にフラグをリセット
        session_had_events.store(false, std::sync::atomic::Ordering::Relaxed);

        // 再接続のたびに、その時点での最新カーソルを取得
        let current_cursor_us = latest_cursor.load(std::sync::atomic::Ordering::Relaxed);
        let mut cursor_dt = if current_cursor_us > 0 {
            DateTime::from_timestamp(
                current_cursor_us / 1_000_000,
                ((current_cursor_us % 1_000_000) * 1_000) as u32,
            )
        } else {
            None
        };

        // 足切り判定: カーソルが古すぎる場合は、諦めて最新から（Noneとして）再開する
        let now = Utc::now();
        if should_reset_cursor(cursor_dt, now) {
            tracing::warn!(
                "Cursor {:?} is too old (threshold: {}s). Resetting to Live Tail (None).",
                cursor_dt,
                CURSOR_RESET_THRESHOLD_SECS
            );
            cursor_dt = None;
        }

        tracing::info!("Starting Jetstream connection with cursor: {:?}", cursor_dt);

        let callback_clone = callback.clone();
        let recv_count_clone = recv_count.clone();
        let last_report_clone = last_report.clone();
        let latest_cursor_clone = latest_cursor.clone();
        let session_had_events_clone = session_had_events.clone();

        let session_start = std::time::Instant::now();
        let result = connect_and_run(cursor_dt, move |event| {
            let callback = callback_clone.clone();
            let recv_count = recv_count_clone.clone();
            let last_report = last_report_clone.clone();
            let latest_cursor = latest_cursor_clone.clone();
            let session_had_events = session_had_events_clone.clone();
            async move {
                // カーソル更新（イベントの time_us をそのまま保持）
                let time_us = match &event {
                    CommitEvent::Create { info, .. } => info.time_us,
                    CommitEvent::Delete { info, .. } => info.time_us,
                    CommitEvent::Update { info, .. } => info.time_us,
                };
                latest_cursor.store(time_us as i64, std::sync::atomic::Ordering::Relaxed);

                // セッション中にイベントを受信したことを記録
                session_had_events.store(true, std::sync::atomic::Ordering::Relaxed);

                // アプリ側のコールバックを実行
                callback(event).await;

                // 受信レートの集計（1分ごとにレポート）
                let count = recv_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                let mut last = last_report.lock().unwrap();
                let elapsed = last.elapsed();
                if elapsed >= std::time::Duration::from_secs(60) {
                    tracing::info!(
                        "METRICS [1min]: recv={} events, rate={:.1}/s",
                        count,
                        count as f64 / elapsed.as_secs_f64()
                    );
                    recv_count.store(0, std::sync::atomic::Ordering::Relaxed);
                    *last = std::time::Instant::now();
                }
            }
        })
        .await;

        let had_events = session_had_events.load(std::sync::atomic::Ordering::Relaxed);
        let session_secs = session_start.elapsed().as_secs();
        let successful = was_session_successful(had_events, session_secs);
        tracing::info!(
            "Session ended: had_events={}, duration={}s, successful={}",
            had_events,
            session_secs,
            successful
        );
        backoff_secs = next_backoff(backoff_secs, successful);

        tracing::warn!(
            "Jetstream disconnected: {:?}. Reconnecting in {} seconds...",
            result,
            backoff_secs
        );
        tokio::time::sleep(tokio::time::Duration::from_secs(backoff_secs)).await;
    }
}

async fn connect_and_run<F, Fut>(cursor: Option<DateTime<Utc>>, callback: F) -> Result<()>
where
    F: Fn(CommitEvent) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = ()> + Send,
{
    let endpoint_url =
        std::env::var("JETSTREAM_URL").expect("JETSTREAM_URL environment variable must be set");
    tracing::info!(
        "Connecting to Jetstream at {} (cursor: {:?})",
        endpoint_url,
        cursor
    );

    let config = JetstreamConfig {
        endpoint: endpoint_url,
        wanted_collections: vec![Nsid::new("app.bsky.feed.post".to_string()).unwrap()],
        wanted_dids: vec![],
        compression: JetstreamCompression::Zstd,
        cursor,
        base_delay_ms: 5000,
        max_delay_ms: 600000,
        max_retries: 0, // 内部リトライを完全に無効化し、自前でループ再接続する
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

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;
    use std::time::Duration;

    /// カーソルの初期読み込みと、最新のカーソルが定期的にDBへ保存されるかの検証
    #[tokio::test]
    async fn test_start_consumer_cursor_management() {
        // テスト環境では実際のJetstreamに繋がないようにダミーURLを設定
        std::env::set_var("JETSTREAM_URL", "ws://localhost:9999/dummy");

        let pool = SqlitePoolOptions::new()
            .connect("sqlite::memory:")
            .await
            .unwrap();

        // テスト用のテーブル作成
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

        // 初期カーソルを設定
        sqlx::query("INSERT INTO jetstream_cursor (id, cursor_us) VALUES (1, 1000000)")
            .execute(&pool)
            .await
            .unwrap();

        let pool_clone = pool.clone();

        // start_consumer は無限ループなので、バックグラウンドで起動
        let handle = tokio::spawn(async move {
            start_consumer(pool_clone, |_| async {}).await;
        });

        // テーブルの更新を待つために少し待機
        tokio::time::sleep(Duration::from_secs(6)).await;

        // カーソルが正しく上書き（あるいは維持）されていることを確認する
        let cursor_us: i64 =
            sqlx::query_scalar("SELECT cursor_us FROM jetstream_cursor WHERE id = 1")
                .fetch_one(&pool)
                .await
                .unwrap();

        // イベントを受信していないのでカーソルは更新されず、初期値のまま維持されていればOK
        assert_eq!(cursor_us, 1000000);

        handle.abort();
    }

    /// 観点1: カーソルが閾値（5分=300秒）以内の場合、そのままのカーソルが維持される（false）こと
    #[test]
    fn test_should_reset_cursor_within_threshold() {
        let now = Utc::now();
        // 4分前（240秒前）のカーソル
        let cursor_dt = Some(now - chrono::Duration::seconds(240));

        // 閾値以内なのでリセットすべきではない
        assert!(!should_reset_cursor(cursor_dt, now));
    }

    /// 観点2: カーソルが閾値（5分=300秒）以上古い場合、リセットされる（true）こと
    #[test]
    fn test_should_reset_cursor_too_old() {
        let now = Utc::now();
        // 6分前（360秒前）のカーソル
        let cursor_dt = Some(now - chrono::Duration::seconds(360));

        // 閾値を超えているのでリセットすべき
        assert!(should_reset_cursor(cursor_dt, now));
    }

    /// 観点3: イベントなしで切断した場合、待ち時間が2倍になること
    #[test]
    fn test_next_backoff_doubles_when_no_events() {
        assert_eq!(next_backoff(5, false), 10);
        assert_eq!(next_backoff(10, false), 20);
        assert_eq!(next_backoff(20, false), 40);
        assert_eq!(next_backoff(40, false), 80);
        assert_eq!(next_backoff(80, false), 160);
        assert_eq!(next_backoff(160, false), 300); // 320 → 上限300でクランプ
    }

    /// 観点4: 上限（300秒）に達したらそれ以上増えないこと
    #[test]
    fn test_next_backoff_capped_at_max() {
        assert_eq!(next_backoff(300, false), 300);
    }

    /// 観点5: 成功セッションの後はバックオフが最小値にリセットされること
    #[test]
    fn test_next_backoff_resets_after_successful_session() {
        assert_eq!(next_backoff(300, true), BACKOFF_MIN_SECS);
        assert_eq!(next_backoff(160, true), BACKOFF_MIN_SECS);
        assert_eq!(next_backoff(5, true), BACKOFF_MIN_SECS);
    }

    /// 観点6: イベントあり かつ 十分な継続時間の場合のみ成功とみなされること
    #[test]
    fn test_was_session_successful() {
        // イベントあり・十分な時間 → 成功
        assert!(was_session_successful(true, SESSION_MIN_SUCCESS_SECS));
        assert!(was_session_successful(true, SESSION_MIN_SUCCESS_SECS + 1));
        assert!(was_session_successful(true, 3600));

        // イベントあり・時間不足（ブリップ接続） → 失敗
        assert!(!was_session_successful(true, 0));
        assert!(!was_session_successful(true, 1));
        assert!(!was_session_successful(true, SESSION_MIN_SUCCESS_SECS - 1));

        // イベントなし → 時間に関わらず失敗
        assert!(!was_session_successful(false, 0));
        assert!(!was_session_successful(false, SESSION_MIN_SUCCESS_SECS));
        assert!(!was_session_successful(false, 3600));
    }
}
