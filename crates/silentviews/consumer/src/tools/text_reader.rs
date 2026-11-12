use std::path::Path;

use vision::filter;
use vision::img::Image;
use vision::ncnn::NcnnModel;
use vision::shape::{self, Pt};

const HEIGHT: usize = 48;
const MIN_WIDTH: usize = 320;
const MAX_WIDTH: usize = 3200;
const MIN_CONFIDENCE: f64 = 0.7;
const ROW_HEIGHT: usize = 32;
const MIN_LETTER_AREA: usize = 6;
const MIN_LETTER_HEIGHT: f64 = 0.3;
const MAX_LETTER_HEIGHT: f64 = 0.95;
const MIN_LETTERS: usize = 4;

pub struct TextReader {
    net: NcnnModel,
}

impl TextReader {
    pub fn load(models: &Path) -> anyhow::Result<Self> {
        let net = NcnnModel::load_named(models, "text_recognition_ppocrv4")?;
        Ok(Self { net })
    }

    pub fn text_like(
        &mut self,
        bgr: &Image,
        gray: &Image,
        boxes: Vec<Vec<Pt>>,
    ) -> anyhow::Result<Vec<Vec<Pt>>> {
        let mut kept = Vec::new();
        for b in boxes {
            if self.confidence(bgr, &b)? >= MIN_CONFIDENCE
                || letters_in_row(gray, &b) >= MIN_LETTERS
            {
                kept.push(b);
            }
        }
        Ok(kept)
    }

    pub fn confidence(&mut self, bgr: &Image, polygon: &[Pt]) -> anyhow::Result<f64> {
        let Some((blob, width)) = line_blob(bgr, polygon) else {
            return Ok(0.0);
        };
        let out = self.net.run(blob, [3, HEIGHT, width])?.swap_remove(0);
        let classes = *out.shape.last().unwrap_or(&0);
        anyhow::ensure!(classes > 1, "unexpected text recognition output");
        Ok(ctc_confidence(&out.data, classes))
    }
}

struct Line {
    cx: f64,
    cy: f64,
    ux: (f64, f64),
    uy: (f64, f64),
    long: f64,
    short: f64,
}

impl Line {
    fn of(polygon: &[Pt]) -> Option<Self> {
        let pts: Vec<(f32, f32)> = polygon.iter().map(|&(x, y)| (x as f32, y as f32)).collect();
        let r = shape::min_area_rect_f(&pts);
        let ang = (r.angle as f64).to_radians();
        let along_w = (ang.cos(), ang.sin());
        let along_h = (-ang.sin(), ang.cos());
        let (mut ux, mut long, mut short) = (along_w, r.w as f64, r.h as f64);
        if short > long {
            (ux, long, short) = (along_h, short, long);
        }
        if ux.0 < 0.0 || (ux.0 == 0.0 && ux.1 < 0.0) {
            ux = (-ux.0, -ux.1);
        }
        if long < 2.0 || short < 2.0 {
            return None;
        }
        let uy = (-ux.1, ux.0);
        Some(Self {
            cx: r.cx as f64,
            cy: r.cy as f64,
            ux,
            uy,
            long,
            short,
        })
    }

    fn sample<const C: usize>(
        &self,
        img: &Image,
        x: usize,
        y: usize,
        w: usize,
        h: usize,
    ) -> [f32; C] {
        let u = ((x as f64 + 0.5) / w as f64 - 0.5) * self.long;
        let v = ((y as f64 + 0.5) / h as f64 - 0.5) * self.short;
        let sx = self.cx + u * self.ux.0 + v * self.uy.0;
        let sy = self.cy + u * self.ux.1 + v * self.uy.1;
        bilinear(img, sx - 0.5, sy - 0.5)
    }
}

fn line_blob(bgr: &Image, polygon: &[Pt]) -> Option<(Vec<f32>, usize)> {
    let line = Line::of(polygon)?;
    let content = ((HEIGHT as f64 * line.long / line.short).ceil() as usize).clamp(1, MAX_WIDTH);
    let width = content.max(MIN_WIDTH);
    let mut blob = vec![0f32; 3 * HEIGHT * width];
    for y in 0..HEIGHT {
        for x in 0..content {
            let px: [f32; 3] = line.sample(bgr, x, y, content, HEIGHT);
            for c in 0..3 {
                blob[(c * HEIGHT + y) * width + x] = (px[c] - 127.5) / 127.5;
            }
        }
    }
    Some((blob, width))
}

