use crate::api::PostFetcher;
use anyhow::Result;
use chrono::Utc;
use models::FeedItem;

const MIN_SEARCH_YEAR: i32 = 2023;
const DEFAULT_LIMIT: usize = 30;

pub async fn fetch_posts_from_past<F: PostFetcher>(
    fetcher: &F,
    service_token: &str,
    actor: &str,
    now_utc: Option<chrono::DateTime<Utc>>, // Injectable "now"
) -> Result<Vec<FeedItem>> {
    // 1. Timezone
    let tz_offset = fetcher.determine_timezone(actor, service_token).await.unwrap_or(chrono::FixedOffset::east_opt(0).unwrap());

    // 現在時刻 (UTC) -> ターゲットタイムゾーンへ変換
    let now_utc = now_utc.unwrap_or_else(Utc::now);
    let now_tz = now_utc.with_timezone(&tz_offset);

    let mut feed_items = Vec::new();
    let limit = DEFAULT_LIMIT;

    // 2. Waterfall Searching
    // Start from 1 year ago, keep going back until we hit the service launch year
    for years_ago in 1.. {
        if feed_items.len() >= limit {
            break;
        }

        use chrono::Datelike;
        let today = now_tz.date_naive();
        let target_year = today.year() - years_ago;

        if target_year < MIN_SEARCH_YEAR {
            break;
        }

        // Handle leap years (Feb 29 -> Feb 28 on non-leap years)
        let target_date = chrono::NaiveDate::from_ymd_opt(target_year, today.month(), today.day())
            .unwrap_or_else(|| chrono::NaiveDate::from_ymd_opt(target_year, 2, 28).unwrap());

        // Start: 00:00:00 user time
        let start_local = target_date.and_hms_opt(0, 0, 0).unwrap()
            .and_local_timezone(tz_offset)
            .unwrap(); // FixedOffset mapping is always single

        // End: Next day 00:00:00 user time (exclusive)
        let end_local = (target_date + chrono::Duration::days(1))
            .and_hms_opt(0, 0, 0).unwrap()
            .and_local_timezone(tz_offset)
            .unwrap();

        // Convert to UTC ISO Strings
        let since = start_local.with_timezone(&Utc).to_rfc3339();
        let until = end_local.with_timezone(&Utc).to_rfc3339();

        let fetch_limit = limit - feed_items.len();
        match fetcher.search_posts(service_token, actor, &since, &until, fetch_limit).await {
            Ok(posts) => {
                for p in posts {
                    feed_items.push(FeedItem { post: p.uri });
                }
            }
            Err(e) => {
                tracing::error!("Failed to fetch posts for {} years ago: {}", years_ago, e);
            }
        }
    }

    Ok(feed_items)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{PostView, PostRecord};
    use mockall::predicate::*;
    use mockall::mock;

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
            ) -> Result<Vec<PostView>>;

            async fn determine_timezone(&self, handle: &str, token: &str) -> Result<chrono::FixedOffset>;
        }
    }

    // 観点1: 十分な件数がある場合 (1年前のみで完結)
    #[tokio::test]
    async fn test_waterfall_single_year_sufficient() {
        let mut mock = MockPostFetcher::new();
        mock.expect_determine_timezone().returning(|_, _| Ok(chrono::FixedOffset::east_opt(0).unwrap()));

        // 1年前: 30件要求に対し、30件フルで返ってくるケース
        mock.expect_search_posts()
            .times(1)
            .with(eq("token"), eq("did:plc:test"), always(), always(), eq(30))
            .returning(|_, _, _, _, _| {
                 let mut posts = Vec::new();
                for i in 0..30 {
                    posts.push(PostView { uri: format!("id:{}", i), record: PostRecord { created_at: String::new() }});
                }
                Ok(posts)
            });

        // 2年前へのリクエストは発生しない

        let items = fetch_posts_from_past(&mock, "token", "did:plc:test", None).await.unwrap();
        assert_eq!(items.len(), 30);
    }

    // 観点2: 件数が不足する場合 (1年前 -> 2年前へと検索が続く)
    #[tokio::test]
    async fn test_waterfall_mixed_years() {
        let mut mock = MockPostFetcher::new();
        mock.expect_determine_timezone()
            .returning(|_, _| Ok(chrono::FixedOffset::east_opt(0).unwrap()));

        // 1年前: 10件しか見つからない
        mock.expect_search_posts()
            .times(1)
            .with(eq("token"), eq("did:plc:test"), always(), always(), eq(30)) // 最初は30件要求
            .returning(|_, _, _, _, _| {
                let mut posts = Vec::new();
                for i in 0..10 {
                    posts.push(PostView { uri: format!("year1:{}", i), record: PostRecord { created_at: String::new() }});
                }
                Ok(posts)
            });

        // 2年前: 残りの20件を要求
        mock.expect_search_posts()
            .times(1)
            .with(eq("token"), eq("did:plc:test"), always(), always(), eq(20)) // 残り20件
            .returning(|_, _, _, _, _| {
                let mut posts = Vec::new();
                for i in 0..20 {
                     posts.push(PostView { uri: format!("year2:{}", i), record: PostRecord { created_at: String::new() }});
                }
                Ok(posts)
            });

        let items = fetch_posts_from_past(&mock, "token", "did:plc:test", None).await.unwrap();

        assert_eq!(items.len(), 30);
        assert_eq!(items[0].post, "year1:0"); // 先頭は1年前
        assert_eq!(items[10].post, "year2:0"); // 11件目は2年前
    }

    // 観点3: サービス開始年(2023)未満になったら停止する
    #[tokio::test]
    async fn test_waterfall_stops_at_service_launch() {
        let mut mock = MockPostFetcher::new();
        mock.expect_determine_timezone().returning(|_, _| Ok(chrono::FixedOffset::east_opt(0).unwrap()));

        // 現在 = 2025年1月1日
        let now = "2025-01-01T00:00:00Z".parse::<chrono::DateTime<Utc>>().unwrap();

        // 1年前 = 2024 (OK)
        // 2年前 = 2023 (OK - MIN_SEARCH_YEAR)
        // 3年前 = 2022 (NG - break)

        // 2回呼ばれるはず
        mock.expect_search_posts()
            .times(2)
            .returning(|_, _, _, _, _| Ok(vec![])); // 常に空を返す

        let items = fetch_posts_from_past(&mock, "token", "did:plc:test", Some(now)).await.unwrap();
        assert_eq!(items.len(), 0);
    }

    // 観点4: 日付境界とタイムゾーン計算 (JSTのケース)
    #[tokio::test]
    async fn test_date_boundary_timezone_conversion() {
        let mut mock = MockPostFetcher::new();

        // ユーザーは JST (+09:00)
        mock.expect_determine_timezone()
            .returning(|_, _| Ok(chrono::FixedOffset::east_opt(9 * 3600).unwrap()));

        // 現在時刻: 2026-02-01 00:00:00 JST (UTC: 1/31 15:00)
        let now_utc = "2026-01-31T15:00:00Z".parse::<chrono::DateTime<Utc>>().unwrap();

        // 期待される「1年前」の指定:
        // JSTでの「去年の今日」: 2025-02-01
        // 範囲 (JST): 2025-02-01 00:00:00 〜 2025-02-02 00:00:00
        // 範囲 (UTC): 2025-01-31 15:00:00 〜 2025-02-01 15:00:00
        mock.expect_search_posts()
            .times(1)
            .with(
                always(),
                always(),
                eq("2025-01-31T15:00:00+00:00"), // since (Strict check)
                eq("2025-02-01T15:00:00+00:00"), // until (Strict check)
                always()
            )
            .returning(|_, _, _, _, _| {
                // Return 30 items to STOP the loop so it doesn't go to year 2
                 let mut posts = Vec::new();
                for i in 0..30 {
                    posts.push(PostView { uri: format!("id:{}", i), record: PostRecord { created_at: String::new() }});
                }
                Ok(posts)
            });

        let _ = fetch_posts_from_past(&mock, "token", "did:plc:test", Some(now_utc)).await.unwrap();
    }
}
