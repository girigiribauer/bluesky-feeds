use axum::{extract::Query, response::IntoResponse, Json};
use axum_extra::extract::cookie::{Cookie, SameSite, SignedCookieJar};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use rand::Rng;
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::env;
use time::Duration;

fn get_privatelist_url() -> String {
    env::var("PRIVATELIST_URL")
        .unwrap_or_else(|_| "https://feeds.bsky.girigiribauer.com".to_string())
}

pub async fn client_metadata() -> impl IntoResponse {
    let base_url = get_privatelist_url();
    let client_id = format!("{}/client-metadata.json", base_url);
    let redirect_uri = format!("{}/oauth/callback", base_url);

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
        "dpop_bound_access_tokens": false
    });

    Json(metadata)
}

pub async fn login(jar: SignedCookieJar) -> impl IntoResponse {
    tracing::info!("Login request via Signed Cookie");

    // 1. Generate State and Code Verifier
    let state: String = rand::thread_rng()
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

    // 3. Store in *Signed* Cookie (Stateless)
    // We combine state and verifier potentially, or store separate cookies.
    // Storing check:
    let cookie_val = serde_json::json!({
        "state": state,
        "verifier": code_verifier
    })
    .to_string();

    let mut cookie = Cookie::new("oauth_context", cookie_val);
    cookie.set_http_only(false); // TEMPORARY: Allow JS to read for debugging
    cookie.set_secure(true);
    cookie.set_same_site(SameSite::Lax); // Standard for top-level nav
    cookie.set_path("/");
    cookie.set_max_age(Duration::minutes(10)); // Short lived

    let jar = jar.add(cookie);

    // 4. Construct Redirect URL
    let base_url = get_privatelist_url();
    let client_id = format!("{}/client-metadata.json", base_url);
    let redirect_uri = format!("{}/oauth/callback", base_url);

    let auth_url = format!(
        "https://bsky.social/oauth/authorize?client_id={}&response_type=code&redirect_uri={}&scope={}&state={}&code_challenge={}&code_challenge_method=S256",
        urlencoding::encode(&client_id),
        urlencoding::encode(&redirect_uri),
        urlencoding::encode("atproto transition:generic"),
        urlencoding::encode(&state),
        urlencoding::encode(&code_challenge)
    );

    // 5. Return Bounce Page (HTML)
    let html = format!(
        r##"
<!DOCTYPE html>
<html>
<head>
    <title>Redirecting...</title>
    <!-- <meta http-equiv="refresh" content="2; url={0}"> -->
</head>
<body>
    <h1>Logging in...</h1>
    <p>Please wait while we redirect you to Bluesky.</p>

    <div style="border: 1px solid #ccc; padding: 1em; background: #eee; font-family: monospace;">
        <strong>Debug Info:</strong><br>
        Cookies visible to JS: <span id="cookies">Checking...</span>
    </div>

    <p>If you are not redirected automatically within a few seconds, <a href="{0}" id="link">click here</a>.</p>

    <script>
        const cookies = document.cookie;
        document.getElementById('cookies').innerText = cookies || "(None)";

        console.log("Redirecting to {0}");

        if (cookies.includes("oauth_context")) {{
            document.body.style.backgroundColor = "#eaffea";
            setTimeout(() => {{
                window.location.href = "{0}";
            }}, 2000);
        }} else {{
            document.body.style.backgroundColor = "#ffcccc";
            alert("Error: Cookie not set! Please check console.");
        }}
    </script>
</body>
</html>
"##,
        auth_url
    );

    (jar, axum::response::Html(html))
}

#[derive(Deserialize, Debug)]
pub struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    _iss: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

#[derive(Deserialize)]
struct OauthContext {
    state: String,
    verifier: String,
}

pub async fn callback(
    jar: SignedCookieJar,
    headers: axum::http::HeaderMap,
    Query(params): Query<CallbackQuery>,
) -> impl IntoResponse {
    tracing::info!("Received callback params: {:?}", params);
    tracing::info!("Received headers: {:?}", headers);

    // Retrieve Cookie
    let cookie = jar.get("oauth_context");
    if cookie.is_none() {
        tracing::error!(
            "Missing oauth_context cookie. Jar keys: {:?}",
            jar.iter().map(|c| c.name().to_string()).collect::<Vec<_>>()
        );
        return "Session expired (Cookie not found)".into_response();
    }
    let cookie_val = cookie.unwrap().value().to_string();
    let context: OauthContext = match serde_json::from_str(&cookie_val) {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("Failed to parse oauth cookie: {}", e);
            return "Invalid session data".into_response();
        }
    };

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

    // 1. Verify State
    if context.state != *state_param {
        tracing::error!(
            "Invalid OAuth state. Received: {}, Stored: {}",
            state_param,
            context.state
        );
        return "Invalid state".into_response();
    }

    // 2. Exchange Code for Token
    tracing::info!("Exchanging Auth Code for Token...");

    let base_url = get_privatelist_url();
    let client_id = format!("{}/client-metadata.json", base_url);
    let redirect_uri = format!("{}/oauth/callback", base_url);
    let token_endpoint = "https://bsky.social/oauth/token";

    let client = reqwest::Client::new();
    let token_params = [
        ("grant_type", "authorization_code"),
        ("code", code),
        ("redirect_uri", &redirect_uri),
        ("client_id", &client_id),
        ("code_verifier", &context.verifier),
    ];

    let res = client
        .post(token_endpoint)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .form(&token_params)
        .send()
        .await;

    // Clear the oauth cookie after use
    let jar = jar.remove(Cookie::from("oauth_context"));

    match res {
        Ok(response) => {
            if response.status().is_success() {
                let body = response.text().await.unwrap_or_default();
                tracing::info!("Token Exchange Successful: {}", body);

                // TODO: Store tokens in a new persistent session cookie
                (jar, format!("Login Successful! \n\nResponse: {}", body)).into_response()
            } else {
                let status = response.status();
                let body = response.text().await.unwrap_or_default();
                tracing::error!("Token Exchange Failed: {} - {}", status, body);
                (
                    jar,
                    format!("Token Exchange Failed: {} \n\n {}", status, body),
                )
                    .into_response()
            }
        }
        Err(e) => {
            tracing::error!("Request Failed: {}", e);
            (jar, format!("Request Failed: {}", e)).into_response()
        }
    }
}
