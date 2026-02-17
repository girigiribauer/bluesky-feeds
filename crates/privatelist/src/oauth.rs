use anyhow::{Context, Result, anyhow};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use p256::ecdsa::SigningKey;
use p256::ecdsa::signature::Signer;
use p256::pkcs8::DecodePrivateKey;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

#[derive(Debug, Serialize, Deserialize)]
pub struct OauthContext {
    pub state: String,
    pub verifier: String,
    pub private_key_pem: String,
}

#[derive(Debug, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: i64,
    pub sub: String, // DID
}

pub struct OauthClient {
    pub client_id: String,
    pub redirect_uri: String,
    pub token_endpoint: String,
    pub http_client: reqwest::Client,
}

impl OauthClient {
    pub fn new(client_id: String, redirect_uri: String) -> Self {
        Self {
            client_id,
            redirect_uri,
            token_endpoint: "https://bsky.social/oauth/token".to_string(), // Default
            http_client: reqwest::Client::new(),
        }
    }

    pub async fn exchange_code(
        &self,
        code: &str,
        verifier: &str,
        private_key_pem: &str,
    ) -> Result<TokenResponse> {
        let params = [
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", &self.redirect_uri),
            ("client_id", &self.client_id),
            ("code_verifier", verifier),
        ];

        self.execute_token_request(&params, private_key_pem).await
    }

    pub async fn refresh_token(
        &self,
        refresh_token: &str,
        private_key_pem: &str,
    ) -> Result<TokenResponse> {
        let params = [
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("client_id", &self.client_id),
        ];

        self.execute_token_request(&params, private_key_pem).await
    }

    async fn execute_token_request(
        &self,
        params: &[(&str, &str)],
        private_key_pem: &str,
    ) -> Result<TokenResponse> {
        let mut nonce: Option<String> = None;
        let mut retry_count = 0;

        loop {
            if retry_count > 1 {
                return Err(anyhow!(
                    "Token request failed: Too many retries for DPoP Nonce"
                ));
            }

            let dpop_proof = create_dpop_proof(
                "POST",
                &self.token_endpoint,
                private_key_pem,
                nonce.as_deref(),
            )?;

            let res = self
                .http_client
                .post(&self.token_endpoint)
                .header("Content-Type", "application/x-www-form-urlencoded")
                .header("DPoP", dpop_proof)
                .form(params)
                .send()
                .await?;

            if res.status().is_success() {
                let body = res.text().await?;
                let token_res: TokenResponse = serde_json::from_str(&body)?;
                return Ok(token_res);
            } else if res.status() == 400 || res.status() == 401 {
                // Check for DPoP Nonce error
                if let Some(new_nonce) = res
                    .headers()
                    .get("DPoP-Nonce")
                    .and_then(|h| h.to_str().ok())
                {
                    tracing::info!("Received DPoP-Nonce, retrying...");
                    nonce = Some(new_nonce.to_string());
                    retry_count += 1;
                    continue;
                }
            }

            let status = res.status();
            let body = res.text().await.unwrap_or_default();
            return Err(anyhow!("Token request failed: {} - {}", status, body));
        }
    }
}

pub fn create_dpop_proof(
    method: &str,
    url: &str,
    private_key_pem: &str,
    nonce: Option<&str>,
) -> Result<String> {
    let signing_key = SigningKey::from_pkcs8_pem(private_key_pem)
        .map_err(|e| anyhow!("Failed to parse private key: {}", e))?;

    let public_key = signing_key.verifying_key();
    let encoded_point = public_key.to_encoded_point(false);
    let x = encoded_point.x().context("Invalid X coordinate")?;
    let y = encoded_point.y().context("Invalid Y coordinate")?;

    let jwk_json = json!({
        "kty": "EC",
        "crv": "P-256",
        "x": URL_SAFE_NO_PAD.encode(x),
        "y": URL_SAFE_NO_PAD.encode(y)
    });

    let header = json!({
        "typ": "dpop+jwt",
        "alg": "ES256",
        "jwk": jwk_json,
    });

    let now = OffsetDateTime::now_utc().unix_timestamp();
    let jti: String = rand::thread_rng()
        .sample_iter(&rand::distributions::Alphanumeric)
        .take(12)
        .map(char::from)
        .collect();

    let mut payload = json!({
        "iat": now,
        "jti": jti,
        "htm": method,
        "htu": url,
    });

    if let Some(n) = nonce {
        payload["nonce"] = json!(n);
    }

    let header_b64 = URL_SAFE_NO_PAD.encode(serde_json::to_string(&header)?);
    let payload_b64 = URL_SAFE_NO_PAD.encode(serde_json::to_string(&payload)?);
    let unsigned_token = format!("{}.{}", header_b64, payload_b64);

    let signature: p256::ecdsa::Signature = signing_key.sign(unsigned_token.as_bytes());
    let signature_b64 = URL_SAFE_NO_PAD.encode(signature.to_bytes());

    Ok(format!("{}.{}", unsigned_token, signature_b64))
}

use rand::Rng;
use serde_json::json;
