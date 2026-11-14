use std::collections::HashSet;

use anyhow::Result;
use sqlx::SqlitePool;

pub const MANUAL: &[&str] = &[
    "did:plc:7kwaqrvmj3coyl5qma7tgbgu",
    "did:plc:5jenymjq7jxaacntwd7k6nmc",
    "did:plc:e5vdlxh2naki2mn7g24t3xaw",
    "did:plc:lyioa2sqsrt27bcm7hwi3nvv",
    "did:plc:hvmjyycxkahfeu7hmllys4op",
    "did:plc:45etljjzylj5fjlenwcllmqy",
    "did:plc:shdnclooxu6a4cdl6hf27ysd",
    "did:plc:rqjdydkzh4em43kff4yvs7qz",
];

const SOURCE_MANUAL: &str = "manual";

pub async fn sync_manual(
    pool: &SqlitePool,
    manual: &[&str],
    now_us: i64,
) -> Result<HashSet<String>> {
    let mut tx = pool.begin().await?;

    let listed: Vec<String> =
        sqlx::query_scalar("SELECT did FROM silentviews_users WHERE source = ?")
            .bind(SOURCE_MANUAL)
            .fetch_all(&mut *tx)
            .await?;
    for did in listed.iter().filter(|d| !manual.contains(&d.as_str())) {
        sqlx::query(
            "UPDATE silentviews_users SET blacklisted_at = NULL, source = NULL WHERE did = ?",
        )
        .bind(did)
        .execute(&mut *tx)
        .await?;
    }

    for did in manual {
        sqlx::query(
            r#"
            INSERT INTO silentviews_users (did, blacklisted_at, source) VALUES (?, ?, ?)
            ON CONFLICT(did) DO UPDATE SET
                source = CASE WHEN blacklisted_at IS NULL THEN excluded.source ELSE source END,
                blacklisted_at = COALESCE(blacklisted_at, excluded.blacklisted_at)
            "#,
        )
        .bind(did)
        .bind(now_us)
        .bind(SOURCE_MANUAL)
        .execute(&mut *tx)
        .await?;
    }

    let blacklisted: Vec<String> =
        sqlx::query_scalar("SELECT did FROM silentviews_users WHERE blacklisted_at IS NOT NULL")
            .fetch_all(&mut *tx)
            .await?;
    for did in &blacklisted {
        let prefix = format!("at://{did}/");
        sqlx::query("DELETE FROM silentviews_posts WHERE substr(uri, 1, ?) = ?")
            .bind(prefix.len() as i64)
            .bind(&prefix)
            .execute(&mut *tx)
            .await?;
    }

    tx.commit().await?;
    Ok(blacklisted.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn pool() -> SqlitePool {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        crate::migrate(&pool).await.unwrap();
        pool
    }

    async fn rows(pool: &SqlitePool) -> Vec<(String, Option<i64>, Option<String>)> {
        sqlx::query_as("SELECT did, blacklisted_at, source FROM silentviews_users ORDER BY did")
            .fetch_all(pool)
            .await
            .unwrap()
    }

    async fn post(pool: &SqlitePool, uri: &str) {
        sqlx::query("INSERT INTO silentviews_posts (uri, cid, indexed_at) VALUES (?, 'cid', 1)")
            .bind(uri)
            .execute(pool)
            .await
            .unwrap();
    }

    async fn uris(pool: &SqlitePool) -> Vec<String> {
        sqlx::query_scalar("SELECT uri FROM silentviews_posts ORDER BY uri")
            .fetch_all(pool)
            .await
            .unwrap()
    }

    /// 手で入れる一覧の人を、手で入れた人としてブラックリストに入れ、その一覧を返すこと
    #[tokio::test]
    async fn test_manual_list_is_blacklisted() {
        let pool = pool().await;

        let got = sync_manual(&pool, &["did:plc:a", "did:plc:b"], 100)
            .await
            .unwrap();

        assert_eq!(got, HashSet::from(["did:plc:a".into(), "did:plc:b".into()]));
        assert_eq!(
            rows(&pool).await,
            vec![
                ("did:plc:a".into(), Some(100), Some("manual".into())),
                ("did:plc:b".into(), Some(100), Some("manual".into())),
            ]
        );
    }

    /// 起動し直しても、ブラックリストに入った時刻は最初のままにすること
    #[tokio::test]
    async fn test_resync_keeps_first_time() {
        let pool = pool().await;
        sync_manual(&pool, &["did:plc:a"], 100).await.unwrap();

        sync_manual(&pool, &["did:plc:a"], 200).await.unwrap();

        assert_eq!(
            rows(&pool).await,
            vec![("did:plc:a".into(), Some(100), Some("manual".into()))]
        );
    }

    /// 一覧から消えた手で入れた人はブラックリストから外し、自動で入った人には触らないこと
    #[tokio::test]
    async fn test_removed_from_list_is_released_but_auto_is_kept() {
        let pool = pool().await;
        sync_manual(&pool, &["did:plc:a"], 100).await.unwrap();
        sqlx::query("INSERT INTO silentviews_users (did, flags, blacklisted_at, source) VALUES ('did:plc:ai', 2, 50, 'ai')")
            .execute(&pool)
            .await
            .unwrap();

        let got = sync_manual(&pool, &[], 200).await.unwrap();

        assert_eq!(got, HashSet::from(["did:plc:ai".into()]));
        assert_eq!(
            rows(&pool).await,
            vec![
                ("did:plc:a".into(), None, None),
                ("did:plc:ai".into(), Some(50), Some("ai".into())),
            ]
        );
    }

    /// 自動で入った人を一覧に足しても、自動で入った人のままにすること（一覧から消したときに外れてしまわないため）
    #[tokio::test]
    async fn test_listing_auto_user_keeps_auto_source() {
        let pool = pool().await;
        sqlx::query("INSERT INTO silentviews_users (did, flags, blacklisted_at, source) VALUES ('did:plc:ai', 2, 50, 'ai')")
            .execute(&pool)
            .await
            .unwrap();

        sync_manual(&pool, &["did:plc:ai"], 100).await.unwrap();

        assert_eq!(
            rows(&pool).await,
            vec![("did:plc:ai".into(), Some(50), Some("ai".into()))]
        );
    }

    /// ブラックリストの人の掲載済み投稿だけを消し、ID の頭が同じ別の人の投稿は残すこと
    #[tokio::test]
    async fn test_blacklisted_users_posts_are_removed() {
        let pool = pool().await;
        post(&pool, "at://did:plc:a/app.bsky.feed.post/1").await;
        post(&pool, "at://did:plc:a/app.bsky.feed.post/2").await;
        post(&pool, "at://did:plc:ab/app.bsky.feed.post/1").await;
        post(&pool, "at://did:plc:b/app.bsky.feed.post/1").await;

        sync_manual(&pool, &["did:plc:a"], 100).await.unwrap();

        assert_eq!(
            uris(&pool).await,
            vec![
                "at://did:plc:ab/app.bsky.feed.post/1",
                "at://did:plc:b/app.bsky.feed.post/1"
            ]
        );
    }
}
