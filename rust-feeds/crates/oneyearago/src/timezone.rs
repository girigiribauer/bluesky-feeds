use anyhow::{Context, Result};
use chrono::FixedOffset;
use regex::Regex;
use reqwest::Client;
use serde::Deserialize;

#[derive(Deserialize)]
struct ProfileResponse {
    #[serde(default)]
    description: Option<String>,
}

/// Bioのテキストからタイムゾーンをパースする（純粋関数）
fn parse_timezone_description(description: &str) -> Option<FixedOffset> {
    // 1. Offsets: UTC+9 (GMT removed)
    // Matches "UTC+9", "UTC-05:00", "utc+9"
    let re_offset = Regex::new(r"(?i)UTC([\+\-]\d{1,2}(?::\d{2})?)").unwrap();
    if let Some(caps) = re_offset.captures(description) {
        if let Some(offset_str) = caps.get(1) {
            // パース: +9, -05:00
            // chronoのFixedOffsetは秒数指定
            let s = offset_str.as_str();
            let (sign, s) = s.split_at(1);
            let sign = if sign == "-" { -1 } else { 1 };

            let parts: Vec<&str> = s.split(':').collect();
            let hours: i32 = parts[0].parse().unwrap_or(0);
            let minutes: i32 = if parts.len() > 1 { parts[1].parse().unwrap_or(0) } else { 0 };

            let offset_secs = sign * (hours * 3600 + minutes * 60);
             if let Some(offset) = FixedOffset::east_opt(offset_secs) {
                return Some(offset);
            }
        }
    }

    // 2. Keywords (Asia/Tokyo only)
    let re_asia = Regex::new(r"(?i)Asia\/Tokyo").unwrap();
    if re_asia.is_match(description) {
        return Some(FixedOffset::east_opt(9 * 3600).unwrap());
    }

    // Removed JST, 日本時間, EST, PST, GMT

    None
}

/// タイムゾーンを決定する
/// 1. Bioから正規表現で抽出
/// 2. 言語設定(ja)から推定 (TODO: APIレスポンスにlangが含まれていないため、現状はBioのみ/Default UTC)
pub async fn determine_timezone(client: &Client, handle: &str, token: &str) -> Result<FixedOffset> {
    let url = "https://api.bsky.app/xrpc/app.bsky.actor.getProfile";
    let res = client
        .get(url)
        .header("Authorization", format!("Bearer {}", token))
        .query(&[("actor", handle)])
        .send()
        .await
        .context("Failed to get profile")?;

    if !res.status().is_success() {
        // プロフィール取得失敗時はUTCとする（非公開アカウントなど）
        return Ok(FixedOffset::east_opt(0).unwrap());
    }

    let profile: ProfileResponse = res.json().await.context("Failed to parse profile")?;
    let description = profile.description.unwrap_or_default();

    // 抽出ロジックに移譲
    Ok(parse_timezone_description(&description).unwrap_or(FixedOffset::east_opt(0).unwrap()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_timezone_offsets() {
        // Standard formats
        let inputs = vec![
            ("Living in UTC+9", Some(9 * 3600)),
            ("UTC+09:00", Some(9 * 3600)),
            ("UTC-05:00", Some(-5 * 3600)),
            ("UTC+5:30", Some(5 * 3600 + 30 * 60)),
            ("utc+9", Some(9 * 3600)),
            // Removed support
            ("Timezone: GMT-5", None),
            ("GMT-03:45", None),
            ("gmt-5", None),
        ];

        for (desc, expected) in inputs {
            let offset_opt = parse_timezone_description(desc).map(|o| o.local_minus_utc());
            assert_eq!(offset_opt, expected, "Failed for {}", desc);
        }
    }

    #[test]
    fn test_timezone_keywords() {
        // Explicit Keywords
        assert_eq!(parse_timezone_description("Asia/Tokyo"), Some(FixedOffset::east_opt(9 * 3600).unwrap()));

        // Removed
        assert_eq!(parse_timezone_description("I am in JST"), None);
        assert_eq!(parse_timezone_description("日本時間です"), None);
        assert_eq!(parse_timezone_description("EST time"), None);
        assert_eq!(parse_timezone_description("PST"), None);

        // No match
        assert_eq!(parse_timezone_description("Hello World"), None);
    }
}
