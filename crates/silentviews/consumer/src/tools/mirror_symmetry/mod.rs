mod judge;

use crate::cv;
use vision::filter;
use vision::img::{self, Image};

pub use judge::judge;

pub fn measure(bgr: &Image) -> f64 {
    let gray = cv::plane(&img::gray(&cv::fit(bgr, 256, false)));
    let v: Vec<f64> = filter::resize_area(&gray, 64, 64)
        .data
        .iter()
        .map(|&x| x as f64)
        .collect();
    let mut centered = vec![0f64; 64 * 64];
    for r in 0..64 {
        let row = &v[r * 64..(r + 1) * 64];
        let mean = row.iter().sum::<f64>() / 64.0;
        for c in 0..64 {
            centered[r * 64 + c] = row[c] - mean;
        }
    }
    let n = centered.len() as f64;
    let mean = centered.iter().sum::<f64>() / n;
    let std = (centered.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n).sqrt() + 1e-6;
    let mut acc = 0.0;
    for r in 0..64 {
        for c in 0..64 {
            acc += (centered[r * 64 + c] / std) * (centered[r * 64 + 63 - c] / std);
        }
    }
    acc / n
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cv::testing::{fill, noise, solid, Rect};

    /// 左右対称な模様は左右対称度が高く、ざらついた模様は低いこと
    #[test]
    fn test_mirror_is_high_only_for_symmetric_images() {
        let mut sym = solid(256, 256, (0.0, 0.0, 0.0));
        fill(&mut sym, Rect::new(20, 40, 40, 100), (255.0, 255.0, 255.0));
        fill(&mut sym, Rect::new(196, 40, 40, 100), (255.0, 255.0, 255.0));
        assert!(measure(&sym) > 0.9);

        let rough = noise(256, 256, (100.0, 100.0, 100.0), 40.0, 5);
        assert!(measure(&rough).abs() < 0.2);
    }
}
