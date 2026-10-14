use std::path::Path;

use vision::img::{self, Image, Interp};
use vision::ncnn::NcnnModel;

#[derive(Debug, Clone, PartialEq)]
pub struct Clip {
    pub toon_p: f64,
    pub close_p: f64,
    pub manmade_p: f64,
    pub window_p: f64,
}

#[derive(serde::Deserialize)]
struct TextEmbeds {
    photo: Vec<Vec<f32>>,
    toon: Vec<Vec<f32>>,
    far: Vec<Vec<f32>>,
    near: Vec<Vec<f32>>,
    scene_nature: Vec<Vec<f32>>,
    scene_manmade: Vec<Vec<f32>>,
    window: Vec<Vec<f32>>,
    tile_toon: Vec<Vec<f32>>,
}

#[derive(serde::Deserialize)]
struct Meta {
    image_size: usize,
    mean: [f32; 3],
    std: [f32; 3],
    logit_scale: f64,
}

pub struct ClipScorer {
    net: NcnnModel,
    text: TextEmbeds,
    meta: Meta,
}

impl ClipScorer {
    pub fn load(dir: &Path) -> anyhow::Result<Self> {
        let meta: Meta =
            serde_json::from_str(&std::fs::read_to_string(dir.join("clip_meta.json"))?)?;
        let net = NcnnModel::load_named(dir, "clip_vision")?;
        let text: TextEmbeds =
            serde_json::from_str(&std::fs::read_to_string(dir.join("clip_text_embeds.json"))?)?;
        Ok(Self { net, text, meta })
    }

    pub fn score(&mut self, bgr: &Image) -> anyhow::Result<Clip> {
        let e = self.image_embed(bgr)?;
        Ok(Clip {
            toon_p: share(&e, &self.text.photo, &self.text.toon, self.meta.logit_scale),
            close_p: share(&e, &self.text.far, &self.text.near, self.meta.logit_scale),
            manmade_p: share(
                &e,
                &self.text.scene_nature,
                &self.text.scene_manmade,
                self.meta.logit_scale,
            ),
            window_p: share(
                &e,
                &self.text.photo,
                &self.text.window,
                self.meta.logit_scale,
            ),
        })
    }

    pub fn tile_toon_max(&mut self, bgr: &Image) -> anyhow::Result<f64> {
        let mut max = 0.0f64;
        for (x, y, w, h) in tiles(bgr.w, bgr.h) {
            let e = self.image_embed(&img::crop(bgr, x, y, w, h))?;
            max = max.max(share(
                &e,
                &self.text.photo,
                &self.text.tile_toon,
                self.meta.logit_scale,
            ));
        }
        Ok(max)
    }

    pub fn image_embed(&mut self, src: &Image) -> anyhow::Result<Vec<f32>> {
        anyhow::ensure!(src.w > 0 && src.h > 0, "empty image");
        let side = src.w.max(src.h);
        let square = img::place(src, side, side, (side - src.w) / 2, (side - src.h) / 2);
        let size = self.meta.image_size;
        let resized = img::resize(&square, size, size, Interp::Area);
        let scale = (1.0f64 / 255.0) as f32;
        let wh = size * size;
        let mut blob = vec![0f32; 3 * wh];
        for (i, p) in resized.data.as_chunks::<3>().0.iter().enumerate() {
            for c in 0..3 {
                blob[c * wh + i] = (p[2 - c] as f32 * scale - self.meta.mean[c]) / self.meta.std[c];
            }
        }
        let v = self.net.run(blob, [3, size, size])?.swap_remove(0).data;
        let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        Ok(v.iter().map(|x| x / norm).collect())
    }
}

const TILES_PER_SIDE: usize = 3;

fn tiles(w: usize, h: usize) -> Vec<(usize, usize, usize, usize)> {
    let n = TILES_PER_SIDE;
    let (tw, th) = (w * 2 / (n + 1), h * 2 / (n + 1));
    let mut out = Vec::with_capacity(n * n);
    for j in 0..n {
        for i in 0..n {
            out.push((i * (w - tw) / (n - 1), j * (h - th) / (n - 1), tw, th));
        }
    }
    out
}

