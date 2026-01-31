#[allow(unused_imports)]
use anyhow::{Context, Result};
use chrono::FixedOffset;
use regex::Regex;
use reqwest::Client;
use serde::Deserialize;

#[derive(Deserialize)]
#[allow(dead_code)]
struct ProfileResponse {
    #[serde(default)]
    description: Option<String>,
}

/// Bioのテキストからタイムゾーンをパースする（純粋関数）
#[allow(dead_code)]
fn parse_timezone_description(description: &str, lang: Option<&str>) -> Option<FixedOffset> {
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

    // 3. Language setting (from Preferences)
    if let Some(l) = lang {
        if l == "ja" {
            return Some(FixedOffset::east_opt(9 * 3600).unwrap());
        }
    }

    // Default UTC
    None
}

/// タイムゾーンを決定する
/// 現在は固定で JST (UTC+09:00) を返す
#[allow(unused_variables)]
pub async fn determine_timezone(client: &Client, handle: &str, token: &str, lang: Option<String>) -> Result<FixedOffset> {
    // 暫定対応: 全ユーザーをJSTとして扱う
    Ok(FixedOffset::east_opt(9 * 3600).unwrap())

    /*
    // 将来的なロジック復帰用
    let url = "https://api.bsky.app/xrpc/app.bsky.actor.getProfile";
    let res = client
        .get(url)
        .header("Authorization", format!("Bearer {}", token))
        .query(&[("actor", handle)])
        .send()
        .await
        .context("Failed to get profile")?;

    if !res.status().is_success() {
        if res.status() == reqwest::StatusCode::UNAUTHORIZED {
            return Err(anyhow::anyhow!("Unauthorized"));
        }
        return Ok(FixedOffset::east_opt(0).unwrap());
    }

    let profile: ProfileResponse = res.json().await.context("Failed to parse profile")?;
    let description = profile.description.unwrap_or_default();

    Ok(parse_timezone_description(&description, lang.as_deref()).unwrap_or(FixedOffset::east_opt(0).unwrap()))
    */
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_timezone_offsets() {
        // Standard formats
        let inputs = vec![
            ("Living in UTC+9", Some(9 * 3600), None),
            ("UTC+09:00", Some(9 * 3600), None),
            ("UTC-05:00", Some(-5 * 3600), None),
            ("UTC+5:30", Some(5 * 3600 + 30 * 60), None),
            ("utc+9", Some(9 * 3600), None),
            // Real world example with emojis
            ("🌍 UTC+09:00 🌏", Some(9 * 3600), None),
            // Real world example with emojis
            ("🌍 UTC+09:00 🌏", Some(9 * 3600), None),
            // Japanese text only -> Implicit JST REMOVED -> Default UTC
            ("こんにちは、日本でエンジニアをしています。", None, None),
            ("移動は善", None, None), // Based on user profile logic removed
            // Priority: Explicit Timezone > Japanese text
            ("I live in NY (UTC-5). 日本語も話せます。", Some(-5 * 3600), None),
            // Language setting "ja"
            ("Hello", Some(9 * 3600), Some("ja")),
            // Language setting "en" (no effect, UTC)
            ("Hello", None, Some("en")),
            // No Japanese -> Default UTC (None)
            ("Hello world", None, None),
        ];

        for (desc, expected, lang) in inputs {
            let offset_opt = parse_timezone_description(desc, lang).map(|o| o.local_minus_utc());
            assert_eq!(offset_opt, expected, "Failed for {} with lang {:?}", desc, lang);
        }
    }

    #[test]
    fn test_timezone_keywords() {
        // Explicit Keywords
        assert_eq!(parse_timezone_description("Asia/Tokyo", None), Some(FixedOffset::east_opt(9 * 3600).unwrap()));

        // Keywords removed, inference removed -> None
        assert_eq!(parse_timezone_description("日本時間です", None), None);

        // "JST" alone has no Japanese chars, -> "ja" setting should make it JST
        assert_eq!(parse_timezone_description("I am in JST", Some("ja")), Some(FixedOffset::east_opt(9 * 3600).unwrap()));

        // "JST" alone, no lang -> None
        assert_eq!(parse_timezone_description("I am in JST", None), None);

        // "EST" / "PST" still None
        assert_eq!(parse_timezone_description("EST time", None), None);
        assert_eq!(parse_timezone_description("PST", None), None);

        // No match
        assert_eq!(parse_timezone_description("Hello World", None), None);
    }
}
