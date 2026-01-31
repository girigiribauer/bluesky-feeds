#[allow(unused_imports)]
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
///
/// 優先順位:
/// 1. 明示的な指定 (UTC+9, Asia/Tokyo)
/// 2. 日本語文字検出 (ひらがな/カタカナ) -> JST
/// 3. Default -> None (Caller should default to UTC)
fn parse_timezone_description(description: &str) -> Option<FixedOffset> {
    // 1. Offsets: UTC+9, GMT-05:00
    // Matches "UTC+9", "UTC-05:00", "GMT+9"
    let re_offset = Regex::new(r"(?i)(?:UTC|GMT)([\+\-]\d{1,2}(?::\d{2})?)").unwrap();
    if let Some(caps) = re_offset.captures(description) {
        if let Some(offset_str) = caps.get(1) {
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

    // 3. Japanese Content Detection (Hiragana/Katakana)
    // ひらがな: \u{3040}-\u{309F}
    // カタカナ: \u{30A0}-\u{30FF}
    let re_kana = Regex::new(r"[\u{3040}-\u{309F}\u{30A0}-\u{30FF}]").unwrap();
    if re_kana.is_match(description) {
        return Some(FixedOffset::east_opt(9 * 3600).unwrap());
    }

    // Default UTC
    None
}

/// タイムゾーンを決定する
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
        if res.status() == reqwest::StatusCode::UNAUTHORIZED {
            return Err(anyhow::anyhow!("Unauthorized"));
        }
        // プロフィール取得失敗時はデフォルトUTC
        return Ok(FixedOffset::east_opt(0).unwrap());
    }

    let profile: ProfileResponse = res.json().await.context("Failed to parse profile")?;
    let description = profile.description.unwrap_or_default();

    Ok(parse_timezone_description(&description).unwrap_or(FixedOffset::east_opt(0).unwrap()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_timezone_logic() {
        // 1. Explicit Offsets
        assert_eq!(parse_timezone_description("UTC+9").map(|o| o.local_minus_utc()), Some(9 * 3600));
        assert_eq!(parse_timezone_description("GMT-5").map(|o| o.local_minus_utc()), Some(-5 * 3600));
        assert_eq!(parse_timezone_description("Living in UTC+09:00 region").map(|o| o.local_minus_utc()), Some(9 * 3600));

        // 2. Explicit Keyword
        assert_eq!(parse_timezone_description("Asia/Tokyo time").map(|o| o.local_minus_utc()), Some(9 * 3600));

        // 3. Japanese Content (Hiragana/Katakana) -> JST
        assert_eq!(parse_timezone_description("こんにちは").map(|o| o.local_minus_utc()), Some(9 * 3600));
        assert_eq!(parse_timezone_description("エンジニアです").map(|o| o.local_minus_utc()), Some(9 * 3600));
        assert_eq!(parse_timezone_description("Profile (JP)").map(|o| o.local_minus_utc()), None); // Kanji/Kana absent

        // 4. Override (Japanese text but explicit offset) -> Explicit wins
        // Note: Regex order matters. We check offset first.
        assert_eq!(parse_timezone_description("NY在住 (UTC-5) です").map(|o| o.local_minus_utc()), Some(-5 * 3600));

        // 5. Default (No match)
        assert_eq!(parse_timezone_description("Hello World").map(|o| o.local_minus_utc()), None);
        assert_eq!(parse_timezone_description("Tokyo, Japan").map(|o| o.local_minus_utc()), None); // Location intentionally ignored
        assert_eq!(parse_timezone_description("JST").map(|o| o.local_minus_utc()), None); // Abbr excluded
    }
}
