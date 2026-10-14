use crate::cv;
use vision::filter;
use vision::img::{self, Image};
use vision::lines;
use vision::shape::{self, Mode};

pub struct Basic {
    pub blur: f64,
    pub flat: f64,
    pub text_lines: u32,
    pub text_area: f64,
    pub horiz: u32,
    pub vert: u32,
    pub diag: u32,
    pub edge: f64,
    pub green: f64,
    pub blue: f64,
    pub earth: f64,
    pub sat_mean: f64,
    pub sat_hi: f64,
}

pub fn measure(bgr: &Image) -> Basic {
    let small = cv::fit(bgr, 256, false);
    let mid = cv::fit(bgr, 512, false);
    let gray = img::gray(&small);
    let (text_lines, text_area) = text_area(&mid);
    let (horiz, vert, diag, edge) = straight_lines(&gray);

    let hsv_img = img::hsv(&small);
    let hsv = cv::pixels(&hsv_img);
    let n = hsv.len() as f64;
    let (mut green, mut blue, mut earth, mut sat_hi, mut sat_sum) = (0u64, 0u64, 0u64, 0u64, 0u64);
    for p in hsv {
        let (h, s, v) = (p[0] as i32, p[1] as i32, p[2] as i32);
        green += ((30..=85).contains(&h) && s >= 40 && v >= 35) as u64;
        blue += ((90..=130).contains(&h) && s >= 35 && v >= 60) as u64;
        earth +=
            ((8..30).contains(&h) && (40..=200).contains(&s) && (30..=200).contains(&v)) as u64;
        sat_hi += (s > 200 && v > 120) as u64;
        sat_sum += s as u64;
    }

    Basic {
        blur: cv::laplacian_variance(&gray),
        flat: cv::top5_share(cv::pixels(&small)),
        text_lines,
        text_area,
        horiz,
        vert,
        diag,
        edge,
        green: green as f64 / n,
        blue: blue as f64 / n,
        earth: earth as f64 / n,
        sat_mean: sat_sum as f64 / n / 255.0,
        sat_hi: sat_hi as f64 / n,
    }
}

fn text_area(bgr: &Image) -> (u32, f64) {
    let gray = img::gray(bgr);
    let grad = filter::morph_gradient(&gray, 3, 3);
    let otsu = filter::otsu(&grad);
    let bw = filter::threshold_binary(&grad, otsu.max(90.0));
    let closed_img = filter::close_rect(&bw, 9, 1);

    let (h, w) = (gray.h as i32, gray.w as i32);
    let (mut lines, mut area) = (0u32, 0i64);
    for c in shape::find_contours(&closed_img, Mode::External) {
        let (rx, ry, rw, rh) = shape::bounding_rect(&c);
        if !((5..=80).contains(&rh) && rw as f64 >= 2.5 * rh as f64 && rw >= 20) {
            continue;
        }
        let filled = (ry..ry + rh)
            .flat_map(|y| (rx..rx + rw).map(move |x| (x, y)))
            .filter(|&(x, y)| closed_img.data[(y * w + x) as usize] != 0)
            .count();
        if filled as f64 / (rw * rh) as f64 >= 0.45 {
            lines += 1;
            area += (rw * rh) as i64;
        }
    }
    (lines, area as f64 / (h * w) as f64)
}

