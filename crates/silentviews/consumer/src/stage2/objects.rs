use std::path::Path;

use vision::img::{self, Image, Interp};
use vision::ncnn::{NcnnModel, Output};
use vision::nms::nms;

const INPUT: i32 = 416;
const STRIDES: [i32; 4] = [8, 16, 32, 64];
const REG_MAX: usize = 7;
const PROB_THRESHOLD: f32 = 0.35;
const IOU_THRESHOLD: f32 = 0.6;
const NMS_PRE: usize = 1000;

pub const COCO: [&str; 80] = [
    "person",
    "bicycle",
    "car",
    "motorcycle",
    "airplane",
    "bus",
    "train",
    "truck",
    "boat",
    "traffic_light",
    "fire_hydrant",
    "stop_sign",
    "parking_meter",
    "bench",
    "bird",
    "cat",
    "dog",
    "horse",
    "sheep",
    "cow",
    "elephant",
    "bear",
    "zebra",
    "giraffe",
    "backpack",
    "umbrella",
    "handbag",
    "tie",
    "suitcase",
    "frisbee",
    "skis",
    "snowboard",
    "sports_ball",
    "kite",
    "baseball_bat",
    "baseball_glove",
    "skateboard",
    "surfboard",
    "tennis_racket",
    "bottle",
    "wine_glass",
    "cup",
    "fork",
    "knife",
    "spoon",
    "bowl",
    "banana",
    "apple",
    "sandwich",
    "orange",
    "broccoli",
    "carrot",
    "hot_dog",
    "pizza",
    "donut",
    "cake",
    "chair",
    "couch",
    "potted_plant",
    "bed",
    "dining_table",
    "toilet",
    "tv",
    "laptop",
    "mouse",
    "remote",
    "keyboard",
    "cell_phone",
    "microwave",
    "oven",
    "toaster",
    "sink",
    "refrigerator",
    "book",
    "clock",
    "vase",
    "scissors",
    "teddy_bear",
    "hair_drier",
    "toothbrush",
];

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Object {
    pub name: &'static str,
    pub conf: f64,
    pub area: f64,
}

pub struct ObjectDetector {
    net: NcnnModel,
}

impl ObjectDetector {
    pub fn load(models: &Path) -> anyhow::Result<Self> {
        let net = NcnnModel::load_named(models, "object_detection_nanodet_2022nov")?;
        Ok(Self { net })
    }

    pub fn detect(&mut self, bgr: &Image) -> anyhow::Result<Vec<Object>> {
        Ok(self
            .detect_with_boxes(bgr)?
            .into_iter()
            .map(|(object, _)| object)
            .collect())
    }

    fn detect_with_boxes(&mut self, bgr: &Image) -> anyhow::Result<Vec<(Object, [f64; 4])>> {
        let (blob, nw, nh) = preprocess(bgr)?;
        let outs = self.net.run(blob, [3, INPUT as usize, INPUT as usize])?;

        let (boxes, scores) = decode(&outs)?;
        let mut rects = Vec::with_capacity(boxes.len());
        let mut confs = Vec::with_capacity(boxes.len());
        let mut classes = Vec::with_capacity(scores.len());
        for (b, row) in boxes.iter().zip(&scores) {
            let (cls, conf) = argmax(row);
            rects.push([b[0], b[1], b[2] - b[0], b[3] - b[1]]);
            confs.push(conf);
            classes.push(cls);
        }
        let keep = nms(
            &rects,
            &confs,
            PROB_THRESHOLD,
            IOU_THRESHOLD,
            0,
            f64::EPSILON,
        );

        let (fw, fh) = (nw as f64, nh as f64);
        Ok(keep
            .into_iter()
            .map(|i| {
                let [x1, y1, x2, y2] = boxes[i];
                let area = ((x2.min(fw) - x1) * (y2.min(fh) - y1) / (fw * fh)).max(0.0);
                let object = Object {
                    name: COCO[classes[i]],
                    conf: round(confs[i] as f64, 2),
                    area: round(area, 3),
                };
                (object, boxes[i])
            })
            .collect())
    }
}

const MEAN: [f64; 3] = [103.53, 116.28, 123.675];
const STD: [f64; 3] = [57.375, 57.12, 58.395];

fn preprocess(src: &Image) -> anyhow::Result<(Vec<f32>, i32, i32)> {
    anyhow::ensure!(src.w > 0 && src.h > 0, "empty image");
    let s = INPUT as f64 / src.h.max(src.w) as f64;
    let (nh, nw) = ((src.h as f64 * s) as usize, (src.w as f64 * s) as usize);
    anyhow::ensure!(nw > 0 && nh > 0, "image too elongated");
    let resized = img::resize(src, nw, nh, Interp::Linear);
    let side = INPUT as usize;
    let canvas = img::place(&resized, side, side, 0, 0);
    let mean = MEAN.map(|v| v as f32);
    let wh = side * side;
    let mut blob = vec![0f32; 3 * wh];
    for (i, p) in canvas.data.as_chunks::<3>().0.iter().enumerate() {
        for c in 0..3 {
            blob[c * wh + i] = ((p[c] as f32 - mean[c]) as f64 / STD[c]) as f32;
        }
    }
    Ok((blob, nw as i32, nh as i32))
}

