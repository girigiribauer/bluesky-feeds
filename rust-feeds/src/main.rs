use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::Json,
    routing::get,
    Router,
};
use serde::Deserialize;
use models::FeedService;
use std::{
    net::SocketAddr,
    sync::{Arc, RwLock},
};
use tower_http::trace::TraceLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Debug, Deserialize)]
struct FeedQuery {
    feed: String,
    cursor: Option<String>,
    limit: Option<usize>,
}

// 共有ステート
type SharedState = Arc<RwLock<AppState>>;

#[derive(Default)]
struct AppState {
    helloworld: helloworld::State,
}

#[tokio::main]
async fn main() {
    println!("Starting Rust Bluesky Feed Generator...");

    // ログ初期化
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    tracing::info!("Log initialized");

    // 共有ステートの初期化
    let state: SharedState = Arc::new(RwLock::new(AppState::default()));

    // Jetstreamインジェスターをバックグラウンドで起動
    let state_for_ingester = state.clone();
    tokio::spawn(async move {
        // インフラ詳細（URLや圧縮設定など）は shared 側にお任せ
        // ここでは「イベントが来たらどこに渡すか」だけを記述
        let result = jetstream::connect_and_run(move |event| {
            if let Ok(mut lock) = state_for_ingester.write() {
                // helloworldフィードにイベントを配給
                helloworld::process_event(&mut lock.helloworld, event);

                // 将来のフィードもここに追加するだけでOK
                // todoapp::process_event(&mut lock.todoapp, event);
            }
        })
        .await;

        if let Err(e) = result {
            tracing::error!("Jetstream ingester failed: {}", e);
        }
    });

    // ルーター構築
    let app = Router::new()
        .route("/", get(root))
        .route("/xrpc/app.bsky.feed.getFeedSkeleton", get(get_feed_skeleton))
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    // サーバー起動
    let port = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(3001);
    let addr = SocketAddr::from(([0, 0, 0, 0], port));

    println!("Attempting to bind/listen on {}", addr);
    tracing::info!("Rust feed server listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("Failed to bind to address");

    println!("Server started successfully");
    axum::serve(listener, app)
        .await
        .expect("Server failed to run");
}

async fn root() -> &'static str {
    "Rust Bluesky Feed Generator"
}

async fn get_feed_skeleton(
    State(state): State<SharedState>,
    Query(params): Query<FeedQuery>,
) -> Result<Json<models::FeedSkeletonResult>, StatusCode> {
    tracing::info!("Received feed request: {} (cursor={:?}, limit={:?})", params.feed, params.cursor, params.limit);

    // フィード名を抽出 (at://did:web:.../app.bsky.feed.generator/helloworld)
    let feed_name = params
        .feed
        .split('/')
        .last()
        .ok_or(StatusCode::BAD_REQUEST)?;

    let service = FeedService::from_str(feed_name).ok_or(StatusCode::NOT_FOUND)?;

    match service {
        FeedService::Helloworld => {
            if let Ok(lock) = state.read() {
                Ok(Json(helloworld::get_feed_skeleton(
                    &lock.helloworld,
                    params.cursor,
                    params.limit,
                )))
            } else {
                Err(StatusCode::INTERNAL_SERVER_ERROR)
            }
        }
        _ => {
            tracing::warn!("Feed not implemented: {:?}", service);
            Err(StatusCode::NOT_IMPLEMENTED)
        }
    }
}
