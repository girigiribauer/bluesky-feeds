//! SQLite によるキャッシュ管理モジュール
//!
//! テーブル: `cache`
//!   - key        : TEXT PRIMARY KEY
//!   - value      : TEXT NOT NULL       (JSON)
//!   - expires_at : INTEGER NOT NULL    (UNIX タイムスタンプ秒)
//!
//! - 失効判定: SELECT 時に `expires_at > 現在時刻` を条件に付与（古いデータは透過的に無視）
//! - 物理削除: cleanup() を非同期で呼び出してゴミを掃除

use anyhow::{Context, Result};
use chrono::{DateTime, FixedOffset, Timelike, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};

// ---------------------------------------------------------------------------
// DB マイグレーション
// ---------------------------------------------------------------------------

/// `oneyearago.db` に必要なテーブルを作成する（冪等）
pub async fn migrate(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS cache (
            key        TEXT    PRIMARY KEY,
            value      TEXT    NOT NULL,
            expires_at INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_cache_expires_at ON cache(expires_at);
        "#,
    )
    .execute(pool)
    .await?;

    Ok(())
}

// ---------------------------------------------------------------------------
// 保存データの型定義
// ---------------------------------------------------------------------------

/// タイムゾーンキャッシュの JSON 構造
/// key: `tz:{did}`
#[derive(Serialize, Deserialize)]
pub struct TimezoneCacheValue {
    /// UTC からのオフセット秒 (例: JST = 32400)
    pub offset: i32,
}

/// フィード結果キャッシュの JSON 構造
/// key: `fn:{did}:{yymmdd}:{limit}:{cursor_hash}`
#[derive(Serialize, Deserialize)]
pub struct FeedCacheValue {
    /// 投稿の AT-URI リスト
    pub uris: Vec<String>,
    /// 次ページのカーソル文字列（最終ページなら None）
    pub next: Option<String>,
}

// ---------------------------------------------------------------------------
// CacheStore: 基本的な get / set / cleanup
// ---------------------------------------------------------------------------

pub struct CacheStore {
    pool: SqlitePool,
}

impl CacheStore {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    // -----------------------------------------------------------------------
    // 内部ヘルパー
    // -----------------------------------------------------------------------

    /// 現在時刻より未来の `expires_at` を持つエントリを取得する
    async fn get_raw(&self, key: &str) -> Result<Option<String>> {
        let now = Utc::now().timestamp();
        let row = sqlx::query("SELECT value FROM cache WHERE key = ? AND expires_at > ?")
            .bind(key)
            .bind(now)
            .fetch_optional(&self.pool)
            .await
            .context("cache: get_raw query failed")?;

        Ok(row.map(|r| r.get::<String, _>(0)))
    }

    /// キャッシュエントリを upsert する
    async fn set_raw(&self, key: &str, value: &str, expires_at: DateTime<Utc>) -> Result<()> {
        sqlx::query("INSERT OR REPLACE INTO cache (key, value, expires_at) VALUES (?, ?, ?)")
            .bind(key)
            .bind(value)
            .bind(expires_at.timestamp())
            .execute(&self.pool)
            .await
            .context("cache: set_raw query failed")?;

        Ok(())
    }

    // -----------------------------------------------------------------------
    // タイムゾーンキャッシュ
    // -----------------------------------------------------------------------

    /// タイムゾーンのキャッシュを取得する
    pub async fn get_timezone(&self, did: &str) -> Result<Option<chrono::FixedOffset>> {
        let key = format!("tz:{}", did);
        let Some(raw) = self.get_raw(&key).await? else {
            return Ok(None);
        };
        let cached: TimezoneCacheValue =
            serde_json::from_str(&raw).context("cache: failed to parse timezone JSON")?;
        Ok(chrono::FixedOffset::east_opt(cached.offset))
    }

    /// タイムゾーンをキャッシュする (TTL: 24 時間)
    pub async fn set_timezone(&self, did: &str, offset: i32) -> Result<()> {
        let key = format!("tz:{}", did);
        let value = serde_json::to_string(&TimezoneCacheValue { offset })?;
        let expires_at = Utc::now() + chrono::Duration::hours(24);
        self.set_raw(&key, &value, expires_at).await
    }

    // -----------------------------------------------------------------------
    // フィード結果キャッシュ
    // -----------------------------------------------------------------------

    /// フィード結果のキャッシュキーを生成する
    ///
    /// カーソル文字列は長くなりうるため、SHA-256 の先頭 8 文字でハッシュ化する。
    fn feed_key(did: &str, date: &str, limit: usize, cursor: Option<&str>) -> String {
        let cursor_hash = match cursor {
            None => "none".to_string(),
            Some(c) => {
                // 簡易ハッシュ: FNV-1a 64bit で代替（外部依存なし）
                let mut hash: u64 = 14695981039346656037;
                for byte in c.bytes() {
                    hash ^= byte as u64;
                    hash = hash.wrapping_mul(1099511628211);
                }
                format!("{:016x}", hash)
            }
        };
        format!("fn:{}:{}:{}:{}", did, date, limit, cursor_hash)
    }

