pub mod image_analyzer;

use anyhow::{Context, Result};
use image_analyzer::{is_blue_sky_image, BlueDetectionConfig};
use jetstream::PostEvent;
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

fn images_to_inspect(post: &PostEvent) -> Option<Vec<String>> {
    let cleaned_text: String = post
        .record
        .text
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();

    static BLUESKY_REGEX: OnceLock<Regex> = OnceLock::new();
    let regex = BLUESKY_REGEX.get_or_init(|| Regex::new(r"(?i)^bluesky[\p{P}\p{S}]*$").unwrap());

    if !regex.is_match(&cleaned_text) {
        return None;
    }

    match extract_image_urls(post.record.embed.as_ref(), &post.did) {
        Some(urls) if !urls.is_empty() => Some(urls),
        _ => None,
    }
}

pub async fn process_event(pool: &SqlitePool, post: &PostEvent) {
    let Some(image_urls) = images_to_inspect(post) else {
        return;
    };

    let uri = post.uri();

    let t_image_start = std::time::Instant::now();
    let sky_status = evaluate_sky_status(&image_urls).await;
    let t_image = t_image_start.elapsed();

    let Some((table_name, indexed_at)) = stored_row(post, sky_status) else {
        tracing::debug!("Excluding post with mixed images: {}", uri);
        return;
    };

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
        .bind(&post.cid)
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

fn stored_row(post: &PostEvent, sky_status: SkyStatus) -> Option<(&'static str, i64)> {
    let table_name = match sky_status {
        SkyStatus::AllFake => "fake_bluesky_posts",
        SkyStatus::AllBlue => "real_bluesky_posts",
        SkyStatus::Mixed => return None,
    };

    Some((table_name, post.indexed_at_us()))
}

pub async fn get_fake_feed_skeleton(
    pool: &SqlitePool,
    limit: usize,
    cursor: Option<String>,
) -> Result<FeedSkeleton> {
    get_skeleton_from_table(pool, "fake_bluesky_posts", limit, cursor).await
}

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

async fn evaluate_sky_status(image_urls: &[String]) -> SkyStatus {
    let config = BlueDetectionConfig::default();
    let semaphore = Arc::new(Semaphore::new(2));

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

    for task in tasks {
        match task.await {
            Ok(Ok(is_blue)) => {
                results.push(is_blue);
            }
            Ok(Err(e)) => {
                tracing::debug!("Image analysis failed: {}", e);
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

fn extract_image_urls(
    embed: Option<&atrium_api::types::Union<atrium_api::app::bsky::feed::post::RecordEmbedRefs>>,
    did: &str,
) -> Option<Vec<String>> {
    use atrium_api::types::{BlobRef, TypedBlobRef, Union};

    let embed = embed?;

    match embed {
        Union::Refs(
            atrium_api::app::bsky::feed::post::RecordEmbedRefs::AppBskyEmbedImagesMain(images),
        ) => {
            let urls: Vec<String> = images
                .images
                .iter()
                .map(|img| {
                    let cid = match &img.image {
                        BlobRef::Typed(TypedBlobRef::Blob(blob)) => blob.r#ref.0.to_string(),
                        BlobRef::Untyped(untyped) => untyped.cid.clone(),
                    };

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
    /// 本文が bluesky 単独（記号・絵文字の付加は可）のときだけ対象にすること
    #[test]
    fn test_bluesky_regex() {
        use super::*;

        let matches = |text: &str| -> bool {
            images_to_inspect(&post_event(
                text,
                "2026-01-01T00:00:00.000Z",
                Some(images_embed()),
            ))
            .is_some()
        };

        assert!(matches("bluesky"));
        assert!(matches("Bluesky"));
        assert!(matches("BLUESKY"));
        assert!(matches("blue sky"));
        assert!(matches("Blue \n Sky"));
        assert!(matches("bluesky!"));
        assert!(matches("  bluesky  "));
        assert!(matches("bluesky✨"));
        assert!(matches("bluesky!!!!"));
        assert!(matches("bluesky🤗"));
        assert!(matches("bluesky..."));

        assert!(!matches("blue-sky"));
        assert!(!matches("blue.sky"));
        assert!(!matches("I love bluesky"));
        assert!(!matches("bluesky is great"));
        assert!(!matches("hello bluesky world"));
    }

    #[tokio::test]
    async fn test_get_feed_skeleton_ordering_and_pagination() {
        use super::*;
        use sqlx::sqlite::SqlitePoolOptions;

        let pool = SqlitePoolOptions::new()
            .connect("sqlite::memory:")
            .await
            .unwrap();

        migrate(&pool).await.unwrap();

        sqlx::query("INSERT INTO fake_bluesky_posts (uri, cid, indexed_at) VALUES (?, ?, ?)")
            .bind("at://did:example:1/foo/1")
            .bind("cid1")
            .bind(1700000000000000_i64)
            .execute(&pool)
            .await
            .unwrap();

        sqlx::query("INSERT INTO fake_bluesky_posts (uri, cid, indexed_at) VALUES (?, ?, ?)")
            .bind("at://did:example:1/foo/2")
            .bind("cid2")
            .bind(1600000000000000_i64)
            .execute(&pool)
            .await
            .unwrap();

        sqlx::query("INSERT INTO fake_bluesky_posts (uri, cid, indexed_at) VALUES (?, ?, ?)")
            .bind("at://did:example:1/foo/3")
            .bind("cid3")
            .bind(1800000000000000_i64)
            .execute(&pool)
            .await
            .unwrap();

        let result1 = get_fake_feed_skeleton(&pool, 2, None).await.unwrap();
        assert_eq!(result1.feed.len(), 2);
        assert_eq!(result1.feed[0].post, "at://did:example:1/foo/3");
        assert_eq!(result1.feed[1].post, "at://did:example:1/foo/1");

        assert_eq!(result1.cursor, Some("1700000000000000".to_string()));

        let result2 = get_fake_feed_skeleton(&pool, 2, result1.cursor)
            .await
            .unwrap();
        assert_eq!(result2.feed.len(), 1);
        assert_eq!(result2.feed[0].post, "at://did:example:1/foo/2");

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

    const DID: &str = "did:plc:z72i7hdynmk6r22z27h6tvur";
    const CID: &str = "bafyreibvjvcv745gig4mvqs4hctx4zfkono4rjejm2ta6gtyzkqxfjeily";
    const IMAGE_CID: &str = "bafkreib7o2gowpvz2qh6ytvgdpkvcbfzowvvrqvgl3cztrmjvhmtxyfwfa";
    const RKEY: &str = "3l3temxelsm2a";
    const TIME_US: i64 = 1_700_000_000_000_000;

    fn post_event(
        text: &str,
        created_at: &str,
        embed: Option<serde_json::Value>,
    ) -> jetstream::PostEvent {
        let mut record = serde_json::json!({
            "$type": "app.bsky.feed.post",
            "text": text,
            "createdAt": created_at,
        });
        if let Some(embed) = embed {
            record["embed"] = embed;
        }
        let json = serde_json::json!({
            "did": DID,
            "time_us": TIME_US,
            "kind": "commit",
            "commit": {
                "operation": "create",
                "rev": RKEY,
                "rkey": RKEY,
                "collection": "app.bsky.feed.post",
                "cid": CID,
                "record": record,
            },
        })
        .to_string();
        let mut skips = jetstream::SkipStats::default();
        match jetstream::event::parse_event(&json, &mut skips) {
            Ok(Some(jetstream::Event::Post(post))) => *post,
            other => panic!("投稿として読めなかった: {}", other.is_ok()),
        }
    }

    fn images_embed() -> serde_json::Value {
        serde_json::json!({
            "$type": "app.bsky.embed.images",
            "images": [{
                "alt": "",
                "image": {
                    "$type": "blob",
                    "ref": { "$link": IMAGE_CID },
                    "mimeType": "image/jpeg",
                    "size": 1000,
                },
            }],
        })
    }

    async fn migrated_pool() -> sqlx::SqlitePool {
        use super::*;
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .connect("sqlite::memory:")
            .await
            .unwrap();
        migrate(&pool).await.unwrap();
        pool
    }

    async fn stored_count(pool: &sqlx::SqlitePool) -> i64 {
        let fake: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM fake_bluesky_posts")
            .fetch_one(pool)
            .await
            .unwrap();
        let real: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM real_bluesky_posts")
            .fetch_one(pool)
            .await
            .unwrap();
        fake + real
    }

    /// 本文が bluesky にマッチしない投稿は、画像を取りに行かないこと
    #[test]
    fn test_images_to_inspect_skips_non_matching_post() {
        use super::*;

        assert_eq!(
            images_to_inspect(&post_event(
                "I love bluesky",
                "2026-01-01T00:00:00.000Z",
                Some(images_embed())
            )),
            None
        );
    }

    /// 添付画像がない投稿は、画像の判定に進まないこと
    #[test]
    fn test_images_to_inspect_skips_post_without_images() {
        use super::*;

        assert_eq!(
            images_to_inspect(&post_event("bluesky", "2026-01-01T00:00:00.000Z", None)),
            None
        );
        assert_eq!(
            images_to_inspect(&post_event(
                "bluesky",
                "2026-01-01T00:00:00.000Z",
                Some(serde_json::json!({ "$type": "app.bsky.embed.images", "images": [] }))
            )),
            None
        );
    }

    /// 本文がマッチし画像もある投稿は、CDN の URL が判定対象になること
    #[test]
    fn test_images_to_inspect_returns_cdn_urls() {
        use super::*;

        assert_eq!(
            images_to_inspect(&post_event(
                "bluesky!",
                "2026-01-01T00:00:00.000Z",
                Some(images_embed())
            )),
            Some(vec![format!(
                "https://cdn.bsky.app/img/feed_fullsize/plain/{DID}/{IMAGE_CID}@jpeg"
            )])
        );
    }

    /// 空白を挟んだ本文（Blue Sky）も、空白を除いてから判定されること
    #[test]
    fn test_images_to_inspect_removes_whitespace_before_matching() {
        use super::*;

        assert!(images_to_inspect(&post_event(
            "Blue Sky",
            "2026-01-01T00:00:00.000Z",
            Some(images_embed())
        ))
        .is_some());
    }

    /// 対象外の投稿では、何も保存されないこと
    #[tokio::test]
    async fn test_process_event_stores_nothing_for_skipped_post() {
        use super::*;
        let pool = migrated_pool().await;

        process_event(
            &pool,
            &post_event("bluesky", "2026-01-01T00:00:00.000Z", None),
        )
        .await;
        process_event(
            &pool,
            &post_event(
                "I love bluesky",
                "2026-01-01T00:00:00.000Z",
                Some(images_embed()),
            ),
        )
        .await;

        assert_eq!(stored_count(&pool).await, 0);
    }

    /// 画像の判定結果から、書き込み先のテーブルと並び順の値が決まること
    #[test]
    fn test_stored_row_decides_table_and_indexed_at() {
        use super::*;
        let post = post_event("bluesky", "2023-11-14T22:13:19.000Z", Some(images_embed()));

        assert_eq!(
            stored_row(&post, SkyStatus::AllFake),
            Some(("fake_bluesky_posts", 1_699_999_999_000_000))
        );
        assert_eq!(
            stored_row(&post, SkyStatus::AllBlue),
            Some(("real_bluesky_posts", 1_699_999_999_000_000))
        );
        assert_eq!(stored_row(&post, SkyStatus::Mixed), None);
    }

    /// 投稿日時が読めない・未来のときは、受信時刻を並び順に使うこと
    #[test]
    fn test_stored_row_falls_back_to_received_time() {
        use super::*;
        let broken = post_event("bluesky", "", Some(images_embed()));
        let future = post_event("bluesky", "2099-01-01T00:00:00.000Z", Some(images_embed()));

        assert_eq!(
            stored_row(&broken, SkyStatus::AllBlue),
            Some(("real_bluesky_posts", TIME_US))
        );
        assert_eq!(
            stored_row(&future, SkyStatus::AllBlue),
            Some(("real_bluesky_posts", TIME_US))
        );
    }

    /// images の添付から CDN の URL を組み立てること
    #[test]
    fn test_extract_image_urls_builds_cdn_urls() {
        use super::*;
        let post = post_event("bluesky", "2026-01-01T00:00:00.000Z", Some(images_embed()));

        assert_eq!(
            extract_image_urls(post.record.embed.as_ref(), &post.did),
            Some(vec![format!(
                "https://cdn.bsky.app/img/feed_fullsize/plain/{DID}/{IMAGE_CID}@jpeg"
            )])
        );
    }

    /// images 以外の添付と、添付なしでは URL を返さないこと
    #[test]
    fn test_extract_image_urls_ignores_other_embeds() {
        use super::*;
        let external = post_event(
            "bluesky",
            "2026-01-01T00:00:00.000Z",
            Some(serde_json::json!({
                "$type": "app.bsky.embed.external",
                "external": { "uri": "https://example.com", "title": "t", "description": "d" },
            })),
        );
        let none = post_event("bluesky", "2026-01-01T00:00:00.000Z", None);

        assert_eq!(
            extract_image_urls(external.record.embed.as_ref(), &external.did),
            None
        );
        assert_eq!(
            extract_image_urls(none.record.embed.as_ref(), &none.did),
            None
        );
    }

    #[test]
    fn test_determine_sky_status() {
        use super::*;

        assert_eq!(determine_sky_status(&[true, true]), SkyStatus::AllBlue);
        assert_eq!(determine_sky_status(&[true]), SkyStatus::AllBlue);

        assert_eq!(determine_sky_status(&[false, false]), SkyStatus::AllFake);
        assert_eq!(determine_sky_status(&[false]), SkyStatus::AllFake);

        assert_eq!(determine_sky_status(&[true, false]), SkyStatus::Mixed);
        assert_eq!(determine_sky_status(&[false, true]), SkyStatus::Mixed);

        assert_eq!(determine_sky_status(&[]), SkyStatus::Mixed);
    }
}
