use shared::{FeedItem, FeedSkeletonResult};

/// helloworldフィードの固定ポストを返す
pub fn get_posts() -> FeedSkeletonResult {
    FeedSkeletonResult {
        cursor: None,
        feed: vec![
            FeedItem {
                post: "at://did:plc:z72i7hdynmk6r22z27h6tvur/app.bsky.feed.post/3lbrqkqsgv22w"
                    .to_string(),
            },
            FeedItem {
                post: "at://did:plc:z72i7hdynmk6r22z27h6tvur/app.bsky.feed.post/3lbrqkfr6ok2w"
                    .to_string(),
            },
        ],
    }
}
