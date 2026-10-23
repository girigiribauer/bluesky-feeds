#[derive(Debug, Clone, PartialEq)]
pub struct Image {
    pub w: usize,
    pub h: usize,
    pub ch: usize,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Interp {
    Linear,
    Area,
}

const COEF_BITS: i32 = 11;
const COEF_SCALE: f32 = (1 << COEF_BITS) as f32;

impl Image {
    pub fn new(w: usize, h: usize, ch: usize) -> Self {
        Self {
            w,
            h,
            ch,
            data: vec![0; w * h * ch],
        }
    }

    fn row(&self, y: usize) -> &[u8] {
        let n = self.w * self.ch;
        &self.data[y * n..(y + 1) * n]
    }
}

fn round_i32(v: f32) -> i32 {
    v.round_ties_even() as i32
}

fn round_f64(v: f64) -> i32 {
    v.round_ties_even() as i32
}

fn sat_u8(v: i32) -> u8 {
    v.clamp(0, 255) as u8
}

fn sat_i16(v: i32) -> i16 {
    v.clamp(i16::MIN as i32, i16::MAX as i32) as i16
}

pub fn resize(src: &Image, dw: usize, dh: usize, interp: Interp) -> Image {
    assert!(src.w > 0 && src.h > 0 && dw > 0 && dh > 0);
    if dw == src.w && dh == src.h {
        return src.clone();
    }
    let inv_x = dw as f64 / src.w as f64;
    let inv_y = dh as f64 / src.h as f64;
    let scale_x = 1.0 / inv_x;
    let scale_y = 1.0 / inv_y;
    let iscale_x = round_f64(scale_x);
    let iscale_y = round_f64(scale_y);
    let area_fast = (scale_x - iscale_x as f64).abs() < f64::EPSILON
        && (scale_y - iscale_y as f64).abs() < f64::EPSILON;
    let mut interp = interp;
    if interp == Interp::Linear && area_fast && iscale_x == 2 && iscale_y == 2 {
        interp = Interp::Area;
    }
    if interp == Interp::Area && scale_x >= 1.0 && scale_y >= 1.0 {
        if area_fast {
            return area_fast_resize(src, dw, dh, iscale_x as usize, iscale_y as usize);
        }
        return area_resize(src, dw, dh, scale_x, scale_y);
    }
    linear_resize(src, dw, dh, interp == Interp::Area)
}

fn coefficients(
    d: usize,
    s: usize,
    scale: f64,
    inv: f64,
    area_mode: bool,
    clamp: bool,
) -> (Vec<i32>, Vec<[i16; 2]>, usize, usize) {
    let (mut lo, mut hi) = (0, d);
    let mut ofs = Vec::with_capacity(d);
    let mut coef = Vec::with_capacity(d);
    for i in 0..d {
        let (mut si, mut f);
        if !area_mode {
            f = ((i as f64 + 0.5) * scale - 0.5) as f32;
            si = f.floor() as i32;
            f -= si as f32;
        } else {
            si = (i as f64 * scale).floor() as i32;
            f = ((i + 1) as f64 - (si + 1) as f64 * inv) as f32;
            f = if f <= 0.0 { 0.0 } else { f - f.floor() };
        }
        if clamp && si < 0 {
            lo = i + 1;
            f = 0.0;
            si = 0;
        }
        if clamp && si + 1 >= s as i32 {
            hi = hi.min(i);
            if si >= s as i32 - 1 {
                f = 0.0;
                si = s as i32 - 1;
            }
        }
        ofs.push(si);
        coef.push([
            sat_i16(round_i32((1.0 - f) * COEF_SCALE)),
            sat_i16(round_i32(f * COEF_SCALE)),
        ]);
    }
    (ofs, coef, lo, hi)
}

fn linear_resize(src: &Image, dw: usize, dh: usize, area_mode: bool) -> Image {
    let cn = src.ch;
    let inv_x = dw as f64 / src.w as f64;
    let inv_y = dh as f64 / src.h as f64;
    let (xofs, alpha, _, xmax) = coefficients(dw, src.w, 1.0 / inv_x, inv_x, area_mode, true);
    let (yofs, beta, _, _) = coefficients(dh, src.h, 1.0 / inv_y, inv_y, area_mode, false);

    let width = dw * cn;
    let hresize = |y: usize| -> Vec<i32> {
        let s = src.row(y);
        let mut d = vec![0i32; width];
        for dx in 0..dw {
            let sx = xofs[dx] as usize * cn;
            let [a0, a1] = alpha[dx];
            for c in 0..cn {
                d[dx * cn + c] = if dx < xmax {
                    s[sx + c] as i32 * a0 as i32 + s[sx + cn + c] as i32 * a1 as i32
                } else {
                    s[sx + c] as i32 * (1 << COEF_BITS)
                };
            }
        }
        d
    };

    let mut out = Image::new(dw, dh, cn);
    let mut cache: Option<(usize, Vec<i32>)> = None;
    for dy in 0..dh {
        let sy0 = yofs[dy];
        let y0 = sy0.clamp(0, src.h as i32 - 1) as usize;
        let y1 = (sy0 + 1).clamp(0, src.h as i32 - 1) as usize;
        let r0 = match cache.take() {
            Some((y, r)) if y == y0 => r,
            _ => hresize(y0),
        };
        let r1 = hresize(y1);
        let [b0, b1] = beta[dy];
        let dst = &mut out.data[dy * width..(dy + 1) * width];
        for x in 0..width {
            let m0 = (b0 as i32 * (r0[x] >> 4)) >> 16;
            let m1 = (b1 as i32 * (r1[x] >> 4)) >> 16;
            dst[x] = sat_u8((m0 + m1 + 2) >> 2);
        }
        cache = Some((y1, r1));
    }
    out
}

fn area_fast_resize(src: &Image, dw: usize, dh: usize, sx: usize, sy: usize) -> Image {
    let cn = src.ch;
    let mut out = Image::new(dw, dh, cn);
    let scale = 1.0f32 / (sx * sy) as f32;
    let two = sx == 2 && sy == 2 && matches!(cn, 1 | 3 | 4);
    for dy in 0..dh {
        for dx in 0..dw {
            for c in 0..cn {
                let mut sum = 0i32;
                for ky in 0..sy {
                    let row = src.row(dy * sy + ky);
                    for kx in 0..sx {
                        sum += row[(dx * sx + kx) * cn + c] as i32;
                    }
                }
                out.data[(dy * dw + dx) * cn + c] = if two {
                    ((sum + 2) >> 2) as u8
                } else {
                    sat_u8(round_i32(sum as f32 * scale))
                };
            }
        }
    }
    out
}

pub(crate) struct Tap {
    pub(crate) di: usize,
    pub(crate) si: usize,
    pub(crate) alpha: f32,
}

pub(crate) fn area_tab(s: usize, d: usize, cn: usize, scale: f64) -> Vec<Tap> {
    let mut tab = Vec::new();
    for dx in 0..d {
        let fsx1 = dx as f64 * scale;
        let fsx2 = fsx1 + scale;
        let cell = scale.min(s as f64 - fsx1);
        let mut sx1 = fsx1.ceil() as i64;
        let mut sx2 = fsx2.floor() as i64;
        sx2 = sx2.min(s as i64 - 1);
        sx1 = sx1.min(sx2);
        if sx1 as f64 - fsx1 > 1e-3 {
            tab.push(Tap {
                di: dx * cn,
                si: (sx1 - 1) as usize * cn,
                alpha: ((sx1 as f64 - fsx1) / cell) as f32,
            });
        }
        for sx in sx1..sx2 {
            tab.push(Tap {
                di: dx * cn,
                si: sx as usize * cn,
                alpha: (1.0 / cell) as f32,
            });
        }
        if fsx2 - sx2 as f64 > 1e-3 {
            tab.push(Tap {
                di: dx * cn,
                si: sx2 as usize * cn,
                alpha: ((fsx2 - sx2 as f64).min(1.0).min(cell) / cell) as f32,
            });
        }
    }
    tab
}

fn area_resize(src: &Image, dw: usize, dh: usize, scale_x: f64, scale_y: f64) -> Image {
    let cn = src.ch;
    let width = dw * cn;
    let xtab = area_tab(src.w, dw, cn, scale_x);
    let ytab = area_tab(src.h, dh, 1, scale_y);
    let mut out = Image::new(dw, dh, cn);
    let mut sum = vec![0f32; width];
    let mut buf = vec![0f32; width];
    let mut prev = ytab[0].di;
    let store = |sum: &[f32], out: &mut Image, dy: usize| {
        for (d, s) in out.data[dy * width..(dy + 1) * width].iter_mut().zip(sum) {
            *d = sat_u8(round_i32(*s));
        }
    };
    for t in &ytab {
        let s = src.row(t.si);
        buf.iter_mut().for_each(|v| *v = 0.0);
        for x in &xtab {
            for c in 0..cn {
                buf[x.di + c] += s[x.si + c] as f32 * x.alpha;
            }
        }
        if t.di != prev {
            store(&sum, &mut out, prev);
            for (v, b) in sum.iter_mut().zip(&buf) {
                *v = b * t.alpha;
            }
            prev = t.di;
        } else {
            for (v, b) in sum.iter_mut().zip(&buf) {
                *v += b * t.alpha;
            }
        }
    }
    store(&sum, &mut out, prev);
    out
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum JpegColor {
    YCbCr,
    Rgb,
}

fn jpeg_color(bytes: &[u8]) -> Option<JpegColor> {
    let (mut jfif, mut adobe, mut ids) = (false, None, None);
    let mut i = 2;
    while i + 4 <= bytes.len() && bytes[i] == 0xff {
        let marker = bytes[i + 1];
        if marker == 0xff {
            i += 1;
            continue;
        }
        let len = u16::from_be_bytes([bytes[i + 2], bytes[i + 3]]) as usize;
        let seg = bytes.get(i + 4..i + 2 + len)?;
        match marker {
            0xe0 if seg.starts_with(b"JFIF\0") => jfif = true,
            0xee if seg.starts_with(b"Adobe") && seg.len() >= 12 => adobe = Some(seg[11]),
            0xc0..=0xcf
                if !matches!(marker, 0xc4 | 0xc8 | 0xcc) && seg.len() >= 15 && seg[5] == 3 =>
            {
                ids = Some((seg[6], seg[9], seg[12]))
            }
            0xda => break,
            _ => {}
        }
        i += 2 + len;
    }
    Some(if jfif {
        JpegColor::YCbCr
    } else if let Some(t) = adobe {
        if t == 0 {
            JpegColor::Rgb
        } else {
            JpegColor::YCbCr
        }
    } else if ids? == (82, 71, 66) {
        JpegColor::Rgb
    } else {
        JpegColor::YCbCr
    })
}

const SCALEBITS: i32 = 16;

fn fix(x: f64) -> i32 {
    (x * (1 << SCALEBITS) as f64 + 0.5) as i32
}

fn ycc_tables() -> &'static [[i32; 256]; 4] {
    static TABLES: std::sync::OnceLock<[[i32; 256]; 4]> = std::sync::OnceLock::new();
    TABLES.get_or_init(|| {
        let half = 1 << (SCALEBITS - 1);
        let x = |i: usize| i as i32 - 128;
        [
            std::array::from_fn(|i| (fix(1.40200) * x(i) + half) >> SCALEBITS),
            std::array::from_fn(|i| (fix(1.77200) * x(i) + half) >> SCALEBITS),
            std::array::from_fn(|i| -fix(0.71414) * x(i)),
            std::array::from_fn(|i| -fix(0.34414) * x(i) + half),
        ]
    })
}

pub fn decode(bytes: &[u8]) -> Option<(Image, Image)> {
    let mut d = jpeg_decoder::Decoder::new(bytes);
    d.read_info().ok()?;
    let info = d.info()?;
    let space = match info.pixel_format {
        jpeg_decoder::PixelFormat::RGB24 => {
            let space = jpeg_color(bytes)?;
            d.set_color_transform(jpeg_decoder::ColorTransform::RGB);
            Some(space)
        }
        jpeg_decoder::PixelFormat::L8 => None,
        _ => return None,
    };
    let mut px = d.decode().ok()?;
    let (w, h) = (info.width as usize, info.height as usize);
    let orientation = d.exif_data().and_then(exif_orientation).unwrap_or(1);
    drop(d);
    let (bgr, gray): (Vec<u8>, Vec<u8>) = match space {
        None => (px.iter().flat_map(|&v| [v, v, v]).collect(), px),
        Some(JpegColor::YCbCr) => {
            let gray = px.as_chunks::<3>().0.iter().map(|p| p[0]).collect();
            let [cr_r, cb_b, cr_g, cb_g] = ycc_tables();
            for p in px.as_chunks_mut::<3>().0 {
                let (y, cb, cr) = (p[0] as i32, p[1] as usize, p[2] as usize);
                *p = [
                    sat_u8(y + cb_b[cb]),
                    sat_u8(y + ((cb_g[cb] + cr_g[cr]) >> SCALEBITS)),
                    sat_u8(y + cr_r[cr]),
                ];
            }
            (px, gray)
        }
        Some(JpegColor::Rgb) => {
            let (r, g, b) = (fix(0.29900), fix(0.58700), fix(0.11400));
            let half = 1 << (SCALEBITS - 1);
            let gray = px
                .as_chunks::<3>()
                .0
                .iter()
                .map(|p| {
                    ((r * p[0] as i32 + g * p[1] as i32 + b * p[2] as i32 + half) >> SCALEBITS)
                        as u8
                })
                .collect();
            for p in px.as_chunks_mut::<3>().0 {
                p.swap(0, 2);
            }
            (px, gray)
        }
    };
    let bgr = Image {
        w,
        h,
        ch: 3,
        data: bgr,
    };
    let gray = Image {
        w,
        h,
        ch: 1,
        data: gray,
    };
    (bgr.data.len() == w * h * 3 && gray.data.len() == w * h)
        .then(|| (orient(bgr, orientation), orient(gray, orientation)))
}

fn exif_orientation(exif: &[u8]) -> Option<u16> {
    let le = match exif.get(0..2)? {
        b"II" => true,
        b"MM" => false,
        _ => return None,
    };
    let u16_at = |i: usize| -> Option<u16> {
        let b: [u8; 2] = exif.get(i..i + 2)?.try_into().ok()?;
        Some(if le {
            u16::from_le_bytes(b)
        } else {
            u16::from_be_bytes(b)
        })
    };
    let u32_at = |i: usize| -> Option<u32> {
        let b: [u8; 4] = exif.get(i..i + 4)?.try_into().ok()?;
        Some(if le {
            u32::from_le_bytes(b)
        } else {
            u32::from_be_bytes(b)
        })
    };
    let ifd = u32_at(4)? as usize;
    (0..u16_at(ifd)? as usize).find_map(|k| {
        let e = ifd + 2 + k * 12;
        (u16_at(e)? == 0x0112).then(|| u16_at(e + 8)).flatten()
    })
}

fn orient(src: Image, orientation: u16) -> Image {
    let (w, h, ch) = (src.w, src.h, src.ch);
    let swap = matches!(orientation, 5..=8);
    let (dw, dh) = if swap { (h, w) } else { (w, h) };
    if !(2..=8).contains(&orientation) {
        return src;
    }
    let mut out = Image::new(dw, dh, ch);
    for y in 0..dh {
        for x in 0..dw {
            let (sx, sy) = match orientation {
                2 => (w - 1 - x, y),
                3 => (w - 1 - x, h - 1 - y),
                4 => (x, h - 1 - y),
                5 => (y, x),
                6 => (y, h - 1 - x),
                7 => (w - 1 - y, h - 1 - x),
                _ => (w - 1 - y, x),
            };
            let (s, d) = ((sy * w + sx) * ch, (y * dw + x) * ch);
            out.data[d..d + ch].copy_from_slice(&src.data[s..s + ch]);
        }
    }
    out
}

fn exact_coeffs(inv: f64, s: usize, d: usize) -> (Vec<usize>, Vec<[u32; 2]>, usize, usize) {
    let scale = 1.0 / inv;
    let (mut lo, mut hi) = (0usize, d);
    let mut ofs = vec![0usize; d];
    let mut coef = vec![[0u32; 2]; d];
    for v in 0..d {
        let f = scale * (v as f64 + 0.5) - 0.5;
        let i = f.floor() as i64;
        if i >= 0 && s > 1 {
            if (i as usize) < s - 1 {
                ofs[v] = i as usize;
                let frac = f - i as f64;
                let c1 = if frac.is_sign_negative() {
                    0
                } else {
                    (frac * 256.0).round_ties_even() as u32 & 0xffff
                };
                coef[v] = [256u32.saturating_sub(c1), c1];
            } else {
                ofs[v] = s - 1;
                hi = hi.min(v);
            }
        } else {
            lo = lo.max(v + 1);
        }
    }
    (ofs, coef, lo, hi)
}

pub fn resize_linear_exact(src: &Image, f: f64) -> Image {
    assert_eq!(src.ch, 1);
    let dw = (src.w as f64 * f).round_ties_even() as usize;
    let dh = (src.h as f64 * f).round_ties_even() as usize;
    if dw == src.w && dh == src.h {
        return src.clone();
    }
    let (xofs, xc, xmin, xmax) = exact_coeffs(f, src.w, dw);
    let (yofs, yc, ymin, ymax) = exact_coeffs(f, src.h, dh);
    let hline = |y: usize| -> Vec<u32> {
        let row = src.row(y);
        (0..dw)
            .map(|x| {
                if x < xmin {
                    (row[0] as u32) << 8
                } else if x < xmax {
                    let p = xofs[x];
                    (xc[x][0] * row[p] as u32).min(0xffff)
                        + (xc[x][1] * row[p + 1] as u32).min(0xffff)
                } else {
                    (row[xofs[dw - 1]] as u32) << 8
                }
            })
            .map(|v| v.min(0xffff))
            .collect()
    };
    let set = |r: &[u32]| -> Vec<u8> {
        r.iter()
            .map(|&v| (((v + 128) & 0xffff) >> 8) as u8)
            .collect()
    };
    let first = hline(0);
    let last = hline(src.h - 1);
    let mut out = Image::new(dw, dh, 1);
    for y in 0..dh {
        let row: Vec<u8> = if y < ymin {
            set(&first)
        } else if y < ymax {
            let (r0, r1) = (hline(yofs[y]), hline(yofs[y] + 1));
            r0.iter()
                .zip(&r1)
                .map(|(&a, &b)| {
                    ((((a * yc[y][0]) as u64 + (b * yc[y][1]) as u64).min(0xffff_ffff) as u32
                        + 32768)
                        >> 16)
                        .min(255) as u8
                })
                .collect()
        } else {
            set(&last)
        };
        out.data[y * dw..(y + 1) * dw].copy_from_slice(&row);
    }
    out
}

struct LabTables {
    gamma: [i32; 256],
    cbrt: Vec<i32>,
    coeffs: [i32; 9],
}

fn lab_tables() -> &'static LabTables {
    static T: std::sync::OnceLock<LabTables> = std::sync::OnceLock::new();
    T.get_or_init(|| {
        use crate::filter::{soft_cbrt, soft_pow};
        let threshold = 809.0f64 / 20000.0;
        let low_scale = 323.0f64 / 25.0;
        let power = 12.0f64 / 5.0;
        let xshift = 11.0f64 / 200.0;
        let apply_gamma = |x: f32| -> f32 {
            let xd = x as f64;
            (if xd <= threshold {
                xd / low_scale
            } else {
                soft_pow((xd + xshift) / (1.0 + xshift), power)
            }) as f32
        };
        let f255 = 255.0f32;
        let int_scale = (255 * 8) as f32;
        let mut gamma = [0i32; 256];
        for (i, g) in gamma.iter_mut().enumerate() {
            *g = (int_scale * apply_gamma(i as f32 / f255)).round_ties_even() as i32;
        }
        let lthresh = 216.0f32 / 24389.0;
        let lscale = 841.0f32 / 108.0;
        let lbias = 16.0f32 / 116.0;
        let cb_scale = 1.0f32 / (f255 * 8.0);
        let lshift2 = (1 << 15) as f32;
        let cbrt = (0..256 * 3 / 2 * 8)
            .map(|i| {
                let x = cb_scale * i as f32;
                let v = if x < lthresh {
                    x.mul_add(lscale, lbias)
                } else {
                    soft_cbrt(x)
                };
                (lshift2 * v).round_ties_even() as i32
            })
            .collect();
        let m = [
            0x3fda65a14488c60du64,
            0x3fd6e297396d0918,
            0x3fc71819d2391d58,
            0x3fcb38cda6e75ff6,
            0x3fe6e297396d0918,
            0x3fb279aae6c8f755,
            0x3f93cc4ac6cdaf4b,
            0x3fbe836eb4e98138,
            0x3fee68427418d691,
        ]
        .map(f64::from_bits);
        let white = [
            f64::from_bits(0x3fee6a22b3892ee8),
            1.0,
            f64::from_bits(0x3ff16b8950763a19),
        ];
        let lshift = 4096.0f64;
        let mut coeffs = [0i32; 9];
        for i in 0..3 {
            coeffs[i * 3 + 2] = (lshift * m[i * 3] / white[i]).round_ties_even() as i32;
            coeffs[i * 3 + 1] = (lshift * m[i * 3 + 1] / white[i]).round_ties_even() as i32;
            coeffs[i * 3] = (lshift * m[i * 3 + 2] / white[i]).round_ties_even() as i32;
        }
        LabTables {
            gamma,
            cbrt,
            coeffs,
        }
    })
}

