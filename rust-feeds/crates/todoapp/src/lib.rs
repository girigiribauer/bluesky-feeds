use anyhow::{Context, Result};
use base64::{engine::general_purpose, Engine as _};
use models::{FeedItem, FeedSkeletonResult};
use reqwest::Client;
use serde::Deserialize;
use std::collections::HashSet;

#[derive(Deserialize, Debug)]
struct SearchResponse {
    posts: Vec<PostView>,
}

#[derive(Deserialize, Debug, Clone)]
struct PostView {
    uri: String,
    record: serde_json::Value,
    #[serde(rename = "indexedAt")]
    indexed_at: String,
}

#[derive(Deserialize, Debug)]
struct Record {
    reply: Option<ReplyRef>,
}

#[derive(Deserialize, Debug)]
struct ReplyRef {
    parent: Link,
}

#[derive(Deserialize, Debug)]
struct Link {
    uri: String,
}

#[derive(Deserialize, Debug)]
struct JwtPayload {
    iss: String,
}

fn extract_did_from_jwt(header: &str) -> Result<String> {
    let parts: Vec<&str> = header.split_whitespace().collect();
    if parts.len() != 2 || parts[0] != "Bearer" {
        anyhow::bail!("Invalid Authorization header format");
    }
    let jwt = parts[1];
    let components: Vec<&str> = jwt.split('.').collect();
    if components.len() != 3 {
        anyhow::bail!("Invalid JWT format");
    }
    let payload_part = components[1];

    let decoded = general_purpose::URL_SAFE_NO_PAD
        .decode(payload_part)
        .or_else(|_| general_purpose::URL_SAFE.decode(payload_part))
        .context("Failed to decode JWT payload")?;

    let payload: JwtPayload = serde_json::from_slice(&decoded).context("Failed to parse JWT payload")?;
    Ok(payload.iss)
}

async fn search_posts(client: &Client, q: &str, author_did: &str) -> Result<Vec<PostView>> {
    let url = "https://public.api.bsky.app/xrpc/app.bsky.feed.searchPosts";
    let query_param = format!("{}", q); // q parameter

    let res = client
        .get(url)
        .query(&[
            ("q", query_param.as_str()),
            ("limit", "100"),
            ("author", author_did),
            ("sort", "latest"),
        ])
        .send()
        .await
        .context("Failed to send search request")?;

    if !res.status().is_success() {
        let status = res.status();
        let text = res.text().await.unwrap_or_default();
        anyhow::bail!("Search API failed: {} - {}", status, text);
    }

    let search_res: SearchResponse = res.json().await.context("Failed to parse search response")?;
    Ok(search_res.posts)
}

pub async fn get_feed_skeleton(auth_header: &str) -> Result<FeedSkeletonResult> {
    let did = extract_did_from_jwt(auth_header).context("Failed to extract DID from auth")?;
    let client = Client::new();

    // 2-Query Strategy: Parallel fetch
    let (todos_res, dones_res) = tokio::join!(
        search_posts(&client, "TODO", &did),
        search_posts(&client, "DONE", &did)
    );

    let todos = todos_res.context("Failed to fetch TODOs")?;
    let dones = dones_res.context("Failed to fetch DONEs")?;

    let feed_items = filter_todos(todos, dones);

    Ok(FeedSkeletonResult {
        cursor: None, // No cursor for now (1 page limit)
        feed: feed_items,
    })
}

