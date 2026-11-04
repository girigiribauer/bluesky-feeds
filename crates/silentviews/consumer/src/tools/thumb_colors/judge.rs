use super::ThumbColors;
use crate::reason::{first_hit, Reason};

pub fn judge(f: &ThumbColors) -> Option<Reason> {
    first_hit([
        (Reason::ThumbIllustration, f.dead >= 0.05),
        (Reason::ThumbFlatColors, f.top5 >= 0.40),
        (
            Reason::ThumbFewNatureColors,
            f.green + f.blue + f.earth < 0.25,
        ),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn passing() -> ThumbColors {
        ThumbColors {
            green: 0.3,
            blue: 0.1,
            earth: 0.1,
            top5: 0.1,
            dead: 0.0,
        }
    }

    /// どの条件にも当てはまらない画像は通すこと
    #[test]
    fn test_passing_features_pass() {
        assert_eq!(judge(&passing()), None);
    }

    /// なめらかな部分のうち、色の変化がまったく無い平らな塗りが 5% 以上なら、イラストとして弾くこと
    #[test]
    fn test_dead_flat_is_rejected_as_illustration() {
        let f = ThumbColors {
            dead: 0.05,
            ..passing()
        };
        assert_eq!(judge(&f), Some(Reason::ThumbIllustration));
    }

    /// よく使われている上位5色で画面の 40% 以上を占めるなら、色数の少ないベタ塗りとして弾くこと
    #[test]
    fn test_few_colors_are_rejected_as_flat() {
        let f = ThumbColors {
            top5: 0.40,
            ..passing()
        };
        assert_eq!(judge(&f), Some(Reason::ThumbFlatColors));
    }

    /// 緑・青・土の色が合わせて 25% 未満なら自然の色不足として弾くこと
    #[test]
    fn test_too_few_nature_colors_are_rejected() {
        let f = ThumbColors {
            green: 0.1,
            blue: 0.1,
            earth: 0.049,
            ..passing()
        };
        assert_eq!(judge(&f), Some(Reason::ThumbFewNatureColors));
    }

    /// 当てはまる条件が複数あるときは、試作と同じくイラスト→ベタ塗り→色不足の順で理由を決めること
    #[test]
    fn test_reasons_are_checked_in_trial_order() {
        let all_bad = ThumbColors {
            dead: 1.0,
            top5: 1.0,
            green: 0.0,
            blue: 0.0,
            earth: 0.0,
        };
        assert_eq!(judge(&all_bad), Some(Reason::ThumbIllustration));
    }
}
