use axum::{extract::State, routing::get, Json, Router};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

pub struct FakeBlueskyApi {
    pub url: String,
    requests: Arc<AtomicUsize>,
}

impl FakeBlueskyApi {
    pub async fn start() -> Self {
        let requests = Arc::new(AtomicUsize::new(0));
        let router = Router::new()
            .route("/xrpc/app.bsky.actor.getProfile", get(get_profile))
            .route("/xrpc/app.bsky.feed.searchPosts", get(search_posts))
            .with_state(requests.clone());

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });

        Self { url, requests }
    }

    pub fn requests(&self) -> usize {
        self.requests.load(Ordering::Relaxed)
    }
}

async fn get_profile(State(requests): State<Arc<AtomicUsize>>) -> Json<Value> {
    requests.fetch_add(1, Ordering::Relaxed);
    Json(json!({ "description": "UTC+9" }))
}

async fn search_posts(State(requests): State<Arc<AtomicUsize>>) -> Json<Value> {
    requests.fetch_add(1, Ordering::Relaxed);
    Json(json!({ "posts": [], "cursor": null }))
}
