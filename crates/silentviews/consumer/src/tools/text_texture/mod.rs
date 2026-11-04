mod judge;

use crate::cv;
use vision::filter;
use vision::img::{self, Image};
use vision::shape::{self, Mode};

pub use judge::judge;

pub fn measure(bgr: &Image) -> f64 {
    let gray = img::gray(&cv::fit(bgr, 512, false));
    let grad = filter::morph_gradient(&gray, 3, 3);
    let otsu = filter::otsu(&grad);
    let bw = filter::threshold_binary(&grad, otsu.max(90.0));
    let closed_img = filter::close_rect(&bw, 9, 1);

    let (h, w) = (gray.h as i32, gray.w as i32);
    let mut area = 0i64;
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
            area += (rw * rh) as i64;
        }
    }
    area as f64 / (h * w) as f64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cv::testing::{fill, noise, solid, Rect};

    /// 細い横長の帯（文字の行のようなもの）が並んだ画像は、文字らしい塊として広さを数えること
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

        assert!(measure(&img) > 0.0);
    }

    /// 細かい模様だけの画像は、文字らしい塊を数えないこと
    #[test]
    fn test_texture_without_bars_has_no_text() {
        assert_eq!(measure(&noise(512, 384, (60.0, 120.0, 60.0), 20.0, 1)), 0.0);
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

        assert_eq!(measure(&img), 0.0);
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

        assert_eq!(measure(&img), 0.0);
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

        assert_eq!(measure(&img), 0.0);
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

        assert_eq!(measure(&img), 0.0);
    }
}
