use bsky_core::{FeedItem, FeedSkeletonResult};
use jetstream::PostEvent;
use regex::Regex;
use sqlx::{Row, SqlitePool};
use std::sync::OnceLock;

static HELLO_REGEX: OnceLock<Regex> = OnceLock::new();

#[derive(Debug, Default, Clone)]
pub struct State {}

pub fn matches_hello_world(text: &str) -> bool {
    let regex = HELLO_REGEX.get_or_init(|| Regex::new(r"(?i)hello[,\s]*world").unwrap());
    regex.is_match(text)
}

pub async fn process_event(pool: &SqlitePool, post: &PostEvent) {
    if !matches_hello_world(&post.record.text) {
        return;
    }

    let post_uri = post.uri();
    tracing::info!("Found hello world post: {}", post_uri);

    let result = sqlx::query(
        "INSERT OR IGNORE INTO helloworld_posts (uri, cid, indexed_at) VALUES (?, ?, ?)",
    )
    .bind(&post_uri)
    .bind(&post.cid)
    .bind(post.indexed_at_us())
    .execute(pool)
    .await;

    if let Err(e) = result {
        tracing::error!("Failed to insert post: {}", e);
    }
}

pub async fn get_feed_skeleton(
    pool: &SqlitePool,
    cursor: Option<String>,
    limit: Option<usize>,
) -> FeedSkeletonResult {
    let limit = limit.unwrap_or(30).min(100);
    let mut feed = Vec::new();

    if cursor.is_none() {
        feed.push(FeedItem {
            post: "at://did:plc:tsvcmd72oxp47wtixs4qllyi/app.bsky.feed.post/3ldy6oad3vk27"
                .to_string(),
        });
        tracing::info!("Added pinned post to feed (first page)");
    }

    let cursor_val = cursor
        .as_ref()
        .and_then(|c| c.parse::<i64>().ok())
        .unwrap_or(i64::MAX);

    let db_limit = if cursor.is_none() {
        (limit - 1).max(1)
    } else {
        limit
    };

    let rows_result = sqlx::query(
        "SELECT uri, indexed_at FROM helloworld_posts WHERE indexed_at < ? ORDER BY indexed_at DESC LIMIT ?"
    )
    .bind(cursor_val)
    .bind(db_limit as i64)
    .fetch_all(pool)
    .await;

    let mut next_cursor = None;

    match rows_result {
        Ok(rows) => {
            if let Some(last) = rows.last() {
                let last_ts: i64 = last.get("indexed_at");
                next_cursor = Some(last_ts.to_string());
            }

            for row in rows {
                let uri: String = row.get("uri");
                feed.push(FeedItem { post: uri });
            }
        }
        Err(e) => {
            tracing::error!("Failed to fetch feed: {}", e);
        }
    }

    tracing::info!(
        "Returning feed with {} items (cursor: {:?})",
        feed.len(),
        next_cursor
    );

    FeedSkeletonResult {
        cursor: next_cursor,
        feed,
    }
}

