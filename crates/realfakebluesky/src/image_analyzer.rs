use anyhow::{Context, Result};
use image::{DynamicImage, GenericImageView};
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct BlueDetectionConfig {
    pub top_percentage: f32,
    pub blue_threshold: f32,
    pub rgb_blue_ratio: f32,
    pub min_blue_value: u8,
    pub max_width: u32,
}

const DEFAULT_TOP_PERCENTAGE: f32 = 0.3;
const DEFAULT_BLUE_THRESHOLD: f32 = 0.5;
const DEFAULT_RGB_BLUE_RATIO: f32 = 1.18;

const DEFAULT_MIN_BLUE_VALUE: u8 = 100;
const DEFAULT_MAX_WIDTH: u32 = 600;

impl Default for BlueDetectionConfig {
    fn default() -> Self {
        Self {
            top_percentage: DEFAULT_TOP_PERCENTAGE,
            blue_threshold: DEFAULT_BLUE_THRESHOLD,
            rgb_blue_ratio: DEFAULT_RGB_BLUE_RATIO,
            min_blue_value: DEFAULT_MIN_BLUE_VALUE,
            max_width: DEFAULT_MAX_WIDTH,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AnalysisResult {
    pub is_blue_sky: bool,
    pub score: f32,
    pub total_pixels: u32,
    pub blue_pixels: u32,
}

pub async fn is_blue_sky_image(image_url: &str, config: &BlueDetectionConfig) -> Result<bool> {
    let result = analyze_image(image_url, config).await?;
    Ok(result.is_blue_sky)
}

pub async fn analyze_image(
    image_url: &str,
    config: &BlueDetectionConfig,
) -> Result<AnalysisResult> {
    let image = tokio::time::timeout(
        Duration::from_secs(5),
        download_and_resize_image(image_url, config.max_width),
    )
    .await
    .context("Image download timeout")?
    .context("Failed to download image")?;

    Ok(perform_analysis(&image, config))
}

async fn download_and_resize_image(url: &str, max_width: u32) -> Result<DynamicImage> {
    let response = reqwest::get(url).await.context("Failed to fetch image")?;

    let bytes = response
        .bytes()
        .await
        .context("Failed to read image bytes")?;

    let img = image::load_from_memory(&bytes).context("Failed to decode image")?;

    let (width, height) = img.dimensions();
    if width > max_width {
        let new_height = (height as f32 * (max_width as f32 / width as f32)) as u32;
        Ok(img.resize(max_width, new_height, image::imageops::FilterType::Triangle))
    } else {
        Ok(img)
    }
}

pub fn perform_analysis(image: &DynamicImage, config: &BlueDetectionConfig) -> AnalysisResult {
    let (width, height) = image.dimensions();
    let top_height = (height as f32 * config.top_percentage) as u32;

    if top_height == 0 {
        return AnalysisResult {
            is_blue_sky: false,
            score: 0.0,
            total_pixels: 0,
            blue_pixels: 0,
        };
    }

    let mut total_pixels = 0;
    let mut blue_pixels = 0;

    for y in 0..top_height {
        for x in 0..width {
            let pixel = image.get_pixel(x, y);
            let r = pixel[0];
            let g = pixel[1];
            let b = pixel[2];

            total_pixels += 1;

            if is_blue_pixel(r, g, b, config) {
                blue_pixels += 1;
            }
        }
    }

    let blue_ratio = blue_pixels as f32 / total_pixels as f32;
    AnalysisResult {
        is_blue_sky: blue_ratio >= config.blue_threshold,
        score: blue_ratio,
        total_pixels,
        blue_pixels,
    }
}

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

    fn analyze_top_pixels(image: &DynamicImage, config: &BlueDetectionConfig) -> bool {
        perform_analysis(image, config).is_blue_sky
    }

    /// 青が十分に強く、赤や緑に負けていないピクセルだけを青と判定すること
    #[test]
    fn test_is_blue_pixel() {
        let config = BlueDetectionConfig::default();

        assert!(is_blue_pixel(50, 50, 150, &config));
        assert!(!is_blue_pixel(50, 50, 80, &config));
        assert!(!is_blue_pixel(150, 50, 120, &config));
        assert!(!is_blue_pixel(50, 150, 120, &config));
    }

    /// 青の値が閾値ちょうどなら青と判定し、灰色は判定しないこと
    #[test]
    fn test_is_blue_pixel_edge_cases() {
        let config = BlueDetectionConfig::default();

        assert!(is_blue_pixel(50, 50, 100, &config));
        assert!(!is_blue_pixel(100, 100, 110, &config));
        assert!(is_blue_pixel(0, 0, 255, &config));
        assert!(!is_blue_pixel(150, 150, 150, &config));
    }

    /// 上部が全て青なら青空と判定すること
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

    /// 青が1つもなければ青空と判定しないこと
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

    /// 判定に使うのは画像の上部だけで、下半分が赤でも青空と判定すること
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

    /// 上部の青が閾値（50%）に届かなければ青空と判定しないこと
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

    /// 設定で渡した青の比率と最小値がそのまま使われること
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

    /// 高さ0の画像でも落ちず、青空と判定しないこと
    #[test]
    fn test_analyze_top_pixels_zero_height() {
        let img = DynamicImage::new_rgb8(10, 0);
        let config = BlueDetectionConfig::default();

        assert!(!analyze_top_pixels(&img, &config));
    }

    /// 上部が1行しかない小さい画像でも判定できること
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
