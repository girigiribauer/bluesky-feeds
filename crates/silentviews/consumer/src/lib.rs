pub mod app;
pub mod cv;
pub mod pipeline;
pub mod reason;
pub mod stats;
pub mod tools;

use anyhow::{Context, Result};
use atrium_api::app::bsky::feed::post::RecordEmbedRefs;
use atrium_api::types::{BlobRef, TypedBlobRef, Union};
use jetstream::PostEvent;
use sqlx::SqlitePool;

pub async fn migrate(pool: &SqlitePool) -> Result<()> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS silentviews_posts (
            uri TEXT PRIMARY KEY,
            cid TEXT NOT NULL,
            indexed_at INTEGER NOT NULL
        );
        "#,
    )
    .execute(pool)
    .await
    .context("Failed to create silentviews_posts table")?;

    sqlx::query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_silentviews_indexed_at
        ON silentviews_posts(indexed_at DESC);
        "#,
    )
    .execute(pool)
    .await
    .context("Failed to create silentviews index")?;

    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateImage {
    pub small_url: String,
    pub large_url: String,
}

pub const MAX_ASPECT: f64 = 3.0;

pub fn too_elongated(width: f64, height: f64) -> bool {
    width.max(height) / width.min(height).max(1.0) >= MAX_ASPECT
}

pub fn candidate_images(post: &PostEvent) -> Option<Vec<CandidateImage>> {
    let record = &post.record;
    if record.is_reply || record.self_labels.is_some() || !is_silent(&record.text) {
        return None;
    }

    let Some(Union::Refs(RecordEmbedRefs::AppBskyEmbedImagesMain(images))) = &record.embed else {
        return None;
    };

    let known_too_elongated = images.images.iter().any(|image| {
        image
            .aspect_ratio
            .as_ref()
            .is_some_and(|r| too_elongated(r.width.get() as f64, r.height.get() as f64))
    });
    if known_too_elongated {
        return None;
    }

    let candidates: Vec<CandidateImage> = images
        .images
        .iter()
        .map(|image| {
            let cid = match &image.image {
                BlobRef::Typed(TypedBlobRef::Blob(blob)) => blob.r#ref.0.to_string(),
                BlobRef::Untyped(untyped) => untyped.cid.clone(),
            };
            CandidateImage {
                small_url: cdn_url("avatar_thumbnail", &post.did, &cid),
                large_url: cdn_url("feed_thumbnail", &post.did, &cid),
            }
        })
        .collect();

    if candidates.is_empty() {
        None
    } else {
        Some(candidates)
    }
}

const MAX_MARKS: usize = 3;
const OWN_TAGS: [&str; 2] = ["silentviews", "silentview"];

fn is_silent(text: &str) -> bool {
    let rest = without_own_tags(text);
    !rest.chars().any(char::is_alphanumeric) && marks(&rest) <= MAX_MARKS
}

