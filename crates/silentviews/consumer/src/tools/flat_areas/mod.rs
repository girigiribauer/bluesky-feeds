mod judge;

pub use judge::judge;

use crate::cv;
use vision::filter;
use vision::img::{self, Image};

pub struct FlatAreas {
    pub smooth_share: f64,
    pub dead_flat: f64,
    pub dead_flat_ns: f64,
}

pub fn measure(bgr: &Image, gray_decoded: &Image) -> FlatAreas {
    let (smooth_share, dead_flat) = {
        let (mean, std) = local_mean_std(&cv::fit(gray_decoded, 768, false));
        let smooth: Vec<bool> = mean
            .iter()
            .zip(&std)
            .map(|(&m, &s)| s < 4.0 && m > 12.0 && m < 243.0)
            .collect();
        drop(mean);
        let smooth_count = smooth.iter().filter(|&&s| s).count();
        let smooth_share = smooth_count as f64 / smooth.len() as f64;
        let dead_flat = if smooth_count < 100 {
            -1.0
        } else {
            dead_share(&smooth, &std, smooth_count)
        };
        (smooth_share, dead_flat)
    };

    let small = cv::fit(bgr, 768, false);
    let gray = img::gray(&small);
    let hsv_img = img::hsv(&small);
    drop(small);
    let (mean, std) = local_mean_std(&gray);
    drop(gray);
    let smooth_ns: Vec<bool> = mean
        .iter()
        .zip(&std)
        .zip(cv::pixels(&hsv_img))
        .map(|((&m, &s), p)| {
            let (h, sat, v) = (p[0] as i32, p[1] as i32, p[2] as i32);
            let skyish =
                ((90..=130).contains(&h) && sat >= 25 && v >= 90) || (sat < 40 && v >= 150);
            s < 4.0 && m > 12.0 && m < 243.0 && !skyish
        })
        .collect();
    drop((mean, hsv_img));
    let count_ns = smooth_ns.iter().filter(|&&s| s).count();
    let dead_flat_ns = if count_ns < 100 {
        0.0
    } else {
        dead_share(&smooth_ns, &std, count_ns)
    };

    FlatAreas {
        smooth_share,
        dead_flat,
        dead_flat_ns,
    }
}

fn dead_share(mask: &[bool], std: &[f32], count: usize) -> f64 {
    let dead = mask.iter().zip(std).filter(|(&m, &s)| m && s < 0.5).count();
    dead as f64 / count as f64
}

fn local_mean_std(gray: &Image) -> (Vec<f32>, Vec<f32>) {
    let g = cv::plane(gray);
    let mean = filter::box_mean(&g, 7, 7).data;
    let squared = filter::square(&g);
    drop(g);
    let mut std = filter::box_mean(&squared, 7, 7).data;
    drop(squared);
    for (s, &m) in std.iter_mut().zip(&mean) {
        *s = (*s - m * m).max(0.0).sqrt();
    }
    (mean, std)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cv::testing::{gray_of, noise, solid};

    /// 一様な灰色の画像は、なめらかな部分も、色の変化がまったく無い平らな部分も全体と測ること
    #[test]
    fn test_uniform_gray_is_all_dead_flat() {
        let img = solid(300, 200, (100.0, 100.0, 100.0));
        let f = measure(&img, &gray_of(&img));

        assert_eq!(f.smooth_share, 1.0);
        assert_eq!(f.dead_flat, 1.0);
        assert_eq!(f.dead_flat_ns, 1.0);
    }

    /// 空のような明るい色の平らな部分は、空を除いた平らさには数えないこと
    #[test]
    fn test_sky_colored_flat_is_excluded_from_no_sky_measure() {
        let img = solid(300, 200, (230.0, 200.0, 170.0));
        let f = measure(&img, &gray_of(&img));

        assert_eq!(f.dead_flat, 1.0);
        assert_eq!(f.dead_flat_ns, 0.0);
    }

    /// ざらついた画像は、なめらかな部分がほぼ無いので、平らさを「測れない」ことを示す -1 にすること
    #[test]
    fn test_noisy_image_has_no_smooth_area() {
        let img = noise(300, 200, (100.0, 100.0, 100.0), 30.0, 4);
        let f = measure(&img, &gray_of(&img));

        assert!(f.smooth_share < 0.01, "{}", f.smooth_share);
        assert_eq!(f.dead_flat, -1.0);
        assert_eq!(f.dead_flat_ns, 0.0);
    }
}