fn letters_in_row(gray: &Image, polygon: &[Pt]) -> usize {
    let Some(line) = Line::of(polygon) else {
        return 0;
    };
    let h = ROW_HEIGHT;
    let w = ((h as f64 * line.long / line.short).round() as usize).clamp(8, MAX_WIDTH);
    let mut row = Image::new(w, h, 1);
    for y in 0..h {
        for x in 0..w {
            let [v]: [f32; 1] = line.sample(gray, x, y, w, h);
            row.data[y * w + x] = v.round().clamp(0.0, 255.0) as u8;
        }
    }
    let t = filter::otsu(&row).floor();
    let bright = row.data.iter().filter(|&&v| v as f64 > t).count();
    let half = row.data.len() as f64 * 127.0 / 255.0;
    let fg_bright = if (row.data.len() - bright) as f64 <= half {
        false
    } else if bright as f64 <= half {
        true
    } else {
        return 0;
    };
    let mask: Vec<u8> = row
        .data
        .iter()
        .map(|&v| u8::from((v as f64 > t) == fg_bright))
        .collect();
    let (labels, areas) = shape::connected_components(&mask, w, h, true);
    let mut span = vec![(usize::MAX, 0usize); areas.len()];
    for (i, &l) in labels.iter().enumerate() {
        if l > 0 {
            let s = &mut span[l as usize];
            s.0 = s.0.min(i / w);
            s.1 = s.1.max(i / w);
        }
    }
    (1..areas.len())
        .filter(|&c| {
            let (top, bottom) = span[c];
            let height = (bottom - top + 1) as f64;
            areas[c] >= MIN_LETTER_AREA
                && height >= MIN_LETTER_HEIGHT * h as f64
                && height <= MAX_LETTER_HEIGHT * h as f64
                && top > 0
                && bottom + 1 < h
        })
        .count()
}

fn bilinear<const C: usize>(img: &Image, x: f64, y: f64) -> [f32; C] {
    let x = x.clamp(0.0, (img.w - 1) as f64);
    let y = y.clamp(0.0, (img.h - 1) as f64);
    let (x0, y0) = (x.floor() as usize, y.floor() as usize);
    let (x1, y1) = ((x0 + 1).min(img.w - 1), (y0 + 1).min(img.h - 1));
    let (fx, fy) = ((x - x0 as f64) as f32, (y - y0 as f64) as f32);
    let at = |xx: usize, yy: usize, c: usize| img.data[(yy * img.w + xx) * C + c] as f32;
    let mut out = [0f32; C];
    for (c, v) in out.iter_mut().enumerate() {
        let top = at(x0, y0, c) * (1.0 - fx) + at(x1, y0, c) * fx;
        let bottom = at(x0, y1, c) * (1.0 - fx) + at(x1, y1, c) * fx;
        *v = top * (1.0 - fy) + bottom * fy;
    }
    out
}

