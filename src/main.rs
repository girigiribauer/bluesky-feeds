use bluesky_feeds::app;
use bluesky_feeds::state::AppState;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("Starting Rust Bluesky Feed Generator...");

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    tracing::info!("Log initialized");

    // Authenticate with Bluesky (Service Auth)
    let handle = std::env::var("APP_HANDLE").unwrap_or_default();
    let password = std::env::var("APP_PASSWORD").unwrap_or_default();

    if password.is_empty() {
        println!("Error: APP_PASSWORD environment variable is not set.");
        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
        panic!("APP_PASSWORD is missing");
    }

    // Initialize Database
    let database_url =
        std::env::var("HELLOWORLD_DB_URL").unwrap_or_else(|_| "sqlite:helloworld.db".to_string());
    tracing::info!("Connecting to database: {}", database_url);

    let helloworld_db = bluesky_feeds::connect_database(&database_url).await?;
    helloworld::migrate(&helloworld_db).await?;

    // Initialize Fake Bluesky Database
    let fakebluesky_db_url =
        std::env::var("FAKEBLUESKY_DB_URL").unwrap_or_else(|_| "sqlite:fakebluesky.db".to_string());
    tracing::info!("Connecting to fakebluesky database: {}", fakebluesky_db_url);
    let fakebluesky_db = bluesky_feeds::connect_database(&fakebluesky_db_url).await?;
    fakebluesky::migrate(&fakebluesky_db).await?;

    // Perform initial authentication
    let http_client = reqwest::Client::builder()
        .user_agent("BlueskyFeedGenerator/1.0 (girigiribauer.com)")
        .build()
        .expect("Failed to build HTTP client");

    let (initial_token, initial_did) = if !handle.is_empty() && !password.is_empty() {
        match todoapp::authenticate(&http_client, &handle, &password).await {
            Ok((token, did)) => {
                tracing::info!("Initial authentication successful (DID: {})", did);
                (Some(token), Some(did))
            }
            Err(e) => {
                tracing::warn!("Initial authentication failed: {}. Feeds requiring auth will fail until first request triggers re-auth.", e);
                (None, None)
            }
        }
    } else {
        tracing::warn!("No credentials provided. Feeds requiring auth will fail.");
        (None, None)
    };

    let app_state = AppState {
        helloworld: helloworld::State::default(),
        http_client,
        service_auth: Arc::new(RwLock::new(bluesky_feeds::state::ServiceAuth {
            token: initial_token,
            did: initial_did,
        })),
        auth_handle: handle,
        auth_password: password,
        helloworld_db,
        fakebluesky_db,
        umami: bluesky_feeds::analytics::UmamiClient::new(
            std::env::var("UMAMI_HOST").expect("UMAMI_HOST must be set"),
            std::env::var("UMAMI_WEBSITE_ID").expect("UMAMI_WEBSITE_ID must be set"),
            Some(
                std::env::var("APP_HOSTNAME")
                    .unwrap_or_else(|_| "feeds.bsky.girigiribauer.com".to_string()),
            ),
        ),
    };

    // Start Jetstream consumer in background
    let state_for_consumer = app_state.clone();
    tokio::spawn(async move {
        let result = jetstream::connect_and_run(move |event| {
            let state = state_for_consumer.clone();
            async move {
                let helloworld_pool = state.helloworld_db.clone();
                let fakebluesky_pool = state.fakebluesky_db.clone();

                // Process event for helloworld
                helloworld::process_event(&helloworld_pool, &event).await;

                // Process event for fakebluesky
                fakebluesky::process_event(&fakebluesky_pool, &event).await;
            }
        })
        .await;

        if let Err(e) = result {
            tracing::error!("Jetstream ingester failed: {}", e);
        }
    });

    let port = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(3000);
    let addr = SocketAddr::from(([0, 0, 0, 0], port));

    println!("Attempting to bind/listen on {}", addr);
    tracing::info!("Rust feed server listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;

    println!("Server started successfully");
    let router = app(app_state);
    axum::serve(listener, router).await?;

    Ok(())
}
