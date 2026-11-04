mod judge;

pub use judge::judge;

use crate::cv;
use vision::filter;
use vision::img::{self, Image, Interp};
use vision::shape;

const SIGMAS: [f64; 4] = [0.7, 1.4, 2.8, 5.6];
const KS: [i32; 3] = [6, 8, 10];

#[derive(Debug, Clone, PartialEq)]
pub struct Nearness {
    pub rt_min: f64,
    pub ln_count: Option<f64>,
    pub top_smooth: f64,
}

struct Region {
    scale: f64,
    core: Vec<bool>,
}

pub fn measure(bgr: &Image, gray_decoded: &Image) -> anyhow::Result<Nearness> {
    let small = fit_truncated(bgr, 512)?;
    let gray = img::gray(&small);
    let bands = bands(&gray);
    let w = small.w;

    let segs = vision::lsd::detect(&gray);

    let mut rt_mins = Vec::new();
    let mut lines = Vec::new();
    for k in KS {
        let found = textured_regions(&small, k, &bands);
        if let Some(finest) = finest(&found) {
            rt_mins.push(finest.scale);
            let (mut n, mut total) = (0usize, 0f64);
            for s in segs.iter() {
                let (x1, y1, x2, y2) = (s[0] as f64, s[1] as f64, s[2] as f64, s[3] as f64);
                let (cx, cy) = (((x1 + x2) / 2.0) as i64, ((y1 + y2) / 2.0) as i64);
                let inside = cy >= 0
                    && cx >= 0
                    && (cy as usize) < small.h
                    && (cx as usize) < w
                    && finest.core[cy as usize * w + cx as usize];
                let len = (x2 - x1).hypot(y2 - y1);
                if inside && len >= 8.0 {
                    n += 1;
                    total += len;
                }
            }
            let area = finest.core.iter().filter(|&&c| c).count() as f64 / 1000.0;
            lines.push((finest.scale, n as f64 / area, total / area));
        } else {
            rt_mins.push(f64::NAN);
        }
    }

    let found: Vec<f64> = rt_mins.iter().copied().filter(|v| !v.is_nan()).collect();
    let mut values = if found.is_empty() {
        vec![0.0; 3]
    } else {
        found
    };
    values.sort_by(f64::total_cmp);
    let rt_min = values[values.len() / 2];

    lines.sort_by(|a, b| {
        a.0.total_cmp(&b.0)
            .then(a.1.total_cmp(&b.1))
            .then(a.2.total_cmp(&b.2))
    });
    let ln_count = (!lines.is_empty()).then(|| lines[lines.len() / 2].1);

    Ok(Nearness {
        rt_min,
        ln_count,
        top_smooth: top_smooth(gray_decoded)?,
    })
}

fn finest(regions: &[Region]) -> Option<&Region> {
    regions.iter().fold(None, |best, r| match best {
        Some(b) if b.scale <= r.scale => Some(b),
        _ => Some(r),
    })
}

fn fit_truncated(src: &Image, side: usize) -> anyhow::Result<Image> {
    let s = side as f64 / src.h.max(src.w) as f64;
    let (w, h) = ((src.w as f64 * s) as usize, (src.h as f64 * s) as usize);
    anyhow::ensure!(w > 0 && h > 0, "image is too narrow to shrink");
    Ok(img::resize(src, w, h, Interp::Area))
}

fn bands(gray: &Image) -> Vec<Vec<f32>> {
    let g = cv::plane(gray);
    let mut blurred = vec![g.data.clone()];
    for sigma in SIGMAS {
        blurred.push(filter::gaussian(&g, sigma).data);
    }
    (0..SIGMAS.len())
        .map(|i| {
            blurred[i]
                .iter()
                .zip(&blurred[i + 1])
                .map(|(a, b)| (a - b).abs())
                .collect()
        })
        .collect()
}

fn textured_regions(small: &Image, k: i32, bands: &[Vec<f32>]) -> Vec<Region> {
    let mut out = Vec::new();
    for mask in color_regions(small, k) {
        let m = Image {
            w: small.w,
            h: small.h,
            ch: 1,
            data: mask,
        };
        let eroded = filter::erode_rect(&m, 13, 13);
        let core: Vec<bool> = eroded.data.iter().map(|&v| v != 0).collect();
        let count = core.iter().filter(|&&c| c).count();
        if count < 200 {
            continue;
        }
        let e: Vec<f64> = bands
            .iter()
            .map(|band| {
                band.iter()
                    .zip(&core)
                    .filter(|(_, &c)| c)
                    .map(|(&v, _)| v as f64)
                    .sum::<f64>()
                    / count as f64
            })
            .collect();
        let sum: f64 = e.iter().sum();
        if sum < 4.0 {
            continue;
        }
        let scale = e.iter().zip(SIGMAS).map(|(v, s)| v / sum * s.log2()).sum();
        out.push(Region { scale, core });
    }
    out
}

fn color_regions(small: &Image, k: i32) -> Vec<Vec<u8>> {
    let blur = filter::gaussian_u8(small, 0, 6.0);
    let samples: Vec<f32> = img::lab(&blur).data.iter().map(|&v| v as f32).collect();
    let labels = vision::kmeans::kmeans_pp(&samples, 3, k as usize, 20, 1.0, 2, 12345);
    split_components(&labels, small.h, small.w, k)
}

fn split_components(labels: &[i32], h: usize, w: usize, k: i32) -> Vec<Vec<u8>> {
    let total = labels.len();
    let mut out = Vec::new();
    for c in 0..k {
        let mask: Vec<u8> = labels.iter().map(|&l| (l == c) as u8).collect();
        let (cc, areas) = shape::connected_components(&mask, w, h, true);
        for (i, &a) in areas.iter().enumerate().skip(1) {
            if a as f64 >= 0.03 * total as f64 {
                out.push(cc.iter().map(|&v| (v == i as i32) as u8).collect());
            }
        }
    }
    out
}

