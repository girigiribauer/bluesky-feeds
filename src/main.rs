use bluesky_feeds::state::{AppState, SharedState};
use bluesky_feeds::app;
use std::net::SocketAddr;
use std::sync::{Arc, RwLock};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() {
    println!("Starting Rust Bluesky Feed Generator...");

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    tracing::info!("Log initialized");

    // Initialize app state with standard HTTP client
    let client = reqwest::Client::builder()
        .user_agent("BlueskyFeedGenerator/1.0 (girigiribauer.com)")
        .build()
        .expect("Failed to build HTTP client");

    // Authenticate with Bluesky (Service Auth)
    let handle = std::env::var("APP_HANDLE").unwrap_or_default();
    let password = std::env::var("APP_PASSWORD").unwrap_or_default();
    let mut service_token = None;
    let mut service_did = None;

    if !handle.is_empty() && !password.is_empty() {
        tracing::info!("Authenticating as {}...", handle);
        match todoapp::authenticate(&client, &handle, &password).await {
            Ok((token, did)) => {
                tracing::info!("Successfully authenticated with Bluesky (DID: {})", did);
                service_token = Some(token);
                service_did = Some(did);
            }
            Err(e) => {
                tracing::error!("Failed to authenticate with Bluesky: {}. Search API will fail.", e);
            }
        }
    } else {
        tracing::error!("APP_HANDLE or APP_PASSWORD not set. Application cannot function correctly.");
        // Prevent tight restart loop
        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
        panic!("Missing required configuration: APP_HANDLE or APP_PASSWORD");
    }

    if handle != "feeds.bsky.girigiribauer.com" && !handle.is_empty() {
        tracing::warn!("CAUTION: APP_HANDLE ({}) does not match production host (feeds.bsky.girigiribauer.com). specific functionality like did.json might be incorrect if this is production.", handle);
    }

    let state: SharedState = Arc::new(RwLock::new(AppState {
        helloworld: helloworld::State::default(),
        http_client: client,
        service_token,
        service_did,
        auth_handle: handle,
        auth_password: password,
    }));

    let state_for_ingester = state.clone();
    tokio::spawn(async move {
        let result = jetstream::connect_and_run(move |event| {
            if let Ok(mut lock) = state_for_ingester.write() {
                helloworld::process_event(&mut lock.helloworld, event);
            }
        })
        .await;

        if let Err(e) = result {
            tracing::error!("Jetstream ingester failed: {}", e);
        }
    });

    let app = app(state);

    let port = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(3000);
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
