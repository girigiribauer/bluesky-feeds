mod judge;

pub use judge::judge;

use crate::cv;
use vision::filter::{self, Plane};
use vision::img::Image;

pub fn grain(bgr: &Image) -> f64 {
    let small = cv::fit(bgr, 1024, false);
    let (w, h) = (small.w, small.h);
    let mut g = Plane::new(w, h);
    for (v, p) in g.data.iter_mut().zip(cv::pixels(&small)) {
        *v = (p[0] as f32 + p[1] as f32 + p[2] as f32) / (3.0 * 255.0);
    }
    drop(small);
    let at = |x: usize, y: usize| g.data[y * w + x];
    let mut mag = Plane::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let gx = if x == 0 || x + 1 == w {
                0.0
            } else {
                at(x + 1, y) - at(x - 1, y)
            };
            let gy = if y == 0 || y + 1 == h {
                0.0
            } else {
                at(x, y + 1) - at(x, y - 1)
            };
            mag.data[y * w + x] = (gx * gx + gy * gy).sqrt();
        }
    }
    let calm = filter::gaussian(&mag, 2.0);
    drop(mag);
    let soft = filter::gaussian(&g, 1.0);
    let (mut sum, mut count) = (0f64, 0usize);
    for ((&v, &s), &c) in g.data.iter().zip(&soft.data).zip(&calm.data) {
        if c < 0.02 {
            sum += (v - s).abs() as f64;
            count += 1;
        }
    }
    if count <= 100 {
        return 0.0;
    }
    sum / count as f64 * 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cv::testing::{noise, solid};

    /// 一色で塗りつぶした画像は、ざらつきがほぼ0と測ること
    #[test]
    fn test_solid_image_has_no_grain() {
        let g = grain(&solid(300, 200, (40.0, 140.0, 60.0)));

        assert!(g < 0.01, "{g}");
    }

    /// 平らな色に細かい粒が乗った画像は、ざらつきが判定の線4.0を超えると測ること（画布の目や絵の具の粒を拾うため）
    #[test]
    fn test_fine_noise_on_flat_color_is_grain() {
        let g = grain(&noise(300, 200, (120.0, 120.0, 120.0), 3.0, 1));

        assert!(g > 4.0, "{g}");
    }

    /// 平らな所がほとんどない画像は、ざらつきを0とすること（画面全体が細かい模様の衛星写真などを、粒の多い絵と取り違えないため）
    #[test]
    fn test_no_flat_area_is_zero() {
        assert_eq!(grain(&noise(300, 200, (120.0, 120.0, 120.0), 60.0, 1)), 0.0);
    }

    /// 1px の画像でも止まらずに測れること
    #[test]
    fn test_single_pixel_image_is_measured() {
        assert_eq!(grain(&solid(1, 1, (120.0, 120.0, 120.0))), 0.0);
    }
}
