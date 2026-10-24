use reqwest::Client;
use serde::Serialize;
use tracing::Instrument;

#[derive(Clone, Debug)]
pub struct UmamiClient {
    client: Client,
    host: String,
    website_id: String,
    hostname: Option<String>,
}

#[derive(Serialize)]
struct EventPayload {
    #[serde(rename = "type")]
    event_type: String,
    payload: EventData,
}

#[derive(Serialize)]
struct EventData {
    website: String,
    hostname: Option<String>,
    url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    language: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    data: Option<serde_json::Value>,
}

impl UmamiClient {
    pub fn new(mut host: String, website_id: String, hostname: Option<String>) -> Self {
        if !host.starts_with("http://") && !host.starts_with("https://") {
            host = format!("https://{}", host);
        }
        if host.ends_with('/') {
            host.pop();
        }

        Self {
            client: Client::new(),
            host,
            website_id,
            hostname,
        }
    }

    pub fn from_settings(
        host: Option<String>,
        website_id: Option<String>,
        hostname: Option<String>,
    ) -> Option<Self> {
        let host = host.filter(|h| !h.trim().is_empty())?;
        let website_id = website_id.filter(|id| !id.trim().is_empty())?;
        Some(Self::new(host, website_id, hostname))
    }

    pub fn send_event(
        &self,
        url: String,
        event_name: Option<String>,
        session_id: Option<String>,
        language: Option<String>,
        data: Option<serde_json::Value>,
    ) {
        let client = self.client.clone();
        let host = self.host.clone();
        let payload = EventPayload {
            event_type: "event".to_string(),
            payload: EventData {
                website: self.website_id.clone(),
                hostname: self.hostname.clone(),
                url,
                name: event_name,
                language,
                id: session_id,
                data,
            },
        };

        tokio::spawn(async move {
            let endpoint = format!("{}/api/send", host);
            match client
                .post(&endpoint)
                .json(&payload)
                .header(
                    "User-Agent",
                    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36"
                )
                .send()
                .await
            {
                Ok(response) => {
                    if !response.status().is_success() {
                        let status = response.status();
                        let text = response.text().await.unwrap_or_default();
                        tracing::warn!(
                            "Umami returned error: status={}, body={}",
                            status,
                            text
                        );
                    } else {
                        tracing::debug!("Analytics event sent successfully");
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to send analytics event: {}", e);
                }
            }
        }
        .in_current_span());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn some(value: &str) -> Option<String> {
        Some(value.to_string())
    }

    /// 送り先と ID が両方そろっていれば、アクセス解析の係を作ること
    #[test]
    fn test_from_settings_with_host_and_id_is_enabled() {
        let client = UmamiClient::from_settings(some("example.com"), some("abc"), None);
        assert!(client.is_some());
    }

    /// ID が無ければ、アクセス解析の係を作らないこと
    #[test]
    fn test_from_settings_without_id_is_disabled() {
        assert!(UmamiClient::from_settings(some("example.com"), None, None).is_none());
    }

    /// 送り先が無ければ、アクセス解析の係を作らないこと
    #[test]
    fn test_from_settings_without_host_is_disabled() {
        assert!(UmamiClient::from_settings(None, some("abc"), None).is_none());
    }

    /// 空の ID は、無いものとして扱うこと
    #[test]
    fn test_from_settings_with_empty_id_is_disabled() {
        assert!(UmamiClient::from_settings(some("example.com"), some(""), None).is_none());
    }
}
