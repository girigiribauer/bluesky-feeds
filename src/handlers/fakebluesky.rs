use crate::error::AppError;
use crate::state::{FeedQuery, SharedState};
use axum::response::Json;

pub async fn handle_fakebluesky(
    state: SharedState,
    params: FeedQuery,
) -> Result<Json<bsky_core::FeedSkeletonResult>, AppError> {
    let skeleton = fakebluesky::get_feed_skeleton(
        &state.fakebluesky_db,
        params.limit.unwrap_or(30),
        params.cursor.clone(),
    )
    .await?;

    // Convert to FeedSkeletonResult
    let result = bsky_core::FeedSkeletonResult {
        feed: skeleton
            .feed
            .into_iter()
            .map(|item| bsky_core::FeedItem { post: item.post })
            .collect(),
        cursor: skeleton.cursor,
    };

    Ok(Json(result))
}
