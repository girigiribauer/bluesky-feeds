use crate::error::AppError;
use crate::handlers::{
    handle_fakebluesky, handle_helloworld, handle_oneyearago, handle_privatelist, handle_todoapp,
    DidResponse, DidService,
};
use crate::state::{FeedQuery, SharedState};
use axum::{
    extract::{Query, State},
    response::Json,
};
use bsky_core::FeedService;

pub async fn get_feed_skeleton(
    State(state): State<SharedState>,
    headers: axum::http::HeaderMap,
    Query(params): Query<FeedQuery>,
) -> Result<Json<bsky_core::FeedSkeletonResult>, AppError> {
    tracing::info!(
        "Received feed request: {} (cursor={:?}, limit={:?})",
        params.feed,
        params.cursor,
        params.limit
    );

    // Analytics
    let requester_did = match headers.get("authorization").and_then(|h| h.to_str().ok()) {
        Some(header) => match bsky_core::extract_did_from_jwt(Some(header)) {
            Ok(did) => did,
            Err(e) => {
                tracing::warn!("Failed to extract DID from Authorization header: {}", e);
                "anonymous".to_string()
            }
        },
        None => "anonymous".to_string(),
    };

    let language =
        bsky_core::get_user_language(headers.get("accept-language").and_then(|h| h.to_str().ok()))
            .unwrap_or_else(|| "en".to_string());

    let cursor_state = if params.cursor.is_some() {
        "exists"
    } else {
        "none"
    };

    let feed_name = params
        .feed
        .split('/')
        .next_back()
        .ok_or(AppError::BadRequest("Invalid feed URI".to_string()))?;

    // Construct URL with query parameters for easier filtering in Umami
    let feed_path = format!("/feeds/{}?did={}", feed_name, requester_did);

    let event_data = serde_json::json!({
        "did": requester_did,
        "cursor": cursor_state,
        "language": language,
    });

    state.umami.send_event(
        feed_path,
        None,
        Some(requester_did.clone()),
        Some(language.clone()), // Clone language as it's used above
        Some(event_data),
    );

    let service =
        FeedService::from_str(feed_name).ok_or(AppError::NotFound("Feed not found".to_string()))?;

    match service {
        FeedService::Helloworld => handle_helloworld(state, headers, params).await,
        FeedService::Todoapp => handle_todoapp(state, headers, params).await,
        FeedService::Oneyearago => handle_oneyearago(state, headers, params).await,
        FeedService::Fakebluesky => handle_fakebluesky(state, params).await,
        FeedService::Privatelist => handle_privatelist(state, headers, params).await,
    }
}

pub async fn describe_feed_generator(
    State(state): State<SharedState>,
) -> Result<Json<bsky_core::DescribeFeedGeneratorResponse>, AppError> {
    let (did, _service_did) = {
        let auth = state.service_auth.read().await;
        let did = auth.did.clone().ok_or(AppError::Internal(anyhow::anyhow!(
            "Service not authenticated yet"
        )))?;
        (did.clone(), did) // logic::service_did
    };

    let feeds = vec![
        bsky_core::FeedUri {
            uri: format!("at://{}/app.bsky.feed.generator/helloworld", did),
        },
        bsky_core::FeedUri {
            uri: format!("at://{}/app.bsky.feed.generator/todoapp", did),
        },
        bsky_core::FeedUri {
            uri: format!("at://{}/app.bsky.feed.generator/oneyearago", did),
        },
        bsky_core::FeedUri {
            uri: format!("at://{}/app.bsky.feed.generator/fakebluesky", did),
        },
        bsky_core::FeedUri {
            uri: format!("at://{}/app.bsky.feed.generator/privatelist", did),
        },
    ];

    Ok(Json(bsky_core::DescribeFeedGeneratorResponse {
        did,
        feeds,
    }))
}

pub async fn get_did_json(
    State(_state): State<SharedState>,
) -> Result<Json<DidResponse>, AppError> {
    let hostname = "feeds.bsky.girigiribauer.com";

    let did = format!("did:web:{}", hostname);
    let service_endpoint = format!("https://{}", hostname);

    let response = DidResponse {
        context: vec!["https://www.w3.org/ns/did/v1".to_string()],
        id: did,
        service: vec![DidService {
            id: "#bsky_fg".to_string(),
            service_type: "BskyFeedGenerator".to_string(),
            service_endpoint,
        }],
    };

    Ok(Json(response))
}
