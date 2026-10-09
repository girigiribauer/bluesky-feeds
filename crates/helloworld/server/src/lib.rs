use bsky_core::{FeedItem, FeedSkeletonResult};
use sqlx::{Row, SqlitePool};

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

#[cfg(test)]
mod tests {
    use super::*;

    const PINNED: &str = "at://did:plc:tsvcmd72oxp47wtixs4qllyi/app.bsky.feed.post/3ldy6oad3vk27";

    async fn pool_with_posts(count: i64) -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        helloworld_consumer::migrate(&pool).await.unwrap();
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
}
