use std::f64::consts::PI;

use crate::filter;
use crate::img::{self, Image};

const SCALE: f64 = 0.8;
const SIGMA_SCALE: f64 = 0.6;
const QUANT: f64 = 2.0;
const ANG_TH: f64 = 22.5;
const DENSITY_TH: f64 = 0.7;
const N_BINS: u32 = 1024;
const NOTDEF: f64 = -1024.0;
const DEG_TO_RADS: f64 = PI / 180.0;

pub fn fast_atan2(y: f32, x: f32) -> f32 {
    let deg = (180.0 / PI) as f32;
    let p1 = 0.999_787_87_f32 * deg;
    let p3 = -0.325_808_4_f32 * deg;
    let p5 = 0.155_578_66_f32 * deg;
    let p7 = -0.044_326_555_f32 * deg;
    let (ax, ay) = (x.abs(), y.abs());
    let eps = f64::EPSILON as f32;
    let mut a = if ax >= ay {
        let c = ay / (ax + eps);
        let c2 = c * c;
        (((p7 * c2 + p5) * c2 + p3) * c2 + p1) * c
    } else {
        let c = ax / (ay + eps);
        let c2 = c * c;
        90.0 - (((p7 * c2 + p5) * c2 + p3) * c2 + p1) * c
    };
    if x < 0.0 {
        a = 180.0 - a;
    }
    if y < 0.0 {
        a = 360.0 - a;
    }
    a
}

fn angle_diff_signed(a: f64, b: f64) -> f64 {
    let mut d = a - b;
    while d <= -PI {
        d += 2.0 * PI;
    }
    while d > PI {
        d -= 2.0 * PI;
    }
    d
}

fn dist(x1: f64, y1: f64, x2: f64, y2: f64) -> f64 {
    ((x2 - x1) * (x2 - x1) + (y2 - y1) * (y2 - y1)).sqrt()
}

#[derive(Clone, Copy)]
struct RegionPoint {
    x: i32,
    y: i32,
    angle: f64,
    modgrad: f64,
}

#[derive(Default, Clone, Copy)]
struct Rect {
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
    width: f64,
}

struct Lsd {
    w: i32,
    h: i32,
    angles: Vec<f64>,
    modgrad: Vec<f64>,
    used: Vec<bool>,
}

impl Lsd {
    fn aligned(&self, x: i32, y: i32, theta: f64, prec: f64) -> bool {
        if x < 0 || y < 0 || x >= self.w || y >= self.h {
            return false;
        }
        let a = self.angles[(y * self.w + x) as usize];
        if a == NOTDEF {
            return false;
        }
        let mut n = theta - a;
        if n < 0.0 {
            n = -n;
        }
        if n > (3.0 * PI) / 2.0 {
            n -= 2.0 * PI;
            if n < 0.0 {
                n = -n;
            }
        }
        n <= prec
    }

    fn region_grow(&mut self, sx: i32, sy: i32, prec: f64) -> (Vec<RegionPoint>, f64) {
        let si = (sy * self.w + sx) as usize;
        let mut reg_angle = self.angles[si];
        let mut reg = vec![RegionPoint {
            x: sx,
            y: sy,
            angle: reg_angle,
            modgrad: self.modgrad[si],
        }];
        let mut sumdx = reg_angle.cos() as f32;
        let mut sumdy = reg_angle.sin() as f32;
        self.used[si] = true;
        let mut i = 0;
        while i < reg.len() {
            let p = reg[i];
            for yy in (p.y - 1).max(0)..=(p.y + 1).min(self.h - 1) {
                for xx in (p.x - 1).max(0)..=(p.x + 1).min(self.w - 1) {
                    let k = (yy * self.w + xx) as usize;
                    if !self.used[k] && self.aligned(xx, yy, reg_angle, prec) {
                        let angle = self.angles[k];
                        self.used[k] = true;
                        reg.push(RegionPoint {
                            x: xx,
                            y: yy,
                            angle,
                            modgrad: self.modgrad[k],
                        });
                        sumdx += (angle as f32).cos();
                        sumdy += (angle as f32).sin();
                        reg_angle = fast_atan2(sumdy, sumdx) as f64 * DEG_TO_RADS;
                    }
                }
            }
            i += 1;
        }
        (reg, reg_angle)
    }

