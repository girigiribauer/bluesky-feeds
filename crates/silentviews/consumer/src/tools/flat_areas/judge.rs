use super::FlatAreas;
use crate::reason::{any_nan, Reason};

pub fn judge(f: &FlatAreas) -> Option<Reason> {
    if any_nan(&[f.dead_flat, f.dead_flat_ns]) {
        return Some(Reason::Unmeasured);
    }
    (f.dead_flat >= 0.12 && f.dead_flat_ns >= 0.05).then_some(Reason::Illustration)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(dead_flat: f64, dead_flat_ns: f64) -> Option<Reason> {
        judge(&FlatAreas {
            smooth_share: 0.1,
            dead_flat,
            dead_flat_ns,
        })
    }

    /// 作り物のルール：平らな塗りが12%以上かつ空を除いても5%以上（イラスト）を弾くこと
    #[test]
    fn test_illustration_rule() {
        assert_eq!(flat(0.12, 0.05), Some(Reason::Illustration));
        assert_eq!(flat(0.12, 0.049), None);
        assert_eq!(flat(0.119, 0.5), None);
    }

    /// 平らな塗りの割合が計算できなかった（NaN の）ときは「未測定」として弾くこと（比べても当てはまらず、黙って通ってしまうのを防ぐため）
    #[test]
    fn test_nan_is_unmeasured() {
        assert_eq!(flat(f64::NAN, 0.0), Some(Reason::Unmeasured));
        assert_eq!(flat(0.0, f64::NAN), Some(Reason::Unmeasured));
    }
}
