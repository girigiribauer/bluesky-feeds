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
    name: String,
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
        event_name: String,
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
                data,
            },
        };

        // Fire and forget
        tokio::spawn(async move {
            let endpoint = format!("{}/api/send", host);
            if let Err(e) = client
                .post(&endpoint)
                .json(&payload)
                .header("User-Agent", "BlueskyFeedGenerator/1.0 (girigiribauer.com)")
                .send()
                .await
            {
                tracing::warn!("Failed to send analytics event: {}", e);
            } else {
                tracing::debug!("Analytics event sent successfully");
            }
        });
    }
}
