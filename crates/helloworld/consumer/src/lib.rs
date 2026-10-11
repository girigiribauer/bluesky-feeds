use jetstream::{App, PostEvent};
use regex::Regex;
use sqlx::SqlitePool;
use std::sync::OnceLock;

static HELLO_REGEX: OnceLock<Regex> = OnceLock::new();

fn matches_hello_world(text: &str) -> bool {
    let regex = HELLO_REGEX.get_or_init(|| Regex::new(r"(?i)hello[,\s]*world").unwrap());
    regex.is_match(text)
}

pub struct HelloWorld {
    pool: SqlitePool,
}

impl HelloWorld {
    pub fn new(pool: SqlitePool) -> Self {
        HelloWorld { pool }
    }
}

#[async_trait::async_trait]
impl App for HelloWorld {
    fn name(&self) -> &'static str {
        "helloworld"
    }

    async fn migrate(&self) -> anyhow::Result<()> {
        migrate(&self.pool).await?;
        Ok(())
    }

    fn sieve(&self, post: &PostEvent) -> bool {
        matches_hello_world(&post.record.text)
    }

    async fn process(&self, post: &PostEvent) {
        let post_uri = post.uri();
        tracing::info!("Found hello world post: {}", post_uri);

        let result = sqlx::query(
            "INSERT OR IGNORE INTO helloworld_posts (uri, cid, indexed_at) VALUES (?, ?, ?)",
        )
        .bind(&post_uri)
        .bind(&post.cid)
        .bind(post.indexed_at_us())
        .execute(&self.pool)
        .await;

        if let Err(e) = result {
            tracing::error!("Failed to insert post: {}", e);
        }
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
    use sqlx::Row;

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

    async fn migrated_app() -> HelloWorld {
        let app = HelloWorld::new(SqlitePool::connect("sqlite::memory:").await.unwrap());
        app.migrate().await.unwrap();
        app
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

    /// ふるいは、本文に hello world を含む投稿だけを通すこと
    #[tokio::test]
    async fn test_sieve_passes_only_hello_world_posts() {
        let app = migrated_app().await;

        assert!(app.sieve(&post_event(
            "create",
            "hello world",
            "2026-01-01T00:00:00.000Z"
        )));
        assert!(!app.sieve(&post_event(
            "create",
            "goodbye world",
            "2026-01-01T00:00:00.000Z"
        )));
    }

    /// 本処理で、投稿が uri / cid / 投稿時刻 とともに 1 行だけ保存されること
    #[tokio::test]
    async fn test_process_stores_post() {
        let app = migrated_app().await;

        app.process(&post_event(
            "create",
            "hello world",
            "2023-11-14T22:13:19.000Z",
        ))
        .await;

        assert_eq!(
            stored_rows(&app.pool).await,
            vec![(
                format!("at://{DID}/app.bsky.feed.post/{RKEY}"),
                CID.to_string(),
                1_699_999_999_000_000
            )]
        );
    }

    /// 投稿日時が読めない投稿でも捨てられず、受信時刻で保存されること
    #[tokio::test]
    async fn test_process_falls_back_to_received_time() {
        let app = migrated_app().await;

        app.process(&post_event("create", "hello world", "")).await;

        let rows = stored_rows(&app.pool).await;
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].2, TIME_US);
    }

    /// 投稿日時が未来の投稿は、受信時刻で頭打ちにされること
    #[tokio::test]
    async fn test_process_caps_future_created_at() {
        let app = migrated_app().await;

        app.process(&post_event(
            "create",
            "hello world",
            "2099-01-01T00:00:00.000Z",
        ))
        .await;

        let rows = stored_rows(&app.pool).await;
        assert_eq!(rows[0].2, TIME_US);
    }

    /// 同じ投稿が作成・更新として2回届いても 1 行のままであること
    #[tokio::test]
    async fn test_process_keeps_single_row_for_updated_post() {
        let app = migrated_app().await;

        app.process(&post_event(
            "create",
            "hello world",
            "2023-11-14T22:13:19.000Z",
        ))
        .await;
        app.process(&post_event(
            "update",
            "hello world again",
            "2023-11-14T22:13:19.000Z",
        ))
        .await;

        assert_eq!(stored_rows(&app.pool).await.len(), 1);
    }
}