fn top_smooth(gray_decoded: &Image) -> anyhow::Result<f64> {
    let g = cv::plane(&fit_truncated(gray_decoded, 256)?);
    let (rows, cols) = (g.h, g.w);
    let mean = filter::box_mean(&g, 9, 9).data;
    let sq = filter::box_mean(&filter::square(&g), 9, 9).data;
    let top = (rows / 3) * cols;
    let smooth = (0..top)
        .filter(|&i| (sq[i] - mean[i] * mean[i]).max(0.0).sqrt() < 6.0)
        .count();
    Ok(smooth as f64 / top as f64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cv::testing::{fill, fixture, gray_of, noise, solid, Rect};

    fn near(img: &Image) -> Nearness {
        measure(img, &gray_of(img)).unwrap()
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    /// テスト用の写真で測った模様の粗さ・線の密度・上部のなめらかさが、記録した値から変わらないこと（色の分け方や塊のつながり方が変わったら気づくため）
    #[test]
    fn test_fixture_measurements_stay_the_same() {
        let cases = [
            (
                "walking_person.jpg",
                1.2080315145718048,
                9.287925696594426,
                0.2546426835196889,
            ),
            (
                "frontal_face.jpg",
                1.151340839654748,
                2.0334928229665072,
                0.5171464330413016,
            ),
        ];
        for (name, rt_min, ln_count, top_smooth) in cases {
            let n = near(&fixture(name));
            assert!(close(n.rt_min, rt_min), "{name} rt_min={}", n.rt_min);
            assert!(
                close(n.ln_count.unwrap(), ln_count),
                "{name} ln_count={:?}",
                n.ln_count
            );
            assert!(
                close(n.top_smooth, top_smooth),
                "{name} top_smooth={}",
                n.top_smooth
            );
        }
    }

    /// 全体に細かい模様が広がる画像（遠くの景色のような）は、手前の被写体が写った写真よりも模様の粗さが小さいこと
    #[test]
    fn test_fine_texture_is_less_coarse_than_close_subject() {
        let fine = near(&noise(512, 384, (60.0, 120.0, 60.0), 40.0, 21));
        let photo = near(&fixture("walking_person.jpg"));

        assert!(fine.rt_min < 0.3, "{}", fine.rt_min);
        assert!(photo.rt_min > 0.9, "{}", photo.rt_min);
    }

    /// 模様がまったく無い画像は、測る場所が無いので、粗さを 0、線の密度を「測れない」にすること
    #[test]
    fn test_flat_image_has_nothing_to_measure() {
        let n = near(&solid(512, 384, (60.0, 120.0, 60.0)));

        assert_eq!(n.rt_min, 0.0);
        assert_eq!(n.ln_count, None);
    }

    /// 画面の上3分の1がなめらかな空なら上部のなめらかさは高く、細かい模様なら 0 になること
    #[test]
    fn test_top_smooth_looks_at_upper_third() {
        let mut sky = noise(512, 384, (60.0, 120.0, 60.0), 40.0, 24);
        fill(&mut sky, Rect::new(0, 0, 512, 128), (220.0, 160.0, 90.0));

        assert!(near(&sky).top_smooth > 0.9);
        assert_eq!(
            near(&noise(512, 384, (60.0, 120.0, 60.0), 40.0, 25)).top_smooth,
            0.0
        );
    }

    fn region(scale: f64, id: bool) -> Region {
        Region {
            scale,
            core: vec![id],
        }
    }

    fn sizes(regions: &[Vec<u8>]) -> Vec<usize> {
        regions
            .iter()
            .map(|m| m.iter().filter(|&&v| v != 0).count())
            .collect()
    }

    /// 一番細かい部分は粗さが最小のものを選び、同じ粗さが並んだときは先に見つけた方を選ぶこと（試作と同じ）
    #[test]
    fn test_finest_picks_smallest_scale_and_first_on_tie() {
        let regions = [region(0.8, false), region(0.5, true), region(0.5, false)];

        let f = finest(&regions).unwrap();

        assert_eq!((f.scale, f.core[0]), (0.5, true));
        assert!(finest(&[]).is_none());
    }

    /// 同じ色の部分が斜めにだけ接していても、1つの塊として数えること（試作が実際に動いていた8方向のつながり方）
    #[test]
    fn test_diagonal_touch_joins_into_one_region() {
        let mut labels = vec![0i32; 100];
        for (x0, y0) in [(0, 0), (3, 3)] {
            for y in y0..y0 + 3 {
                for x in x0..x0 + 3 {
                    labels[y * 10 + x] = 1;
                }
            }
        }

        let regions = split_components(&labels, 10, 10, 2);

        assert_eq!(sizes(&regions), vec![82, 18]);
    }

    /// 画面の 3% に満たない小さな塊は、部分として扱わないこと
    #[test]
    fn test_tiny_components_are_dropped() {
        let mut labels = vec![0i32; 100];
        for i in [11, 12, 13, 55] {
            labels[i] = 1;
        }

        let regions = split_components(&labels, 10, 10, 2);

        assert_eq!(sizes(&regions), vec![96, 3]);
    }

    /// 読み込めなかった（空の）画像は、止まらずに「測れない」として返すこと
    #[test]
    fn test_empty_image_is_an_error() {
        assert!(measure(&Image::new(0, 0, 3), &Image::new(0, 0, 1)).is_err());
    }
}
