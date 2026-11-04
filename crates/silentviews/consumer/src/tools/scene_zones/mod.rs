mod judge;

pub use judge::judge;

use crate::cv;
use vision::filter;
use vision::img::{self, Image};
use vision::shape;

const COLS: usize = 12;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Zone {
    Sky,
    Far,
    Near,
}

pub struct Zones {
    pub sky: f64,
    pub near_blob: f64,
    pub center_near: f64,
    pub side_near: f64,
}

struct Cell {
    fine: f32,
    coarse: f32,
    h: f64,
    s: f64,
    v: f64,
}

pub fn measure(bgr: &Image) -> anyhow::Result<Zones> {
    let small = cv::fit(bgr, 256, true);
    let (h, w) = (small.h, small.w);
    let cs = w / COLS;
    anyhow::ensure!(cs > 0 && h >= cs, "image is too narrow to split into cells");
    let rows = h / cs;
    let cells = cells(&small, rows, cs);
    let zones = classify(&cells, rows);

    let n = zones.len() as f64;
    let count = |pred: &dyn Fn(usize, usize) -> bool| -> f64 {
        let mut k = 0;
        let mut total = 0;
        for r in 0..rows {
            for c in 0..COLS {
                if pred(r, c) {
                    total += 1;
                    k += (zones[r * COLS + c] == Zone::Near) as usize;
                }
            }
        }
        if total == 0 {
            f64::NAN
        } else {
            k as f64 / total as f64
        }
    };
    let (r0, r1) = (rows / 5, rows - rows / 5);
    let (c0, c1) = (COLS / 4, COLS - COLS / 4);

    Ok(Zones {
        sky: zones.iter().filter(|&&z| z == Zone::Sky).count() as f64 / n,
        near_blob: largest_near_blob(&zones, rows),
        center_near: count(&|r, c| r >= r0 && r < r1 && c >= c0 && c < c1),
        side_near: count(&|r, c| r >= r0 && r < r1 && (c < c0 || c >= c1)),
    })
}

fn cells(small: &Image, rows: usize, cs: usize) -> Vec<Cell> {
    let w = small.w;
    let gray = cv::plane(&img::gray(small));
    let hsv_img = img::hsv(small);
    let hsv = cv::pixels(&hsv_img);

    let fine: Vec<f32> = filter::laplacian(&gray)
        .data
        .iter()
        .map(|v| v.abs())
        .collect();

    let blur = filter::gaussian(&gray, 4.0);
    let (gx, gy) = (filter::sobel_x(&blur).data, filter::sobel_y(&blur).data);

    let mut out = Vec::with_capacity(rows * COLS);
    for r in 0..rows {
        for c in 0..COLS {
            let (mut fine_sum, mut coarse_sum) = (0f64, 0f64);
            let (mut hs, mut ss, mut vs) = (Vec::new(), Vec::new(), Vec::new());
            for y in r * cs..(r + 1) * cs {
                for x in c * cs..(c + 1) * cs {
                    let i = y * w + x;
                    fine_sum += fine[i] as f64;
                    coarse_sum += (gx[i] * gx[i] + gy[i] * gy[i]).sqrt() as f64;
                    hs.push(hsv[i][0]);
                    ss.push(hsv[i][1]);
                    vs.push(hsv[i][2]);
                }
            }
            let k = (cs * cs) as f64;
            out.push(Cell {
                fine: (fine_sum / k) as f32,
                coarse: (coarse_sum / k) as f32,
                h: median(&mut hs),
                s: median(&mut ss),
                v: median(&mut vs),
            });
        }
    }
    out
}

fn median(values: &mut [u8]) -> f64 {
    values.sort_unstable();
    let n = values.len();
    if n % 2 == 1 {
        values[n / 2] as f64
    } else {
        (values[n / 2 - 1] as f64 + values[n / 2] as f64) / 2.0
    }
}

fn classify(cells: &[Cell], rows: usize) -> Vec<Zone> {
    let candidate: Vec<bool> = cells
        .iter()
        .map(|c| {
            let smooth = c.fine < 6.0;
            let skycolor = ((90.0..=130.0).contains(&c.h) && c.s >= 25.0 && c.v >= 90.0)
                || (c.s < 40.0 && c.v >= 150.0);
            smooth && skycolor
        })
        .collect();

    let mut zone: Vec<Option<Zone>> = vec![None; cells.len()];
    let mut stack: Vec<(usize, usize)> = (0..COLS)
        .filter(|&c| candidate[c])
        .map(|c| (0, c))
        .collect();
    while let Some((r, c)) = stack.pop() {
        if zone[r * COLS + c] == Some(Zone::Sky) {
            continue;
        }
        zone[r * COLS + c] = Some(Zone::Sky);
        let neighbors = [
            (r + 1, c as isize),
            (r.wrapping_sub(1), c as isize),
            (r, c as isize + 1),
            (r, c as isize - 1),
        ];
        for (rr, cc) in neighbors {
            if rr < rows && cc >= 0 && (cc as usize) < COLS {
                let i = rr * COLS + cc as usize;
                if candidate[i] && zone[i] != Some(Zone::Sky) {
                    stack.push((rr, cc as usize));
                }
            }
        }
    }

    for (i, cell) in cells.iter().enumerate() {
        if zone[i] != Some(Zone::Sky) {
            let ratio = cell.fine / (cell.coarse + 1.0);
            let far = cell.fine >= 8.0 && ratio >= 1.2;
            zone[i] = Some(if far || candidate[i] {
                Zone::Far
            } else {
                Zone::Near
            });
        }
    }
    let mut zones: Vec<Zone> = zone.into_iter().map(|z| z.unwrap()).collect();

    for c in 0..COLS {
        let Some(lowest) = (0..rows).rev().find(|&r| zones[r * COLS + c] == Zone::Sky) else {
            continue;
        };
        for r in 0..rows.min(lowest + 3) {
            if zones[r * COLS + c] == Zone::Near {
                zones[r * COLS + c] = Zone::Far;
            }
        }
    }
    zones
}

