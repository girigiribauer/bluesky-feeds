use crate::error::AppError;
use crate::state::{FeedQuery, SharedState};
use axum::{
    async_trait,
    extract::{FromRequestParts, State},
    http::{request::Parts, StatusCode},
    response::{IntoResponse, Json},
};
use axum_extra::extract::cookie::SignedCookieJar;
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub struct PrivateListTarget {
    pub target: String,
}

#[derive(Serialize)]
pub struct WhoAmIResponse {
    pub did: String,
}

pub struct AuthenticatedUser(pub String);

#[async_trait]
impl FromRequestParts<SharedState> for AuthenticatedUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &SharedState,
    ) -> Result<Self, Self::Rejection> {
        // 1. Check Cookie
        let jar = SignedCookieJar::from_headers(&parts.headers, state.key.clone());
        if let Some(cookie) = jar.get("privatelist_session") {
            let session_id: &str = cookie.value();
            if let Some(mut session) =
                privatelist::get_session(&state.privatelist_db, session_id).await?
            {
                // Auto-refresh if needed
                refresh_token_if_needed(&state.privatelist_db, &mut session, &state.config).await?;
                return Ok(AuthenticatedUser(session.did));
            }
        }

        // 2. Check Header
        if let Some(auth_header) = parts
            .headers
            .get("authorization")
            .and_then(|h| h.to_str().ok())
        {
            if let Ok(did) = bsky_core::extract_did_from_jwt(Some(auth_header)) {
                return Ok(AuthenticatedUser(did));
            }
        }

        Err(AppError::Auth(
            "Missing or invalid authorization".to_string(),
        ))
    }
}

// Helper: Authenticate via Cookie (+ Refresh) OR Header (Old version - keeping for compatibility if needed, but extractor is preferred)
#[allow(dead_code)]
async fn authenticate_user(
    jar: &SignedCookieJar,
    headers: &axum::http::HeaderMap,
    state: &SharedState,
) -> Result<String, AppError> {
    // 1. Try Cookie (Session)
    if let Some(cookie) = jar.get("privatelist_session") {
        let session_id = cookie.value();
        if let Some(mut session) =
            privatelist::get_session(&state.privatelist_db, session_id).await?
        {
            refresh_token_if_needed(&state.privatelist_db, &mut session, &state.config).await?;
            return Ok(session.did);
        }
    }

    // 2. Try Header (Bearer JWT)
    if let Some(auth_header) = headers.get("authorization").and_then(|h| h.to_str().ok()) {
        if let Ok(did) = bsky_core::extract_did_from_jwt(Some(auth_header)) {
            return Ok(did);
        }
    }

    Err(AppError::Auth(
        "Missing or invalid authorization".to_string(),
    ))
}

pub async fn privatelist_me(user: AuthenticatedUser) -> impl IntoResponse {
    Json(WhoAmIResponse { did: user.0 })
}

pub async fn privatelist_add(
    user: AuthenticatedUser,
    State(state): State<SharedState>,
    Json(payload): Json<PrivateListTarget>,
) -> Result<StatusCode, AppError> {
    privatelist::add_user(&state.privatelist_db, &user.0, &payload.target).await?;

    Ok(StatusCode::OK)
}

pub async fn privatelist_remove(
    user: AuthenticatedUser,
    State(state): State<SharedState>,
    Json(payload): Json<PrivateListTarget>,
) -> Result<StatusCode, AppError> {
    privatelist::remove_user(&state.privatelist_db, &user.0, &payload.target).await?;

    Ok(StatusCode::OK)
}

pub async fn privatelist_list(
    user: AuthenticatedUser,
    State(state): State<SharedState>,
) -> Result<Json<Vec<String>>, AppError> {
    let users = privatelist::list_users(&state.privatelist_db, &user.0).await?;

    Ok(Json(users))
}