    fn get_theta(reg: &[RegionPoint], x: f64, y: f64, reg_angle: f64, prec: f64) -> f64 {
        let (mut ixx, mut iyy, mut ixy) = (0f64, 0f64, 0f64);
        for p in reg {
            let dx = p.x as f64 - x;
            let dy = p.y as f64 - y;
            ixx += dy * dy * p.modgrad;
            iyy += dx * dx * p.modgrad;
            ixy -= dx * dy * p.modgrad;
        }
        let lambda = 0.5 * (ixx + iyy - ((ixx - iyy) * (ixx - iyy) + 4.0 * ixy * ixy).sqrt());
        let mut theta = if ixx.abs() > iyy.abs() {
            fast_atan2((lambda - ixx) as f32, ixy as f32) as f64
        } else {
            fast_atan2(ixy as f32, (lambda - iyy) as f32) as f64
        };
        theta *= DEG_TO_RADS;
        if angle_diff_signed(theta, reg_angle).abs() > prec {
            theta += PI;
        }
        theta
    }

    fn region2rect(reg: &[RegionPoint], reg_angle: f64, prec: f64) -> Rect {
        let (mut x, mut y, mut sum) = (0f64, 0f64, 0f64);
        for p in reg {
            x += p.x as f64 * p.modgrad;
            y += p.y as f64 * p.modgrad;
            sum += p.modgrad;
        }
        x /= sum;
        y /= sum;
        let theta = Self::get_theta(reg, x, y, reg_angle, prec);
        let (dx, dy) = (theta.cos(), theta.sin());
        let (mut l_min, mut l_max, mut w_min, mut w_max) = (0f64, 0f64, 0f64, 0f64);
        for p in reg {
            let rx = p.x as f64 - x;
            let ry = p.y as f64 - y;
            let l = rx * dx + ry * dy;
            let w = -rx * dy + ry * dx;
            if l > l_max {
                l_max = l;
            } else if l < l_min {
                l_min = l;
            }
            if w > w_max {
                w_max = w;
            } else if w < w_min {
                w_min = w;
            }
        }
        let mut width = w_max - w_min;
        if width < 1.0 {
            width = 1.0;
        }
        Rect {
            x1: x + l_min * dx,
            y1: y + l_min * dy,
            x2: x + l_max * dx,
            y2: y + l_max * dy,
            width,
        }
    }

    fn density(reg: &[RegionPoint], r: &Rect) -> f64 {
        reg.len() as f64 / (dist(r.x1, r.y1, r.x2, r.y2) * r.width)
    }

    fn refine(&mut self, reg: &mut Vec<RegionPoint>, prec: f64, rec: &mut Rect) -> bool {
        if Self::density(reg, rec) >= DENSITY_TH {
            return true;
        }
        let (xc, yc) = (reg[0].x as f64, reg[0].y as f64);
        let ang_c = reg[0].angle;
        let (mut sum, mut s_sum, mut n) = (0f64, 0f64, 0i32);
        for p in reg.iter() {
            self.used[(p.y * self.w + p.x) as usize] = false;
            if dist(xc, yc, p.x as f64, p.y as f64) < rec.width {
                let d = angle_diff_signed(p.angle, ang_c);
                sum += d;
                s_sum += d * d;
                n += 1;
            }
        }
        let mean = sum / n as f64;
        let tau = 2.0 * ((s_sum - 2.0 * mean * sum) / n as f64 + mean * mean).sqrt();
        let (sx, sy) = (reg[0].x, reg[0].y);
        let (new_reg, reg_angle) = self.region_grow(sx, sy, tau);
        *reg = new_reg;
        if reg.len() < 2 {
            return false;
        }
        *rec = Self::region2rect(reg, reg_angle, prec);
        let density = Self::density(reg, rec);
        if density < DENSITY_TH {
            self.reduce_region_radius(reg, reg_angle, prec, rec, density)
        } else {
            true
        }
    }

