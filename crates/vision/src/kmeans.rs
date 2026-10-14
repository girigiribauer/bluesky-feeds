use crate::lines::Rng;

fn dist2(a: &[f32], b: &[f32]) -> f32 {
    let mut d = 0f32;
    for j in 0..a.len() {
        let t = a[j] - b[j];
        d += t * t;
    }
    d
}

fn centers_pp(data: &[f32], dims: usize, k: usize, rng: &mut Rng, trials: usize) -> Vec<f32> {
    let n = data.len() / dims;
    let row = |i: usize| &data[i * dims..(i + 1) * dims];
    let mut centers = vec![0usize; k];
    centers[0] = (rng.next() % n as u32) as usize;
    let mut dist: Vec<f32> = (0..n).map(|i| dist2(row(i), row(centers[0]))).collect();
    let mut sum0: f64 = dist.iter().map(|&d| d as f64).sum();
    let mut tdist = vec![0f32; n];
    let mut tdist2 = vec![0f32; n];
    for c in centers.iter_mut().skip(1) {
        let (mut best_sum, mut best_center) = (f64::MAX, None);
        for _ in 0..trials {
            let mut p = rng.next_f64() * sum0;
            let mut ci = 0;
            while ci < n - 1 {
                p -= dist[ci] as f64;
                if p <= 0.0 {
                    break;
                }
                ci += 1;
            }
            for i in 0..n {
                tdist2[i] = dist2(row(i), row(ci)).min(dist[i]);
            }
            let mut s = 0f64;
            let mut i = 0;
            while i + 7 < n {
                s += (tdist2[i]
                    + tdist2[i + 1]
                    + tdist2[i + 2]
                    + tdist2[i + 3]
                    + tdist2[i + 4]
                    + tdist2[i + 5]
                    + tdist2[i + 6]
                    + tdist2[i + 7]) as f64;
                i += 8;
            }
            while i < n {
                s += tdist2[i] as f64;
                i += 1;
            }
            if s < best_sum {
                best_sum = s;
                best_center = Some(ci);
                std::mem::swap(&mut tdist, &mut tdist2);
            }
        }
        *c = best_center.expect("kmeans: can't update cluster center");
        sum0 = best_sum;
        std::mem::swap(&mut dist, &mut tdist);
    }
    centers.iter().flat_map(|&c| row(c).to_vec()).collect()
}

fn sum_f64(v: &[f64]) -> f64 {
    let mut s = 0f64;
    let mut i = 0;
    while i + 4 <= v.len() {
        s += v[i] + v[i + 1] + v[i + 2] + v[i + 3];
        i += 4;
    }
    while i < v.len() {
        s += v[i];
        i += 1;
    }
    s
}