pub async fn migrate(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS helloworld_posts (
            uri TEXT PRIMARY KEY,
            cid TEXT NOT NULL,
            indexed_at INTEGER NOT NULL
        );
        "#,
    )
    .execute(pool)
    .await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// helloworld の正規表現が意図通りにマッチするか検証
    #[test]
    fn test_matches_hello_world() {
        assert!(matches_hello_world("Hello world"));
        assert!(matches_hello_world("HelloWorld"));
        assert!(matches_hello_world("hello, world"));
        assert!(matches_hello_world("HELLO WORLD"));
        assert!(matches_hello_world("hello  world"));

        assert!(!matches_hello_world("Hello everyone in the world"));
        assert!(!matches_hello_world("world hello"));
        assert!(!matches_hello_world("hello"));
        assert!(!matches_hello_world("world"));
    }

    /// マイグレーションが `helloworld_posts` テーブルを正しく作成するか検証
    #[tokio::test]
    async fn test_migrate_creates_table() {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();

        migrate(&pool).await.unwrap();

        let result = sqlx::query(
            "SELECT name FROM sqlite_master WHERE type='table' AND name='helloworld_posts'",
        )
        .fetch_one(&pool)
        .await;

        assert!(result.is_ok());
    }

    /// マイグレーションが冪等性を持つか検証（複数回実行してもエラーにならない）
    #[tokio::test]
    async fn test_migrate_is_idempotent() {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();

        migrate(&pool).await.unwrap();
        let result = migrate(&pool).await;

        assert!(result.is_ok());
    }

    use serde_json::json;

    const DID: &str = "did:plc:z72i7hdynmk6r22z27h6tvur";
    const CID: &str = "bafyreibvjvcv745gig4mvqs4hctx4zfkono4rjejm2ta6gtyzkqxfjeily";
    const RKEY: &str = "3l3temxelsm2a";
    const TIME_US: i64 = 1_700_000_000_000_000;

    fn post_event(operation: &str, text: &str, created_at: &str) -> PostEvent {
        let json = json!({
            "did": DID,
            "time_us": TIME_US,
            "kind": "commit",
            "commit": {
                "operation": operation,
                "rev": RKEY,
                "rkey": RKEY,
                "collection": "app.bsky.feed.post",
                "cid": CID,
                "record": {
                    "$type": "app.bsky.feed.post",
                    "text": text,
                    "createdAt": created_at,
                },
            },
        })
        .to_string();
        let mut skips = jetstream::SkipStats::default();
        match jetstream::event::parse_event(&json, &mut skips) {
            Ok(Some(jetstream::Event::Post(post))) => *post,
            other => panic!("投稿として読めなかった: {}", other.is_ok()),
        }
    }

    async fn migrated_pool() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        migrate(&pool).await.unwrap();
        pool
    }

    async fn stored_rows(pool: &SqlitePool) -> Vec<(String, String, i64)> {
        sqlx::query("SELECT uri, cid, indexed_at FROM helloworld_posts")
            .fetch_all(pool)
            .await
            .unwrap()
            .iter()
            .map(|row| (row.get("uri"), row.get("cid"), row.get("indexed_at")))
            .collect()
    }

    const PINNED: &str = "at://did:plc:tsvcmd72oxp47wtixs4qllyi/app.bsky.feed.post/3ldy6oad3vk27";

    async fn pool_with_posts(count: i64) -> SqlitePool {
        let pool = migrated_pool().await;
        for i in 0..count {
            sqlx::query("INSERT INTO helloworld_posts (uri, cid, indexed_at) VALUES (?, ?, ?)")
                .bind(format!("at://did:example/app.bsky.feed.post/{i}"))
                .bind("cid")
                .bind(1_700_000_000_000_000 + i)
                .execute(&pool)
                .await
                .unwrap();
        }
        pool
    }

    /// 1ページ目の先頭に固定の投稿が入ること
    #[tokio::test]
    async fn test_get_feed_skeleton_pins_a_post_on_the_first_page() {
        let pool = pool_with_posts(3).await;

        let first = get_feed_skeleton(&pool, None, Some(10)).await;
        let next = get_feed_skeleton(&pool, first.cursor.clone(), Some(10)).await;

        assert_eq!(first.feed[0].post, PINNED);
        assert!(next.feed.iter().all(|item| item.post != PINNED));
    }

    /// 投稿が新しい順に並ぶこと
    #[tokio::test]
    async fn test_get_feed_skeleton_returns_newest_first() {
        let pool = pool_with_posts(3).await;

        let result = get_feed_skeleton(&pool, None, Some(10)).await;

        assert_eq!(result.feed[1].post, "at://did:example/app.bsky.feed.post/2");
        assert_eq!(result.feed[2].post, "at://did:example/app.bsky.feed.post/1");
        assert_eq!(result.feed[3].post, "at://did:example/app.bsky.feed.post/0");
    }

    /// 1ページ目は固定の投稿のぶんだけ取得件数を減らすこと
    #[tokio::test]
    async fn test_get_feed_skeleton_reserves_a_slot_for_the_pinned_post() {
        let pool = pool_with_posts(10).await;

        let result = get_feed_skeleton(&pool, None, Some(3)).await;

        assert_eq!(result.feed.len(), 3);
        assert_eq!(result.feed[0].post, PINNED);
    }

    /// 2ページ目以降は、カーソルより古い投稿だけを返すこと
    #[tokio::test]
    async fn test_get_feed_skeleton_continues_from_cursor() {
        let pool = pool_with_posts(5).await;

        let first = get_feed_skeleton(&pool, None, Some(3)).await;
        let second = get_feed_skeleton(&pool, first.cursor.clone(), Some(3)).await;

        assert_eq!(first.cursor, Some(1_700_000_000_000_003_i64.to_string()));
        assert_eq!(second.feed[0].post, "at://did:example/app.bsky.feed.post/2");
        assert_eq!(second.feed.len(), 3);
    }

    /// 取得件数の指定がなければ 30 件、大きすぎる指定は 100 件で頭打ちにすること
    #[tokio::test]
    async fn test_get_feed_skeleton_caps_the_limit() {
        let pool = pool_with_posts(120).await;

        let default_limit = get_feed_skeleton(&pool, None, None).await;
        let too_large = get_feed_skeleton(&pool, None, Some(1000)).await;

        assert_eq!(default_limit.feed.len(), 30);
        assert_eq!(too_large.feed.len(), 100);
    }

    /// マッチする投稿が uri / cid / 投稿時刻 とともに 1 行だけ保存されること
    #[tokio::test]
    async fn test_process_event_stores_matching_post() {
        let pool = migrated_pool().await;

        process_event(
            &pool,
            &post_event("create", "hello world", "2023-11-14T22:13:19.000Z"),
        )
        .await;

        assert_eq!(
            stored_rows(&pool).await,
            vec![(
                format!("at://{DID}/app.bsky.feed.post/{RKEY}"),
                CID.to_string(),
                1_699_999_999_000_000
            )]
        );
    }

    /// 投稿日時が読めない投稿でも捨てられず、受信時刻で保存されること
    #[tokio::test]
    async fn test_process_event_falls_back_to_received_time() {
        let pool = migrated_pool().await;

        process_event(&pool, &post_event("create", "hello world", "")).await;

        let rows = stored_rows(&pool).await;
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].2, TIME_US);
    }

    /// 投稿日時が未来の投稿は、受信時刻で頭打ちにされること
    #[tokio::test]
    async fn test_process_event_caps_future_created_at() {
        let pool = migrated_pool().await;

        process_event(
            &pool,
            &post_event("create", "hello world", "2099-01-01T00:00:00.000Z"),
        )
        .await;

        let rows = stored_rows(&pool).await;
        assert_eq!(rows[0].2, TIME_US);
    }

    /// マッチしない投稿は保存されないこと
    #[tokio::test]
    async fn test_process_event_ignores_non_matching_post() {
        let pool = migrated_pool().await;

        process_event(
            &pool,
            &post_event("create", "goodbye world", "2026-01-01T00:00:00.000Z"),
        )
        .await;

        assert!(stored_rows(&pool).await.is_empty());
    }

    /// 同じ投稿が作成・更新として2回届いても 1 行のままであること
    #[tokio::test]
    async fn test_process_event_keeps_single_row_for_updated_post() {
        let pool = migrated_pool().await;

        process_event(
            &pool,
            &post_event("create", "hello world", "2023-11-14T22:13:19.000Z"),
        )
        .await;
        process_event(
            &pool,
            &post_event("update", "hello world again", "2023-11-14T22:13:19.000Z"),
        )
        .await;

        assert_eq!(stored_rows(&pool).await.len(), 1);
    }
}
