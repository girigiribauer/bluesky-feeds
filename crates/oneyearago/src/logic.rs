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
    now_utc: Option<chrono::DateTime<Utc>>,
    cache: Option<&CacheStore>,
) -> Result<(Vec<FeedItem>, Option<String>)> {
    let tz_offset = if let Some(store) = cache {
        match store.get_timezone(actor).await {
            Ok(Some(cached)) => {
                tracing::debug!("[cache] TZ hit for {}", actor);
                cached
            }
            _ => {
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

    let now_utc = now_utc.unwrap_or_else(Utc::now);
    let now_tz = now_utc.with_timezone(&tz_offset);

    let safe_limit = if limit == 0 { DEFAULT_LIMIT } else { limit };

    let today_naive = now_tz.date_naive();
    let date_key = format!(
        "{}:{}",
        today_naive.format("%y%m%d"),
        tz_offset.local_minus_utc()
    );

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
            break None;
        }

        let target_date = chrono::NaiveDate::from_ymd_opt(target_year, today.month(), today.day())
            .unwrap_or_else(|| chrono::NaiveDate::from_ymd_opt(target_year, 2, 28).unwrap());

        let start_local = target_date
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_local_timezone(tz_offset)
            .unwrap();

        let end_local = (target_date + chrono::Duration::days(1))
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_local_timezone(tz_offset)
            .unwrap();

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

                if current_api_cursor.is_none() {
                    years_ago += 1;
                }
            }
            Err(e) => {
                tracing::error!("Failed to fetch posts for {} years ago: {}", years_ago, e);
                years_ago += 1;
                current_api_cursor = None;
            }
        }
    };

    if let Some(store) = cache {
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

    // 十分な件数がある場合 (1年前のみで完結)
    #[tokio::test]
    async fn test_waterfall_single_year_sufficient() {
        let mut mock = MockPostFetcher::new();
        mock.expect_determine_timezone()
            .returning(|_, _| Ok(chrono::FixedOffset::east_opt(0).unwrap()));

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

    // 件数が不足する場合 (1年前 -> 2年前へと検索が続く)
    #[tokio::test]
    async fn test_waterfall_mixed_years() {
        let mut mock = MockPostFetcher::new();
        mock.expect_determine_timezone()
            .returning(|_, _| Ok(chrono::FixedOffset::east_opt(0).unwrap()));

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

    // サービス開始年未満で停止
    #[tokio::test]
    async fn test_waterfall_stops_at_service_launch() {
        let mut mock = MockPostFetcher::new();
        mock.expect_determine_timezone()
            .returning(|_, _| Ok(chrono::FixedOffset::east_opt(0).unwrap()));

        let now = "2025-01-01T00:00:00Z"
            .parse::<chrono::DateTime<Utc>>()
            .unwrap();

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

    // カーソル指定による再開 (1年前の途中から)
    #[tokio::test]
    async fn test_resume_from_cursor_same_year() {
        let mut mock = MockPostFetcher::new();
        mock.expect_determine_timezone()
            .returning(|_, _| Ok(chrono::FixedOffset::east_opt(0).unwrap()));

        let input_cursor = Some("v1::1::cursor_123".to_string());

        mock.expect_search_posts()
            .times(1)
            .with(
                always(),
                always(),
                always(),
                always(),
                always(),
                eq(Some("cursor_123".to_string())),
            )
            .returning(|_, _, _, _, _, _| {
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

    // カーソル指定による再開 (2年前の頭から)
    #[tokio::test]
    async fn test_resume_from_cursor_next_year() {
        let mut mock = MockPostFetcher::new();
        mock.expect_determine_timezone()
            .returning(|_, _| Ok(chrono::FixedOffset::east_opt(0).unwrap()));

        let input_cursor = Some("v1::2::".to_string());

        mock.expect_search_posts()
            .times(1)
            .with(always(), always(), always(), always(), always(), eq(None))
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
     */

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

        mock.expect_determine_timezone()
            .times(1)
            .returning(|_, _| Ok(chrono::FixedOffset::east_opt(0).unwrap()));

        mock.expect_search_posts()
            .returning(|_, _, _, _, _, _| Ok((vec![], None)));

        let cache = make_cache_store().await;

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
                    Some("cursor_next".to_string()),
                ))
            });

        let fixed_now: chrono::DateTime<chrono::Utc> = "2099-03-01T12:00:00Z".parse().unwrap();

        let cache = make_cache_store().await;

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

        let fixed_now: chrono::DateTime<chrono::Utc> = "2099-03-01T12:00:00Z".parse().unwrap();
        let cache = make_cache_store().await;

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

        mock.expect_determine_timezone()
            .times(1)
            .returning(|_, _| Ok(chrono::FixedOffset::east_opt(9 * 3600).unwrap()));

        mock.expect_search_posts()
            .returning(|_, _, _, _, _, _| Ok((vec![], None)));

        let cache = make_cache_store().await;

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

        let tz = cache.get_timezone("did:plc:jst").await.unwrap();
        assert!(tz.is_some(), "TZがキャッシュに保存されているべき");
        assert_eq!(
            tz.unwrap().local_minus_utc(),
            9 * 3600,
            "JSTのオフセットが正しく保存されているべき"
        );

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
    // 同一日付・同一利用者であっても、タイムゾーン（オフセット）が変われば
    // フィードキャッシュはミスする（UX改善：TZ変更時の即時反映を保証する）
    #[tokio::test]
    async fn integration_feed_cache_invalidated_after_timezone_change() {
        let mut mock = MockPostFetcher::new();

        mock.expect_determine_timezone()
            .times(1)
            .returning(|_, _| Ok(chrono::FixedOffset::east_opt(9 * 3600).unwrap()));

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
        let fixed_now: chrono::DateTime<chrono::Utc> = "2099-02-21T12:00:00Z".parse().unwrap();
        let actor = "did:plc:user";

        fetch_posts_from_past(
            &mock,
            "token",
            "auth",
            actor,
            1,
            None,
            Some(fixed_now),
            Some(&cache),
        )
        .await
        .unwrap();

        cache.set_timezone(actor, -8 * 3600).await.unwrap();

        let (items, _) = fetch_posts_from_past(
            &mock,
            "token",
            "auth",
            actor,
            1,
            None,
            Some(fixed_now),
            Some(&cache),
        )
        .await
        .unwrap();

        assert_eq!(items.len(), 1);
    }
}
