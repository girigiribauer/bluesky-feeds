use std::path::Path;
use std::time::Instant;

use silentviews_consumer::stage2::{
    basic, clip, depth, extra, faces, flat, manmade, near, objects, ocr,
};

fn hwm() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .unwrap_or_default()
        .lines()
        .find(|l| l.starts_with("VmHWM:"))
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0)
}

fn main() -> anyhow::Result<()> {
    let which = std::env::args().nth(1).unwrap_or_default();
    let path = std::env::args().nth(2).expect("image");
    let models = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/models"));
    let raw = std::fs::read(path)?;
    let before_decode = hwm();
    let (img, gray) = vision::img::decode(&raw).ok_or_else(|| anyhow::anyhow!("decode failed"))?;
    let before = hwm();
    let t = Instant::now();
    match which.as_str() {
        "decode" => {}
        "stage1" => {
            silentviews_consumer::stage1::features(&img)?;
        }
        "basic" => {
            basic::measure(&img);
        }
        "depth" => {
            depth::measure(&img)?;
        }
        "flat" => {
            flat::measure(&img, &gray);
        }
        "extra" => {
            extra::measure(&img);
        }
        "manmade" => {
            manmade::measure(&img);
        }
        "near" => {
            near::measure(&img, &gray)?;
        }
        "faces" => {
            faces::FaceDetectors::load(models)?.measure(&img)?;
        }
        "objects" => {
            objects::ObjectDetector::load(models)?.detect(&img)?;
        }
        "ocr" => {
            ocr::TextDetector::load(models)?.detect(&img)?;
        }
        "clip" => {
            clip::ClipScorer::load(models)?.score(&img)?;
        }
        _ => anyhow::bail!("unknown part"),
    }
    let ms = t.elapsed().as_secs_f64() * 1000.0;
    let (decode_kb, part_kb) = if which == "decode" {
        (before - before_decode, 0)
    } else {
        (before - before_decode, hwm() - before)
    };
    println!(
        "{which:<8} {}x{} 読み込み +{:.1} MB  部品 +{:.1} MB  {ms:.0} ms",
        img.w,
        img.h,
        decode_kb as f64 / 1024.0,
        part_kb as f64 / 1024.0
    );
    Ok(())
}
