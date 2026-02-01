use anyhow::{Context, Result};
use dotenv::dotenv;
use reqwest::{Client, ClientBuilder};
use serde::{Deserialize, Serialize};
use std::env;

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
}

#[derive(Serialize)]
struct DeleteRecordRequest {
    repo: String,
    collection: String,
    rkey: String,
}

// Ported from packages/shared/src/index.ts (only needed for validation if STRICT, but simple arg check is enough)
const AVAILABLE_SERVICES: &[&str] = &["helloworld", "todoapp", "oneyearago"];

#[tokio::main]
async fn main() -> Result<()> {
    dotenv().ok();

    // Check args
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: unpublish_feed <feed_service_id>");
        std::process::exit(1);
    }
    let target_service = &args[1];

    if !AVAILABLE_SERVICES.contains(&target_service.as_str()) {
        eprintln!("Warning: '{}' is not in the known service list. Proceeding anyway...", target_service);
    }

    let handle = env::var("APP_HANDLE").context("APP_HANDLE not set in .env (checked current and parent directories)")?;
    let password = env::var("APP_PASSWORD").context("APP_PASSWORD not set in .env (checked current and parent directories)")?;

    let client = ClientBuilder::new().build()?;

    println!("Logging in as {}...", handle);
    let session = create_session(&client, &handle, &password).await?;
    println!("Login successful. DID: {}", session.did);

    println!("Unpublishing feed '{}'...", target_service);
    delete_record(&client, &session.access_jwt, &session.did, target_service).await?;
    println!("Successfully unpublished {}", target_service);

    Ok(())
}

async fn create_session(client: &Client, identifier: &str, password: &str) -> Result<CreateSessionResponse> {
    let res = client.post("https://bsky.social/xrpc/com.atproto.server.createSession")
        .json(&CreateSessionRequest { identifier, password })
        .send()
        .await?
        .error_for_status()?;

    Ok(res.json().await?)
}

async fn delete_record(client: &Client, token: &str, repo: &str, rkey: &str) -> Result<()> {
    let req = DeleteRecordRequest {
        repo: repo.to_string(),
        collection: "app.bsky.feed.generator".to_string(),
        rkey: rkey.to_string(),
    };

    client.post("https://bsky.social/xrpc/com.atproto.repo.deleteRecord")
        .header("Authorization", format!("Bearer {}", token))
        .json(&req)
        .send()
        .await?
        .error_for_status()?;

    Ok(())
}
