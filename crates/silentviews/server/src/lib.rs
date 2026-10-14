use anyhow::{Context, Result};
use sqlx::SqlitePool;

#[derive(Debug, PartialEq, Eq)]
pub struct FeedSkeleton {
    pub posts: Vec<String>,
    pub cursor: Option<String>,
}

pub async fn get_feed_skeleton(
    pool: &SqlitePool,
    limit: usize,
    cursor: Option<String>,
) -> Result<FeedSkeleton> {
    let limit = limit.min(100);
    let before = cursor
        .as_deref()
        .and_then(|c| c.parse::<i64>().ok())
        .unwrap_or(i64::MAX);

    let rows = sqlx::query_as::<_, (String, i64)>(
        r#"
        SELECT uri, indexed_at
        FROM silentviews_posts
        WHERE indexed_at < ?
        ORDER BY indexed_at DESC
        LIMIT ?
        "#,
    )
    .bind(before)
    .bind(limit as i64 + 1)
    .fetch_all(pool)
    .await
    .context("Failed to fetch posts from silentviews_posts")?;

    let has_more = rows.len() > limit;
    let rows: Vec<_> = rows.into_iter().take(limit).collect();
    let cursor = if has_more {
        rows.last().map(|(_, indexed_at)| indexed_at.to_string())
    } else {
        None
    };

    Ok(FeedSkeleton {
        posts: rows.into_iter().map(|(uri, _)| uri).collect(),
        cursor,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    async fn pool_with(rows: &[(&str, i64)]) -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .connect("sqlite::memory:")
            .await
            .unwrap();
        silentviews_consumer::migrate(&pool).await.unwrap();
        for (uri, indexed_at) in rows {
            sqlx::query(
                "INSERT INTO silentviews_posts (uri, cid, indexed_at) VALUES (?, 'cid', ?)",
            )
            .bind(uri)
            .bind(indexed_at)
            .execute(&pool)
            .await
            .unwrap();
        }
        pool
    }

    /// 新しい順に並べ、続きがあるときだけ次のページの目印を返すこと
    #[tokio::test]
    async fn test_feed_is_newest_first_with_cursor_for_next_page() {
        let pool = pool_with(&[("at://a/1", 100), ("at://a/2", 300), ("at://a/3", 200)]).await;

        let first = get_feed_skeleton(&pool, 2, None).await.unwrap();
        assert_eq!(first.posts, vec!["at://a/2", "at://a/3"]);
        assert_eq!(first.cursor, Some("200".to_string()));

        let second = get_feed_skeleton(&pool, 2, first.cursor).await.unwrap();
        assert_eq!(second.posts, vec!["at://a/1"]);
        assert_eq!(second.cursor, None);
    }

    /// 1回に返す件数は多く頼まれても100件までにすること
    #[tokio::test]
    async fn test_limit_is_capped_at_100() {
        let rows: Vec<(String, i64)> = (0..120).map(|i| (format!("at://a/{i}"), i)).collect();
        let rows: Vec<(&str, i64)> = rows.iter().map(|(u, i)| (u.as_str(), *i)).collect();
        let pool = pool_with(&rows).await;

        let page = get_feed_skeleton(&pool, 500, None).await.unwrap();

        assert_eq!(page.posts.len(), 100);
        assert!(page.cursor.is_some());
    }

    /// 読めない目印は無視して先頭から返すこと
    #[tokio::test]
    async fn test_broken_cursor_starts_from_newest() {
        let pool = pool_with(&[("at://a/1", 100)]).await;

        let page = get_feed_skeleton(&pool, 10, Some("きのう".to_string()))
            .await
            .unwrap();

        assert_eq!(page.posts, vec!["at://a/1"]);
    }

    /// 投稿が1件もないときは、空のフィードを次のページの目印なしで返すこと
    #[tokio::test]
    async fn test_empty_table_gives_empty_feed_without_cursor() {
        let pool = pool_with(&[]).await;

        let page = get_feed_skeleton(&pool, 30, None).await.unwrap();

        assert_eq!(
            page,
            FeedSkeleton {
                posts: vec![],
                cursor: None
            }
        );
    }
}
