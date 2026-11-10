use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::reason::Reason;

pub enum Outcome {
    Pass,
    Reject(Reason),
    Failed(&'static str),
}

pub fn failure_kind(error: &anyhow::Error) -> &'static str {
    match error.downcast_ref::<reqwest::Error>() {
        Some(e) if e.is_timeout() => "timeout",
        Some(e) => match e.status().map(|s| s.as_u16()) {
            Some(500..=599) => "5xx",
            Some(400..=499) => "4xx",
            _ => "network",
        },
        None => "other",
    }
}

pub struct Stats {
    interval: Duration,
    window_start: Instant,
    busy_at_start: Duration,
    passed: u64,
    rejected: HashMap<Reason, u64>,
    failed: HashMap<&'static str, u64>,
}

impl Stats {
    pub fn new(interval: Duration, now: Instant) -> Self {
        Stats {
            interval,
            window_start: now,
            busy_at_start: Duration::ZERO,
            passed: 0,
            rejected: HashMap::new(),
            failed: HashMap::new(),
        }
    }

    pub fn record(&mut self, outcome: Outcome) {
        match outcome {
            Outcome::Pass => self.passed += 1,
            Outcome::Reject(reason) => *self.rejected.entry(reason).or_default() += 1,
            Outcome::Failed(kind) => *self.failed.entry(kind).or_default() += 1,
        }
    }

    pub fn report(&mut self, now: Instant, busy_total: Duration) -> Option<String> {
        let elapsed = now.duration_since(self.window_start);
        if elapsed < self.interval {
            return None;
        }
        let rejected: u64 = self.rejected.values().sum();
        let failed: u64 = self.failed.values().sum();
        let busy = busy_total.saturating_sub(self.busy_at_start);
        let line = format!(
            "SUMMARY [{}s]: judged={} pass={} reject={} failed={} busy={:.0}% reject{{{}}} failed{{{}}}",
            elapsed.as_secs(),
            self.passed + rejected + failed,
            self.passed,
            rejected,
            failed,
            busy.as_secs_f64() / elapsed.as_secs_f64() * 100.0,
            breakdown(self.rejected.iter().map(|(r, n)| (format!("{r:?}"), *n))),
            breakdown(self.failed.iter().map(|(k, n)| (k.to_string(), *n))),
        );
        *self = Stats::new(self.interval, now);
        self.busy_at_start = busy_total;
        Some(line)
    }
}

pub fn memory_and_reset_peak() -> String {
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    let _ = std::fs::write("/proc/self/clear_refs", "5");
    memory(&status)
}

fn memory(status: &str) -> String {
    let mb = |key: &str| {
        status
            .lines()
            .find_map(|l| l.strip_prefix(key))
            .and_then(|v| v.trim().trim_end_matches("kB").trim().parse::<u64>().ok())
            .map_or("?".to_string(), |kb| format!("{}MB", kb / 1024))
    };
    format!("rss={} peak={}", mb("VmRSS:"), mb("VmHWM:"))
}

