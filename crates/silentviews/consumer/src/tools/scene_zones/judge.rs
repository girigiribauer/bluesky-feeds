use super::Zones;
use crate::reason::{any_nan, first_hit, Reason};

pub fn judge(z: &Zones, smooth_share: f64) -> Option<Reason> {
    if any_nan(&[z.sky, z.near_blob, z.center_near, z.side_near, smooth_share]) {
        return Some(Reason::Unmeasured);
    }
    first_hit([
        (
            Reason::NearBlob,
            z.near_blob >= 0.35 && z.sky < 0.05 && smooth_share < 0.3,
        ),
        (Reason::CenterSubject, z.center_near - z.side_near >= 0.3),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zones() -> Zones {
        Zones {
            sky: 0.0,
            near_blob: 0.1,
            center_near: 0.1,
            side_near: 0.1,
        }
    }

    /// 構図のルール：空が5%未満でなめらかな部分が30%未満のときの近景の塊35%以上（試作は40%。読み込みの差し替え後に試作との一致が最多になる値）を弾くこと
    #[test]
    fn test_near_blob_rule() {
        let near = |near_blob: f64, sky: f64, smooth: f64| {
            judge(
                &Zones {
                    near_blob,
                    sky,
                    ..zones()
                },
                smooth,
            )
        };
        assert_eq!(near(0.35, 0.049, 0.299), Some(Reason::NearBlob));
        assert_eq!(near(0.349, 0.0, 0.0), None);
        assert_eq!(near(0.35, 0.05, 0.0), None);
        assert_eq!(near(0.35, 0.0, 0.3), None);
    }

    /// 構図のルール：真ん中の近景が左右より30ポイント以上多いときを弾くこと
    #[test]
    fn test_center_subject_rule() {
        let center = |center_near: f64, side_near: f64| {
            judge(
                &Zones {
                    center_near,
                    side_near,
                    ..zones()
                },
                0.1,
            )
        };
        assert_eq!(center(0.5, 0.2), Some(Reason::CenterSubject));
        assert_eq!(center(0.5, 0.21), None);
    }

    /// 区分けの値かなめらかな部分の割合のどれか1つでも計算できなかった（NaN の）ときは「未測定」として弾くこと（比べても当てはまらず、黙って通ってしまうのを防ぐため）
    #[test]
    fn test_nan_is_unmeasured() {
        let cases = [
            Zones {
                sky: f64::NAN,
                ..zones()
            },
            Zones {
                near_blob: f64::NAN,
                ..zones()
            },
            Zones {
                center_near: f64::NAN,
                ..zones()
            },
            Zones {
                side_near: f64::NAN,
                ..zones()
            },
        ];
        for (i, z) in cases.iter().enumerate() {
            assert_eq!(judge(z, 0.1), Some(Reason::Unmeasured), "case {i}");
        }
        assert_eq!(judge(&zones(), f64::NAN), Some(Reason::Unmeasured));
    }
}
