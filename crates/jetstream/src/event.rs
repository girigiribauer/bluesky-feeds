use atrium_api::app::bsky::feed::post::{Record, RecordEmbedRefs};
use atrium_api::types::Union;
use serde::Deserialize;
use serde_json::value::RawValue;

pub const POST_COLLECTION: &str = "app.bsky.feed.post";

pub type PostEmbed = Union<RecordEmbedRefs>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    Create,
    Update,
}

#[derive(Debug, Clone)]
pub enum Event {
    Post(Box<PostEvent>),
    PostDelete(PostDelete),
}

#[derive(Debug, Clone)]
pub struct PostEvent {
    pub did: String,
    pub time_us: i64,
    pub collection: String,
    pub rkey: String,
    pub cid: String,
    pub operation: Operation,
    pub record: PostRecord,
}

#[derive(Debug, Clone)]
pub struct PostDelete {
    pub did: String,
    pub time_us: i64,
    pub collection: String,
    pub rkey: String,
}

#[derive(Debug, Clone)]
pub struct PostRecord {
    pub text: String,
    pub created_at_us: Option<i64>,
    pub embed: Option<PostEmbed>,
    pub strict: bool,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SkipStats {
    pub json_error: u64,
    pub non_commit: u64,
    pub other_collection: u64,
    pub unknown_operation: u64,
    pub missing_record: u64,
    pub loose_record: u64,
    pub binary_frame: u64,
}

impl SkipStats {
    pub fn since(&self, earlier: &SkipStats) -> SkipStats {
        SkipStats {
            json_error: self.json_error - earlier.json_error,
            non_commit: self.non_commit - earlier.non_commit,
            other_collection: self.other_collection - earlier.other_collection,
            unknown_operation: self.unknown_operation - earlier.unknown_operation,
            missing_record: self.missing_record - earlier.missing_record,
            loose_record: self.loose_record - earlier.loose_record,
            binary_frame: self.binary_frame - earlier.binary_frame,
        }
    }

    pub fn merge(&mut self, other: &SkipStats) {
        self.json_error += other.json_error;
        self.non_commit += other.non_commit;
        self.other_collection += other.other_collection;
        self.unknown_operation += other.unknown_operation;
        self.missing_record += other.missing_record;
        self.loose_record += other.loose_record;
        self.binary_frame += other.binary_frame;
    }
}

impl Event {
    pub fn time_us(&self) -> i64 {
        match self {
            Event::Post(post) => post.time_us,
            Event::PostDelete(delete) => delete.time_us,
        }
    }
}

impl PostEvent {
    pub fn uri(&self) -> String {
        format!("at://{}/{}/{}", self.did, self.collection, self.rkey)
    }

    pub fn indexed_at_us(&self) -> i64 {
        self.record
            .created_at_us
            .map(|created| created.min(self.time_us))
            .unwrap_or(self.time_us)
    }
}

impl PostDelete {
    pub fn uri(&self) -> String {
        format!("at://{}/{}/{}", self.did, self.collection, self.rkey)
    }
}

#[derive(Deserialize)]
struct RawEvent<'a> {
    did: String,
    time_us: i64,
    kind: String,
    #[serde(default, borrow)]
    commit: Option<RawCommit<'a>>,
}

#[derive(Deserialize)]
struct RawCommit<'a> {
    operation: String,
    collection: String,
    rkey: String,
    #[serde(default)]
    cid: Option<String>,
    #[serde(default, borrow)]
    record: Option<&'a RawValue>,
}

pub fn parse_event(json: &str, skips: &mut SkipStats) -> Result<Option<Event>, serde_json::Error> {
    let raw: RawEvent = match serde_json::from_str(json) {
        Ok(raw) => raw,
        Err(e) => {
            skips.json_error += 1;
            return Err(e);
        }
    };

    if raw.kind != "commit" {
        skips.non_commit += 1;
        return Ok(None);
    }

    let Some(commit) = raw.commit else {
        skips.missing_record += 1;
        return Ok(None);
    };

    if commit.collection != POST_COLLECTION {
        skips.other_collection += 1;
        return Ok(None);
    }

    if commit.operation == "delete" {
        return Ok(Some(Event::PostDelete(PostDelete {
            did: raw.did,
            time_us: raw.time_us,
            collection: commit.collection,
            rkey: commit.rkey,
        })));
    }

    let operation = match commit.operation.as_str() {
        "create" => Operation::Create,
        "update" => Operation::Update,
        _ => {
            skips.unknown_operation += 1;
            return Ok(None);
        }
    };

    let (Some(cid), Some(record)) = (commit.cid, commit.record) else {
        skips.missing_record += 1;
        return Ok(None);
    };

    Ok(Some(Event::Post(Box::new(PostEvent {
        did: raw.did,
        time_us: raw.time_us,
        collection: commit.collection,
        rkey: commit.rkey,
        cid,
        operation,
        record: parse_post_record(record, skips),
    }))))
}

