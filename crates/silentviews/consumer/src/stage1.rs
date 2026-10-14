use vision::filter;
use vision::img::{self, Image};

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Stage1Features {
    pub s1_green: f64,
    pub s1_blue: f64,
    pub s1_earth: f64,
    pub s1_top5: f64,
    pub s1_dead: f64,
    pub s1_lap: f64,
    pub s1_sat_mean: f64,
    pub s1_sat_hi: f64,
    pub s1_vivid_hues: u32,
    pub s1_colorful: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage1Verdict {
    Pass,
    Blurry,
    Illustration,
    FlatColors,
    TooFewNatureColors,
    UnnaturalColors,
}

pub fn judge(f: &Stage1Features) -> Stage1Verdict {
    if f.s1_lap < 150.0 {
        Stage1Verdict::Blurry
    } else if f.s1_dead >= 0.05 {
        Stage1Verdict::Illustration
    } else if f.s1_top5 >= 0.40 {
        Stage1Verdict::FlatColors
    } else if f.s1_green + f.s1_blue + f.s1_earth < 0.25 {
        Stage1Verdict::TooFewNatureColors
    } else if f.s1_vivid_hues >= 4 || f.s1_colorful >= 90.0 || f.s1_sat_hi >= 0.2 {
        Stage1Verdict::UnnaturalColors
    } else {
        Stage1Verdict::Pass
    }
}

pub fn features(bgr: &Image) -> anyhow::Result<Stage1Features> {
    anyhow::ensure!(
        bgr.ch == 3 && bgr.w > 0 && bgr.h > 0,
        "expected a color image"
    );
    let gray = img::gray(bgr);
    let hsv = img::hsv(bgr);

    let pixels = crate::cv::pixels(bgr);
    let hsv_pixels = crate::cv::pixels(&hsv);
    let n = pixels.len() as f64;

    let (mut green, mut blue, mut earth, mut sat_hi, mut sat_sum) = (0u64, 0u64, 0u64, 0u64, 0u64);
    let mut vivid_hist = [0u64; 12];
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
        if s > 200 && v > 120 {
            sat_hi += 1;
        }
        if s > 120 && v > 90 {
            vivid_hist[(h / 15) as usize] += 1;
        }
        sat_sum += s as u64;
    }

    let mut color_counts = vec![0u32; 32 * 32 * 32];
    for p in pixels {
        let key = (p[0] as usize / 8) * 1024 + (p[1] as usize / 8) * 32 + (p[2] as usize / 8);
        color_counts[key] += 1;
    }
    color_counts.sort_unstable_by(|a, b| b.cmp(a));
    let top5 = color_counts.iter().take(5).map(|&c| c as u64).sum::<u64>() as f64 / n;

    Ok(Stage1Features {
        s1_green: green as f64 / n,
        s1_blue: blue as f64 / n,
        s1_earth: earth as f64 / n,
        s1_top5: top5,
        s1_dead: dead_flat(&gray),
        s1_lap: laplacian_variance(&gray),
        s1_sat_mean: sat_sum as f64 / n / 255.0,
        s1_sat_hi: sat_hi as f64 / n,
        s1_vivid_hues: vivid_hist.iter().filter(|&&c| c as f64 / n > 0.02).count() as u32,
        s1_colorful: colorfulness(pixels),
    })
}

