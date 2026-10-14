use crate::img::Image;

pub fn canny(src: &Image, low: f64, high: f64) -> Image {
    assert_eq!(src.ch, 1);
    let (w, h) = (src.w, src.h);
    let px = |x: isize, y: isize| {
        let x = x.clamp(0, w as isize - 1) as usize;
        let y = y.clamp(0, h as isize - 1) as usize;
        src.data[y * w + x] as i32
    };
    let mut dx = vec![0i32; w * h];
    let mut dy = vec![0i32; w * h];
    for y in 0..h as isize {
        for x in 0..w as isize {
            let gx = (px(x + 1, y - 1) + 2 * px(x + 1, y) + px(x + 1, y + 1))
                - (px(x - 1, y - 1) + 2 * px(x - 1, y) + px(x - 1, y + 1));
            let gy = (px(x - 1, y + 1) + 2 * px(x, y + 1) + px(x + 1, y + 1))
                - (px(x - 1, y - 1) + 2 * px(x, y - 1) + px(x + 1, y - 1));
            dx[y as usize * w + x as usize] = gx.clamp(i16::MIN as i32, i16::MAX as i32);
            dy[y as usize * w + x as usize] = gy.clamp(i16::MIN as i32, i16::MAX as i32);
        }
    }
    let mag = |x: isize, y: isize| -> i32 {
        if x < 0 || y < 0 || x >= w as isize || y >= h as isize {
            0
        } else {
            let i = y as usize * w + x as usize;
            dx[i].abs() + dy[i].abs()
        }
    };
    let (low, high) = (low.floor() as i32, high.floor() as i32);
    let (low, high) = (low.min(high), low.max(high));
    let mw = w + 2;
    let mut map = vec![1u8; mw * (h + 2)];
    let mut stack = Vec::new();
    const TG22: i32 = 13573;
    for y in 0..h {
        for x in 0..w {
            let (xi, yi) = (x as isize, y as isize);
            let m = mag(xi, yi);
            let cell = (y + 1) * mw + x + 1;
            let mut kept = false;
            if m > low {
                let (xs, ys) = (dx[y * w + x], dy[y * w + x]);
                let ax = xs.abs();
                let ay = ys.abs() << 15;
                let tg22x = ax * TG22;
                kept = if ay < tg22x {
                    m > mag(xi - 1, yi) && m >= mag(xi + 1, yi)
                } else {
                    let tg67x = tg22x + (ax << 16);
                    if ay > tg67x {
                        m > mag(xi, yi - 1) && m >= mag(xi, yi + 1)
                    } else {
                        let s: isize = if (xs ^ ys) < 0 { -1 } else { 1 };
                        m > mag(xi - s, yi - 1) && m > mag(xi + s, yi + 1)
                    }
                };
            }
            map[cell] = if !kept {
                1
            } else if m > high {
                stack.push(cell);
                2
            } else {
                0
            };
        }
    }
    while let Some(c) = stack.pop() {
        for n in [
            c - mw - 1,
            c - mw,
            c - mw + 1,
            c - 1,
            c + 1,
            c + mw - 1,
            c + mw,
            c + mw + 1,
        ] {
            if map[n] == 0 {
                map[n] = 2;
                stack.push(n);
            }
        }
    }
    let mut out = Image::new(w, h, 1);
    for y in 0..h {
        for x in 0..w {
            out.data[y * w + x] = if map[(y + 1) * mw + x + 1] == 2 {
                255
            } else {
                0
            };
        }
    }
    out
}

pub(crate) struct Rng(pub(crate) u64);

impl Rng {
    pub(crate) fn next(&mut self) -> u32 {
        self.0 = (self.0 as u32 as u64)
            .wrapping_mul(4164903690)
            .wrapping_add(self.0 >> 32);
        self.0 as u32
    }

    pub(crate) fn next_f64(&mut self) -> f64 {
        let t = self.next() as u64;
        ((t << 32) | self.next() as u64) as f64 * 5.421_010_862_427_522e-20
    }

    pub(crate) fn uniform(&mut self, a: i32, b: i32) -> i32 {
        if a == b {
            a
        } else {
            (self.next() % (b - a) as u32) as i32 + a
        }
    }
}

