use crate::state::{FeedQuery, SharedState};
use axum::{extract::State, http::StatusCode, response::Json};

#[derive(serde::Deserialize)]
pub struct PrivateListTarget {
    pub target: String,
}

pub async fn privatelist_add(
    State(state): State<SharedState>,
    headers: axum::http::HeaderMap,
    Json(payload): Json<PrivateListTarget>,
) -> Result<StatusCode, (StatusCode, String)> {
    let auth_header = headers
        .get("authorization")
        .and_then(|h| h.to_str().ok())
        .ok_or((
            StatusCode::UNAUTHORIZED,
            "Missing authorization header".to_string(),
        ))?;

    let user_did = bsky_core::extract_did_from_jwt(Some(auth_header))
        .map_err(|_| (StatusCode::UNAUTHORIZED, "Invalid JWT".to_string()))?;

    privatelist::add_user(&state.privatelist_db, &user_did, &payload.target)
        .await
        .map_err(|e| {
            tracing::error!("Failed to add user to private list: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Database error".to_string(),
            )
        })?;

    Ok(StatusCode::OK)
}

pub async fn privatelist_remove(
    State(state): State<SharedState>,
    headers: axum::http::HeaderMap,
    Json(payload): Json<PrivateListTarget>,
) -> Result<StatusCode, (StatusCode, String)> {
    let auth_header = headers
        .get("authorization")
        .and_then(|h| h.to_str().ok())
        .ok_or((
            StatusCode::UNAUTHORIZED,
            "Missing authorization header".to_string(),
        ))?;

    let user_did = bsky_core::extract_did_from_jwt(Some(auth_header))
        .map_err(|_| (StatusCode::UNAUTHORIZED, "Invalid JWT".to_string()))?;

    privatelist::remove_user(&state.privatelist_db, &user_did, &payload.target)
        .await
        .map_err(|e| {
            tracing::error!("Failed to remove user from private list: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Database error".to_string(),
            )
        })?;

    Ok(StatusCode::OK)
}

pub async fn privatelist_list(
    State(state): State<SharedState>,
    headers: axum::http::HeaderMap,
) -> Result<Json<Vec<String>>, (StatusCode, String)> {
    let auth_header = headers
        .get("authorization")
        .and_then(|h| h.to_str().ok())
        .ok_or((
            StatusCode::UNAUTHORIZED,
            "Missing authorization header".to_string(),
        ))?;

    let user_did = bsky_core::extract_did_from_jwt(Some(auth_header))
        .map_err(|_| (StatusCode::UNAUTHORIZED, "Invalid JWT".to_string()))?;

    let users = privatelist::list_users(&state.privatelist_db, &user_did)
        .await
        .map_err(|e| {
            tracing::error!("Failed to list users in private list: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Database error".to_string(),
            )
        })?;

    Ok(Json(users))
}

