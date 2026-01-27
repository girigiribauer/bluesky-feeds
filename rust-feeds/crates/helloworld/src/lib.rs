use std::collections::VecDeque;
use models::{FeedItem, FeedSkeletonResult};
use jetstream_oxide::events::commit::CommitEvent;
use atrium_api::record::KnownRecord;

const MAX_POSTS: usize = 1000;
const DEFAULT_LIMIT: usize = 30;

#[derive(Debug)]
pub struct PostData {
    pub uri: String,
    pub indexed_at: i64, // micro seconds
}

#[derive(Debug, Default)]
pub struct State {
    pub posts: VecDeque<PostData>,
}

pub fn process_event(state: &mut State, event: &CommitEvent) {
    if let CommitEvent::Create { info, commit } = event {
        if let KnownRecord::AppBskyFeedPost(post) = &commit.record {
            process_record(
                state,
                commit.info.collection.as_str(),
                commit.info.rkey.as_str(),
                info.did.as_str(),
                post,
            );
        }
    }
}

pub fn process_record(
    state: &mut State,
    collection: &str,
    rkey: &str,
    did: &str,
    post: &atrium_api::app::bsky::feed::post::Record,
) {
    if collection != "app.bsky.feed.post" {
        return;
    }

    if post.text.to_lowercase().contains("hello world") {
        let post_uri = format!("at://{}/{}/{}", did, collection, rkey);
        tracing::info!("Found hello world post: {}", post_uri);
        tracing::info!("Link: https://bsky.app/profile/{}/post/{}", did, rkey);

        let indexed_at = chrono::Utc::now().timestamp_micros();

        state.posts.push_front(PostData {
            uri: post_uri,
            indexed_at,
        });

        if state.posts.len() > MAX_POSTS {
            state.posts.pop_back();
        }
    }
}

pub fn get_feed_skeleton(
    state: &State,
    cursor: Option<String>,
    limit: Option<usize>,
) -> FeedSkeletonResult {
    let mut feed = Vec::new();
    let limit = limit.unwrap_or(DEFAULT_LIMIT);

    // カーソルがない場合（初回リクエスト）のみ固定ポストを追加
    if cursor.is_none() {
        feed.push(FeedItem {
            post: "at://did:plc:tsvcmd72oxp47wtixs4qllyi/app.bsky.feed.post/3ldcooerekc2y".to_string(),
        });
    }

    let cursor_timestamp = cursor.and_then(|c| c.parse::<i64>().ok());

    let dynamic_posts: Vec<_> = state
        .posts
        .iter()
        .filter(|post| {
            // カーソルがある場合は、それより古い（小さい）ものだけを対象にする
            match cursor_timestamp {
                Some(ts) => post.indexed_at < ts,
                None => true,
            }
        })
        .take(limit)
        .collect();

    let next_cursor = dynamic_posts.last().map(|p| p.indexed_at.to_string());

    feed.extend(dynamic_posts.into_iter().map(|p| FeedItem {
        post: p.uri.clone(),
    }));

    FeedSkeletonResult {
        cursor: next_cursor,
        feed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use atrium_api::app::bsky::feed::post::{Record, RecordData};
    use atrium_api::types::string::Datetime;
    use ipld_core::ipld::Ipld;
    use std::collections::BTreeMap;

    fn make_record(text: &str) -> Record {
        Record {
            data: RecordData {
                text: text.to_string(),
                created_at: Datetime::new(chrono::DateTime::parse_from_rfc3339("2024-01-01T00:00:00Z").unwrap()),
                embed: None,
                entities: None,
                facets: None,
                labels: None,
                langs: None,
                reply: None,
                tags: None,
            },
            extra_data: Ipld::Map(BTreeMap::new()),
        }
    }

    #[test]
    fn test_hello_detection() {
        let mut state = State::default();
        let record = make_record("Hello world this is a test");

        process_record(&mut state, "app.bsky.feed.post", "rkey", "did:plc:test", &record);

        assert_eq!(state.posts.len(), 1);
        assert_eq!(state.posts[0].uri, "at://did:plc:test/app.bsky.feed.post/rkey");
    }

    #[test]
    fn test_case_insensitive() {
        let mut state = State::default();
        let record = make_record("hELLO wORLD this is a test");

        process_record(&mut state, "app.bsky.feed.post", "rkey", "did:plc:test", &record);

        assert_eq!(state.posts.len(), 1);
    }

    #[test]
    fn test_ignore_other_text() {
        let mut state = State::default();
        let record = make_record("Goodbye world");

        process_record(&mut state, "app.bsky.feed.post", "rkey", "did:plc:test", &record);

        assert_eq!(state.posts.len(), 0);
    }

    #[test]
    fn test_ignore_other_collection() {
        let mut state = State::default();
        let record = make_record("Hello world");

        process_record(&mut state, "app.bsky.graph.follow", "rkey", "did:plc:test", &record);

        assert_eq!(state.posts.len(), 0);
    }
}
