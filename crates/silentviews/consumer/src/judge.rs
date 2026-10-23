use crate::stage2::clip::Clip;
use crate::stage2::faces::Faces;
use crate::stage2::near::Near;
use crate::stage2::objects::Object;
use crate::stage2::ocr::Ocr;
use crate::stage2::LightFeatures;
use crate::MAX_ASPECT;

const WILD: [&str; 14] = [
    "bird",
    "giraffe",
    "elephant",
    "zebra",
    "bear",
    "sheep",
    "cow",
    "horse",
    "potted_plant",
    "broccoli",
    "banana",
    "apple",
    "orange",
    "kite",
];
const BIG: [&str; 17] = [
    "person",
    "dog",
    "cat",
    "horse",
    "cow",
    "sheep",
    "bear",
    "elephant",
    "giraffe",
    "zebra",
    "teddy_bear",
    "kite",
    "umbrella",
    "fire_hydrant",
    "airplane",
    "car",
    "potted_plant",
];
const ANIMALS: [&str; 11] = [
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
    "teddy_bear",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    Unmeasured,
    Elongated,
    TooSmall,
    LineArt,
    Illustration,
    FlatColors,
    Text,
    TextOcr,
    NearBlob,
    CenterSubject,
    FaceYunet,
    Object,
    BigObject,
    PersonObject,
    Animal,
    Mirror,
    Pavement,
    Manmade,
    NoTexture,
    TooFewNatureColors,
    ManmadeScene,
    TooClose,
    TooCloseLines,
    Toon,
    GardenOrLookingDown,
    ToonTile,
    ThroughWindow,
    EdgeText,
    ForegroundObject,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Early {
    pub light: LightFeatures,
    pub faces: Faces,
    pub objects: Vec<Object>,
    pub ocr: Ocr,
    pub max_side: i32,
    pub manmade_p: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Late {
    pub near: Near,
    pub clip: Clip,
    pub sky: f64,
}

impl Late {
    fn has_nan(&self) -> bool {
        [
            self.near.rt_min,
            self.near.top_smooth,
            self.clip.toon_p,
            self.clip.close_p,
            self.clip.window_p,
            self.sky,
        ]
        .iter()
        .chain(&self.near.ln_count)
        .any(|v| v.is_nan())
    }
}

fn any_nan(values: &[f64]) -> bool {
    values.iter().any(|v| v.is_nan())
}

fn first_hit<const N: usize>(rules: [(Reason, bool); N]) -> Option<Reason> {
    rules.into_iter().find(|(_, hit)| *hit).map(|(r, _)| r)
}

pub fn judge_light(f: &LightFeatures, max_side: i32) -> Option<Reason> {
    let measured = [
        f.blur,
        f.flat,
        f.text_area,
        f.edge,
        f.green,
        f.blue,
        f.earth,
        f.sat_mean,
        f.sat_hi,
        f.sky,
        f.near_blob,
        f.center_near,
        f.side_near,
        f.smooth_share,
        f.dead_flat,
        f.dead_flat_ns,
        f.ground_gray,
        f.mirror,
        f.aspect,
        f.colorfulness,
    ];
    if any_nan(&measured) {
        return Some(Reason::Unmeasured);
    }
    first_hit([
        (Reason::Elongated, f.aspect >= MAX_ASPECT),
        (Reason::TooSmall, max_side < 400),
        (Reason::LineArt, f.blur > 12000.0),
        (
            Reason::Illustration,
            f.dead_flat >= 0.12 && f.dead_flat_ns >= 0.05,
        ),
        (Reason::FlatColors, f.flat >= 0.35),
        (Reason::Text, f.text_area >= 0.011),
        (
            Reason::NearBlob,
            f.near_blob >= 0.35 && f.sky < 0.05 && f.smooth_share < 0.3,
        ),
        (Reason::CenterSubject, f.center_near - f.side_near >= 0.3),
        (Reason::Mirror, f.mirror >= 0.6),
        (Reason::Pavement, f.ground_gray >= 0.2),
        (
            Reason::Manmade,
            f.corners >= 5
                || f.seg_v >= 20
                || (f.seg_hv >= 50 && f.seg_v >= 8)
                || f.vert + f.diag > 5
                || f.horiz > 6,
        ),
        (Reason::NoTexture, f.edge < 0.005),
        (Reason::TooFewNatureColors, too_few_nature_colors(f)),
    ])
}

pub fn judge_objects(objects: &[Object]) -> Option<Reason> {
    if objects.iter().any(|o| o.conf.is_nan() || o.area.is_nan()) {
        return Some(Reason::Unmeasured);
    }
    first_hit([
        (
            Reason::Object,
            objects
                .iter()
                .any(|o| o.conf >= 0.6 || (o.conf >= 0.5 && !WILD.contains(&o.name))),
        ),
        (
            Reason::BigObject,
            objects
                .iter()
                .any(|o| o.conf >= 0.4 && o.area >= 0.2 && BIG.contains(&o.name)),
        ),
        (
            Reason::PersonObject,
            objects
                .iter()
                .any(|o| o.name == "person" && o.conf >= 0.35 && o.area >= 0.02),
        ),
        (
            Reason::Animal,
            objects
                .iter()
                .filter(|o| ANIMALS.contains(&o.name) && o.conf >= 0.35)
                .map(|o| o.area)
                .sum::<f64>()
                >= 0.1,
        ),
    ])
}

pub fn judge_scene(manmade_p: f64) -> Option<Reason> {
    if manmade_p.is_nan() {
        return Some(Reason::Unmeasured);
    }
    (manmade_p >= 0.5).then_some(Reason::ManmadeScene)
}

pub fn judge_ocr(o: &Ocr) -> Option<Reason> {
    if o.ocr_area.is_nan() {
        return Some(Reason::Unmeasured);
    }
    first_hit([
        (Reason::TextOcr, o.ocr_lines >= 1 || o.ocr_n >= 3),
        (Reason::EdgeText, o.ocr_edge >= 1),
    ])
}

pub fn judge_faces(f: &Faces) -> Option<Reason> {
    if f.yn_max_area.is_nan() {
        return Some(Reason::Unmeasured);
    }
    (f.yn_count >= 2 || f.yn_max_area >= 0.002).then_some(Reason::FaceYunet)
}

pub fn judge_early(e: &Early) -> Option<Reason> {
    judge_light(&e.light, e.max_side)
        .or_else(|| judge_objects(&e.objects))
        .or_else(|| judge_scene(e.manmade_p))
        .or_else(|| judge_ocr(&e.ocr))
        .or_else(|| judge_faces(&e.faces))
}

pub fn judge_late(l: &Late) -> Option<Reason> {
    if l.has_nan() {
        return Some(Reason::Unmeasured);
    }
    let Some(ln_count) = l.near.ln_count else {
        return Some(Reason::Unmeasured);
    };
    first_hit([
        (Reason::TooClose, l.near.rt_min >= 0.89 && l.sky < 0.2),
        (
            Reason::TooCloseLines,
            ln_count >= 4.5 && l.near.top_smooth < 0.1,
        ),
        (Reason::Toon, l.clip.toon_p >= 0.294),
        (Reason::GardenOrLookingDown, l.clip.close_p >= 0.7),
        (Reason::ThroughWindow, l.clip.window_p >= 0.6),
    ])
}

pub fn judge_tiles(toon_tile_max: f64) -> Option<Reason> {
    if toon_tile_max.is_nan() {
        return Some(Reason::Unmeasured);
    }
    (toon_tile_max >= 0.25).then_some(Reason::ToonTile)
}

pub fn judge_foreground(lower_object_share: f64) -> Option<Reason> {
    if lower_object_share.is_nan() {
        return Some(Reason::Unmeasured);
    }
    (lower_object_share >= 0.025).then_some(Reason::ForegroundObject)
}

fn too_few_nature_colors(f: &LightFeatures) -> bool {
    let nat = f.green + f.blue;
    nat < 0.2 || !(nat >= 0.25 || nat + f.earth >= 0.4)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::stage2::objects::COCO;

    type Change<T> = Box<dyn Fn(&mut T)>;

    pub(crate) fn passing() -> Early {
        Early {
            light: LightFeatures {
                blur: 1000.0,
                flat: 0.1,
                text_lines: 0,
                text_area: 0.0,
                horiz: 0,
                vert: 0,
                diag: 0,
                edge: 0.1,
                green: 0.4,
                blue: 0.1,
                earth: 0.1,
                sat_mean: 0.3,
                sat_hi: 0.0,
                sky: 0.0,
                near_blob: 0.1,
                center_near: 0.1,
                side_near: 0.1,
                smooth_share: 0.1,
                dead_flat: 0.0,
                dead_flat_ns: 0.0,
                ground_gray: 0.0,
                mirror: 0.0,
                aspect: 1.5,
                seg_hv: 0,
                seg_v: 0,
                corners: 0,
                vivid_hues: 1,
                colorfulness: 30.0,
            },
            faces: Faces {
                yn_count: 0,
                yn_max_area: 0.0,
            },
            objects: vec![],
            ocr: Ocr {
                ocr_n: 0,
                ocr_area: 0.0,
                ocr_lines: 0,
                ocr_edge: 0,
            },
            max_side: 1000,
            manmade_p: 0.1,
        }
    }

    pub(crate) fn passing_late() -> Late {
        Late {
            near: Near {
                rt_min: 0.5,
                ln_count: Some(1.0),
                top_smooth: 0.5,
            },
            clip: Clip {
                toon_p: 0.0,
                close_p: 0.5,
                manmade_p: 0.0,
                window_p: 0.0,
            },
            sky: 0.0,
        }
    }

    fn with(change: impl Fn(&mut Early)) -> Option<Reason> {
        let mut e = passing();
        change(&mut e);
        judge_early(&e)
    }

    fn late_with(change: impl Fn(&mut Late)) -> Option<Reason> {
        let mut l = passing_late();
        change(&mut l);
        judge_late(&l)
    }

    fn object(name: &str, conf: f64, area: f64) -> Object {
        Object {
            name: COCO.iter().find(|c| **c == name).unwrap(),
            conf,
            area,
        }
    }

    /// どのルールにも当てはまらない数値は、前半も後半も通すこと
    #[test]
    fn test_passing_features_pass() {
        assert_eq!(judge_early(&passing()), None);
        assert_eq!(judge_late(&passing_late()), None);
    }

    /// 画質のルール：縦横比3以上を弾き、基準の手前は通すこと
    #[test]
    fn test_elongated_rule() {
        assert_eq!(with(|e| e.light.aspect = 3.0), Some(Reason::Elongated));
        assert_eq!(with(|e| e.light.aspect = 2.99), None);
    }

    /// 画質のルール：長辺400px未満を弾き、基準の手前は通すこと
    #[test]
    fn test_too_small_rule() {
        assert_eq!(with(|e| e.max_side = 399), Some(Reason::TooSmall));
        assert_eq!(with(|e| e.max_side = 400), None);
    }

    /// 画質のルール：くっきり度12000超（漫画の線）を弾き、基準の手前は通すこと
    #[test]
    fn test_line_art_rule() {
        assert_eq!(with(|e| e.light.blur = 12000.1), Some(Reason::LineArt));
        assert_eq!(with(|e| e.light.blur = 12000.0), None);
    }

    /// くっきり度が低い（ぼやけている）だけでは弾かないこと（ぼやけていても実害がなく、模様の少ない良い空や海の写真を落としていたため）
    #[test]
    fn test_blur_alone_is_not_rejected() {
        assert_eq!(with(|e| e.light.blur = 0.0), None);
    }

    /// 作り物のルール：平らな塗りが12%以上かつ空を除いても5%以上（イラスト）を弾くこと
    #[test]
    fn test_illustration_rule() {
        let illust = |a: f64, b: f64| {
            with(move |e| {
                e.light.dead_flat = a;
                e.light.dead_flat_ns = b;
            })
        };
        assert_eq!(illust(0.12, 0.05), Some(Reason::Illustration));
        assert_eq!(illust(0.12, 0.049), None);
        assert_eq!(illust(0.119, 0.5), None);
    }

    /// 作り物のルール：上位5色が35%以上（のっぺり）を弾くこと
    #[test]
    fn test_flat_colors_rule() {
        assert_eq!(with(|e| e.light.flat = 0.35), Some(Reason::FlatColors));
        assert_eq!(with(|e| e.light.flat = 0.349), None);
    }

    /// 作り物のルール：左右対称度0.6以上を弾くこと
    #[test]
    fn test_mirror_rule() {
        assert_eq!(with(|e| e.light.mirror = 0.6), Some(Reason::Mirror));
        assert_eq!(with(|e| e.light.mirror = 0.599), None);
    }

    /// 画面の上下の端の帯に文字の塊が1つでもあれば、塊が少なくても弾くこと（写真に重ねた短い見出し E_70507「Bend」のため。通過中の写真で端の帯に入るものは無かった）
    #[test]
    fn test_edge_text_rule() {
        assert_eq!(with(|e| e.ocr.ocr_edge = 1), Some(Reason::EdgeText));
        assert_eq!(with(|e| e.ocr.ocr_edge = 0), None);
    }

    /// 文字のルール：横長の塊の広さ1.1%以上を弾くこと
    #[test]
    fn test_text_area_rule() {
        assert_eq!(with(|e| e.light.text_area = 0.011), Some(Reason::Text));
        assert_eq!(with(|e| e.light.text_area = 0.0109), None);
    }

    /// 文字のルール：文字検出で横長の列1つ以上か塊3つ以上を弾くこと
    #[test]
    fn test_text_ocr_rule() {
        assert_eq!(with(|e| e.ocr.ocr_lines = 1), Some(Reason::TextOcr));
        assert_eq!(with(|e| e.ocr.ocr_n = 3), Some(Reason::TextOcr));
        assert_eq!(with(|e| e.ocr.ocr_n = 2), None);
    }

    /// 構図のルール：空が5%未満でなめらかな部分が30%未満のときの近景の塊35%以上（試作は40%。読み込みの差し替え後に試作との一致が最多になる値）を弾くこと
    #[test]
    fn test_near_blob_rule() {
        let near = |blob: f64, sky: f64, smooth: f64| {
            with(move |e| {
                e.light.near_blob = blob;
                e.light.sky = sky;
                e.light.smooth_share = smooth;
            })
        };
        assert_eq!(near(0.35, 0.049, 0.299), Some(Reason::NearBlob));
        assert_eq!(near(0.349, 0.0, 0.0), None);
        assert_eq!(near(0.35, 0.05, 0.0), None);
        assert_eq!(near(0.35, 0.0, 0.3), None);
    }

    /// 構図のルール：真ん中の近景が左右より30ポイント以上多いときを弾くこと
    #[test]
    fn test_center_subject_rule() {
        let center = |c: f64, s: f64| {
            with(move |e| {
                e.light.center_near = c;
                e.light.side_near = s;
            })
        };
        assert_eq!(center(0.5, 0.2), Some(Reason::CenterSubject));
        assert_eq!(center(0.5, 0.21), None);
    }

    /// 人のルール：YuNet の顔（2つ以上か広さ0.2%以上）があれば弾くこと（全身の HOG・上半身の Haar は誤検出が多いので外した。正面の顔の rustface は、YuNet だけで落とせない画像が全データで0件だったので外した）
    #[test]
    fn test_people_rules() {
        assert_eq!(with(|e| e.faces.yn_count = 2), Some(Reason::FaceYunet));
        assert_eq!(
            with(|e| {
                e.faces.yn_count = 1;
                e.faces.yn_max_area = 0.002
            }),
            Some(Reason::FaceYunet)
        );
        assert_eq!(
            with(|e| {
                e.faces.yn_count = 1;
                e.faces.yn_max_area = 0.0019
            }),
            None
        );
    }

    /// 物のルール：確からしさ0.6以上の物、自然にありがちな物以外で0.5以上の物を弾き、自然にありがちな物（鳥・牛・鉢植えなど）は0.6未満なら通すこと
    #[test]
    fn test_object_rule() {
        assert_eq!(
            with(|e| e.objects = vec![object("bird", 0.6, 0.0)]),
            Some(Reason::Object)
        );
        assert_eq!(with(|e| e.objects = vec![object("bird", 0.59, 0.0)]), None);
        assert_eq!(
            with(|e| e.objects = vec![object("car", 0.5, 0.0)]),
            Some(Reason::Object)
        );
        assert_eq!(with(|e| e.objects = vec![object("car", 0.49, 0.0)]), None);
    }

    /// 大きな物のルール：人・動物・車・傘などが確からしさ0.4以上で画面の2割以上なら弾くこと
    #[test]
    fn test_big_object_rule() {
        assert_eq!(
            with(|e| e.objects = vec![object("umbrella", 0.4, 0.2)]),
            Some(Reason::BigObject)
        );
        assert_eq!(
            with(|e| e.objects = vec![object("umbrella", 0.4, 0.199)]),
            None
        );
        assert_eq!(
            with(|e| e.objects = vec![object("umbrella", 0.39, 0.5)]),
            None
        );
        assert_eq!(with(|e| e.objects = vec![object("bench", 0.45, 0.5)]), None);
    }

    /// 自然にありがちな物（鳥・キリン・ゾウ・シマウマ・クマ・ヒツジ・ウシ・ウマ・鉢植え・ブロッコリー・バナナ・リンゴ・オレンジ・凧）は確からしさ0.5台なら通し、それ以外の物は弾くこと
    #[test]
    fn test_wild_object_list() {
        let wild = [
            "bird",
            "giraffe",
            "elephant",
            "zebra",
            "bear",
            "sheep",
            "cow",
            "horse",
            "potted_plant",
            "broccoli",
            "banana",
            "apple",
            "orange",
            "kite",
        ];
        for name in COCO {
            let expected = if wild.contains(&name) {
                None
            } else {
                Some(Reason::Object)
            };
            assert_eq!(
                with(|e| e.objects = vec![object(name, 0.55, 0.0)]),
                expected,
                "{name}"
            );
        }
    }

    /// 大きな物のルールの対象は、人・犬・猫・ウマ・ウシ・ヒツジ・クマ・ゾウ・キリン・シマウマ・ぬいぐるみ・凧・傘・消火栓・飛行機・車・鉢植えであること（対象外の鳥は、あとの生き物のルールで弾かれる）
    #[test]
    fn test_big_object_list() {
        let big = [
            "person",
            "dog",
            "cat",
            "horse",
            "cow",
            "sheep",
            "bear",
            "elephant",
            "giraffe",
            "zebra",
            "teddy_bear",
            "kite",
            "umbrella",
            "fire_hydrant",
            "airplane",
            "car",
            "potted_plant",
        ];
        for name in COCO {
            let expected = if big.contains(&name) {
                Some(Reason::BigObject)
            } else if ["bird"].contains(&name) {
                Some(Reason::Animal)
            } else {
                None
            };
            assert_eq!(
                with(|e| e.objects = vec![object(name, 0.45, 0.25)]),
                expected,
                "{name}"
            );
        }
    }

    /// 生き物のルールの対象は、鳥・猫・犬・ウマ・ヒツジ・ウシ・ゾウ・クマ・シマウマ・キリン・ぬいぐるみであること（人は、その前の人のルールで弾かれる）
    #[test]
    fn test_animal_list() {
        let animals = [
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
            "teddy_bear",
        ];
        for name in COCO {
            let expected = if name == "person" {
                Some(Reason::PersonObject)
            } else {
                animals.contains(&name).then_some(Reason::Animal)
            };
            assert_eq!(
                with(|e| e.objects = vec![object(name, 0.35, 0.1)]),
                expected,
                "{name}"
            );
        }
    }

    /// 人（物体検出）のルール：人が確からしさ0.35以上で画面の2%以上なら弾くこと
    #[test]
    fn test_person_object_rule() {
        assert_eq!(
            with(|e| e.objects = vec![object("person", 0.35, 0.02)]),
            Some(Reason::PersonObject)
        );
        assert_eq!(
            with(|e| e.objects = vec![object("person", 0.35, 0.019)]),
            None
        );
        assert_eq!(
            with(|e| e.objects = vec![object("person", 0.349, 0.5)]),
            None
        );
    }

    /// 生き物のルール：確からしさ0.35以上の生き物の広さを足して画面の1割以上なら弾き、生き物でない物は足さないこと
    #[test]
    fn test_animal_rule() {
        assert_eq!(
            with(|e| e.objects = vec![object("bird", 0.35, 0.05), object("dog", 0.4, 0.05)]),
            Some(Reason::Animal)
        );
        assert_eq!(
            with(|e| e.objects = vec![object("bird", 0.35, 0.05), object("dog", 0.34, 0.05)]),
            None
        );
        assert_eq!(
            with(|e| e.objects = vec![object("bird", 0.35, 0.05), object("kite", 0.4, 0.05)]),
            None
        );
    }

    /// 人工物のルール：舗装が2割以上を弾くこと
    #[test]
    fn test_pavement_rule() {
        assert_eq!(with(|e| e.light.ground_gray = 0.2), Some(Reason::Pavement));
        assert_eq!(with(|e| e.light.ground_gray = 0.199), None);
    }

    /// 人工物のルール：直角5個以上、縦の線20本以上、縦横の線50本以上かつ縦8本以上、長い直線（縦と斜めで6本以上か横7本以上）を弾くこと
    #[test]
    fn test_manmade_lines_rule() {
        assert_eq!(with(|e| e.light.corners = 5), Some(Reason::Manmade));
        assert_eq!(with(|e| e.light.corners = 4), None);
        assert_eq!(with(|e| e.light.seg_v = 20), Some(Reason::Manmade));
        assert_eq!(with(|e| e.light.seg_v = 19), None);
        let hv = |hv: u32, v: u32| {
            with(move |e| {
                e.light.seg_hv = hv;
                e.light.seg_v = v;
            })
        };
        assert_eq!(hv(50, 8), Some(Reason::Manmade));
        assert_eq!(hv(49, 8), None);
        assert_eq!(hv(50, 7), None);
        let lines = |v: u32, d: u32, h: u32| {
            with(move |e| {
                e.light.vert = v;
                e.light.diag = d;
                e.light.horiz = h;
            })
        };
        assert_eq!(lines(3, 3, 0), Some(Reason::Manmade));
        assert_eq!(lines(3, 2, 0), None);
        assert_eq!(lines(0, 0, 7), Some(Reason::Manmade));
        assert_eq!(lines(0, 0, 6), None);
    }

    /// 色と模様のルール：輪郭がほぼ無い（0.5%未満）画像を弾くこと
    #[test]
    fn test_no_texture_rule() {
        assert_eq!(with(|e| e.light.edge = 0.0049), Some(Reason::NoTexture));
        assert_eq!(with(|e| e.light.edge = 0.005), None);
    }

    /// 色と模様のルール：緑＋青が20%未満か、25%未満で土の色を足しても40%未満の画像を弾くこと（緑＋青がちょうど20%で土の色が多ければ通す）
    #[test]
    fn test_nature_colors_rule() {
        let colors = |g: f64, b: f64, earth: f64| {
            with(move |e| {
                e.light.green = g;
                e.light.blue = b;
                e.light.earth = earth;
            })
        };
        assert_eq!(colors(0.1, 0.099, 0.5), Some(Reason::TooFewNatureColors));
        assert_eq!(colors(0.12, 0.1, 0.17), Some(Reason::TooFewNatureColors));
        assert_eq!(colors(0.12, 0.1, 0.18), None);
        assert_eq!(colors(0.15, 0.1, 0.0), None);
        assert_eq!(colors(0.1, 0.1, 0.5), None);
    }

    /// 場面のルール：CLIP で「街・建物・部屋・絵」などの文章に寄る割合が0.5以上なら弾くこと（試作は Places365 で、自然の場所らしさ0.6未満か屋外らしさ0.85未満。CLIP の方が湖や渓流を巻き込まず、電柱や歩道を弾けたので置き換えた）
    #[test]
    fn test_scene_rule() {
        assert_eq!(with(|e| e.manmade_p = 0.5), Some(Reason::ManmadeScene));
        assert_eq!(with(|e| e.manmade_p = 0.4999), None);
    }

    /// 当てはまるルールが複数あるときは、軽い計算のルール（その中は試作と同じ順番）→物→場面→文字検出→顔の順で、最初に当てはまったものを理由にすること（重い測定を後回しにして、軽いルールで落ちる画像には重い測定をしないため）
    #[test]
    fn test_reason_follows_light_first_order() {
        assert_eq!(
            with(|e| {
                e.light.aspect = 4.0;
                e.light.blur = 0.0;
                e.manmade_p = 1.0
            }),
            Some(Reason::Elongated)
        );
        assert_eq!(
            with(|e| {
                e.light.horiz = 9;
                e.ocr.ocr_lines = 1
            }),
            Some(Reason::Manmade)
        );
        assert_eq!(
            with(|e| {
                e.objects = vec![object("car", 0.9, 0.1)];
                e.manmade_p = 1.0
            }),
            Some(Reason::Object)
        );
        assert_eq!(
            with(|e| {
                e.manmade_p = 1.0;
                e.ocr.ocr_lines = 1
            }),
            Some(Reason::ManmadeScene)
        );
        assert_eq!(
            with(|e| {
                e.ocr.ocr_lines = 1;
                e.faces.yn_count = 2
            }),
            Some(Reason::TextOcr)
        );
    }

    /// 距離のルール：模様の粗さ0.89以上（近すぎる。試作は0.9。読み込みの差し替え後に試作との一致が最多になる値）を弾くこと
    #[test]
    fn test_too_close_rule() {
        assert_eq!(late_with(|l| l.near.rt_min = 0.89), Some(Reason::TooClose));
        assert_eq!(late_with(|l| l.near.rt_min = 0.889), None);
        assert_eq!(
            late_with(|l| {
                l.near.rt_min = 0.89;
                l.sky = 0.1999;
            }),
            Some(Reason::TooClose)
        );
    }

    /// 距離のルール：上部のなめらかさ0.1未満のときの線の密度4.5以上（近すぎる・線）を弾くこと
    #[test]
    fn test_too_close_lines_rule() {
        let lines = |ln: f64, top: f64| {
            late_with(move |l| {
                l.near.ln_count = Some(ln);
                l.near.top_smooth = top;
            })
        };
        assert_eq!(lines(4.5, 0.099), Some(Reason::TooCloseLines));
        assert_eq!(lines(4.49, 0.0), None);
        assert_eq!(lines(4.5, 0.1), None);
    }

    /// 前半の数値のどれか1つでも計算できなかった（NaN の）ときは「未測定」として弾き、その数値と同じ測定のルールより先に判定すること（比べても当てはまらず、黙って通ってしまうのを防ぐため）
    #[test]
    fn test_nan_in_any_early_value_is_unmeasured() {
        let changes: Vec<Change<Early>> = vec![
            Box::new(|e| e.light.blur = f64::NAN),
            Box::new(|e| e.light.flat = f64::NAN),
            Box::new(|e| e.light.text_area = f64::NAN),
            Box::new(|e| e.light.edge = f64::NAN),
            Box::new(|e| e.light.green = f64::NAN),
            Box::new(|e| e.light.blue = f64::NAN),
            Box::new(|e| e.light.earth = f64::NAN),
            Box::new(|e| e.light.sat_mean = f64::NAN),
            Box::new(|e| e.light.sat_hi = f64::NAN),
            Box::new(|e| e.light.sky = f64::NAN),
            Box::new(|e| e.light.near_blob = f64::NAN),
            Box::new(|e| e.light.center_near = f64::NAN),
            Box::new(|e| e.light.side_near = f64::NAN),
            Box::new(|e| e.light.smooth_share = f64::NAN),
            Box::new(|e| e.light.dead_flat = f64::NAN),
            Box::new(|e| e.light.dead_flat_ns = f64::NAN),
            Box::new(|e| e.light.ground_gray = f64::NAN),
            Box::new(|e| e.light.mirror = f64::NAN),
            Box::new(|e| e.light.aspect = f64::NAN),
            Box::new(|e| e.light.colorfulness = f64::NAN),
            Box::new(|e| e.faces.yn_max_area = f64::NAN),
            Box::new(|e| e.ocr.ocr_area = f64::NAN),
            Box::new(|e| e.manmade_p = f64::NAN),
            Box::new(|e| e.objects = vec![object("bird", f64::NAN, 0.0)]),
            Box::new(|e| e.objects = vec![object("bird", 0.1, f64::NAN)]),
        ];
        for (i, change) in changes.iter().enumerate() {
            let mut e = passing();
            change(&mut e);
            assert_eq!(judge_early(&e), Some(Reason::Unmeasured), "case {i}");
        }
        assert_eq!(
            with(|e| {
                e.light.flat = f64::NAN;
                e.light.blur = 0.0
            }),
            Some(Reason::Unmeasured)
        );
        assert_eq!(
            with(|e| {
                e.objects = vec![object("bird", f64::NAN, 0.0)];
                e.objects.push(object("car", 0.9, 0.1))
            }),
            Some(Reason::Unmeasured)
        );
    }

    /// 軽い計算のルールで弾く画像は、重い測定の数値が計算できなかった（NaN の）ときでも、軽いルールの理由で弾くこと（重い測定はしないので、計算できたかどうかは分からないため）
    #[test]
    fn test_light_rule_wins_over_unmeasured_heavy_values() {
        assert_eq!(
            with(|e| {
                e.light.aspect = 3.0;
                e.faces.yn_max_area = f64::NAN;
                e.manmade_p = f64::NAN
            }),
            Some(Reason::Elongated)
        );
    }

    /// 空が2割以上写っている写真は、模様が粗くても「近すぎる」では弾かないこと（模様のない空を近いと見誤り、夕焼けや雲の写真を落としていたため。全データで約26件が新しく通り、怪しいものの多くはマスのアニメ調で落ちる）
    #[test]
    fn test_sky_photos_are_not_too_close() {
        let sky = |v: f64| {
            late_with(move |l| {
                l.near.rt_min = 1.5;
                l.sky = v;
            })
        };
        assert_eq!(sky(0.2), None);
        assert_eq!(sky(0.1999), Some(Reason::TooClose));
    }

    /// 後半の数値のどれか1つでも計算できなかった（NaN の）ときは、どのルールより先に「未測定」として弾くこと
    #[test]
    fn test_nan_in_any_late_value_is_unmeasured() {
        let changes: Vec<Change<Late>> = vec![
            Box::new(|l| l.near.rt_min = f64::NAN),
            Box::new(|l| l.near.top_smooth = f64::NAN),
            Box::new(|l| l.near.ln_count = Some(f64::NAN)),
            Box::new(|l| l.clip.toon_p = f64::NAN),
            Box::new(|l| l.clip.close_p = f64::NAN),
            Box::new(|l| l.clip.window_p = f64::NAN),
            Box::new(|l| l.sky = f64::NAN),
        ];
        for (i, change) in changes.iter().enumerate() {
            let mut l = passing_late();
            change(&mut l);
            l.near.rt_min = if l.near.rt_min.is_nan() {
                l.near.rt_min
            } else {
                1.0
            };
            assert_eq!(judge_late(&l), Some(Reason::Unmeasured), "case {i}");
        }
    }

    /// 後半で当てはまるルールが複数あるときは、近すぎる→近すぎる（線）→アニメ調→庭・見下ろし→窓ごしの順で理由を決めること
    #[test]
    fn test_late_reasons_in_trial_order() {
        let all = |l: &mut Late| {
            l.near.rt_min = 1.0;
            l.near.ln_count = Some(9.0);
            l.near.top_smooth = 0.0;
            l.clip.toon_p = 0.9;
            l.clip.close_p = 0.999;
            l.clip.window_p = 0.9;
        };
        assert_eq!(late_with(all), Some(Reason::TooClose));
        assert_eq!(
            late_with(|l| {
                all(l);
                l.near.rt_min = 0.0
            }),
            Some(Reason::TooCloseLines)
        );
        assert_eq!(
            late_with(|l| {
                all(l);
                l.near.rt_min = 0.0;
                l.near.ln_count = Some(0.0)
            }),
            Some(Reason::Toon)
        );
        assert_eq!(
            late_with(|l| {
                all(l);
                l.near.rt_min = 0.0;
                l.near.ln_count = Some(0.0);
                l.clip.toon_p = 0.0
            }),
            Some(Reason::GardenOrLookingDown)
        );
        assert_eq!(
            late_with(|l| {
                all(l);
                l.near.rt_min = 0.0;
                l.near.ln_count = Some(0.0);
                l.clip.toon_p = 0.0;
                l.clip.close_p = 0.0
            }),
            Some(Reason::ThroughWindow)
        );
    }

    /// 窓ごしのルール：「車の中から撮った写真」側の割合が0.6以上なら弾くこと（車の窓ごしに見た巨人の絵 A_73365 0.88 や雨粒の窓ごしの木 A_13821 0.84 を落とすため。残す通過写真の最大は車窓の木々 A_66191 の0.30）
    #[test]
    fn test_window_rule() {
        assert_eq!(
            late_with(|l| l.clip.window_p = 0.6),
            Some(Reason::ThroughWindow)
        );
        assert_eq!(late_with(|l| l.clip.window_p = 0.5999), None);
    }

    /// 線の密度が測れなかった画像は「未測定」として弾くこと（測れないものを黙って通さないため）
    #[test]
    fn test_unmeasured_line_density_is_rejected() {
        assert_eq!(
            late_with(|l| l.near.ln_count = None),
            Some(Reason::Unmeasured)
        );
    }

    /// 3×3 のマスのどれかでアニメ調の割合が0.25以上なら弾き、測れなければ「未測定」にすること（景色の中に小さく写ったアニメのぬいぐるみやグッズ、アニメ絵の川 E_73700・ゲーム CG の温泉 E_12526 を捕まえるため。マスにはアニメ調の文章に「ぬいぐるみ」「ゲームの場面・CG・アニメの場面」を足して聞く。良い写真も数枚落ちるが、線が引けないときは除外を優先する方針）
    #[test]
    fn test_tile_rule() {
        assert_eq!(judge_tiles(0.25), Some(Reason::ToonTile));
        assert_eq!(judge_tiles(0.2499), None);
        assert_eq!(judge_tiles(f64::NAN), Some(Reason::Unmeasured));
    }

    /// 画面の下半分にまとまって目立つ物の広さが合計2.5%以上なら弾き、測れなければ「未測定」にすること（景色の手前に置いたぬいぐるみや物を捕まえるため。通過した良い写真の最大1.95%と、すり抜けたぬいの最小2.91%の間をとった値。手前の岩場や桟橋のような良い写真も少し落ちるが、安全な方を優先する方針）
    #[test]
    fn test_foreground_rule() {
        assert_eq!(judge_foreground(0.025), Some(Reason::ForegroundObject));
        assert_eq!(judge_foreground(0.0249), None);
        assert_eq!(judge_foreground(f64::NAN), Some(Reason::Unmeasured));
    }

    /// CLIP のルール：アニメ調の割合0.294以上を弾くこと（アニメ調は TinyCLIP 8M で試作との一致が最多になる値、試作の MobileCLIP では0.5）
    #[test]
    fn test_toon_rule() {
        assert_eq!(late_with(|l| l.clip.toon_p = 0.294), Some(Reason::Toon));
        assert_eq!(late_with(|l| l.clip.toon_p = 0.2939), None);
    }

    /// CLIP のルール：庭・見下ろしの割合0.7以上を弾くこと（庭・見下ろしは「少し遠景であること」の方針で、木の幹や茂みの大写しを落とすため試作由来の0.884から下げた）
    #[test]
    fn test_garden_rule() {
        assert_eq!(
            late_with(|l| l.clip.close_p = 0.7),
            Some(Reason::GardenOrLookingDown)
        );
        assert_eq!(late_with(|l| l.clip.close_p = 0.6999), None);
    }
}
