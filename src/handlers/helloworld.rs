use crate::state::{FeedQuery, SharedState};
use axum::{http::StatusCode, response::Json};

pub async fn handle_helloworld(
    state: SharedState,
    headers: axum::http::HeaderMap,
    params: FeedQuery,
) -> Result<Json<bsky_core::FeedSkeletonResult>, (StatusCode, String)> {
    let _auth_header = headers
        .get("authorization")
        .and_then(|h| h.to_str().ok())
        .ok_or((
            StatusCode::UNAUTHORIZED,
            "Missing or invalid authorization header".to_string(),
        ))?;

    let pool = state.helloworld_db.clone();
    let skeleton = helloworld::get_feed_skeleton(&pool, params.cursor, params.limit).await;
    Ok(Json(skeleton))
}