fn round(v: f64, digits: i32) -> f64 {
    let p = 10f64.powi(digits);
    (v * p).round() / p
}

fn argmax(row: &[f32]) -> (usize, f32) {
    let mut best = (0, f32::MIN);
    for (i, &v) in row.iter().enumerate() {
        if v > best.1 {
            best = (i, v);
        }
    }
    best
}

type Boxes = Vec<[f64; 4]>;
type Scores = Vec<Vec<f32>>;

fn by_level(outs: &[Output], width: usize) -> Vec<&Output> {
    let mut level: Vec<&Output> = outs
        .iter()
        .filter(|o| o.shape.last() == Some(&width))
        .collect();
    level.sort_by_key(|o| std::cmp::Reverse(o.data.len()));
    level
}

fn decode(outs: &[Output]) -> anyhow::Result<(Boxes, Scores)> {
    let cls_levels = by_level(outs, COCO.len());
    let reg_levels = by_level(outs, 4 * (REG_MAX + 1));
    anyhow::ensure!(
        !cls_levels.is_empty() && cls_levels.len() == reg_levels.len(),
        "unexpected nanodet outputs"
    );
    let mut boxes = Vec::new();
    let mut scores = Vec::new();
    for (level, &stride) in STRIDES.iter().enumerate().take(cls_levels.len()) {
        let cls_v = &cls_levels[level].data;
        let reg_v = &reg_levels[level].data;
        let n = cls_v.len() / COCO.len();
        let feat = INPUT / stride;
        let anchors: Vec<(f64, f64)> = (0..feat * feat)
            .map(|k| {
                let (x, y) = ((k % feat) * stride, (k / feat) * stride);
                (
                    x as f64 + 0.5 * (stride - 1) as f64,
                    y as f64 + 0.5 * (stride - 1) as f64,
                )
            })
            .collect();

        let mut order: Vec<usize> = (0..n).collect();
        if n > NMS_PRE {
            let max_score = |k: usize| {
                cls_v[k * COCO.len()..(k + 1) * COCO.len()]
                    .iter()
                    .cloned()
                    .fold(f32::MIN, f32::max)
            };
            order.sort_by(|&a, &b| max_score(b).total_cmp(&max_score(a)));
            order.truncate(NMS_PRE);
        }

        for k in order {
            let mut d = [0f64; 4];
            for (side, dist) in d.iter_mut().enumerate() {
                let bins =
                    &reg_v[(k * 4 + side) * (REG_MAX + 1)..(k * 4 + side + 1) * (REG_MAX + 1)];
                let exps: Vec<f32> = bins.iter().map(|v| v.exp()).collect();
                let sum: f32 = exps.iter().sum();
                *dist = exps
                    .iter()
                    .enumerate()
                    .map(|(j, e)| (e / sum) as f64 * j as f64)
                    .sum::<f64>()
                    * stride as f64;
            }
            let (cx, cy) = anchors[k];
            let clip = |v: f64| v.clamp(0.0, INPUT as f64);
            boxes.push([
                clip(cx - d[0]),
                clip(cy - d[1]),
                clip(cx + d[2]),
                clip(cy + d[3]),
            ]);
            scores.push(cls_v[k * COCO.len()..(k + 1) * COCO.len()].to_vec());
        }
    }
    Ok((boxes, scores))
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::cv::testing::{crop, paste, Fnv, Rect};

    /// 物体探しに渡す数字の並びが、OpenCV で作った場合と同じことを確かめたときの記録から変わらないこと（横長・縦長・小さい画像）
    #[test]
    fn test_preprocess_stays_recorded() {
        let mut h = Fnv::new();
        for img in [
            fixture("walking_person.jpg"),
            fixture("frontal_face.jpg"),
            solid(100, 30, (10.0, 200.0, 90.0)),
        ] {
            let (blob, nw, nh) = preprocess(&img).unwrap();
            h.f32s(&blob);
            h.i32s(&[nw, nh]);
        }
        assert_eq!(h.0, 4841119074689102811);
    }

    /// 横長の画像は、長い辺を 416 に合わせて縮めて左上に置き、残りの余白は黒（0 から色ごとの平均を引いて割った値）で埋めること
    #[test]
    fn test_preprocess_places_image_top_left_and_pads_black() {
        let (blob, nw, nh) = preprocess(&solid(100, 30, (10.0, 200.0, 90.0))).unwrap();
        assert_eq!((nw, nh), (416, 124));
        let wh = 416 * 416;
        for (c, v) in [10.0f32, 200.0, 90.0].into_iter().enumerate() {
            let inside = ((v - MEAN[c] as f32) as f64 / STD[c]) as f32;
            let pad = ((0.0 - MEAN[c] as f32) as f64 / STD[c]) as f32;
            assert_eq!(blob[c * wh + 123 * 416 + 415], inside);
            assert_eq!(blob[c * wh + 124 * 416], pad);
            assert_eq!(blob[c * wh + 415 * 416 + 415], pad);
        }
    }

    use crate::cv::testing::{fixture, models, solid, text_image};

    fn object(name: &'static str, conf: f64, area: f64) -> Object {
        Object { name, conf, area }
    }

    /// 人の全身が写った写真では、人（person）を、物だという自信の度合い（確からしさ）と画面に占める広さ付きで見つけ、確からしさの低いもの（0.35 以上）まで返すこと
    #[test]
    fn test_walking_person_photo() {
        let mut detector = ObjectDetector::load(models()).unwrap();

        let found = detector.detect(&fixture("walking_person.jpg")).unwrap();

        assert_eq!(
            found,
            vec![
                object("person", 0.75, 0.562),
                object("person", 0.54, 0.092),
                object("skateboard", 0.35, 0.076),
            ]
        );
    }

    /// 胸から上の人物写真では、画面の大部分を占める人を見つけること（縦長の写真でも、正方形にするために足した余白は広さに含めない）
    #[test]
    fn test_portrait_photo() {
        let mut detector = ObjectDetector::load(models()).unwrap();

        let found = detector.detect(&fixture("frontal_face.jpg")).unwrap();

        assert_eq!(found, vec![object("person", 0.79, 0.757)]);
    }

    /// 文字だけの画像では、物を見つけないこと
    #[test]
    fn test_text_only_image_has_no_objects() {
        let mut detector = ObjectDetector::load(models()).unwrap();

        assert!(detector.detect(&text_image(0.0)).unwrap().is_empty());
    }

    fn on_canvas(w: i32, h: i32, places: &[(Rect, Rect)]) -> Image {
        let person = fixture("walking_person.jpg");
        let mut canvas = solid(w, h, (120.0, 140.0, 120.0));
        for (from, to) in places {
            let part = img::resize(
                &crop(&person, *from),
                to.width as usize,
                to.height as usize,
                Interp::Area,
            );
            paste(&mut canvas, &part, to.x as usize, to.y as usize);
        }
        canvas
    }

    /// 見つけた枠の位置が、試作と同じ計算（画面を区切ったマス目の中心から、上下左右にどれだけ伸びるか）で求めた位置になること
    #[test]
    fn test_box_position_matches_trial_calculation() {
        let mut detector = ObjectDetector::load(models()).unwrap();

        let found = detector
            .detect_with_boxes(&fixture("walking_person.jpg"))
            .unwrap();

        let expected = [38.38, 49.78, 171.04, 397.04];
        for (a, e) in found[0].1.iter().zip(expected) {
            assert!((a - e).abs() < 0.05, "{:?}", found[0].1);
        }
    }

    /// 画像の端で切れた人の枠は、はみ出した分を画像の端で切ること
    #[test]
    fn test_box_cut_at_image_edge() {
        let full = Rect::new(0, 0, 250, 600);
        let canvas = on_canvas(1200, 600, &[(full, Rect::new(950, 0, 250, 600))]);
        let mut detector = ObjectDetector::load(models()).unwrap();

        let found = detector.detect_with_boxes(&canvas).unwrap();

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].1[2], 416.0);
    }

    /// 2人が重なって写っていても、枠どうしの重なり具合が基準（0.6）未満なら、1つにまとめずに別々に数えること
    #[test]
    fn test_overlapping_people_are_kept_apart() {
        let full = Rect::new(0, 0, 364, 768);
        let canvas = on_canvas(
            700,
            768,
            &[
                (full, Rect::new(0, 0, 364, 768)),
                (full, Rect::new(220, 0, 364, 768)),
            ],
        );
        let mut detector = ObjectDetector::load(models()).unwrap();

        let names: Vec<_> = detector
            .detect(&canvas)
            .unwrap()
            .into_iter()
            .map(|o| o.name)
            .collect();

        assert_eq!(
            names,
            [
                "person",
                "person",
                "person",
                "person",
                "person",
                "skateboard"
            ]
        );
    }

    /// 小さな人がたくさん写っていても、途中で候補を絞りすぎずに、試作と同じ数（37）を見つけること
    #[test]
    fn test_crowd_of_small_people() {
        let full = Rect::new(0, 0, 364, 768);
        let places: Vec<_> = (0..4)
            .flat_map(|r| (0..8).map(move |c| (full, Rect::new(c * 91, r * 192, 91, 192))))
            .collect();
        let mut detector = ObjectDetector::load(models()).unwrap();

        assert_eq!(
            detector
                .detect(&on_canvas(728, 768, &places))
                .unwrap()
                .len(),
            37
        );
    }

    /// モデルのファイルが見つからないときは、読み込みを失敗にすること（検出せずに通してしまわないため）
    #[test]
    fn test_missing_model_fails_to_load() {
        assert!(ObjectDetector::load(Path::new("/nonexistent")).is_err());
    }
}
