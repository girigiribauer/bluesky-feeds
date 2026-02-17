pub mod analytics;
pub mod error;
pub mod handlers;
pub mod state;

use axum::{
    body::Body,
    extract::Host,
    http::{Request, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Router,
};
use sqlx::SqlitePool;
use state::SharedState;
use tower::ServiceExt;
use tower_http::{
    services::{ServeDir, ServeFile},
    trace::TraceLayer,
};

pub fn app(state: SharedState) -> Router {
    let feed_router = create_feed_router(state.clone());
    let webui_router = create_webui_router(state.clone());

    Router::new()
        .fallback(move |host: Host, req: Request<Body>| async move {
            let host_str = host.0.to_lowercase();
            tracing::debug!("Routing request for host: {}", host_str);

            if host_str.starts_with("privatelist") {
                match webui_router.oneshot(req).await {
                    Ok(res) => res.into_response(),
                    Err(e) => (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("WebUI Router Error: {}", e),
                    )
                        .into_response(),
                }
            } else {
                match feed_router.oneshot(req).await {
                    Ok(res) => res.into_response(),
                    Err(e) => (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("Feed Router Error: {}", e),
                    )
                        .into_response(),
                }
            }
        })
        .with_state(state)
}

fn create_feed_router(state: SharedState) -> Router {
    Router::new()
        .route("/", get(handlers::root))
        .route("/health", get(handlers::health))
        .route(
            "/xrpc/app.bsky.feed.getFeedSkeleton",
            get(handlers::get_feed_skeleton),
        )
        .route(
            "/xrpc/app.bsky.feed.describeFeedGenerator",
            get(handlers::describe_feed_generator),
        )
        .route("/.well-known/did.json", get(handlers::get_did_json))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

fn create_webui_router(state: SharedState) -> Router {
    // API routes for WebUI (prefixed with /privatelist)
    let api_router = Router::new()
        .route("/me", get(handlers::privatelist_me))
        .route("/list", get(handlers::privatelist_list))
        .route("/add", post(handlers::privatelist_add))
        .route("/remove", post(handlers::privatelist_remove))
        .route("/refresh", post(handlers::privatelist_refresh));

    Router::new()
        .nest("/privatelist", api_router)
        .route("/client-metadata.json", get(handlers::client_metadata))
        .route("/oauth/login", get(handlers::login))
        .route("/oauth/callback", get(handlers::callback))
        .route("/oauth/logout", get(handlers::logout))
        // Static files with Fallback for SPA (History API Fallback)
        .fallback_service(
            ServeDir::new("webui/dist").not_found_service(ServeFile::new("webui/dist/index.html")),
        )
        .layer(TraceLayer::new_for_http())
        .layer(
            tower_http::cors::CorsLayer::new()
                .allow_origin(tower_http::cors::Any)
                .allow_methods(tower_http::cors::Any)
                .allow_headers(tower_http::cors::Any),
        )
        .with_state(state)
}

use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use std::str::FromStr;

pub async fn connect_database(url: &str) -> anyhow::Result<SqlitePool> {
    let options = SqliteConnectOptions::from_str(url)?.create_if_missing(true);

    let pool = SqlitePoolOptions::new().connect_with(options).await?;

    Ok(pool)
}
