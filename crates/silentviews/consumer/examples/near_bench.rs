use std::time::Instant;

use silentviews_consumer::cv;
use vision::img::{self, Image, Interp};
use vision::{filter, kmeans, lsd, shape};

fn main() -> anyhow::Result<()> {
    let list = std::env::args().nth(1).expect("image list");
    let images: Vec<_> = std::fs::read_to_string(list)?
        .lines()
        .filter_map(|p| img::decode(&std::fs::read(p).ok()?))
        .collect();
    let names = [
        "fit",
        "gray",
        "bands",
        "lsd",
        "blur_u8",
        "lab",
        "kmeans",
        "components",
        "erode",
        "top",
    ];
    let mut total = [0f64; 10];
    let t0 = Instant::now();
    for (bgr, gray_decoded) in &images {
        let mut t = Instant::now();
        let mut lap = |i: usize, t: &mut Instant| {
            total[i] += t.elapsed().as_secs_f64();
            *t = Instant::now();
        };
        let s = 512.0 / bgr.h.max(bgr.w) as f64;
        let small = img::resize(
            bgr,
            (bgr.w as f64 * s) as usize,
            (bgr.h as f64 * s) as usize,
            Interp::Area,
        );
        lap(0, &mut t);
        let gray = img::gray(&small);
        lap(1, &mut t);
        let g = cv::plane(&gray);
        for sigma in [0.7, 1.4, 2.8, 5.6] {
            filter::gaussian(&g, sigma);
        }
        lap(2, &mut t);
        lsd::detect(&gray);
        lap(3, &mut t);
        let blur = filter::gaussian_u8(&small, 0, 6.0);
        lap(4, &mut t);
        let samples: Vec<f32> = img::lab(&blur).data.iter().map(|&v| v as f32).collect();
        lap(5, &mut t);
        for k in [6, 8, 10] {
            let labels = kmeans::kmeans_pp(&samples, 3, k, 20, 1.0, 2, 12345);
            lap(6, &mut t);
            for c in 0..k as i32 {
                let mask: Vec<u8> = labels.iter().map(|&l| (l == c) as u8).collect();
                let (cc, areas) = shape::connected_components(&mask, small.w, small.h, true);
                lap(7, &mut t);
                for (i, &a) in areas.iter().enumerate().skip(1) {
                    if a as f64 >= 0.03 * labels.len() as f64 {
                        let m = Image {
                            w: small.w,
                            h: small.h,
                            ch: 1,
                            data: cc.iter().map(|&v| (v == i as i32) as u8).collect(),
                        };
                        filter::erode_rect(&m, 13, 13);
                    }
                }
                lap(8, &mut t);
            }
        }
        let s = 256.0 / gray_decoded.h.max(gray_decoded.w) as f64;
        let tg = cv::plane(&img::resize(
            gray_decoded,
            (gray_decoded.w as f64 * s) as usize,
            (gray_decoded.h as f64 * s) as usize,
            Interp::Area,
        ));
        filter::box_mean(&tg, 9, 9);
        filter::box_mean(&filter::square(&tg), 9, 9);
        lap(9, &mut t);
    }
    let n = images.len() as f64;
    for (name, s) in names.iter().zip(&total) {
        println!("{name:>10} {:7.1} ms", s * 1000.0 / n);
    }
    println!(
        "{:>10} {:7.1} ms",
        "total",
        t0.elapsed().as_secs_f64() * 1000.0 / n
    );
    Ok(())
}
