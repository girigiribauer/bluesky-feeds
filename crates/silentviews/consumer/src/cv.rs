use vision::filter::Plane;
use vision::img::{self, Image, Interp};

pub fn plane(m: &Image) -> Plane {
    assert_eq!(m.ch, 1);
    Plane::from_u8(m.w, m.h, &m.data)
}

pub fn fit(img: &Image, side: usize, allow_upscale: bool) -> Image {
    let mut s = side as f64 / img.h.max(img.w) as f64;
    if !allow_upscale {
        s = s.min(1.0);
    }
    let w = ((img.w as f64 * s) as usize).max(1);
    let h = ((img.h as f64 * s) as usize).max(1);
    img::resize(img, w, h, Interp::Area)
}

pub fn pixels(m: &Image) -> &[[u8; 3]] {
    assert_eq!(m.ch, 3);
    m.data.as_chunks::<3>().0
}

pub fn top5_share(pixels: &[[u8; 3]]) -> f64 {
    let mut counts = vec![0u32; 32 * 32 * 32];
    for p in pixels {
        counts[(p[0] as usize / 8) * 1024 + (p[1] as usize / 8) * 32 + (p[2] as usize / 8)] += 1;
    }
    counts.sort_unstable_by(|a, b| b.cmp(a));
    counts.iter().take(5).map(|&c| c as u64).sum::<u64>() as f64 / pixels.len() as f64
}

#[cfg(test)]
pub mod testing {

    use vision::img::Image;

    #[derive(Debug, Clone, Copy, PartialEq)]
    pub struct Rect {
        pub x: i32,
        pub y: i32,
        pub width: i32,
        pub height: i32,
    }