fn parse_post_record(raw: &RawValue, skips: &mut SkipStats) -> PostRecord {
    if let Ok(record) = serde_json::from_str::<Record>(raw.get()) {
        return PostRecord {
            text: record.text.clone(),
            created_at_us: Some(record.created_at.as_ref().timestamp_micros()),
            embed: record.embed.clone(),
            strict: true,
        };
    }

    skips.loose_record += 1;
    let value: serde_json::Value =
        serde_json::from_str(raw.get()).unwrap_or(serde_json::Value::Null);

    PostRecord {
        text: value
            .get("text")
            .and_then(|text| text.as_str())
            .unwrap_or_default()
            .to_string(),
        created_at_us: value
            .get("createdAt")
            .and_then(|created| created.as_str())
            .and_then(parse_created_at_us),
        embed: value
            .get("embed")
            .cloned()
            .and_then(|embed| serde_json::from_value::<PostEmbed>(embed).ok()),
        strict: false,
    }
}

fn parse_created_at_us(text: &str) -> Option<i64> {
    if let Ok(parsed) = chrono::DateTime::parse_from_rfc3339(text) {
        return Some(parsed.timestamp_micros());
    }
    for format in ["%Y-%m-%dT%H:%M:%S%.f", "%Y-%m-%d %H:%M:%S%.f"] {
        if let Ok(parsed) = chrono::NaiveDateTime::parse_from_str(text, format) {
            return Some(parsed.and_utc().timestamp_micros());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    use serde_json::{json, Value};

    const DID: &str = "did:plc:z72i7hdynmk6r22z27h6tvur";
    const CID: &str = "bafyreibvjvcv745gig4mvqs4hctx4zfkono4rjejm2ta6gtyzkqxfjeily";
    const RKEY: &str = "3l3temxelsm2a";
    const TIME_US: i64 = 1_700_000_000_000_000;
    const TIME: &str = "2026-01-01T00:00:00.000Z";
    const IMAGE_CID: &str = "bafkreib7o2gowpvz2qh6ytvgdpkvcbfzowvvrqvgl3cztrmjvhmtxyfwfa";

    fn commit(operation: &str, collection: &str, record: Option<Value>) -> String {
        let mut commit = json!({
            "operation": operation,
            "rev": RKEY,
            "rkey": RKEY,
            "collection": collection,
        });
        if let Some(record) = record {
            commit["cid"] = json!(CID);
            commit["record"] = record;
        }
        json!({ "did": DID, "time_us": TIME_US, "kind": "commit", "commit": commit }).to_string()
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
        commit(
            "create",
            POST_COLLECTION,
            Some(post_record(created_at, extra)),
        )
    }

    fn images_embed() -> Value {
        json!({
            "$type": "app.bsky.embed.images",
            "images": [{
                "alt": "",
                "image": {
                    "$type": "blob",
                    "ref": { "$link": IMAGE_CID },
                    "mimeType": "image/jpeg",
                    "size": 1000,
                },
            }],
        })
    }

    fn event(kind: &str, body: Value) -> String {
        let mut event = json!({ "did": DID, "time_us": TIME_US, "kind": kind });
        if let Value::Object(body) = body {
            for (key, value) in body {
                event[key] = value;
            }
        }
        event.to_string()
    }

    fn parse(json: &str) -> Result<Option<Event>, serde_json::Error> {
        parse_event(json, &mut SkipStats::default())
    }

    fn parse_post(json: &str) -> PostEvent {
        match parse(json) {
            Ok(Some(Event::Post(post))) => *post,
            other => panic!("投稿として読めなかった: {:?}", other.map(|e| e.is_some())),
        }
    }

    /// 正常な投稿から did / rkey / cid / 本文 / 受信時刻が取り出せること
    #[test]
    fn test_parse_create_post() {
        let event = parse_post(&post(TIME, json!({ "langs": ["ja"] })));

        assert_eq!(event.did, DID);
        assert_eq!(event.time_us, TIME_US);
        assert_eq!(event.rkey, "3l3temxelsm2a");
        assert_eq!(event.cid, CID);
        assert_eq!(event.operation, Operation::Create);
        assert_eq!(event.record.text, "hello");
        assert!(event.record.strict);
        assert_eq!(
            event.uri(),
            format!("at://{DID}/app.bsky.feed.post/3l3temxelsm2a")
        );
    }

    /// 編集（update）も作成と同じ形で読めること
    #[test]
    fn test_parse_update_is_read_like_create() {
        let json = commit(
            "update",
            POST_COLLECTION,
            Some(post_record(TIME, json!({}))),
        );
        let event = parse_post(&json);

        assert_eq!(event.operation, Operation::Update);
        assert_eq!(event.record.text, "hello");
    }

    /// 投稿日時が壊れていても投稿を捨てないこと
    #[test]
    fn test_parse_keeps_post_with_broken_created_at() {
        for created_at in ["", "2026-01-01T00:00:00.000", "2026-01-01 00:00:00.000Z"] {
            let event = parse_post(&post(created_at, json!({})));
            assert_eq!(event.record.text, "hello", "createdAt={created_at:?}");
            assert!(!event.record.strict, "createdAt={created_at:?}");
        }
    }

    /// 投稿日時が読めないときは受信時刻を並び順に使うこと
    #[test]
    fn test_indexed_at_falls_back_to_time_us() {
        let event = parse_post(&post("", json!({})));

        assert_eq!(event.record.created_at_us, None);
        assert_eq!(event.indexed_at_us(), TIME_US);
    }

    /// 投稿日時が読めるときはそれを並び順に使うこと
    #[test]
    fn test_indexed_at_uses_created_at_when_readable() {
        let event = parse_post(&post("2023-11-14T22:13:19.000Z", json!({})));

        assert_eq!(event.record.created_at_us, Some(1_699_999_999_000_000));
        assert_eq!(event.indexed_at_us(), 1_699_999_999_000_000);
    }

    /// 投稿日時が未来のときは受信時刻で頭打ちにすること
    #[test]
    fn test_indexed_at_is_capped_by_time_us() {
        let event = parse_post(&post("2099-01-01T00:00:00.000Z", json!({})));

        assert_eq!(event.indexed_at_us(), TIME_US);
    }

    /// 言語コードが規格外でも、本文と添付は読めること
    #[test]
    fn test_parse_keeps_text_and_embed_when_langs_are_broken() {
        let json = post(
            TIME,
            json!({ "langs": ["日本語"], "embed": images_embed() }),
        );

        let event = parse_post(&json);

        assert!(!event.record.strict);
        assert_eq!(event.record.text, "hello");
        assert!(event.record.embed.is_some());
    }

    /// 添付が読めなくても本文は読め、添付なしとして扱うこと
    #[test]
    fn test_parse_keeps_text_when_embed_is_broken() {
        let json = post(TIME, json!({ "embed": { "images": [] } }));

        let event = parse_post(&json);

        assert_eq!(event.record.text, "hello");
        assert!(event.record.embed.is_none());
    }

    /// アカウント状態とハンドル変更は中身を読まずに読み飛ばし、エラーにしないこと
    #[test]
    fn test_parse_skips_account_and_identity_without_reading_them() {
        let account = event(
            "account",
            json!({ "account": { "active": false, "did": DID, "seq": 1, "status": "desynchronized", "time": TIME } }),
        );
        let identity = event(
            "identity",
            json!({ "identity": { "did": DID, "handle": "alice", "seq": 1, "time": TIME } }),
        );

        let mut skips = SkipStats::default();
        assert!(parse_event(&account, &mut skips).unwrap().is_none());
        assert!(parse_event(&identity, &mut skips).unwrap().is_none());
        assert_eq!(skips.non_commit, 2);
        assert_eq!(skips.json_error, 0);
    }

    /// 未知の種類のイベントを読み飛ばすこと
    #[test]
    fn test_parse_skips_unknown_kind() {
        let json = event("somethingnew", json!({}));

        let mut skips = SkipStats::default();
        assert!(parse_event(&json, &mut skips).unwrap().is_none());
        assert_eq!(skips.non_commit, 1);
    }

    /// 投稿以外のコレクションを読み飛ばすこと
    #[test]
    fn test_parse_skips_other_collection() {
        let json = commit(
            "create",
            "app.bsky.feed.like",
            Some(json!({ "foo": "bar" })),
        );

        let mut skips = SkipStats::default();
        assert!(parse_event(&json, &mut skips).unwrap().is_none());
        assert_eq!(skips.other_collection, 1);
    }

    /// 削除イベントを削除として読めること
    #[test]
    fn test_parse_delete_becomes_post_delete() {
        let json = commit("delete", POST_COLLECTION, None);

        match parse(&json) {
            Ok(Some(Event::PostDelete(delete))) => {
                assert_eq!(delete.rkey, "3l3temxelsm2a");
                assert_eq!(delete.time_us, TIME_US);
            }
            other => panic!("削除として読めなかった: {}", other.is_ok()),
        }
    }

    /// 本体（record / cid）のない作成イベントを読み飛ばすこと
    #[test]
    fn test_parse_skips_create_without_record() {
        let json = commit("create", POST_COLLECTION, None);

        let mut skips = SkipStats::default();
        assert!(parse_event(&json, &mut skips).unwrap().is_none());
        assert_eq!(skips.missing_record, 1);
    }

    /// 未知の操作を読み飛ばすこと
    #[test]
    fn test_parse_skips_unknown_operation() {
        let json = commit("archive", POST_COLLECTION, Some(json!({ "text": "hello" })));

        let mut skips = SkipStats::default();
        assert!(parse_event(&json, &mut skips).unwrap().is_none());
        assert_eq!(skips.unknown_operation, 1);
    }

    /// 壊れた JSON はエラーとして数え、panic しないこと
    #[test]
    fn test_parse_malformed_json_returns_error() {
        let mut skips = SkipStats::default();

        assert!(parse_event("{\"did\":", &mut skips).is_err());
        assert!(parse_event("", &mut skips).is_err());
        assert_eq!(skips.json_error, 2);
    }

    /// 日時は多少崩れた書き方でも読み、読めないものは None にすること
    #[test]
    fn test_parse_created_at_us_accepts_loose_formats() {
        const UTC_MIDNIGHT: i64 = 1_767_225_600_000_000;

        assert_eq!(
            parse_created_at_us("2026-01-01T00:00:00.000Z"),
            Some(UTC_MIDNIGHT)
        );
        assert_eq!(
            parse_created_at_us("2026-01-01T00:00:00Z"),
            Some(UTC_MIDNIGHT)
        );
        assert_eq!(
            parse_created_at_us("2026-01-01T00:00:00.000+09:00"),
            Some(1_767_193_200_000_000)
        );
        assert_eq!(
            parse_created_at_us("2026-01-01T00:00:00.000"),
            Some(UTC_MIDNIGHT)
        );
        assert_eq!(
            parse_created_at_us("2026-01-01 00:00:00.000Z"),
            Some(UTC_MIDNIGHT)
        );
        assert_eq!(
            parse_created_at_us("2026-01-01 00:00:00.000"),
            Some(UTC_MIDNIGHT)
        );
        assert_eq!(
            parse_created_at_us("2026-01-01T00:00:00.123456Z"),
            Some(UTC_MIDNIGHT + 123_456)
        );
        assert_eq!(parse_created_at_us(""), None);
        assert_eq!(parse_created_at_us("きのう"), None);
    }

    /// どの種類のイベントからもカーソル用の受信時刻を取り出せること
    #[test]
    fn test_event_time_us_is_readable_for_every_variant() {
        let created = parse(&post(TIME, json!({}))).unwrap().unwrap();
        let deleted = parse(&commit("delete", POST_COLLECTION, None))
            .unwrap()
            .unwrap();

        assert_eq!(created.time_us(), TIME_US);
        assert_eq!(deleted.time_us(), TIME_US);
    }
}
