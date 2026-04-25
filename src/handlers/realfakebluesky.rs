use crate::error::AppError;
use crate::state::{FeedQuery, SharedState};
use axum::response::Json;

pub async fn handle_fakebluesky(
    state: SharedState,
    params: FeedQuery,
) -> Result<Json<bsky_core::FeedSkeletonResult>, AppError> {
    let skeleton = realfakebluesky::get_fake_feed_skeleton(
        &state.realfakebluesky_db,
        params.limit.unwrap_or(30),
        params.cursor.clone(),
    )
    .await?;

    Ok(Json(bsky_core::FeedSkeletonResult {
        feed: skeleton
            .feed
            .into_iter()
            .map(|item| bsky_core::FeedItem { post: item.post })
            .collect(),
        cursor: skeleton.cursor,
    }))
}

pub async fn handle_realbluesky(
    state: SharedState,
    params: FeedQuery,
) -> Result<Json<bsky_core::FeedSkeletonResult>, AppError> {
    let skeleton = realfakebluesky::get_real_feed_skeleton(
        &state.realfakebluesky_db,
        params.limit.unwrap_or(30),
        params.cursor.clone(),
    )
    .await?;

    Ok(Json(bsky_core::FeedSkeletonResult {
        feed: skeleton
            .feed
            .into_iter()
            .map(|item| bsky_core::FeedItem { post: item.post })
            .collect(),
        cursor: skeleton.cursor,
    }))
}