    /// フィード結果を取得する
    pub async fn get_feed(
        &self,
        did: &str,
        date: &str,
        limit: usize,
        cursor: Option<&str>,
    ) -> Result<Option<FeedCacheValue>> {
        let key = Self::feed_key(did, date, limit, cursor);
        let Some(raw) = self.get_raw(&key).await? else {
            return Ok(None);
        };
        let cached: FeedCacheValue =
            serde_json::from_str(&raw).context("cache: failed to parse feed JSON")?;
        Ok(Some(cached))
    }

    /// フィード結果をキャッシュする (TTL: 当日 UTC 23:59:59 まで)
    ///
    /// `day_end_utc` はキャッシュを無効化すべき UTCの期限（通常はユーザーのタイムゾーンでの当日終わり）。
    #[allow(clippy::too_many_arguments)]
    pub async fn set_feed(
        &self,
        did: &str,
        date: &str,
        limit: usize,
        cursor: Option<&str>,
        uris: Vec<String>,
        next: Option<String>,
        expires_at: DateTime<Utc>,
    ) -> Result<()> {
        let key = Self::feed_key(did, date, limit, cursor);
        let value = serde_json::to_string(&FeedCacheValue { uris, next })?;
        self.set_raw(&key, &value, expires_at).await
    }

    // -----------------------------------------------------------------------
    // クリーンアップ
    // -----------------------------------------------------------------------

    /// 期限切れエントリを物理削除する
    ///
    /// ユーザーのレスポンスを遅延させないよう、呼び出し元は `tokio::spawn` で非同期実行すること。
    /// 期限切れエントリを物理削除する
    pub async fn cleanup(&self) -> Result<u64> {
        self.cleanup_at(Utc::now()).await
    }

    /// 指定時刻（UTC）を基準に期限切れエントリを物理削除する
    ///
    /// 【実行条件】
    /// 1. JST 午前4時以降であること。
    /// 2. その日にまだクリーンアップが実行されていないこと（1日1回制限）。
    pub async fn cleanup_at(&self, now: chrono::DateTime<Utc>) -> Result<u64> {
        let jst_offset = FixedOffset::east_opt(9 * 3600).unwrap();
        let now_jst = now.with_timezone(&jst_offset);

        // 条件1: 4時前なら何もしない
        if now_jst.hour() < 4 {
            tracing::debug!(
                "[cache] Cleanup skipped: before 4am JST (current: {:02}:00)",
                now_jst.hour()
            );
            return Ok(0);
        }

        let today = now_jst.format("%y%m%d").to_string();
        let status_key = "internal:last_cleanup_date";

        // 条件2: 今日すでに実行済みならスキップ
        if let Some(last_date) = self.get_raw(status_key).await? {
            if last_date == today {
                tracing::debug!(
                    "[cache] Cleanup skipped: already executed today ({})",
                    today
                );
                return Ok(0);
            }
        }

        // 物理削除の実行
        let now_ts = now.timestamp();
        let result = sqlx::query("DELETE FROM cache WHERE expires_at <= ?")
            .bind(now_ts)
            .execute(&self.pool)
            .await
            .context("cache: cleanup query failed")?;

        let affected = result.rows_affected();

        // 実行済みフラグを更新（10年先まで消えないキーとして保存）
        let far_future = now + chrono::Duration::days(365 * 10);
        self.set_raw(status_key, &today, far_future).await?;

        Ok(affected)
    }
}

