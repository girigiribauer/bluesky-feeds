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

#[derive(Debug, PartialEq, Eq)]
pub enum SkyStatus {
    AllBlue,
    AllFake,
    Mixed,
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
    .context("Failed to create fake index")?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS real_bluesky_posts (
            uri TEXT PRIMARY KEY,
            cid TEXT NOT NULL,
            indexed_at INTEGER NOT NULL
        );
        "#,
    )
    .execute(pool)
    .await
    .context("Failed to create real_bluesky_posts table")?;

    sqlx::query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_real_bluesky_indexed_at
        ON real_bluesky_posts(indexed_at DESC);
        "#,
    )
    .execute(pool)
    .await
    .context("Failed to create real index")?;

    // Jetstream カーソル永続化テーブル（常に1行のみ）
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS jetstream_cursor (
            id        INTEGER PRIMARY KEY CHECK (id = 1),
            cursor_us INTEGER NOT NULL
        );
        "#,
    )
    .execute(pool)
    .await
    .context("Failed to create jetstream_cursor table")?;

    // マイグレーション: 古い秒単位のデータ（10桁/11桁: < 10000000000）を新しいマイクロ秒単位（16桁）に変換する
    sqlx::query(
        r#"
        UPDATE fake_bluesky_posts
        SET indexed_at = indexed_at * 1000000
        WHERE indexed_at < 10000000000;
        "#,
    )
    .execute(pool)
    .await
    .context("Failed to migrate old indexed_at data to microseconds")?;

    Ok(())
}

/// Process Jetstream event
pub async fn process_event(pool: &SqlitePool, event: &CommitEvent) {
    // Only process Create events
    if let CommitEvent::Create { info, commit } = event {
        // Only process posts
        if commit.info.collection.as_str() != "app.bsky.feed.post" {
            return;
        }

        // Extract post record
        let post = match &commit.record {
            KnownRecord::AppBskyFeedPost(post) => post,
            _ => return,
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
            return;
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
            _ => return,
        };

        // Check if post has blue sky images
        // 計測: 画像解析（HTTP通信）の所要時間
        let t_image_start = std::time::Instant::now();
        let sky_status = evaluate_sky_status(&image_urls).await;
        let t_image = t_image_start.elapsed();

        let table_name = match sky_status {
            SkyStatus::AllFake => "fake_bluesky_posts",
            SkyStatus::AllBlue => "real_bluesky_posts",
            SkyStatus::Mixed => {
                tracing::debug!("Excluding post with mixed images: {}", uri);
                return;
            }
        };

        // Store in database
        let indexed_at = post.created_at.as_ref().timestamp_micros();
        // 計測: DB書き込み（ディスクI/O）の所要時間
        let t_db_start = std::time::Instant::now();
        let query = format!(
            r#"
            INSERT OR REPLACE INTO {} (uri, cid, indexed_at)
            VALUES (?, ?, ?)
            "#,
            table_name
        );
        match sqlx::query(&query)
            .bind(&uri)
            .bind(&cid)
            .bind(indexed_at)
            .execute(pool)
            .await
        {
            Ok(_) => {
                let t_db = t_db_start.elapsed();
                tracing::info!(
                    "MATCH [{}]: t_image={:.1}ms, t_db={:.1}ms, uri={}",
                    table_name.split('_').next().unwrap_or("unknown"),
                    t_image.as_secs_f64() * 1000.0,
                    t_db.as_secs_f64() * 1000.0,
                    uri
                );
            }
            Err(e) => {
                tracing::error!("Failed to store post in {}: {}", table_name, e);
            }
        }
    }
}

/// Get fake feed skeleton
pub async fn get_fake_feed_skeleton(
    pool: &SqlitePool,
    limit: usize,
    cursor: Option<String>,
) -> Result<FeedSkeleton> {
    get_skeleton_from_table(pool, "fake_bluesky_posts", limit, cursor).await
}

/// Get real feed skeleton
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

