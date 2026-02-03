use anyhow::{Context, Result};
use dotenv::dotenv;
use reqwest::{Client, ClientBuilder};
use serde::{Deserialize, Serialize};
use std::env;
use std::path::{Path, PathBuf};

#[derive(Serialize)]
struct CreateSessionRequest<'a> {
    identifier: &'a str,
    password: &'a str,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct CreateSessionResponse {
    did: String,
    #[serde(rename = "accessJwt")]
    access_jwt: String,
    #[serde(rename = "refreshJwt")]
    refresh_jwt: String,
}

#[derive(Serialize)]
struct PutRecordRequest<T> {
    repo: String,
    collection: String,
    rkey: String,
    record: T,
}

#[derive(Serialize)]
struct FeedGeneratorRecord {
    did: String,
    #[serde(rename = "displayName")]
    display_name: String,
    description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    avatar: Option<BlobRef>,
    #[serde(rename = "createdAt")]
    created_at: String,
}

#[derive(Serialize, Deserialize)]
struct BlobRef {
    #[serde(rename = "$type")]
    res_type: String,
    #[serde(rename = "ref")]
    ref_: BlobRefLink,
    #[serde(rename = "mimeType")]
    mime_type: String,
    size: usize,
}

#[derive(Serialize, Deserialize)]
struct BlobRefLink {
    #[serde(rename = "$link")]
    link: String,
}

#[derive(Deserialize)]
struct UploadBlobResponse {
    blob: BlobRef,
}

struct FeedServiceConfig {
    service: &'static str,
    display_name: &'static str,
    description: &'static str,
    avatar: Option<&'static str>,
}

const SERVICE_DID: &str = "did:web:feeds.bsky.girigiribauer.com";

const AVAILABLE_FEED_SERVICES: &[FeedServiceConfig] = &[
    FeedServiceConfig {
        service: "helloworld",
        display_name: "Helloworld feed",
        description: "固定投稿と hello world 投稿のテスト",
        avatar: Some("assets/helloworld.png"),
    },
    FeedServiceConfig {
        service: "todoapp",
        display_name: "TODO feed",
        description: "Only your posts starting with `TODO` are displayed. Replying with `DONE` will remove them.\n\n`TODO` と頭につけた自分の投稿だけが表示されます。 `DONE` と返信すると消えます。",
        avatar: Some("assets/todoapp.png"),
    },
    FeedServiceConfig {
        service: "oneyearago",
        display_name: "OneYearAgo feed",
        description: "Posts from exactly one year ago (±24 hours) are displayed.\n\nちょうど1年前の自分の投稿が表示されます（前後24時間）",
        avatar: Some("assets/oneyearago.png"),
    },
];

#[tokio::main]
async fn main() -> Result<()> {
    dotenv().ok();

    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: publish_feed <feed_service_id>");
        std::process::exit(1);
    }
    let target_service = &args[1];

    let handle = env::var("APP_HANDLE")
        .context("APP_HANDLE not set in .env (checked current and parent directories)")?;
    let password = env::var("APP_PASSWORD")
        .context("APP_PASSWORD not set in .env (checked current and parent directories)")?;

    let config = AVAILABLE_FEED_SERVICES
        .iter()
        .find(|c| c.service == target_service)
        .context(format!("Feed service '{}' not found", target_service))?;

    let client = ClientBuilder::new().build()?;

    println!("Logging in as {}...", handle);
    let session = create_session(&client, &handle, &password).await?;
    println!("Login successful. DID: {}", session.did);

    let avatar_blob = if let Some(avatar_path) = config.avatar {
        let path = Path::new(avatar_path);
        let final_path = if path.exists() {
            path.to_path_buf()
        } else {
            let alt = Path::new("..").join(path);
            if alt.exists() {
                alt
            } else {
                path.to_path_buf()
            }
        };

        if final_path.exists() {
            println!("Uploading avatar: {:?}", final_path);
            Some(upload_blob(&client, &session.access_jwt, &final_path).await?)
        } else {
            println!(
                "Avatar file not found at {:?}, skipping avatar upload.",
                final_path
            );
            None
        }
    } else {
        None
    };

    println!("Publishing feed '{}'...", config.display_name);
    let record = FeedGeneratorRecord {
        did: SERVICE_DID.to_string(),
        display_name: config.display_name.to_string(),
        description: config.description.to_string(),
        avatar: avatar_blob,
        created_at: chrono::Utc::now().to_rfc3339(),
    };

    put_record(
        &client,
        &session.access_jwt,
        &session.did,
        config.service,
        record,
    )
    .await?;
    println!("Successfully published {}", config.service);

    Ok(())
}

async fn create_session(
    client: &Client,
    identifier: &str,
    password: &str,
) -> Result<CreateSessionResponse> {
    let res = client
        .post("https://bsky.social/xrpc/com.atproto.server.createSession")
        .json(&CreateSessionRequest {
            identifier,
            password,
        })
        .send()
        .await?
        .error_for_status()?;

    Ok(res.json().await?)
}

async fn upload_blob(client: &Client, token: &str, path: &PathBuf) -> Result<BlobRef> {
    let bytes = std::fs::read(path).context("Failed to read avatar file")?;
    let res = client
        .post("https://bsky.social/xrpc/com.atproto.repo.uploadBlob")
        .header("Authorization", format!("Bearer {}", token))
        .header("Content-Type", "image/png")
        .body(bytes)
        .send()
        .await?
        .error_for_status()?;

    let wrap: UploadBlobResponse = res.json().await?;
    Ok(wrap.blob)
}

async fn put_record(
    client: &Client,
    token: &str,
    repo: &str,
    rkey: &str,
    record: FeedGeneratorRecord,
) -> Result<()> {
    let req = PutRecordRequest {
        repo: repo.to_string(),
        collection: "app.bsky.feed.generator".to_string(),
        rkey: rkey.to_string(),
        record,
    };

    client
        .post("https://bsky.social/xrpc/com.atproto.repo.putRecord")
        .header("Authorization", format!("Bearer {}", token))
        .json(&req)
        .send()
        .await?
        .error_for_status()?;

    Ok(())
}
