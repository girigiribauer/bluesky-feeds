use bluesky_feeds::app;
use bluesky_feeds::state::AppState;
use chrono::DateTime;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicI64, Ordering};
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
        // 起動時に DB からカーソルを読み込む（マイクロ秒 i64）
        let initial_cursor_us: Option<i64> =
            sqlx::query_scalar("SELECT cursor FROM jetstream_cursor WHERE id = 1")
                .fetch_optional(&app_state.fakebluesky_db)
                .await
                .unwrap_or(None);
        tracing::info!(
            "Jetstream initial cursor from DB: {:?} us",
            initial_cursor_us
        );

        // JetstreamConfig に渡す DateTime<Utc>（マイクロ秒 → DateTime 変換）
        let initial_cursor_dt = initial_cursor_us.and_then(|us| {
            DateTime::from_timestamp(us / 1_000_000, ((us % 1_000_000) * 1_000) as u32)
        });

        // コールバック内でカーソルを更新するための共有 AtomicI64（マイクロ秒）
        let latest_cursor = Arc::new(AtomicI64::new(initial_cursor_us.unwrap_or(0)));

        // 5秒ごとにカーソルを DB に保存するタスク
        {
            let cursor_for_save = latest_cursor.clone();
            let pool_for_save = app_state.fakebluesky_db.clone();
            tokio::spawn(async move {
                loop {
                    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                    let cursor = cursor_for_save.load(Ordering::Relaxed);
                    if cursor > 0 {
                        if let Err(e) = sqlx::query(
                            "INSERT OR REPLACE INTO jetstream_cursor (id, cursor) VALUES (1, ?)",
                        )
                        .bind(cursor)
                        .execute(&pool_for_save)
                        .await
                        {
                            tracing::warn!("Failed to save Jetstream cursor: {}", e);
                        }
                    }
                }
            });
        }

        let state_for_consumer = app_state.clone();
        tokio::spawn(async move {
            // 受信レート計測用（ロックフリー。コールバックをまたいで状態を保持するために Arc を使う）
            let recv_count = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
            let last_report = std::sync::Arc::new(std::sync::Mutex::new(std::time::Instant::now()));

            let result = jetstream::connect_and_run(initial_cursor_dt, move |event| {
                let state = state_for_consumer.clone();
                let recv_count = recv_count.clone();
                let last_report = last_report.clone();
                let latest_cursor = latest_cursor.clone();
                async move {
                    let helloworld_pool = state.helloworld_db.clone();
                    let fakebluesky_pool = state.fakebluesky_db.clone();

                    // Process event for helloworld
                    helloworld::process_event(&helloworld_pool, &event).await;

                    // Process event for fakebluesky
                    fakebluesky::process_event(&fakebluesky_pool, &event).await;

                    // カーソル更新（イベントの time_us をそのまま保持）
                    use jetstream_oxide::events::commit::CommitEvent;
                    let time_us = match &event {
                        CommitEvent::Create { info, .. } => info.time_us,
                        CommitEvent::Delete { info, .. } => info.time_us,
                        CommitEvent::Update { info, .. } => info.time_us,
                    };
                    latest_cursor.store(time_us as i64, Ordering::Relaxed);

                    // 受信レートの集計（1分ごとにレポート）
                    let count = recv_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                    let mut last = last_report.lock().unwrap();
                    let elapsed = last.elapsed();
                    if elapsed >= std::time::Duration::from_secs(60) {
                        tracing::info!(
                            "METRICS [1min]: recv={} events, rate={:.1}/s",
                            count,
                            count as f64 / elapsed.as_secs_f64()
                        );
                        recv_count.store(0, std::sync::atomic::Ordering::Relaxed);
                        *last = std::time::Instant::now();
                    }
                }
            })
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
