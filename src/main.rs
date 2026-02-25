use bluesky_feeds::app;
use bluesky_feeds::state::AppState;
use jetstream_oxide::events::commit::CommitEvent;
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

    // privatelist サービスは別でDBを持つので、こちらは参照のみとする
    let privatelist_db_url = std::env::var("PRIVATELIST_DB_URL")
        .unwrap_or_else(|_| "sqlite:data/privatelist.db".to_string());
    tracing::info!(
        "Connecting to privatelist database (read-only): {}",
        privatelist_db_url
    );
    let privatelist_db = bluesky_feeds::connect_database_readonly(&privatelist_db_url).await?;

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
    };

    // Start Jetstream consumer in background
    let enable_jetstream = std::env::var("ENABLE_JETSTREAM").unwrap_or_else(|_| "true".to_string());
    if enable_jetstream == "true" {
        let state_for_consumer = app_state.clone();
        let cursor_db = app_state.fakebluesky_db.clone();
        // 現在のカーソルを共有するための Arc<Mutex>
        let current_cursor = Arc::new(Mutex::new(initial_cursor));
        // DB 書き込み頻度を制限するためのタイマー
        let last_db_write = Arc::new(Mutex::new(std::time::Instant::now()));

        tokio::spawn(async move {
            let cursor_for_callback = current_cursor.clone();
            let last_db_write_for_callback = last_db_write.clone();
            let result = jetstream::connect_and_run(
                move |event| {
                    let state = state_for_consumer.clone();
                    let cursor_ref = cursor_for_callback.clone();
                    let last_write_ref = last_db_write_for_callback.clone();
                    let db = cursor_db.clone();
                    async move {
                        let helloworld_pool = state.helloworld_db.clone();
                        let fakebluesky_pool = state.fakebluesky_db.clone();

                        // 1. イベント自体の time_us を独立して取得
                        let event_time_us = match &event {
                            CommitEvent::Create { info, .. } => Some(info.time_us as i64),
                            CommitEvent::Update { info, .. } => Some(info.time_us as i64),
                            CommitEvent::Delete { info, .. } => Some(info.time_us as i64),
                        };

                        // Process event for helloworld and fakebluesky
                        let _ = helloworld::process_event(&helloworld_pool, &event).await;
                        let _ = fakebluesky::process_event(&fakebluesky_pool, &event).await;

                        // 3. 取得した time_us でカーソルを更新 (単調増加を保証)
                        if let Some(time_us) = event_time_us {
                            let mut current = cursor_ref.lock().await;
                            let next_cursor = match *current {
                                Some(c) => std::cmp::max(c, time_us), // Jetstreamは順序非保証なので巻き戻りを防ぐ
                                None => time_us,
                            };
                            *current = Some(next_cursor);
                            drop(current);

                            // DB への書き込み頻度を制限 (5秒に1回)
                            // 毎イベント書くとバックフィル時に SQLite がボトルネックになるため
                            // MAX(cursor_us, ?) により、既存値より古い値では上書きされない（逆行防止）
                            let mut last_write = last_write_ref.lock().await;
                            if last_write.elapsed() >= std::time::Duration::from_secs(5) {
                                if let Err(e) = sqlx::query(
                                    r#"
                                    INSERT INTO jetstream_cursor (id, cursor_us) VALUES (1, ?)
                                    ON CONFLICT(id) DO UPDATE SET cursor_us = MAX(cursor_us, excluded.cursor_us)
                                    "#
                                )
                                .bind(next_cursor)
                                .execute(&db)
                                .await
                                {
                                    tracing::error!("Failed to save Jetstream cursor: {}", e);
                                }
                                *last_write = std::time::Instant::now();
                            }

                            Some(next_cursor)
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

        // 画像解析のバックグラウンドワーカー起動
        // pending_posts を定期ポーリングし、画像解析を行って本番テーブルへ移動する
        let pending_pool = app_state.fakebluesky_db.clone();
        tokio::spawn(async move {
            loop {
                fakebluesky::process_pending(&pending_pool).await;
                tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
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
