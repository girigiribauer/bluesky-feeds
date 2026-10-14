use std::path::Path;

use silentviews_consumer::stage2::ocr;

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
    let list = std::env::args().nth(1).expect("image list");
    let n: usize = std::env::args()
        .nth(2)
        .map(|v| v.parse().unwrap())
        .unwrap_or(20);
    let models = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/models"));
    let start = std::time::Instant::now();
    let mut found = Vec::new();
    for path in std::fs::read_to_string(list)?.lines().take(n) {
        let (bgr, _) = vision::img::decode(&std::fs::read(path)?).expect("decodable image");
        let o = ocr::TextDetector::load(models)?.detect(&bgr)?;
        found.push((o.ocr_n, o.ocr_lines, o.ocr_edge));
    }
    println!(
        "最大 {} MB・今 {} MB・{:.1}秒・結果 {:?}",
        status("VmHWM:"),
        status("VmRSS:"),
        start.elapsed().as_secs_f64(),
        &found[..found.len().min(8)]
    );
    Ok(())
}
