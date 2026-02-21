use crate::error::AppError;
use crate::state::{FeedQuery, SharedState};
use axum::response::Json;
use oneyearago::cache::CacheStore;

pub async fn handle_oneyearago(
    state: SharedState,
    headers: axum::http::HeaderMap,
    params: FeedQuery,
) -> Result<Json<bsky_core::FeedSkeletonResult>, AppError> {
    let auth_header = headers
        .get("authorization")
        .and_then(|h| h.to_str().ok())
        .ok_or(AppError::Auth(
            "Missing or invalid authorization header".to_string(),
        ))?;

    // Extract DID from JWT
    let did = bsky_core::extract_did_from_jwt(Some(auth_header))
        .map_err(|_| AppError::Auth("Invalid JWT".to_string()))?;

    // Read client and current token
    let (client, current_token) = {
        let auth = state.service_auth.read().await;
        (state.http_client.clone(), auth.token.clone())
    };

    let token = current_token.ok_or(AppError::Internal(anyhow::anyhow!(
        "Service not authenticated"
    )))?;

    let cache_store = CacheStore::new(state.oneyearago_db.clone());
    let cache = Some(&cache_store);

    let results = match oneyearago::get_feed_skeleton(
        &client,
        auth_header,
        &token,
        &did,
        params.limit.unwrap_or(30),
        params.cursor.clone(),
        cache,
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
                                cache,
                            )
                            .await
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
                tracing::error!("Oneyearago error: {:#}", e);
                Err(AppError::Internal(e))
            }
        }
    };

    // 正常終了した場合のみ、非同期でクリーンアップを実行する
    if results.is_ok() {
        let store_for_cleanup = CacheStore::new(state.oneyearago_db.clone());
        tokio::spawn(async move {
            match store_for_cleanup.cleanup().await {
                Ok(n) if n > 0 => tracing::info!("[cache] Cleaned up {} expired entries", n),
                Ok(_) => {}
                Err(e) => tracing::warn!("[cache] Cleanup error: {}", e),
            }
        });
    }

    results
}
