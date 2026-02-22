pub mod image_analyzer;

use anyhow::{Context, Result};
use atrium_api::record::KnownRecord;
use image_analyzer::{is_blue_sky_image, BlueDetectionConfig};
use jetstream_oxide::events::commit::CommitEvent;
use regex::Regex;
use serde::Serialize;
use sqlx::SqlitePool;
use std::sync::{Arc, OnceLock};
use tokio::sync::Semaphore;

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

/// Run database migrations
pub async fn migrate(pool: &SqlitePool) -> Result<()> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS fake_bluesky_posts (
            uri TEXT PRIMARY KEY,
            cid TEXT NOT NULL,
            indexed_at INTEGER NOT NULL
        );
        "#,
    )
    .execute(pool)
    .await
    .context("Failed to create fake_bluesky_posts table")?;

    sqlx::query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_fake_bluesky_indexed_at
        ON fake_bluesky_posts(indexed_at DESC);
        "#,
    )
    .execute(pool)
    .await
    .context("Failed to create index")?;

    Ok(())
}

/// Process Jetstream event
///
/// 処理したイベントの `time_us`（マイクロ秒）を返す。
/// これをカーソルとして保存することで、再接続時のバックフィルに利用できる。
/// Create イベント以外の場合は `None` を返す。
pub async fn process_event(pool: &SqlitePool, event: &CommitEvent) -> Option<i64> {
    // Only process Create events
    if let CommitEvent::Create { info, commit } = event {
        let time_us = info.time_us as i64;

        // Only process posts
        if commit.info.collection.as_str() != "app.bsky.feed.post" {
            return Some(time_us);
        }

        // Extract post record
        let post = match &commit.record {
            KnownRecord::AppBskyFeedPost(post) => post,
            _ => return Some(time_us),
        };

        // Filter by text content
        // 1. Remove all whitespace
        // 2. Must start with "bluesky" (case-insensitive)
        // 3. Can be followed only by punctuation and emojis

        // Remove all whitespace
        let cleaned_text: String = post.text.chars().filter(|c| !c.is_whitespace()).collect();

        // Regex: (?i)^bluesky[\p{P}\p{S}]*$
        static BLUESKY_REGEX: OnceLock<Regex> = OnceLock::new();
        let regex =
            BLUESKY_REGEX.get_or_init(|| Regex::new(r"(?i)^bluesky[\p{P}\p{S}]*$").unwrap());

        if !regex.is_match(&cleaned_text) {
            return Some(time_us);
        }

        // Extract post data
        let did = info.did.as_str();
        let rkey = commit.info.rkey.as_str();
        let collection = commit.info.collection.as_str();
        let uri = format!("at://{}/{}/{}", did, collection, rkey);
        let cid = commit.cid.as_ref().to_string();

        // If no images, skip
        let image_urls = match extract_image_urls(post, did) {
            Some(urls) if !urls.is_empty() => urls,
            _ => return Some(time_us),
        };

        // Check if post has blue sky images
        let has_blue_sky = has_blue_sky_images(&image_urls).await;

        // If any image is blue sky, exclude this post
        if has_blue_sky {
            tracing::debug!("Excluding post with blue sky image: {}", uri);
            return Some(time_us);
        }

        // Store in database
        // indexed_at にはイベントの元時刻（time_us）を秒単位に変換して使用する。
        // バックフィル時も元の投稿順序で表示される。
        let indexed_at = time_us / 1_000_000;
        match sqlx::query(
            r#"
            INSERT OR IGNORE INTO fake_bluesky_posts (uri, cid, indexed_at)
            VALUES (?, ?, ?)
            "#,
        )
        .bind(&uri)
        .bind(&cid)
        .bind(indexed_at)
        .execute(pool)
        .await
        {
            Ok(result) if result.rows_affected() > 0 => {
                tracing::info!("Stored fake bluesky post: {}", uri);
            }
            Ok(_) => {
                tracing::debug!("Skipped duplicate post: {}", uri);
            }
            Err(e) => {
                tracing::error!("Failed to store post: {}", e);
            }
        }

        Some(time_us)
    } else {
        None
    }
}

