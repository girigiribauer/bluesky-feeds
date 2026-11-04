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

    /// 平らな画像からは、輪郭線が見つからないこと
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
}
