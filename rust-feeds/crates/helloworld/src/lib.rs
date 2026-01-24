use shared::{FeedItem, FeedSkeletonResult};

/// helloworldフィードの固定ポストを返す
pub fn get_posts() -> FeedSkeletonResult {
    FeedSkeletonResult {
        cursor: None,
        feed: vec![
            FeedItem {
                post: "at://did:plc:z72i7hdynmk6r22z27h6tvur/app.bsky.feed.post/3jtadqcbi7r2a"
                    .to_string(),
            },
            FeedItem {
                post: "at://did:plc:z72i7hdynmk6r22z27h6tvur/app.bsky.feed.post/3ldy6oad3vk27"
                    .to_string(),
            },
        ],
    }
}
