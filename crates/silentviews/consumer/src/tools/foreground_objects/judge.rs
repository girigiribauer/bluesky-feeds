use crate::reason::Reason;

pub fn judge(lower_object_share: f64) -> Option<Reason> {
    if lower_object_share.is_nan() {
        return Some(Reason::Unmeasured);
    }
    (lower_object_share >= 0.14).then_some(Reason::ForegroundObject)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 画面の下半分にまとまって目立つ物の広さが合計14%以上なら弾き、測れなければ「未測定」にすること（画面の主役になるほど大きく手前に写った物だけを弾くため。画面の約1/3を占めるぬい N_05230 18.3% は弾き、ボーダーのぬい N_bear 10.7% と手前の物だけに弾かれていた良い写真の最大 H_97199 10.8% は通す）
    #[test]
    fn test_foreground_rule() {
        assert_eq!(judge(0.14), Some(Reason::ForegroundObject));
        assert_eq!(judge(0.1399), None);
        assert_eq!(judge(f64::NAN), Some(Reason::Unmeasured));
    }
}