fn largest_near_blob(zones: &[Zone], rows: usize) -> f64 {
    let data: Vec<u8> = zones.iter().map(|&z| (z == Zone::Near) as u8).collect();
    let (_, areas) = shape::connected_components(&data, COLS, rows, false);
    let best = areas.iter().skip(1).copied().max().unwrap_or(0);
    best as f64 / zones.len() as f64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cv::testing::{fill, noise, solid, Rect};

    /// 上の端につながるなめらかな青は空、細かい模様は遠景（遠くの景色）とみなし、近景の塊を作らないこと
    #[test]
    fn test_blue_sky_over_fine_texture() {
        let mut img = noise(384, 288, (60.0, 120.0, 60.0), 40.0, 2);
        fill(&mut img, Rect::new(0, 0, 384, 128), (220.0, 160.0, 90.0));

        let d = measure(&img).unwrap();

        assert!(d.sky > 0.3, "sky={}", d.sky);
        assert_eq!(d.near_blob, 0.0);
    }

    /// 全体がのっぺりした（模様の無い）暗い色の画像は、全部を近景（手前にある物）とみなすこと
    #[test]
    fn test_flat_dark_image_is_all_near() {
        let d = measure(&solid(384, 288, (30.0, 60.0, 30.0))).unwrap();

        assert_eq!(d.sky, 0.0);
        assert_eq!(d.near_blob, 1.0);
        assert_eq!(d.center_near, 1.0);
        assert_eq!(d.side_near, 1.0);
    }

    /// 真ん中だけがのっぺりした画像は、真ん中が近景で左右は近景でないと測ること
    #[test]
    fn test_center_blob_is_near_only_in_center() {
        let mut img = noise(384, 288, (60.0, 120.0, 60.0), 40.0, 3);
        fill(&mut img, Rect::new(96, 0, 192, 288), (30.0, 60.0, 30.0));

        let d = measure(&img).unwrap();

        assert!(d.center_near > 0.9, "center={}", d.center_near);
        assert_eq!(d.side_near, 0.0);
    }

    /// 空のすぐ下（3段まで）にある近景は、遠くの景色の一部とみなして遠景に直すこと
    #[test]
    fn test_near_cells_just_below_sky_become_far() {
        let mut img = solid(384, 288, (30.0, 60.0, 30.0));
        fill(&mut img, Rect::new(0, 0, 384, 32), (220.0, 160.0, 90.0));

        let d = measure(&img).unwrap();

        assert!(
            d.near_blob < 1.0 - d.sky - 0.2,
            "near_blob={} sky={}",
            d.near_blob,
            d.sky
        );
    }

    /// のっぺりした塊が2つあるときは、大きい方の広さを近景の塊とすること
    #[test]
    fn test_near_blob_is_the_largest_one() {
        let mut img = noise(384, 288, (60.0, 120.0, 60.0), 40.0, 9);
        fill(&mut img, Rect::new(0, 0, 64, 64), (30.0, 60.0, 30.0));
        fill(&mut img, Rect::new(0, 160, 384, 128), (30.0, 60.0, 30.0));

        let d = measure(&img).unwrap();

        assert!(d.near_blob > 0.3, "near_blob={}", d.near_blob);
    }

    /// 空色でも、上の端とつながっていない部分は空とみなさないこと
    #[test]
    fn test_sky_must_touch_the_top() {
        let mut img = noise(384, 288, (60.0, 120.0, 60.0), 40.0, 10);
        fill(&mut img, Rect::new(0, 128, 384, 64), (220.0, 160.0, 90.0));

        assert_eq!(measure(&img).unwrap().sky, 0.0);
    }

    /// 極端に細長い画像は、マス目に分けられないので、止まらずに「測れない」として返すこと
    #[test]
    fn test_extremely_elongated_image_is_an_error() {
        for (w, h) in [(2000, 20), (20, 2000), (3, 500), (500, 3)] {
            assert!(
                measure(&solid(w, h, (120.0, 120.0, 120.0))).is_err(),
                "{w}x{h}"
            );
        }
    }
}
