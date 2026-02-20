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
use chrono::{DateTime, Utc};
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
    pub async fn cleanup(&self) -> Result<u64> {
        let now = Utc::now().timestamp();
        let result = sqlx::query("DELETE FROM cache WHERE expires_at <= ?")
            .bind(now)
            .execute(&self.pool)
            .await
            .context("cache: cleanup query failed")?;

        Ok(result.rows_affected())
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
}