fn dead_flat(gray: &Image) -> f64 {
    let g = crate::cv::plane(gray);
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

fn laplacian_variance(gray: &Image) -> f64 {
    crate::cv::laplacian_variance(gray)
}

fn colorfulness(pixels: &[[u8; 3]]) -> f64 {
    let n = pixels.len() as f64;
    let (mut rg_sum, mut yb_sum, mut rg_sq, mut yb_sq) = (0.0, 0.0, 0.0, 0.0);
    for p in pixels {
        let (b, g, r) = (p[0] as f64, p[1] as f64, p[2] as f64);
        let rg = r - g;
        let yb = 0.5 * (r + g) - b;
        rg_sum += rg;
        yb_sum += yb;
        rg_sq += rg * rg;
        yb_sq += yb * yb;
    }
    let (rg_mean, yb_mean) = (rg_sum / n, yb_sum / n);
    let rg_var = rg_sq / n - rg_mean * rg_mean;
    let yb_var = yb_sq / n - yb_mean * yb_mean;
    (rg_var + yb_var).sqrt() + 0.3 * (rg_mean * rg_mean + yb_mean * yb_mean).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cv::testing::{fill, Rect};

    fn passing() -> Stage1Features {
        Stage1Features {
            s1_green: 0.3,
            s1_blue: 0.1,
            s1_earth: 0.1,
            s1_top5: 0.1,
            s1_dead: 0.0,
            s1_lap: 500.0,
            s1_sat_mean: 0.3,
            s1_sat_hi: 0.0,
            s1_vivid_hues: 1,
            s1_colorful: 30.0,
        }
    }

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

    /// どの条件にも当てはまらない画像は通すこと
    #[test]
    fn test_passing_features_pass() {
        assert_eq!(judge(&passing()), Stage1Verdict::Pass);
    }

    /// 輪郭のくっきり度（輪郭がどれだけはっきりしているか）が 150 未満の画像は、ブレているとして弾くこと
    #[test]
    fn test_blurry_is_rejected() {
        let f = Stage1Features {
            s1_lap: 149.9,
            ..passing()
        };
        assert_eq!(judge(&f), Stage1Verdict::Blurry);
        let f = Stage1Features {
            s1_lap: 150.0,
            ..passing()
        };
        assert_eq!(judge(&f), Stage1Verdict::Pass);
    }

    /// なめらかな部分のうち、色の変化がまったく無い平らな塗りが 5% 以上なら、イラストとして弾くこと
    #[test]
    fn test_dead_flat_is_rejected_as_illustration() {
        let f = Stage1Features {
            s1_dead: 0.05,
            ..passing()
        };
        assert_eq!(judge(&f), Stage1Verdict::Illustration);
    }

    /// よく使われている上位5色で画面の 40% 以上を占めるなら、色数の少ないベタ塗りとして弾くこと
    #[test]
    fn test_few_colors_are_rejected_as_flat() {
        let f = Stage1Features {
            s1_top5: 0.40,
            ..passing()
        };
        assert_eq!(judge(&f), Stage1Verdict::FlatColors);
    }

    /// 緑・青・土の色が合わせて 25% 未満なら自然の色不足として弾くこと
    #[test]
    fn test_too_few_nature_colors_are_rejected() {
        let f = Stage1Features {
            s1_green: 0.1,
            s1_blue: 0.1,
            s1_earth: 0.049,
            ..passing()
        };
        assert_eq!(judge(&f), Stage1Verdict::TooFewNatureColors);
    }

    /// 鮮やかな色の種類が4つ以上・色の派手さが 90 以上・とても強い彩度の部分が 20% 以上のどれかなら、不自然な色として弾くこと
    #[test]
    fn test_unnatural_colors_are_rejected() {
        for f in [
            Stage1Features {
                s1_vivid_hues: 4,
                ..passing()
            },
            Stage1Features {
                s1_colorful: 90.0,
                ..passing()
            },
            Stage1Features {
                s1_sat_hi: 0.2,
                ..passing()
            },
        ] {
            assert_eq!(judge(&f), Stage1Verdict::UnnaturalColors, "{f:?}");
        }
    }

    /// 当てはまる条件が複数あるときは、試作と同じくブレ→イラスト→ベタ塗り→色不足→不自然な色の順で理由を決めること
    #[test]
    fn test_reasons_are_checked_in_trial_order() {
        let all_bad = Stage1Features {
            s1_lap: 0.0,
            s1_dead: 1.0,
            s1_top5: 1.0,
            s1_green: 0.0,
            s1_blue: 0.0,
            s1_earth: 0.0,
            s1_vivid_hues: 12,
            ..passing()
        };
        assert_eq!(judge(&all_bad), Stage1Verdict::Blurry);
        assert_eq!(
            judge(&Stage1Features {
                s1_lap: 500.0,
                ..all_bad.clone()
            }),
            Stage1Verdict::Illustration
        );
    }

    /// 一色で塗りつぶした画像は、輪郭が無く（くっきり度 0）、上位5色と平らな塗りで全体を占めると測ること
    #[test]
    fn test_solid_image_measures_flat() {
        let f = features(&solid(40.0, 140.0, 60.0)).unwrap();

        assert_eq!(f.s1_lap, 0.0);
        assert_eq!(f.s1_top5, 1.0);
        assert_eq!(f.s1_dead, 1.0);
        assert_eq!(f.s1_green, 1.0);
        assert_eq!(judge(&f), Stage1Verdict::Blurry);
    }

    /// ざらつきのある緑の画像は、くっきりしていて、緑が多く、平らな塗りが無いと測ること
    #[test]
    fn test_noisy_green_image_measures_textured_and_green() {
        let f = features(&noisy_green()).unwrap();

        assert!(f.s1_lap > 150.0, "{f:?}");
        assert!(f.s1_green > 0.5, "{f:?}");
        assert_eq!(f.s1_dead, 0.0);
        assert!(f.s1_top5 < 0.4, "{f:?}");
    }

    /// 赤・黄・緑・青・紫の鮮やかな帯が並ぶ画像は、鮮やかな色を4種類以上と数えること
    #[test]
    fn test_vivid_stripes_count_hues() {
        let mut img = solid(0.0, 0.0, 0.0);
        let colors = [
            (0.0, 0.0, 255.0),
            (0.0, 255.0, 255.0),
            (0.0, 255.0, 0.0),
            (255.0, 0.0, 0.0),
            (255.0, 0.0, 255.0),
        ];
        for (i, (b, g, r)) in colors.iter().enumerate() {
            let rect = Rect::new(0, i as i32 * 25, 128, 25);
            fill(&mut img, rect, (*b, *g, *r));
        }

        let f = features(&img).unwrap();

        assert!(f.s1_vivid_hues >= 4, "{f:?}");
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

        let f = features(&img).unwrap();

        assert_eq!(f.s1_top5, 1.0);
        assert_eq!(
            judge(&Stage1Features {
                s1_lap: 500.0,
                s1_dead: 0.0,
                ..f
            }),
            Stage1Verdict::FlatColors
        );
    }
}
