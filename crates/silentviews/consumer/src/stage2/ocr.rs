use std::path::Path;

use vision::img::{self, Image, Interp};
use vision::ncnn::NcnnModel;
use vision::shape::{self, Mode, Pt, RotRect};

const SIDE: i32 = 736;
const BINARY_THRESHOLD: f64 = 0.3;
const POLYGON_THRESHOLD: f64 = 0.5;
const MAX_CANDIDATES: usize = 200;
const UNCLIP_RATIO: f64 = 2.0;

#[derive(Debug, Clone, PartialEq)]
pub struct Ocr {
    pub ocr_n: u32,
    pub ocr_area: f64,
    pub ocr_lines: u32,
    pub ocr_edge: u32,
}

const EDGE_BAND: f64 = 0.12;

fn in_edge_band(top: i32, bottom: i32, h: usize) -> bool {
    let (top, bottom, h) = (top as f64, bottom as f64, h as f64);
    bottom <= h * EDGE_BAND || top >= h * (1.0 - EDGE_BAND)
}

pub struct TextDetector {
    net: NcnnModel,
}

impl TextDetector {
    pub fn load(models: &Path) -> anyhow::Result<Self> {
        let net = NcnnModel::load_named(models, "text_detection_ppocrv3_2023may")?;
        Ok(Self { net })
    }

    pub fn polygons(&mut self, bgr: &Image) -> anyhow::Result<Vec<Vec<Pt>>> {
        anyhow::ensure!(bgr.w > 0 && bgr.h > 0, "empty image");
        let resized = img::resize(bgr, SIDE as usize, SIDE as usize, Interp::Linear);
        let mean = [122.67891434f64, 116.66876762, 104.00698793].map(|v| v as f32);
        let blob = img::blob(&resized, true, mean, (1.0f64 / 255.0) as f32);
        drop(resized);
        let out = self
            .net
            .run(blob, [3, SIDE as usize, SIDE as usize])?
            .swap_remove(0);
        let (oh, ow) = match out.shape[..] {
            [.., h, w] => (h, w),
            _ => anyhow::bail!("unexpected text detection output"),
        };
        let binary = &out.data;
        let threshold = BINARY_THRESHOLD as f32;
        let bitmap = Image {
            w: ow,
            h: oh,
            ch: 1,
            data: binary
                .iter()
                .map(|&v| if v > threshold { 255 } else { 0 })
                .collect(),
        };
        let scale_h = bgr.h as f32 / oh as f32;
        let scale_w = bgr.w as f32 / ow as f32;

        let mut results = Vec::new();
        for contour in shape::find_contours(&bitmap, Mode::List)
            .iter()
            .take(MAX_CANDIDATES)
        {
            if contour_score(binary, ow, oh, contour) < POLYGON_THRESHOLD {
                continue;
            }
            let scaled: Vec<Pt> = contour
                .iter()
                .map(|&(x, y)| ((x as f32 * scale_w) as i32, (y as f32 * scale_h) as i32))
                .collect();
            let mut b = shape::min_area_rect(&scaled);
            let min_len = (b.h / scale_w).min(b.w / scale_h);
            if min_len < 3.0 {
                continue;
            }
            if b.w < b.h || b.angle.abs() >= 60.0 {
                std::mem::swap(&mut b.w, &mut b.h);
                if b.angle < 0.0 {
                    b.angle += 90.0;
                } else if b.angle > 0.0 {
                    b.angle -= 90.0;
                }
            }
            let polygon = unclip(&shape::corners(&b));
            if polygon.is_empty() {
                continue;
            }
            results.push(polygon.into_iter().map(round_point).collect());
        }
        Ok(results)
    }

