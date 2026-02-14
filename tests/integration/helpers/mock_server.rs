use axum::{extract::Query, routing::get, Json, Router};
use std::collections::HashMap;
use tokio::sync::oneshot;

pub struct MockServer {
    pub port: u16,
    shutdown_tx: Option<oneshot::Sender<()>>,
}

impl MockServer {
    pub async fn start() -> Self {
        let app = Router::new().route("/xrpc/app.bsky.feed.searchPosts", get(handle_search));

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let (tx, rx) = oneshot::channel();

        tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    rx.await.ok();
                })
                .await
                .unwrap();
        });

        MockServer {
            port,
            shutdown_tx: Some(tx),
        }
    }

    pub fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }
}

impl Drop for MockServer {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
    }
}

async fn handle_search(Query(params): Query<HashMap<String, String>>) -> Json<serde_json::Value> {
    let q = params.get("q").map(|s| s.as_str()).unwrap_or("");

    // Simple mock logic: return a post for any "from:DID" query
    let did = if q.starts_with("from:") {
        q.strip_prefix("from:").unwrap()
    } else {
        "unknown"
    };

    Json(serde_json::json!({
        "posts": [
            {
                "uri": format!("at://{}/app.bsky.feed.post/1", did),
                "cid": "bafyreicid",
                "record": {
                    "text": format!("Post from {}", did),
                    "createdAt": "2023-01-01T00:00:00Z"
                },
                "indexedAt": "2023-01-01T00:00:00Z",
                "author": {
                    "did": did,
                    "handle": format!("{}.test", did),
                    "displayName": format!("User {}", did),
                    "avatar": "https://example.com/avatar.png",
                    "labels": [],
                    "viewer": {
                        "muted": false,
                        "blockedBy": false
                    }
                },
                "replyCount": 0,
                "repostCount": 0,
                "likeCount": 0
            }
        ]
    }))
}
