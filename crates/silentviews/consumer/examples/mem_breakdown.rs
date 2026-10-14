use std::path::Path;

use silentviews_consumer::stage2::{clip, faces, measure_light, near, objects, ocr};

fn mem() -> (u64, u64) {
    let s = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    let get = |k: &str| {
        s.lines()
            .find(|l| l.starts_with(k))
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(0)
            / 1024
    };
    (get("VmRSS:"), get("VmHWM:"))
}

fn mark(label: &str) {
    let (rss, hwm) = mem();
    println!("{label:<28} 今 {rss:>4} MB  最大 {hwm:>4} MB");
}

fn main() -> anyhow::Result<()> {
    let list = std::env::args().nth(1).expect("image list");
    let models = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/models"));
    let images: Vec<(Vec<u8>, _)> = std::fs::read_to_string(list)?
        .lines()
        .filter_map(|p| {
            let raw = std::fs::read(p).ok()?;
            let d = vision::img::decode(&raw)?;
            Some((raw, d))
        })
        .collect();
    mark(&format!("起動・画像{}枚読み込み", images.len()));
    let mut f = faces::FaceDetectors::load(models)?;
    mark("顔のモデル読み込み");
    let mut o = objects::ObjectDetector::load(models)?;
    mark("物のモデル読み込み");
    let mut t = ocr::TextDetector::load(models)?;
    mark("文字のモデル読み込み");
    let mut c = clip::ClipScorer::load(models)?;
    mark("CLIP のモデル読み込み");
    let (_, (bgr, gray)) = &images[0];
    measure_light(bgr, gray)?;
    mark("1枚目 軽い計算");
    o.detect(bgr)?;
    mark("1枚目 物");
    t.detect(bgr)?;
    mark("1枚目 文字");
    f.measure(bgr)?;
    mark("1枚目 顔");
    near::measure(bgr, gray).ok();
    mark("1枚目 距離");
    c.score(bgr)?;
    mark("1枚目 CLIP");
    for (_, (bgr, gray)) in &images[1..] {
        measure_light(bgr, gray)?;
        o.detect(bgr)?;
        t.detect(bgr)?;
        f.measure(bgr)?;
        near::measure(bgr, gray).ok();
        c.score(bgr)?;
    }
    mark("全部の画像で全部の部品");
    Ok(())
}
