use std::collections::BTreeMap;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let (model, list, out) = (&args[1], &args[2], &args[3]);
    let mut clip =
        silentviews_consumer::stage2::clip::ClipScorer::load(std::path::Path::new(model))?;
    let mut result = BTreeMap::new();
    for path in std::fs::read_to_string(list)?.lines() {
        let Some((bgr, _)) = vision::img::decode(&std::fs::read(path)?) else {
            continue;
        };
        result.insert(path.to_string(), clip.image_embed(&bgr)?);
    }
    std::fs::write(out, serde_json::to_string(&result)?)?;
    println!("{} images", result.len());
    Ok(())
}