pub fn lab(bgr: &Image) -> Image {
    assert_eq!(bgr.ch, 3);
    let t = lab_tables();
    let c = t.coeffs;
    let descale = |v: i32, n: i32| (v + (1 << (n - 1))) >> n;
    let lscale = (116 * 255 + 50) / 100;
    let lshift = -((16 * 255 * (1 << 15) + 50) / 100);
    let data = bgr
        .data
        .as_chunks::<3>()
        .0
        .iter()
        .flat_map(|p| {
            let (r, g, b) = (
                t.gamma[p[0] as usize],
                t.gamma[p[1] as usize],
                t.gamma[p[2] as usize],
            );
            let fx = t.cbrt[descale(r * c[0] + g * c[1] + b * c[2], 12) as usize];
            let fy = t.cbrt[descale(r * c[3] + g * c[4] + b * c[5], 12) as usize];
            let fz = t.cbrt[descale(r * c[6] + g * c[7] + b * c[8], 12) as usize];
            let l = descale(lscale * fy + lshift, 15);
            let a = descale(500 * (fx - fy) + 128 * (1 << 15), 15);
            let bb = descale(200 * (fy - fz) + 128 * (1 << 15), 15);
            [sat_u8(l), sat_u8(a), sat_u8(bb)]
        })
        .collect();
    Image {
        w: bgr.w,
        h: bgr.h,
        ch: 3,
        data,
    }
}

