use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;
use tokio_tungstenite::tungstenite::handshake::server::{
    Callback, ErrorResponse, Request, Response,
};
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::protocol::CloseFrame;
use tokio_tungstenite::tungstenite::Message;

#[derive(Debug, Clone)]
pub enum Script {
    Text(String),
    Binary(Vec<u8>),
    Ping,
    Sleep(Duration),
    Close(u16, String),
    Drop,
}

pub struct LocalJetstream {
    port: u16,
    requested_uris: Arc<Mutex<Vec<String>>>,
    pongs: Arc<AtomicU64>,
    shutdown: Option<oneshot::Sender<()>>,
}

impl LocalJetstream {
    pub async fn start(script: Vec<Script>) -> Self {
        Self::start_sessions(vec![script]).await
    }

    pub async fn start_sessions(sessions: Vec<Vec<Script>>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let requested_uris = Arc::new(Mutex::new(Vec::new()));
        let pongs = Arc::new(AtomicU64::new(0));
        let (shutdown_tx, mut shutdown_rx) = oneshot::channel();

        let uri_log = requested_uris.clone();
        let pong_counter = pongs.clone();
        tokio::spawn(async move {
            let serving = async {
                let mut accepted = 0;
                while let Ok((stream, _)) = listener.accept().await {
                    let script = sessions[accepted.min(sessions.len() - 1)].clone();
                    accepted += 1;
                    serve(stream, script, uri_log.clone(), pong_counter.clone()).await;
                }
            };
            tokio::select! {
                _ = &mut shutdown_rx => {}
                _ = serving => {}
            }
        });

        LocalJetstream {
            port,
            requested_uris,
            pongs,
            shutdown: Some(shutdown_tx),
        }
    }

    pub fn url(&self) -> String {
        format!("ws://127.0.0.1:{}/subscribe", self.port)
    }

    pub fn requested_uri(&self) -> Option<String> {
        self.requested_uris.lock().unwrap().last().cloned()
    }

    pub fn requested_uris(&self) -> Vec<String> {
        self.requested_uris.lock().unwrap().clone()
    }

    pub fn pongs(&self) -> u64 {
        self.pongs.load(Ordering::Relaxed)
    }
}

impl Drop for LocalJetstream {
    fn drop(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
    }
}

struct UriRecorder(Arc<Mutex<Vec<String>>>);

impl Callback for UriRecorder {
    fn on_request(self, request: &Request, response: Response) -> Result<Response, ErrorResponse> {
        self.0.lock().unwrap().push(request.uri().to_string());
        Ok(response)
    }
}

async fn serve(
    stream: TcpStream,
    script: Vec<Script>,
    uri_log: Arc<Mutex<Vec<String>>>,
    pongs: Arc<AtomicU64>,
) {
    let Ok(mut socket) = tokio_tungstenite::accept_hdr_async(stream, UriRecorder(uri_log)).await
    else {
        return;
    };

    for step in script {
        let message = match step {
            Script::Text(json) => Message::Text(json),
            Script::Binary(bytes) => Message::Binary(bytes),
            Script::Ping => Message::Ping(b"are you there".to_vec()),
            Script::Sleep(duration) => {
                tokio::time::sleep(duration).await;
                continue;
            }
            Script::Close(code, reason) => Message::Close(Some(CloseFrame {
                code: CloseCode::from(code),
                reason: reason.into(),
            })),
            Script::Drop => return,
        };
        if socket.send(message).await.is_err() {
            return;
        }
    }

    while let Some(Ok(message)) = socket.next().await {
        if let Message::Pong(_) = message {
            pongs.fetch_add(1, Ordering::Relaxed);
        }
    }
}

const DID: &str = "did:plc:z72i7hdynmk6r22z27h6tvur";
const CID: &str = "bafyreibvjvcv745gig4mvqs4hctx4zfkono4rjejm2ta6gtyzkqxfjeily";
const RKEY: &str = "3l3temxelsm2a";
const TIME: &str = "2026-01-01T00:00:00.000Z";

pub fn post_json(text: &str, time_us: i64) -> String {
    json!({
        "did": DID,
        "time_us": time_us,
        "kind": "commit",
        "commit": {
            "operation": "create",
            "rev": RKEY,
            "rkey": RKEY,
            "collection": "app.bsky.feed.post",
            "cid": CID,
            "record": {
                "$type": "app.bsky.feed.post",
                "text": text,
                "createdAt": TIME,
            },
        },
    })
    .to_string()
}

pub fn delete_json(time_us: i64) -> String {
    json!({
        "did": DID,
        "time_us": time_us,
        "kind": "commit",
        "commit": {
            "operation": "delete",
            "rev": RKEY,
            "rkey": RKEY,
            "collection": "app.bsky.feed.post",
        },
    })
    .to_string()
}

pub fn poison_account_json() -> String {
    json!({
        "did": DID,
        "time_us": 1_700_000_000_000_001_i64,
        "kind": "account",
        "account": {
            "active": false,
            "did": DID,
            "seq": 1,
            "status": "desynchronized",
            "time": TIME,
        },
    })
    .to_string()
}

pub fn poison_identity_json() -> String {
    json!({
        "did": DID,
        "time_us": 1_700_000_000_000_002_i64,
        "kind": "identity",
        "identity": {
            "did": DID,
            "handle": "alice",
            "seq": 1,
            "time": TIME,
        },
    })
    .to_string()
}
