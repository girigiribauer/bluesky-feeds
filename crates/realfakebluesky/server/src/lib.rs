use anyhow::{Context, Result};
use serde::Serialize;
use sqlx::SqlitePool;

#[derive(Debug, Serialize)]
pub struct FeedSkeleton {
    pub feed: Vec<FeedItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct FeedItem {
    pub post: String,
}

pub async fn get_fake_feed_skeleton(
    pool: &SqlitePool,
    limit: usize,
    cursor: Option<String>,
) -> Result<FeedSkeleton> {
    get_skeleton_from_table(pool, "fake_bluesky_posts", limit, cursor).await
}

pub async fn get_real_feed_skeleton(
    pool: &SqlitePool,
    limit: usize,
    cursor: Option<String>,
) -> Result<FeedSkeleton> {
    get_skeleton_from_table(pool, "real_bluesky_posts", limit, cursor).await
}

async fn get_skeleton_from_table(
    pool: &SqlitePool,
    table: &str,
    limit: usize,
    cursor: Option<String>,
) -> Result<FeedSkeleton> {
    let limit = limit.min(100);
    let indexed_at_cursor = cursor
        .as_ref()
        .and_then(|c| c.parse::<i64>().ok())
        .unwrap_or(i64::MAX);

    let query = format!(
        r#"
        SELECT uri, indexed_at
        FROM {}
        WHERE indexed_at < ?
        ORDER BY indexed_at DESC
        LIMIT ?
        "#,
        table
    );

    let rows = sqlx::query_as::<_, (String, i64)>(&query)
        .bind(indexed_at_cursor)
        .bind(limit as i64 + 1)
        .fetch_all(pool)
        .await
        .context(format!("Failed to fetch posts from {}", table))?;

    let has_more = rows.len() > limit;
    let posts: Vec<_> = rows.into_iter().take(limit).collect();

    let feed: Vec<FeedItem> = posts
        .iter()
        .map(|(uri, _)| FeedItem { post: uri.clone() })
        .collect();

    let cursor = if has_more {
        posts.last().map(|(_, indexed_at)| indexed_at.to_string())
    } else {
        None
    };

    Ok(FeedSkeleton { feed, cursor })
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn test_get_feed_skeleton_ordering_and_pagination() {
        use super::*;
        use sqlx::sqlite::SqlitePoolOptions;

        let pool = SqlitePoolOptions::new()
            .connect("sqlite::memory:")
            .await
            .unwrap();

        realfakebluesky_consumer::migrate(&pool).await.unwrap();

        sqlx::query("INSERT INTO fake_bluesky_posts (uri, cid, indexed_at) VALUES (?, ?, ?)")
            .bind("at://did:example:1/foo/1")
            .bind("cid1")
            .bind(1700000000000000_i64)
            .execute(&pool)
            .await
            .unwrap();

        sqlx::query("INSERT INTO fake_bluesky_posts (uri, cid, indexed_at) VALUES (?, ?, ?)")
            .bind("at://did:example:1/foo/2")
            .bind("cid2")
            .bind(1600000000000000_i64)
            .execute(&pool)
            .await
            .unwrap();

        sqlx::query("INSERT INTO fake_bluesky_posts (uri, cid, indexed_at) VALUES (?, ?, ?)")
            .bind("at://did:example:1/foo/3")
            .bind("cid3")
            .bind(1800000000000000_i64)
            .execute(&pool)
            .await
            .unwrap();

        let result1 = get_fake_feed_skeleton(&pool, 2, None).await.unwrap();
        assert_eq!(result1.feed.len(), 2);
        assert_eq!(result1.feed[0].post, "at://did:example:1/foo/3");
        assert_eq!(result1.feed[1].post, "at://did:example:1/foo/1");

        assert_eq!(result1.cursor, Some("1700000000000000".to_string()));

        let result2 = get_fake_feed_skeleton(&pool, 2, result1.cursor)
            .await
            .unwrap();
        assert_eq!(result2.feed.len(), 1);
        assert_eq!(result2.feed[0].post, "at://did:example:1/foo/2");

        assert_eq!(result2.cursor, None);
    }

    #[tokio::test]
    async fn test_get_real_feed_skeleton() {
        use super::*;
        use sqlx::sqlite::SqlitePoolOptions;

        let pool = SqlitePoolOptions::new()
            .connect("sqlite::memory:")
            .await
            .unwrap();
        realfakebluesky_consumer::migrate(&pool).await.unwrap();

        sqlx::query("INSERT INTO real_bluesky_posts (uri, cid, indexed_at) VALUES (?, ?, ?)")
            .bind("at://did:example:1/foo/real1")
            .bind("cid_real1")
            .bind(1900000000000000_i64)
            .execute(&pool)
            .await
            .unwrap();

        let result = get_real_feed_skeleton(&pool, 10, None).await.unwrap();
        assert_eq!(result.feed.len(), 1);
        assert_eq!(result.feed[0].post, "at://did:example:1/foo/real1");
    }
}
