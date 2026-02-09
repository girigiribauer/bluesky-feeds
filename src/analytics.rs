use reqwest::Client;
use serde::Serialize;

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
    name: Option<String>,
    language: Option<String>,
    data: Option<serde_json::Value>,
}

impl UmamiClient {
    pub fn new(mut host: String, website_id: String, hostname: Option<String>) -> Self {
        if !host.starts_with("http://") && !host.starts_with("https://") {
            host = format!("https://{}", host);
        }
        // Remove trailing slash if present
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

    pub fn send_event(
        &self,
        url: String,
        event_name: Option<String>,
        language: Option<String>,
        data: Option<serde_json::Value>,
    ) {
        let client = self.client.clone();
        let host = self.host.clone();
        let payload = EventPayload {
            event_type: "pageview".to_string(),
            payload: EventData {
                website: self.website_id.clone(),
                hostname: self.hostname.clone(),
                url,
                name: event_name,
                language,
                data,
            },
        };

        tokio::spawn(async move {
            let endpoint = format!("{}/api/send", host);
            match client
                .post(&endpoint)
                .json(&payload)
                // Umami に弾かれないようにするためにUser-Agentを偽装する
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
        });
    }
}