    fn reduce_region_radius(
        &mut self,
        reg: &mut Vec<RegionPoint>,
        reg_angle: f64,
        prec: f64,
        rec: &mut Rect,
        density: f64,
    ) -> bool {
        let (xc, yc) = (reg[0].x as f64, reg[0].y as f64);
        let d2 = |x1: f64, y1: f64, x2: f64, y2: f64| (x2 - x1) * (x2 - x1) + (y2 - y1) * (y2 - y1);
        let r1 = d2(xc, yc, rec.x1, rec.y1);
        let r2 = d2(xc, yc, rec.x2, rec.y2);
        let mut rad_sq = if r1 > r2 { r1 } else { r2 };
        let mut density = density;
        while density < DENSITY_TH {
            rad_sq *= 0.75 * 0.75;
            let mut i = 0;
            while i < reg.len() {
                if d2(xc, yc, reg[i].x as f64, reg[i].y as f64) > rad_sq {
                    self.used[(reg[i].y * self.w + reg[i].x) as usize] = false;
                    let last = reg.len() - 1;
                    reg.swap(i, last);
                    reg.pop();
                } else {
                    i += 1;
                }
            }
            if reg.len() < 2 {
                return false;
            }
            *rec = Self::region2rect(reg, reg_angle, prec);
            density = Self::density(reg, rec);
        }
        true
    }
}

