use atrium_api::record::KnownRecord;
use bsky_core::{FeedItem, FeedSkeletonResult};
use jetstream_oxide::events::commit::CommitEvent;
use regex::Regex;
use sqlx::{Row, SqlitePool};
use std::sync::OnceLock;

static HELLO_REGEX: OnceLock<Regex> = OnceLock::new();

#[derive(Debug, Default, Clone)]
pub struct State {}

/// "Hello World" パターンにマッチするかを判定
///
/// 正規表現: `(?i)hello[,\s]*world`
/// - 大文字小文字を区別しない
/// - "hello" と "world" の間にカンマやスペースが0個以上あることを許容
pub fn matches_hello_world(text: &str) -> bool {
    let regex = HELLO_REGEX.get_or_init(|| Regex::new(r"(?i)hello[,\s]*world").unwrap());
    regex.is_match(text)
}

pub async fn process_event(pool: &SqlitePool, event: &CommitEvent) {
    if let CommitEvent::Create { info, commit } = event {
        if let KnownRecord::AppBskyFeedPost(post) = &commit.record {
            let collection = commit.info.collection.as_str();
            if collection != "app.bsky.feed.post" {
                return;
            }

            let text = &post.text;

            if matches_hello_world(text) {
                let rkey = commit.info.rkey.as_str();
                let did = info.did.as_str();
                let post_uri = format!("at://{}/{}/{}", did, collection, rkey);
                tracing::info!("Found hello world post: {}", post_uri);

                let indexed_at = chrono::Utc::now().timestamp_micros();
                let cid = commit.cid.as_ref().to_string();

                let result = sqlx::query(
                    "INSERT OR IGNORE INTO helloworld_posts (uri, cid, indexed_at) VALUES (?, ?, ?)"
                )
                .bind(&post_uri)
                .bind(cid)
                .bind(indexed_at)
                .execute(pool)
                .await;

                if let Err(e) = result {
                    tracing::error!("Failed to insert post: {}", e);
                }
            }
        }
    }
}

pub async fn get_feed_skeleton(
    pool: &SqlitePool,
    cursor: Option<String>,
    limit: Option<usize>,
) -> FeedSkeletonResult {
    let limit = limit.unwrap_or(30).min(100);
    let mut feed = Vec::new();

    // Fixed pinned post at the top for first page
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

    // Adjust limit to account for pinned post on first page
    let db_limit = if cursor.is_none() {
        (limit - 1).max(1) // Reserve 1 slot for pinned post
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
        // Should match
        assert!(matches_hello_world("Hello world"));
        assert!(matches_hello_world("HelloWorld"));
        assert!(matches_hello_world("hello, world"));
        assert!(matches_hello_world("HELLO WORLD"));
        assert!(matches_hello_world("hello  world"));

        // Should NOT match
        assert!(!matches_hello_world("Hello everyone in the world"));
        assert!(!matches_hello_world("world hello"));
        assert!(!matches_hello_world("hello"));
        assert!(!matches_hello_world("world"));
    }

    /// マイグレーションが `helloworld_posts` テーブルを正しく作成するか検証
    #[tokio::test]
    async fn test_migrate_creates_table() {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();

        // Run migration
        migrate(&pool).await.unwrap();

        // Verify table exists by querying it
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

        // Run migration twice
        migrate(&pool).await.unwrap();
        let result = migrate(&pool).await;

        // Should not error on second run
        assert!(result.is_ok());
    }
}
