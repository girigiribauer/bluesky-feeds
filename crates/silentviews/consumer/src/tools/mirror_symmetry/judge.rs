use crate::reason::Reason;

pub fn judge(mirror: f64) -> Option<Reason> {
    if mirror.is_nan() {
        return Some(Reason::Unmeasured);
    }
    (mirror >= 0.6).then_some(Reason::Mirror)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 作り物のルール：左右対称度0.6以上を弾くこと
    #[test]
    fn test_mirror_rule() {
        assert_eq!(judge(0.6), Some(Reason::Mirror));
        assert_eq!(judge(0.599), None);
    }

    /// 左右対称度が計算できなかった（NaN の）ときは「未測定」として弾くこと（比べても当てはまらず、黙って通ってしまうのを防ぐため）
    #[test]
    fn test_nan_is_unmeasured() {
        assert_eq!(judge(f64::NAN), Some(Reason::Unmeasured));
    }
}
