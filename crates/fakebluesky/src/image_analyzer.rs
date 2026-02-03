use anyhow::{Context, Result};
use image::{DynamicImage, GenericImageView};
use std::time::Duration;

/// Configuration for blue sky detection
#[derive(Debug, Clone)]
pub struct BlueDetectionConfig {
    /// Percentage of top pixels to analyze (0.0 - 1.0)
    pub top_percentage: f32,
    /// Threshold for blue pixel ratio (0.0 - 1.0)
    pub blue_threshold: f32,
    /// Ratio for B > R and B > G (e.g., 1.2 means B > R * 1.2)
    pub rgb_blue_ratio: f32,
    /// Minimum blue value (0-255)
    pub min_blue_value: u8,
    /// Maximum image width for resizing
    pub max_width: u32,
}

impl Default for BlueDetectionConfig {
    fn default() -> Self {
        Self {
            top_percentage: 0.3,
            blue_threshold: 0.5,
            rgb_blue_ratio: 1.2,
            min_blue_value: 100,
            max_width: 600,
        }
    }
}

/// Check if an image is a blue sky image
pub async fn is_blue_sky_image(
    image_url: &str,
    config: &BlueDetectionConfig,
) -> Result<bool> {
    // Download and resize image with timeout
    let image = tokio::time::timeout(
        Duration::from_secs(5),
        download_and_resize_image(image_url, config.max_width),
    )
    .await
    .context("Image download timeout")?
    .context("Failed to download image")?;

    // Analyze top pixels
    Ok(analyze_top_pixels(&image, config))
}

/// Download image from URL and resize if needed
async fn download_and_resize_image(url: &str, max_width: u32) -> Result<DynamicImage> {
    // Download image
    let response = reqwest::get(url)
        .await
        .context("Failed to fetch image")?;

    let bytes = response
        .bytes()
        .await
        .context("Failed to read image bytes")?;

    // Decode image
    let img = image::load_from_memory(&bytes).context("Failed to decode image")?;

    // Resize if needed
    let (width, height) = img.dimensions();
    if width > max_width {
        let new_height = (height as f32 * (max_width as f32 / width as f32)) as u32;
        Ok(img.resize(max_width, new_height, image::imageops::FilterType::Triangle))
    } else {
        Ok(img)
    }
}

/// Analyze top pixels of image to detect blue sky
fn analyze_top_pixels(image: &DynamicImage, config: &BlueDetectionConfig) -> bool {
    let (width, height) = image.dimensions();
    let top_height = (height as f32 * config.top_percentage) as u32;

    if top_height == 0 {
        return false;
    }

    let mut total_pixels = 0;
    let mut blue_pixels = 0;

    // Analyze top portion of image
    for y in 0..top_height {
        for x in 0..width {
            let pixel = image.get_pixel(x, y);
            let r = pixel[0];
            let g = pixel[1];
            let b = pixel[2];

            total_pixels += 1;

            // Check if pixel is "blue"
            if is_blue_pixel(r, g, b, config) {
                blue_pixels += 1;
            }
        }
    }

    // Calculate blue ratio
    let blue_ratio = blue_pixels as f32 / total_pixels as f32;
    blue_ratio >= config.blue_threshold
}

/// Check if a single pixel is considered "blue"
fn is_blue_pixel(r: u8, g: u8, b: u8, config: &BlueDetectionConfig) -> bool {
    let r_f = r as f32;
    let g_f = g as f32;
    let b_f = b as f32;

    b >= config.min_blue_value
        && b_f > r_f * config.rgb_blue_ratio
        && b_f > g_f * config.rgb_blue_ratio
}

#[cfg(test)]
mod tests {
    use super::*;

    /// テスト観点: 青色ピクセル判定の基本動作
    /// - 青色条件を満たすピクセルが正しく判定される
    /// - B値が低い、R/G優位の場合は青色と判定されない
    #[test]
    fn test_is_blue_pixel() {
        let config = BlueDetectionConfig::default();

        assert!(is_blue_pixel(50, 50, 150, &config));
        assert!(!is_blue_pixel(50, 50, 80, &config));
        assert!(!is_blue_pixel(150, 50, 120, &config));
        assert!(!is_blue_pixel(50, 150, 120, &config));
    }