pub async fn privatelist_refresh(
    user: AuthenticatedUser,
    State(state): State<SharedState>,
) -> Result<StatusCode, AppError> {
    // Read client and current token
    let (client, current_token) = {
        let auth = state.service_auth.read().await;
        (state.http_client.clone(), auth.token.clone())
    };

    let token = current_token.ok_or(AppError::BadRequest(
        "Service not authenticated".to_string(),
    ))?;

    // First attempt
    match privatelist::refresh_list(
        &state.privatelist_db,
        &client,
        &state.config.bsky_api_url,
        &user.0,
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
                                &state.config.bsky_api_url,
                                &user.0,
                                &new_token,
                            )
                            .await
                            {
                                Ok(_) => Ok(StatusCode::OK),
                                Err(e2) => {
                                    tracing::error!("Retry refresh failed: {:#}", e2);
                                    Err(AppError::Internal(anyhow::anyhow!(
                                        "Retry refresh failed: {:#}",
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
                tracing::error!("Privatelist refresh error: {:#}", e);
                Err(AppError::Internal(e))
            }
        }
    }
}

pub async fn handle_privatelist(
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

    let res = privatelist::get_feed_skeleton(
        &state.privatelist_db,
        &state.http_client, // Passed but unused
        &did,               // user_did (requester)
        "",                 // service_token unused
        params.cursor.clone(),
        params.limit.unwrap_or(30),
    )
    .await?;

    Ok(Json(res))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analytics::UmamiClient;
    use crate::state::{AppState, ServiceAuth};
    use std::sync::Arc;
    use tokio::sync::RwLock;

    async fn create_test_state() -> SharedState {
        use crate::state::AppConfig;
        let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
        privatelist::migrate(&pool).await.unwrap();

        AppState {
            config: AppConfig {
                privatelist_url: "http://localhost:3000".to_string(),
                bsky_api_url: "https://api.bsky.app".to_string(),
                client_id: "http://localhost:3000/client-metadata.json".to_string(),
                redirect_uri: "http://localhost:3000/oauth/callback".to_string(),
            },
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
        let _headers = create_auth_headers(user_did);
        let _jar = SignedCookieJar::new(state.key.clone()); // Mock Jar

        // 1. Initial list should be empty
        let list = privatelist_list(
            AuthenticatedUser(user_did.to_string()),
            State(state.clone()),
        )
        .await
        .unwrap();
        assert!(list.0.is_empty());

        // 2. Add a user
        let target_did = "did:plc:target1";
        let payload = Json(PrivateListTarget {
            target: target_did.to_string(),
        });
        let status = privatelist_add(
            AuthenticatedUser(user_did.to_string()),
            State(state.clone()),
            payload,
        )
        .await
        .unwrap();
        assert_eq!(status, StatusCode::OK);

        // 3. List should contain target
        let list = privatelist_list(
            AuthenticatedUser(user_did.to_string()),
            State(state.clone()),
        )
        .await
        .unwrap();
        assert_eq!(list.0.len(), 1);
        assert_eq!(list.0[0], target_did);

        // 4. Remove user
        let payload = Json(PrivateListTarget {
            target: target_did.to_string(),
        });
        let status = privatelist_remove(
            AuthenticatedUser(user_did.to_string()),
            State(state.clone()),
            payload,
        )
        .await
        .unwrap();
        assert_eq!(status, StatusCode::OK);

        // 5. List should be empty again
        let list = privatelist_list(
            AuthenticatedUser(user_did.to_string()),
            State(state.clone()),
        )
        .await
        .unwrap();
        assert!(list.0.is_empty());
    }
}

pub async fn refresh_token_if_needed(
    pool: &sqlx::SqlitePool,
    session: &mut privatelist::Session,
    config: &crate::state::AppConfig,
) -> anyhow::Result<String> {
    let now = time::OffsetDateTime::now_utc().unix_timestamp();
    // Refresh if expired or expiring in less than 5 minutes
    if session.expires_at > now + 300 {
        return Ok(session.access_token.clone());
    }

    tracing::info!("Access Token expired or expiring soon, refreshing...");

    let client_id = config.client_id.clone();
    let redirect_uri = config.redirect_uri.clone();

    let oauth_client = privatelist::oauth::OauthClient::new(client_id, redirect_uri);
    let token_res = oauth_client
        .refresh_token(&session.refresh_token, &session.dpop_private_key)
        .await?;

    // Update Session
    session.access_token = token_res.access_token;
    if !token_res.refresh_token.is_empty() {
        session.refresh_token = token_res.refresh_token;
    }
    session.expires_at = time::OffsetDateTime::now_utc().unix_timestamp() + token_res.expires_in;

    privatelist::update_session(pool, session).await?;

    tracing::info!("Session Refreshed Successfully");
    Ok(session.access_token.clone())
}
