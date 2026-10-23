use std::collections::BTreeMap;
use std::path::Path;

use silentviews_consumer::pipeline::Models;
use silentviews_consumer::stage2::{clip, faces, measure_light, near, objects, ocr};

fn status(key: &str) -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .unwrap_or_default()
        .lines()
        .find(|l| l.starts_with(key))
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0)
}

fn reset_peak() {
    let _ = std::fs::write("/proc/self/clear_refs", "5");
}

fn trim() {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    unsafe {
        libc::malloc_trim(0);
    }
}

fn regions(label: &str) {
    let smaps = std::fs::read_to_string("/proc/self/smaps").unwrap_or_default();
    let mut groups: BTreeMap<String, u64> = BTreeMap::new();
    let mut name = String::new();
    for line in smaps.lines() {
        let first = line.split_whitespace().next().unwrap_or("");
        if first.contains('-') && !first.ends_with(':') {
            let path = line.split_whitespace().nth(5).unwrap_or("");
            name = if path.is_empty() {
                "確保した分（名前なし）".to_string()
            } else if path.starts_with('[') {
                path.to_string()
            } else if path.contains(".so") {
                "共通の部品（.so）".to_string()
            } else if path.contains("rosetta") {
                "Rosetta".to_string()
            } else {
                format!(
                    "プログラム本体（{}）",
                    path.rsplit('/').next().unwrap_or(path)
                )
            };
        } else if line.starts_with("Rss:") {
            let kb: u64 = line
                .split_whitespace()
                .nth(1)
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);
            *groups.entry(name.clone()).or_default() += kb;
        }
    }
    println!("== {label}: 合計 {} KB", status("VmRSS:"));
    let mut list: Vec<_> = groups.into_iter().collect();
    list.sort_by_key(|item| std::cmp::Reverse(item.1));
    for (name, kb) in list {
        if kb >= 100 {
            println!("  {kb:>7} KB  {name}");
        }
    }
}

fn main() -> anyhow::Result<()> {
    let list = std::env::args().nth(1).expect("image list");
    let n: usize = std::env::args()
        .nth(2)
        .map(|v| v.parse().unwrap())
        .unwrap_or(20);
    let models = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/models"));
    regions("起動直後");
    let _models = Models::load(models)?;
    trim();
    regions("モデルを一度読んで捨てた後");
    let mut peaks: BTreeMap<&str, (u64, u64, u64, u64)> = BTreeMap::new();
    let mut step = |label: &'static str, run: &mut dyn FnMut()| {
        trim();
        let base = status("VmRSS:");
        reset_peak();
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let watcher = {
            let stop = stop.clone();
            std::thread::spawn(move || {
                let (mut anon, mut file) = (0, 0);
                while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                    anon = anon.max(status("RssAnon:"));
                    file = file.max(status("RssFile:"));
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
                (anon, file)
            })
        };
        run();
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        let (anon, file) = watcher.join().unwrap();
        let peak = status("VmHWM:");
        let e = peaks.entry(label).or_default();
        e.0 = e.0.max(peak);
        e.1 = e.1.max(peak.saturating_sub(base));
        e.2 = e.2.max(anon);
        e.3 = e.3.max(file);
    };
    for path in std::fs::read_to_string(list)?.lines().take(n) {
        let raw = std::fs::read(path)?;
        let mut decoded = None;
        step("1 画像を広げる", &mut || {
            decoded = vision::img::decode(&raw)
        });
        let (bgr, gray) = decoded.expect("decodable image");
        step("2 軽い計算", &mut || {
            measure_light(&bgr, &gray).ok();
        });
        step("2a basic", &mut || {
            silentviews_consumer::stage2::basic::measure(&bgr);
        });
        step("2b depth", &mut || {
            silentviews_consumer::stage2::depth::measure(&bgr).ok();
        });
        step("2c flat", &mut || {
            silentviews_consumer::stage2::flat::measure(&bgr, &gray);
        });
        step("2d extra", &mut || {
            silentviews_consumer::stage2::extra::measure(&bgr);
        });
        step("2e manmade", &mut || {
            silentviews_consumer::stage2::manmade::measure(&bgr);
        });
        step("3 物探し", &mut || {
            objects::ObjectDetector::load(models)
                .unwrap()
                .detect(&bgr)
                .ok();
        });
        step("4 CLIP", &mut || {
            clip::ClipScorer::load(models).unwrap().score(&bgr).ok();
        });
        step("5 文字探し", &mut || {
            ocr::TextDetector::load(models).unwrap().detect(&bgr).ok();
        });
        step("6 顔探し", &mut || {
            faces::FaceDetectors::load(models)
                .unwrap()
                .measure(&bgr)
                .ok();
        });
        step("7 距離", &mut || {
            near::measure(&bgr, &gray).ok();
        });
        let s = 512.0 / bgr.w.max(bgr.h) as f64;
        let small = vision::img::resize(
            &bgr,
            (bgr.w as f64 * s) as usize,
            (bgr.h as f64 * s) as usize,
            vision::img::Interp::Area,
        );
        let gray512 = vision::img::gray(&small);
        step("7a 線（LSD）", &mut || {
            vision::lsd::detect(&gray512);
        });
        step("7b ぼかし4枚", &mut || {
            let g = silentviews_consumer::cv::plane(&gray512);
            for sigma in [0.7, 1.4, 2.8, 5.6] {
                vision::filter::gaussian(&g, sigma);
            }
        });
        step("7c 色で分ける（k=10）", &mut || {
            let blur = vision::filter::gaussian_u8(&small, 0, 6.0);
            let samples: Vec<f32> = vision::img::lab(&blur)
                .data
                .iter()
                .map(|&v| v as f32)
                .collect();
            vision::kmeans::kmeans_pp(&samples, 3, 10, 20, 1.0, 2, 12345);
        });
        step("8 マス", &mut || {
            clip::ClipScorer::load(models)
                .unwrap()
                .tile_toon_max(&bgr)
                .ok();
        });
    }
    trim();
    regions("全部の画像を測った後");
    let smaps = std::fs::read_to_string("/proc/self/smaps").unwrap_or_default();
    let mut head = String::new();
    let mut anon = Vec::new();
    for line in smaps.lines() {
        let first = line.split_whitespace().next().unwrap_or("");
        if first.contains('-') && !first.ends_with(':') {
            head = line.to_string();
        } else if line.starts_with("Rss:") && head.split_whitespace().nth(5).is_none() {
            let kb: u64 = line
                .split_whitespace()
                .nth(1)
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);
            anon.push((kb, head.clone()));
        }
    }
    anon.sort_by_key(|item| std::cmp::Reverse(item.0));
    for (kb, head) in anon.iter().take(8) {
        println!("  名前なし {kb:>6} KB  {head}");
    }
    println!("== 段階ごとの最大（その段階の最中のいちばん多いとき・始める前からの増え方）");
    for (label, (peak, rise, anon, file)) in peaks {
        println!(
            "  {label:<10} 最大 {:>6} KB  増え方 {:>6} KB  確保 {:>6} KB  ファイル {:>6} KB",
            peak, rise, anon, file
        );
    }
    Ok(())
}
