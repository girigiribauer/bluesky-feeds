use crate::reason::Reason;

pub fn judge(text_area: f64) -> Option<Reason> {
    if text_area.is_nan() {
        return Some(Reason::Unmeasured);
    }
    (text_area >= 0.011).then_some(Reason::Text)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 文字のルール：横長の塊の広さ1.1%以上を弾くこと
    #[test]
    fn test_text_area_rule() {
        assert_eq!(judge(0.011), Some(Reason::Text));
        assert_eq!(judge(0.0109), None);
    }

    /// 塊の広さが計算できなかった（NaN の）ときは「未測定」として弾くこと（比べても当てはまらず、黙って通ってしまうのを防ぐため）
    #[test]
    fn test_nan_is_unmeasured() {
        assert_eq!(judge(f64::NAN), Some(Reason::Unmeasured));
    }
}
