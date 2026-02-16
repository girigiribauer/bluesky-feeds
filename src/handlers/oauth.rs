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
use std::env;
use time::{Duration, OffsetDateTime};

use p256::ecdsa::signature::Signer;
use p256::ecdsa::SigningKey;
use p256::pkcs8::{DecodePrivateKey, EncodePrivateKey};
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};

use crate::state::SharedState;

fn get_privatelist_url() -> String {
    env::var("PRIVATELIST_URL")
        .unwrap_or_else(|_| "https://privatelist.bsky.girigiribauer.com".to_string())
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
        "dpop_bound_access_tokens": true
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

    // 3. Generate DPoP Key Pair (P-256)
    let signing_key = SigningKey::random(&mut OsRng);
    let private_key_pem = signing_key
        .to_pkcs8_pem(p256::pkcs8::LineEnding::LF)
        .unwrap()
        .to_string();

    // 4. Store in *Signed* Cookie (Stateless)
    // We combine state, verifier, and private key.
    let cookie_val = serde_json::to_string(&OauthContext {
        state: state.clone(),
        verifier: code_verifier,
        private_key_pem,
    })
    .unwrap();

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

fn create_dpop_proof(
    method: &str,
    url: &str,
    private_key_pem: &str,
    nonce: Option<&str>,
) -> Result<String, Box<dyn std::error::Error>> {
    // use jwt_simple::prelude::*; // Conflict with p256::SigningKey

    let signing_key = SigningKey::from_pkcs8_pem(private_key_pem)?;
    let verifying_key = signing_key.verifying_key();

    // Manually construct JWK using to_encoded_point (Standard P-256)
    let encoded_point = verifying_key.to_encoded_point(false);
    let x = encoded_point.x().ok_or("Invalid X coordinate")?;
    let y = encoded_point.y().ok_or("Invalid Y coordinate")?;

    let public_key_json = json!({
        "kty": "EC",
        "crv": "P-256",
        "x": URL_SAFE_NO_PAD.encode(x),
        "y": URL_SAFE_NO_PAD.encode(y)
    });

    // jwt-simple doesn't support raw ES256 with custom headers easily for DPoP "jwk" header field requirements in some versions,
    // but let's try to construct it manually or use a library feature if available.
    // Actually, let's use the 'p256' crate capabilities combined with base64/serde for a manual JWT construction to be sure we match the spec exactly.
    // DPoP requires:
    // Header: { "typ": "dpop+jwt", "alg": "ES256", "jwk": { ... } }
    // Payload: { "jti": "...", "htm": "...", "htu": "...", "iat": ..., "nonce": ... }

    let header = json!({
        "typ": "dpop+jwt",
        "alg": "ES256",
        "jwk": public_key_json
    });

    let jti: String = rand::thread_rng()
        .sample_iter(&rand::distributions::Alphanumeric)
        .take(20)
        .map(char::from)
        .collect();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();

    let mut payload = json!({
        "jti": jti,
        "htm": method,
        "htu": url,
        "iat": now
    });

    if let Some(n) = nonce {
        payload
            .as_object_mut()
            .unwrap()
            .insert("nonce".to_string(), json!(n));
    }

    let header_b64 = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&header)?);
    let payload_b64 = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&payload)?);
    let message = format!("{}.{}", header_b64, payload_b64);

    let signature: p256::ecdsa::Signature = signing_key.try_sign(message.as_bytes())?;
    let signature_b64 = URL_SAFE_NO_PAD.encode(signature.to_bytes());

    Ok(format!("{}.{}", message, signature_b64))
}

#[derive(Deserialize, Debug)]
pub struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    _iss: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

#[derive(Deserialize, Serialize)]
struct OauthContext {
    state: String,
    verifier: String,
    private_key_pem: String,
}