/// 投稿内の画像の空の状態を判定
///
/// # Arguments
/// * `image_urls` - 分析する画像URLのリスト
///
/// # Returns
/// * `SkyStatus::AllBlue` - 全ての画像が青空
/// * `SkyStatus::AllFake` - 全ての画像が青空でない
/// * `SkyStatus::Mixed` - 青空とそうでないものが混在、またはエラー
async fn evaluate_sky_status(image_urls: &[String]) -> SkyStatus {
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

    let mut results = Vec::new();

    // Wait for all analyses to complete
    for task in tasks {
        match task.await {
            Ok(Ok(is_blue)) => {
                results.push(is_blue);
            }
            Ok(Err(e)) => {
                tracing::debug!("Image analysis failed: {}", e);
                // エラー時は安全のために Mixed 扱い（除外）にする
                return SkyStatus::Mixed;
            }
            Err(e) => {
                tracing::error!("Task join error: {}", e);
                return SkyStatus::Mixed;
            }
        }
    }

    determine_sky_status(&results)
}

fn determine_sky_status(results: &[bool]) -> SkyStatus {
    if results.is_empty() {
        return SkyStatus::Mixed;
    }
    let mut has_blue = false;
    let mut has_fake = false;

    for &is_blue in results {
        if is_blue {
            has_blue = true;
        } else {
            has_fake = true;
        }
    }

    match (has_blue, has_fake) {
        (true, false) => SkyStatus::AllBlue,
        (false, true) => SkyStatus::AllFake,
        _ => SkyStatus::Mixed,
    }
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

    #[tokio::test]
    async fn test_get_feed_skeleton_ordering_and_pagination() {
        use super::*;
        use sqlx::sqlite::SqlitePoolOptions;

        let pool = SqlitePoolOptions::new()
            .connect("sqlite::memory:")
            .await
            .unwrap();

        // テーブル作成
        migrate(&pool).await.unwrap();

        // テスト用データ（マイクロ秒単位の indexed_at）を挿入
        // 時系列順: uri3 (最新) -> uri1 -> uri2 (最古)
        sqlx::query("INSERT INTO fake_bluesky_posts (uri, cid, indexed_at) VALUES (?, ?, ?)")
            .bind("at://did:example:1/foo/1")
            .bind("cid1")
            .bind(1700000000000000_i64) // 中間
            .execute(&pool)
            .await
            .unwrap();

        sqlx::query("INSERT INTO fake_bluesky_posts (uri, cid, indexed_at) VALUES (?, ?, ?)")
            .bind("at://did:example:1/foo/2")
            .bind("cid2")
            .bind(1600000000000000_i64) // 最古
            .execute(&pool)
            .await
            .unwrap();

        sqlx::query("INSERT INTO fake_bluesky_posts (uri, cid, indexed_at) VALUES (?, ?, ?)")
            .bind("at://did:example:1/foo/3")
            .bind("cid3")
            .bind(1800000000000000_i64) // 最新
            .execute(&pool)
            .await
            .unwrap();

        // 1. limit=2 で取得（最新の2件が降順で返るはず）
        let result1 = get_fake_feed_skeleton(&pool, 2, None).await.unwrap();
        assert_eq!(result1.feed.len(), 2);
        assert_eq!(result1.feed[0].post, "at://did:example:1/foo/3"); // 180...
        assert_eq!(result1.feed[1].post, "at://did:example:1/foo/1"); // 170...

        // カーソルは2件目の indexed_at と同じはず
        assert_eq!(result1.cursor, Some("1700000000000000".to_string()));

        // 2. カーソルを使って続きを取得（残りの最古の1件が返るはず）
        let result2 = get_fake_feed_skeleton(&pool, 2, result1.cursor)
            .await
            .unwrap();
        assert_eq!(result2.feed.len(), 1);
        assert_eq!(result2.feed[0].post, "at://did:example:1/foo/2"); // 160...

        // もう続きはないのでカーソルはNoneになるはず
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
        migrate(&pool).await.unwrap();

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

    #[test]
    fn test_determine_sky_status() {
        use super::*;

        // 全て青空
        assert_eq!(determine_sky_status(&[true, true]), SkyStatus::AllBlue);
        assert_eq!(determine_sky_status(&[true]), SkyStatus::AllBlue);

        // 全て偽物
        assert_eq!(determine_sky_status(&[false, false]), SkyStatus::AllFake);
        assert_eq!(determine_sky_status(&[false]), SkyStatus::AllFake);

        // 混在
        assert_eq!(determine_sky_status(&[true, false]), SkyStatus::Mixed);
        assert_eq!(determine_sky_status(&[false, true]), SkyStatus::Mixed);

        // 空（通常ありえないが）
        assert_eq!(determine_sky_status(&[]), SkyStatus::Mixed);
    }
}
