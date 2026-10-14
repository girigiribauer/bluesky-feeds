use std::path::Path;

use vision::img::{self, Image, Interp};
use vision::ncnn::NcnnModel;
use vision::ncnn::Output;
use vision::nms::nms;

const YUNET_STRIDES: [usize; 3] = [8, 16, 32];
const YUNET_DIVISOR: usize = 32;
const YUNET_SCORE: f32 = 0.6;
const YUNET_NMS: f32 = 0.3;
const YUNET_TOP_K: usize = 5000;

#[derive(Debug, Clone, PartialEq)]
pub struct Faces {
    pub yn_count: u32,
    pub yn_max_area: f64,
}

pub struct FaceDetectors {
    yunet: NcnnModel,
}

impl FaceDetectors {
    pub fn load(models: &Path) -> anyhow::Result<Self> {
        let yunet = NcnnModel::load_named(models, "face_detection_yunet_2023mar")?;
        Ok(Self { yunet })
    }

    pub fn measure(&mut self, bgr: &Image) -> anyhow::Result<Faces> {
        let (yn_count, yn_max_area) = self.yunet(bgr)?;
        Ok(Faces {
            yn_count,
            yn_max_area,
        })
    }

    fn yunet(&mut self, bgr: &Image) -> anyhow::Result<(u32, f64)> {
        anyhow::ensure!(
            bgr.ch == 3 && bgr.w > 0 && bgr.h > 0,
            "expected a color image"
        );
        let (h, w) = (bgr.h, bgr.w);
        let s = (640.0 / h.max(w) as f64).min(1.0);
        let iw = ((w as f64 * s) as usize).max(1);
        let ih = ((h as f64 * s) as usize).max(1);
        let small = img::resize(bgr, iw, ih, Interp::Linear);
        let pad_w = (iw - 1) / YUNET_DIVISOR * YUNET_DIVISOR + YUNET_DIVISOR;
        let pad_h = (ih - 1) / YUNET_DIVISOR * YUNET_DIVISOR + YUNET_DIVISOR;

        let pixels = crate::cv::pixels(&small);
        let mut blob = vec![0f32; 3 * pad_h * pad_w];
        for y in 0..ih {
            for x in 0..iw {
                let p = pixels[y * iw + x];
                for c in 0..3 {
                    blob[(c * pad_h + y) * pad_w + x] = p[c] as f32;
                }
            }
        }

        let outs = self.yunet.run(blob, [3, pad_h, pad_w])?;
        anyhow::ensure!(outs.len() == 12, "unexpected yunet outputs");

        let (boxes, scores) = yunet_decode(&outs, pad_h, pad_w);
        let keep = if boxes.len() > 1 {
            let rects: Vec<[f64; 4]> = boxes
                .iter()
                .map(|b| b.map(|v| v.trunc() as i32 as f64))
                .collect();
            nms(&rects, &scores, YUNET_SCORE, YUNET_NMS, YUNET_TOP_K, 0.0)
        } else {
            (0..boxes.len()).collect()
        };
        let area = (iw * ih) as f64;
        let max_area = keep
            .iter()
            .map(|&i| boxes[i][2] as f64 * boxes[i][3] as f64 / area)
            .fold(0.0, f64::max);
        Ok((keep.len() as u32, max_area))
    }
}

