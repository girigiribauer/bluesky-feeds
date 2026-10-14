use crate::cv;
use vision::filter::{self, Plane};
use vision::img::{self, Image};

pub struct Extra {
    pub ground_gray: f64,
    pub mirror: f64,
    pub aspect: f64,
}

pub fn measure(bgr: &Image) -> Extra {
    let small = cv::fit(bgr, 256, false);
    let (h, w) = (small.h, small.w);
    let hsv_img = img::hsv(&small);
    let hsv = cv::pixels(&hsv_img);
    let gray = cv::plane(&img::gray(&small));

    let mut abs_lap = filter::laplacian(&gray);
    abs_lap.data.iter_mut().for_each(|v| *v = v.abs());
    let lap_blur = filter::box_mean(&abs_lap, 9, 9).data;
    let mut plane = 0usize;
    for i in (h / 2) * w..h * w {
        let (s, v) = (hsv[i][1] as i32, hsv[i][2] as i32);
        if s < 40 && v > 60 && v < 220 && lap_blur[i] < 12.0 {
            plane += 1;
        }
    }
    let lower = (h - h / 2) * w;

    Extra {
        ground_gray: plane as f64 / lower as f64,
        mirror: mirror(&gray),
        aspect: w.max(h) as f64 / w.min(h).max(1) as f64,
    }
}

fn mirror(gray: &Plane) -> f64 {
    let v: Vec<f64> = filter::resize_area(gray, 64, 64)
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

    /// 横長・縦長どちらでも、長い辺÷短い辺を縦横比とすること（試作と同じく 256px に縮めた後の大きさで測る）
    #[test]
    fn test_aspect_is_long_side_over_short_side() {
        assert_eq!(measure(&solid(512, 256, (0.0, 0.0, 0.0))).aspect, 2.0);
        assert_eq!(measure(&solid(256, 512, (0.0, 0.0, 0.0))).aspect, 2.0);
    }

    /// 左右対称な模様は左右対称度が高く、ざらついた模様は低いこと
    #[test]
    fn test_mirror_is_high_only_for_symmetric_images() {
        let mut sym = solid(256, 256, (0.0, 0.0, 0.0));
        fill(&mut sym, Rect::new(20, 40, 40, 100), (255.0, 255.0, 255.0));
        fill(&mut sym, Rect::new(196, 40, 40, 100), (255.0, 255.0, 255.0));
        assert!(measure(&sym).mirror > 0.9);

        let rough = noise(256, 256, (100.0, 100.0, 100.0), 40.0, 5);
        assert!(measure(&rough).mirror.abs() < 0.2);
    }

    /// 画面の下半分がのっぺりした灰色なら舗装の割合を全部と測り、上半分の灰色は数えないこと
    #[test]
    fn test_ground_gray_looks_only_at_lower_half() {
        let mut img = noise(256, 256, (60.0, 120.0, 60.0), 40.0, 6);
        fill(&mut img, Rect::new(0, 128, 256, 128), (120.0, 120.0, 120.0));
        assert!(measure(&img).ground_gray > 0.95);

        let mut img = noise(256, 256, (60.0, 120.0, 60.0), 40.0, 6);
        fill(&mut img, Rect::new(0, 0, 256, 128), (120.0, 120.0, 120.0));
        assert!(measure(&img).ground_gray < 0.05);
    }

    /// 下半分が灰色でも、ざらついた模様なら舗装とみなさないこと
    #[test]
    fn test_textured_gray_is_not_ground() {
        let mut img = noise(256, 256, (120.0, 120.0, 120.0), 40.0, 11);
        let top = noise(256, 128, (60.0, 120.0, 60.0), 40.0, 12);
        img.data[..top.data.len()].copy_from_slice(&top.data);

        assert!(measure(&img).ground_gray < 0.05);
    }
}