    pub fn detect(&mut self, bgr: &Image) -> anyhow::Result<Ocr> {
        let (h, w) = (bgr.h, bgr.w);
        let (mut n, mut area, mut lines, mut edge) = (0u32, 0f64, 0u32, 0u32);
        for b in self.polygons(bgr)? {
            let pts: Vec<(f32, f32)> = b.iter().map(|&(x, y)| (x as f32, y as f32)).collect();
            let rect: RotRect = shape::min_area_rect_f(&pts);
            let (rw, rh) = (rect.w as f64, rect.h as f64);
            if rw.min(rh) < 3.0 {
                continue;
            }
            n += 1;
            area += rw * rh;
            let top = b.iter().map(|p| p.1).min().unwrap_or(0);
            let bottom = b.iter().map(|p| p.1).max().unwrap_or(0);
            if in_edge_band(top, bottom, h) {
                edge += 1;
            }
            let (long, short) = (rw.max(rh), rw.min(rh));
            let ang = rect.angle as f64;
            let tilt = if rw >= rh { ang } else { ang - 90.0 };
            let tilt = ((tilt + 90.0).rem_euclid(180.0) - 90.0).abs();
            if long >= 2.5 * short && tilt <= 15.0 {
                lines += 1;
            }
        }
        Ok(Ocr {
            ocr_n: n,
            ocr_area: area / (w * h) as f64,
            ocr_lines: lines,
            ocr_edge: edge,
        })
    }
}

fn contour_score(binary: &[f32], w: usize, h: usize, contour: &[Pt]) -> f64 {
    let (rx, ry, rw, rh) = shape::bounding_rect(contour);
    let xmin = rx.max(0);
    let xmax = (rx + rw).min(w as i32 - 1);
    let ymin = ry.max(0);
    let ymax = (ry + rh).min(h as i32 - 1);
    let (mw, mh) = ((xmax - xmin + 1) as usize, (ymax - ymin + 1) as usize);
    let mut mask = Image::new(mw, mh, 1);
    let shifted: Vec<Pt> = contour.iter().map(|&(x, y)| (x - xmin, y - ymin)).collect();
    shape::fill_poly(&mut mask, &shifted, 1);
    let (mut sum, mut count) = (0f64, 0usize);
    for y in 0..mh {
        for x in 0..mw {
            if mask.data[y * mw + x] != 0 {
                sum += binary[(y + ymin as usize) * w + x + xmin as usize] as f64;
                count += 1;
            }
        }
    }
    if count == 0 {
        0.0
    } else {
        sum / count as f64
    }
}

fn round_point(p: (f32, f32)) -> Pt {
    (p.0.round_ties_even() as i32, p.1.round_ties_even() as i32)
}

fn norm(x: f32, y: f32) -> f64 {
    ((x as f64) * (x as f64) + (y as f64) * (y as f64)).sqrt()
}