/// Get feed skeleton
pub async fn get_feed_skeleton(
    pool: &SqlitePool,
    limit: usize,
    cursor: Option<String>,
) -> Result<FeedSkeleton> {
    let limit = limit.min(100);
    let indexed_at_cursor = cursor
        .as_ref()
        .and_then(|c| c.parse::<i64>().ok())
        .unwrap_or(i64::MAX);

    let rows = sqlx::query_as::<_, (String, i64)>(
        r#"
        SELECT uri, indexed_at
        FROM fake_bluesky_posts
        WHERE indexed_at < ?
        ORDER BY indexed_at DESC
        LIMIT ?
        "#,
    )
    .bind(indexed_at_cursor)
    .bind(limit as i64 + 1)
    .fetch_all(pool)
    .await
    .context("Failed to fetch posts")?;

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

/// 投稿内の画像に青空が含まれているかチェック
///
/// # Arguments
/// * `image_urls` - 分析する画像URLのリスト
///
/// # Returns
/// * `true` - 1枚でも青空画像が含まれている
/// * `false` - 青空画像が含まれていない、またはエラーが発生した
async fn has_blue_sky_images(image_urls: &[String]) -> bool {
    let config = BlueDetectionConfig::default();
    let semaphore = Arc::new(Semaphore::new(2)); // Max 2 concurrent image analyses

    let mut tasks = Vec::new();
    for url in image_urls {
        let permit = semaphore.clone().acquire_owned().await.unwrap();
        let config = config.clone();
        let url = url.clone();

        let task = tokio::spawn(async move {
            let result = is_blue_sky_image(&url, &config).await;
            drop(permit);
            result
        });

        tasks.push(task);
    }

    // Wait for all analyses to complete
    for task in tasks {
        match task.await {
            Ok(Ok(is_blue)) => {
                if is_blue {
                    return true; // Found blue sky, no need to check other images
                }
            }
            Ok(Err(e)) => {
                tracing::debug!("Image analysis failed: {}", e);
                // On error, conservatively assume it's a blue sky (exclude post)
                return true;
            }
            Err(e) => {
                tracing::error!("Task join error: {}", e);
                // On error, conservatively assume it's a blue sky (exclude post)
                return true;
            }
        }
    }

    false
}

/// Extract image URLs from post record
fn extract_image_urls(
    post: &atrium_api::app::bsky::feed::post::Record,
    did: &str,
) -> Option<Vec<String>> {
    use atrium_api::types::{BlobRef, TypedBlobRef, Union};

    let embed = post.embed.as_ref()?;

    // Try to extract images from embed
    match embed {
        Union::Refs(
            atrium_api::app::bsky::feed::post::RecordEmbedRefs::AppBskyEmbedImagesMain(images),
        ) => {
            // Extract CIDs from blob refs and construct CDN URLs
            let urls: Vec<String> = images
                .images
                .iter()
                .map(|img| {
                    // BlobRef is an enum with Typed and Untyped variants
                    let cid = match &img.image {
                        BlobRef::Typed(TypedBlobRef::Blob(blob)) => {
                            // Typed blob has r#ref field with CidLink
                            // CidLink is a tuple struct wrapping Cid, access via .0
                            blob.r#ref.0.to_string()
                        }
                        BlobRef::Untyped(untyped) => {
                            // Untyped blob has cid field as String
                            untyped.cid.clone()
                        }
                    };

                    // Construct CDN URL
                    format!(
                        "https://cdn.bsky.app/img/feed_fullsize/plain/{}/{}@jpeg",
                        did, cid
                    )
                })
                .collect();

            if urls.is_empty() {
                None
            } else {
                tracing::debug!("Extracted {} image URLs for analysis", urls.len());
                Some(urls)
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bluesky_regex() {
        use regex::Regex;
        let regex = Regex::new(r"(?i)^bluesky[\p{P}\p{S}]*$").unwrap();

        // Helper to simulate whitespace removal
        let check = |text: &str| -> bool {
            let cleaned: String = text.chars().filter(|c| !c.is_whitespace()).collect();
            regex.is_match(&cleaned)
        };

        // Should match
        assert!(check("bluesky"));
        assert!(check("Bluesky"));
        assert!(check("BLUESKY"));
        assert!(check("blue sky")); // Becomes "bluesky"
        assert!(check("Blue \n Sky")); // Becomes "BlueSky"
        assert!(check("bluesky!"));
        assert!(check("  bluesky  ")); // Becomes "bluesky"
        assert!(check("bluesky✨"));
        assert!(check("bluesky!!!!"));
        assert!(check("bluesky🤗"));
        assert!(check("bluesky..."));

        // Should NOT match
        assert!(!check("blue-sky")); // Hyphen remains -> "blue-sky" (no match)
        assert!(!check("blue.sky")); // Dot remains -> "blue.sky" (no match)
        assert!(!check("I love bluesky"));
        assert!(!check("bluesky is great"));
        assert!(!check("hello bluesky world"));
    }

    /// カーソル保存テーブルのヘルパー（本番 main.rs と同じ SQL）
    async fn setup_cursor_table(pool: &SqlitePool) {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS jetstream_cursor (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                cursor_us INTEGER NOT NULL
            );
            "#,
        )
        .execute(pool)
        .await
        .expect("Failed to create jetstream_cursor table");
    }

    /// カーソルを書き込み、読み出す（本番 main.rs と同じ SQL）
    async fn save_cursor(pool: &SqlitePool, cursor_us: i64) {
        sqlx::query("INSERT OR REPLACE INTO jetstream_cursor (id, cursor_us) VALUES (1, ?)")
            .bind(cursor_us)
            .execute(pool)
            .await
            .expect("Failed to save cursor");
    }

    async fn load_cursor(pool: &SqlitePool) -> Option<i64> {
        sqlx::query_scalar("SELECT cursor_us FROM jetstream_cursor WHERE id = 1")
            .fetch_optional(pool)
            .await
            .unwrap_or(None)
    }

    #[tokio::test]
    async fn test_cursor_save_and_load() {
        // インメモリ SQLite DB を使う
        let pool = SqlitePool::connect(":memory:")
            .await
            .expect("Failed to connect to in-memory SQLite");

        setup_cursor_table(&pool).await;

        // 初期状態: カーソルは存在しない
        let cursor = load_cursor(&pool).await;
        assert!(cursor.is_none(), "カーソルは初期状態では None であるべき");

        // カーソルを書き込む
        let test_cursor: i64 = 1_740_000_000_000_000; // 代表的な time_us の値
        save_cursor(&pool, test_cursor).await;

        // 書き込んだ値が正しく読み出せる
        let loaded = load_cursor(&pool).await;
        assert_eq!(
            loaded,
            Some(test_cursor),
            "保存したカーソルが正しく読み出せるべき"
        );
    }

    #[tokio::test]
    async fn test_cursor_is_updated_to_latest() {
        let pool = SqlitePool::connect(":memory:")
            .await
            .expect("Failed to connect to in-memory SQLite");

        setup_cursor_table(&pool).await;

        let cursor_1: i64 = 1_740_000_000_000_000;
        let cursor_2: i64 = 1_740_000_000_100_000; // より新しいカーソル

        save_cursor(&pool, cursor_1).await;
        save_cursor(&pool, cursor_2).await;

        // 2回書き込んだ場合、最新の値に上書きされているべき
        let loaded = load_cursor(&pool).await;
        assert_eq!(
            loaded,
            Some(cursor_2),
            "カーソルは最新の値に更新されているべき"
        );

        // DB に行は1行だけ存在すること（id = 1 の制約通り）
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM jetstream_cursor")
            .fetch_one(&pool)
            .await
            .expect("Failed to count rows");
        assert_eq!(
            count, 1,
            "jetstream_cursor テーブルには常に1行だけ存在するべき"
        );
    }
}
