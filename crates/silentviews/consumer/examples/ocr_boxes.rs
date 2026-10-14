use std::collections::BTreeMap;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let (list, out) = (&args[1], &args[2]);
    let models = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/models"));
    let mut ocr = silentviews_consumer::stage2::ocr::TextDetector::load(models)?;
    let mut result = BTreeMap::new();
    for path in std::fs::read_to_string(list)?.split_whitespace() {
        let Some((bgr, _)) = vision::img::decode(&std::fs::read(path)?) else {
            continue;
        };
        let (w, h) = (bgr.w as f64, bgr.h as f64);
        let boxes: Vec<[f64; 4]> = ocr
            .polygons(&bgr)?
            .iter()
            .map(|p| {
                let xs = p.iter().map(|q| q.0 as f64);
                let ys = p.iter().map(|q| q.1 as f64);
                [
                    xs.clone().fold(f64::MAX, f64::min) / w,
                    ys.clone().fold(f64::MAX, f64::min) / h,
                    xs.fold(f64::MIN, f64::max) / w,
                    ys.fold(f64::MIN, f64::max) / h,
                ]
            })
            .collect();
        result.insert(path.to_string(), boxes);
    }
    std::fs::write(out, serde_json::to_string(&result)?)?;
    Ok(())
}
