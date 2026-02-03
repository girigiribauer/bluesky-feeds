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
    tracing::info!(
        "Received feed request: {} (cursor={:?}, limit={:?})",
        params.feed,
        params.cursor,
        params.limit
    );

    let feed_name = params
        .feed
        .split('/')
        .next_back()
        .ok_or((StatusCode::BAD_REQUEST, "Invalid feed URI".to_string()))?;

    let service = FeedService::from_str(feed_name)
        .ok_or((StatusCode::NOT_FOUND, "Feed not found".to_string()))?;

    match service {
        FeedService::Helloworld => {
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
        FeedService::Todoapp => {
            let auth_header = headers
                .get("authorization")
                .and_then(|h| h.to_str().ok())
                .ok_or((
                    StatusCode::UNAUTHORIZED,
                    "Missing or invalid authorization header".to_string(),
                ))?;

            // Read client and current token
            let (client, current_token) = {
                let auth = state.service_auth.read().await;
                (state.http_client.clone(), auth.token.clone())
            };

            let token = current_token.ok_or((
                StatusCode::INTERNAL_SERVER_ERROR,
                "Service not authenticated".to_string(),
            ))?;

            // First attempt
            match todoapp::get_feed_skeleton(&client, auth_header, &token).await {
                Ok(res) => Ok(Json(res)),
                Err(e) => {
                    let err_msg = format!("{:?}", e);
                    // Check if error is due to expired token (401 or specific message)
                    if err_msg.contains("ExpiredToken")
                        || err_msg.contains("401")
                        || err_msg.contains("Unauthorized")
                    {
                        tracing::warn!("Token expired, attempting refresh... ({})", err_msg);

                        // RE-AUTHENTICATION LOGIC
                        let handle = &state.auth_handle;
                        let password = &state.auth_password;

                        if !handle.is_empty() && !password.is_empty() {
                            match todoapp::authenticate(&client, handle, password).await {
                                Ok((new_token, new_did)) => {
                                    tracing::info!("Token refresh successful (DID: {})", new_did);
                                    // Update state with new token
                                    {
                                        let mut auth = state.service_auth.write().await;
                                        auth.token = Some(new_token.clone());
                                        auth.did = Some(new_did);
                                    }

                                    // Retry request with new token
                                    match todoapp::get_feed_skeleton(
                                        &client,
                                        auth_header,
                                        &new_token,
                                    )
                                    .await
                                    {
                                        Ok(res) => Ok(Json(res)),
                                        Err(e2) => {
                                            tracing::error!("Retry failed: {:#}", e2);
                                            Err((
                                                StatusCode::INTERNAL_SERVER_ERROR,
                                                format!("Retry failed: {:#}", e2),
                                            ))
                                        }
                                    }
                                }
                                Err(reauth_err) => {
                                    tracing::error!("Re-authentication failed: {}", reauth_err);
                                    Err((
                                        StatusCode::INTERNAL_SERVER_ERROR,
                                        "Re-authentication failed".to_string(),
                                    ))
                                }
                            }
                        } else {
                            tracing::error!("Cannot refresh token: credentials missing");
                            Err((
                                StatusCode::INTERNAL_SERVER_ERROR,
                                "Credentials missing for refresh".to_string(),
                            ))
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
                .ok_or((
                    StatusCode::UNAUTHORIZED,
                    "Missing or invalid authorization header".to_string(),
                ))?;

            // Extract DID from JWT
            let did = todoapp::api::extract_did_from_jwt(auth_header)
                .map_err(|_| (StatusCode::UNAUTHORIZED, "Invalid JWT".to_string()))?;

            // Read client and current token
            let (client, current_token) = {
                let auth = state.service_auth.read().await;
                (state.http_client.clone(), auth.token.clone())
            };

            let token = current_token.ok_or((
                StatusCode::INTERNAL_SERVER_ERROR,
                "Service not authenticated".to_string(),
            ))?;

            // First attempt
            match oneyearago::get_feed_skeleton(
                &client,
                auth_header,
                &token,
                &did,
                params.limit.unwrap_or(30),
                params.cursor.clone(),
            )
            .await
            {
                Ok(res) => Ok(Json(res)),
                Err(e) => {
                    let err_msg = format!("{:?}", e);
                    if err_msg.contains("ExpiredToken")
                        || err_msg.contains("401")
                        || err_msg.contains("Unauthorized")
                    {
                        tracing::warn!("Token expired, attempting refresh... ({})", err_msg);

                        // RE-AUTHENTICATION LOGIC
                        let handle = &state.auth_handle;
                        let password = &state.auth_password;

                        if !handle.is_empty() && !password.is_empty() {
                            match todoapp::authenticate(&client, handle, password).await {
                                Ok((new_token, new_did)) => {
                                    tracing::info!("Token refresh successful (DID: {})", new_did);
                                    // Update state with new token
                                    {
                                        let mut auth = state.service_auth.write().await;
                                        auth.token = Some(new_token.clone());
                                        auth.did = Some(new_did);
                                    }

                                    // Retry request with new token
                                    match oneyearago::get_feed_skeleton(
                                        &client,
                                        auth_header,
                                        &new_token,
                                        &did,
                                        params.limit.unwrap_or(30),
                                        params.cursor.clone(),
                                    )
                                    .await
                                    {
                                        Ok(res) => Ok(Json(res)),
                                        Err(e2) => {
                                            tracing::error!("Retry failed: {:#}", e2);
                                            Err((
                                                StatusCode::INTERNAL_SERVER_ERROR,
                                                format!("Retry failed: {:#}", e2),
                                            ))
                                        }
                                    }
                                }
                                Err(reauth_err) => {
                                    tracing::error!("Re-authentication failed: {}", reauth_err);
                                    Err((
                                        StatusCode::INTERNAL_SERVER_ERROR,
                                        "Re-authentication failed".to_string(),
                                    ))
                                }
                            }
                        } else {
                            tracing::error!("Cannot refresh token: credentials missing");
                            Err((
                                StatusCode::INTERNAL_SERVER_ERROR,
                                "Credentials missing for refresh".to_string(),
                            ))
                        }
                    } else {
                        tracing::error!("Oneyearago error: {:#}", e);
                        Err((StatusCode::INTERNAL_SERVER_ERROR, format!("{:#}", e)))
                    }
                }
            }
        }
        FeedService::Fakebluesky => {
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
            let result = models::FeedSkeletonResult {
                feed: skeleton.feed.into_iter().map(|item| models::FeedItem {
                    post: item.post,
                }).collect(),
                cursor: skeleton.cursor,
            };

            Ok(Json(result))
        }
    }
}

pub async fn describe_feed_generator(
    State(state): State<SharedState>,
) -> Result<Json<models::DescribeFeedGeneratorResponse>, (StatusCode, String)> {
    let (did, _service_did) = {
        let auth = state.service_auth.read().await;
        // Authenticated Service DID (from .env/auth) or default from context if we hardcoded it?
        // Ideally we use the authenticated DID.
        let did = auth.did.clone().ok_or((
            StatusCode::SERVICE_UNAVAILABLE,
            "Service not authenticated yet".to_string(),
        ))?;
        (did.clone(), did) // logic::service_did
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
        models::FeedUri {
            uri: format!("at://{}/app.bsky.feed.generator/fakebluesky", did),
        },
    ];

    Ok(Json(models::DescribeFeedGeneratorResponse { did, feeds }))
}

#[derive(serde::Serialize)]
pub struct DidResponse {
    #[serde(rename = "@context")]
    pub context: Vec<String>,
    pub id: String,
    pub service: Vec<DidService>,
}

#[derive(serde::Serialize)]
pub struct DidService {
    pub id: String,
    #[serde(rename = "type")]
    pub service_type: String,
    #[serde(rename = "serviceEndpoint")]
    pub service_endpoint: String,
}

pub async fn get_did_json(
    State(_state): State<SharedState>,
) -> Result<Json<DidResponse>, (StatusCode, String)> {
    let hostname = "feeds.bsky.girigiribauer.com";

    let did = format!("did:web:{}", hostname);
    let service_endpoint = format!("https://{}", hostname);

    let response = DidResponse {
        context: vec!["https://www.w3.org/ns/did/v1".to_string()],
        id: did,
        service: vec![DidService {
            id: "#bsky_fg".to_string(),
            service_type: "BskyFeedGenerator".to_string(),
            service_endpoint,
        }],
    };

    Ok(Json(response))
}

pub async fn health() -> &'static str {
    "OK"
}
