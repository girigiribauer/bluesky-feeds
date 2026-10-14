pub mod basic;
pub mod clip;
pub mod depth;
pub mod extra;
pub mod faces;
pub mod flat;
pub mod manmade;
pub mod near;
pub mod objects;
pub mod ocr;

use vision::img::Image;

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct LightFeatures {
    pub blur: f64,
    pub flat: f64,
    pub text_lines: u32,
    pub text_area: f64,
    pub horiz: u32,
    pub vert: u32,
    pub diag: u32,
    pub edge: f64,
    pub green: f64,
    pub blue: f64,
    pub earth: f64,
    pub sat_mean: f64,
    pub sat_hi: f64,
    pub sky: f64,
    pub near_blob: f64,
    pub center_near: f64,
    pub side_near: f64,
    pub smooth_share: f64,
    pub dead_flat: f64,
    pub dead_flat_ns: f64,
    pub ground_gray: f64,
    pub mirror: f64,
    pub aspect: f64,
    pub seg_hv: u32,
    pub seg_v: u32,
    pub corners: u32,
    pub vivid_hues: u32,
    pub colorfulness: f64,
}

pub fn measure_light(bgr: &Image, gray_decoded: &Image) -> anyhow::Result<LightFeatures> {
    anyhow::ensure!(
        bgr.ch == 3 && bgr.w > 0 && bgr.h > 0,
        "expected a color image"
    );
    anyhow::ensure!(
        gray_decoded.ch == 1 && gray_decoded.w > 0 && gray_decoded.h > 0,
        "expected a gray image"
    );
    let b = basic::measure(bgr);
    let d = depth::measure(bgr)?;
    let f = flat::measure(bgr, gray_decoded);
    let e = extra::measure(bgr);
    let m = manmade::measure(bgr);
    Ok(LightFeatures {
        blur: b.blur,
        flat: b.flat,
        text_lines: b.text_lines,
        text_area: b.text_area,
        horiz: b.horiz,
        vert: b.vert,
        diag: b.diag,
        edge: b.edge,
        green: b.green,
        blue: b.blue,
        earth: b.earth,
        sat_mean: b.sat_mean,
        sat_hi: b.sat_hi,
        sky: d.sky,
        near_blob: d.near_blob,
        center_near: d.center_near,
        side_near: d.side_near,
        smooth_share: f.smooth_share,
        dead_flat: f.dead_flat,
        dead_flat_ns: f.dead_flat_ns,
        ground_gray: e.ground_gray,
        mirror: e.mirror,
        aspect: e.aspect,
        seg_hv: m.seg_hv,
        seg_v: m.seg_v,
        corners: m.corners,
        vivid_hues: m.vivid_hues,
        colorfulness: m.colorfulness,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cv::testing::{fixture, fixture_gray, gray_of, solid};

    /// まとめた結果の各項目が、それぞれの部品で測った同じ名前の値と一致すること（取り違えないこと）
    #[test]
    fn test_light_features_take_each_value_from_its_part() {
        let img = fixture("walking_person.jpg");
        let gray = fixture_gray("walking_person.jpg");

        let all = measure_light(&img, &gray).unwrap();
        let (b, d, f, e, m) = (
            basic::measure(&img),
            depth::measure(&img).unwrap(),
            flat::measure(&img, &gray),
            extra::measure(&img),
            manmade::measure(&img),
        );

        let expected = LightFeatures {
            blur: b.blur,
            flat: b.flat,
            text_lines: b.text_lines,
            text_area: b.text_area,
            horiz: b.horiz,
            vert: b.vert,
            diag: b.diag,
            edge: b.edge,
            green: b.green,
            blue: b.blue,
            earth: b.earth,
            sat_mean: b.sat_mean,
            sat_hi: b.sat_hi,
            sky: d.sky,
            near_blob: d.near_blob,
            center_near: d.center_near,
            side_near: d.side_near,
            smooth_share: f.smooth_share,
            dead_flat: f.dead_flat,
            dead_flat_ns: f.dead_flat_ns,
            ground_gray: e.ground_gray,
            mirror: e.mirror,
            aspect: e.aspect,
            seg_hv: m.seg_hv,
            seg_v: m.seg_v,
            corners: m.corners,
            vivid_hues: m.vivid_hues,
            colorfulness: m.colorfulness,
        };
        assert_eq!(all, expected);
    }

    /// 読み込めなかった（空の）画像は、止まらずに「測れない」として返すこと（判定に失敗した画像は載せないため）
    #[test]
    fn test_empty_image_is_an_error() {
        let empty = Image::new(0, 0, 3);
        assert!(measure_light(&empty, &Image::new(0, 0, 1)).is_err());
        assert!(crate::stage1::features(&empty).is_err());
        let models = crate::cv::testing::models();
        let mut faces = faces::FaceDetectors::load(models).unwrap();
        assert!(faces.measure(&empty).is_err());
        let mut objects = objects::ObjectDetector::load(models).unwrap();
        assert!(objects.detect(&empty).is_err());
        let mut ocr = ocr::TextDetector::load(models).unwrap();
        assert!(ocr.detect(&empty).is_err());
    }

    /// 1px の画像でも、止まらずに測れること
    #[test]
    fn test_single_pixel_image_is_measured() {
        let img = solid(1, 1, (120.0, 120.0, 120.0));
        assert!(measure_light(&img, &gray_of(&img)).is_ok());
    }
}
