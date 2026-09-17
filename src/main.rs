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

    let handle = std::env::var("APP_HANDLE").unwrap_or_default();
    let password = std::env::var("APP_PASSWORD").unwrap_or_default();

    if password.is_empty() {
        println!("Error: APP_PASSWORD environment variable is not set.");
        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
        panic!("APP_PASSWORD is missing");
    }

    let database_url = std::env::var("HELLOWORLD_DB_URL")
        .unwrap_or_else(|_| "sqlite:data/helloworld.db".to_string());
    tracing::info!("Connecting to database: {}", database_url);

    let helloworld_db = bluesky_feeds::connect_database(&database_url).await?;
    helloworld::migrate(&helloworld_db).await?;

    let realfakebluesky_db_url = std::env::var("REALFAKEBLUESKY_DB_URL")
        .unwrap_or_else(|_| "sqlite:data/fakebluesky.db".to_string());
    tracing::info!(
        "Connecting to realfakebluesky database: {}",
        realfakebluesky_db_url
    );
    let realfakebluesky_db = bluesky_feeds::connect_database(&realfakebluesky_db_url).await?;
    realfakebluesky::migrate(&realfakebluesky_db).await?;

    let oneyearago_db_url = std::env::var("ONEYEARAGO_DB_URL")
        .unwrap_or_else(|_| "sqlite:data/oneyearago.db".to_string());
    tracing::info!(
        "Connecting to oneyearago cache database: {}",
        oneyearago_db_url
    );
    let oneyearago_db = bluesky_feeds::connect_database(&oneyearago_db_url).await?;
    oneyearago::cache::migrate(&oneyearago_db).await?;

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
        realfakebluesky_db,
        oneyearago_db,
        umami: bluesky_feeds::analytics::UmamiClient::new(
            std::env::var("UMAMI_HOST").expect("UMAMI_HOST must be set"),
            std::env::var("UMAMI_WEBSITE_ID").expect("UMAMI_WEBSITE_ID must be set"),
            Some(
                std::env::var("APP_HOSTNAME")
                    .unwrap_or_else(|_| "feeds.bsky.girigiribauer.com".to_string()),
            ),
        ),
    };

    let enable_jetstream = std::env::var("ENABLE_JETSTREAM").unwrap_or_else(|_| "true".to_string());
    if enable_jetstream == "true" {
        let state_for_consumer = app_state.clone();
        tokio::spawn(async move {
            jetstream::start_consumer(
                state_for_consumer.realfakebluesky_db.clone(),
                jetstream::ConsumerConfig::from_env(),
                move |event| {
                    let state = state_for_consumer.clone();
                    async move {
                        let jetstream::Event::Post(post) = event else {
                            return;
                        };

                        helloworld::process_event(&state.helloworld_db, &post).await;

                        realfakebluesky::process_event(&state.realfakebluesky_db, &post).await;
                    }
                },
            )
            .await;
        });
    } else {
        tracing::info!("Jetstream consumer is disabled (ENABLE_JETSTREAM != true)");
    }

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
