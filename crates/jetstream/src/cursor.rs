use sqlx::SqlitePool;

pub async fn migrate_cursor(pool: &SqlitePool) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS jetstream_cursor (
            id        INTEGER PRIMARY KEY CHECK (id = 1),
            cursor_us INTEGER NOT NULL
        );
        "#,
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn adopt_legacy_cursor(pool: &SqlitePool, legacy: &SqlitePool) -> anyhow::Result<()> {
    let legacy_exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'jetstream_cursor')",
    )
    .fetch_one(legacy)
    .await?;
    if !legacy_exists {
        return Ok(());
    }

    let current = read_cursor(pool).await?;
    if current.is_none() {
        if let Some(legacy_cursor) = read_cursor(legacy).await? {
            sqlx::query("INSERT INTO jetstream_cursor (id, cursor_us) VALUES (1, ?)")
                .bind(legacy_cursor)
                .execute(pool)
                .await?;
            if read_cursor(pool).await? != Some(legacy_cursor) {
                anyhow::bail!("Failed to copy legacy Jetstream cursor");
            }
            tracing::info!("Copied legacy Jetstream cursor: {} us", legacy_cursor);
        }
    }

    sqlx::query("DROP TABLE jetstream_cursor")
        .execute(legacy)
        .await?;
    tracing::info!("Dropped legacy Jetstream cursor table");
    Ok(())
}

async fn read_cursor(pool: &SqlitePool) -> anyhow::Result<Option<i64>> {
    Ok(
        sqlx::query_scalar("SELECT cursor_us FROM jetstream_cursor WHERE id = 1")
            .fetch_optional(pool)
            .await?,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn memory_pool() -> SqlitePool {
        SqlitePool::connect("sqlite::memory:").await.unwrap()
    }

    async fn pool_with_cursor(cursor_us: Option<i64>) -> SqlitePool {
        let pool = memory_pool().await;
        migrate_cursor(&pool).await.unwrap();
        if let Some(cursor_us) = cursor_us {
            sqlx::query("INSERT INTO jetstream_cursor (id, cursor_us) VALUES (1, ?)")
                .bind(cursor_us)
                .execute(&pool)
                .await
                .unwrap();
        }
        pool
    }

    async fn has_cursor_table(pool: &SqlitePool) -> bool {
        sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'jetstream_cursor')",
        )
        .fetch_one(pool)
        .await
        .unwrap()
    }

    /// カーソルの表の準備を何度実行しても、エラーにならず中身も消えないこと
    #[tokio::test]
    async fn test_migrate_cursor_is_idempotent() {
        let pool = pool_with_cursor(Some(100)).await;

        migrate_cursor(&pool).await.unwrap();

        assert_eq!(read_cursor(&pool).await.unwrap(), Some(100));
    }

    /// 古い置き場所にカーソルの表が無ければ、何もしないこと
    #[tokio::test]
    async fn test_adopt_does_nothing_without_legacy_table() {
        let pool = pool_with_cursor(None).await;
        let legacy = memory_pool().await;

        adopt_legacy_cursor(&pool, &legacy).await.unwrap();

        assert_eq!(read_cursor(&pool).await.unwrap(), None);
    }

    /// 新しい置き場所にカーソルが無ければ、古いカーソルを写してから古い表を消すこと
    #[tokio::test]
    async fn test_adopt_copies_legacy_cursor_then_drops_legacy_table() {
        let pool = pool_with_cursor(None).await;
        let legacy = pool_with_cursor(Some(1_700_000_000_000_000)).await;

        adopt_legacy_cursor(&pool, &legacy).await.unwrap();

        assert_eq!(
            read_cursor(&pool).await.unwrap(),
            Some(1_700_000_000_000_000)
        );
        assert!(!has_cursor_table(&legacy).await);
    }

    /// 新しい置き場所にすでにカーソルがあれば、上書きせずに古い表だけを消すこと（写した後、消す前に止まった場合の続き）
    #[tokio::test]
    async fn test_adopt_keeps_existing_cursor_and_drops_legacy_table() {
        let pool = pool_with_cursor(Some(200)).await;
        let legacy = pool_with_cursor(Some(100)).await;

        adopt_legacy_cursor(&pool, &legacy).await.unwrap();

        assert_eq!(read_cursor(&pool).await.unwrap(), Some(200));
        assert!(!has_cursor_table(&legacy).await);
    }

    /// 古いカーソルの表が空なら、何も写さずに古い表を消すこと
    #[tokio::test]
    async fn test_adopt_drops_empty_legacy_table() {
        let pool = pool_with_cursor(None).await;
        let legacy = pool_with_cursor(None).await;

        adopt_legacy_cursor(&pool, &legacy).await.unwrap();

        assert_eq!(read_cursor(&pool).await.unwrap(), None);
        assert!(!has_cursor_table(&legacy).await);
    }
}