fn unclip(poly: &[(f32, f32); 4]) -> Vec<(f32, f32)> {
    let area = shape::contour_area(poly);
    let length = shape::arc_length_closed(poly);
    if length == 0.0 {
        return Vec::new();
    }
    let distance = area * UNCLIP_RATIO / length;
    let n = poly.len();
    let lines: Vec<((f32, f32), (f32, f32))> = (0..n)
        .map(|i| {
            let p1 = round_point(poly[i]);
            let p2 = round_point(poly[(i + n - 1) % n]);
            let (vx, vy) = (p1.0 - p2.0, p1.1 - p2.1);
            let dis = (distance / norm(vx as f32, vy as f32)) as f32;
            let (rx, ry) = (vy as f32 * dis, -(vx as f32) * dis);
            (
                (p1.0 as f32 + rx, p1.1 as f32 + ry),
                (p2.0 as f32 + rx, p2.1 as f32 + ry),
            )
        })
        .collect();
    (0..n)
        .map(|i| {
            let (a, b) = lines[i];
            let (c, d) = lines[(i + 1) % n];
            let (v1x, v1y) = (b.0 - a.0, b.1 - a.1);
            let (v2x, v2y) = (d.0 - c.0, d.1 - c.1);
            let cos = (v1x * v2x + v1y * v2y) as f64 / (norm(v1x, v1y) * norm(v2x, v2y));
            if cos.abs() > 0.7 {
                (
                    ((b.0 + c.0) as f64 * 0.5) as f32,
                    ((b.1 + c.1) as f64 * 0.5) as f32,
                )
            } else {
                let (ax, ay, bx) = (a.0 as f64, a.1 as f64, b.0 as f64);
                let (cx, dx) = (c.0 as f64, d.0 as f64);
                let denom = ax * (d.1 - c.1) as f64
                    + bx * (c.1 - d.1) as f64
                    + dx * (b.1 - a.1) as f64
                    + cx * (a.1 - b.1) as f64;
                let num =
                    ax * (d.1 - c.1) as f64 + cx * (a.1 - d.1) as f64 + dx * (c.1 - a.1) as f64;
                let s = num / denom;
                (
                    (ax + s * (b.0 - a.0) as f64) as f32,
                    (ay + s * (b.1 - a.1) as f64) as f32,
                )
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cv::testing::{fixture, models, noise, png, text_image};

    fn detect(img: &Image) -> Ocr {
        TextDetector::load(models()).unwrap().detect(img).unwrap()
    }

    /// 文字の塊が画面の上12%か下12%の帯にまるごと収まるときだけ「端の文字」とすること（写真に後から重ねた見出しや署名を拾い、景色の中で文字と見誤った模様は拾わないため）
    #[test]
    fn test_edge_band() {
        assert!(in_edge_band(3, 12, 100));
        assert!(in_edge_band(88, 97, 100));
        assert!(!in_edge_band(3, 13, 100));
        assert!(!in_edge_band(87, 97, 100));
        assert!(!in_edge_band(40, 60, 100));
    }

    /// 画面の真ん中あたりに並んだ文字は、端の文字と数えないこと
    #[test]
    fn test_middle_text_is_not_edge_text() {
        assert_eq!(detect(&text_image(0.0)).ocr_edge, 0);
    }

    /// 横書きの文字が4行ある画像では、文字の塊を4つ、横長の列を4つと数えること
    #[test]
    fn test_horizontal_text_lines() {
        let o = detect(&text_image(0.0));

        assert_eq!((o.ocr_n, o.ocr_lines), (4, 4));
        assert!(o.ocr_area > 0.1, "{}", o.ocr_area);
    }

    /// 10度傾いた文字の行は、傾きの基準（15度）以内なので横長の列に数えること
    #[test]
    fn test_slightly_tilted_text_counts_as_lines() {
        assert_eq!(detect(&text_image(10.0)).ocr_lines, 4);
    }

    /// 30度傾いた文字は、文字の塊としては数えても、横長の列には数えないこと
    #[test]
    fn test_steeply_tilted_text_is_not_lines() {
        let o = detect(&text_image(30.0));

        assert!(o.ocr_n > 0);
        assert_eq!(o.ocr_lines, 0);
    }

    /// 縦向き（90度）の文字は、文字の塊としては数えても、横長の列には数えないこと
    #[test]
    fn test_vertical_text_is_not_lines() {
        let o = detect(&text_image(90.0));

        assert_eq!((o.ocr_n, o.ocr_lines), (4, 0));
    }

    /// 文字の無い画像では、文字の塊を数えないこと
    #[test]
    fn test_no_text_in_texture() {
        let o = detect(&noise(800, 600, (60.0, 120.0, 60.0), 20.0, 15));

        assert_eq!((o.ocr_n, o.ocr_lines, o.ocr_area), (0, 0, 0.0));
    }

    /// 人物写真の中の小さな文字（服の記章など）も文字の塊として数えること
    #[test]
    fn test_small_text_in_photo() {
        let o = detect(&fixture("frontal_face.jpg"));

        assert_eq!((o.ocr_n, o.ocr_lines), (3, 1));
    }

    /// 色の付いた文字や、うすい文字も見つけ、その広さが記録した値から変わらないこと（設定の変更や OpenCV の更新に気づくため）
    #[test]
    fn test_colored_and_faint_text_areas_stay_the_same() {
        let cases = [
            ("text_red_on_blue.png", 0.153305),
            ("text_faint.png", 0.147783),
        ];
        for (name, area) in cases {
            let o = detect(&png(name));
            assert_eq!((o.ocr_n, o.ocr_lines), (4, 4));
            assert!((o.ocr_area - area).abs() < 1e-5, "{}", o.ocr_area);
        }
        let o = detect(&fixture("frontal_face.jpg"));
        assert!((o.ocr_area - 0.026406).abs() < 1e-5, "{}", o.ocr_area);
    }

    /// 幅か高さが 3px 未満のごく小さな塊（小さな記号など）は、文字の塊に数えないこと
    #[test]
    fn test_tiny_specks_are_not_counted() {
        assert_eq!(detect(&png("text_speck.png")).ocr_n, 0);
    }

    /// モデルのファイルが見つからないときは、読み込みを失敗にすること（検出せずに通してしまわないため）
    #[test]
    fn test_missing_model_fails_to_load() {
        assert!(TextDetector::load(Path::new("/nonexistent")).is_err());
    }
}
