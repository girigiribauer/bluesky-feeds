use jetstream::event::{parse_event, Event, SkipStats};
use serde_json::{json, Value};

const DID: &str = "did:plc:z72i7hdynmk6r22z27h6tvur";
const CID: &str = "bafyreibvjvcv745gig4mvqs4hctx4zfkono4rjejm2ta6gtyzkqxfjeily";
const RKEY: &str = "3l3temxelsm2a";
const TIME_US: i64 = 1_700_000_000_000_000;
const TIME: &str = "2026-01-01T00:00:00.000Z";
const STRICT_POST: &str = "投稿として通る（厳密に読めた）";
const LOOSE_POST: &str = "投稿として通る（読み直しで救済）";
const SKIPPED: &str = "読み飛ばす";
const BROKEN_JSON: &str = "壊れた JSON として数える";

fn envelope(did: &str, collection: &str, record: Value) -> String {
    json!({
        "did": did,
        "time_us": TIME_US,
        "kind": "commit",
        "commit": {
            "operation": "create",
            "rev": RKEY,
            "rkey": RKEY,
            "collection": collection,
            "cid": CID,
            "record": record,
        },
    })
    .to_string()
}

fn commit(did: &str, record: Value) -> String {
    envelope(did, "app.bsky.feed.post", record)
}

fn post_record(created_at: &str, extra: Value) -> Value {
    let mut record = json!({
        "$type": "app.bsky.feed.post",
        "text": "hello",
        "createdAt": created_at,
    });
    if let Value::Object(extra) = extra {
        for (key, value) in extra {
            record[key] = value;
        }
    }
    record
}

fn post(created_at: &str, extra: Value) -> String {
    commit(DID, post_record(created_at, extra))
}

fn langs(value: Value) -> String {
    post(TIME, json!({ "langs": value }))
}

fn account(status: Option<&str>) -> String {
    let mut account = json!({ "active": false, "did": DID, "seq": 1, "time": TIME });
    if let Some(status) = status {
        account["status"] = json!(status);
    }
    json!({ "did": DID, "time_us": TIME_US, "kind": "account", "account": account }).to_string()
}

fn identity(handle: Option<&str>) -> String {
    let mut identity = json!({ "did": DID, "seq": 1, "time": TIME });
    if let Some(handle) = handle {
        identity["handle"] = json!(handle);
    }
    json!({ "did": DID, "time_us": TIME_US, "kind": "identity", "identity": identity }).to_string()
}

fn describe(json: &str) -> &'static str {
    let mut skips = SkipStats::default();
    match parse_event(json, &mut skips) {
        Ok(Some(Event::Post(post))) => {
            if post.record.strict {
                STRICT_POST
            } else {
                LOOSE_POST
            }
        }
        Ok(Some(Event::PostDelete(_))) => SKIPPED,
        Ok(None) => SKIPPED,
        Err(_) => BROKEN_JSON,
    }
}