// ---------------------------------------------------------------------------
// テスト
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    async fn in_memory_store() -> CacheStore {
        let pool = SqlitePool::connect(":memory:").await.unwrap();
        migrate(&pool).await.unwrap();
        CacheStore::new(pool)
    }

    // -- タイムゾーン -------------------------------------------------------

    #[tokio::test]
    async fn test_timezone_hit() {
        let store = in_memory_store().await;
        store.set_timezone("did:plc:test", 32400).await.unwrap();

        let result = store.get_timezone("did:plc:test").await.unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap().local_minus_utc(), 32400);
    }

    #[tokio::test]
    async fn test_timezone_miss_expired() {
        let store = in_memory_store().await;

        // 期限切れのエントリを直接挿入する
        let past = Utc::now() - Duration::seconds(1);
        store
            .set_raw("tz:did:plc:expired", r#"{"offset":32400}"#, past)
            .await
            .unwrap();

        let result = store.get_timezone("did:plc:expired").await.unwrap();
        assert!(result.is_none(), "期限切れなので None を返すべき");
    }

    #[tokio::test]
    async fn test_timezone_miss_unknown() {
        let store = in_memory_store().await;
        let result = store.get_timezone("did:plc:unknown").await.unwrap();
        assert!(result.is_none());
    }

    // -- フィード結果 -------------------------------------------------------

    #[tokio::test]
    async fn test_feed_hit() {
        let store = in_memory_store().await;
        let expires_at = Utc::now() + Duration::hours(1);

        store
            .set_feed(
                "did:plc:test",
                "260220",
                30,
                None,
                vec!["at://test/post/1".to_string()],
                None,
                expires_at,
            )
            .await
            .unwrap();

        let result = store
            .get_feed("did:plc:test", "260220", 30, None)
            .await
            .unwrap();
        assert!(result.is_some());
        let val = result.unwrap();
        assert_eq!(val.uris, vec!["at://test/post/1"]);
        assert!(val.next.is_none());
    }

    #[tokio::test]
    async fn test_feed_miss_expired() {
        let store = in_memory_store().await;
        let past = Utc::now() - Duration::seconds(1);

        store
            .set_feed(
                "did:plc:test",
                "260220",
                30,
                None,
                vec!["at://test/post/1".to_string()],
                None,
                past,
            )
            .await
            .unwrap();

        let result = store
            .get_feed("did:plc:test", "260220", 30, None)
            .await
            .unwrap();
        assert!(result.is_none(), "期限切れなので None を返すべき");
    }

    #[tokio::test]
    async fn test_feed_key_separation_by_limit() {
        let store = in_memory_store().await;
        let expires_at = Utc::now() + Duration::hours(1);

        store
            .set_feed(
                "did:plc:test",
                "260220",
                30,
                None,
                vec!["at://a".to_string()],
                None,
                expires_at,
            )
            .await
            .unwrap();

        // limit が違うと別キャッシュとして扱われる
        let result = store
            .get_feed("did:plc:test", "260220", 10, None)
            .await
            .unwrap();
        assert!(
            result.is_none(),
            "limit が異なるので別キャッシュでヒットしないはず"
        );
    }

    #[tokio::test]
    async fn test_feed_key_separation_by_cursor() {
        let store = in_memory_store().await;
        let expires_at = Utc::now() + Duration::hours(1);

        store
            .set_feed(
                "did:plc:test",
                "260220",
                30,
                None,
                vec!["at://a".to_string()],
                Some("v1::1::cursor_abc".to_string()),
                expires_at,
            )
            .await
            .unwrap();

        // cursor が違う（2ページ目）とミスになる
        let result = store
            .get_feed("did:plc:test", "260220", 30, Some("v1::1::cursor_xyz"))
            .await
            .unwrap();
        assert!(
            result.is_none(),
            "cursor が異なるので別キャッシュでヒットしないはず"
        );
    }

    // -- クリーンアップ -----------------------------------------------------

    #[tokio::test]
    async fn test_cleanup_removes_expired() {
        let store = in_memory_store().await;

        let past = Utc::now() - Duration::seconds(1);
        let future = Utc::now() + Duration::hours(1);

        store
            .set_raw("expired_key", r#"{"offset":0}"#, past)
            .await
            .unwrap();
        store
            .set_raw("valid_key", r#"{"offset":0}"#, future)
            .await
            .unwrap();

        let deleted = store.cleanup().await.unwrap();
        assert_eq!(deleted, 1, "期限切れの1件だけ削除されるべき");

        // 有効なキーはまだ存在する
        let still_there = store.get_raw("valid_key").await.unwrap();
        assert!(still_there.is_some());
    }

    #[tokio::test]
    async fn test_cleanup_trigger_conditions() {
        use chrono::TimeZone;
        let store = in_memory_store().await;

        // 【準備】期限切れデータを1件用意（確実にテスト時刻より前の過去時刻にする）
        let past = Utc.with_ymd_and_hms(2000, 1, 1, 0, 0, 0).unwrap();
        store
            .set_raw("expired_key", r#"{"offset":0}"#, past)
            .await
            .unwrap();

        // 1. JST 午前3:00 -> 実行されない
        let t1 = Utc.with_ymd_and_hms(2026, 2, 21, 18, 0, 0).unwrap(); // 3:00 JST
        assert_eq!(
            store.cleanup_at(t1).await.unwrap(),
            0,
            "4時前は実行されないこと"
        );

        // 2. JST 午前4:00 (その日初めてのアクセス) -> 実行される
        let t2 = Utc.with_ymd_and_hms(2026, 2, 21, 19, 0, 0).unwrap(); // 4:00 JST
        assert_eq!(
            store.cleanup_at(t2).await.unwrap(),
            1,
            "4時以降の初回は実行されること"
        );

        // 3. JST 午前4:10 (同じ日の2回目) -> スキップされる
        let t3 = Utc.with_ymd_and_hms(2026, 2, 21, 19, 10, 0).unwrap(); // 4:10 JST
        assert_eq!(
            store.cleanup_at(t3).await.unwrap(),
            0,
            "同じ日の2回目以降は実行されないこと"
        );

        // 新たなゴミを1件用意
        let past = Utc.with_ymd_and_hms(2000, 1, 1, 0, 0, 0).unwrap();
        store
            .set_raw("expired_key2", r#"{"offset":0}"#, past)
            .await
            .unwrap();
        let t4 = Utc.with_ymd_and_hms(2026, 2, 22, 19, 0, 0).unwrap(); // 翌4:00 JST
        assert_eq!(
            store.cleanup_at(t4).await.unwrap(),
            1,
            "翌日になれば再び実行されること"
        );
    }
}
