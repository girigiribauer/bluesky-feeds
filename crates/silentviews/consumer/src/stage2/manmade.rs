use crate::cv;
use vision::img::{self, Image};

pub struct Manmade {
    pub seg_hv: u32,
    pub seg_v: u32,
    pub corners: u32,
    pub vivid_hues: u32,
    pub colorfulness: f64,
}

pub fn measure(bgr: &Image) -> Manmade {
    let small = cv::fit(bgr, 384, false);
    let gray = img::gray(&small);
    let (h, w) = (gray.h as f64, gray.w as f64);

    let segs = vision::lsd::detect(&gray);
    let min_len = h.min(w) * 0.06;
    let (mut hs, mut vs) = (Vec::new(), Vec::new());
    for s in segs.iter() {
        let (x1, y1, x2, y2) = (s[0], s[1], s[2], s[3]);
        if ((x2 - x1).hypot(y2 - y1) as f64) < min_len {
            continue;
        }
        let ang = ((y2 - y1).atan2(x2 - x1) as f64).to_degrees().abs() % 180.0;
        let ang = ang.min(180.0 - ang);
        if ang < 6.0 {
            hs.push(s);
        } else if ang > 84.0 {
            vs.push(s);
        }
    }

    let tol = 3.0f64.max(h.min(w) * 0.015);
    let mut corners = 0;
    for a in &hs {
        for b in &vs {
            for (ax, ay) in [(a[0], a[1]), (a[2], a[3])] {
                for (bx, by) in [(b[0], b[1]), (b[2], b[3])] {
                    if ((ax - bx).abs() as f64) < tol && ((ay - by).abs() as f64) < tol {
                        corners += 1;
                    }
                }
            }
        }
    }

    Manmade {
        seg_hv: (hs.len() + vs.len()) as u32,
        seg_v: vs.len() as u32,
        corners,
        vivid_hues: cv::vivid_hues(cv::pixels(&img::hsv(&small))),
        colorfulness: cv::colorfulness(cv::pixels(&small)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cv::testing::{fill, noise, solid, Rect};

    /// 窓が並んだ建物のような四角の格子は、直角と縦の線を数えること
    #[test]
    fn test_grid_of_boxes_has_corners_and_vertical_lines() {
        let mut img = solid(384, 384, (200.0, 200.0, 200.0));
        for r in 0..4 {
            for c in 0..4 {
                fill(
                    &mut img,
                    Rect::new(30 + c * 85, 30 + r * 85, 50, 50),
                    (40.0, 40.0, 40.0),
                );
            }
        }

        let m = measure(&img);

        assert!(m.corners >= 5, "corners={}", m.corners);
        assert!(m.seg_v >= 8, "seg_v={}", m.seg_v);
        assert!(m.seg_hv > m.seg_v);
    }

    /// 細かい模様だけの画像は、直角を数えないこと
    #[test]
    fn test_texture_has_no_corners() {
        let m = measure(&noise(384, 384, (60.0, 120.0, 60.0), 20.0, 8));

        assert_eq!(m.corners, 0);
    }

    /// 赤・黄・緑・青・紫の帯が並ぶ画像は、鮮やかな色を5種類と数え、派手さも高いこと
    #[test]
    fn test_vivid_stripes() {
        let mut img = solid(384, 380, (0.0, 0.0, 0.0));
        let colors = [
            (0.0, 0.0, 255.0),
            (0.0, 255.0, 255.0),
            (0.0, 255.0, 0.0),
            (255.0, 0.0, 0.0),
            (255.0, 0.0, 255.0),
        ];
        for (i, c) in colors.iter().enumerate() {
            fill(&mut img, Rect::new(0, i as i32 * 76, 384, 76), *c);
        }

        let m = measure(&img);

        assert_eq!(m.vivid_hues, 5);
        assert!(m.colorfulness > 90.0);
    }

    /// 赤一色の画像は、色のばらつきが無くても赤みの強さの分だけ派手さを測ること
    #[test]
    fn test_colorfulness_of_solid_red_comes_from_mean_term() {
        let m = measure(&solid(200, 200, (0.0, 0.0, 255.0)));

        assert!((m.colorfulness - 85.53).abs() < 0.01, "{}", m.colorfulness);
    }
}