fn ctc_confidence(probs: &[f32], classes: usize) -> f64 {
    let mut kept = Vec::new();
    let mut prev = 0;
    for step in probs.chunks_exact(classes) {
        let (best, p) =
            step.iter().enumerate().fold(
                (0, f32::MIN),
                |acc, (i, &p)| if p > acc.1 { (i, p) } else { acc },
            );
        if best != 0 && best != prev {
            kept.push(p as f64);
        }
        prev = best;
    }
    if kept.is_empty() {
        0.0
    } else {
        kept.iter().sum::<f64>() / kept.len() as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cv::testing::{fill, gray_of, models, noise, png, solid, text_image, Rect};
    use crate::tools::text_detector::TextDetector;

    fn step(classes: usize, best: usize, p: f32) -> Vec<f32> {
        let mut v = vec![(1.0 - p) / (classes - 1) as f32; classes];
        v[best] = p;
        v
    }

    /// 読み取りの自信は、空白（0番）と、同じ字が続いた2つ目以降を除いた字の確からしさの平均にすること
    #[test]
    fn test_ctc_confidence_skips_blanks_and_repeats() {
        let probs: Vec<f32> = [
            step(5, 1, 0.9),
            step(5, 1, 0.5),
            step(5, 0, 0.99),
            step(5, 1, 0.7),
            step(5, 3, 0.8),
        ]
        .concat();

        assert!((ctc_confidence(&probs, 5) - 0.8).abs() < 1e-6);
    }

    /// 1字も読めなかったときは、自信を0にすること
    #[test]
    fn test_ctc_confidence_is_zero_without_letters() {
        let probs: Vec<f32> = [step(5, 0, 0.9), step(5, 0, 0.6)].concat();

        assert_eq!(ctc_confidence(&probs, 5), 0.0);
    }

    fn boxes(img: &Image) -> Vec<Vec<Pt>> {
        TextDetector::load(models()).unwrap().polygons(img).unwrap()
    }

    /// 横書き・傾き・縦向き・色つき・うすい文字の行は、どれも文字として残すこと
    #[test]
    fn test_real_text_lines_are_kept() {
        let mut reader = TextReader::load(models()).unwrap();
        for (img, lines) in [
            (text_image(0.0), 4),
            (text_image(10.0), 4),
            (text_image(30.0), 12),
            (text_image(90.0), 4),
            (png("text_red_on_blue.png"), 4),
            (png("text_faint.png"), 4),
        ] {
            let kept = reader.text_like(&img, &gray_of(&img), boxes(&img)).unwrap();
            assert_eq!(kept.len(), lines);
        }
    }

    /// 文字の無い模様を切り出しても、読めないので自信が低いこと
    #[test]
    fn test_texture_is_not_readable() {
        let img = noise(800, 600, (60.0, 120.0, 60.0), 20.0, 15);
        let whole = vec![(100, 250), (700, 250), (700, 330), (100, 330)];

        let c = TextReader::load(models())
            .unwrap()
            .confidence(&img, &whole)
            .unwrap();

        assert!(c < MIN_CONFIDENCE, "{c}");
    }

    /// 幅か高さが2px未満の枠は、読まずに自信0とすること
    #[test]
    fn test_too_thin_box_is_not_read() {
        let img = noise(100, 100, (60.0, 120.0, 60.0), 20.0, 15);
        let thin = vec![(10, 10), (90, 10), (90, 11), (10, 11)];

        assert!(line_blob(&img, &thin).is_none());
    }

    fn letter_row(letters: i32) -> (Image, Vec<Pt>) {
        let mut img = solid(400, 100, (230.0, 230.0, 230.0));
        for i in 0..letters {
            fill(
                &mut img,
                Rect::new(30 + i * 40, 30, 20, 40),
                (30.0, 30.0, 30.0),
            );
        }
        (img, vec![(10, 20), (390, 20), (390, 80), (10, 80)])
    }

    /// 読めない字でも、枠の上下に触れない字の大きさの塊が並んでいれば、その数を数えること
    #[test]
    fn test_letters_in_row_counts_separate_blobs() {
        let (img, row) = letter_row(6);

        assert_eq!(letters_in_row(&gray_of(&img), &row), 6);
    }

    /// 地平線のように枠の上下をまたぐ明暗の境目は、字として数えないこと
    #[test]
    fn test_letters_in_row_ignores_band_across_box() {
        let mut img = solid(400, 100, (230.0, 230.0, 230.0));
        fill(&mut img, Rect::new(0, 50, 400, 50), (30.0, 30.0, 30.0));
        let row = vec![(10, 20), (390, 20), (390, 80), (10, 80)];

        assert_eq!(letters_in_row(&gray_of(&img), &row), 0);
    }

    /// 読めなくても字の塊が4つ以上並ぶ枠は文字として残し、3つ以下なら外すこと
    #[test]
    fn test_unreadable_row_of_letters_is_kept() {
        let mut reader = TextReader::load(models()).unwrap();
        for (letters, kept) in [(4, 1), (3, 0)] {
            let (img, row) = letter_row(letters);

            let found = reader.text_like(&img, &gray_of(&img), vec![row]).unwrap();

            assert_eq!(found.len(), kept);
        }
    }

    /// モデルのファイルが見つからないときは、読み込みを失敗にすること（読めないまま文字を通してしまわないため）
    #[test]
    fn test_missing_model_fails_to_load() {
        assert!(TextReader::load(Path::new("/nonexistent")).is_err());
    }
}