    impl Rect {
        pub fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
            Self {
                x,
                y,
                width,
                height,
            }
        }
    }

    pub use vision::testing::Fnv;

    pub fn solid(w: i32, h: i32, bgr: (f64, f64, f64)) -> Image {
        let px = [bgr.0 as u8, bgr.1 as u8, bgr.2 as u8];
        Image {
            w: w as usize,
            h: h as usize,
            ch: 3,
            data: px.repeat((w * h) as usize),
        }
    }

    pub fn fill(img: &mut Image, rect: Rect, bgr: (f64, f64, f64)) {
        let px = [bgr.0 as u8, bgr.1 as u8, bgr.2 as u8];
        let x0 = rect.x.max(0) as usize;
        let y0 = rect.y.max(0) as usize;
        let x1 = ((rect.x + rect.width) as usize).min(img.w);
        let y1 = ((rect.y + rect.height) as usize).min(img.h);
        for y in y0..y1 {
            for x in x0..x1 {
                let o = (y * img.w + x) * 3;
                img.data[o..o + 3].copy_from_slice(&px);
            }
        }
    }

    pub fn randn(len: usize, seed: u64) -> Vec<f32> {
        let m1 = 2147483648.0f64;
        let (mut dn, mut tn, vn) = (3.442619855899f64, 3.442619855899f64, 9.91256303526217e-3f64);
        let (mut kn, mut wn, mut fn_) = ([0u32; 128], [0f32; 128], [0f32; 128]);
        let q = vn / (-0.5 * dn * dn).exp();
        kn[0] = ((dn / q) * m1) as u32;
        wn[0] = (q / m1) as f32;
        wn[127] = (dn / m1) as f32;
        fn_[0] = 1.0;
        fn_[127] = (-0.5 * dn * dn).exp() as f32;
        for i in (1..=126).rev() {
            dn = (-2.0 * (vn / dn + (-0.5 * dn * dn).exp()).ln()).sqrt();
            kn[i + 1] = ((dn / tn) * m1) as u32;
            tn = dn;
            fn_[i] = (-0.5 * dn * dn).exp() as f32;
            wn[i] = (dn / m1) as f32;
        }

        let next = |s: u64| {
            (s as u32 as u64)
                .wrapping_mul(4164903690)
                .wrapping_add(s >> 32)
        };
        let r = 3.442_62_f32;
        let rng_flt = 2.328_306_4e-10_f32;
        let mut st = if seed == 0 { 0xffffffff } else { seed };
        let mut out = Vec::with_capacity(len);
        for _ in 0..len {
            let x = loop {
                let hz = st as u32 as i32;
                st = next(st);
                let iz = (hz & 127) as usize;
                let x = hz as f32 * wn[iz];
                if (hz.wrapping_abs() as u32) < kn[iz] {
                    break x;
                }
                if iz == 0 {
                    let (mut x, mut y);
                    loop {
                        x = st as u32 as f32 * rng_flt;
                        st = next(st);
                        y = st as u32 as f32 * rng_flt;
                        st = next(st);
                        x = (-((x + f32::MIN_POSITIVE).ln()) as f64 * 0.2904764) as f32;
                        y = -((y + f32::MIN_POSITIVE).ln());
                        if y + y >= x * x {
                            break;
                        }
                    }
                    break if hz > 0 { r + x } else { -r - x };
                }
                let y = st as u32 as f32 * rng_flt;
                st = next(st);
                if ((fn_[iz] + y * (fn_[iz - 1] - fn_[iz])) as f64)
                    < (-0.5 * x as f64 * x as f64).exp()
                {
                    break x;
                }
            };
            out.push(x);
        }
        out
    }

    pub fn noise(w: i32, h: i32, base: (f64, f64, f64), sigma: f64, seed: u64) -> Image {
        let mut img = solid(w, h, base);
        let sigma = sigma as f32;
        for (v, z) in img.data.iter_mut().zip(randn((w * h * 3) as usize, seed)) {
            let n = ((z * sigma + 0.0).round_ties_even() as i32)
                .clamp(i16::MIN as i32, i16::MAX as i32);
            *v = (*v as i32 + n).clamp(0, 255) as u8;
        }
        img
    }

    pub fn png(name: &str) -> Image {
        let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
        let rgb = image::open(path).unwrap().to_rgb8();
        Image {
            w: rgb.width() as usize,
            h: rgb.height() as usize,
            ch: 3,
            data: rgb.pixels().flat_map(|p| [p[2], p[1], p[0]]).collect(),
        }
    }

    pub fn text_image(angle: f64) -> Image {
        png(&format!("text_{angle}.png"))
    }

    pub fn fixture_bytes(name: &str) -> Vec<u8> {
        std::fs::read(format!(
            "{}/tests/fixtures/{name}",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap()
    }

    pub fn fixture(name: &str) -> Image {
        vision::img::decode(&fixture_bytes(name)).unwrap().0
    }

    pub fn crop(src: &Image, r: Rect) -> Image {
        let mut out = Image::new(r.width as usize, r.height as usize, src.ch);
        let n = r.width as usize * src.ch;
        for y in 0..r.height as usize {
            let o = ((r.y as usize + y) * src.w + r.x as usize) * src.ch;
            out.data[y * n..(y + 1) * n].copy_from_slice(&src.data[o..o + n]);
        }
        out
    }

    pub fn paste(dst: &mut Image, src: &Image, x: usize, y: usize) {
        let n = src.w * src.ch;
        for row in 0..src.h {
            let o = ((y + row) * dst.w + x) * dst.ch;
            dst.data[o..o + n].copy_from_slice(&src.data[row * n..(row + 1) * n]);
        }
    }

    pub fn jpeg(img: &Image) -> Vec<u8> {
        let rgb: Vec<u8> = img
            .data
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|p| [p[2], p[1], p[0]])
            .collect();
        let mut out = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 95)
            .encode(&rgb, img.w as u32, img.h as u32, image::ColorType::Rgb8)
            .unwrap();
        out
    }

    pub fn models() -> &'static std::path::Path {
        std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/models"))
    }

    pub fn gray_of(img: &Image) -> Image {
        vision::img::gray(img)
    }
}

#[cfg(test)]
mod tests {
    use super::testing::solid;
    use super::*;

    type NoiseCase = (i32, i32, (f64, f64, f64), f64, u64);