pub fn place(src: &Image, w: usize, h: usize, x: usize, y: usize) -> Image {
    let mut out = Image::new(w, h, src.ch);
    let n = src.w * src.ch;
    for r in 0..src.h {
        let start = ((y + r) * w + x) * src.ch;
        out.data[start..start + n].copy_from_slice(src.row(r));
    }
    out
}

pub fn crop(src: &Image, x: usize, y: usize, w: usize, h: usize) -> Image {
    assert!(x + w <= src.w && y + h <= src.h);
    let mut out = Image::new(w, h, src.ch);
    let n = w * src.ch;
    for r in 0..h {
        let start = x * src.ch;
        out.data[r * n..(r + 1) * n].copy_from_slice(&src.row(y + r)[start..start + n]);
    }
    out
}

pub fn gray(bgr: &Image) -> Image {
    assert_eq!(bgr.ch, 3);
    let data = bgr
        .data
        .as_chunks::<3>()
        .0
        .iter()
        .map(|p| {
            ((p[0] as i32 * 3735 + p[1] as i32 * 19235 + p[2] as i32 * 9798 + (1 << 14)) >> 15)
                as u8
        })
        .collect();
    Image {
        w: bgr.w,
        h: bgr.h,
        ch: 1,
        data,
    }
}

fn hsv_tables() -> &'static ([i32; 256], [i32; 256]) {
    static TABLES: std::sync::OnceLock<([i32; 256], [i32; 256])> = std::sync::OnceLock::new();
    TABLES.get_or_init(|| {
        let (mut sdiv, mut hdiv) = ([0i32; 256], [0i32; 256]);
        for i in 1..256 {
            sdiv[i] = round_f64((255 << 12) as f64 / i as f64);
            hdiv[i] = round_f64((180 << 12) as f64 / (6.0 * i as f64));
        }
        (sdiv, hdiv)
    })
}

