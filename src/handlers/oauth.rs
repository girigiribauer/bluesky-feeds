use axum::{
    extract::{Query, State},
    response::IntoResponse,
    Json,
};
use axum_extra::extract::cookie::{Cookie, SameSite, SignedCookieJar};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use rand::Rng;
use serde_json::json;
use sha2::{Digest, Sha256};
use time::{Duration, OffsetDateTime};

use p256::ecdsa::SigningKey;
use p256::pkcs8::EncodePrivateKey;
use rand::rngs::OsRng;
use serde::Deserialize;

use crate::state::SharedState;
use privatelist::oauth::{OauthClient, OauthContext};

pub async fn client_metadata(State(state): State<SharedState>) -> impl IntoResponse {
    let base_url = &state.config.privatelist_url;
    let client_id = &state.config.client_id;
    let redirect_uri = &state.config.redirect_uri;

    let metadata = json!({
        "client_id": client_id,
        "client_name": "Bluesky Private List",
        "client_uri": base_url,
        "logo_uri": format!("{}/logo.png", base_url),
        "redirect_uris": [
            redirect_uri
        ],
        "scope": "atproto transition:generic",
        "grant_types": ["authorization_code", "refresh_token"],
        "response_types": ["code"],
        "application_type": "web",
        "token_endpoint_auth_method": "none",
        "dpop_bound_access_tokens": true
    });

    Json(metadata)
}

pub async fn login(jar: SignedCookieJar, State(state): State<SharedState>) -> impl IntoResponse {
    tracing::info!("Login request: Generating OAuth authorize URL and redirecting...");

    // 1. Generate OAuth State and Code Verifier
    let oauth_state: String = rand::thread_rng()
        .sample_iter(&rand::distributions::Alphanumeric)
        .take(32)
        .map(char::from)
        .collect();

    let code_verifier: String = rand::thread_rng()
        .sample_iter(&rand::distributions::Alphanumeric)
        .take(43)
        .map(char::from)
        .collect();

    // 2. Calculate Code Challenge (S256)
    let mut hasher = Sha256::new();
    hasher.update(code_verifier.as_bytes());
    let code_challenge = URL_SAFE_NO_PAD.encode(hasher.finalize());

    // 3. Generate DPoP Key Pair (P-256)
    let signing_key = SigningKey::random(&mut OsRng);
    let private_key_pem = signing_key
        .to_pkcs8_pem(p256::pkcs8::LineEnding::LF)
        .unwrap()
        .to_string();

    // 4. Store in *Signed* Cookie (Stateless)
    let cookie_val = serde_json::to_string(&OauthContext {
        state: oauth_state.clone(),
        verifier: code_verifier,
        private_key_pem,
    })
    .unwrap();

    let mut cookie = Cookie::new("oauth_context", cookie_val);
    cookie.set_http_only(true);
    cookie.set_secure(true);
    cookie.set_same_site(SameSite::Lax);
    cookie.set_path("/");
    cookie.set_max_age(Duration::minutes(10));

    let jar = jar.add(cookie);

    // 5. Construct Redirect URL
    let client_id = &state.config.client_id;
    let redirect_uri = &state.config.redirect_uri;

    let auth_url = format!(
        "https://bsky.social/oauth/authorize?client_id={}&response_type=code&redirect_uri={}&scope={}&state={}&code_challenge={}&code_challenge_method=S256",
        urlencoding::encode(client_id),
        urlencoding::encode(redirect_uri),
        urlencoding::encode("atproto transition:generic"),
        urlencoding::encode(&oauth_state),
        urlencoding::encode(&code_challenge)
    );

    (jar, axum::response::Redirect::to(&auth_url))
}

#[derive(Deserialize, Debug)]
pub struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    _iss: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

pub async fn callback(
    jar: SignedCookieJar,
    State(state): State<SharedState>,
    _headers: axum::http::HeaderMap,
    Query(params): Query<CallbackQuery>,
) -> impl IntoResponse {
    tracing::info!("Received callback params: {:?}", params);

    // 1. Retrieve & Parse Cookie
    let cookie = match jar.get("oauth_context") {
        Some(c) => c,
        None => return "Session expired (Cookie not found)".into_response(),
    };

    let context: OauthContext = match serde_json::from_str(cookie.value()) {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("Failed to parse oauth cookie: {}", e);
            return "Invalid session data".into_response();
        }
    };

    // 2. Check for OAuth errors
    if let Some(error) = &params.error {
        let desc = params
            .error_description
            .as_deref()
            .unwrap_or("No description");
        tracing::error!("OAuth Error: {} - {}", error, desc);
        return format!("OAuth Error: {} - {}", error, desc).into_response();
    }

    let code = match &params.code {
        Some(c) => c,
        None => return "Missing code in callback".into_response(),
    };

    let state_param = match &params.state {
        Some(s) => s,
        None => return "Missing state in callback".into_response(),
    };

    // 3. Verify State
    if context.state != *state_param {
        tracing::error!(
            "Invalid OAuth state. Received: {}, Stored: {}",
            state_param,
            context.state
        );
        return "Invalid state".into_response();
    }

    // 4. Exchange Code for Token using Library
    let client_id = state.config.client_id.clone();
    let redirect_uri = state.config.redirect_uri.clone();

    let oauth_client = OauthClient::new(client_id, redirect_uri);

    let token_res = match oauth_client
        .exchange_code(code, &context.verifier, &context.private_key_pem)
        .await
    {
        Ok(res) => res,
        Err(e) => {
            tracing::error!("Token exchange failed: {}", e);
            let jar = jar.remove(Cookie::from("oauth_context"));
            return (jar, format!("Token Exchange Failed: {}", e)).into_response();
        }
    };

    // 5. Create Session in DB
    let session_bytes: Vec<u8> = rand::thread_rng()
        .sample_iter(&rand::distributions::Alphanumeric)
        .take(32)
        .collect();
    let session_id = URL_SAFE_NO_PAD.encode(session_bytes);

    let session = privatelist::Session {
        session_id: session_id.clone(),
        did: token_res.sub,
        access_token: token_res.access_token,
        refresh_token: token_res.refresh_token,
        dpop_private_key: context.private_key_pem.clone(),
        expires_at: OffsetDateTime::now_utc().unix_timestamp() + token_res.expires_in,
    };

    if let Err(e) = privatelist::create_session(&state.privatelist_db, &session).await {
        tracing::error!("Failed to create session: {}", e);
        return "Login failed: Database error".into_response();
    };

    // 6. Set Session Cookie & Clear OAuth context
    let mut cookie = Cookie::new("privatelist_session", session_id);
    cookie.set_http_only(true);
    cookie.set_secure(true);
    cookie.set_same_site(SameSite::Lax);
    cookie.set_path("/");
    cookie.set_max_age(Duration::days(30));

    let jar = jar.add(cookie);
    let jar = jar.remove(Cookie::from("oauth_context"));

    (jar, axum::response::Redirect::to("/")).into_response()
}

pub async fn logout(
    jar: SignedCookieJar,
    State(state): State<crate::state::SharedState>,
) -> impl IntoResponse {
    if let Some(cookie) = jar.get("privatelist_session") {
        let session_id = cookie.value();
        if let Err(e) = privatelist::delete_session(&state.privatelist_db, session_id).await {
            tracing::error!("Failed to delete session: {}", e);
        }
    }

    let jar = jar.remove(Cookie::from("privatelist_session"));
    (jar, axum::response::Redirect::to("/"))
}
