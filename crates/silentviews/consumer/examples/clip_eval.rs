use std::collections::BTreeMap;
use std::time::Instant;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let (model, list, out) = (&args[1], &args[2], &args[3]);
    let mut clip =
        silentviews_consumer::stage2::clip::ClipScorer::load(std::path::Path::new(model))?;
    let mut result = BTreeMap::new();
    let mut elapsed = 0.0;
    for path in std::fs::read_to_string(list)?.lines() {
        let Some((bgr, _)) = vision::img::decode(&std::fs::read(path)?) else {
            continue;
        };
        let t = Instant::now();
        let c = clip.score(&bgr)?;
        elapsed += t.elapsed().as_secs_f64();
        result.insert(path.to_string(), (c.toon_p, c.close_p));
    }
    std::fs::write(out, serde_json::to_string(&result)?)?;
    println!(
        "{} images, {:.1} ms/image",
        result.len(),
        elapsed * 1000.0 / result.len() as f64
    );
    Ok(())
}
