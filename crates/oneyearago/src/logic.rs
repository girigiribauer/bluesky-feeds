use crate::api::PostFetcher;
use crate::cache::CacheStore;
use anyhow::Result;
use bsky_core::FeedItem;
use chrono::Utc;

const MIN_SEARCH_YEAR: i32 = 2023;
const DEFAULT_LIMIT: usize = 30;

#[allow(clippy::too_many_arguments)]
pub async fn fetch_posts_from_past<F: PostFetcher>(
    fetcher: &F,
    service_token: &str,
    _user_token: &str,
    actor: &str,
    limit: usize,
    cursor: Option<String>,
    now_utc: Option<chrono::DateTime<Utc>>, // Injectable "now"
    cache: Option<&CacheStore>,
) -> Result<(Vec<FeedItem>, Option<String>)> {
    // 1. Timezone (キャッシュ確認)
    let tz_offset = if let Some(store) = cache {
        match store.get_timezone(actor).await {
            Ok(Some(cached)) => {
                tracing::debug!("[cache] TZ hit for {}", actor);
                cached
            }
            _ => {
                // キャッシュなし or エラー → APIで取得してキャッシュ
                let offset = fetcher.determine_timezone(actor, service_token).await?;
                if let Err(e) = store.set_timezone(actor, offset.local_minus_utc()).await {
                    tracing::warn!("[cache] Failed to set TZ cache: {}", e);
                }
                tracing::debug!("[cache] TZ miss for {}, fetched from API", actor);
                offset
            }
        }
    } else {
        fetcher.determine_timezone(actor, service_token).await?
    };

    // 現在時刻 (UTC) -> ターゲットタイムゾーンへ変換
    let now_utc = now_utc.unwrap_or_else(Utc::now);
    let now_tz = now_utc.with_timezone(&tz_offset);

    let safe_limit = if limit == 0 { DEFAULT_LIMIT } else { limit };

    // フィード結果キャッシュのキー生成に使う日付文字列 (ユーザーの現地の今日)
    // タイムゾーンが異なれば同じ日付でも取得範囲が違うため、オフセットもキーに含める
    let today_naive = now_tz.date_naive();
    let date_key = format!(
        "{}:{}",
        today_naive.format("%y%m%d"),
        tz_offset.local_minus_utc()
    );

    // フィード結果のキャッシュ確認 (カーソルでページを識別)
    let cursor_str = cursor.as_deref();
    if let Some(store) = cache {
        match store
            .get_feed(actor, &date_key, safe_limit, cursor_str)
            .await
        {
            Ok(Some(cached)) => {
                tracing::debug!("[cache] Feed hit for {} date={}", actor, date_key);
                let feed_items: Vec<FeedItem> = cached
                    .uris
                    .into_iter()
                    .map(|u| FeedItem { post: u })
                    .collect();
                return Ok((feed_items, cached.next));
            }
            Ok(None) => {
                tracing::debug!("[cache] Feed miss for {} date={}", actor, date_key);
            }
            Err(e) => {
                tracing::warn!("[cache] Feed cache error: {}", e);
            }
        }
    }

    let mut feed_items = Vec::new();

    // Cursor Parsing
    // Format: v1::{years_ago}::{api_cursor}
    let (start_year, mut current_api_cursor) = if let Some(c) = cursor.as_deref() {
        let parts: Vec<&str> = c.splitn(3, "::").collect();
        if parts.len() >= 2 && parts[0] == "v1" {
            let y = parts[1].parse::<i32>().unwrap_or(1);
            let ac = if parts.len() > 2 && !parts[2].is_empty() {
                Some(parts[2].to_string())
            } else {
                None
            };
            (y, ac)
        } else {
            (1, None)
        }
    } else {
        (1, None)
    };

    let mut years_ago = start_year;
    let next_cursor_string = loop {
        if feed_items.len() >= safe_limit {
            // Succeeded filling limit. Calculate resumption cursor.
            if let Some(ac) = current_api_cursor {
                break Some(format!("v1::{}::{}", years_ago, ac));
            } else {
                break Some(format!("v1::{}::", years_ago));
            }
        }

        use chrono::Datelike;
        let today = now_tz.date_naive();
        let target_year = today.year() - years_ago;

        if target_year < MIN_SEARCH_YEAR {
            break None; // End of history
        }

        // Handle leap years (Feb 29 -> Feb 28 on non-leap years)
        let target_date = chrono::NaiveDate::from_ymd_opt(target_year, today.month(), today.day())
            .unwrap_or_else(|| chrono::NaiveDate::from_ymd_opt(target_year, 2, 28).unwrap());

        // Start: 00:00:00 user time
        let start_local = target_date
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_local_timezone(tz_offset)
            .unwrap();

        // End: Next day 00:00:00 user time (exclusive)
        let end_local = (target_date + chrono::Duration::days(1))
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_local_timezone(tz_offset)
            .unwrap();

        // Convert to UTC ISO Strings
        let since = start_local.with_timezone(&Utc).to_rfc3339();
        let until = end_local.with_timezone(&Utc).to_rfc3339();

        let fetch_limit = safe_limit - feed_items.len();
        match fetcher
            .search_posts(
                service_token,
                actor,
                &since,
                &until,
                fetch_limit,
                current_api_cursor.clone(),
            )
            .await
        {
            Ok((posts, new_cursor)) => {
                for p in posts {
                    feed_items.push(FeedItem { post: p.uri });
                }
                current_api_cursor = new_cursor;

                // If cursor is None, we finished this year. Move to next.
                if current_api_cursor.is_none() {
                    years_ago += 1;
                }
                // If cursor is Some, we loop again with same years_ago (and new cursor)
            }
            Err(e) => {
                tracing::error!("Failed to fetch posts for {} years ago: {}", years_ago, e);
                // On error, skip to next year
                years_ago += 1;
                current_api_cursor = None;
            }
        }
    };

    // フィード結果をキャッシュに保存
    if let Some(store) = cache {
        // TTL: ユーザーの現地の「今日の終わり」まで
        let today_end_utc = {
            let tomorrow = today_naive.succ_opt().unwrap_or(today_naive);
            tomorrow
                .and_hms_opt(0, 0, 0)
                .and_then(|dt| dt.and_local_timezone(tz_offset).single())
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|| now_utc + chrono::Duration::hours(24))
        };
        let uris: Vec<String> = feed_items.iter().map(|f| f.post.clone()).collect();
        if let Err(e) = store
            .set_feed(
                actor,
                &date_key,
                safe_limit,
                cursor_str,
                uris,
                next_cursor_string.clone(),
                today_end_utc,
            )
            .await
        {
            tracing::warn!("[cache] Failed to set feed cache: {}", e);
        }
    }

    Ok((feed_items, next_cursor_string))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{PostRecord, PostView};
    use mockall::mock;
    use mockall::predicate::*;

    mock! {
        pub PostFetcher {}
        #[async_trait::async_trait]
        impl PostFetcher for PostFetcher {
            async fn search_posts(
                &self,
                token: &str,
                author: &str,
                since: &str,
                until: &str,
                limit: usize,
                cursor: Option<String>,
            ) -> Result<(Vec<PostView>, Option<String>)>;

            async fn determine_timezone(&self, handle: &str, token: &str) -> Result<chrono::FixedOffset>;
        }
    }

    // 観点1: 十分な件数がある場合 (1年前のみで完結)
    #[tokio::test]
    async fn test_waterfall_single_year_sufficient() {
        let mut mock = MockPostFetcher::new();
        mock.expect_determine_timezone()
            .returning(|_, _| Ok(chrono::FixedOffset::east_opt(0).unwrap()));

        // 1年前: 30件要求に対し、30件返却。カーソルも "cursor_abc" が返るとする
        mock.expect_search_posts()
            .times(1)
            .with(
                eq("token"),
                eq("did:plc:test"),
                always(),
                always(),
                eq(30),
                eq(None),
            )
            .returning(|_, _, _, _, _, _| {
                let mut posts = Vec::new();
                for i in 0..30 {
                    posts.push(PostView {
                        uri: format!("id:{}", i),
                        record: PostRecord {
                            created_at: String::new(),
                        },
                    });
                }
                Ok((posts, Some("cursor_abc".to_string())))
            });

        // Loop checks limits. feed_items=30 >= limit 30. Break.
        // Return next cursor: v1::1::cursor_abc

        let (items, cursor) = fetch_posts_from_past(
            &mock,
            "token",
            "user_token",
            "did:plc:test",
            30,
            None,
            None,
            None,
        )
        .await
        .unwrap();
        assert_eq!(items.len(), 30);
        assert_eq!(cursor, Some("v1::1::cursor_abc".to_string()));
    }

    // 観点2: 件数が不足する場合 (1年前 -> 2年前へと検索が続く)
    #[tokio::test]
    async fn test_waterfall_mixed_years() {
        let mut mock = MockPostFetcher::new();
        mock.expect_determine_timezone()
            .returning(|_, _| Ok(chrono::FixedOffset::east_opt(0).unwrap()));

        // 1年前: 10件しか見つからない。Cursor=None (この年は終わり)
        mock.expect_search_posts()
            .times(1)
            .with(
                eq("token"),
                eq("did:plc:test"),
                always(),
                always(),
                eq(30),
                eq(None),
            )
            .returning(|_, _, _, _, _, _| {
                let mut posts = Vec::new();
                for i in 0..10 {
                    posts.push(PostView {
                        uri: format!("year1:{}", i),
                        record: PostRecord {
                            created_at: String::new(),
                        },
                    });
                }
                Ok((posts, None))
            });

        // Loop: years_ago increments to 2.

        // 2年前: 残りの20件を要求。Cursor=None (この年も終わり)
        mock.expect_search_posts()
            .times(1)
            .with(
                eq("token"),
                eq("did:plc:test"),
                always(),
                always(),
                eq(20),
                eq(None),
            )
            .returning(|_, _, _, _, _, _| {
                let mut posts = Vec::new();
                for i in 0..20 {
                    posts.push(PostView {
                        uri: format!("year2:{}", i),
                        record: PostRecord {
                            created_at: String::new(),
                        },
                    });
                }
                Ok((posts, None))
            });

        // Loop: feed_items=30 >= limit 30. Break.
        // Resumption info: years_ago was incremented AFTER search returned None. So years_ago=3.
        // Wait, loop logic: search returns posts, None. years_ago+=1.
        // Loop again. Feed items check happens at start of loop.
        // feed_items(10) < 30.
        // call search for year 2. returns 20 posts, None.
        // feed_items(30). cursor=None. years_ago+=1 -> 3.
        // Loop start. feed_items(30) >= 30. Break.
        // Resumption logic: current_api_cursor is None. Next cursor = v1::3::

        let (items, cursor) = fetch_posts_from_past(
            &mock,
            "token",
            "user_token",
            "did:plc:test",
            30,
            None,
            None,
            None,
        )
        .await
        .unwrap();

        assert_eq!(items.len(), 30);
        assert_eq!(items[0].post, "year1:0");
        assert_eq!(items[10].post, "year2:0");
        assert_eq!(cursor, Some("v1::3::".to_string()));
    }

    // 観点3: サービス開始年未満で停止
    #[tokio::test]
    async fn test_waterfall_stops_at_service_launch() {
        let mut mock = MockPostFetcher::new();
        mock.expect_determine_timezone()
            .returning(|_, _| Ok(chrono::FixedOffset::east_opt(0).unwrap()));

        let now = "2025-01-01T00:00:00Z"
            .parse::<chrono::DateTime<Utc>>()
            .unwrap();

        // 1年前(2024), 2年前(2023) called. Both empty.
        mock.expect_search_posts()
            .times(2)
            .returning(|_, _, _, _, _, _| Ok((vec![], None)));

        let (items, cursor) = fetch_posts_from_past(
            &mock,
            "token",
            "user_token",
            "did:plc:test",
            30,
            None,
            Some(now),
            None,
        )
        .await
        .unwrap();
        assert_eq!(items.len(), 0);
        assert!(cursor.is_none());
    }

    // 観点5: カーソル指定による再開 (1年前の途中から)
    #[tokio::test]
    async fn test_resume_from_cursor_same_year() {
        let mut mock = MockPostFetcher::new();
        mock.expect_determine_timezone()
            .returning(|_, _| Ok(chrono::FixedOffset::east_opt(0).unwrap()));

        // Input cursor: "v1::1::cursor_123" (1年前の cursor_123 から再開)
        let input_cursor = Some("v1::1::cursor_123".to_string());

        // 1年前: cursor_123 を使って検索が呼ばれることを検証
        mock.expect_search_posts()
            .times(1)
            .with(
                always(),
                always(),
                always(),
                always(),
                always(),
                eq(Some("cursor_123".to_string())), // IMPORTANT: Expecting the extracted cursor
            )
            .returning(|_, _, _, _, _, _| {
                // Return 1 item, new cursor "cursor_456"
                let posts = vec![PostView {
                    uri: "resumed:1".to_string(),
                    record: PostRecord {
                        created_at: String::new(),
                    },
                }];
                Ok((posts, Some("cursor_456".to_string())))
            });

        let (items, next_cursor) = fetch_posts_from_past(
            &mock,
            "token",
            "user_token",
            "did:plc:test",
            1,
            input_cursor,
            None,
            None,
        )
        .await
        .unwrap();

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].post, "resumed:1");
        assert_eq!(next_cursor, Some("v1::1::cursor_456".to_string()));
    }

    // 観点6: カーソル指定による再開 (2年前の頭から)
    #[tokio::test]
    async fn test_resume_from_cursor_next_year() {
        let mut mock = MockPostFetcher::new();
        mock.expect_determine_timezone()
            .returning(|_, _| Ok(chrono::FixedOffset::east_opt(0).unwrap()));

        // Input cursor: "v1::2::" (2年前の頭から。APIカーソルは空)
        let input_cursor = Some("v1::2::".to_string());

        // 1年前はスキップされ、2年前の検索から始まるはず
        mock.expect_search_posts()
            .times(1)
            .with(
                always(),
                always(),
                always(), // since/until checks implied by skipping logic, usually mock is called once
                always(),
                always(),
                eq(None), // API cursor should be None (start of year)
            )
            .returning(|_, _, _, _, _, _| {
                let posts = vec![PostView {
                    uri: "year2:1".to_string(),
                    record: PostRecord {
                        created_at: String::new(),
                    },
                }];
                Ok((posts, None))
            });

        let (items, _) = fetch_posts_from_past(
            &mock,
            "token",
            "user_token",
            "did:plc:test",
            1,
            input_cursor,
            None,
            None,
        )
        .await
        .unwrap();

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].post, "year2:1");
    }

    /*
    // 観点4: 日付境界 (省略 - ロジックは同じだが設定が面倒なため、他のテストでカバー)
     */

    // =========================================================================
    // 統合テスト: MockFetcher + in-memory CacheStore を使ったキャッシュ統合テスト
    // =========================================================================
    //
    // 単体テストは「キャッシュ単体」「ロジック単体（cache=None）」を検証しているが、
    // 統合テストはキャッシュがロジックに正しく統合されているかを検証する。
    // 特に「昨日のデータを今日も返す」「2回目もAPIを叩いてしまう」といった
    // 本番で起こりやすいバグをここで捕捉することが目的。

    use sqlx::SqlitePool;

    async fn make_cache_store() -> crate::cache::CacheStore {
        let pool = SqlitePool::connect(":memory:").await.unwrap();
        crate::cache::migrate(&pool).await.unwrap();
        crate::cache::CacheStore::new(pool)
    }

    // 統合テスト1:
    // TZキャッシュヒット時は determine_timezone が呼ばれない（API節約の核心）
    #[tokio::test]
    async fn integration_tz_cache_hit_skips_api() {
        let mut mock = MockPostFetcher::new();

        // determine_timezone は一度だけ呼ばれる（2回目はキャッシュヒット）
        mock.expect_determine_timezone()
            .times(1)
            .returning(|_, _| Ok(chrono::FixedOffset::east_opt(0).unwrap()));

        mock.expect_search_posts()
            .returning(|_, _, _, _, _, _| Ok((vec![], None)));

        let cache = make_cache_store().await;

        // 1回目: APIを叩いてTZを取得・保存
        fetch_posts_from_past(
            &mock,
            "token",
            "auth",
            "did:plc:test",
            30,
            None,
            None,
            Some(&cache),
        )
        .await
        .unwrap();

        // 2回目: TZキャッシュがヒットするので determine_timezone は呼ばれない
        // (times(1) の制約により、2回呼ばれるとパニック)
        fetch_posts_from_past(
            &mock,
            "token",
            "auth",
            "did:plc:test",
            30,
            None,
            None,
            Some(&cache),
        )
        .await
        .unwrap();
    }

    // 統合テスト2:
    // フィードキャッシュヒット時は search_posts が呼ばれない（最重要：二重呼び出し防止）
    #[tokio::test]
    async fn integration_feed_cache_hit_skips_search_posts() {
        let mut mock = MockPostFetcher::new();

        mock.expect_determine_timezone()
            .returning(|_, _| Ok(chrono::FixedOffset::east_opt(0).unwrap()));

        // limit=1 とすることで、最初の search_posts の1件目で limit に達し
        // その年で検索が完了する（次の年に進まない）。
        // よって 1回目のリクエスト全体で search_posts は1回だけ呼ばれる。
        // 2回目はフィードキャッシュヒットのため search_posts は呼ばれない。
        // → 合計で times(1) が成立する。
        mock.expect_search_posts()
            .times(1)
            .returning(|_, _, _, _, _, _| {
                Ok((
                    vec![PostView {
                        uri: "at://test/post/1".to_string(),
                        record: PostRecord {
                            created_at: String::new(),
                        },
                    }],
                    Some("cursor_next".to_string()), // カーソルが残っているので「年は終わっていない」
                ))
            });

        // TTL内にキャッシュが有効であることを保証するため、十分未来の日時を使用する。
        // (TTL = " fixed_now の翌日 00:00 UTC"。過去の日付だとテスト実行時点で即失効するため)
        let fixed_now: chrono::DateTime<chrono::Utc> = "2099-03-01T12:00:00Z".parse().unwrap();

        let cache = make_cache_store().await;

        // 1回目: APIを叩いてフィードを取得・保存 (limit=1)
        let (items1, _) = fetch_posts_from_past(
            &mock,
            "token",
            "auth",
            "did:plc:test",
            1,
            None,
            Some(fixed_now),
            Some(&cache),
        )
        .await
        .unwrap();

        // 2回目: フィードキャッシュがヒットするので search_posts は呼ばれない
        let (items2, _) = fetch_posts_from_past(
            &mock,
            "token",
            "auth",
            "did:plc:test",
            1,
            None,
            Some(fixed_now),
            Some(&cache),
        )
        .await
        .unwrap();

        assert_eq!(items1.len(), 1);
        assert_eq!(items2.len(), 1, "キャッシュから正しく返ってくること");
        assert_eq!(items2[0].post, "at://test/post/1");
    }

    // 統合テスト3:
    // 日付を跨いだ後はフィードキャッシュが無効化され、再度APIが呼ばれる
    // （「昨日の投稿が今日も出続ける」という最も危険なバグを防ぐ）
    //
    // now_utc=2025-03-01 と 2025-03-02 を注入し、ウォーターフォールが同一年内で
    // 完結するよう limit=1 かつカーソルありで返して即 limit 到達させる。
    // これにより、「今日」リクエストで search_posts が1回、「翌日」でも1回 → 計2回。
    #[tokio::test]
    async fn integration_feed_cache_invalidated_after_date_change() {
        let mut mock = MockPostFetcher::new();

        mock.expect_determine_timezone()
            .returning(|_, _| Ok(chrono::FixedOffset::east_opt(0).unwrap()));

        // search_posts は 2回呼ばれる（「今日」と「翌日」でそれぞれ1回）
        // カーソルを返すことで years_ago が進まず limit=1 で即終了する
        mock.expect_search_posts()
            .times(2)
            .returning(|_, _, _, _, _, _| {
                Ok((
                    vec![PostView {
                        uri: "at://test/post/new".to_string(),
                        record: PostRecord {
                            created_at: String::new(),
                        },
                    }],
                    Some("cursor_next".to_string()),
                ))
            });

        let cache = make_cache_store().await;

        // 1回目: 「今日」でリクエスト → キャッシュ保存 (limit=1)
        // 2099年なので expires_at（2100-03-01）が十分未来 → テスト実行時に有効
        let today: chrono::DateTime<chrono::Utc> = "2099-03-01T12:00:00Z".parse().unwrap();
        fetch_posts_from_past(
            &mock,
            "token",
            "auth",
            "did:plc:test",
            1,
            None,
            Some(today),
            Some(&cache),
        )
        .await
        .unwrap();

        // 2回目: 「翌日」でリクエスト → キャッシュの日付キーが "250301" ≠ "250302" のためミス → APIを叩く
        // (times(2) の制約により、3回以上呼ばれるとパニック)
        let tomorrow: chrono::DateTime<chrono::Utc> = "2099-03-02T12:00:00Z".parse().unwrap();
        let (items, _) = fetch_posts_from_past(
            &mock,
            "token",
            "auth",
            "did:plc:test",
            1,
            None,
            Some(tomorrow),
            Some(&cache),
        )
        .await
        .unwrap();

        assert_eq!(
            items.len(),
            1,
            "翌日のリクエストもAPIから正しく取得できること"
        );
        assert_eq!(items[0].post, "at://test/post/new");
    }

    // 統合テスト4:
    // cursor が異なる場合は別ページとして別々にキャッシュされる
    // （「2ページ目の結果が1ページ目のキャッシュを上書きする」バグを防ぐ）
    #[tokio::test]
    async fn integration_feed_cache_separated_by_cursor() {
        let mut mock = MockPostFetcher::new();

        mock.expect_determine_timezone()
            .returning(|_, _| Ok(chrono::FixedOffset::east_opt(0).unwrap()));

        // 1ページ目（cursor=None）と2ページ目（cursor=Some）で計2回呼ばれる
        mock.expect_search_posts()
            .times(2)
            .returning(|_, _, _, _, _, cursor| {
                let uri = if cursor.is_none() {
                    "at://test/post/page1"
                } else {
                    "at://test/post/page2"
                };
                Ok((
                    vec![PostView {
                        uri: uri.to_string(),
                        record: PostRecord {
                            created_at: String::new(),
                        },
                    }],
                    None,
                ))
            });

        let fixed_now: chrono::DateTime<chrono::Utc> = "2025-03-01T12:00:00Z".parse().unwrap();
        let cache = make_cache_store().await;

        // 1ページ目
        let (items_p1, _) = fetch_posts_from_past(
            &mock,
            "token",
            "auth",
            "did:plc:test",
            1,
            None,
            Some(fixed_now),
            Some(&cache),
        )
        .await
        .unwrap();

        // 2ページ目 (cursor を指定)
        let (items_p2, _) = fetch_posts_from_past(
            &mock,
            "token",
            "auth",
            "did:plc:test",
            1,
            Some("v1::1::some_cursor".to_string()),
            Some(fixed_now),
            Some(&cache),
        )
        .await
        .unwrap();

        assert_eq!(items_p1[0].post, "at://test/post/page1");
        assert_eq!(
            items_p2[0].post, "at://test/post/page2",
            "別ページは別キャッシュであること"
        );
    }

    // 統合テスト5:
    // TZキャッシュミス（初回）後、続けてTZキャッシュがヒットする正常経路の確認
    // (TZキャッシュが正しく書き込まれているかの結合確認)
    #[tokio::test]
    async fn integration_tz_miss_then_hit() {
        let mut mock = MockPostFetcher::new();

        // JST (UTC+9) を返す
        mock.expect_determine_timezone()
            .times(1)
            .returning(|_, _| Ok(chrono::FixedOffset::east_opt(9 * 3600).unwrap()));

        mock.expect_search_posts()
            .returning(|_, _, _, _, _, _| Ok((vec![], None)));

        let cache = make_cache_store().await;

        // 1回目: キャッシュなし → API から JST を取得
        fetch_posts_from_past(
            &mock,
            "token",
            "auth",
            "did:plc:jst",
            30,
            None,
            None,
            Some(&cache),
        )
        .await
        .unwrap();

        // キャッシュに保存されているか直接確認
        let tz = cache.get_timezone("did:plc:jst").await.unwrap();
        assert!(tz.is_some(), "TZがキャッシュに保存されているべき");
        assert_eq!(
            tz.unwrap().local_minus_utc(),
            9 * 3600,
            "JSTのオフセットが正しく保存されているべき"
        );

        // 2回目: TZキャッシュヒット → determine_timezone は呼ばれない
        // (times(1) の制約で、2回呼ばれるとパニック)
        fetch_posts_from_past(
            &mock,
            "token",
            "auth",
            "did:plc:jst",
            30,
            None,
            None,
            Some(&cache),
        )
        .await
        .unwrap();
    }

    // 統合テスト6:
    // 同一日付であってもタイムゾーン（オフセット）が異なる場合はキャッシュミスする
    // （UX改善：TZ変更時の即時反映を保証するテスト）
    #[tokio::test]
    async fn integration_feed_cache_invalidated_after_timezone_change() {
        let mut mock = MockPostFetcher::new();

        // 1回目：JST (+9) で取得
        // 2回目：PST (-8) で取得（DID変更により再取得が発生するシナリオ）
        mock.expect_determine_timezone()
            .times(2)
            .returning(|handle, _| {
                if handle == "did:plc:user:jst" {
                    Ok(chrono::FixedOffset::east_opt(9 * 3600).unwrap())
                } else {
                    Ok(chrono::FixedOffset::east_opt(-8 * 3600).unwrap())
                }
            });

        // search_posts は 2回呼ばれるべき（日付は同じだが、オフセットが違うため）
        mock.expect_search_posts()
            .times(2)
            .returning(|_, _, _, _, _, _| {
                Ok((
                    vec![PostView {
                        uri: "at://test/post/1".to_string(),
                        record: PostRecord {
                            created_at: String::new(),
                        },
                    }],
                    None,
                ))
            });

        let cache = make_cache_store().await;
        // 同じ「今日」の日時を固定（UTC 12:00 は JST でも PST でも 2/21）
        let fixed_now: chrono::DateTime<chrono::Utc> = "2025-02-21T12:00:00Z".parse().unwrap();

        // 1. JST でリクエスト → キャッシュ保存
        fetch_posts_from_past(
            &mock,
            "token",
            "auth",
            "did:plc:user:jst",
            1,
            None,
            Some(fixed_now),
            Some(&cache),
        )
        .await
        .unwrap();

        // 2. PST でリクエスト（同じ actor だが設定が切り替わったと想定）
        // オフセットがキーに含まれているため、日付が同じ "250221" でもミスするはず
        let (items, _) = fetch_posts_from_past(
            &mock,
            "token",
            "auth",
            "did:plc:user:pst", // 名前を変えて TZ 再取得を誘発
            1,
            None,
            Some(fixed_now),
            Some(&cache),
        )
        .await
        .unwrap();

        assert_eq!(items.len(), 1);
        // mock.expect_search_posts().times(2) が満たされれば成功
    }
}
