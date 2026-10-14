use std::path::Path;

use silentviews_consumer::pipeline::{self, Models};

fn hwm() -> u64 {
    status("VmHWM:")
}

fn rss() -> u64 {
    status("VmRSS:")
}

fn status(key: &str) -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .unwrap_or_default()
        .lines()
        .find(|l| l.starts_with(key))
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0)
        / 1024
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let (work, samples, allow, small_dir, large_dir) =
        (&args[1], &args[2], &args[3], &args[4], &args[5]);
    let models = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/models"));
    let m = Models::load(models)?;
    println!("起動後 最大 {} MB", hwm());
    let counts: Vec<usize> = std::fs::read_to_string(format!("{work}/{samples}"))?
        .lines()
        .map(|l| {
            serde_json::from_str::<serde_json::Value>(l).unwrap()["images"]
                .as_array()
                .unwrap()
                .len()
        })
        .collect();
    let mut last = hwm();
    let mut gaps = Vec::new();
    for n in std::fs::read_to_string(format!("{work}/{allow}"))?.split_whitespace() {
        let n: usize = n.parse()?;
        let keys: Vec<String> = (0..counts[n]).map(|k| format!("{n:05}_{k}")).collect();
        let read = |dir: &str| -> Option<Vec<Vec<u8>>> {
            keys.iter()
                .map(|k| std::fs::read(format!("{work}/{dir}/{k}.jpg")).ok())
                .collect()
        };
        let (Some(smalls), Some(larges)) = (read(small_dir), read(large_dir)) else {
            continue;
        };
        let verdict = match pipeline::stage1(&smalls) {
            Err(r) => format!("{r:?}"),
            Ok(()) => format!("{:?}", pipeline::stage2(&m, &larges)),
        };
        gaps.push(rss());
        let now = hwm();
        if now > last + 2 {
            let sizes: Vec<String> = larges
                .iter()
                .filter_map(|b| vision::img::decode(b).map(|(i, _)| format!("{}x{}", i.w, i.h)))
                .collect();
            println!("{n:05} 最大 {last} → {now} MB 画像 {sizes:?} {verdict}");
            last = now;
        }
    }
    gaps.sort_unstable();
    let at = |q: f64| gaps[((gaps.len() - 1) as f64 * q) as usize];
    println!(
        "投稿の合間 {}件: 最小 {} 中央 {} 9割 {} 最大 {} MB",
        gaps.len(),
        at(0.0),
        at(0.5),
        at(0.9),
        at(1.0)
    );
    println!("最後 最大 {} MB", hwm());
    Ok(())
}
