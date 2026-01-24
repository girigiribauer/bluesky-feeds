use shared::{FeedItem, FeedSkeletonResult};

/// helloworldフィードの固定ポストを返す
pub fn get_posts() -> FeedSkeletonResult {
    FeedSkeletonResult {
        cursor: None,
        feed: vec![
            FeedItem {
                post: "at://did:plc:tsvcmd72oxp47wtixs4qllyi/app.bsky.feed.post/3ldy6oad3vk27"
                    .to_string(),
            },
            FeedItem {
                post: "at://did:plc:tsvcmd72oxp47wtixs4qllyi/app.bsky.feed.post/3ldcooerekc2y"
                    .to_string(),
            },
        ],
    }
}