    const NOISE_CASES: [NoiseCase; 20] = [
        (128, 128, (60.0, 140.0, 60.0), 25.0, 31),
        (256, 128, (60.0, 120.0, 60.0), 40.0, 12),
        (256, 256, (100.0, 100.0, 100.0), 40.0, 5),
        (256, 256, (120.0, 120.0, 120.0), 40.0, 11),
        (256, 256, (60.0, 120.0, 60.0), 40.0, 6),
        (30, 20, (60.0, 120.0, 60.0), 20.0, 14),
        (300, 200, (100.0, 100.0, 100.0), 30.0, 4),
        (384, 288, (60.0, 120.0, 60.0), 40.0, 10),
        (384, 288, (60.0, 120.0, 60.0), 40.0, 2),
        (384, 288, (60.0, 120.0, 60.0), 40.0, 3),
        (384, 288, (60.0, 120.0, 60.0), 40.0, 9),
        (384, 384, (60.0, 120.0, 60.0), 20.0, 8),
        (480, 640, (60.0, 120.0, 60.0), 20.0, 22),
        (512, 384, (60.0, 120.0, 60.0), 20.0, 1),
        (512, 384, (60.0, 120.0, 60.0), 40.0, 21),
        (512, 384, (60.0, 120.0, 60.0), 40.0, 24),
        (512, 384, (60.0, 120.0, 60.0), 40.0, 25),
        (640, 480, (60.0, 120.0, 60.0), 20.0, 13),
        (640, 480, (60.0, 120.0, 60.0), 20.0, 21),
        (800, 600, (60.0, 120.0, 60.0), 20.0, 15),
    ];

    fn size_of(m: &Image) -> (usize, usize) {
        (m.w, m.h)
    }

    /// テスト用のノイズ画像20通りが、OpenCV の randn で作ったものと1画素も違わないことを確かめたときの記録から変わらないこと
    #[test]
    fn test_noise_stays_recorded() {
        let mut h = super::testing::Fnv::new();
        for (w, ht, base, sigma, seed) in NOISE_CASES {
            h.bytes(&super::testing::noise(w, ht, base, sigma, seed).data);
        }
        assert_eq!(h.0, 3610586445928833249);
    }

    /// 正規分布の乱数は、たくさん作ると平均がほぼ 0、ばらつき（標準偏差）がほぼ 1 になること
    #[test]
    fn test_randn_is_standard_normal() {
        let v = super::testing::randn(200_000, 7);
        let n = v.len() as f64;
        let mean = v.iter().map(|&x| x as f64).sum::<f64>() / n;
        let var = v.iter().map(|&x| (x as f64 - mean).powi(2)).sum::<f64>() / n;
        assert!(mean.abs() < 0.01, "{mean}");
        assert!((var.sqrt() - 1.0).abs() < 0.01, "{}", var.sqrt());
    }

    /// 長い辺を指定の大きさに合わせて縮め、短い辺の端数は切り捨てること（試作と同じ）
    #[test]
    fn test_fit_shrinks_and_truncates() {
        assert_eq!(
            size_of(&fit(&solid(600, 200, (0.0, 0.0, 0.0)), 256, false)),
            (256, 85)
        );
        assert_eq!(
            size_of(&fit(&solid(200, 600, (0.0, 0.0, 0.0)), 256, false)),
            (85, 256)
        );
        assert_eq!(
            size_of(&fit(&solid(700, 300, (0.0, 0.0, 0.0)), 256, false)),
            (256, 109)
        );
        assert_eq!(
            size_of(&fit(&solid(300, 700, (0.0, 0.0, 0.0)), 256, false)),
            (109, 256)
        );
    }

    /// 拡大を許さないときは、小さな画像をそのままの大きさにすること
    #[test]
    fn test_fit_keeps_small_image_without_upscale() {
        assert_eq!(
            size_of(&fit(&solid(100, 50, (0.0, 0.0, 0.0)), 256, false)),
            (100, 50)
        );
    }

    /// 拡大を許すときは、小さな画像も長い辺を指定の大きさまで広げること
    #[test]
    fn test_fit_upscales_when_allowed() {
        assert_eq!(
            size_of(&fit(&solid(100, 50, (0.0, 0.0, 0.0)), 256, true)),
            (256, 128)
        );
    }

    /// とても細長い画像でも、短い辺を 0 にせず 1 にすること
    #[test]
    fn test_fit_never_makes_zero_side() {
        assert_eq!(
            size_of(&fit(&solid(2000, 2, (0.0, 0.0, 0.0)), 256, false)),
            (256, 1)
        );
    }
}