fn straight_lines(gray: &Image) -> (u32, u32, u32, f64) {
    let edges = lines::canny(gray, 80.0, 200.0);
    let min_len = (gray.h.min(gray.w) as f64 * 0.25) as i32;
    let segs = lines::hough_lines_p(
        &edges,
        1.0,
        std::f64::consts::PI / 180.0,
        40,
        min_len as f64,
        2.0,
    );
    let (mut horiz, mut vert, mut diag) = (0, 0, 0);
    for s in segs.iter() {
        let ang = ((s[3] - s[1]) as f64)
            .atan2((s[2] - s[0]) as f64)
            .to_degrees()
            .abs()
            % 180.0;
        let ang = ang.min(180.0 - ang);
        if ang < 8.0 {
            horiz += 1;
        } else if ang > 82.0 {
            vert += 1;
        } else {
            diag += 1;
        }
    }
    let e = &edges.data;
    let edge = e.iter().map(|&v| v as f64).sum::<f64>() / e.len() as f64 / 255.0;
    (horiz, vert, diag, edge)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cv::testing::{fill, noise, solid, Rect};

    /// 細い横長の帯（文字の行のようなもの）が並んだ画像は、文字らしい塊として数えること
    #[test]
    fn test_thin_horizontal_bars_count_as_text() {
        let mut img = solid(512, 384, (30.0, 30.0, 30.0));
        for i in 0..5 {
            fill(
                &mut img,
                Rect::new(60, 60 + i * 50, 200, 3),
                (255.0, 255.0, 255.0),
            );
        }

        let b = measure(&img);

        assert_eq!(b.text_lines, 5);
        assert!(b.text_area > 0.0);
    }

    /// 細かい模様だけの画像は、文字らしい塊を数えないこと
    #[test]
    fn test_texture_without_bars_has_no_text() {
        let b = measure(&noise(512, 384, (60.0, 120.0, 60.0), 20.0, 1));

        assert_eq!(b.text_lines, 0);
        assert_eq!(b.text_area, 0.0);
    }

    /// 縦の長い直線は縦、横の長い直線は横として数えること
    #[test]
    fn test_long_lines_are_counted_by_direction() {
        let mut img = solid(256, 256, (0.0, 0.0, 0.0));
        for i in 0..3 {
            fill(
                &mut img,
                Rect::new(30 + i * 60, 20, 2, 200),
                (255.0, 255.0, 255.0),
            );
        }
        let b = measure(&img);
        assert!(
            b.vert >= 3 && b.horiz == 0 && b.diag == 0,
            "vert={} horiz={} diag={}",
            b.vert,
            b.horiz,
            b.diag
        );

        let mut img = solid(256, 256, (0.0, 0.0, 0.0));
        for i in 0..3 {
            fill(
                &mut img,
                Rect::new(20, 30 + i * 60, 200, 2),
                (255.0, 255.0, 255.0),
            );
        }
        let b = measure(&img);
        assert!(
            b.horiz >= 3 && b.vert == 0 && b.diag == 0,
            "vert={} horiz={} diag={}",
            b.vert,
            b.horiz,
            b.diag
        );
        assert!(b.edge > 0.0);
    }

    /// 緑一色の画像は、緑が全体で、輪郭が無く（くっきり度 0）、上位5色で全体を占めると測ること
    #[test]
    fn test_solid_green_measures() {
        let b = measure(&solid(300, 200, (40.0, 140.0, 60.0)));

        assert_eq!(b.green, 1.0);
        assert_eq!(b.blue, 0.0);
        assert_eq!(b.blur, 0.0);
        assert_eq!(b.flat, 1.0);
        assert_eq!(b.edge, 0.0);
    }

    /// 空色一色は青、茶色一色は土の色、鮮やかな赤一色は強い彩度として測ること
    #[test]
    fn test_color_groups() {
        assert_eq!(measure(&solid(100, 100, (200.0, 140.0, 60.0))).blue, 1.0);
        assert_eq!(measure(&solid(100, 100, (40.0, 80.0, 120.0))).earth, 1.0);
        let red = measure(&solid(100, 100, (0.0, 0.0, 255.0)));
        assert_eq!(red.sat_hi, 1.0);
        assert_eq!(red.sat_mean, 1.0);
    }

    /// 高さが 5px に満たないごく細い線は、文字に数えないこと
    #[test]
    fn test_hairlines_are_not_text() {
        let mut img = solid(512, 384, (30.0, 30.0, 30.0));
        for i in 0..5 {
            fill(
                &mut img,
                Rect::new(60, 60 + i * 50, 200, 1),
                (255.0, 255.0, 255.0),
            );
        }

        assert_eq!(measure(&img).text_lines, 0);
    }

    /// 幅も高さもある四角い塊は、横長ではないので文字の行とみなさないこと
    #[test]
    fn test_square_blocks_are_not_text() {
        let mut img = solid(512, 384, (30.0, 30.0, 30.0));
        for block in 0..4 {
            for line in 0..15 {
                let rect = Rect::new(40 + block * 110, 100 + line * 2, 30, 1);
                fill(&mut img, rect, (255.0, 255.0, 255.0));
            }
        }

        assert_eq!(measure(&img).text_lines, 0);
    }

    /// 中が空いた横長の枠は、埋まっていないので文字に数えないこと
    #[test]
    fn test_hollow_frame_is_not_text() {
        let mut img = solid(512, 384, (30.0, 30.0, 30.0));
        for r in [
            Rect::new(60, 100, 300, 1),
            Rect::new(60, 139, 300, 1),
            Rect::new(60, 100, 1, 40),
            Rect::new(359, 100, 1, 40),
        ] {
            fill(&mut img, r, (255.0, 255.0, 255.0));
        }

        assert_eq!(measure(&img).text_lines, 0);
    }

    /// 背景との明るさの差が小さい帯は、文字に数えないこと
    #[test]
    fn test_low_contrast_bars_are_not_text() {
        let mut img = solid(512, 384, (100.0, 100.0, 100.0));
        for i in 0..5 {
            fill(
                &mut img,
                Rect::new(60, 60 + i * 50, 200, 3),
                (130.0, 130.0, 130.0),
            );
        }

        assert_eq!(measure(&img).text_lines, 0);
    }
}
