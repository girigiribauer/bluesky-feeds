use crate::reason::{any_nan, Reason};

pub fn judge(paint_p: f64, grain: impl FnOnce() -> f64) -> Option<Reason> {
    if any_nan(&[paint_p]) {
        return Some(Reason::Unmeasured);
    }
    if paint_p >= 0.45 {
        return Some(Reason::Painting);
    }
    if paint_p < 0.1 {
        return None;
    }
    let g = grain();
    if any_nan(&[g]) {
        return Some(Reason::Unmeasured);
    }
    (g >= 4.0).then_some(Reason::Painting)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    /// CLIP で「写真」より「絵画」の文章に寄る割合が0.45以上なら、ざらつきに関係なく弾くこと（油絵 dG_362507 0.79・ケシの油絵 dH_158806 などを拾い、残る写真の巻き込みは色の濃い紅葉の湖 dE_38098・氷河 dB_22595 など4件にとどまる）
    #[test]
    fn test_painting_share_rule() {
        assert_eq!(judge(0.45, || 0.0), Some(Reason::Painting));
        assert_eq!(judge(0.4499, || 0.0), None);
    }

    /// 割合が0.1以上0.45未満でも、ざらつきが4.0以上なら弾くこと（CLIP が写真と見なした筆の絵 dH_214794 0.14・dG_85322 0.12 を、平らな所に乗る画布の目や絵の具の粒で拾うため。水辺の草 dG_53422 など写真4件を巻き込む）
    #[test]
    fn test_grain_rule() {
        assert_eq!(judge(0.1, || 4.0), Some(Reason::Painting));
        assert_eq!(judge(0.1, || 3.9999), None);
        assert_eq!(judge(0.4499, || 4.0), Some(Reason::Painting));
    }

    /// 割合が0.1未満なら、ざらつきが大きくても通すこと（ざらつきだけでは森や木の写真と絵を分けられないため）
    #[test]
    fn test_grain_alone_passes() {
        assert_eq!(judge(0.0999, || 100.0), None);
    }

    /// ざらつきは割合が0.1以上0.45未満のときだけ測ること（大きい画像を測り直す手間を、必要なときに限るため）
    #[test]
    fn test_grain_measured_only_when_needed() {
        let calls = Cell::new(0);
        let count = || {
            calls.set(calls.get() + 1);
            5.0
        };
        judge(0.45, count);
        judge(0.0999, count);
        assert_eq!(calls.get(), 0);
        judge(0.2, count);
        assert_eq!(calls.get(), 1);
    }

    /// 割合かざらつきが計算できなかった（NaN の）ときは「未測定」として弾くこと（比べても当てはまらず、黙って通ってしまうのを防ぐため）
    #[test]
    fn test_nan_is_unmeasured() {
        assert_eq!(judge(f64::NAN, || 0.0), Some(Reason::Unmeasured));
        assert_eq!(judge(0.2, || f64::NAN), Some(Reason::Unmeasured));
    }
}