fn yunet_decode(outs: &[Output], pad_h: usize, pad_w: usize) -> (Vec<[f32; 4]>, Vec<f32>) {
    let mut boxes = Vec::new();
    let mut scores = Vec::new();
    for (i, &stride) in YUNET_STRIDES.iter().enumerate() {
        let (cols, rows) = (pad_w / stride, pad_h / stride);
        let cls = &outs[i].data;
        let obj = &outs[i + 3].data;
        let bbox = &outs[i + 6].data;
        let st = stride as f32;
        for r in 0..rows {
            for c in 0..cols {
                let idx = r * cols + c;
                let cls_score = cls[idx].clamp(0.0, 1.0);
                let obj_score = obj[idx].clamp(0.0, 1.0);
                let score = (cls_score * obj_score).sqrt();
                if score < YUNET_SCORE {
                    continue;
                }
                let cx = (c as f32 + bbox[idx * 4]) * st;
                let cy = (r as f32 + bbox[idx * 4 + 1]) * st;
                let w = bbox[idx * 4 + 2].exp() * st;
                let h = bbox[idx * 4 + 3].exp() * st;
                boxes.push([cx - w / 2.0, cy - h / 2.0, w, h]);
                scores.push(score);
            }
        }
    }
    (boxes, scores)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cv::testing::{fixture, noise, paste, solid};

    fn models() -> &'static Path {
        Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/models"))
    }

    /// 人も顔も写っていない画像では、顔を数えないこと
    #[test]
    fn test_no_faces_in_texture() {
        let mut detectors = FaceDetectors::load(models()).unwrap();
        let img = noise(640, 480, (60.0, 120.0, 60.0), 20.0, 13);

        let f = detectors.measure(&img).unwrap();

        assert_eq!(f.yn_count, 0);
        assert_eq!(f.yn_max_area, 0.0);
    }

    /// 小さな画像でも、止まらずに測れること
    #[test]
    fn test_tiny_image_is_measured() {
        let mut detectors = FaceDetectors::load(models()).unwrap();
        let img = noise(30, 20, (60.0, 120.0, 60.0), 20.0, 14);

        assert!(detectors.measure(&img).is_ok());
    }

    /// モデルのファイルが見つからないときは、読み込みを失敗にすること（検出せずに通してしまわないため）
    #[test]
    fn test_missing_models_fail_to_load() {
        assert!(FaceDetectors::load(Path::new("/nonexistent")).is_err());
    }

    /// 正面を向いた顔の写真では、YuNet が、弾く基準を超える大きさの顔を見つけること
    #[test]
    fn test_frontal_face_is_found() {
        let mut detectors = FaceDetectors::load(models()).unwrap();
        let bgr = fixture("frontal_face.jpg");

        let f = detectors.measure(&bgr).unwrap();

        assert_eq!(f.yn_count, 1);
        assert!(f.yn_max_area >= 0.002, "yn_max_area={}", f.yn_max_area);
    }

    /// 顔探しは、1つのモデルのまま、大きさの違う画像を続けて測れること（大きさごとにモデルを作り直さないため）
    #[test]
    fn test_yunet_measures_different_sizes_with_one_model() {
        let mut detectors = FaceDetectors::load(models()).unwrap();
        let wide = noise(640, 480, (60.0, 120.0, 60.0), 20.0, 21);
        let tall = noise(480, 640, (60.0, 120.0, 60.0), 20.0, 22);

        assert!(detectors.yunet(&wide).is_ok());
        assert!(detectors.yunet(&tall).is_ok());
        assert!(detectors.yunet(&wide).is_ok());
    }

    fn yunet_outputs(scores: &[(usize, f32)]) -> Vec<Output> {
        let mut outs = Vec::new();
        for kind in [1usize, 1, 4, 10] {
            for (level, stride) in YUNET_STRIDES.iter().enumerate() {
                let n = (32 / stride) * (32 / stride);
                let mut data = vec![0f32; n * kind];
                if kind == 1 && level == 0 {
                    for &(cell, score) in scores {
                        data[cell] = score;
                    }
                }
                if kind == 4 {
                    for cell in data.chunks_mut(4) {
                        cell[2] = 4f32.ln();
                        cell[3] = 4f32.ln();
                    }
                }
                outs.push(Output {
                    shape: vec![1, n, kind],
                    data,
                });
            }
        }
        outs
    }

    /// YuNet の確からしさ（顔だという自信の度合い）は試作と同じく 0.6 が基準で、0.6 ちょうどは残し、それ未満は捨てること
    #[test]
    fn test_yunet_score_threshold_matches_trial() {
        let outs = yunet_outputs(&[(0, YUNET_SCORE), (5, 0.5999)]);

        let (boxes, scores) = yunet_decode(&outs, 32, 32);

        assert_eq!(YUNET_SCORE, 0.6);
        assert_eq!(scores, vec![YUNET_SCORE]);
        assert_eq!(boxes.len(), 1);
    }

    /// 重なった枠を1つにまとめる基準は試作と同じ 0.3 であること（隣り合うマスの枠は重なりが 0.3 を超えるので1つにまとまる）
    #[test]
    fn test_yunet_nms_threshold_matches_trial() {
        assert_eq!((YUNET_SCORE, YUNET_NMS), (0.6, 0.3));
        let outs = yunet_outputs(&[(0, 0.9), (1, 0.8)]);
        let (boxes, scores) = yunet_decode(&outs, 32, 32);
        let rects: Vec<[f64; 4]> = boxes.iter().map(|b| b.map(|v| v.trunc() as f64)).collect();

        assert_eq!(
            nms(&rects, &scores, YUNET_SCORE, YUNET_NMS, YUNET_TOP_K, 0.0),
            vec![0]
        );
    }

    fn close(actual: f64, expected: f64) -> bool {
        (actual - expected).abs() < 0.001
    }

    /// テスト用の写真で測った値が試作の設定どおりの値から変わらないこと（設定の変更や OpenCV の更新で検出の結果が変わったら気づくため）
    #[test]
    fn test_fixture_measurements_stay_the_same() {
        let mut detectors = FaceDetectors::load(models()).unwrap();

        let bgr = fixture("frontal_face.jpg");
        let f = detectors.measure(&bgr).unwrap();
        assert_eq!(f.yn_count, 1);
        assert!(
            close(f.yn_max_area, 0.0508),
            "yn_max_area={}",
            f.yn_max_area
        );

        let bgr = fixture("walking_person.jpg");
        let f = detectors.measure(&bgr).unwrap();
        assert_eq!(f.yn_count, 1);
        assert!(
            close(f.yn_max_area, 0.0190),
            "yn_max_area={}",
            f.yn_max_area
        );
    }

    /// 顔が2つ写っているときは2つと数え、顔の広さは合計ではなく大きい方の顔のものにすること
    #[test]
    fn test_two_faces_counts_both_and_keeps_largest_area() {
        let face = fixture("frontal_face.jpg");
        let mut canvas = solid(960, 640, (90.0, 120.0, 90.0));
        paste(&mut canvas, &face, 0, 0);
        let small = img::resize(&face, face.w * 6 / 10, face.h * 6 / 10, Interp::Area);
        paste(&mut canvas, &small, 560, 100);

        let f = detectors_measure(&canvas);

        assert_eq!(f.yn_count, 2);
        assert!(
            close(f.yn_max_area, 0.0259),
            "yn_max_area={}",
            f.yn_max_area
        );
    }

    fn detectors_measure(bgr: &Image) -> Faces {
        FaceDetectors::load(models()).unwrap().measure(bgr).unwrap()
    }
}
