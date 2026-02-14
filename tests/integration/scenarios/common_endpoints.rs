use crate::helpers::client::TestClient;
use axum::http::StatusCode;

/// 観点: /health エンドポイントが 200 OK を返すか
#[tokio::test]
async fn test_health_check() {
    let client = TestClient::new().await;
    let (status, body) = client.get_health().await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "OK");
}

/// 観点: /.well-known/did.json が正しい構造とIDを返すか
#[tokio::test]
async fn test_did_json_response() {
    let client = TestClient::new().await;
    let (status, body) = client.get_did_json().await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["id"], "did:web:feeds.bsky.girigiribauer.com");

    let services = body["service"].as_array().unwrap();
    let first_service = &services[0];
    assert_eq!(
        first_service["serviceEndpoint"],
        "https://feeds.bsky.girigiribauer.com"
    );
}
