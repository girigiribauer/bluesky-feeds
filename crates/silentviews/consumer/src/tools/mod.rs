pub mod clip_words;
pub mod edge_amount;
pub mod flat_areas;
pub mod foreground_objects;
pub mod mirror_symmetry;
pub mod nearness;
pub mod object_detector;
pub mod scene_zones;
pub mod text_detector;
pub mod text_texture;
pub mod thumb_colors;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cv::testing::{gray_of, models, solid};
    use vision::img::Image;

    /// 読み込めなかった（空の）画像は、モデルを使う道具でも止まらずに「測れない」として返すこと（判定に失敗した画像は載せないため）
    #[test]
    fn test_empty_image_is_an_error() {
        let empty = Image::new(0, 0, 3);
        let models = models();
        let mut objects = object_detector::ObjectDetector::load(models).unwrap();
        assert!(objects.detect(&empty).is_err());
        let mut ocr = text_detector::TextDetector::load(models).unwrap();
        assert!(ocr.detect(&empty).is_err());
    }

    /// 1px の画像でも、モデルを使わない道具は止まらずに測れること
    #[test]
    fn test_single_pixel_image_is_measured() {
        let img = solid(1, 1, (120.0, 120.0, 120.0));
        let gray = gray_of(&img);
        text_texture::measure(&img);
        edge_amount::measure(&img);
        flat_areas::measure(&img, &gray);
        assert!(scene_zones::measure(&img).is_ok());
        mirror_symmetry::measure(&img);
    }
}
