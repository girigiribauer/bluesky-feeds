use bluesky_feeds::app;
use bluesky_feeds::state::AppState;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
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
    let database_url = std::env::var("HELLOWORLD_DB_URL")
        .unwrap_or_else(|_| "sqlite:data/helloworld.db".to_string());
    tracing::info!("Connecting to database: {}", database_url);

    let helloworld_db = bluesky_feeds::connect_database(&database_url).await?;
    helloworld::migrate(&helloworld_db).await?;

    // Initialize Fake Bluesky Database
    let fakebluesky_db_url = std::env::var("FAKEBLUESKY_DB_URL")
        .unwrap_or_else(|_| "sqlite:data/fakebluesky.db".to_string());
    tracing::info!("Connecting to fakebluesky database: {}", fakebluesky_db_url);
    let fakebluesky_db = bluesky_feeds::connect_database(&fakebluesky_db_url).await?;
    fakebluesky::migrate(&fakebluesky_db).await?;

    // Jetstream カーソル保存テーブルの作成（バックフィル対応）
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS jetstream_cursor (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            cursor_us INTEGER NOT NULL
        );
        "#,
    )
    .execute(&fakebluesky_db)
    .await?;

    // 前回保存したカーソルを読み込む
    let initial_cursor: Option<i64> =
        sqlx::query_scalar("SELECT cursor_us FROM jetstream_cursor WHERE id = 1")
            .fetch_optional(&fakebluesky_db)
            .await
            .unwrap_or(None);

    if let Some(cursor) = initial_cursor {
        tracing::info!("Resuming Jetstream from saved cursor: {}", cursor);
    } else {
        tracing::info!("No saved Jetstream cursor found. Starting from live tail.");
    }

    // Initialize Private List Database
    let privatelist_db_url = std::env::var("PRIVATELIST_DB_URL")
        .unwrap_or_else(|_| "sqlite:data/privatelist.db".to_string());
    tracing::info!("Connecting to privatelist database: {}", privatelist_db_url);
    let privatelist_db = bluesky_feeds::connect_database(&privatelist_db_url).await?;
    privatelist::migrate(&privatelist_db).await?;

    // Initialize OneYearAgo Database
    let oneyearago_db_url = std::env::var("ONEYEARAGO_DB_URL")
        .unwrap_or_else(|_| "sqlite:data/oneyearago.db".to_string());
    tracing::info!(
        "Connecting to oneyearago cache database: {}",
        oneyearago_db_url
    );
    let oneyearago_db = bluesky_feeds::connect_database(&oneyearago_db_url).await?;
    oneyearago::cache::migrate(&oneyearago_db).await?;

    // Initialize HTTP Client
    let http_client = reqwest::Client::builder()
        .user_agent("BlueskyFeedGenerator/1.0 (girigiribauer.com)")
        .build()
        .expect("Failed to build HTTP client");

    // Perform initial authentication
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

    let privatelist_url =
        std::env::var("PRIVATELIST_URL").unwrap_or_else(|_| "http://localhost:3000".to_string());
    let bsky_api_url =
        std::env::var("BSKY_API_URL").unwrap_or_else(|_| "https://api.bsky.app".to_string());

    let config = bluesky_feeds::state::AppConfig {
        privatelist_url: privatelist_url.clone(),
        bsky_api_url: bsky_api_url.clone(),
        client_id: format!("{}/client-metadata.json", privatelist_url),
        redirect_uri: format!("{}/oauth/callback", privatelist_url),
    };

    let app_state = AppState {
        config,
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
        privatelist_db,
        oneyearago_db,
        umami: bluesky_feeds::analytics::UmamiClient::new(
            std::env::var("UMAMI_HOST").expect("UMAMI_HOST must be set"),
            std::env::var("UMAMI_WEBSITE_ID").expect("UMAMI_WEBSITE_ID must be set"),
            Some(
                std::env::var("APP_HOSTNAME")
                    .unwrap_or_else(|_| "feeds.bsky.girigiribauer.com".to_string()),
            ),
        ),
        key: axum_extra::extract::cookie::Key::from(
             &std::env::var("COOKIE_SECRET")
                .unwrap_or_else(|_| "very-secret-key-that-is-at-least-64-bytes-long-for-security-reasons-please-change-me".to_string())
                .into_bytes()
        ),
    };

    // Start Jetstream consumer in background
    let enable_jetstream = std::env::var("ENABLE_JETSTREAM").unwrap_or_else(|_| "true".to_string());
    if enable_jetstream == "true" {
        let state_for_consumer = app_state.clone();
        let cursor_db = app_state.fakebluesky_db.clone();
        // 現在のカーソルを共有するための Arc<Mutex>
        let current_cursor = Arc::new(Mutex::new(initial_cursor));

        tokio::spawn(async move {
            let cursor_for_callback = current_cursor.clone();
            let result = jetstream::connect_and_run(
                move |event| {
                    let state = state_for_consumer.clone();
                    let cursor_ref = cursor_for_callback.clone();
                    let db = cursor_db.clone();
                    async move {
                        let helloworld_pool = state.helloworld_db.clone();
                        let fakebluesky_pool = state.fakebluesky_db.clone();

                        // Process event for helloworld and fakebluesky
                        let hw_cursor = helloworld::process_event(&helloworld_pool, &event).await;
                        let fb_cursor = fakebluesky::process_event(&fakebluesky_pool, &event).await;

                        // 最新の time_us をカーソルとして保存
                        let new_cursor = fb_cursor.or(hw_cursor);
                        if let Some(cursor_us) = new_cursor {
                            let mut current = cursor_ref.lock().await;
                            *current = Some(cursor_us);
                            drop(current);

                            // DB への書き込み（失敗してもパニックしない）
                            if let Err(e) = sqlx::query(
                                "INSERT OR REPLACE INTO jetstream_cursor (id, cursor_us) VALUES (1, ?)"
                            )
                            .bind(cursor_us)
                            .execute(&db)
                            .await
                            {
                                tracing::error!("Failed to save Jetstream cursor: {}", e);
                            }

                            Some(cursor_us)
                        } else {
                            None
                        }
                    }
                },
                initial_cursor,
            )
            .await;

            if let Err(e) = result {
                tracing::error!("Jetstream ingester failed: {}", e);
            }
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
