use anyhow::{Context, Result};
use image::imageops::FilterType;
use image::{DynamicImage, GenericImageView, ImageBuffer, Pixel};
use std::sync::OnceLock;
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

pub(crate) async fn is_blue_sky_image(
    image_url: &str,
    config: &BlueDetectionConfig,
) -> Result<bool> {
    let result = analyze_image(image_url, config).await?;
    Ok(result.is_blue_sky)
}

pub async fn analyze_image(
    image_url: &str,
    config: &BlueDetectionConfig,
) -> Result<AnalysisResult> {
    let top = tokio::time::timeout(
        Duration::from_secs(5),
        download_and_resize_top(image_url, config),
    )
    .await
    .context("Image download timeout")?
    .context("Failed to download image")?;

    Ok(count_blue(&top, top.height(), config))
}

async fn download_and_resize_top(url: &str, config: &BlueDetectionConfig) -> Result<DynamicImage> {
    static HTTP: OnceLock<reqwest::Client> = OnceLock::new();
    let response = HTTP
        .get_or_init(reqwest::Client::new)
        .get(url)
        .send()
        .await
        .context("Failed to fetch image")?;

    let bytes = response
        .bytes()
        .await
        .context("Failed to read image bytes")?;

    let img = decode_image(&bytes).context("Failed to decode image")?;
    drop(bytes);

    Ok(resize_top(&img, config.max_width, config.top_percentage))
}

const MAX_DECODED_BYTES: u64 = 512 * 1024 * 1024;

fn decode_image(bytes: &[u8]) -> Result<DynamicImage> {
    if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        let mut decoder = jpeg_decoder::Decoder::new(bytes);
        decoder.read_info()?;
        let info = decoder.info().context("JPEG has no header")?;
        let (width, height) = (u32::from(info.width), u32::from(info.height));
        let channels: u64 = match info.pixel_format {
            jpeg_decoder::PixelFormat::RGB24 => 3,
            jpeg_decoder::PixelFormat::L8 => 1,
            _ => 0,
        };
        if channels > 0 && u64::from(width) * u64::from(height) * channels <= MAX_DECODED_BYTES {
            let pixels = decoder.decode()?;
            let img = if channels == 3 {
                ImageBuffer::from_raw(width, height, pixels).map(DynamicImage::ImageRgb8)
            } else {
                ImageBuffer::from_raw(width, height, pixels).map(DynamicImage::ImageLuma8)
            };
            return img.context("JPEG pixel count does not match its size");
        }
    }
    Ok(image::load_from_memory(bytes)?)
}

fn top_rows(height: u32, top_percentage: f32) -> u32 {
    (height as f32 * top_percentage) as u32
}

fn resize_top(img: &DynamicImage, max_width: u32, top_percentage: f32) -> DynamicImage {
    let (width, height) = img.dimensions();
    if width <= max_width {
        return img.crop_imm(0, 0, width, top_rows(height, top_percentage));
    }

    let new_height = (height as f32 * (max_width as f32 / width as f32)) as u32;
    let (nwidth, nheight) = fit_within(width, height, max_width, new_height);
    let rows = top_rows(nheight, top_percentage);
    match img {
        DynamicImage::ImageLuma8(p) => {
            DynamicImage::ImageLuma8(triangle_resize_rows(p, nwidth, nheight, rows))
        }
        DynamicImage::ImageLumaA8(p) => {
            DynamicImage::ImageLumaA8(triangle_resize_rows(p, nwidth, nheight, rows))
        }
        DynamicImage::ImageRgb8(p) => {
            DynamicImage::ImageRgb8(triangle_resize_rows(p, nwidth, nheight, rows))
        }
        DynamicImage::ImageRgba8(p) => {
            DynamicImage::ImageRgba8(triangle_resize_rows(p, nwidth, nheight, rows))
        }
        _ => img
            .resize_exact(nwidth, nheight, FilterType::Triangle)
            .crop_imm(0, 0, nwidth, rows),
    }
}

fn fit_within(width: u32, height: u32, nwidth: u32, nheight: u32) -> (u32, u32) {
    let ratio = f64::min(nwidth as f64 / width as f64, nheight as f64 / height as f64);
    let nw = ((width as f64 * ratio).round() as u64).max(1);
    let nh = ((height as f64 * ratio).round() as u64).max(1);
    (nw as u32, nh as u32)
}

fn clamp<N: PartialOrd>(a: N, min: N, max: N) -> N {
    if a < min {
        min
    } else if a > max {
        max
    } else {
        a
    }
}