pub async fn callback(
    jar: SignedCookieJar,
    State(state): State<SharedState>,
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

    // Retry loop for DPoP Nonce
    let mut nonce: Option<String> = None;
    let mut retry_count = 0;

    loop {
        if retry_count > 1 {
            return "Token exchange failed: Too many retries".into_response();
        }

        let dpop_proof = create_dpop_proof(
            "POST",
            token_endpoint,
            &context.private_key_pem,
            nonce.as_deref(),
        )
        .unwrap_or_else(|e| {
            tracing::error!("Failed to create DPoP proof: {}", e);
            "error".to_string()
        });

        let res = client
            .post(token_endpoint)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .header("DPoP", dpop_proof)
            .form(&token_params)
            .send()
            .await;

        match res {
            Ok(response) => {
                if response.status().is_success() {
                    let body = response.text().await.unwrap_or_default();
                    tracing::info!("Token Exchange Successful: {}", body);

                    // Parse response
                    let token_res: serde_json::Value =
                        serde_json::from_str(&body).unwrap_or_default();
                    let access_token = token_res["access_token"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string();
                    let refresh_token = token_res["refresh_token"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string();
                    let expires_in = token_res["expires_in"].as_i64().unwrap_or(3600);
                    let sub = token_res["sub"].as_str().unwrap_or_default().to_string(); // DID

                    if sub.is_empty() {
                        return "Login failed: Missing DID in response".into_response();
                    }

                    // Generate Session ID
                    let session_bytes: Vec<u8> = rand::thread_rng()
                        .sample_iter(&rand::distributions::Alphanumeric)
                        .take(32)
                        .collect();
                    let session_id = URL_SAFE_NO_PAD.encode(session_bytes);

                    // Create Session
                    let session = privatelist::Session {
                        session_id: session_id.clone(),
                        did: sub,
                        access_token,
                        refresh_token,
                        dpop_private_key: context.private_key_pem.clone(),
                        expires_at: OffsetDateTime::now_utc().unix_timestamp() + expires_in,
                    };

                    if let Err(e) =
                        privatelist::create_session(&state.privatelist_db, &session).await
                    {
                        tracing::error!("Failed to create session: {}", e);
                        return "Login failed: Database error".into_response();
                    };

                    // Set Cookie
                    let mut cookie = Cookie::new("privatelist_session", session_id);
                    cookie.set_http_only(true);
                    cookie.set_secure(true);
                    cookie.set_same_site(SameSite::Lax);
                    cookie.set_path("/");
                    cookie.set_max_age(Duration::days(30));

                    let jar = jar.add(cookie);
                    // Clear OAuth context
                    let jar = jar.remove(Cookie::from("oauth_context"));

                    return (jar, axum::response::Redirect::to("/")).into_response();
                } else if response.status() == 400 || response.status() == 401 {
                    // Check for DPoP Nonce error
                    if let Some(new_nonce) = response.headers().get("DPoP-Nonce") {
                        if let Ok(n) = new_nonce.to_str() {
                            tracing::info!("Received DPoP-Nonce: {}, retrying...", n);
                            nonce = Some(n.to_string());
                            retry_count += 1;
                            continue;
                        }
                    }
                    // Fallthrough if no nonce or parsing failed
                    let status = response.status();
                    let body = response.text().await.unwrap_or_default();
                    tracing::error!(
                        "Token Exchange Failed (No Nonce Retry): {} - {}",
                        status,
                        body
                    );
                    let jar = jar.remove(Cookie::from("oauth_context"));
                    return (
                        jar,
                        format!("Token Exchange Failed: {} \n\n {}", status, body),
                    )
                        .into_response();
                } else {
                    let status = response.status();
                    let body = response.text().await.unwrap_or_default();
                    tracing::error!("Token Exchange Failed: {} - {}", status, body);
                    let jar = jar.remove(Cookie::from("oauth_context"));
                    return (
                        jar,
                        format!("Token Exchange Failed: {} \n\n {}", status, body),
                    )
                        .into_response();
                }
            }
            Err(e) => {
                tracing::error!("Request Failed: {}", e);
                let jar = jar.remove(Cookie::from("oauth_context"));
                return (jar, format!("Request Failed: {}", e)).into_response();
            }
        }
    }
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
