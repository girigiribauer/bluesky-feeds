use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use futures_util::SinkExt;
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::handshake::server::{Request, Response};
use tokio_tungstenite::tungstenite::Message;

fn event(row: &Value) -> String {
    let created = chrono::DateTime::from_timestamp_micros(row["time_us"].as_i64().unwrap())
        .unwrap()
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let images: Vec<Value> = row["images"]
        .as_array()
        .unwrap()
        .iter()
        .map(|img| {
            let mut v = json!({
                "alt": img["alt"],
                "image": { "$type": "blob", "ref": { "$link": img["cid"] }, "mimeType": "image/jpeg", "size": 1000 },
            });
            if let Some(a) = img.get("aspect").filter(|a| !a.is_null()) {
                v["aspectRatio"] = a.clone();
            }
            v
        })
        .collect();
    let mut record = json!({
        "$type": "app.bsky.feed.post",
        "text": row["text"],
        "createdAt": created,
        "embed": { "$type": "app.bsky.embed.images", "images": images },
    });
    if row["reply"].as_bool() == Some(true) {
        let parent = json!({ "uri": format!("at://{}/app.bsky.feed.post/parent", row["did"].as_str().unwrap()), "cid": "bafyreiparent" });
        record["reply"] = json!({ "root": parent, "parent": parent });
    }
    let labels: Vec<Value> = row["labels"].as_array().cloned().unwrap_or_default();
    if !labels.is_empty() {
        record["labels"] = json!({
            "$type": "com.atproto.label.defs#selfLabels",
            "values": labels.iter().map(|l| json!({ "val": l })).collect::<Vec<_>>(),
        });
    }
    json!({
        "did": row["did"], "time_us": row["time_us"], "kind": "commit",
        "commit": {
            "operation": "create", "rev": row["rkey"], "rkey": row["rkey"],
            "collection": "app.bsky.feed.post", "cid": "bafyreibvjvcv745gig4mvqs4hctx4zfkono4rjejm2ta6gtyzkqxfjeily",
            "record": record,
        },
    })
    .to_string()
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let (work, samples, small_dir, large_dir) = (&args[1], &args[2], &args[3], &args[4]);
    let limit: usize = args[5].parse()?;
    let per_sec: u64 = args[6].parse()?;

    let rows: Vec<Value> = std::fs::read_to_string(format!("{work}/{samples}"))?
        .lines()
        .take(limit)
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let mut files = HashMap::new();
    for (n, row) in rows.iter().enumerate() {
        for (i, img) in row["images"].as_array().unwrap().iter().enumerate() {
            files.insert(
                img["cid"].as_str().unwrap().to_string(),
                format!("{n:05}_{i}.jpg"),
            );
        }
    }
    let events: Arc<Vec<(i64, String)>> = Arc::new(
        rows.iter()
            .map(|r| (r["time_us"].as_i64().unwrap(), event(r)))
            .collect(),
    );
    let files = Arc::new(files);

    let ws = TcpListener::bind("127.0.0.1:0").await?;
    let http = TcpListener::bind("127.0.0.1:0").await?;
    println!("JETSTREAM_URL=ws://{}/subscribe", ws.local_addr()?);
    println!("SILENTVIEWS_CDN_BASE=http://{}", http.local_addr()?);

    let (work2, small2, large2) = (work.clone(), small_dir.clone(), large_dir.clone());
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = http.accept().await {
            let files = files.clone();
            let (work, small, large) = (work2.clone(), small2.clone(), large2.clone());
            tokio::spawn(async move {
                let mut buf = vec![0u8; 4096];
                let n = stream.read(&mut buf).await.unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]);
                let path = req.split_whitespace().nth(1).unwrap_or("");
                let dir = if path.contains("/avatar_thumbnail/") {
                    &small
                } else {
                    &large
                };
                let cid = path
                    .rsplit('/')
                    .next()
                    .unwrap_or("")
                    .trim_end_matches("@jpeg");
                let body = files
                    .get(cid)
                    .and_then(|f| std::fs::read(format!("{work}/{dir}/{f}")).ok());
                let resp = match body {
                    Some(b) => [format!("HTTP/1.1 200 OK\r\nContent-Type: image/jpeg\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", b.len()).into_bytes(), b].concat(),
                    None => b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(),
                };
                let _ = stream.write_all(&resp).await;
            });
        }
    });

    while let Ok((stream, _)) = ws.accept().await {
        let events = events.clone();
        tokio::spawn(async move {
            let mut cursor = 0i64;
            let callback = |req: &Request, resp: Response| {
                if let Some(q) = req.uri().query() {
                    for kv in q.split('&') {
                        if let Some(v) = kv.strip_prefix("cursor=") {
                            cursor = v.parse().unwrap_or(0);
                        }
                    }
                }
                Ok(resp)
            };
            let Ok(mut socket) = tokio_tungstenite::accept_hdr_async(stream, callback).await else {
                return;
            };
            eprintln!("consumer connected (cursor={cursor})");
            let gap = Duration::from_micros(1_000_000 / per_sec.max(1));
            for (t, e) in events.iter() {
                if *t <= cursor {
                    continue;
                }
                if socket.send(Message::Text(e.clone())).await.is_err() {
                    return;
                }
                tokio::time::sleep(gap).await;
            }
            eprintln!("all {} events sent", events.len());
            loop {
                tokio::time::sleep(Duration::from_secs(10)).await;
                if socket.send(Message::Ping(vec![])).await.is_err() {
                    return;
                }
            }
        });
    }
    Ok(())
}
