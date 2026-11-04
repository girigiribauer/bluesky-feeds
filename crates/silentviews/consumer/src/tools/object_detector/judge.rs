use super::Object;
use crate::reason::{first_hit, Reason};

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

pub fn judge(objects: &[Object]) -> Option<Reason> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::object_detector::COCO;

    fn object(name: &str, conf: f64, area: f64) -> Object {
        Object {
            name: COCO.iter().find(|c| **c == name).unwrap(),
            conf,
            area,
        }
    }

    /// 物のルール：確からしさ0.6以上の物、自然にありがちな物以外で0.5以上の物を弾き、自然にありがちな物（鳥・牛・鉢植えなど）は0.6未満なら通すこと
    #[test]
    fn test_object_rule() {
        assert_eq!(judge(&[object("bird", 0.6, 0.0)]), Some(Reason::Object));
        assert_eq!(judge(&[object("bird", 0.59, 0.0)]), None);
        assert_eq!(judge(&[object("car", 0.5, 0.0)]), Some(Reason::Object));
        assert_eq!(judge(&[object("car", 0.49, 0.0)]), None);
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
            assert_eq!(judge(&[object(name, 0.55, 0.0)]), expected, "{name}");
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
            assert_eq!(judge(&[object(name, 0.35, 0.1)]), expected, "{name}");
        }
    }

    /// 人（物体検出）のルール：人が確からしさ0.35以上で画面の2%以上なら弾くこと
    #[test]
    fn test_person_object_rule() {
        assert_eq!(
            judge(&[object("person", 0.35, 0.02)]),
            Some(Reason::PersonObject)
        );
        assert_eq!(judge(&[object("person", 0.35, 0.019)]), None);
        assert_eq!(judge(&[object("person", 0.349, 0.5)]), None);
    }

    /// 生き物のルール：確からしさ0.35以上の生き物の広さを足して画面の1割以上なら弾き、生き物でない物は足さないこと
    #[test]
    fn test_animal_rule() {
        assert_eq!(
            judge(&[object("bird", 0.35, 0.05), object("dog", 0.4, 0.05)]),
            Some(Reason::Animal)
        );
        assert_eq!(
            judge(&[object("bird", 0.35, 0.05), object("dog", 0.34, 0.05)]),
            None
        );
        assert_eq!(
            judge(&[object("bird", 0.35, 0.05), object("kite", 0.4, 0.05)]),
            None
        );
    }

    /// 物の確からしさか広さが1つでも計算できなかった（NaN の）ときは、ほかの物が当てはまっても「未測定」として弾くこと（比べても当てはまらず、黙って通ってしまうのを防ぐため）
    #[test]
    fn test_nan_is_unmeasured() {
        assert_eq!(
            judge(&[object("bird", f64::NAN, 0.0)]),
            Some(Reason::Unmeasured)
        );
        assert_eq!(
            judge(&[object("bird", 0.1, f64::NAN)]),
            Some(Reason::Unmeasured)
        );
        assert_eq!(
            judge(&[object("bird", f64::NAN, 0.0), object("car", 0.9, 0.1)]),
            Some(Reason::Unmeasured)
        );
    }
}
