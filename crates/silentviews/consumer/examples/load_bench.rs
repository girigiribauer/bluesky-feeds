use std::path::Path;
use std::time::Instant;

use silentviews_consumer::stage2::{clip, faces, objects, ocr};

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

fn main() -> anyhow::Result<()> {
    let path = std::env::args().nth(1).expect("image");
    let raw = std::fs::read(path)?;
    let (bgr, _) = vision::img::decode(&raw).unwrap();
    let models = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/models"));
    println!("始め 今 {} MB", rss());
    for round in 1..=3 {
        for name in ["objects", "ocr", "faces", "clip"] {
            let before = rss();
            let t = Instant::now();
            let load_ms;

            let peak = match name {
                "objects" => {
                    let mut m = objects::ObjectDetector::load(models)?;
                    load_ms = t.elapsed().as_millis();
                    m.detect(&bgr)?;
                    rss()
                }
                "ocr" => {
                    let mut m = ocr::TextDetector::load(models)?;
                    load_ms = t.elapsed().as_millis();
                    m.detect(&bgr)?;
                    rss()
                }
                "faces" => {
                    let mut m = faces::FaceDetectors::load(models)?;
                    load_ms = t.elapsed().as_millis();
                    m.measure(&bgr)?;
                    rss()
                }
                _ => {
                    let mut m = clip::ClipScorer::load(models)?;
                    load_ms = t.elapsed().as_millis();
                    m.score(&bgr)?;
                    rss()
                }
            };
            println!(
                "{round}回目 {name:<8} 読み込み {load_ms:>5} ms  前 {before:>4} MB  使用中 {peak:>4} MB  捨てた後 {:>4} MB",
                rss()
            );
        }
    }
    Ok(())
}
