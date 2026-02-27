use crate::error::AppError;
use crate::state::{FeedQuery, SharedState};
use axum::response::Json;

pub async fn handle_todoapp(
    state: SharedState,
    headers: axum::http::HeaderMap,
    _params: FeedQuery,
) -> Result<Json<bsky_core::FeedSkeletonResult>, AppError> {
    let auth_header = headers
        .get("authorization")
        .and_then(|h| h.to_str().ok())
        .ok_or(AppError::Auth(
            "Missing or invalid authorization header".to_string(),
        ))?;

    // Read client and current token
    let (client, current_token) = {
        let auth = state.service_auth.read().await;
        (state.http_client.clone(), auth.token.clone())
    };

    let token = current_token.ok_or(AppError::Internal(anyhow::anyhow!(
        "Service not authenticated"
    )))?;

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
                            match todoapp::get_feed_skeleton(&client, auth_header, &new_token).await
                            {
                                Ok(res) => Ok(Json(res)),
                                Err(e2) => {
                                    tracing::error!("Retry failed: {:#}", e2);
                                    Err(AppError::Internal(anyhow::anyhow!(
                                        "Retry failed: {:#}",
                                        e2
                                    )))
                                }
                            }
                        }
                        Err(reauth_err) => {
                            tracing::error!("Re-authentication failed: {}", reauth_err);
                            Err(AppError::Internal(anyhow::anyhow!(
                                "Re-authentication failed"
                            )))
                        }
                    }
                } else {
                    tracing::error!("Cannot refresh token: credentials missing");
                    Err(AppError::BadRequest(
                        "Credentials missing for refresh".to_string(),
                    ))
                }
            } else {
                // Other error
                tracing::error!("Todoapp error: {:#}", e);
                Err(AppError::Internal(e))
            }
        }
    }
}
