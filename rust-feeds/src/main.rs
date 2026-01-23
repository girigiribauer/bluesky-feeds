use axum::{
    extract::Query,
    http::StatusCode,
    response::Json,
    routing::get,
    Router,
};
use serde::Deserialize;
use shared::FeedService;
use std::net::SocketAddr;
use tower_http::trace::TraceLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Debug, Deserialize)]
struct FeedQuery {
    feed: String,
}

#[tokio::main]
async fn main() {
    // ログ初期化
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // ルーター構築
    let app = Router::new()
        .route("/", get(root))
        .route("/xrpc/app.bsky.feed.getFeedSkeleton", get(get_feed_skeleton))
        .layer(TraceLayer::new_for_http());

    // サーバー起動
    let port = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(3001);
    let addr = SocketAddr::from(([0, 0, 0, 0], port));

    tracing::info!("Rust feed server listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn root() -> &'static str {
    "Rust Bluesky Feed Generator"
}

async fn get_feed_skeleton(
    Query(params): Query<FeedQuery>,
) -> Result<Json<shared::FeedSkeletonResult>, StatusCode> {
    tracing::info!("Received feed request: {}", params.feed);

    // フィード名を抽出 (at://did:web:.../app.bsky.feed.generator/helloworld)
    let feed_name = params
        .feed
        .split('/')
        .last()
        .ok_or(StatusCode::BAD_REQUEST)?;

    let service = FeedService::from_str(feed_name).ok_or(StatusCode::NOT_FOUND)?;

    match service {
        FeedService::Helloworld => {
            let result = helloworld::get_posts();
            Ok(Json(result))
        }
        _ => {
            tracing::warn!("Feed not implemented: {:?}", service);
            Err(StatusCode::NOT_IMPLEMENTED)
        }
    }
}
