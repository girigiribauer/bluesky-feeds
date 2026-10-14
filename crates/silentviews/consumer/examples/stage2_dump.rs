use std::collections::BTreeMap;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let (dir, keys, out) = (&args[1], &args[2], &args[3]);
    let keys: Vec<String> = std::fs::read_to_string(keys)?
        .split_whitespace()
        .map(str::to_string)
        .collect();
    let mut faces = silentviews_consumer::stage2::faces::FaceDetectors::load(
        std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/models")),
    )?;
    let models = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/models"));
    let mut objects = silentviews_consumer::stage2::objects::ObjectDetector::load(models)?;
    let mut ocr = silentviews_consumer::stage2::ocr::TextDetector::load(models)?;
    let mut clip = silentviews_consumer::stage2::clip::ClipScorer::load(models)?;
    let mut result = BTreeMap::new();
    for key in keys {
        let Ok(raw) = std::fs::read(format!("{dir}/{key}.jpg")) else {
            continue;
        };
        let Some((bgr, gray)) = vision::img::decode(&raw) else {
            continue;
        };
        let mut measure = || -> anyhow::Result<serde_json::Value> {
            let mut v =
                serde_json::to_value(silentviews_consumer::stage2::measure_light(&bgr, &gray)?)?;
            let f = faces.measure(&bgr)?;
            v["yn_count"] = f.yn_count.into();
            v["yn_max_area"] = f.yn_max_area.into();
            let objs: Vec<(String, f64, f64)> = objects
                .detect(&bgr)?
                .into_iter()
                .map(|o| (o.name.to_string(), o.conf, o.area))
                .collect();
            v["objects"] = serde_json::to_value(objs)?;
            let o = ocr.detect(&bgr)?;
            v["ocr_n"] = o.ocr_n.into();
            v["ocr_area"] = o.ocr_area.into();
            v["ocr_lines"] = o.ocr_lines.into();
            v["ocr_edge"] = o.ocr_edge.into();
            v["max_side"] = bgr.h.max(bgr.w).into();
            let c = clip.score(&bgr)?;
            v["toon_p"] = c.toon_p.into();
            v["close_p"] = c.close_p.into();
            v["manmade_p"] = c.manmade_p.into();
            v["window_p"] = c.window_p.into();
            let n = silentviews_consumer::stage2::near::measure(&bgr, &gray)?;
            v["rt_min"] = n.rt_min.into();
            v["top_smooth"] = n.top_smooth.into();
            if let Some(ln) = n.ln_count {
                v["ln_count"] = ln.into();
            }
            Ok(v)
        };
        if let Ok(v) = measure() {
            result.insert(key, v);
        }
    }
    std::fs::write(out, serde_json::to_string(&result)?)?;
    println!("{}", result.len());
    Ok(())
}