fn without_own_tags(text: &str) -> String {
    text.split_whitespace()
        .filter(|word| {
            let tag = word.strip_prefix(['#', '＃']).map(str::to_lowercase);
            !tag.is_some_and(|t| OWN_TAGS.contains(&t.as_str()))
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn marks(text: &str) -> usize {
    const ZWJ: char = '\u{200D}';
    let mut count = 0;
    let mut flag_half = false;
    let mut prev = ' ';
    for c in text.chars() {
        let joined = prev == ZWJ;
        prev = c;
        if joined || c == ZWJ || c.is_whitespace() || is_modifier(c) {
            continue;
        }
        if ('\u{1F1E6}'..='\u{1F1FF}').contains(&c) {
            flag_half = !flag_half;
            if !flag_half {
                continue;
            }
        }
        count += 1;
    }
    count
}

fn is_modifier(c: char) -> bool {
    matches!(c,
        '\u{FE00}'..='\u{FE0F}'
        | '\u{1F3FB}'..='\u{1F3FF}'
        | '\u{E0020}'..='\u{E007F}'
        | '\u{20E3}')
}

pub const CDN_BASE: &str = "https://cdn.bsky.app";

fn cdn_url(kind: &str, did: &str, cid: &str) -> String {
    format!("{CDN_BASE}/img/{kind}/plain/{did}/{cid}@jpeg")
}

#[cfg(test)]
mod tests {
    use super::*;

    use serde_json::{json, Value};

    const DID: &str = "did:plc:z72i7hdynmk6r22z27h6tvur";
    const CID: &str = "bafyreibvjvcv745gig4mvqs4hctx4zfkono4rjejm2ta6gtyzkqxfjeily";
    const RKEY: &str = "3l3temxelsm2a";
    const IMAGE_CID: &str = "bafkreib7o2gowpvz2qh6ytvgdpkvcbfzowvvrqvgl3cztrmjvhmtxyfwfa";
    const TIME_US: i64 = 1_700_000_000_000_000;

    fn image() -> Value {
        json!({
            "alt": "",
            "image": {
                "$type": "blob",
                "ref": { "$link": IMAGE_CID },
                "mimeType": "image/jpeg",
                "size": 1000,
            },
        })
    }

    fn images_embed(count: usize) -> Value {
        json!({
            "$type": "app.bsky.embed.images",
            "images": (0..count).map(|_| image()).collect::<Vec<_>>(),
        })
    }

    fn post_event(text: &str, extra: Value) -> PostEvent {
        let mut record = json!({
            "$type": "app.bsky.feed.post",
            "text": text,
            "createdAt": "2026-01-01T00:00:00.000Z",
            "embed": images_embed(1),
        });
        if let Value::Object(extra) = extra {
            for (key, value) in extra {
                record[key] = value;
            }
        }
        let json = json!({
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
        match jetstream::event::parse_event(&json, &mut Default::default()) {
            Ok(Some(jetstream::Event::Post(post))) => *post,
            _ => panic!("投稿として読めなかった"),
        }
    }

    fn is_candidate(text: &str, extra: Value) -> bool {
        candidate_images(&post_event(text, extra)).is_some()
    }

    /// 本文が空か、絵文字・記号・空白だけの画像付き投稿を候補にすること
    #[test]
    fn test_wordless_image_post_is_candidate() {
        for text in ["", "   \n", "\u{3000}", "🌄", "✨🌊✨", "…", "!?", "♪"] {
            assert!(is_candidate(text, json!({})), "text={text:?}");
        }
    }

    /// 絵文字や記号が3個までなら候補にし、4個以上なら喋っているものとして候補にしないこと
    #[test]
    fn test_more_than_three_marks_are_not_candidate() {
        assert!(is_candidate("🌄✨🌊", json!({})));
        assert!(!is_candidate("😆😆😆😆", json!({})));
        assert!(!is_candidate("✨ 🌊 ✨ 🌄", json!({})));
    }

    /// 本文がこのフィードのタグ（#silentviews・#silentview、大文字小文字や全角の＃も可）だけなら、候補にすること
    #[test]
    fn test_own_tag_alone_is_candidate() {
        for text in [
            "#silentviews",
            "#silentview",
            "#SilentViews",
            "＃silentviews",
            "#silentviews\n#silentview",
        ] {
            assert!(is_candidate(text, json!({})), "text={text:?}");
        }
    }

    /// このフィードのタグは、絵文字や記号3個までと一緒に書いてもよく、4個以上なら候補にしないこと
    #[test]
    fn test_own_tag_with_marks_follows_mark_limit() {
        assert!(is_candidate("#silentviews 🌄✨🌊", json!({})));
        assert!(!is_candidate("#silentviews 😆😆😆😆", json!({})));
    }

    /// このフィードのタグがあっても、ほかに文字や別のタグがあれば候補にしないこと
    #[test]
    fn test_own_tag_with_words_is_not_candidate() {
        for text in [
            "#silentviews きれい",
            "#silentviews #nature",
            "#silentviewsfan",
            "#silentviews.",
            "silentviews",
        ] {
            assert!(!is_candidate(text, json!({})), "text={text:?}");
        }
    }

    /// 肌の色つき・国旗・つないだ絵文字は、見た目どおり1個と数えること
    #[test]
    fn test_marks_count_what_looks_like_one_as_one() {
        for (text, n) in [
            ("👍🏼", 1),
            ("🇯🇵", 1),
            ("🇯🇵🇺🇸", 2),
            ("🐈\u{200D}⬛", 1),
            ("👨\u{200D}👩\u{200D}👧", 1),
            ("❤\u{FE0F}", 1),
            (
                "🏴\u{E0067}\u{E0062}\u{E0073}\u{E0063}\u{E0074}\u{E007F}",
                1,
            ),
        ] {
            assert_eq!(marks(text), n, "text={text:?}");
        }
    }

    /// 本文に文字や数字が1つでもあれば候補にしないこと（言語を問わない）
    #[test]
    fn test_post_with_words_is_not_candidate() {
        for text in [
            "a",
            "空",
            "あ",
            "ㅎ",
            "1",
            "١",
            "🌄 2026",
            "#nature",
            "Привет",
        ] {
            assert!(!is_candidate(text, json!({})), "text={text:?}");
        }
    }

    /// 丸で囲んだ英字や数字の絵文字は、文字・数字が入っているものとして候補にしないこと（安全側に倒す）
    #[test]
    fn test_enclosed_letters_and_keycap_digits_count_as_words() {
        for text in ["Ⓐ", "1️⃣"] {
            assert!(!is_candidate(text, json!({})), "text={text:?}");
        }
    }

    /// 投稿の一部が規格どおりでなくても、言葉なしの画像付き投稿なら候補にすること
    #[test]
    fn test_loosely_parsed_wordless_post_is_candidate() {
        let post = post_event("", json!({ "langs": ["日本語"] }));

        assert!(!post.record.strict);
        assert!(candidate_images(&post).is_some());
    }

    /// 返信は候補にしないこと（返信先の投稿が一緒に表示されるため）
    #[test]
    fn test_reply_is_not_candidate() {
        let parent = json!({ "uri": format!("at://{DID}/app.bsky.feed.post/{RKEY}"), "cid": CID });
        let reply = json!({ "root": parent, "parent": parent });

        assert!(!is_candidate("", json!({ "reply": reply })));
    }

    /// 投稿者が警告ラベル（アダルト・ヌード・生々しい表現など）を付けた投稿は候補にしないこと
    #[test]
    fn test_self_labeled_post_is_not_candidate() {
        let labels = json!({
            "$type": "com.atproto.label.defs#selfLabels",
            "values": [{ "val": "nudity" }],
        });

        assert!(!is_candidate("", json!({ "labels": labels })));
    }

    /// 知らない書き方の警告ラベルでも、警告ラベルが付いている投稿は候補にしないこと（安心して見られることを優先する）
    #[test]
    fn test_post_with_unknown_label_format_is_not_candidate() {
        let labels = json!({
            "$type": "com.example.futureLabels",
            "values": [{ "val": "something" }],
        });

        assert!(!is_candidate("", json!({ "labels": labels })));
    }

    /// 引用付きの画像投稿は候補にしないこと（引用先の本文が一緒に表示されるため）
    #[test]
    fn test_quote_with_images_is_not_candidate() {
        let embed = json!({
            "$type": "app.bsky.embed.recordWithMedia",
            "record": {
                "$type": "app.bsky.embed.record",
                "record": { "uri": format!("at://{DID}/app.bsky.feed.post/{RKEY}"), "cid": CID },
            },
            "media": images_embed(1),
        });

        assert!(!is_candidate("", json!({ "embed": embed })));
    }

    /// 画像の無い投稿は候補にしないこと
    #[test]
    fn test_post_without_images_is_not_candidate() {
        let mut post = post_event("", json!({}));
        post.record.embed = None;

        assert!(candidate_images(&post).is_none());
    }

    /// 動画やリンクカードなど、画像以外の添付の投稿は候補にしないこと
    #[test]
    fn test_post_with_other_embeds_is_not_candidate() {
        let video = json!({
            "$type": "app.bsky.embed.video",
            "video": {
                "$type": "blob",
                "ref": { "$link": IMAGE_CID },
                "mimeType": "video/mp4",
                "size": 1000,
            },
        });
        let external = json!({
            "$type": "app.bsky.embed.external",
            "external": { "uri": "https://example.com", "title": "", "description": "" },
        });

        assert!(!is_candidate("", json!({ "embed": video })));
        assert!(!is_candidate("", json!({ "embed": external })));
    }

    fn image_with_aspect(width: u64, height: u64) -> Value {
        let mut img = image();
        img["aspectRatio"] = json!({ "width": width, "height": height });
        img
    }

    fn images_embed_of(images: Vec<Value>) -> Value {
        json!({ "$type": "app.bsky.embed.images", "images": images })
    }

    /// 投稿データに書かれた縦横比が3倍以上の画像（横長・縦長とも）を含む投稿は、画像を取りに行く前に候補から外すこと
    #[test]
    fn test_post_with_known_elongated_image_is_not_candidate() {
        for (w, h) in [(3000, 1000), (1000, 3000), (6000, 1000)] {
            let embed = images_embed_of(vec![image_with_aspect(w, h)]);
            assert!(!is_candidate("", json!({ "embed": embed })), "{w}x{h}");
        }
    }

    /// 複数画像のうち1枚でも縦横比が3倍以上なら、投稿ごと候補から外すこと（全部の画像が通ったときだけ載せるため）
    #[test]
    fn test_one_elongated_image_excludes_whole_post() {
        let embed = images_embed_of(vec![
            image_with_aspect(1000, 1000),
            image_with_aspect(3000, 1000),
        ]);

        assert!(!is_candidate("", json!({ "embed": embed })));
    }

    /// 縦横比が3倍未満の画像や、縦横比が書かれていない画像は、候補に残すこと（書かれていない分は画像を見てから判断する）
    #[test]
    fn test_normal_or_unknown_aspect_is_candidate() {
        let embed = images_embed_of(vec![image_with_aspect(2999, 1000), image()]);

        assert!(is_candidate("", json!({ "embed": embed })));
    }

    /// 縦横比の判定は、長い辺÷短い辺が3以上かどうかで、向き（横長・縦長）を問わないこと
    #[test]
    fn test_too_elongated_threshold() {
        assert!(too_elongated(300.0, 100.0));
        assert!(too_elongated(100.0, 300.0));
        assert!(!too_elongated(299.0, 100.0));
        assert!(!too_elongated(100.0, 100.0));
    }

    /// 隠しタグ（tags 欄）は表示されないので、付いていても候補にすること
    #[test]
    fn test_hidden_tags_do_not_exclude() {
        assert!(is_candidate("", json!({ "tags": ["nature", "空"] })));
    }

    /// 複数画像の投稿は全部の画像を候補として返すこと
    #[test]
    fn test_all_images_of_post_are_returned() {
        let images =
            candidate_images(&post_event("", json!({ "embed": images_embed(3) }))).unwrap();

        assert_eq!(images.len(), 3);
    }

    /// 1段目用の小さいサムネイルと2段目用の大きいサムネイルの URL を作ること
    #[test]
    fn test_candidate_has_small_and_large_thumbnail_urls() {
        let images = candidate_images(&post_event("", json!({}))).unwrap();

        assert_eq!(
            images[0],
            CandidateImage {
                small_url: format!(
                    "https://cdn.bsky.app/img/avatar_thumbnail/plain/{DID}/{IMAGE_CID}@jpeg"
                ),
                large_url: format!(
                    "https://cdn.bsky.app/img/feed_thumbnail/plain/{DID}/{IMAGE_CID}@jpeg"
                ),
            }
        );
    }

    /// 古い書き方の画像データでも、画像の URL を作れること
    #[test]
    fn test_legacy_image_blob_gives_urls() {
        let embed = json!({
            "$type": "app.bsky.embed.images",
            "images": [{ "alt": "", "image": { "cid": IMAGE_CID, "mimeType": "image/jpeg" } }],
        });

        let images = candidate_images(&post_event("", json!({ "embed": embed }))).unwrap();

        assert_eq!(
            images[0].large_url,
            format!("https://cdn.bsky.app/img/feed_thumbnail/plain/{DID}/{IMAGE_CID}@jpeg")
        );
    }

    /// 起動のたびに表を作る処理を動かしても、壊れず中身も消えないこと
    #[tokio::test]
    async fn test_migrate_can_run_again_without_losing_rows() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .connect("sqlite::memory:")
            .await
            .unwrap();
        migrate(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO silentviews_posts (uri, cid, indexed_at) VALUES ('at://a/1', 'cid', 1)",
        )
        .execute(&pool)
        .await
        .unwrap();

        migrate(&pool).await.unwrap();

        let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM silentviews_posts")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 1);
    }
}