fn filter_todos(todos: Vec<PostView>, dones: Vec<PostView>) -> Vec<FeedItem> {
    let mut done_target_uris = HashSet::new();
    for post in dones {
        if let Ok(record) = serde_json::from_value::<Record>(post.record) {
            if let Some(reply) = record.reply {
                done_target_uris.insert(reply.parent.uri);
            }
        }
    }

    let mut feed_items = Vec::new();
    for post in todos {
        if done_target_uris.contains(&post.uri) {
            continue;
        }

        if let Ok(record) = serde_json::from_value::<Record>(post.record.clone()) {
            if record.reply.is_none() {
                feed_items.push(FeedItem { post: post.uri });
            }
        }
    }
    feed_items
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn create_post(uri: &str, text: &str, reply_parent: Option<&str>) -> PostView {
        let reply = reply_parent.map(|parent_uri| {
            json!({
                "parent": { "uri": parent_uri }
            })
        });

        let mut record_json = json!({
            "text": text,
            "createdAt": "2024-01-01T00:00:00Z"
        });

        if let Some(r) = reply {
            record_json["reply"] = r;
        }

        PostView {
            uri: uri.to_string(),
            record: record_json,
            indexed_at: "2024-01-01T00:00:00Z".to_string(),
        }
    }

    struct TestCase {
        name: &'static str,
        todos: Vec<PostView>,
        dones: Vec<PostView>,
        expected_uris: Vec<&'static str>,
    }

    #[test]
    fn test_filter_todos_table_driven() {
        let cases = vec![
            TestCase {
                name: "Basic: Pure TODO should remain",
                todos: vec![create_post("uri:todo1", "TODO", None)],
                dones: vec![],
                expected_uris: vec!["uri:todo1"],
            },
            TestCase {
                name: "Basic: TODO with DONE should be removed",
                todos: vec![create_post("uri:todo1", "TODO", None)],
                dones: vec![create_post("uri:done1", "DONE", Some("uri:todo1"))],
                expected_uris: vec![],
            },
            TestCase {
                name: "Edge: TODO being a reply itself should be removed (only root TODOs)",
                todos: vec![create_post("uri:todo_reply", "TODO", Some("uri:original"))],
                dones: vec![],
                expected_uris: vec![],
            },
            TestCase {
                name: "Edge: DONE referencing unrelated URI should not affect TODO",
                todos: vec![create_post("uri:todo1", "TODO", None)],
                dones: vec![create_post("uri:done_orphan", "DONE", Some("uri:other"))],
                expected_uris: vec!["uri:todo1"],
            },
            TestCase {
                name: "Complex: Mixed scenario",
                todos: vec![
                    create_post("uri:todo1", "TODO active", None),
                    create_post("uri:todo2", "TODO finished", None),
                    create_post("uri:todo3", "TODO reply", Some("uri:root")),
                ],
                dones: vec![
                    create_post("uri:done2", "DONE", Some("uri:todo2")),
                ],
                expected_uris: vec!["uri:todo1"],
            },
            TestCase {
                name: "Complex: Multiple DONEs for same TODO (idempotent)",
                todos: vec![create_post("uri:todo1", "TODO", None)],
                dones: vec![
                    create_post("uri:done1a", "DONE", Some("uri:todo1")),
                    create_post("uri:done1b", "DONE", Some("uri:todo1")),
                ],
                expected_uris: vec![],
            },
        ];

        for case in cases {
            let result = filter_todos(case.todos, case.dones);
            let result_uris: Vec<String> = result.into_iter().map(|item| item.post).collect();
            assert_eq!(result_uris, case.expected_uris, "Failed case: {}", case.name);
        }
    }

    #[test]
    fn test_extract_did_from_jwt() {
        // Mock a simple JWT (header.payload.signature)
        // Payload: {"iss": "did:plc:12345", ...}
        // Base64Url for payload: eyJpc3MiOiJkaWQ6cGxjOjEyMzQ1In0 ({"iss":"did:plc:12345"})

        let valid_header = "Bearer header.eyJpc3MiOiJkaWQ6cGxjOjEyMzQ1In0.signature";
        let did = extract_did_from_jwt(valid_header).expect("Should parse valid JWT");
        assert_eq!(did, "did:plc:12345");

        let invalid_format = "Basic auth";
        assert!(extract_did_from_jwt(invalid_format).is_err());

        let invalid_jwt = "Bearer invalid.jwt";
        assert!(extract_did_from_jwt(invalid_jwt).is_err());
    }
}