pub fn hough_lines_p(
    edges: &Image,
    rho: f64,
    theta: f64,
    threshold: i32,
    min_len: f64,
    max_gap: f64,
) -> Vec<[i32; 4]> {
    let (rho, theta) = (rho as f32, theta as f32);
    let (line_length, line_gap) = (
        min_len.round_ties_even() as i32,
        max_gap.round_ties_even() as i32,
    );
    let irho = 1.0 / rho;
    let mut rng = Rng(u64::MAX);
    let (width, height) = (edges.w as i32, edges.h as i32);
    let step = theta as f64;
    let mut numangle = ((std::f64::consts::PI - 0.0) / step).floor() as i32 + 1;
    if numangle > 1 && (std::f64::consts::PI - (numangle - 1) as f64 * step).abs() < step / 2.0 {
        numangle -= 1;
    }
    let numrho = ((((width + height) * 2 + 1) as f32) / rho).round_ties_even() as i32;
    let mut accum = vec![0i32; (numangle * numrho) as usize];
    let trig: Vec<(f32, f32)> = (0..numangle)
        .map(|n| {
            let a = n as f64 * step;
            (
                (a.cos() * irho as f64) as f32,
                (a.sin() * irho as f64) as f32,
            )
        })
        .collect();
    let mut mask = vec![0u8; (width * height) as usize];
    let mut nzloc: Vec<(i32, i32)> = Vec::new();
    for y in 0..height {
        for x in 0..width {
            if edges.data[(y * width + x) as usize] != 0 {
                mask[(y * width + x) as usize] = 1;
                nzloc.push((x, y));
            }
        }
    }
    let rho_of = |j: i64, i: i64, n: usize| {
        ((j as f32 * trig[n].0 + i as f32 * trig[n].1).round_ties_even() as i32) + (numrho - 1) / 2
    };
    let mut lines = Vec::new();
    let shift = 16;
    let mut count = nzloc.len() as i32;
    while count > 0 {
        let idx = rng.uniform(0, count) as usize;
        let (j, i) = nzloc[idx];
        nzloc[idx] = nzloc[count as usize - 1];
        count -= 1;
        if mask[(i * width + j) as usize] == 0 {
            continue;
        }
        let (mut max_val, mut max_n) = (threshold - 1, 0usize);
        for n in 0..numangle as usize {
            let r = rho_of(j as i64, i as i64, n);
            let cell = &mut accum[n * numrho as usize + r as usize];
            *cell += 1;
            if max_val < *cell {
                max_val = *cell;
                max_n = n;
            }
        }
        if max_val < threshold {
            continue;
        }
        let a = -trig[max_n].1;
        let b = trig[max_n].0;
        let (mut x0, mut y0) = (j as i64, i as i64);
        let (xflag, dx0, dy0);
        if a.abs() > b.abs() {
            xflag = true;
            dx0 = if a > 0.0 { 1 } else { -1 };
            dy0 = (b * (1 << shift) as f32 / a.abs()).round_ties_even() as i64;
            y0 = (y0 << shift) + (1 << (shift - 1));
        } else {
            xflag = false;
            dy0 = if b > 0.0 { 1 } else { -1 };
            dx0 = (a * (1 << shift) as f32 / b.abs()).round_ties_even() as i64;
            x0 = (x0 << shift) + (1 << (shift - 1));
        }
        let pos = |x: i64, y: i64| {
            if xflag {
                (x, y >> shift)
            } else {
                (x >> shift, y)
            }
        };
        let mut ends = [(0i64, 0i64); 2];
        for (k, end) in ends.iter_mut().enumerate() {
            let mut gap = 0;
            let (mut x, mut y) = (x0, y0);
            let (dx, dy) = if k > 0 { (-dx0, -dy0) } else { (dx0, dy0) };
            loop {
                let (j1, i1) = pos(x, y);
                if j1 < 0 || j1 >= width as i64 || i1 < 0 || i1 >= height as i64 {
                    break;
                }
                if mask[(i1 * width as i64 + j1) as usize] != 0 {
                    gap = 0;
                    *end = (j1, i1);
                } else {
                    gap += 1;
                    if gap > line_gap {
                        break;
                    }
                }
                x += dx;
                y += dy;
            }
        }
        let good = (ends[1].0 - ends[0].0).abs() >= line_length as i64
            || (ends[1].1 - ends[0].1).abs() >= line_length as i64;
        for (k, end) in ends.iter().enumerate() {
            let (mut x, mut y) = (x0, y0);
            let (dx, dy) = if k > 0 { (-dx0, -dy0) } else { (dx0, dy0) };
            loop {
                let (j1, i1) = pos(x, y);
                let cell = (i1 * width as i64 + j1) as usize;
                if mask[cell] != 0 {
                    if good {
                        for n in 0..numangle as usize {
                            let r = rho_of(j1, i1, n);
                            accum[n * numrho as usize + r as usize] -= 1;
                        }
                    }
                    mask[cell] = 0;
                }
                if (j1, i1) == *end {
                    break;
                }
                x += dx;
                y += dy;
            }
        }
        if good {
            lines.push([
                ends[0].0 as i32,
                ends[0].1 as i32,
                ends[1].0 as i32,
                ends[1].1 as i32,
            ]);
        }
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
            *v = 90 + rnd(30) as u8;
        }
        for _ in 0..12 {
            let (x0, y0, x1, y1) = (
                rnd(w as u32) as i32,
                rnd(h as u32) as i32,
                rnd(w as u32) as i32,
                rnd(h as u32) as i32,
            );
            let v = rnd(256) as u8;
            let steps = (x1 - x0).abs().max((y1 - y0).abs()).max(1);
            for t in 0..=steps {
                let x = x0 + (x1 - x0) * t / steps;
                let y = y0 + (y1 - y0) * t / steps;
                for d in 0..2 {
                    let (xx, yy) = ((x + d).min(w as i32 - 1) as usize, y as usize);
                    img.data[yy * w + xx] = v;
                }
            }
        }
        for _ in 0..4 {
            let (x0, y0) = (rnd(w as u32) as usize, rnd(h as u32) as usize);
            let (rw, rh) = (1 + rnd(60) as usize, 1 + rnd(60) as usize);
            let v = rnd(256) as u8;
            for y in y0..(y0 + rh).min(h) {
                for x in x0..(x0 + rw).min(w) {
                    img.data[y * w + x] = v;
                }
            }
        }
        img
    }

    const SIZES: [(usize, usize); 6] =
        [(256, 192), (109, 256), (3, 2), (17, 5), (384, 288), (1, 1)];

    use crate::testing::Fnv;

    /// 輪郭線の出力が、OpenCV と1画素も違わないことを確かめたときの記録から変わらないこと
    #[test]
    fn test_canny_stays_recorded() {
        let mut h = Fnv::new();
        for (i, &(w, ht)) in SIZES.iter().enumerate() {
            h.bytes(&canny(&scene(w, ht, i as u64), 80.0, 200.0).data);
        }
        assert_eq!(h.0, 6755413773359600913);
    }

    /// 確率的 Hough 変換で見つけた直線が、OpenCV と同じことを確かめたときの記録から変わらないこと
    #[test]
    fn test_hough_lines_p_stays_recorded() {
        let mut h = Fnv::new();
        for (i, &(w, ht)) in SIZES.iter().enumerate() {
            let edges = canny(&scene(w, ht, i as u64 + 10), 80.0, 200.0);
            for min_len in [((w.min(ht) as f64) * 0.25).floor(), 10.0] {
                let segs =
                    hough_lines_p(&edges, 1.0, std::f64::consts::PI / 180.0, 40, min_len, 2.0);
                h.i32s(&[segs.len() as i32]);
                for s in segs {
                    h.i32s(&s);
                }
            }
        }
        assert_eq!(h.0, 13721575249581295409);
    }

    /// 平らな画像からは、輪郭線も直線も見つからないこと
    #[test]
    fn test_flat_image_has_no_edges_or_lines() {
        let img = Image {
            w: 64,
            h: 48,
            ch: 1,
            data: vec![128; 64 * 48],
        };
        let edges = canny(&img, 80.0, 200.0);
        assert!(edges.data.iter().all(|&v| v == 0));
        assert!(hough_lines_p(&edges, 1.0, std::f64::consts::PI / 180.0, 40, 10.0, 2.0).is_empty());
    }

    /// 左が黒・右が白の境目では、輪郭線（255）が境目の縦1列だけに出ること
    #[test]
    fn test_canny_marks_one_column_at_step() {
        let (w, ht) = (40, 30);
        let img = Image {
            w,
            h: ht,
            ch: 1,
            data: (0..w * ht)
                .map(|i| if i % w < 20 { 0 } else { 255 })
                .collect(),
        };
        let edges = canny(&img, 80.0, 200.0);
        let cols: std::collections::BTreeSet<usize> = edges
            .data
            .iter()
            .enumerate()
            .filter(|(_, &v)| v == 255)
            .map(|(i, _)| i % w)
            .collect();
        assert_eq!(cols.len(), 1, "{cols:?}");
        let c = *cols.iter().next().unwrap();
        assert!((19..=20).contains(&c), "{c}");
        assert!(edges.data.iter().all(|&v| v == 0 || v == 255));
    }

    /// 輪郭線の画像に引いた長い横線は、横向きで長さが最短の長さ以上の直線として見つかること
    #[test]
    fn test_hough_finds_long_horizontal_line() {
        let (w, ht) = (100, 60);
        let mut edges = Image::new(w, ht, 1);
        for x in 10..90 {
            edges.data[30 * w + x] = 255;
        }
        let segs = hough_lines_p(&edges, 1.0, std::f64::consts::PI / 180.0, 40, 50.0, 2.0);
        assert!(!segs.is_empty());
        for s in segs {
            assert_eq!((s[1], s[3]), (30, 30), "{s:?}");
            assert!((s[2] - s[0]).abs() >= 50, "{s:?}");
        }
    }
}
