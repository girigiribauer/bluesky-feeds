use super::Ocr;
use crate::reason::{first_hit, Reason};

pub fn judge(o: &Ocr) -> Option<Reason> {
    if o.ocr_area.is_nan() {
        return Some(Reason::Unmeasured);
    }
    first_hit([
        (Reason::TextOcr, o.ocr_lines >= 1 || o.ocr_n >= 3),
        (Reason::EdgeText, o.ocr_edge >= 1),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn none() -> Ocr {
        Ocr {
            ocr_n: 0,
            ocr_area: 0.0,
            ocr_lines: 0,
            ocr_edge: 0,
        }
    }

    /// 文字のルール：文字検出で横長の列1つ以上か塊3つ以上を弾くこと
    #[test]
    fn test_text_ocr_rule() {
        assert_eq!(
            judge(&Ocr {
                ocr_lines: 1,
                ..none()
            }),
            Some(Reason::TextOcr)
        );
        assert_eq!(judge(&Ocr { ocr_n: 3, ..none() }), Some(Reason::TextOcr));
        assert_eq!(judge(&Ocr { ocr_n: 2, ..none() }), None);
    }

    /// 画面の上下の端の帯に文字の塊が1つでもあれば、塊が少なくても弾くこと（写真に重ねた短い見出し E_70507「Bend」のため。通過中の写真で端の帯に入るものは無かった）
    #[test]
    fn test_edge_text_rule() {
        assert_eq!(
            judge(&Ocr {
                ocr_edge: 1,
                ..none()
            }),
            Some(Reason::EdgeText)
        );
        assert_eq!(judge(&none()), None);
    }

    /// 文字の広さが計算できなかった（NaN の）ときは「未測定」として弾くこと（比べても当てはまらず、黙って通ってしまうのを防ぐため）
    #[test]
    fn test_nan_is_unmeasured() {
        assert_eq!(
            judge(&Ocr {
                ocr_area: f64::NAN,
                ..none()
            }),
            Some(Reason::Unmeasured)
        );
    }
}
