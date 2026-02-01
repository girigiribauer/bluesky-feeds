use crate::state::{FeedQuery, SharedState};
use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::Json,
};
use models::FeedService;

pub async fn root() -> &'static str {
    "お試しで Bluesky のフィードを作っています https://github.com/girigiribauer/bluesky-feeds"
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
            let (client, current_token, handle, password) = if let Ok(lock) = state.read() {
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
                                Ok((new_token, new_did)) => {
                                    tracing::info!("Token refresh successful (DID: {})", new_did);
                                    // Update state with new token (Write Lock)
                                    if let Ok(mut lock) = state.write() {
                                        lock.service_token = Some(new_token.clone());
                                        lock.service_did = Some(new_did);
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
        FeedService::Oneyearago => {
            let auth_header = headers
                .get("authorization")
                .and_then(|h| h.to_str().ok())
                .ok_or((StatusCode::UNAUTHORIZED, "Missing or invalid authorization header".to_string()))?;

            // Extract DID from JWT
            let did = todoapp::api::extract_did_from_jwt(auth_header)
                .map_err(|_| (StatusCode::UNAUTHORIZED, "Invalid JWT".to_string()))?;

            // Read client and current token from state
            let (client, current_token, handle, password) = if let Ok(lock) = state.read() {
                (lock.http_client.clone(), lock.service_token.clone(), lock.auth_handle.clone(), lock.auth_password.clone())
            } else {
                return Err((StatusCode::INTERNAL_SERVER_ERROR, "Lock error".to_string()));
            };

            let token = current_token.ok_or((StatusCode::INTERNAL_SERVER_ERROR, "Service not authenticated".to_string()))?;

            // First attempt
            match oneyearago::get_feed_skeleton(&client, auth_header, &token, &did, params.limit.unwrap_or(30), params.cursor.clone()).await {
                Ok(res) => Ok(Json(res)),
                Err(e) => {
                    let err_msg = format!("{:?}", e);
                    if err_msg.contains("ExpiredToken") || err_msg.contains("401") || err_msg.contains("Unauthorized") {
                        tracing::warn!("Token expired, attempting refresh... ({})", err_msg);

                         // RE-AUTHENTICATION LOGIC
                        if !handle.is_empty() && !password.is_empty() {
                            match todoapp::authenticate(&client, &handle, &password).await {
                                Ok((new_token, new_did)) => {
                                    tracing::info!("Token refresh successful (DID: {})", new_did);
                                    // Update state with new token (Write Lock)
                                    if let Ok(mut lock) = state.write() {
                                        lock.service_token = Some(new_token.clone());
                                        lock.service_did = Some(new_did);
                                    }

                                    // Retry request with new token
                                    match oneyearago::get_feed_skeleton(&client, auth_header, &new_token, &did, params.limit.unwrap_or(30), params.cursor.clone()).await {
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
                        tracing::error!("Oneyearago error: {:#}", e);
                        Err((StatusCode::INTERNAL_SERVER_ERROR, format!("{:#}", e)))
                    }
                }
            }
        }
    }
}

pub async fn describe_feed_generator(
    State(state): State<SharedState>,
) -> Result<Json<models::DescribeFeedGeneratorResponse>, (StatusCode, String)> {
    let (did, _service_did) = if let Ok(lock) = state.read() {
        // Authenticated Service DID (from .env/auth) or default from context if we hardcoded it?
        // Ideally we use the authenticated DID.
        let did = lock.service_did.clone().ok_or((
            StatusCode::SERVICE_UNAVAILABLE,
            "Service not authenticated yet".to_string(),
        ))?;
        (did.clone(), did) // logic::service_did
    } else {
        return Err((StatusCode::INTERNAL_SERVER_ERROR, "Lock error".to_string()));
    };

    let feeds = vec![
        models::FeedUri {
            uri: format!("at://{}/app.bsky.feed.generator/helloworld", did),
        },
        models::FeedUri {
            uri: format!("at://{}/app.bsky.feed.generator/todoapp", did),
        },
        models::FeedUri {
            uri: format!("at://{}/app.bsky.feed.generator/oneyearago", did),
        },
    ];

    Ok(Json(models::DescribeFeedGeneratorResponse { did, feeds }))
}
