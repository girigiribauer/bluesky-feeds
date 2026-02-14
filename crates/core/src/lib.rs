use serde::{Deserialize, Serialize};

/// フィードスケルトンのレスポンス型
#[derive(Debug, Serialize, Deserialize)]
pub struct FeedSkeletonResult {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    pub feed: Vec<FeedItem>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FeedItem {
    pub post: String,
}

/// フィードサービス名の列挙型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeedService {
    Helloworld,
    Todoapp,
    Oneyearago,
    Fakebluesky,
    Privatelist,
}

impl FeedService {
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "helloworld" => Some(Self::Helloworld),
            "todoapp" => Some(Self::Todoapp),
            "oneyearago" => Some(Self::Oneyearago),
            "fakebluesky" => Some(Self::Fakebluesky),
            "privatelist" => Some(Self::Privatelist),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Helloworld => "helloworld",
            Self::Todoapp => "todoapp",
            Self::Oneyearago => "oneyearago",
            Self::Fakebluesky => "fakebluesky",
            Self::Privatelist => "privatelist",
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DescribeFeedGeneratorResponse {
    pub did: String,
    pub feeds: Vec<FeedUri>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FeedUri {
    pub uri: String,
}

#[derive(Debug, Deserialize)]
struct JwtPayload {
    iss: String,
}

pub fn extract_did_from_jwt(header: Option<&str>) -> anyhow::Result<String> {
    use anyhow::Context;
    use base64::{engine::general_purpose, Engine as _};

    let header = header.context("Missing Authorization header")?;

    let parts: Vec<&str> = header.split_whitespace().collect();
    if parts.len() != 2 || !parts[0].eq_ignore_ascii_case("Bearer") {
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

    let payload: JwtPayload =
        serde_json::from_slice(&decoded).context("Failed to parse JWT payload")?;
    Ok(payload.iss)
}

pub fn get_user_language(header: Option<&str>) -> Option<String> {
    let header = header?;
    let mut languages: Vec<(&str, f32)> = header
        .split(',')
        .filter_map(|s| {
            let mut parts = s.split(';');
            let lang = parts.next()?.trim();
            if lang.is_empty() {
                return None;
            }
            let q = parts
                .next()
                .and_then(|p| p.trim().strip_prefix("q="))
                .and_then(|v| v.parse::<f32>().ok())
                .unwrap_or(1.0);
            Some((lang, q))
        })
        .collect();

    // Sort by q-value descending
    languages.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    languages.first().map(|(lang, _)| lang.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// JWTからDIDを抽出できているかを各種検証する
    #[test]
    fn test_extract_did_from_jwt() {
        // Basic happy path
        let valid_jwt = "Bearer eyJhbGciOiJIUzI1NiJ9.eyJpc3MiOiJkaWQ6cGxjOmV4YW1wbGUifQ.signature";
        assert_eq!(
            extract_did_from_jwt(Some(valid_jwt)).unwrap(),
            "did:plc:example"
        );

        // Missing header
        assert!(extract_did_from_jwt(None).is_err());

        // Case-insensitive Bearer
        let lowercase_bearer =
            "bearer eyJhbGciOiJIUzI1NiJ9.eyJpc3MiOiJkaWQ6cGxjOmV4YW1wbGUifQ.signature";
        assert_eq!(
            extract_did_from_jwt(Some(lowercase_bearer)).unwrap(),
            "did:plc:example"
        );

        // Invalid format (missing Bearer)
        assert!(extract_did_from_jwt(Some("InvalidToken")).is_err());

        // Invalid JWT (not enought parts)
        assert!(extract_did_from_jwt(Some("Bearer invalid.jwt")).is_err());
    }

    /// ヘッダーから最も優先度が高い言語を取得する
    #[test]
    fn test_get_user_language() {
        assert_eq!(get_user_language(Some("en-US")), Some("en-US".to_string()));
        assert_eq!(
            get_user_language(Some("en-US;q=0.8,ja;q=1.0")),
            Some("ja".to_string())
        );
        assert_eq!(
            get_user_language(Some("da, en-gb;q=0.8, en;q=0.7")),
            Some("da".to_string())
        );
        assert_eq!(
            get_user_language(Some("en;q=0.8, ja;q=1.0")),
            Some("ja".to_string())
        );
        assert_eq!(get_user_language(None), None);
        assert_eq!(get_user_language(Some("")), None);
        assert_eq!(get_user_language(Some("   ")), None);
    }
}