pub fn kmeans_pp(
    data: &[f32],
    dims: usize,
    k: usize,
    max_count: usize,
    epsilon: f64,
    attempts: usize,
    seed: u64,
) -> Vec<i32> {
    let n = data.len() / dims;
    assert!(n >= k && k > 0);
    let row = |i: usize| &data[i * dims..(i + 1) * dims];
    let mut rng = Rng(seed);
    let eps = epsilon.max(0.0).powi(2);
    let max_count = if k == 1 { 2 } else { max_count.clamp(2, 100) };
    let attempts = if k == 1 { 1 } else { attempts.max(1) };
    let mut labels = vec![0i32; n];
    let mut best_labels = vec![0i32; n];
    let mut best_compactness = f64::MAX;
    let mut centers = vec![0f32; k * dims];
    let mut old_centers = vec![0f32; k * dims];
    let mut dists = vec![0f64; n];
    for _ in 0..attempts {
        let compactness;
        let mut iter = 0;
        loop {
            let mut max_shift = if iter == 0 { f64::MAX } else { 0.0 };
            std::mem::swap(&mut centers, &mut old_centers);
            if iter == 0 {
                centers = centers_pp(data, dims, k, &mut rng, 3);
            } else {
                centers.iter_mut().for_each(|v| *v = 0.0);
                let mut counters = vec![0i64; k];
                for (i, &label) in labels.iter().enumerate().take(n) {
                    let c = label as usize;
                    for j in 0..dims {
                        centers[c * dims + j] += row(i)[j];
                    }
                    counters[c] += 1;
                }
                for c in 0..k {
                    if counters[c] != 0 {
                        continue;
                    }
                    let mut max_k = 0;
                    for k1 in 1..k {
                        if counters[max_k] < counters[k1] {
                            max_k = k1;
                        }
                    }
                    let scale = 1.0f32 / counters[max_k] as f32;
                    let base: Vec<f32> = centers[max_k * dims..(max_k + 1) * dims]
                        .iter()
                        .map(|v| v * scale)
                        .collect();
                    let (mut max_dist, mut farthest) = (0f64, None);
                    for (i, &label) in labels.iter().enumerate().take(n) {
                        if label as usize != max_k {
                            continue;
                        }
                        let d = dist2(row(i), &base) as f64;
                        if max_dist <= d {
                            max_dist = d;
                            farthest = Some(i);
                        }
                    }
                    let f = farthest.expect("kmeans: empty cluster");
                    counters[max_k] -= 1;
                    counters[c] += 1;
                    labels[f] = c as i32;
                    for j in 0..dims {
                        centers[max_k * dims + j] -= row(f)[j];
                        centers[c * dims + j] += row(f)[j];
                    }
                }
                for c in 0..k {
                    let scale = 1.0f32 / counters[c] as f32;
                    let mut d = 0f64;
                    for j in 0..dims {
                        centers[c * dims + j] *= scale;
                        if iter > 0 {
                            let t = (centers[c * dims + j] - old_centers[c * dims + j]) as f64;
                            d += t * t;
                        }
                    }
                    if iter > 0 {
                        max_shift = max_shift.max(d);
                    }
                }
            }
            iter += 1;
            if iter == max_count || max_shift <= eps {
                for (i, &label) in labels.iter().enumerate().take(n) {
                    let c = label as usize;
                    dists[i] = dist2(row(i), &centers[c * dims..(c + 1) * dims]) as f64;
                }
                compactness = sum_f64(&dists);
                break;
            }
            for i in 0..n {
                let (mut best, mut min_d) = (0usize, f64::MAX);
                for c in 0..k {
                    let d = dist2(row(i), &centers[c * dims..(c + 1) * dims]) as f64;
                    if min_d > d {
                        min_d = d;
                        best = c;
                    }
                }
                dists[i] = min_d;
                labels[i] = best as i32;
            }
        }
        if compactness < best_compactness {
            best_compactness = compactness;
            best_labels.copy_from_slice(&labels);
        }
    }
    best_labels
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::Fnv;

    fn colors(n: usize, seed: u64) -> Vec<f32> {
        let mut s = seed;
        let mut rnd = |m: u32| {
            s = s
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (s >> 33) as u32 % m
        };
        let centers: Vec<[u32; 3]> = (0..7).map(|_| [rnd(256), rnd(256), rnd(256)]).collect();
        (0..n)
            .flat_map(|_| {
                let c = centers[rnd(7) as usize];
                c.map(|v| (v as i32 + rnd(40) as i32 - 20).clamp(0, 255) as f32)
            })
            .collect()
    }

    /// 色の似た点の分け方が、OpenCV と同じことを確かめたときの記録から変わらないこと（k=6・8・10、20万点を含む）
    #[test]
    fn test_kmeans_stays_recorded() {
        let mut h = Fnv::new();
        for (i, n) in [500usize, 5000, 200_000].into_iter().enumerate() {
            let data = colors(n, i as u64 + 1);
            for k in [6, 8, 10] {
                h.i32s(&kmeans_pp(&data, 3, k, 20, 1.0, 2, 12345));
            }
        }
        assert_eq!(h.0, 10704533839900237482);
    }

    /// はっきり離れた3つの色の集まりを3つに分けると、同じ集まりの点は同じ番号に、違う集まりの点は違う番号になること
    #[test]
    fn test_separated_clusters_get_one_label_each() {
        let centers = [
            [20.0f32, 20.0, 20.0],
            [128.0, 200.0, 60.0],
            [240.0, 30.0, 220.0],
        ];
        let mut data = Vec::new();
        for i in 0..300 {
            let c = centers[i % 3];
            let d = (i % 7) as f32 - 3.0;
            data.extend([c[0] + d, c[1] - d, c[2] + d]);
        }
        let labels = kmeans_pp(&data, 3, 3, 20, 1.0, 2, 12345);
        assert!(labels.iter().all(|&l| (0..3).contains(&l)));
        for i in 0..300 {
            assert_eq!(labels[i], labels[i % 3], "{i}");
        }
        assert!(labels[0] != labels[1] && labels[1] != labels[2] && labels[0] != labels[2]);
    }
}
