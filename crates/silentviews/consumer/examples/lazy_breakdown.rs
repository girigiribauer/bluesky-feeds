use std::path::Path;

use silentviews_consumer::stage2::{clip, faces, measure_light, near, objects, ocr};

fn rss() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .unwrap_or_default()
        .lines()
        .find(|l| l.starts_with("VmRSS:"))
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0)
        / 1024
}

fn kinds() -> String {
    let s = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    let get = |k: &str| {
        s.lines()
            .find(|l| l.starts_with(k))
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(0)
            / 1024
    };
    format!(
        "確保 {} MB・ファイル {} MB・スレッド {}",
        get("RssAnon:"),
        get("RssFile:"),
        get("Threads:") * 1024
    )
}

fn regions() {
    let smaps = std::fs::read_to_string("/proc/self/smaps").unwrap_or_default();
    let mut list = Vec::new();
    let mut head = String::new();
    for line in smaps.lines() {
        if line.starts_with("Rss:") {
            let kb: u64 = line
                .split_whitespace()
                .nth(1)
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);
            list.push((kb, head.clone()));
        } else if line
            .split_whitespace()
            .next()
            .is_some_and(|w| w.contains('-') && !w.ends_with(':'))
        {
            head = line.to_string();
        }
    }
    list.sort_by(|a, b| b.0.cmp(&a.0));
    for (kb, head) in list.iter().take(10) {
        println!("{:>6} KB  {head}", kb);
    }
}

fn trim() {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    unsafe {
        libc::malloc_trim(0);
    }
}

fn heap() -> String {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    unsafe {
        let m = libc::mallinfo2();
        return format!(
            "使用中 {} MB・借りている {} MB・大きな塊 {} MB",
            m.uordblks >> 20,
            m.arena >> 20,
            m.hblkhd >> 20
        );
    }
    #[allow(unreachable_code)]
    String::new()
}

fn step(label: &str, run: impl FnOnce()) {
    let before = rss();
    run();
    trim();
    println!(
        "{label:<8} {before:>4} → {:>4} MB  {}  {}",
        rss(),
        kinds(),
        heap()
    );
}

fn main() -> anyhow::Result<()> {
    let list = std::env::args().nth(1).expect("image list");
    let rounds: usize = std::env::args()
        .nth(2)
        .map(|v| v.parse().unwrap())
        .unwrap_or(3);
    let models = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/models"));
    let paths: Vec<String> = std::fs::read_to_string(list)?
        .lines()
        .map(str::to_string)
        .collect();
    trim();
    println!("起動 {} MB", rss());
    for path in paths.iter().take(rounds) {
        println!("-- {path}");
        let raw = std::fs::read(path)?;
        let mut decoded = None;
        step("読み込み", || decoded = vision::img::decode(&raw));
        let (bgr, gray) = decoded.expect("decodable image");
        step("軽い計算", || {
            measure_light(&bgr, &gray).ok();
        });
        step("物", || {
            objects::ObjectDetector::load(models)
                .unwrap()
                .detect(&bgr)
                .ok();
        });
        step("CLIP", || {
            clip::ClipScorer::load(models).unwrap().score(&bgr).ok();
        });
        step("文字", || {
            ocr::TextDetector::load(models).unwrap().detect(&bgr).ok();
        });
        step("顔", || {
            faces::FaceDetectors::load(models)
                .unwrap()
                .measure(&bgr)
                .ok();
        });
        step("距離", || {
            near::measure(&bgr, &gray).ok();
        });
        step("マス", || {
            clip::ClipScorer::load(models)
                .unwrap()
                .tile_toon_max(&bgr)
                .ok();
        });
        drop((bgr, gray));
        trim();
        println!("画像を捨てた後 {} MB", rss());
    }
    regions();
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    unsafe {
        libc::malloc_stats();
    }
    Ok(())
}
