mod image_analyzer;

use anyhow::{Context, Result};
use atrium_api::record::KnownRecord;
use image_analyzer::{is_blue_sky_image, BlueDetectionConfig};
use jetstream_oxide::events::commit::CommitEvent;
use serde::Serialize;
use sqlx::SqlitePool;
use std::sync::Arc;
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
        let has_blue_sky = has_blue_sky_images(&image_urls).await;

        // If any image is blue sky, exclude this post
        if has_blue_sky {
            tracing::debug!("Excluding post with blue sky image: {}", uri);
            return;
        }

        // Store in database
        let indexed_at = chrono::Utc::now().timestamp();
        match sqlx::query(
            r#"
            INSERT OR REPLACE INTO fake_bluesky_posts (uri, cid, indexed_at)
            VALUES (?, ?, ?)
            "#,
        )
        .bind(&uri)
        .bind(&cid)
        .bind(indexed_at)
        .execute(pool)
        .await
        {
            Ok(_) => {
                tracing::info!("Stored fake bluesky post: {}", uri);
            }
            Err(e) => {
                tracing::error!("Failed to store post: {}", e);
            }
        }
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
