use crate::reason::{any_nan, Reason};

pub fn judge(edge: f64, sky: f64) -> Option<Reason> {
    if any_nan(&[edge, sky]) {
        return Some(Reason::Unmeasured);
    }
    (edge < 0.005 && sky < 0.2).then_some(Reason::NoTexture)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 模様のルール：輪郭がほぼ無く（0.5%未満）、空の割合が0.2未満の画像を弾くこと
    #[test]
    fn test_no_texture_rule() {
        assert_eq!(judge(0.0049, 0.0), Some(Reason::NoTexture));
        assert_eq!(judge(0.005, 0.0), None);
    }

    /// 輪郭がほぼ無くても、空の割合が0.2以上なら通すこと（青いノイズ画像 G_283542 の空0.0は弾き、雲だけの空 H_115873 0.78・霧の湖 G_507567 0.39 は通すため）
    #[test]
    fn test_textureless_sky_passes() {
        assert_eq!(judge(0.0, 0.1999), Some(Reason::NoTexture));
        assert_eq!(judge(0.0, 0.2), None);
    }

    /// 輪郭の量か空の割合が計算できなかった（NaN の）ときは「未測定」として弾くこと（比べても当てはまらず、黙って通ってしまうのを防ぐため）
    #[test]
    fn test_nan_is_unmeasured() {
        assert_eq!(judge(f64::NAN, 0.0), Some(Reason::Unmeasured));
        assert_eq!(judge(0.1, f64::NAN), Some(Reason::Unmeasured));
    }
}
