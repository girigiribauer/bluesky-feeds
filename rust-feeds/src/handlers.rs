use crate::state::{FeedQuery, SharedState};
use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::Json,
};
use models::FeedService;
use std::str::FromStr;

pub async fn root() -> &'static str {
    "Rust Bluesky Feed Generator"
}

pub async fn get_feed_skeleton(
    State(state): State<SharedState>,
    headers: axum::http::HeaderMap,
    Query(params): Query<FeedQuery>,
) -> Result<Json<models::FeedSkeletonResult>, (StatusCode, String)> {
    tracing::info!("Received feed request: {} (cursor={:?}, limit={:?})", params.feed, params.cursor, params.limit);

    let feed_name = params
        .feed
        .split('/')
        .last()
        .ok_or((StatusCode::BAD_REQUEST, "Invalid feed param".to_string()))?;

    let service = FeedService::from_str(feed_name).ok_or((StatusCode::NOT_FOUND, "Feed not found".to_string()))?;

    match service {
        FeedService::Helloworld => {
            if let Ok(lock) = state.read() {
                Ok(Json(helloworld::get_feed_skeleton(
                    &lock.helloworld,
                    params.cursor,
                    params.limit,
                )))
            } else {
                Err((StatusCode::INTERNAL_SERVER_ERROR, "Lock error".to_string()))
            }
        }
        FeedService::Todoapp => {
            let auth_header = headers
                .get("authorization")
                .and_then(|h| h.to_str().ok())
                .ok_or((StatusCode::UNAUTHORIZED, "Missing or invalid authorization header".to_string()))?;

            // Read client and current token from state (Read Lock)
            let (client, mut current_token, handle, password) = if let Ok(lock) = state.read() {
                (lock.http_client.clone(), lock.service_token.clone(), lock.auth_handle.clone(), lock.auth_password.clone())
            } else {
                return Err((StatusCode::INTERNAL_SERVER_ERROR, "Lock error".to_string()));
            };

            let token = current_token.ok_or((StatusCode::INTERNAL_SERVER_ERROR, "Service not authenticated".to_string()))?;

            // First attempt
            match todoapp::get_feed_skeleton(&client, auth_header, &token).await {
                Ok(res) => Ok(Json(res)),
                Err(e) => {
                    let err_msg = format!("{:?}", e);
                    // Check if error is due to expired token (401 or specific message)
                    if err_msg.contains("ExpiredToken") || err_msg.contains("401") || err_msg.contains("Unauthorized") {
                        tracing::warn!("Token expired, attempting refresh... ({})", err_msg);

                        // RE-AUTHENTICATION LOGIC
                        if !handle.is_empty() && !password.is_empty() {
                            match todoapp::authenticate(&client, &handle, &password).await {
                                Ok(new_token) => {
                                    tracing::info!("Token refresh successful");
                                    // Update state with new token (Write Lock)
                                    if let Ok(mut lock) = state.write() {
                                        lock.service_token = Some(new_token.clone());
                                    }

                                    // Retry request with new token
                                    match todoapp::get_feed_skeleton(&client, auth_header, &new_token).await {
                                        Ok(res) => Ok(Json(res)),
                                        Err(e2) => {
                                            tracing::error!("Retry failed: {:#}", e2);
                                            Err((StatusCode::INTERNAL_SERVER_ERROR, format!("Retry failed: {:#}", e2)))
                                        }
                                    }
                                }
                                Err(reauth_err) => {
                                    tracing::error!("Re-authentication failed: {}", reauth_err);
                                    Err((StatusCode::INTERNAL_SERVER_ERROR, "Re-authentication failed".to_string()))
                                }
                            }
                        } else {
                            tracing::error!("Cannot refresh token: credentials missing");
                            Err((StatusCode::INTERNAL_SERVER_ERROR, "Credentials missing for refresh".to_string()))
                        }
                    } else {
                        // Other error
                        tracing::error!("Todoapp error: {:#}", e);
                        Err((StatusCode::INTERNAL_SERVER_ERROR, format!("{:#}", e)))
                    }
                }
            }
        }
        _ => {
            tracing::warn!("Feed not implemented: {:?}", service);
            Err((StatusCode::NOT_IMPLEMENTED, "Not implemented".to_string()))
        }
    }
}