fn breakdown(items: impl Iterator<Item = (String, u64)>) -> String {
    let mut items: Vec<_> = items.collect();
    items.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    items
        .iter()
        .map(|(name, n)| format!("{name}={n}"))
        .collect::<Vec<_>>()
        .join(",")
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOUR: Duration = Duration::from_secs(3600);

    /// 決めた間隔が過ぎるまでは報告せず、数え続けること
    #[test]
    fn test_no_report_before_interval() {
        let start = Instant::now();
        let mut stats = Stats::new(HOUR, start);
        stats.record(Outcome::Pass);

        assert_eq!(stats.report(start + HOUR / 2, Duration::ZERO), None);
        assert!(stats
            .report(start + HOUR, Duration::ZERO)
            .unwrap()
            .contains("pass=1"));
    }

    /// 判定に回った件数・通った件数・弾いた件数・失敗の件数と、弾いた理由と失敗の種類ごとの内訳を多い順に出すこと
    #[test]
    fn test_report_counts_and_breakdowns() {
        let start = Instant::now();
        let mut stats = Stats::new(HOUR, start);
        stats.record(Outcome::Pass);
        stats.record(Outcome::Reject(Reason::Text));
        stats.record(Outcome::Reject(Reason::Object));
        stats.record(Outcome::Reject(Reason::Object));
        stats.record(Outcome::Failed("timeout"));

        assert_eq!(
            stats.report(start + HOUR, Duration::ZERO).unwrap(),
            "SUMMARY [3600s]: judged=5 pass=1 reject=3 failed=1 busy=0% reject{Object=2,Text=1} failed{timeout=1}"
        );
    }

    /// 判定の係が働いていた時間を、その間隔の長さに対する割合で出すこと
    #[test]
    fn test_report_busy_share_of_interval() {
        let start = Instant::now();
        let mut stats = Stats::new(HOUR, start);

        let line = stats.report(start + HOUR, HOUR / 4).unwrap();

        assert!(line.contains("busy=25%"), "{line}");
    }

    /// 報告したあとは、件数も働いた時間も0から数え直すこと
    #[test]
    fn test_report_starts_over() {
        let start = Instant::now();
        let mut stats = Stats::new(HOUR, start);
        stats.record(Outcome::Pass);
        stats.report(start + HOUR, HOUR / 4);

        let line = stats.report(start + HOUR * 2, HOUR / 2).unwrap();

        assert_eq!(
            line,
            "SUMMARY [3600s]: judged=0 pass=0 reject=0 failed=0 busy=25% reject{} failed{}"
        );
    }

    /// 今使っているメモリと、前回からの一番多かったメモリを MB で出すこと
    #[test]
    fn test_memory_reads_current_and_peak() {
        let status = "Name:\tapp\nVmHWM:\t   75460 kB\nVmRSS:\t   11652 kB\nVmSwap:\t    1148 kB\n";

        assert_eq!(memory(status), "rss=11MB peak=73MB");
    }

    /// メモリの値が読めない環境では、? と出して止まらないこと
    #[test]
    fn test_memory_unknown_without_proc() {
        assert_eq!(memory(""), "rss=? peak=?");
    }

    /// 画像置き場とのやり取り以外で起きた失敗は、その他として分けること
    #[test]
    fn test_failure_kind_other() {
        assert_eq!(
            failure_kind(&anyhow::anyhow!("judge thread has stopped")),
            "other"
        );
    }

    async fn fetch_error(reply: Option<&'static str>) -> anyhow::Error {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 1024];
            let _ = socket.read(&mut buf).await;
            match reply {
                Some(reply) => socket.write_all(reply.as_bytes()).await.unwrap(),
                None => tokio::time::sleep(Duration::from_secs(5)).await,
            }
        });
        let client = reqwest::Client::builder()
            .timeout(Duration::from_millis(200))
            .build()
            .unwrap();
        let result = async {
            client
                .get(format!("http://{addr}/"))
                .send()
                .await?
                .error_for_status()?;
            anyhow::Ok(())
        }
        .await;
        result.unwrap_err()
    }

    /// 画像置き場が忙しい・時間切れと返したときは 5xx、画像が無いと返したときは 4xx、返事が来ないまま待ちきれなかったときは timeout に分けること
    #[tokio::test]
    async fn test_failure_kind_of_fetch_errors() {
        let busy = fetch_error(Some(
            "HTTP/1.1 503 Service Unavailable\r\ncontent-length: 0\r\n\r\n",
        ))
        .await;
        let missing =
            fetch_error(Some("HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\n\r\n")).await;
        let silent = fetch_error(None).await;

        assert_eq!(
            [
                failure_kind(&busy),
                failure_kind(&missing),
                failure_kind(&silent)
            ],
            ["5xx", "4xx", "timeout"]
        );
    }
}
