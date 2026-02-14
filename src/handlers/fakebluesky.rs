use crate::state::{FeedQuery, SharedState};
use axum::{http::StatusCode, response::Json};

pub async fn handle_fakebluesky(
    state: SharedState,
    params: FeedQuery,
) -> Result<Json<bsky_core::FeedSkeletonResult>, (StatusCode, String)> {
    let skeleton = fakebluesky::get_feed_skeleton(
        &state.fakebluesky_db,
        params.limit.unwrap_or(30),
        params.cursor.clone(),
    )
    .await
    .map_err(|e| {
        tracing::error!("Fakebluesky error: {:#}", e);
        (StatusCode::INTERNAL_SERVER_ERROR, format!("{:#}", e))
    })?;

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