fn triangle_weights(src_len: u32, dst_len: u32, dst: u32) -> (u32, Vec<f32>) {
    let ratio = src_len as f32 / dst_len as f32;
    let sratio = if ratio < 1.0 { 1.0 } else { ratio };
    let src_support = sratio;
    let center = (dst as f32 + 0.5) * ratio;

    let left = (center - src_support).floor() as i64;
    let left = clamp(left, 0, i64::from(src_len) - 1) as u32;
    let right = (center + src_support).ceil() as i64;
    let right = clamp(right, i64::from(left) + 1, i64::from(src_len)) as u32;

    let center = center - 0.5;
    let mut ws = Vec::with_capacity((right - left) as usize);
    let mut sum = 0.0;
    for i in left..right {
        let x = ((i as f32 - center) / sratio).abs();
        let w = if x < 1.0 { 1.0 - x } else { 0.0 };
        ws.push(w);
        sum += w;
    }
    ws.iter_mut().for_each(|w| *w /= sum);
    (left, ws)
}

fn triangle_resize_rows<P: Pixel<Subpixel = u8> + 'static>(
    image: &ImageBuffer<P, Vec<u8>>,
    nwidth: u32,
    nheight: u32,
    rows: u32,
) -> ImageBuffer<P, Vec<u8>> {
    let (width, height) = image.dimensions();
    let ch = P::CHANNEL_COUNT as usize;
    let columns: Vec<(u32, Vec<f32>)> = (0..nwidth)
        .map(|x| triangle_weights(width, nwidth, x))
        .collect();
    let mut out: ImageBuffer<P, Vec<u8>> = ImageBuffer::new(nwidth, rows);
    let mut line = vec![0.0f32; width as usize * ch];

    for y in 0..rows {
        let (top, ws) = triangle_weights(height, nheight, y);
        line.fill(0.0);
        for x in 0..width {
            let acc = &mut line[x as usize * ch..(x as usize + 1) * ch];
            for (i, w) in ws.iter().enumerate() {
                let p = image.get_pixel(x, top + i as u32).channels();
                for c in 0..ch {
                    acc[c] += p[c] as f32 * w;
                }
            }
        }

        for (x, (left, ws)) in columns.iter().enumerate() {
            let mut t = [0.0f32; 4];
            for (i, w) in ws.iter().enumerate() {
                let at = (*left as usize + i) * ch;
                for c in 0..ch {
                    t[c] += line[at + c] * w;
                }
            }
            let px = out.get_pixel_mut(x as u32, y).channels_mut();
            for c in 0..ch {
                px[c] = clamp(t[c], 0.0, 255.0).round() as u8;
            }
        }
    }

    out
}

pub fn perform_analysis(image: &DynamicImage, config: &BlueDetectionConfig) -> AnalysisResult {
    let top_height = top_rows(image.height(), config.top_percentage);
    count_blue(image, top_height, config)
}

