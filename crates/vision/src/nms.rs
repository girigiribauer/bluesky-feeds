pub fn nms(
    boxes: &[[f64; 4]],
    scores: &[f32],
    score_threshold: f32,
    iou_threshold: f32,
    top_k: usize,
    epsilon: f64,
) -> Vec<usize> {
    let mut order: Vec<(f32, usize)> = scores
        .iter()
        .enumerate()
        .filter(|(_, &s)| s > score_threshold)
        .map(|(i, &s)| (s, i))
        .collect();
    order.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    if top_k > 0 && top_k < order.len() {
        order.truncate(top_k);
    }
    let mut kept: Vec<usize> = Vec::new();
    for (_, i) in order {
        if kept
            .iter()
            .all(|&k| overlap(&boxes[i], &boxes[k], epsilon) <= iou_threshold)
        {
            kept.push(i);
        }
    }
    kept
}

fn overlap(a: &[f64; 4], b: &[f64; 4], epsilon: f64) -> f32 {
    let area = |r: &[f64; 4]| r[2] * r[3];
    let (aa, ab) = (area(a), area(b));
    if aa + ab <= epsilon {
        return 1.0;
    }
    let inter = area(&intersect(a, b));
    (1.0 - (1.0 - inter / (aa + ab - inter))) as f32
}

fn intersect(a: &[f64; 4], b: &[f64; 4]) -> [f64; 4] {
    let empty = |r: &[f64; 4]| r[2] <= 0.0 || r[3] <= 0.0;
    if empty(a) || empty(b) {
        return [0.0; 4];
    }
    let (x_min, x_max) = if a[0] < b[0] { (a, b) } else { (b, a) };
    let (y_min, y_max) = if a[1] < b[1] { (a, b) } else { (b, a) };
    if (x_min[0] < 0.0 && x_min[0] + x_min[2] < x_max[0])
        || (y_min[1] < 0.0 && y_min[1] + y_min[3] < y_max[1])
    {
        return [0.0; 4];
    }
    let r = [
        x_max[0],
        y_max[1],
        (x_min[2] - (x_max[0] - x_min[0])).min(x_max[2]),
        (y_min[3] - (y_max[1] - y_min[1])).min(y_max[3]),
    ];
    if empty(&r) {
        [0.0; 4]
    } else {
        r
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 重なりが基準を超える候補は、点数の高い方だけを残すこと
    #[test]
    fn test_nms_keeps_higher_score_of_overlapping_boxes() {
        let boxes = [
            [0.0, 0.0, 10.0, 10.0],
            [1.0, 1.0, 10.0, 10.0],
            [50.0, 50.0, 10.0, 10.0],
        ];

        assert_eq!(nms(&boxes, &[0.5, 0.9, 0.7], 0.3, 0.3, 0, 0.0), vec![1, 2]);
    }

    /// 重なりがちょうど基準と同じなら、両方とも残すこと（OpenCV と同じく「基準以下なら残す」）
    #[test]
    fn test_nms_keeps_box_when_overlap_equals_threshold() {
        let boxes = [[0.0, 0.0, 10.0, 10.0], [5.0, 0.0, 10.0, 10.0]];
        let iou = 50.0 / 150.0;

        assert_eq!(nms(&boxes, &[0.9, 0.8], 0.0, iou as f32, 0, 0.0).len(), 2);
        assert_eq!(
            nms(&boxes, &[0.9, 0.8], 0.0, iou as f32 - 1e-4, 0, 0.0),
            vec![0]
        );
    }

    /// 点数が基準ちょうどの候補は捨てること（OpenCV と同じく「基準より大きいものだけ」）
    #[test]
    fn test_nms_drops_score_equal_to_threshold() {
        let boxes = [[0.0, 0.0, 10.0, 10.0]];

        assert!(nms(&boxes, &[0.5], 0.5, 0.3, 0, 0.0).is_empty());
    }

    /// 同じ点数の候補は、元の順番で先にあるものを優先すること（並べ替えで順番が入れ替わらないこと）
    #[test]
    fn test_nms_prefers_earlier_box_on_tie() {
        let boxes = [[0.0, 0.0, 10.0, 10.0], [1.0, 1.0, 10.0, 10.0]];

        assert_eq!(nms(&boxes, &[0.8, 0.8], 0.0, 0.3, 0, 0.0), vec![0]);
    }

    /// 上位いくつまで見るかの指定があれば、点数の高い順にその数だけを候補にすること
    #[test]
    fn test_nms_limits_candidates_to_top_k() {
        let boxes = [
            [0.0, 0.0, 1.0, 1.0],
            [10.0, 0.0, 1.0, 1.0],
            [20.0, 0.0, 1.0, 1.0],
        ];

        assert_eq!(nms(&boxes, &[0.5, 0.9, 0.7], 0.0, 0.3, 2, 0.0), vec![1, 2]);
    }

    /// 離れた四角どうしは重なり0、片方が空なら重なり0、両方とも面積0なら重なり1とみなすこと
    #[test]
    fn test_overlap_edge_cases() {
        assert_eq!(
            overlap(&[0.0, 0.0, 1.0, 1.0], &[5.0, 5.0, 1.0, 1.0], 0.0),
            0.0
        );
        assert_eq!(
            overlap(&[0.0, 0.0, 1.0, 1.0], &[0.0, 0.0, 0.0, 1.0], 0.0),
            0.0
        );
        assert_eq!(
            overlap(&[0.0, 0.0, 0.0, 0.0], &[0.0, 0.0, 0.0, 0.0], 0.0),
            1.0
        );
    }
}
