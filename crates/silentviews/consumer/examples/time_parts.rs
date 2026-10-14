use std::time::Instant;

use silentviews_consumer::stage2::{
    basic, clip, depth, extra, faces, flat, manmade, near, objects, ocr,
};

fn main() -> anyhow::Result<()> {
    let list = std::env::args().nth(1).expect("image list");
    let models = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/models"));
    let mut faces = faces::FaceDetectors::load(models)?;
    let mut objects = objects::ObjectDetector::load(models)?;
    let mut ocr = ocr::TextDetector::load(models)?;
    let mut clip = clip::ClipScorer::load(models)?;
    let names = [
        "decode", "stage1", "basic", "depth", "flat", "extra", "manmade", "faces", "objects",
        "ocr", "near", "clip",
    ];
    let mut total = vec![0f64; names.len()];
    let mut n = 0;
    for path in std::fs::read_to_string(list)?.lines() {
        let raw = std::fs::read(path)?;
        let mut t = Instant::now();
        let mut lap = |i: usize, t: &mut Instant| {
            total[i] += t.elapsed().as_secs_f64();
            *t = Instant::now();
        };
        let Some((bgr, gray)) = vision::img::decode(&raw) else {
            continue;
        };
        lap(0, &mut t);
        silentviews_consumer::stage1::features(&bgr)?;
        lap(1, &mut t);
        basic::measure(&bgr);
        lap(2, &mut t);
        depth::measure(&bgr).ok();
        lap(3, &mut t);
        flat::measure(&bgr, &gray);
        lap(4, &mut t);
        extra::measure(&bgr);
        lap(5, &mut t);
        manmade::measure(&bgr);
        lap(6, &mut t);
        faces.measure(&bgr)?;
        lap(7, &mut t);
        objects.detect(&bgr)?;
        lap(8, &mut t);
        ocr.detect(&bgr)?;
        lap(9, &mut t);
        near::measure(&bgr, &gray).ok();
        lap(10, &mut t);
        clip.score(&bgr)?;
        lap(11, &mut t);
        n += 1;
    }
    for (name, s) in names.iter().zip(&total) {
        println!("{name:>8} {:7.1} ms", s * 1000.0 / n as f64);
    }
    println!(
        "{:>8} {:7.1} ms ({n} images)",
        "total",
        total.iter().sum::<f64>() * 1000.0 / n as f64
    );
    Ok(())
}