pub async fn privatelist_refresh(
    State(state): State<SharedState>,
    headers: axum::http::HeaderMap,
) -> Result<StatusCode, (StatusCode, String)> {
    let auth_header = headers
        .get("authorization")
        .and_then(|h| h.to_str().ok())
        .ok_or((
            StatusCode::UNAUTHORIZED,
            "Missing authorization header".to_string(),
        ))?;

    let user_did = bsky_core::extract_did_from_jwt(Some(auth_header))
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
    match privatelist::refresh_list(
        &state.privatelist_db,
        &client,
        &state.bsky_api_url,
        &user_did,
        &token,
    )
    .await
    {
        Ok(_) => Ok(StatusCode::OK),
        Err(e) => {
            let err_msg = format!("{:?}", e);
            if err_msg.contains("ExpiredToken")
                || err_msg.contains("401")
                || err_msg.contains("Unauthorized")
            {
                tracing::warn!(
                    "Token expired during refresh, attempting re-auth... ({})",
                    err_msg
                );

                // RE-AUTHENTICATION LOGIC
                let handle = &state.auth_handle;
                let password = &state.auth_password;

                if !handle.is_empty() && !password.is_empty() {
                    match todoapp::authenticate(&client, handle, password).await {
                        Ok((new_token, new_did_service)) => {
                            tracing::info!(
                                "Token refresh successful (Service DID: {})",
                                new_did_service
                            );
                            {
                                let mut auth = state.service_auth.write().await;
                                auth.token = Some(new_token.clone());
                                auth.did = Some(new_did_service);
                            }

                            // Retry with new token
                            match privatelist::refresh_list(
                                &state.privatelist_db,
                                &client,
                                &state.bsky_api_url,
                                &user_did,
                                &new_token,
                            )
                            .await
                            {
                                Ok(_) => Ok(StatusCode::OK),
                                Err(e2) => {
                                    tracing::error!("Retry refresh failed: {:#}", e2);
                                    Err((
                                        StatusCode::INTERNAL_SERVER_ERROR,
                                        format!("Retry refresh failed: {:#}", e2),
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
                tracing::error!("Privatelist refresh error: {:#}", e);
                Err((StatusCode::INTERNAL_SERVER_ERROR, format!("{:#}", e)))
            }
        }
    }
}

pub async fn handle_privatelist(
    state: SharedState,
    headers: axum::http::HeaderMap,
    params: FeedQuery,
) -> Result<Json<bsky_core::FeedSkeletonResult>, (StatusCode, String)> {
    let auth_header = headers
        .get("authorization")
        .and_then(|h| h.to_str().ok())
        .ok_or((
            StatusCode::UNAUTHORIZED,
            "Missing or invalid authorization header".to_string(),
        ))?;

    // Extract DID from JWT
    let did = bsky_core::extract_did_from_jwt(Some(auth_header))
        .map_err(|_| (StatusCode::UNAUTHORIZED, "Invalid JWT".to_string()))?;

    // No need for service token or HTTP client since we read from DB cache now.

    match privatelist::get_feed_skeleton(
        &state.privatelist_db,
        &state.http_client, // Passed but unused
        &did,               // user_did (requester)
        "",                 // service_token unused
        params.cursor.clone(),
        params.limit.unwrap_or(30),
    )
    .await
    {
        Ok(res) => Ok(Json(res)),
        Err(e) => {
            tracing::error!("Privatelist feed generation error: {:#}", e);
            Err((StatusCode::INTERNAL_SERVER_ERROR, format!("{:#}", e)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analytics::UmamiClient;
    use crate::state::{AppState, ServiceAuth};
    use std::sync::Arc;
    use tokio::sync::RwLock;

    async fn create_test_state() -> SharedState {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
        privatelist::migrate(&pool).await.unwrap();

        AppState {
            helloworld: helloworld::State::default(),
            http_client: reqwest::Client::new(),
            service_auth: Arc::new(RwLock::new(ServiceAuth {
                token: Some("test_token".to_string()),
                did: Some("did:plc:test".to_string()),
            })),
            auth_handle: "test_handle".to_string(),
            auth_password: "test_password".to_string(),
            helloworld_db: pool.clone(),
            fakebluesky_db: pool.clone(),
            privatelist_db: pool,
            umami: UmamiClient::new("http://localhost".to_string(), "site_id".to_string(), None),
            bsky_api_url: "https://api.bsky.app".to_string(),
            key: axum_extra::extract::cookie::Key::generate(),
        }
    }

    fn create_auth_headers(did: &str) -> axum::http::HeaderMap {
        use base64::{engine::general_purpose, Engine as _};
        let mut headers = axum::http::HeaderMap::new();
        let payload = serde_json::json!({ "iss": did });
        let payload_b64 =
            general_purpose::URL_SAFE_NO_PAD.encode(serde_json::to_vec(&payload).unwrap());
        let token = format!("Bearer header.{}.signature", payload_b64);
        headers.insert("authorization", token.parse().unwrap());
        headers
    }

    #[sqlx::test]
    async fn test_privatelist_add_remove_list() {
        let state = create_test_state().await;
        let user_did = "did:plc:user1";
        let headers = create_auth_headers(user_did);

        // 1. Initial list should be empty
        let list = privatelist_list(State(state.clone()), headers.clone())
            .await
            .unwrap();
        assert!(list.0.is_empty());

        // 2. Add a user
        let target_did = "did:plc:target1";
        let payload = Json(PrivateListTarget {
            target: target_did.to_string(),
        });
        let status = privatelist_add(State(state.clone()), headers.clone(), payload)
            .await
            .unwrap();
        assert_eq!(status, StatusCode::OK);

        // 3. List should contain target
        let list = privatelist_list(State(state.clone()), headers.clone())
            .await
            .unwrap();
        assert_eq!(list.0.len(), 1);
        assert_eq!(list.0[0], target_did);

        // 4. Remove user
        let payload = Json(PrivateListTarget {
            target: target_did.to_string(),
        });
        let status = privatelist_remove(State(state.clone()), headers.clone(), payload)
            .await
            .unwrap();
        assert_eq!(status, StatusCode::OK);

        // 5. List should be empty again
        let list = privatelist_list(State(state.clone()), headers.clone())
            .await
            .unwrap();
        assert!(list.0.is_empty());
    }
}