fn share(e: &[f32], base: &[Vec<f32>], target: &[Vec<f32>], logit_scale: f64) -> f64 {
    let logits: Vec<f64> = base
        .iter()
        .chain(target)
        .map(|t| logit_scale * t.iter().zip(e).map(|(a, b)| a * b).sum::<f32>() as f64)
        .collect();
    let max = logits.iter().cloned().fold(f64::MIN, f64::max);
    let exps: Vec<f64> = logits.iter().map(|l| (l - max).exp()).collect();
    let sum: f64 = exps.iter().sum();
    exps[base.len()..].iter().sum::<f64>() / sum
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cv::testing::{fixture, models, text_image};

    /// 画像が写真側とアニメ側の文章に同じくらい近ければ割合は半分、どちらかにぴったり近ければその側がほぼ全部になること
    #[test]
    fn test_share_is_softmax_over_both_sides() {
        let photo = vec![vec![1.0, 0.0]];
        let toon = vec![vec![0.0, 1.0]];
        let middle = [std::f32::consts::FRAC_1_SQRT_2; 2];

        assert!((share(&middle, &photo, &toon, 100.0) - 0.5).abs() < 1e-9);
        assert!(share(&[0.0, 1.0], &photo, &toon, 100.0) > 0.999);
        assert!(share(&[1.0, 0.0], &photo, &toon, 100.0) < 0.001);
    }

    /// マスは縦横それぞれ半分の大きさで、4分の1ずつずらして3×3に並べること（境目にかかった物も、どれかのマスには丸ごと入るようにするため）
    #[test]
    fn test_tiles_are_half_size_and_overlap_by_half() {
        let t = tiles(400, 300);

        assert_eq!(t.len(), 9);
        assert!(t.iter().all(|&(_, _, w, h)| (w, h) == (200, 150)));
        assert_eq!(t[0], (0, 0, 200, 150));
        assert_eq!(t[4], (100, 75, 200, 150));
        assert_eq!(t[8], (200, 150, 200, 150));
    }

    /// 縦横4分の1までの大きさの物は、どこに置いても、どれかのマスに丸ごと入ること
    #[test]
    fn test_every_quarter_sized_box_fits_in_some_tile() {
        let (w, h) = (400, 300);
        let t = tiles(w, h);
        for x in 0..=(w - w / 4) {
            for y in (0..=(h - h / 4)).step_by(5) {
                assert!(
                    t.iter().any(|&(tx, ty, tw, th)| tx <= x
                        && x + w / 4 <= tx + tw
                        && ty <= y
                        && y + h / 4 <= ty + th),
                    "{x} {y}"
                );
            }
        }
    }

    /// アニメ側の文章が複数あるときは、それぞれの割合を足し合わせること
    #[test]
    fn test_share_sums_all_target_texts() {
        let photo = vec![vec![1.0, 0.0]];
        let toon = vec![vec![0.0, 1.0], vec![0.0, 1.0]];
        let middle = [std::f32::consts::FRAC_1_SQRT_2; 2];

        assert!((share(&middle, &photo, &toon, 100.0) - 2.0 / 3.0).abs() < 1e-9);
    }

    /// マスごとのアニメ調の割合の最大が、記録した値から変わらないこと。全体で見るより、どれかのマスで見たほうが高く出ること（小さく写ったアニメ調の物を拾うのが目的のため）
    #[test]
    fn test_tile_toon_max_stays_the_same() {
        let mut c = ClipScorer::load(models()).unwrap();
        let cases = [
            (
                "walking_person.jpg",
                fixture("walking_person.jpg"),
                0.6354761689861143,
            ),
            (
                "frontal_face.jpg",
                fixture("frontal_face.jpg"),
                0.7351547969856078,
            ),
        ];
        for (name, img, expected) in cases {
            let tile = c.tile_toon_max(&img).unwrap();
            let whole = c.score(&img).unwrap().toon_p;
            assert!((tile - expected).abs() < 1e-5, "{name} {tile}");
            assert!(tile > whole, "{name} {tile} {whole}");
        }
    }

    /// モデルのファイルが見つからないときは、読み込みを失敗にすること（判定せずに通してしまわないため）
    #[test]
    fn test_missing_model_fails_to_load() {
        assert!(ClipScorer::load(Path::new("/nonexistent")).is_err());
    }

    /// テスト用の画像で、アニメ調・庭や見下ろし・人工物の割合が記録した値から変わらないこと（正方形に広げる余白・縮小・色の並びが変わったら気づくため。古い白黒写真はアニメ側に寄る）
    #[test]
    fn test_scores_stay_the_same() {
        let mut c = ClipScorer::load(models()).unwrap();
        let cases = [
            (
                "walking_person.jpg",
                fixture("walking_person.jpg"),
                0.4301417818532445,
                0.8979417993430812,
                0.9722825042508212,
            ),
            (
                "frontal_face.jpg",
                fixture("frontal_face.jpg"),
                0.22665355479725735,
                0.6519762463642799,
                0.7777400892983101,
            ),
            (
                "text",
                text_image(0.0),
                0.6651439323247276,
                0.6703453163926393,
                0.8476360259032683,
            ),
        ];
        for (name, img, toon_p, close_p, manmade_p) in cases {
            let s = c.score(&img).unwrap();
            assert!((s.toon_p - toon_p).abs() < 1e-5, "{name} {}", s.toon_p);
            assert!((s.close_p - close_p).abs() < 1e-5, "{name} {}", s.close_p);
            assert!(
                (s.manmade_p - manmade_p).abs() < 1e-5,
                "{name} {}",
                s.manmade_p
            );
        }
    }
}
