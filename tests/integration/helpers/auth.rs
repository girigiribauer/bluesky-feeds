use base64::{engine::general_purpose, Engine as _};
use serde_json::json;

pub struct TestAuth {
    pub did: String,
}

impl TestAuth {
    pub fn new(did: &str) -> Self {
        Self {
            did: did.to_string(),
        }
    }

    pub fn generate_token(&self) -> String {
        let header = json!({
            "alg": "ES256",
            "typ": "JWT"
        });

        let payload = json!({
            "iss": self.did,
            "aud": "did:web:feeds.bsky.girigiribauer.com",
            "exp": 1999999999, // far future
            "iat": 1700000000
        });

        let header_part = general_purpose::URL_SAFE_NO_PAD.encode(header.to_string());
        let payload_part = general_purpose::URL_SAFE_NO_PAD.encode(payload.to_string());
        let signature_part = "dummy_signature"; // App doesn't verify signature yet

        format!("{}.{}.{}", header_part, payload_part, signature_part)
    }

    pub fn header_value(&self) -> String {
        format!("Bearer {}", self.generate_token())
    }
}
