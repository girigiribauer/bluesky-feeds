use crate::state::{FeedQuery, SharedState};
use axum::{http::StatusCode, response::Json};

pub async fn handle_oneyearago(
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