pub fn hsv(bgr: &Image) -> Image {
    assert_eq!(bgr.ch, 3);
    let (sdiv, hdiv) = hsv_tables();
    let mut data = Vec::with_capacity(bgr.data.len());
    for p in bgr.data.as_chunks::<3>().0 {
        let (b, g, r) = (p[0] as i32, p[1] as i32, p[2] as i32);
        let v = b.max(g).max(r);
        let vmin = b.min(g).min(r);
        let diff = v - vmin;
        let vr = if v == r { -1 } else { 0 };
        let vg = if v == g { -1 } else { 0 };
        let s = (diff * sdiv[v as usize] + (1 << 11)) >> 12;
        let mut h =
            (vr & (g - b)) + (!vr & ((vg & (b - r + 2 * diff)) + (!vg & (r - g + 4 * diff))));
        h = (h * hdiv[diff as usize] + (1 << 11)) >> 12;
        if h < 0 {
            h += 180;
        }
        data.extend([sat_u8(h), s as u8, v as u8]);
    }
    Image {
        w: bgr.w,
        h: bgr.h,
        ch: 3,
        data,
    }
}

pub fn blob(img: &Image, swap_rb: bool, mean: [f32; 3], scale: f32) -> Vec<f32> {
    assert_eq!(img.ch, 3);
    let wh = img.w * img.h;
    let mut out = vec![0f32; 3 * wh];
    for (i, p) in img.data.as_chunks::<3>().0.iter().enumerate() {
        for (c, &v) in p.iter().enumerate() {
            let dst = if swap_rb { 2 - c } else { c };
            out[dst * wh + i] = v as f32;
        }
    }
    if mean != [0.0; 3] || scale != 1.0 {
        for c in 0..3 {
            for v in &mut out[c * wh..(c + 1) * wh] {
                *v = (*v - mean[c]) * scale;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::Fnv;

    fn random(w: usize, h: usize, ch: usize, seed: u64) -> Image {
        let mut s = seed;
        let data = (0..w * h * ch)
            .map(|_| {
                s = s
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                (s >> 56) as u8
            })
            .collect();
        Image { w, h, ch, data }
    }

    const SIZES: [(usize, usize, usize, usize); 14] = [
        (640, 480, 256, 192),
        (640, 480, 320, 240),
        (1000, 750, 256, 192),
        (1000, 667, 768, 512),
        (333, 777, 109, 256),
        (2000, 3, 256, 1),
        (100, 50, 256, 128),
        (97, 61, 256, 161),
        (64, 64, 416, 416),
        (500, 375, 736, 736),
        (1200, 900, 736, 736),
        (300, 300, 224, 224),
        (1024, 768, 256, 192),
        (17, 13, 5, 4),
    ];

    fn fixture_bytes(name: &str) -> Vec<u8> {
        std::fs::read(format!(
            "{}/tests/fixtures/{name}",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap()
    }

    fn with_orientation(jpeg: &[u8], orientation: u8) -> Vec<u8> {
        let mut tiff =
            b"MM\x00\x2a\x00\x00\x00\x08\x00\x01\x01\x12\x00\x03\x00\x00\x00\x01\x00".to_vec();
        tiff.extend([orientation, 0, 0, 0, 0, 0, 0, 0]);
        let mut app1 = b"Exif\x00\x00".to_vec();
        app1.extend(tiff);
        let len = (app1.len() + 2) as u16;
        let mut out = jpeg[..2].to_vec();
        out.extend([0xff, 0xe1]);
        out.extend(len.to_be_bytes());
        out.extend(app1);
        out.extend(&jpeg[2..]);
        out
    }

    /// 切り出しは、指定した位置と大きさの画素だけを、並びを変えずに取り出すこと
    #[test]
    fn test_crop_takes_the_given_rectangle() {
        let mut src = Image::new(4, 3, 2);
        for (i, v) in src.data.iter_mut().enumerate() {
            *v = i as u8;
        }

        let out = crop(&src, 1, 1, 2, 2);

        assert_eq!((out.w, out.h, out.ch), (2, 2, 2));
        assert_eq!(out.data, vec![10, 11, 12, 13, 18, 19, 20, 21]);
    }

    /// JPEG として読めないデータは、読み込み失敗（None）にすること
    #[test]
    fn test_decode_rejects_broken_bytes() {
        assert!(decode(b"not a jpeg").is_none());
        assert!(decode(&fixture_bytes("frontal_face.jpg")[..500]).is_none());
    }

    /// 同じ大きさへの縮小は、元の画像をそのまま返すこと
    #[test]
    fn test_resize_to_same_size_is_copy() {
        let img = random(40, 30, 3, 1);

        assert_eq!(resize(&img, 40, 30, Interp::Linear), img);
    }

    fn all_colors() -> Image {
        let data = (0..1usize << 24)
            .flat_map(|v| [(v & 255) as u8, ((v >> 8) & 255) as u8, (v >> 16) as u8])
            .collect();
        Image {
            w: 4096,
            h: 4096,
            ch: 3,
            data,
        }
    }

    fn image_hash(images: impl IntoIterator<Item = Image>) -> u64 {
        let mut h = Fnv::new();
        for i in images {
            h.bytes(&i.data);
        }
        h.0
    }

    /// 直線補間と面積平均の縮小・拡大の出力が、OpenCV と1画素も違わないことを確かめたときの記録から変わらないこと（決めた14通り×白黒・カラー）
    #[test]
    fn test_resize_stays_recorded() {
        let mut out = Vec::new();
        for (i, &(sw, sh, dw, dh)) in SIZES.iter().enumerate() {
            for ch in [1, 3] {
                let img = random(sw, sh, ch, i as u64 * 10 + ch as u64);
                for interp in [Interp::Linear, Interp::Area] {
                    out.push(resize(&img, dw, dh, interp));
                }
            }
        }
        assert_eq!(image_hash(out), 17476610293254203439);
    }

    /// でたらめに選んだ300通りの大きさの縮小・拡大の出力が、OpenCV と1画素も違わないことを確かめたときの記録から変わらないこと
    #[test]
    fn test_resize_random_sizes_stay_recorded() {
        let mut s = 12345u64;
        let mut next = |n: usize| {
            s = s
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            1 + (s >> 33) as usize % n
        };
        let mut h = Fnv::new();
        for i in 0..300 {
            let (sw, sh, dw, dh, ch) = (
                next(700),
                next(700),
                next(800),
                next(800),
                if next(2) == 1 { 1 } else { 3 },
            );
            let img = random(sw, sh, ch, i);
            for interp in [Interp::Linear, Interp::Area] {
                h.bytes(&resize(&img, dw, dh, interp).data);
            }
        }
        assert_eq!(h.0, 10251832596468864555);
    }

    /// テスト用の写真2枚を読み込んだ結果（カラーと白黒、向きの情報8通り）が記録から変わらないこと（OpenCV との差が白黒1・カラー5以内だと確かめたときの記録）
    #[test]
    fn test_decode_stays_recorded() {
        let mut out = Vec::new();
        for name in ["walking_person.jpg", "frontal_face.jpg"] {
            let bytes = fixture_bytes(name);
            for orientation in 1..=8 {
                let (bgr, gray) = decode(&with_orientation(&bytes, orientation)).unwrap();
                out.push(bgr);
                out.push(gray);
            }
        }
        assert_eq!(image_hash(out), 11663287799714675741);
    }

    fn transform(img: &Image, orientation: u8) -> Image {
        let (w, h, ch) = (img.w, img.h, img.ch);
        let turned = (5..=8).contains(&orientation);
        let (ow, oh) = if turned { (h, w) } else { (w, h) };
        let mut out = Image::new(ow, oh, ch);
        for y in 0..oh {
            for x in 0..ow {
                let (sx, sy) = match orientation {
                    1 => (x, y),
                    2 => (w - 1 - x, y),
                    3 => (w - 1 - x, h - 1 - y),
                    4 => (x, h - 1 - y),
                    5 => (y, x),
                    6 => (y, h - 1 - x),
                    7 => (w - 1 - y, h - 1 - x),
                    _ => (w - 1 - y, x),
                };
                let (o, i) = ((y * ow + x) * ch, (sy * w + sx) * ch);
                out.data[o..o + ch].copy_from_slice(&img.data[i..i + ch]);
            }
        }
        out
    }

    /// 向きの情報が付いた画像は、向きの情報なしで読んだ画像を、その向きのとおりに反転・回転したものと1画素も違わないこと（8通りすべて）
    #[test]
    fn test_decode_orientation_rotates_plain_image() {
        let bytes = fixture_bytes("frontal_face.jpg");
        let (plain, plain_gray) = decode(&bytes).unwrap();
        for orientation in 1..=8 {
            let (bgr, gray) = decode(&with_orientation(&bytes, orientation)).unwrap();
            assert!(bgr == transform(&plain, orientation), "{orientation}");
            assert!(gray == transform(&plain_gray, orientation), "{orientation}");
        }
    }

    /// 整数だけで計算する 0.8 倍の縮小の出力が、OpenCV と1画素も違わないことを確かめたときの記録から変わらないこと
    #[test]
    fn test_resize_linear_exact_stays_recorded() {
        let out = [
            (320usize, 240usize),
            (256, 192),
            (109, 256),
            (3, 2),
            (1, 5),
            (5, 1),
            (17, 13),
            (480, 640),
        ]
        .iter()
        .enumerate()
        .map(|(i, &(w, h))| resize_linear_exact(&random(w, h, 1, i as u64 + 90), 0.8));
        assert_eq!(image_hash(out), 208406618738992418);
    }

    /// 0.8 倍の縮小の大きさは、元の大きさの 0.8 倍を四捨五入した値になること
    #[test]
    fn test_resize_linear_exact_rounds_size() {
        let out = resize_linear_exact(&random(17, 13, 1, 1), 0.8);
        assert_eq!((out.w, out.h), (14, 10));
    }

    /// 平らな画像は、直線補間・面積平均・0.8 倍のどの縮小や拡大をしても平らなままであること
    #[test]
    fn test_flat_image_stays_flat_when_resized() {
        let img = Image {
            w: 37,
            h: 23,
            ch: 3,
            data: vec![93; 37 * 23 * 3],
        };
        for (w, h) in [(10, 7), (37, 23), (80, 50)] {
            for interp in [Interp::Linear, Interp::Area] {
                assert!(resize(&img, w, h, interp).data.iter().all(|&v| v == 93));
            }
        }
        let gray = Image {
            w: 37,
            h: 23,
            ch: 1,
            data: vec![93; 37 * 23],
        };
        assert!(resize_linear_exact(&gray, 0.8)
            .data
            .iter()
            .all(|&v| v == 93));
    }

    /// 縦横ちょうど半分への面積平均の縮小は、2×2 の4画素の平均になること
    #[test]
    fn test_area_halves_by_block_average() {
        let img = Image {
            w: 4,
            h: 2,
            ch: 1,
            data: vec![0, 4, 8, 8, 4, 8, 0, 0],
        };
        assert_eq!(resize(&img, 2, 1, Interp::Area).data, vec![4, 4]);
    }

    /// 白黒・HSV・Lab への変換の出力が、約1677万色すべてで OpenCV と同じことを確かめたときの記録から変わらないこと
    #[test]
    fn test_color_conversions_stay_recorded() {
        let img = all_colors();
        assert_eq!(
            image_hash([gray(&img), hsv(&img), lab(&img)]),
            7076915452816866366
        );
    }

    fn px(bgr: [u8; 3]) -> Image {
        Image {
            w: 1,
            h: 1,
            ch: 3,
            data: bgr.to_vec(),
        }
    }

    /// 灰色の画素は白黒にしても同じ明るさで、HSV ではあざやかさが 0 になること
    #[test]
    fn test_gray_pixels_keep_brightness() {
        for v in [0, 1, 77, 128, 254, 255] {
            assert_eq!(gray(&px([v, v, v])).data, vec![v]);
            assert_eq!(hsv(&px([v, v, v])).data, vec![0, 0, v]);
        }
    }

    /// 赤・緑・青の純色は、HSV の色合い（0〜180 の目盛り）がそれぞれ 0・60・120 で、あざやかさと明るさが最大になること
    #[test]
    fn test_pure_colors_in_hsv() {
        assert_eq!(hsv(&px([0, 0, 255])).data, vec![0, 255, 255]);
        assert_eq!(hsv(&px([0, 255, 0])).data, vec![60, 255, 255]);
        assert_eq!(hsv(&px([255, 0, 0])).data, vec![120, 255, 255]);
    }

    /// Lab では、白は明るさが最大、黒は明るさが 0 で、どちらも色み（a・b）が真ん中の 128 になること
    #[test]
    fn test_white_and_black_in_lab() {
        assert_eq!(lab(&px([255, 255, 255])).data, vec![255, 128, 128]);
        assert_eq!(lab(&px([0, 0, 0])).data, vec![0, 128, 128]);
    }

    /// モデルに渡す数字の並びの出力が、OpenCV と同じことを確かめたときの記録から変わらないこと（CLIP・場所当て・文字探しで使う組み合わせ）
    #[test]
    fn test_blob_stays_recorded() {
        let img = random(64, 48, 3, 7);
        let ocr_mean = [122.67891434f64, 116.66876762, 104.00698793].map(|v| v as f32);
        let mut h = Fnv::new();
        h.f32s(&blob(&img, true, [0.0; 3], (1.0f64 / 255.0) as f32));
        h.f32s(&blob(&img, false, [104.0, 117.0, 123.0], 1.0));
        h.f32s(&blob(
            &resize(&img, 736, 736, Interp::Linear),
            true,
            ocr_mean,
            (1.0f64 / 255.0) as f32,
        ));
        assert_eq!(h.0, 7415101158874820616);
    }

    /// モデルに渡す数字の並びは、色ごとにまとめて並べ（入れ替えありなら赤・緑・青の順）、色ごとに平均を引いてから倍率をかけること
    #[test]
    fn test_blob_layout() {
        let img = Image {
            w: 2,
            h: 1,
            ch: 3,
            data: vec![10, 20, 30, 40, 50, 60],
        };
        assert_eq!(
            blob(&img, false, [0.0; 3], 1.0),
            vec![10.0, 40.0, 20.0, 50.0, 30.0, 60.0]
        );
        assert_eq!(
            blob(&img, true, [0.0; 3], 1.0),
            vec![30.0, 60.0, 20.0, 50.0, 10.0, 40.0]
        );
        assert_eq!(
            blob(&img, false, [10.0, 20.0, 30.0], 0.5),
            vec![0.0, 15.0, 0.0, 15.0, 0.0, 15.0]
        );
    }
}