    /// テスト観点: 青色ピクセル判定の境界値テスト
    /// - 閾値ちょうどの値での動作
    /// - 完全な青色、グレーなどの極端なケース
    #[test]
    fn test_is_blue_pixel_edge_cases() {
        let config = BlueDetectionConfig::default();

        assert!(is_blue_pixel(50, 50, 100, &config));
        assert!(!is_blue_pixel(100, 100, 110, &config));
        assert!(is_blue_pixel(0, 0, 255, &config));
        assert!(!is_blue_pixel(150, 150, 150, &config));
    }

    /// テスト観点: 画像全体が青色の場合の判定
    /// - 上位30%が全て青色ピクセルの場合、青空画像と判定される
    #[test]
    fn test_analyze_top_pixels_all_blue() {
        let img = DynamicImage::new_rgb8(10, 10);
        let mut img = img.to_rgb8();
        for pixel in img.pixels_mut() {
            *pixel = image::Rgb([50, 50, 200]);
        }
        let img = DynamicImage::ImageRgb8(img);

        let config = BlueDetectionConfig::default();
        assert!(analyze_top_pixels(&img, &config));
    }

    /// テスト観点: 青色ピクセルが無い場合の判定
    /// - 青色ピクセルが存在しない場合、青空画像と判定されない
    #[test]
    fn test_analyze_top_pixels_no_blue() {
        let img = DynamicImage::new_rgb8(10, 10);
        let mut img = img.to_rgb8();
        for pixel in img.pixels_mut() {
            *pixel = image::Rgb([200, 50, 50]);
        }
        let img = DynamicImage::ImageRgb8(img);

        let config = BlueDetectionConfig::default();
        assert!(!analyze_top_pixels(&img, &config));
    }

    /// テスト観点: 部分的に青色が含まれる画像の判定
    /// - 上半分が青、下半分が赤の場合、上位30%は全て青なので青空と判定される
    #[test]
    fn test_analyze_top_pixels_partial_blue() {
        let img = DynamicImage::new_rgb8(10, 10);
        let mut img = img.to_rgb8();
        for y in 0..10 {
            for x in 0..10 {
                let pixel = if y < 5 {
                    image::Rgb([50, 50, 200])
                } else {
                    image::Rgb([200, 50, 50])
                };
                img.put_pixel(x, y, pixel);
            }
        }
        let img = DynamicImage::ImageRgb8(img);

        let config = BlueDetectionConfig::default();
        assert!(analyze_top_pixels(&img, &config));
    }

    /// テスト観点: 青色比率が閾値未満の場合の判定
    /// - 上位30%のうち33%が青色（閾値50%未満）の場合、青空と判定されない
    #[test]
    fn test_analyze_top_pixels_threshold() {
        let img = DynamicImage::new_rgb8(10, 10);
        let mut img = img.to_rgb8();

        for y in 0..10 {
            for x in 0..10 {
                let pixel = if y == 0 {
                    image::Rgb([50, 50, 200])
                } else {
                    image::Rgb([200, 50, 50])
                };
                img.put_pixel(x, y, pixel);
            }
        }
        let img = DynamicImage::ImageRgb8(img);

        let config = BlueDetectionConfig::default();
        assert!(!analyze_top_pixels(&img, &config));
    }

    /// テスト観点: カスタム設定での動作確認
    /// - デフォルト以外の閾値・比率での正しい判定
    #[test]
    fn test_config_custom_values() {
        let config = BlueDetectionConfig {
            top_percentage: 0.5,
            blue_threshold: 0.3,
            rgb_blue_ratio: 1.5,
            min_blue_value: 120,
            max_width: 800,
        };

        assert!(is_blue_pixel(80, 80, 130, &config));
        assert!(!is_blue_pixel(80, 80, 115, &config));
    }

    /// テスト観点: 高さ0の画像のエッジケース
    /// - 高さ0の場合、青空と判定されない（クラッシュしない）
    #[test]
    fn test_analyze_top_pixels_zero_height() {
        let img = DynamicImage::new_rgb8(10, 0);
        let config = BlueDetectionConfig::default();

        assert!(!analyze_top_pixels(&img, &config));
    }

    /// テスト観点: 小さい画像での動作確認
    /// - 5x5の小さい画像でも正しく判定される
    #[test]
    fn test_analyze_top_pixels_small_image() {
        let img = DynamicImage::new_rgb8(5, 5);
        let mut img = img.to_rgb8();
        for pixel in img.pixels_mut() {
            *pixel = image::Rgb([50, 50, 200]);
        }
        let img = DynamicImage::ImageRgb8(img);

        let config = BlueDetectionConfig::default();
        assert!(analyze_top_pixels(&img, &config));
    }
}
