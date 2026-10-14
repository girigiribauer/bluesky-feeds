use std::collections::BTreeMap;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let (dir, out) = (&args[1], &args[2]);
    let mut names: Vec<_> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok().map(|e| e.file_name().to_string_lossy().into_owned()))
        .filter(|name| name.ends_with(".jpg"))
        .collect();
    names.sort();
    let mut result = BTreeMap::new();
    for name in names {
        let Some((bgr, _)) = vision::img::decode(&std::fs::read(format!("{dir}/{name}"))?) else {
            continue;
        };
        let Ok(features) = silentviews_consumer::stage1::features(&bgr) else {
            continue;
        };
        result.insert(name.trim_end_matches(".jpg").to_string(), features);
    }
    std::fs::write(out, serde_json::to_string(&result)?)?;
    println!("{}", result.len());
    Ok(())
}
