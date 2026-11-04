mod judge;

use crate::cv;
use vision::filter;
use vision::img::{self, Image};

pub use judge::judge;

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ThumbColors {
    pub green: f64,
    pub blue: f64,
    pub earth: f64,
    pub top5: f64,
    pub dead: f64,
}

pub fn measure(bgr: &Image) -> anyhow::Result<ThumbColors> {
    anyhow::ensure!(
        bgr.ch == 3 && bgr.w > 0 && bgr.h > 0,
        "expected a color image"
    );
    let gray = img::gray(bgr);
    let hsv = img::hsv(bgr);

    let pixels = cv::pixels(bgr);
    let hsv_pixels = cv::pixels(&hsv);
    let n = pixels.len() as f64;

    let (mut green, mut blue, mut earth) = (0u64, 0u64, 0u64);
    for p in hsv_pixels {
        let (h, s, v) = (p[0] as i32, p[1] as i32, p[2] as i32);
        if (30..=85).contains(&h) && s >= 40 && v >= 35 {
            green += 1;
        }
        if (90..=130).contains(&h) && s >= 35 && v >= 60 {
            blue += 1;
        }
        if (8..30).contains(&h) && (40..=200).contains(&s) && (30..=200).contains(&v) {
            earth += 1;
        }
    }

    Ok(ThumbColors {
        green: green as f64 / n,
        blue: blue as f64 / n,
        earth: earth as f64 / n,
        top5: cv::top5_share(pixels),
        dead: dead_flat(&gray),
    })
}

fn dead_flat(gray: &Image) -> f64 {
    let g = cv::plane(gray);
    let mean = filter::box_mean(&g, 5, 5);
    let sq = filter::box_mean(&filter::square(&g), 5, 5);

    let (mut smooth, mut dead) = (0u64, 0u64);
    for (&m, &s) in mean.data.iter().zip(&sq.data) {
        let std = (s - m * m).max(0.0).sqrt();
        if std < 4.0 && m > 12.0 && m < 243.0 {
            smooth += 1;
            if std < 0.5 {
                dead += 1;
            }
        }
    }
    if smooth >= 50 {
        dead as f64 / smooth as f64
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cv::testing::{fill, Rect};
    use crate::reason::Reason;

    fn solid(b: f64, g: f64, r: f64) -> Image {
        crate::cv::testing::solid(128, 128, (b, g, r))
    }

    fn noisy_green() -> Image {
        let mut img = solid(40.0, 140.0, 60.0);
        for (v, z) in img
            .data
            .iter_mut()
            .zip(crate::cv::testing::randn(128 * 128 * 3, 1))
        {
            let n = (z * 25.0 + 0.0).round_ties_even().clamp(0.0, 255.0) as i32;
            *v = (*v as i32 + n).min(255) as u8;
        }
        img
    }

    /// 一色で塗りつぶした画像は、上位5色と平らな塗りで全体を占めると測ること
    #[test]
    fn test_solid_image_measures_flat() {
        let f = measure(&solid(40.0, 140.0, 60.0)).unwrap();

        assert_eq!(f.top5, 1.0);
        assert_eq!(f.dead, 1.0);
        assert_eq!(f.green, 1.0);
        assert_eq!(judge(&f), Some(Reason::ThumbIllustration));
    }

    /// ざらつきのある緑の画像は、緑が多く、平らな塗りが無いと測ること
    #[test]
    fn test_noisy_green_image_measures_textured_and_green() {
        let f = measure(&noisy_green()).unwrap();

        assert!(f.green > 0.5, "{f:?}");
        assert_eq!(f.dead, 0.0);
        assert!(f.top5 < 0.4, "{f:?}");
    }

    /// 4色の帯だけでできた画像は、上位5色で全体を占めると測ること（1色だけを数えないこと）
    #[test]
    fn test_four_color_stripes_fill_top5() {
        let mut img = solid(0.0, 0.0, 0.0);
        let colors = [
            (0.0, 0.0, 200.0),
            (0.0, 200.0, 0.0),
            (200.0, 0.0, 0.0),
            (200.0, 200.0, 200.0),
        ];
        for (i, (b, g, r)) in colors.iter().enumerate() {
            let rect = Rect::new(0, i as i32 * 32, 128, 32);
            fill(&mut img, rect, (*b, *g, *r));
        }

        let f = measure(&img).unwrap();

        assert_eq!(f.top5, 1.0);
        assert_eq!(
            judge(&ThumbColors { dead: 0.0, ..f }),
            Some(Reason::ThumbFlatColors)
        );
    }

    /// 読み込めなかった（空の）画像は、止まらずに「測れない」として返すこと（判定に失敗した画像は載せないため）
    #[test]
    fn test_empty_image_is_an_error() {
        assert!(measure(&Image::new(0, 0, 3)).is_err());
    }
}
