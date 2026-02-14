use sqlx::{Error, Row, SqlitePool};

pub async fn migrate(pool: &SqlitePool) -> Result<(), Error> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS private_list_members (
            user_did TEXT NOT NULL,
            target_did TEXT NOT NULL,
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
            PRIMARY KEY (user_did, target_did)
        );
        CREATE INDEX IF NOT EXISTS idx_private_list_members_user ON private_list_members(user_did);

        CREATE TABLE IF NOT EXISTS private_list_post_cache (
            uri TEXT PRIMARY KEY,
            cid TEXT NOT NULL,
            author_did TEXT NOT NULL,
            indexed_at INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_private_list_post_cache_author ON private_list_post_cache(author_did);
        CREATE INDEX IF NOT EXISTS idx_private_list_post_cache_indexed_at ON private_list_post_cache(indexed_at DESC);
        "#,
    )
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn add_user(pool: &SqlitePool, user_did: &str, target_did: &str) -> Result<(), Error> {
    sqlx::query("INSERT OR IGNORE INTO private_list_members (user_did, target_did) VALUES (?, ?)")
        .bind(user_did)
        .bind(target_did)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn remove_user(pool: &SqlitePool, user_did: &str, target_did: &str) -> Result<(), Error> {
    sqlx::query("DELETE FROM private_list_members WHERE user_did = ? AND target_did = ?")
        .bind(user_did)
        .bind(target_did)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn list_users(pool: &SqlitePool, user_did: &str) -> Result<Vec<String>, Error> {
    let rows = sqlx::query(
        "SELECT target_did FROM private_list_members WHERE user_did = ? ORDER BY created_at DESC",
    )
    .bind(user_did)
    .fetch_all(pool)
    .await?;

    let mut users = Vec::new();
    for row in rows {
        users.push(row.try_get("target_did")?);
    }
    Ok(users)
}

pub async fn cache_post(
    pool: &SqlitePool,
    uri: &str,
    cid: &str,
    author_did: &str,
    indexed_at: i64,
) -> Result<(), Error> {
    sqlx::query(
        "INSERT OR REPLACE INTO private_list_post_cache (uri, cid, author_did, indexed_at) VALUES (?, ?, ?, ?)"
    )
    .bind(uri)
    .bind(cid)
    .bind(author_did)
    .bind(indexed_at)
    .execute(pool)
    .await?;
    Ok(())
}

pub struct CachedPost {
    pub uri: String,
    pub cid: String,
    pub author_did: String,
    pub indexed_at: i64,
}

pub async fn get_cached_posts(
    pool: &SqlitePool,
    authors: &[String],
    limit: usize,
    cursor: Option<i64>,
) -> Result<Vec<CachedPost>, Error> {
    if authors.is_empty() {
        return Ok(Vec::new());
    }

    // Build query with IN clause
    let placeholders = authors.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let sql = format!(
        "SELECT uri, cid, author_did, indexed_at FROM private_list_post_cache WHERE author_did IN ({}) AND indexed_at < ? ORDER BY indexed_at DESC LIMIT ?",
        placeholders
    );

    let mut query = sqlx::query(&sql);
    for author in authors {
        query = query.bind(author);
    }

    let cursor_val = cursor.unwrap_or(chrono::Utc::now().timestamp_micros());
    query = query.bind(cursor_val);
    query = query.bind(limit as i64);

    let rows = query.fetch_all(pool).await?;

    let mut posts = Vec::new();
    for row in rows {
        posts.push(CachedPost {
            uri: row.try_get("uri")?,
            cid: row.try_get("cid")?,
            author_did: row.try_get("author_did")?,
            indexed_at: row.try_get("indexed_at")?,
        });
    }
    Ok(posts)
}