fn count_blue(
    image: &DynamicImage,
    top_height: u32,
    config: &BlueDetectionConfig,
) -> AnalysisResult {
    let width = image.width();

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

    fn noise(len: usize, seed: u32) -> Vec<u8> {
        let mut s = seed;
        (0..len)
            .map(|_| {
                s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (s >> 24) as u8
            })
            .collect()
    }

    fn resized_then_cropped(
        img: &DynamicImage,
        max_width: u32,
        top_percentage: f32,
    ) -> DynamicImage {
        let (width, height) = img.dimensions();
        let resized = if width > max_width {
            let new_height = (height as f32 * (max_width as f32 / width as f32)) as u32;
            img.resize(max_width, new_height, FilterType::Triangle)
        } else {
            img.clone()
        };
        let rows = (resized.height() as f32 * top_percentage) as u32;
        resized.crop_imm(0, 0, resized.width(), rows)
    }

    fn assert_same_as_full_resize(img: &DynamicImage) {
        for top_percentage in [0.3, 1.0] {
            let expected = resized_then_cropped(img, 600, top_percentage);
            let actual = resize_top(img, 600, top_percentage);
            assert_eq!(actual.dimensions(), expected.dimensions());
            assert!(actual.as_bytes() == expected.as_bytes());
        }
    }

    /// カラー写真を上部だけ縮めた結果が、全体を縮めてから上部を切り出した結果と1画素も違わないこと
    #[test]
    fn test_resize_top_matches_full_resize_rgb() {
        for (w, h, seed) in [
            (1600, 1200, 1),
            (1203, 907, 2),
            (601, 1500, 3),
            (4000, 30, 4),
        ] {
            let buf = ImageBuffer::from_raw(w, h, noise((w * h * 3) as usize, seed)).unwrap();
            assert_same_as_full_resize(&DynamicImage::ImageRgb8(buf));
        }
    }

    /// 白黒や透明度つきの画像でも、全体を縮めてから上部を切り出した結果と1画素も違わないこと
    #[test]
    fn test_resize_top_matches_full_resize_other_8bit_formats() {
        let luma = ImageBuffer::from_raw(999, 777, noise(999 * 777, 5)).unwrap();
        assert_same_as_full_resize(&DynamicImage::ImageLuma8(luma));
        let luma_a = ImageBuffer::from_raw(650, 650, noise(650 * 650 * 2, 6)).unwrap();
        assert_same_as_full_resize(&DynamicImage::ImageLumaA8(luma_a));
        let rgba = ImageBuffer::from_raw(777, 1500, noise(777 * 1500 * 4, 7)).unwrap();
        assert_same_as_full_resize(&DynamicImage::ImageRgba8(rgba));
    }

    /// 16ビット画像は従来の縮め方に回し、全体を縮めてから上部を切り出した結果と同じになること
    #[test]
    fn test_resize_top_matches_full_resize_16bit() {
        let bytes = noise(800 * 600 * 6, 8);
        let data = bytes
            .chunks(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        let rgb16 = ImageBuffer::from_raw(800, 600, data).unwrap();
        assert_same_as_full_resize(&DynamicImage::ImageRgb16(rgb16));
    }

    /// 横に極端に長く、縮めると高さが0になる画像でも、従来と同じ大きさ・同じ画素になること
    #[test]
    fn test_resize_top_matches_full_resize_extreme_aspect() {
        for (w, h) in [(2000, 3), (601, 1), (5000, 1)] {
            let buf = ImageBuffer::from_raw(w, h, noise((w * h * 3) as usize, w)).unwrap();
            assert_same_as_full_resize(&DynamicImage::ImageRgb8(buf));
        }
    }

    /// 幅が上限以下の画像は縮めず、上部をそのまま使うこと
    #[test]
    fn test_resize_top_keeps_small_image() {
        let buf = ImageBuffer::from_raw(600, 400, noise(600 * 400 * 3, 9)).unwrap();
        let img = DynamicImage::ImageRgb8(buf);

        let top = resize_top(&img, 600, 0.3);

        assert_eq!(top.dimensions(), (600, 120));
        assert!(top.as_bytes() == img.crop_imm(0, 0, 600, 120).as_bytes());
    }

    fn encode(img: &DynamicImage, format: image::ImageOutputFormat) -> Vec<u8> {
        let mut bytes = std::io::Cursor::new(Vec::new());
        img.write_to(&mut bytes, format).unwrap();
        bytes.into_inner()
    }

    fn assert_same_as_image_crate(bytes: &[u8]) {
        let expected = image::load_from_memory(bytes).unwrap();
        let actual = decode_image(bytes).unwrap();
        assert_eq!(actual.color(), expected.color());
        assert_eq!(actual.dimensions(), expected.dimensions());
        assert!(actual.as_bytes() == expected.as_bytes());
    }

    /// カラーと白黒の JPEG を、image クレートで読んだときと同じ種類・同じ画素で読めること
    #[test]
    fn test_decode_image_matches_image_crate_for_jpeg() {
        let rgb = ImageBuffer::from_raw(321, 123, noise(321 * 123 * 3, 10)).unwrap();
        let jpeg = encode(
            &DynamicImage::ImageRgb8(rgb),
            image::ImageOutputFormat::Jpeg(90),
        );
        assert_same_as_image_crate(&jpeg);

        let luma = ImageBuffer::from_raw(123, 321, noise(123 * 321, 11)).unwrap();
        let jpeg = encode(
            &DynamicImage::ImageLuma8(luma),
            image::ImageOutputFormat::Jpeg(90),
        );
        assert_same_as_image_crate(&jpeg);
    }

    /// JPEG 以外の画像は image クレートに任せ、同じ結果になること
    #[test]
    fn test_decode_image_falls_back_for_other_formats() {
        let rgba = ImageBuffer::from_raw(64, 48, noise(64 * 48 * 4, 12)).unwrap();
        let png = encode(
            &DynamicImage::ImageRgba8(rgba),
            image::ImageOutputFormat::Png,
        );
        assert_same_as_image_crate(&png);
    }

    /// 途中で切れた JPEG は読めずに失敗すること
    #[test]
    fn test_decode_image_rejects_truncated_jpeg() {
        let rgb = ImageBuffer::from_raw(64, 64, noise(64 * 64 * 3, 13)).unwrap();
        let jpeg = encode(
            &DynamicImage::ImageRgb8(rgb),
            image::ImageOutputFormat::Jpeg(90),
        );

        assert!(decode_image(&jpeg[..20]).is_err());
    }
}
