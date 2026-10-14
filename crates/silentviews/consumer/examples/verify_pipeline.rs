use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

use silentviews_consumer::pipeline::{self, Models};

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let (work, samples, allow, small_dir, large_dir, out) =
        (&args[1], &args[2], &args[3], &args[4], &args[5], &args[6]);
    let models = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/models"));
    let m = Models::load(models)?;
    let counts: Vec<usize> = std::fs::read_to_string(format!("{work}/{samples}"))?
        .lines()
        .map(|l| {
            serde_json::from_str::<serde_json::Value>(l).unwrap()["images"]
                .as_array()
                .unwrap()
                .len()
        })
        .collect();
    let allow: Vec<usize> = std::fs::read_to_string(format!("{work}/{allow}"))?
        .split_whitespace()
        .map(|v| v.parse().unwrap())
        .collect();
    let mut result = BTreeMap::new();
    let total = allow.len();
    for (i, n) in allow.into_iter().enumerate() {
        let keys: Vec<String> = (0..counts[n]).map(|k| format!("{n:05}_{k}")).collect();
        let read = |dir: &str| -> Option<Vec<Vec<u8>>> {
            keys.iter()
                .map(|k| std::fs::read(format!("{work}/{dir}/{k}.jpg")).ok())
                .collect()
        };
        let verdict = match read(small_dir) {
            None => "missing_small".to_string(),
            Some(smalls) => match pipeline::stage1(&smalls) {
                Err(r) => format!("{r:?}"),
                Ok(()) => match read(large_dir) {
                    None => "missing_large".to_string(),
                    Some(larges) => match pipeline::stage2(&m, &larges) {
                        Ok(()) => "PASS".to_string(),
                        Err(r) => format!("{r:?}"),
                    },
                },
            },
        };
        result.insert(n, verdict);
        if i % 2000 == 0 {
            eprintln!("{i}/{total}");
            std::io::stderr().flush().ok();
        }
    }
    std::fs::write(out, serde_json::to_string(&result)?)?;
    Ok(())
}
