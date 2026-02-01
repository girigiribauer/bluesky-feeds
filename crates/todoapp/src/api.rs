use crate::structs::{JwtPayload, PostView, SearchResponse, SessionResponse};
use anyhow::{Context, Result};
use base64::{engine::general_purpose, Engine as _};
use reqwest::Client;

pub async fn authenticate(client: &Client, handle: &str, password: &str) -> Result<(String, String)> {
    let url = "https://bsky.social/xrpc/com.atproto.server.createSession";
    let body = serde_json::json!({
        "identifier": handle,
        "password": password,
    });

    let res = client
        .post(url)
        .json(&body)
        .send()
        .await
        .context("Failed to send auth request")?;

    if !res.status().is_success() {
        let status = res.status();
        let text = res.text().await.unwrap_or_default();
        anyhow::bail!("Auth failed: {} - {}", status, text);
    }

    let session: SessionResponse = res.json().await.context("Failed to parse auth response")?;
    Ok((session.access_jwt, session.did))
}

pub async fn search_posts(client: &Client, q: &str, author_did: &str, service_token: &str) -> Result<Vec<PostView>> {
    // Authenticated API request using Service Token
    let url = "https://api.bsky.app/xrpc/app.bsky.feed.searchPosts";
    let query_param = format!("{}", q); // q parameter

    let res = client
        .get(url)
        .header("Authorization", format!("Bearer {}", service_token))
        .query(&[
            ("q", query_param.as_str()),
            ("limit", "100"),
            ("author", author_did),
            ("sort", "latest"),
        ])
        .send()
        .await
        .context("Failed to send search request")?;

    if !res.status().is_success() {
        let status = res.status();
        let text = res.text().await.unwrap_or_default();
        anyhow::bail!("Search API failed: {} - {}", status, text);
    }

    let search_res: SearchResponse = res.json().await.context("Failed to parse search response")?;
    Ok(search_res.posts)
}

pub fn extract_did_from_jwt(header: &str) -> Result<String> {
    let parts: Vec<&str> = header.split_whitespace().collect();
    if parts.len() != 2 || parts[0] != "Bearer" {
        anyhow::bail!("Invalid Authorization header format");
    }
    let jwt = parts[1];
    let components: Vec<&str> = jwt.split('.').collect();
    if components.len() != 3 {
        anyhow::bail!("Invalid JWT format");
    }
    let payload_part = components[1];

    let decoded = general_purpose::URL_SAFE_NO_PAD
        .decode(payload_part)
        .or_else(|_| general_purpose::URL_SAFE.decode(payload_part))
        .context("Failed to decode JWT payload")?;

    let payload: JwtPayload = serde_json::from_slice(&decoded).context("Failed to parse JWT payload")?;
    Ok(payload.iss)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_did_from_jwt() {
        // Mock a simple JWT (header.payload.signature)
        // Payload: {"iss": "did:plc:12345", ...}
        // Base64Url for payload: eyJpc3MiOiJkaWQ6cGxjOjEyMzQ1In0 ({"iss":"did:plc:12345"})

        let valid_header = "Bearer header.eyJpc3MiOiJkaWQ6cGxjOjEyMzQ1In0.signature";
        let did = extract_did_from_jwt(valid_header).expect("Should parse valid JWT");
        assert_eq!(did, "did:plc:12345");

        let invalid_format = "Basic auth";
        assert!(extract_did_from_jwt(invalid_format).is_err());

        let invalid_jwt = "Bearer invalid.jwt";
        assert!(extract_did_from_jwt(invalid_jwt).is_err());
    }
}
