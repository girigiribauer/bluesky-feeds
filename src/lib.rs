pub mod analytics;
pub mod error;
pub mod handlers;
pub mod state;

use axum::{
    extract::{Query, Request},
    routing::get,
    Router,
};
use state::{FeedQuery, SharedState};
use tower_http::trace::TraceLayer;
use tracing::field::display;

fn request_span(request: &Request) -> tracing::Span {
    let query = Query::<FeedQuery>::try_from_uri(request.uri()).ok();
    let feed = query.as_ref().map(|q| display(q.feed_name()));
    tracing::info_span!("request", feed)
}

pub fn app(state: SharedState) -> Router {
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
        .layer(TraceLayer::new_for_http().make_span_with(request_span))
        .with_state(state)
}
