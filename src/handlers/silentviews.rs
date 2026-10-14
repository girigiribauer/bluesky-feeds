use crate::error::AppError;
use crate::state::{FeedQuery, SharedState};
use axum::response::Json;

pub async fn handle_silentviews(
    state: SharedState,
    params: FeedQuery,
) -> Result<Json<bsky_core::FeedSkeletonResult>, AppError> {
    let skeleton = silentviews_server::get_feed_skeleton(
        &state.silentviews_db,
        params.limit.unwrap_or(30),
        params.cursor.clone(),
    )
    .await?;

    Ok(Json(bsky_core::FeedSkeletonResult {
        feed: skeleton
            .posts
            .into_iter()
            .map(|post| bsky_core::FeedItem { post })
            .collect(),
        cursor: skeleton.cursor,
    }))
}
