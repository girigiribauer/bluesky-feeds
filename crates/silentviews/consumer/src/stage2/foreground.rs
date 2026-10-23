use std::path::Path;

use vision::img::{self, Image, Interp};
use vision::ncnn::NcnnModel;
use vision::shape;

const INPUT: usize = 320;
const GRID: usize = 64;
const MEAN: [f32; 3] = [0.485, 0.456, 0.406];
const STD: [f32; 3] = [0.229, 0.224, 0.225];
const SALIENT: f32 = 0.5;
const MIN_TALL: f64 = 0.5;
const MIN_PIECE: f64 = 0.01;

pub struct ForegroundDetector {
    net: NcnnModel,
}

impl ForegroundDetector {
    pub fn load(models: &Path) -> anyhow::Result<Self> {
        let net = NcnnModel::load_bf16(
            &models.join("u2netp.ncnn.param"),
            &models.join("u2netp.ncnn.bin"),
        )?;
        Ok(Self { net })
    }

    pub fn measure(&mut self, bgr: &Image) -> anyhow::Result<f64> {
        let small = img::resize(bgr, INPUT, INPUT, Interp::Area);
        let max = small.data.iter().copied().max().unwrap_or(0).max(1) as f32;
        let wh = INPUT * INPUT;
        let mut blob = vec![0f32; 3 * wh];
        for (i, p) in small.data.as_chunks::<3>().0.iter().enumerate() {
            for c in 0..3 {
                blob[c * wh + i] = (p[2 - c] as f32 / max - MEAN[c]) / STD[c];
            }
        }
        drop(small);
        let map = self.net.run_output(blob, [3, INPUT, INPUT], "out2")?;
        anyhow::ensure!(map.data.len() == wh, "unexpected saliency map size");
        Ok(lower_object_share(&map.data, INPUT))
    }
}

fn lower_object_share(map: &[f32], side: usize) -> f64 {
    let cell = side / GRID;
    let mask: Vec<u8> = (0..GRID * GRID)
        .map(|i| {
            let (gy, gx) = (i / GRID, i % GRID);
            let mut sum = 0f32;
            for y in gy * cell..(gy + 1) * cell {
                for x in gx * cell..(gx + 1) * cell {
                    sum += map[y * side + x];
                }
            }
            (sum / (cell * cell) as f32 > SALIENT) as u8
        })
        .collect();
    let (labels, areas) = shape::connected_components(&mask, GRID, GRID, false);
    let mut bounds = vec![(GRID, 0, GRID, 0); areas.len()];
    for (i, &label) in labels.iter().enumerate() {
        if label > 0 {
            let b = &mut bounds[label as usize];
            let (y, x) = (i / GRID, i % GRID);
            *b = (b.0.min(y), b.1.max(y), b.2.min(x), b.3.max(x));
        }
    }
    let total = (GRID * GRID) as f64;
    (1..areas.len())
        .filter(|&label| {
            let (y0, y1, x0, x1) = bounds[label];
            let tall = (y1 - y0 + 1) as f64 / (x1 - x0 + 1) as f64;
            y0 * 2 >= GRID && tall >= MIN_TALL && areas[label] as f64 / total >= MIN_PIECE
        })
        .map(|label| areas[label] as f64 / total)
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map_with(rects: &[(usize, usize, usize, usize)]) -> Vec<f32> {
        let mut map = vec![0f32; INPUT * INPUT];
        for &(x, y, w, h) in rects {
            for yy in y..y + h {
                for xx in x..x + w {
                    map[yy * INPUT + xx] = 1.0;
                }
            }
        }
        map
    }

    /// 下半分にまとまった目立つ塊があれば、その広さを返すこと
    #[test]
    fn test_compact_object_in_lower_half_counts() {
        let map = map_with(&[(20, 200, 60, 80)]);

        let share = lower_object_share(&map, INPUT);

        assert!((share - 12.0 * 16.0 / 4096.0).abs() < 1e-9, "share={share}");
    }

    /// 上の端が上半分にかかる塊は、遠くの景色の一部とみなして数えないこと
    #[test]
    fn test_object_reaching_upper_half_is_ignored() {
        let map = map_with(&[(20, 140, 60, 140)]);

        assert_eq!(lower_object_share(&map, INPUT), 0.0);
    }

    /// 下半分でも、横に長く平たい帯（水平線や波打ち際）は数えないこと
    #[test]
    fn test_flat_band_is_ignored() {
        let map = map_with(&[(0, 220, 320, 40)]);

        assert_eq!(lower_object_share(&map, INPUT), 0.0);
    }

    /// 下半分にまとまった小さな塊がいくつもあるときは、広さを合計すること
    #[test]
    fn test_several_small_objects_add_up() {
        let map = map_with(&[(20, 200, 40, 50), (200, 200, 40, 50)]);

        let share = lower_object_share(&map, INPUT);

        assert!(
            (share - 2.0 * 8.0 * 10.0 / 4096.0).abs() < 1e-9,
            "share={share}"
        );
    }

    /// 広さ1%に満たない点のような塊は数えないこと
    #[test]
    fn test_specks_are_ignored() {
        let map = map_with(&[(100, 250, 15, 20)]);

        assert_eq!(lower_object_share(&map, INPUT), 0.0);
    }
}
