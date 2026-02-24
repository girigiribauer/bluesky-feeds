use crate::error::AppError;
use crate::state::{FeedQuery, SharedState};
use axum::{http::HeaderMap, response::Json};

pub async fn handle_privatelist(
    state: SharedState,
    headers: HeaderMap,
    params: FeedQuery,
) -> Result<Json<bsky_core::FeedSkeletonResult>, AppError> {
    let requester_did = match headers.get("authorization").and_then(|h| h.to_str().ok()) {
        Some(header) => match bsky_core::extract_did_from_jwt(Some(header)) {
            Ok(did) => did,
            Err(e) => {
                tracing::warn!("Failed to extract DID from Authorization header: {}", e);
                return Err(AppError::Unauthorized(
                    "Invalid Authorization header".to_string(),
                ));
            }
        },
        None => {
            return Err(AppError::Unauthorized(
                "Missing Authorization header".to_string(),
            ));
        }
    };

    let limit = params.limit.unwrap_or(20);
    let limit = std::cmp::min(limit, 100);

    let result = privatelist::get_feed_skeleton(
        &state.privatelist_db,
        &state.http_client,
        &requester_did,
        "",
        params.cursor,
        limit,
    )
    .await;

    match result {
        Ok(feed) => Ok(Json(feed)),
        Err(e) => {
            tracing::error!("Failed to get privatelist feed skeleton: {}", e);
            Err(AppError::Internal(e))
        }
    }
}
