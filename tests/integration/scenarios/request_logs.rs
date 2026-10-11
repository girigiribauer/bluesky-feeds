use crate::helpers::{auth::TestAuth, client::TestClient, logs::CapturedLogs};

/// フィードを問い合わせたときのログに、そのフィード名の印（request{feed=フィード名}）が付くこと
#[tokio::test]
async fn test_feed_request_log_is_tagged_with_feed_name() {
    let logs = CapturedLogs::start();
    let client = TestClient::new().await;
    let auth = TestAuth::new("did:plc:alice");

    client
        .get_feed_skeleton(
            "at://did:example:123/app.bsky.feed.generator/helloworld",
            Some(&auth.header_value()),
        )
        .await;

    assert!(logs
        .line_containing("Received feed request")
        .contains("request{feed=helloworld}"));
}

/// Bluesky 本番のように記号を %3A などに置き換えたフィード URI で問い合わせても、ログにフィード名の印が付くこと
#[tokio::test]
async fn test_feed_request_log_is_tagged_when_feed_uri_is_percent_encoded() {
    let logs = CapturedLogs::start();
    let client = TestClient::new().await;

    client
        .get_feed_skeleton(
            "at%3A%2F%2Fdid%3Aexample%3A123%2Fapp.bsky.feed.generator%2Frealbluesky",
            None,
        )
        .await;

    assert!(logs
        .line_containing("Received feed request")
        .contains("request{feed=realbluesky}"));
}
