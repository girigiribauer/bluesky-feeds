use axum::{extract::State, routing::post, Json, Router};
use serde_json::Value;
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub struct FakeUmami {
    pub url: String,
    urls: Arc<Mutex<Vec<String>>>,
}

impl FakeUmami {
    pub async fn start() -> Self {
        let urls = Arc::new(Mutex::new(Vec::new()));
        let router = Router::new()
            .route("/api/send", post(send))
            .with_state(urls.clone());

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });

        Self { url, urls }
    }

    pub fn urls(&self) -> Vec<String> {
        self.urls.lock().unwrap().clone()
    }

    pub async fn wait_for_urls(&self, count: usize) -> Vec<String> {
        for _ in 0..100 {
            if self.urls().len() >= count {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        self.urls()
    }
}

async fn send(State(urls): State<Arc<Mutex<Vec<String>>>>, Json(body): Json<Value>) {
    if let Some(url) = body["payload"]["url"].as_str() {
        urls.lock().unwrap().push(url.to_string());
    }
}