pub fn detect(gray: &Image) -> Vec<[f32; 4]> {
    assert_eq!(gray.ch, 1);
    let prec = PI * ANG_TH / 180.0;
    let p = ANG_TH / 180.0;
    let rho = QUANT / prec.sin();
    let sigma = SIGMA_SCALE / SCALE;
    let half = (sigma * (2.0 * 3.0 * 10f64.ln()).sqrt()).ceil() as usize;
    let scaled = img::resize_linear_exact(&filter::gaussian_u8(gray, 1 + 2 * half, sigma), SCALE);
    let (w, h) = (scaled.w as i32, scaled.h as i32);
    let mut angles = vec![NOTDEF; (w * h) as usize];
    let mut modgrad = vec![0f64; (w * h) as usize];
    let mut max_grad = -1f64;
    let px = |x: i32, y: i32| scaled.data[(y * w + x) as usize] as i32;
    for y in 0..h - 1 {
        for x in 0..w - 1 {
            let da = px(x + 1, y + 1) - px(x, y);
            let bc = px(x + 1, y) - px(x, y + 1);
            let (gx, gy) = (da + bc, da - bc);
            let norm = ((gx * gx + gy * gy) as f64 / 4.0).sqrt();
            let k = (y * w + x) as usize;
            modgrad[k] = norm;
            if norm > rho {
                angles[k] = fast_atan2(gx as f32, -gy as f32) as f64 * DEG_TO_RADS;
                if norm > max_grad {
                    max_grad = norm;
                }
            }
        }
    }
    let bin_coef = if max_grad > 0.0 {
        (N_BINS - 1) as f64 / max_grad
    } else {
        0.0
    };
    let mut ordered: Vec<(i32, i32, i32)> =
        Vec::with_capacity(((w - 1).max(0) * (h - 1).max(0)) as usize);
    for y in 0..h - 1 {
        for x in 0..w - 1 {
            ordered.push((x, y, (modgrad[(y * w + x) as usize] * bin_coef) as i32));
        }
    }
    ordered.sort_by_key(|a| std::cmp::Reverse(a.2));
    let log_nt = 5.0 * ((w as f64).log10() + (h as f64).log10()) / 2.0 + 11f64.log10();
    let min_reg_size = (-log_nt / p.log10()) as usize;
    let mut lsd = Lsd {
        w,
        h,
        angles,
        modgrad,
        used: vec![false; (w * h) as usize],
    };
    let mut lines = Vec::new();
    for &(x, y, _) in &ordered {
        let k = (y * w + x) as usize;
        if lsd.used[k] || lsd.angles[k] == NOTDEF {
            continue;
        }
        let (mut reg, reg_angle) = lsd.region_grow(x, y, prec);
        if reg.len() < min_reg_size {
            continue;
        }
        let mut rec = Lsd::region2rect(&reg, reg_angle, prec);
        if !lsd.refine(&mut reg, prec, &mut rec) {
            continue;
        }
        let f = |v: f64| ((v + 0.5) / SCALE) as f32;
        lines.push([f(rec.x1), f(rec.y1), f(rec.x2), f(rec.y2)]);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scene(w: usize, h: usize, seed: u64) -> Image {
        let mut s = seed;
        let mut rnd = |n: u32| {
            s = s
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (s >> 33) as u32 % n
        };
        let mut img = Image::new(w, h, 1);
        for v in img.data.iter_mut() {
            *v = 100 + rnd(20) as u8;
        }
        for _ in 0..8 {
            let (x0, y0) = (rnd(w as u32) as usize, rnd(h as u32) as usize);
            let (rw, rh) = (1 + rnd(80) as usize, 1 + rnd(80) as usize);
            let v = rnd(256) as u8;
            for y in y0..(y0 + rh).min(h) {
                for x in x0..(x0 + rw).min(w) {
                    img.data[y * w + x] = v;
                }
            }
        }
        for _ in 0..6 {
            let (x0, y0, x1, y1) = (
                rnd(w as u32) as i32,
                rnd(h as u32) as i32,
                rnd(w as u32) as i32,
                rnd(h as u32) as i32,
            );
            let v = rnd(256) as u8;
            let steps = (x1 - x0).abs().max((y1 - y0).abs()).max(1);
            for t in 0..=steps {
                let (x, y) = (
                    (x0 + (x1 - x0) * t / steps) as usize,
                    (y0 + (y1 - y0) * t / steps) as usize,
                );
                img.data[y * w + x] = v;
            }
        }
        img
    }

    use crate::testing::Fnv;

    fn hash_segments(h: &mut Fnv, segs: &[[f32; 4]]) {
        h.i32s(&[segs.len() as i32]);
        for s in segs {
            h.f32s(s);
        }
    }

    /// 角度の近似式の値が、OpenCV の fastAtan2 と同じことを確かめたときの記録から変わらないこと
    #[test]
    fn test_fast_atan2_stays_recorded() {
        let mut h = Fnv::new();
        for (y, x) in [
            (0.0f32, 0.0f32),
            (1.0, 0.0),
            (0.0, -1.0),
            (-3.5, 2.25),
            (7.0, -0.001),
            (-0.3, -0.7),
            (1e-6, 5.0),
            (123.0, 45.6),
        ] {
            h.f32s(&[fast_atan2(y, x)]);
        }
        assert_eq!(h.0, 12298294955879329741);
    }

    /// 線分の出力（両端の座標と順番）が、OpenCV の LSD と同じことを確かめたときの記録から変わらないこと（作った画像6通りと写真2枚）
    #[test]
    fn test_lsd_stays_recorded() {
        let mut h = Fnv::new();
        for (i, &(w, ht)) in [
            (256usize, 192usize),
            (109, 256),
            (384, 288),
            (17, 13),
            (3, 2),
            (256, 256),
        ]
        .iter()
        .enumerate()
        {
            hash_segments(&mut h, &detect(&scene(w, ht, i as u64 + 3)));
        }
        for name in ["walking_person.jpg", "frontal_face.jpg"] {
            hash_segments(&mut h, &detect(&crate::testing::fixture_gray(name)));
        }
        assert_eq!(h.0, 10616821969917149925);
    }

    /// 角度の近似式は、右・上・左・下でそれぞれ 0・90・180・270 度を返し、斜め45度ではほぼ45度になること
    #[test]
    fn test_fast_atan2_directions() {
        assert_eq!(fast_atan2(0.0, 1.0), 0.0);
        assert_eq!(fast_atan2(1.0, 0.0), 90.0);
        assert_eq!(fast_atan2(0.0, -1.0), 180.0);
        assert_eq!(fast_atan2(-1.0, 0.0), 270.0);
        assert!((fast_atan2(1.0, 1.0) - 45.0).abs() < 0.1);
    }

    /// 平らな画像からは線分が見つからないこと
    #[test]
    fn test_flat_image_has_no_segments() {
        assert!(detect(&Image {
            w: 64,
            h: 48,
            ch: 1,
            data: vec![120; 64 * 48]
        })
        .is_empty());
    }

    /// 明るい背景の上の暗い四角からは、四角の4辺に沿った縦と横の線分が見つかること
    #[test]
    fn test_square_gives_horizontal_and_vertical_segments() {
        let (w, ht) = (120, 100);
        let mut img = Image {
            w,
            h: ht,
            ch: 1,
            data: vec![200; w * ht],
        };
        for y in 20..80 {
            for x in 30..90 {
                img.data[y * w + x] = 40;
            }
        }
        let segs = detect(&img);
        let horiz = segs
            .iter()
            .filter(|s| (s[1] - s[3]).abs() < 1.0 && (s[0] - s[2]).abs() > 40.0)
            .count();
        let vert = segs
            .iter()
            .filter(|s| (s[0] - s[2]).abs() < 1.0 && (s[1] - s[3]).abs() > 40.0)
            .count();
        assert!(horiz >= 2 && vert >= 2, "{segs:?}");
    }
}
