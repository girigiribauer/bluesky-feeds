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

        CREATE TABLE IF NOT EXISTS privatelist_sessions (
            session_id TEXT PRIMARY KEY,
            did TEXT NOT NULL,
            access_token TEXT NOT NULL,
            refresh_token TEXT NOT NULL,
            dpop_private_key TEXT NOT NULL,
            expires_at INTEGER NOT NULL,
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP
        );
        CREATE INDEX IF NOT EXISTS idx_privatelist_sessions_did ON privatelist_sessions(did);
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

pub struct Session {
    pub session_id: String,
    pub did: String,
    pub access_token: String,
    pub refresh_token: String,
    pub dpop_private_key: String,
    pub expires_at: i64,
}

pub async fn create_session(pool: &SqlitePool, session: &Session) -> Result<(), Error> {
    sqlx::query(
        "INSERT INTO privatelist_sessions (session_id, did, access_token, refresh_token, dpop_private_key, expires_at) VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(&session.session_id)
    .bind(&session.did)
    .bind(&session.access_token)
    .bind(&session.refresh_token)
    .bind(&session.dpop_private_key)
    .bind(session.expires_at)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get_session(pool: &SqlitePool, session_id: &str) -> Result<Option<Session>, Error> {
    let row = sqlx::query("SELECT * FROM privatelist_sessions WHERE session_id = ?")
        .bind(session_id)
        .fetch_optional(pool)
        .await?;

    if let Some(row) = row {
        Ok(Some(Session {
            session_id: row.try_get("session_id")?,
            did: row.try_get("did")?,
            access_token: row.try_get("access_token")?,
            refresh_token: row.try_get("refresh_token")?,
            dpop_private_key: row.try_get("dpop_private_key")?,
            expires_at: row.try_get("expires_at")?,
        }))
    } else {
        Ok(None)
    }
}

pub async fn delete_session(pool: &SqlitePool, session_id: &str) -> Result<(), Error> {
    sqlx::query("DELETE FROM privatelist_sessions WHERE session_id = ?")
        .bind(session_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn update_session(pool: &SqlitePool, session: &Session) -> Result<(), Error> {
    sqlx::query(
        "UPDATE privatelist_sessions SET access_token = ?, refresh_token = ?, expires_at = ? WHERE session_id = ?",
    )
    .bind(&session.access_token)
    .bind(&session.refresh_token)
    .bind(session.expires_at)
    .bind(&session.session_id)
    .execute(pool)
    .await?;
    Ok(())
}
