use super::Nearness;
use crate::reason::{any_nan, first_hit, Reason};

pub fn judge(n: &Nearness, sky: f64) -> Option<Reason> {
    let Some(ln_count) = n.ln_count else {
        return Some(Reason::Unmeasured);
    };
    if any_nan(&[n.rt_min, n.top_smooth, ln_count, sky]) {
        return Some(Reason::Unmeasured);
    }
    first_hit([
        (Reason::TooClose, n.rt_min >= 0.89 && sky < 0.2),
        (Reason::TooCloseLines, ln_count >= 4.5 && n.top_smooth < 0.1),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn passing() -> Nearness {
        Nearness {
            rt_min: 0.5,
            ln_count: Some(1.0),
            top_smooth: 0.5,
        }
    }

    /// どのルールにも当てはまらない数値は通すこと
    #[test]
    fn test_passing_values_pass() {
        assert_eq!(judge(&passing(), 0.0), None);
    }

    /// 距離のルール：模様の粗さ0.89以上（近すぎる。試作は0.9。読み込みの差し替え後に試作との一致が最多になる値）を弾くこと
    #[test]
    fn test_too_close_rule() {
        let rough = |rt_min: f64, sky: f64| {
            judge(
                &Nearness {
                    rt_min,
                    ..passing()
                },
                sky,
            )
        };
        assert_eq!(rough(0.89, 0.0), Some(Reason::TooClose));
        assert_eq!(rough(0.889, 0.0), None);
        assert_eq!(rough(0.89, 0.1999), Some(Reason::TooClose));
    }

    /// 空が2割以上写っている写真は、模様が粗くても「近すぎる」では弾かないこと（模様のない空を近いと見誤り、夕焼けや雲の写真を落としていたため。全データで約26件が新しく通り、怪しいものの多くはマスのアニメ調で落ちる）
    #[test]
    fn test_sky_photos_are_not_too_close() {
        let sky = |v: f64| {
            judge(
                &Nearness {
                    rt_min: 1.5,
                    ..passing()
                },
                v,
            )
        };
        assert_eq!(sky(0.2), None);
        assert_eq!(sky(0.1999), Some(Reason::TooClose));
    }

    /// 距離のルール：上部のなめらかさ0.1未満のときの線の密度4.5以上（近すぎる・線）を弾くこと
    #[test]
    fn test_too_close_lines_rule() {
        let lines = |ln: f64, top: f64| {
            judge(
                &Nearness {
                    ln_count: Some(ln),
                    top_smooth: top,
                    ..passing()
                },
                0.0,
            )
        };
        assert_eq!(lines(4.5, 0.099), Some(Reason::TooCloseLines));
        assert_eq!(lines(4.49, 0.0), None);
        assert_eq!(lines(4.5, 0.1), None);
    }

    /// 近すぎるにも近すぎる（線）にも当てはまるときは、近すぎるを理由にすること（試作と同じ順番）
    #[test]
    fn test_too_close_comes_before_lines() {
        let both = Nearness {
            rt_min: 1.0,
            ln_count: Some(9.0),
            top_smooth: 0.0,
        };
        assert_eq!(judge(&both, 0.0), Some(Reason::TooClose));
    }

    /// 線の密度が測れなかった画像は「未測定」として弾くこと（測れないものを黙って通さないため）
    #[test]
    fn test_unmeasured_line_density_is_rejected() {
        let n = Nearness {
            ln_count: None,
            ..passing()
        };
        assert_eq!(judge(&n, 0.0), Some(Reason::Unmeasured));
    }

    /// 近さの数値か空の割合のどれか1つでも計算できなかった（NaN の）ときは、どのルールより先に「未測定」として弾くこと
    #[test]
    fn test_nan_is_unmeasured() {
        let cases = [
            Nearness {
                rt_min: f64::NAN,
                ..passing()
            },
            Nearness {
                top_smooth: f64::NAN,
                rt_min: 1.0,
                ..passing()
            },
            Nearness {
                ln_count: Some(f64::NAN),
                rt_min: 1.0,
                ..passing()
            },
        ];
        for (i, n) in cases.iter().enumerate() {
            assert_eq!(judge(n, 0.0), Some(Reason::Unmeasured), "case {i}");
        }
        assert_eq!(
            judge(
                &Nearness {
                    rt_min: 1.0,
                    ..passing()
                },
                f64::NAN
            ),
            Some(Reason::Unmeasured)
        );
    }
}