/// 本番で接続を殺していたデータのどれも、接続を切る理由にならないこと
#[test]
fn no_payload_can_kill_the_connection() {
    let cases: Vec<(&str, &str, String)> = vec![
        ("基準", "正常な投稿", post(TIME, json!({}))),
        ("langs", r#"["ja"]"#, langs(json!(["ja"]))),
        ("langs", r#"["ja-JP"]"#, langs(json!(["ja-JP"]))),
        ("langs", r#"["ja_JP"] 下線"#, langs(json!(["ja_JP"]))),
        ("langs", r#"["日本語"]"#, langs(json!(["日本語"]))),
        ("langs", r#"[""] 空"#, langs(json!([""]))),
        ("langs", r#"["EN"] 大文字"#, langs(json!(["EN"]))),
        ("langs", r#"["i-klingon"]"#, langs(json!(["i-klingon"]))),
        ("langs", r#"["zh-Hant-TW"]"#, langs(json!(["zh-Hant-TW"]))),
        (
            "langs",
            r#"["ja","en","xx"]"#,
            langs(json!(["ja", "en", "xx"])),
        ),
        (
            "createdAt",
            "ミリ秒なし Z",
            post("2026-01-01T00:00:00Z", json!({})),
        ),
        (
            "createdAt",
            "+09:00",
            post("2026-01-01T00:00:00.000+09:00", json!({})),
        ),
        (
            "createdAt",
            "タイムゾーンなし",
            post("2026-01-01T00:00:00.000", json!({})),
        ),
        (
            "createdAt",
            "T が空白",
            post("2026-01-01 00:00:00.000Z", json!({})),
        ),
        (
            "createdAt",
            "ナノ秒 9 桁",
            post("2026-01-01T00:00:00.000000000Z", json!({})),
        ),
        ("createdAt", "空文字", post("", json!({}))),
        (
            "createdAt",
            "西暦 5 桁",
            post("12026-01-01T00:00:00.000Z", json!({})),
        ),
        (
            "$type",
            "未知の種類",
            commit(DID, json!({ "$type": "com.example.unknown", "foo": "bar" })),
        ),
        (
            "$type",
            "$type なし",
            commit(DID, json!({ "text": "hello", "createdAt": TIME })),
        ),
        ("account", "status なし", account(None)),
        ("account", "deactivated", account(Some("deactivated"))),
        ("account", "takendown", account(Some("takendown"))),
        ("account", "desynchronized", account(Some("desynchronized"))),
        ("account", "throttled", account(Some("throttled"))),
        (
            "account",
            "takenDown 大文字混在",
            account(Some("takenDown")),
        ),
        (
            "identity",
            "handle あり",
            identity(Some("alice.bsky.social")),
        ),
        ("identity", "handle なし", identity(None)),
        (
            "identity",
            "handle に下線",
            identity(Some("alice_bob.bsky.social")),
        ),
        ("identity", "handle が空", identity(Some(""))),
        (
            "did",
            "did:web",
            commit("did:web:example.com", post_record(TIME, json!({}))),
        ),
        (
            "did",
            "形式違反",
            commit("not-a-did", post_record(TIME, json!({}))),
        ),
    ];

    println!();
    println!("{:<12} {:<28} 結果", "分類", "与えたデータ");
    println!("{}", "-".repeat(76));
    let mut killed = Vec::new();
    for (category, label, json) in &cases {
        let result = describe(json);
        println!("{:<12} {:<28} {}", category, label, result);
        if result == BROKEN_JSON {
            killed.push(format!("{} / {}", category, label));
        }
    }
    println!();

    assert!(
        killed.is_empty(),
        "壊れた JSON として数えられたデータがある: {:?}",
        killed
    );
    assert_eq!(describe(&post(TIME, json!({}))), STRICT_POST);
    assert_eq!(describe(&account(Some("desynchronized"))), SKIPPED);
    assert_eq!(describe(&identity(Some("alice"))), SKIPPED);
    assert_eq!(describe(&post("", json!({}))), LOOSE_POST);
}

/// 規格外のハンドルは、どれも読み飛ばすこと
#[test]
fn no_handle_can_kill_the_connection() {
    let handles = vec![
        "alice.bsky.social",
        "handle.invalid",
        "ALICE.BSKY.SOCIAL",
        "alice.bsky.social.",
        "alice",
        "a.b",
        "xn--80ak6aa92e.com",
        "alice..bsky.social",
        "-alice.bsky.social",
        "alice.bsky.social-",
        "alice.123",
        "1alice.bsky.social",
    ];

    println!();
    println!("{:<28} 結果", "handle の値");
    println!("{}", "-".repeat(56));
    for handle in &handles {
        let result = describe(&identity(Some(handle)));
        println!("{:<28} {}", handle, result);
        assert_eq!(result, SKIPPED, "handle={handle}");
    }
    println!();
}

/// アカウント状態とハンドル変更は、中身を読まずに読み飛ばすこと
#[test]
fn account_and_identity_are_skipped_without_reading() {
    let mut skips = SkipStats::default();

    assert!(parse_event(&account(Some("desynchronized")), &mut skips)
        .unwrap()
        .is_none());
    assert!(parse_event(&identity(Some("alice")), &mut skips)
        .unwrap()
        .is_none());

    assert_eq!(skips.non_commit, 2);
    assert_eq!(skips.missing_record, 0);
}

/// 読み飛ばした理由が区別できること（投稿以外のコレクションとして捨てたこと）
#[test]
fn other_collections_are_skipped_without_reading() {
    let like = envelope(
        DID,
        "app.bsky.feed.like",
        json!({ "subject": { "uri": "at://x" } }),
    );

    let mut skips = SkipStats::default();
    assert!(parse_event(&like, &mut skips).unwrap().is_none());

    assert_eq!(skips.other_collection, 1);
    assert_eq!(skips.non_commit, 0);
}

/// 壊れた JSON は数えるだけで、接続を切る理由にしないこと
#[test]
fn malformed_json_is_counted_but_not_fatal() {
    let mut skips = SkipStats::default();

    assert!(parse_event("{\"did\":", &mut skips).is_err());
    assert!(parse_event("not json at all", &mut skips).is_err());
    assert_eq!(skips.json_error, 2);
}
