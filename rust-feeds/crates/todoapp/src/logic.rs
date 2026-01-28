use crate::structs::{PostView, Record};
use models::FeedItem;
use std::collections::HashSet;

pub fn filter_todos(todos: Vec<PostView>, dones: Vec<PostView>) -> Vec<FeedItem> {
    let mut done_target_uris = HashSet::new();
    for post in dones {
        if let Some(text) = post.record.get("text").and_then(|v| v.as_str()) {
            if !is_valid_keyword(text, "DONE") {
                continue;
            }
        } else {
            continue;
        }

        if let Ok(record) = serde_json::from_value::<Record>(post.record.clone()) {
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

        if let Some(text) = post.record.get("text").and_then(|v| v.as_str()) {
            if !is_valid_keyword(text, "TODO") {
                continue;
            }
        } else {
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

fn is_valid_keyword(text: &str, keyword: &str) -> bool {
    if !text.starts_with(keyword) {
        return false;
    }
    // Check char after keyword to ensure word boundary
    match text.chars().nth(keyword.len()) {
        None => true, // Exact match "TODO"
        Some(c) => c.is_whitespace() || c == ':' || c == '：', // Whitespace or Colon
        // Any other char (e.g. 'i' in todoist, or 'フ' in TODOフィード) -> False
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // --- Helper Functions ---
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

    // --- Unit Tests (Low Level) ---

    #[test]
    fn test_is_valid_keyword() {
        // 正常系: 正しいキーワードと区切り文字
        assert!(is_valid_keyword("TODO list", "TODO"), "スペース区切りはOK");
        assert!(is_valid_keyword("TODO: task", "TODO"), "コロン区切りはOK");
        assert!(is_valid_keyword("TODO：タスク", "TODO"), "全角コロン区切りはOK");
        assert!(is_valid_keyword("TODO\nnext", "TODO"), "改行区切りはOK");
        assert!(is_valid_keyword("TODO", "TODO"), "完全一致はOK");

        // 異常系: 単語の一部になっている (誤爆回避)
        assert!(!is_valid_keyword("TODOist", "TODO"), "単語の一部(todoist)はNG");
        assert!(!is_valid_keyword("TODOapp", "TODO"), "単語の一部(todoapp)はNG");
        assert!(!is_valid_keyword("TODOフィード", "TODO"), "日本語の続き文字はNG");

        // 異常系: 文中にある
        assert!(!is_valid_keyword("I will do TODO", "TODO"), "文中のTODOはNG (前方一致のみ)");

        // DONEについても同様
        assert!(is_valid_keyword("DONE task", "DONE"));
        assert!(!is_valid_keyword("DONE!", "DONE"), "記号でも定義外のものはNG (現状の実装では)");
    }

    // --- Integration Tests (High Level / Feed Logic) ---

    struct TestCase {
        name: &'static str,
        todos: Vec<PostView>,
        dones: Vec<PostView>,
        expected_uris: Vec<&'static str>,
    }

    #[test]
    fn test_filter_todos_feed_logic() {
        let cases = vec![
            TestCase {
                name: "基本: TODOのみの投稿は抽出される",
                todos: vec![create_post("uri:todo1", "TODO", None)],
                dones: vec![],
                expected_uris: vec!["uri:todo1"],
            },
            TestCase {
                name: "基本: DONEされたTODOは消える (Replyによる紐付け)",
                todos: vec![create_post("uri:todo1", "TODO", None)],
                dones: vec![create_post("uri:done1", "DONE", Some("uri:todo1"))],
                expected_uris: vec![],
            },
            TestCase {
                name: "仕様: DONE自体もキーワード判定を通っていないと有効にならない",
                todos: vec![create_post("uri:todo1", "TODO", None)],
                dones: vec![
                    create_post("uri:done_fake", "I have DONE it", Some("uri:todo1")), // 文中DONEは無効
                ],
                expected_uris: vec!["uri:todo1"], // 消えない
            },
            TestCase {
                name: "除外: TODO自体が返信である場合はフィードに出ない (ルート投稿のみ)",
                todos: vec![create_post("uri:todo_reply", "TODO", Some("uri:original"))],
                dones: vec![],
                expected_uris: vec![],
            },
            TestCase {
                name: "除外: 無関係なDONEはTODOや他のDONEに影響しない",
                todos: vec![create_post("uri:todo1", "TODO", None)],
                dones: vec![create_post("uri:done_orphan", "DONE", Some("uri:other"))],
                expected_uris: vec!["uri:todo1"],
            },
            TestCase {
                name: "複雑: 複数のTODOとDONEが混在するケース",
                todos: vec![
                    create_post("uri:todo1", "TODO active", None),
                    create_post("uri:todo2", "TODO finished", None),
                ],
                dones: vec![
                    create_post("uri:done2", "DONE", Some("uri:todo2")),
                ],
                expected_uris: vec!["uri:todo1"],
            },
        ];

        for case in cases {
            let result = filter_todos(case.todos, case.dones);
            let result_uris: Vec<String> = result.into_iter().map(|item| item.post).collect();
            assert_eq!(result_uris, case.expected_uris, "失敗したケース: {}", case.name);
        }
    }
}
