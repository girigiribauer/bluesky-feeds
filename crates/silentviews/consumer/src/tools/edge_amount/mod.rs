mod judge;

use crate::cv;
use vision::img::{self, Image};
use vision::lines;

pub use judge::judge;

pub fn measure(bgr: &Image) -> f64 {
    let gray = img::gray(&cv::fit(bgr, 256, false));
    let edges = lines::canny(&gray, 80.0, 200.0);
    let e = &edges.data;
    e.iter().map(|&v| v as f64).sum::<f64>() / e.len() as f64 / 255.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cv::testing::{fill, solid, Rect};

    /// 一色で塗りつぶした画像は、輪郭が無いと測ること
    #[test]
    fn test_solid_image_has_no_edges() {
        assert_eq!(measure(&solid(300, 200, (40.0, 140.0, 60.0))), 0.0);
    }

    /// 線を引いた画像は、輪郭があると測ること
    #[test]
    fn test_drawn_lines_have_edges() {
        let mut img = solid(256, 256, (0.0, 0.0, 0.0));
        fill(&mut img, Rect::new(20, 30, 200, 2), (255.0, 255.0, 255.0));

        assert!(measure(&img) > 0.0);
    }
}
