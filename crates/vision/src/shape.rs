use crate::img::Image;

pub type Pt = (i32, i32);

const DELTAS: [Pt; 8] = [
    (1, 0),
    (1, -1),
    (0, -1),
    (-1, -1),
    (-1, 0),
    (-1, 1),
    (0, 1),
    (1, 1),
];
const RIGHT: i8 = -128;
const NEW: i8 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    External,
    List,
}

struct Scanner {
    img: Vec<i8>,
    w: usize,
}

impl Scanner {
    fn at(&self, x: i32, y: i32) -> i8 {
        self.img[y as usize * self.w + x as usize]
    }

    fn idx(&self, x: i32, y: i32) -> usize {
        y as usize * self.w + x as usize
    }

    fn fetch(&mut self, start: Pt, is_hole: bool) -> Vec<Pt> {
        let mut pts = Vec::new();
        let i0 = start;
        let s_end0: i32 = if is_hole { 0 } else { 4 };
        let mut s = s_end0;
        let mut i1;
        loop {
            s = (s - 1) & 7;
            i1 = (i0.0 + DELTAS[s as usize].0, i0.1 + DELTAS[s as usize].1);
            if self.at(i1.0, i1.1) != 0 || s == s_end0 {
                break;
            }
        }
        let mut pt = (start.0 - 1, start.1 - 1);
        if s == s_end0 {
            let k = self.idx(i0.0, i0.1);
            self.img[k] = NEW | RIGHT;
            pts.push(pt);
            return pts;
        }
        let mut i3 = i0;
        let mut prev_s = s ^ 4;
        loop {
            let s_end = s;
            s = s.min(15);
            let mut i4 = i3;
            while s < 15 {
                s += 1;
                let d = DELTAS[(s & 7) as usize];
                i4 = (i3.0 + d.0, i3.1 + d.1);
                if self.at(i4.0, i4.1) != 0 {
                    break;
                }
            }
            s &= 7;
            let k = self.idx(i3.0, i3.1);
            if ((s - 1) as u32) < s_end as u32 {
                self.img[k] = NEW | RIGHT;
            } else if self.img[k] == 1 {
                self.img[k] = NEW;
            }
            if s != prev_s {
                pts.push(pt);
            }
            prev_s = s;
            pt = (pt.0 + DELTAS[s as usize].0, pt.1 + DELTAS[s as usize].1);
            if i4 == i0 && i3 == i1 {
                break;
            }
            i3 = i4;
            s = (s + 4) & 7;
        }
        pts
    }
}

pub fn find_contours(bin: &Image, mode: Mode) -> Vec<Vec<Pt>> {
    assert_eq!(bin.ch, 1);
    let (w, h) = (bin.w + 2, bin.h + 2);
    let mut img = vec![0i8; w * h];
    for y in 0..bin.h {
        for x in 0..bin.w {
            img[(y + 1) * w + x + 1] = (bin.data[y * bin.w + x] != 0) as i8;
        }
    }
    let mut sc = Scanner { img, w };
    let mut found = Vec::new();
    let (width, height) = (w as i32 - 1, h as i32 - 1);
    let mut last_pos: Pt = (0, 1);
    for y in 1..height {
        let mut x = 1;
        let mut prev = sc.at(0, y);
        while x < width {
            let mut p = 0i8;
            while x < width {
                p = sc.at(x, y);
                if p != prev {
                    break;
                }
                x += 1;
            }
            if x >= width {
                break;
            }
            let mut is_hole = false;
            let mut accepted = true;
            if !(prev == 0 && p == 1) {
                if p != 0 || prev < 1 {
                    accepted = false;
                } else {
                    if prev & !1 != 0 {
                        last_pos.0 = x - 1;
                    }
                    is_hole = true;
                }
            }
            if accepted && mode == Mode::External && (is_hole || sc.at(last_pos.0, last_pos.1) > 0)
            {
                accepted = false;
            }
            if accepted {
                last_pos.0 = x - is_hole as i32;
                let start = (x - is_hole as i32, y);
                found.push(sc.fetch(start, is_hole));
                prev = sc.at(x, y);
                x += 1;
                continue;
            }
            prev = p;
            if prev & !1 != 0 {
                last_pos.0 = x;
            }
            x += 1;
        }
        last_pos = (0, y + 1);
    }
    found.reverse();
    found
}

pub fn bounding_rect(pts: &[Pt]) -> (i32, i32, i32, i32) {
    let (mut x0, mut y0, mut x1, mut y1) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
    for &(x, y) in pts {
        x0 = x0.min(x);
        y0 = y0.min(y);
        x1 = x1.max(x);
        y1 = y1.max(y);
    }
    (x0, y0, x1 - x0 + 1, y1 - y0 + 1)
}

pub fn connected_components(
    mask: &[u8],
    w: usize,
    h: usize,
    eight: bool,
) -> (Vec<i32>, Vec<usize>) {
    let mut labels = vec![-1i32; w * h];
    let mut comps: Vec<(usize, Vec<usize>)> = Vec::new();
    let mut stack = Vec::new();
    for start in 0..w * h {
        if mask[start] == 0 || labels[start] != -1 {
            continue;
        }
        let id = comps.len() as i32;
        let mut pixels = Vec::new();
        labels[start] = id;
        stack.push(start);
        while let Some(i) = stack.pop() {
            pixels.push(i);
            let (x, y) = ((i % w) as i64, (i / w) as i64);
            for dy in -1i64..=1 {
                for dx in -1i64..=1 {
                    if (dx == 0 && dy == 0) || (!eight && dx != 0 && dy != 0) {
                        continue;
                    }
                    let (nx, ny) = (x + dx, y + dy);
                    if nx < 0 || ny < 0 || nx >= w as i64 || ny >= h as i64 {
                        continue;
                    }
                    let j = ny as usize * w + nx as usize;
                    if mask[j] != 0 && labels[j] == -1 {
                        labels[j] = id;
                        stack.push(j);
                    }
                }
            }
        }
        let key = if eight {
            pixels
                .iter()
                .map(|&i| ((i / w) / 2, (i % w) / 2))
                .min()
                .map(|(by, bx)| by * w + bx)
                .unwrap()
        } else {
            *pixels.iter().min().unwrap()
        };
        comps.push((key, pixels));
    }
    let mut order: Vec<usize> = (0..comps.len()).collect();
    order.sort_by_key(|&c| comps[c].0);
    let mut out = vec![0i32; w * h];
    let mut areas = vec![w * h - comps.iter().map(|c| c.1.len()).sum::<usize>()];
    for (rank, &c) in order.iter().enumerate() {
        for &i in &comps[c].1 {
            out[i] = rank as i32 + 1;
        }
        areas.push(comps[c].1.len());
    }
    (out, areas)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RotRect {
    pub cx: f32,
    pub cy: f32,
    pub w: f32,
    pub h: f32,
    pub angle: f32,
}

trait Coord: Copy + PartialOrd {
    fn f(self) -> f32;
    fn sub(self, o: Self) -> Self;
    fn sign(self) -> i32;
    fn is_zero(self) -> bool;
}

impl Coord for i32 {
    fn f(self) -> f32 {
        self as f32
    }
    fn sub(self, o: Self) -> Self {
        self - o
    }
    fn sign(self) -> i32 {
        self.signum()
    }
    fn is_zero(self) -> bool {
        self == 0
    }
}

impl Coord for f32 {
    fn f(self) -> f32 {
        self
    }
    fn sub(self, o: Self) -> Self {
        self - o
    }
    fn sign(self) -> i32 {
        (self > 0.0) as i32 - (self < 0.0) as i32
    }
    fn is_zero(self) -> bool {
        self == 0.0
    }
}

fn sklansky<T: Coord>(
    p: &[(T, T)],
    start: usize,
    end: usize,
    stack: &mut [usize],
    nsign: i32,
    sign2: i32,
    is_float: bool,
) -> usize {
    let incr: isize = if end > start { 1 } else { -1 };
    let at = |i: isize| p[i as usize];
    let (start, end) = (start as isize, end as isize);
    let (mut pprev, mut pcur) = (start, start + incr);
    let mut pnext = pcur + incr;
    let mut stacksize = 3usize;
    if start == end || (at(start).0 == at(end).0 && at(start).1 == at(end).1) {
        stack[0] = start as usize;
        return 1;
    }
    stack[0] = pprev as usize;
    stack[1] = pcur as usize;
    stack[2] = pnext as usize;
    let end = end + incr;
    while pnext != end {
        let cury = at(pcur).1;
        let nexty = at(pnext).1;
        let by = nexty.sub(cury);
        if by.sign() != nsign {
            let (ax, ay) = (at(pcur).0.sub(at(pprev).0), cury.sub(at(pprev).1));
            let (bx, by2) = (at(pnext).0.sub(at(pcur).0), by);
            let convexity = if is_float {
                let norm = |x: f32, y: f32| {
                    let nv = ((x as f64) * (x as f64) + (y as f64) * (y as f64)).sqrt();
                    let k = if nv != 0.0 { 1.0 / nv } else { 0.0 };
                    ((x as f64 * k) as f32, (y as f64 * k) as f32)
                };
                let (a0, a1) = norm(ax.f(), ay.f());
                let (b0, b1) = norm(bx.f(), by2.f());
                let c = a1 as f64 * b0 as f64 - a0 as f64 * b1 as f64;
                (c > 0.0) as i32 - (c < 0.0) as i32
            } else {
                let c = ay.f() as i64 * bx.f() as i64 - ax.f() as i64 * by2.f() as i64;
                c.signum() as i32
            };
            if convexity == sign2 && !(ax.is_zero() && ay.is_zero()) {
                pprev = pcur;
                pcur = pnext;
                pnext += incr;
                stack[stacksize] = pnext as usize;
                stacksize += 1;
            } else if pprev == start {
                pcur = pnext;
                stack[1] = pcur as usize;
                pnext += incr;
                stack[2] = pnext as usize;
            } else {
                stack[stacksize - 2] = pnext as usize;
                pcur = pprev;
                pprev = stack[stacksize - 4] as isize;
                stacksize -= 1;
            }
        } else {
            pnext += incr;
            stack[stacksize - 1] = pnext as usize;
        }
    }
    stacksize - 1
}

fn convex_hull<T: Coord>(pts: &[(T, T)], is_float: bool) -> Vec<(T, T)> {
    let total = pts.len();
    if total == 0 {
        return Vec::new();
    }
    let mut order: Vec<usize> = (0..total).collect();
    order.sort_by(|&a, &b| {
        let (pa, pb) = (pts[a], pts[b]);
        pa.0.partial_cmp(&pb.0)
            .unwrap()
            .then(pa.1.partial_cmp(&pb.1).unwrap())
            .then(a.cmp(&b))
    });
    let sorted: Vec<(T, T)> = order.iter().map(|&i| pts[i]).collect();
    let (mut miny, mut maxy) = (0, 0);
    for i in 1..total {
        let y = sorted[i].1;
        if sorted[miny].1 > y {
            miny = i;
        }
        if sorted[maxy].1 < y {
            maxy = i;
        }
    }
    let mut hull: Vec<usize> = Vec::new();
    let mut stack = vec![0usize; total + 2];
    if sorted[0].0 == sorted[total - 1].0 && sorted[0].1 == sorted[total - 1].1 {
        hull.push(0);
    } else {
        let mut tl = vec![0usize; total + 2];
        let mut tr = vec![0usize; total + 2];
        let tl_count = sklansky(&sorted, 0, maxy, &mut tl, -1, 1, is_float);
        let tr_count = sklansky(&sorted, total - 1, maxy, &mut tr, -1, -1, is_float);
        let (tl, tl_count, tr, tr_count) = (tr, tr_count, tl, tl_count);
        for &v in &tl[..tl_count.saturating_sub(1)] {
            hull.push(v);
        }
        for i in (1..tr_count).rev() {
            hull.push(tr[i]);
        }
        let stop_idx: isize = if tr_count > 2 {
            tr[1] as isize
        } else if tl_count > 2 {
            tl[tl_count - 2] as isize
        } else {
            -1
        };
        let mut bl = vec![0usize; total + 2];
        let mut br = vec![0usize; total + 2];
        let mut bl_count = sklansky(&sorted, 0, miny, &mut bl, 1, -1, is_float);
        let mut br_count = sklansky(&sorted, total - 1, miny, &mut br, 1, 1, is_float);
        if stop_idx >= 0 {
            let check_idx: isize = if bl_count > 2 {
                bl[1] as isize
            } else if bl_count + br_count > 2 {
                br[2 - bl_count] as isize
            } else {
                -1
            };
            if check_idx == stop_idx
                || (check_idx >= 0
                    && sorted[check_idx as usize].0 == sorted[stop_idx as usize].0
                    && sorted[check_idx as usize].1 == sorted[stop_idx as usize].1)
            {
                bl_count = bl_count.min(2);
                br_count = br_count.min(2);
            }
        }
        for &v in &bl[..bl_count.saturating_sub(1)] {
            hull.push(v);
        }
        for i in (1..br_count).rev() {
            hull.push(br[i]);
        }
    }
    let mut hullbuf: Vec<usize> = hull.iter().map(|&i| order[i]).collect();
    let nout = hullbuf.len();
    if nout >= 3 {
        let (mut min_idx, mut max_idx, mut lt) = (0usize, 0usize, 0usize);
        for i in 1..nout {
            let idx = hullbuf[i];
            lt += (hullbuf[i - 1] < idx) as usize;
            if lt > 1 && lt + 2 <= i {
                break;
            }
            if idx < hullbuf[min_idx] {
                min_idx = i;
            }
            if idx > hullbuf[max_idx] {
                max_idx = i;
            }
        }
        let mmdist = (max_idx as isize - min_idx as isize).unsigned_abs();
        if (mmdist == 1 || mmdist == nout - 1) && (lt <= 1 || lt + 2 >= nout) {
            let ascending = (max_idx + 1) % nout == min_idx;
            let i0 = if ascending { min_idx } else { max_idx };
            let mut j = i0;
            if i0 > 0 {
                let mut i = 0;
                while i < nout {
                    let curr = hullbuf[j];
                    stack[i] = curr;
                    let next_j = if j + 1 < nout { j + 1 } else { 0 };
                    let next = hullbuf[next_j];
                    if i < nout - 1 && (ascending != (curr < next)) {
                        break;
                    }
                    j = next_j;
                    i += 1;
                }
                if i == nout {
                    hullbuf.copy_from_slice(&stack[..nout]);
                }
            }
        }
    }
    hullbuf.iter().map(|&i| pts[i]).collect()
}

fn rotating_calipers(points: &[(f32, f32)]) -> [(f32, f32); 3] {
    let n = points.len();
    let mut minarea = f32::MAX;
    let mut vect = vec![(0f32, 0f32); n];
    let mut inv = vec![0f32; n];
    let (mut left, mut bottom, mut right, mut top) = (0usize, 0usize, 0usize, 0usize);
    let mut pt0 = points[0];
    let (mut left_x, mut right_x, mut top_y, mut bottom_y) = (pt0.0, pt0.0, pt0.1, pt0.1);
    for i in 0..n {
        if pt0.0 < left_x {
            left_x = pt0.0;
            left = i;
        }
        if pt0.0 > right_x {
            right_x = pt0.0;
            right = i;
        }
        if pt0.1 > top_y {
            top_y = pt0.1;
            top = i;
        }
        if pt0.1 < bottom_y {
            bottom_y = pt0.1;
            bottom = i;
        }
        let pt = points[if i + 1 < n { i + 1 } else { 0 }];
        let dx = (pt.0 - pt0.0) as f64;
        let dy = (pt.1 - pt0.1) as f64;
        vect[i] = (dx as f32, dy as f32);
        inv[i] = (1.0 / (dx * dx + dy * dy).sqrt()) as f32;
        pt0 = pt;
    }
    let mut seq = [bottom, right, top, left];
    let mut buf = (0usize, 0f32, 0f32, 0f32, 0f32, 0usize);
    let first_is_right = |v1: (f32, f32), v2: (f32, f32)| (v1.1 * v2.0 + -v1.0 * v2.1) < 0.0;
    for _ in 0..n {
        let rot = [
            vect[seq[0]],
            (vect[seq[1]].1, -vect[seq[1]].0),
            (-vect[seq[2]].0, -vect[seq[2]].1),
            (-vect[seq[3]].1, vect[seq[3]].0),
        ];
        let mut main = 0;
        for i in 1..4 {
            if first_is_right(rot[i], rot[main]) {
                main = i;
            }
        }
        let p = seq[main];
        let (lx, ly) = (vect[p].0 * inv[p], vect[p].1 * inv[p]);
        let (base_a, base_b) = match main {
            0 => (lx, ly),
            1 => (ly, -lx),
            2 => (-lx, -ly),
            _ => (-ly, lx),
        };
        seq[main] = if seq[main] + 1 == n { 0 } else { seq[main] + 1 };
        let dx = points[seq[1]].0 - points[seq[3]].0;
        let dy = points[seq[1]].1 - points[seq[3]].1;
        let width = dx * base_a + dy * base_b;
        let dx = points[seq[2]].0 - points[seq[0]].0;
        let dy = points[seq[2]].1 - points[seq[0]].1;
        let height = -dx * base_b + dy * base_a;
        let area = width * height;
        if area <= minarea {
            minarea = area;
            buf = (seq[3], base_a, width, base_b, height, seq[0]);
        }
    }
    let (a1, b1) = (buf.1, buf.3);
    let (a2, b2) = (-buf.3, buf.1);
    let c1 = a1 * points[buf.0].0 + points[buf.0].1 * b1;
    let c2 = a2 * points[buf.5].0 + points[buf.5].1 * b2;
    let idet = 1.0f32 / (a1 * b2 - a2 * b1);
    let px = (c1 * b2 - c2 * b1) * idet;
    let py = (a1 * c2 - a2 * c1) * idet;
    [(px, py), (a1 * buf.2, b1 * buf.2), (a2 * buf.4, b2 * buf.4)]
}

fn min_area_rect_hull(h: &[(f32, f32)]) -> RotRect {
    let mut angle = -std::f64::consts::PI / 2.0;
    let mut r = RotRect {
        cx: 0.0,
        cy: 0.0,
        w: 0.0,
        h: 0.0,
        angle: 0.0,
    };
    let n = h.len();
    if n > 2 {
        let out = rotating_calipers(h);
        r.cx = out[0].0 + (out[1].0 + out[2].0) * 0.5;
        r.cy = out[0].1 + (out[1].1 + out[2].1) * 0.5;
        r.w = ((out[2].0 as f64) * out[2].0 as f64 + (out[2].1 as f64) * out[2].1 as f64).sqrt()
            as f32;
        r.h = ((out[1].0 as f64) * out[1].0 as f64 + (out[1].1 as f64) * out[1].1 as f64).sqrt()
            as f32;
        if out[1].0 == 0.0 && out[1].1 > 0.0 {
            std::mem::swap(&mut r.w, &mut r.h);
        } else {
            angle = -(out[1].0 as f64).atan2(out[1].1 as f64);
        }
    } else if n == 2 {
        r.cx = (h[0].0 + h[1].0) * 0.5;
        r.cy = (h[0].1 + h[1].1) * 0.5;
        let dx = (h[0].0 - h[1].0) as f64;
        let dy = (h[0].1 - h[1].1) as f64;
        r.w = 0.0;
        r.h = (dx * dx + dy * dy).sqrt() as f32;
        if dx == 0.0 {
            std::mem::swap(&mut r.w, &mut r.h);
        } else if dy < 0.0 {
            angle = dy.atan2(dx);
            std::mem::swap(&mut r.w, &mut r.h);
        } else if dy > 0.0 {
            angle = -dx.atan2(dy);
        }
    } else if n == 1 {
        r.cx = h[0].0;
        r.cy = h[0].1;
    }
    r.angle = (angle * 180.0 / std::f64::consts::PI) as f32;
    r
}

pub fn min_area_rect(pts: &[Pt]) -> RotRect {
    let hull: Vec<(f32, f32)> = convex_hull(pts, false)
        .iter()
        .map(|&(x, y)| (x as f32, y as f32))
        .collect();
    min_area_rect_hull(&hull)
}

pub fn min_area_rect_f(pts: &[(f32, f32)]) -> RotRect {
    min_area_rect_hull(&convex_hull(pts, true))
}

pub fn corners(r: &RotRect) -> [(f32, f32); 4] {
    let ang = r.angle as f64 * std::f64::consts::PI / 180.0;
    let b = ang.cos() as f32 * 0.5;
    let a = ang.sin() as f32 * 0.5;
    let (ah, aw, bh, bw) = (a * r.h, a * r.w, b * r.h, b * r.w);
    [
        (r.cx - ah - bw, r.cy + bh - aw),
        (r.cx + ah - bw, r.cy - bh - aw),
        (r.cx + ah + bw, r.cy - bh + aw),
        (r.cx - ah + bw, r.cy + bh + aw),
    ]
}

pub fn contour_area(pts: &[(f32, f32)]) -> f64 {
    if pts.is_empty() {
        return 0.0;
    }
    let mut a = 0f64;
    let mut prev = pts[pts.len() - 1];
    for &p in pts {
        a += prev.0 as f64 * p.1 as f64 - prev.1 as f64 * p.0 as f64;
        prev = p;
    }
    (a * 0.5).abs()
}

pub fn arc_length_closed(pts: &[(f32, f32)]) -> f64 {
    if pts.len() <= 1 {
        return 0.0;
    }
    let mut perimeter = 0f64;
    let mut prev = pts[pts.len() - 1];
    for &p in pts {
        let (dx, dy) = (p.0 - prev.0, p.1 - prev.1);
        perimeter += (dx * dx + dy * dy).sqrt() as f64;
        prev = p;
    }
    perimeter
}

fn line8(img: &mut Image, p1: Pt, p2: Pt, value: u8) {
    let (mut p1, mut p2) = (p1, p2);
    let (mut dx, mut dy) = (p2.0 - p1.0, p2.1 - p1.1);
    if dx < 0 {
        dx = -dx;
        dy = -dy;
        std::mem::swap(&mut p1, &mut p2);
    }
    let (mut sx, mut sy) = (1, 1);
    if dy < 0 {
        dy = -dy;
        sy = -1;
    }
    let vert = dy > dx;
    if vert {
        std::mem::swap(&mut dx, &mut dy);
        std::mem::swap(&mut sx, &mut sy);
    }
    let mut err = dx - (dy + dy);
    let (plus, minus) = (dx + dx, -(dy + dy));
    let (mut x, mut y) = p1;
    for _ in 0..=dx {
        img.data[y as usize * img.w + x as usize] = value;
        let step = err < 0;
        err += minus + if step { plus } else { 0 };
        if !vert {
            x += sx;
            if step {
                y += sy;
            }
        } else {
            y += sx;
            if step {
                x += sy;
            }
        }
    }
}

const XY_SHIFT: i64 = 16;
const NIL: usize = usize::MAX;

#[derive(Clone, Copy)]
struct Edge {
    y0: i32,
    y1: i32,
    x: i64,
    dx: i64,
    next: usize,
}

pub fn fill_poly(img: &mut Image, pts: &[Pt], value: u8) {
    assert_eq!(img.ch, 1);
    let n = pts.len();
    if n == 0 {
        return;
    }
    let mut edges = Vec::new();
    let mut pt0 = pts[n - 1];
    for &pt1 in pts {
        line8(img, pt0, pt1, value);
        if pt0.1 != pt1.1 {
            let (x0, x1) = ((pt0.0 as i64) << XY_SHIFT, (pt1.0 as i64) << XY_SHIFT);
            let dx = (x1 - x0) / (pt1.1 - pt0.1) as i64;
            edges.push(if pt0.1 < pt1.1 {
                Edge {
                    y0: pt0.1,
                    y1: pt1.1,
                    x: x0,
                    dx,
                    next: NIL,
                }
            } else {
                Edge {
                    y0: pt1.1,
                    y1: pt0.1,
                    x: x1,
                    dx,
                    next: NIL,
                }
            });
        }
        pt0 = pt1;
    }
    let total = edges.len();
    if total < 2 {
        return;
    }
    let (w, h) = (img.w as i32, img.h as i32);
    let (mut y_min, mut y_max) = (i32::MAX, i32::MIN);
    let (mut x_min, mut x_max) = (i64::MAX, -1i64);
    for e in &edges {
        let x1 = e.x + (e.y1 - e.y0) as i64 * e.dx;
        y_min = y_min.min(e.y0);
        y_max = y_max.max(e.y1);
        x_min = x_min.min(e.x).min(x1);
        x_max = x_max.max(e.x).max(x1);
    }
    if y_max < 0 || y_min >= h || x_max < 0 || x_min >= (w as i64) << XY_SHIFT {
        return;
    }
    edges.sort_by(|a, b| a.y0.cmp(&b.y0).then(a.x.cmp(&b.x)).then(a.dx.cmp(&b.dx)));
    edges.push(Edge {
        y0: i32::MAX,
        y1: 0,
        x: 0,
        dx: 0,
        next: NIL,
    });
    let head = edges.len();
    edges.push(Edge {
        y0: i32::MAX,
        y1: 0,
        x: 0,
        dx: 0,
        next: NIL,
    });
    let delta = (1i64 << XY_SHIFT) - 1;
    let mut i = 0usize;
    let mut e = 0usize;
    let y_max = y_max.min(h);
    let mut y = edges[0].y0;
    while y < y_max {
        let mut draw = false;
        let clipline = y < 0;
        let mut prelast = head;
        let mut last = edges[head].next;
        while last != NIL || edges[e].y0 == y {
            if last != NIL && edges[last].y1 == y {
                edges[prelast].next = edges[last].next;
                last = edges[last].next;
                continue;
            }
            let keep = prelast;
            if last != NIL && (edges[e].y0 > y || edges[last].x < edges[e].x) {
                prelast = last;
                last = edges[last].next;
            } else if i < total {
                edges[prelast].next = e;
                edges[e].next = last;
                prelast = e;
                i += 1;
                e = i;
            } else {
                break;
            }
            if draw {
                if !clipline {
                    let (kx, px) = (edges[keep].x, edges[prelast].x);
                    let (mut x1, mut x2) = if kx > px {
                        (((px + delta) >> XY_SHIFT) as i32, (kx >> XY_SHIFT) as i32)
                    } else {
                        (((kx + delta) >> XY_SHIFT) as i32, (px >> XY_SHIFT) as i32)
                    };
                    if x1 < w && x2 >= 0 {
                        x1 = x1.max(0);
                        x2 = x2.min(w - 1);
                        let row = y as usize * img.w;
                        for x in x1..=x2 {
                            img.data[row + x as usize] = value;
                        }
                    }
                }
                edges[keep].x += edges[keep].dx;
                edges[prelast].x += edges[prelast].dx;
            }
            draw = !draw;
        }
        let mut keep = NIL;
        loop {
            let mut prelast = head;
            let mut last = edges[head].next;
            let mut last_exchange = NIL;
            while last != keep && edges[last].next != NIL {
                let te = edges[last].next;
                if edges[last].x > edges[te].x {
                    edges[prelast].next = te;
                    edges[last].next = edges[te].next;
                    edges[te].next = last;
                    prelast = te;
                    last_exchange = prelast;
                } else {
                    prelast = last;
                    last = te;
                }
            }
            if last_exchange == NIL {
                break;
            }
            keep = last_exchange;
            if !(keep != edges[head].next && keep != head) {
                break;
            }
        }
        y += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::Fnv;

    fn blobs(w: usize, h: usize, seed: u64, density: u32) -> Image {
        let mut s = seed;
        let mut rnd = |n: u32| {
            s = s
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (s >> 33) as u32 % n
        };
        let mut img = Image::new(w, h, 1);
        for _ in 0..density {
            let (cx, cy, r) = (
                rnd(w as u32) as i32,
                rnd(h as u32) as i32,
                1 + rnd(9) as i32,
            );
            let hole = rnd(3) == 0;
            for y in (cy - r).max(0)..(cy + r + 1).min(h as i32) {
                for x in (cx - r).max(0)..(cx + r + 1).min(w as i32) {
                    let d = (x - cx) * (x - cx) + (y - cy) * (y - cy);
                    if d <= r * r {
                        img.data[y as usize * w + x as usize] = if hole && d <= (r / 2) * (r / 2) {
                            0
                        } else {
                            255
                        };
                    }
                }
            }
        }
        for _ in 0..w * h / 50 {
            let (x, y) = (rnd(w as u32) as usize, rnd(h as u32) as usize);
            img.data[y * w + x] ^= 255;
        }
        img
    }

    fn polygons(seed: u64) -> Vec<Vec<Pt>> {
        let mut out = Vec::new();
        for (i, &(w, h)) in [(200, 150), (736, 736), (64, 48)].iter().enumerate() {
            for density in [3, 30, 300] {
                out.extend(find_contours(
                    &blobs(w, h, seed + i as u64 * 3 + density as u64, density),
                    Mode::List,
                ));
            }
        }
        let mut s = seed;
        let mut rnd = |n: i32| {
            s = s
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((s >> 33) % n as u64) as i32
        };
        for k in 0..300 {
            let n = 1 + rnd(12) as usize;
            out.push((0..n).map(|_| (rnd(60 + k % 7), rnd(40 + k % 5))).collect());
        }
        out.push(vec![(5, 5), (5, 5), (5, 5)]);
        out.push(vec![(0, 0), (10, 0)]);
        out.push(vec![(0, 0), (0, 10)]);
        out.push(vec![(0, 0), (10, 10), (20, 20)]);
        out
    }

    fn hash_contours(h: &mut Fnv, cs: &[Vec<Pt>]) {
        h.i32s(&[cs.len() as i32]);
        for c in cs {
            h.i32s(&[c.len() as i32]);
            for &(x, y) in c {
                h.i32s(&[x, y]);
            }
        }
    }

    fn hash_rect(h: &mut Fnv, r: &RotRect) {
        h.f32s(&[r.cx, r.cy, r.w, r.h, r.angle]);
    }

    /// 輪郭（外側だけ・全部）の点の並びと順番が、OpenCV と同じことを確かめたときの記録から変わらないこと
    #[test]
    fn test_find_contours_stays_recorded() {
        let mut h = Fnv::new();
        for (i, &(w, ht)) in [
            (64, 48),
            (200, 150),
            (1, 1),
            (3, 2),
            (17, 5),
            (5, 17),
            (736, 736),
        ]
        .iter()
        .enumerate()
        {
            for density in [3, 30, 300] {
                let img = blobs(w, ht, i as u64 * 7 + density as u64, density);
                hash_contours(&mut h, &find_contours(&img, Mode::List));
                hash_contours(&mut h, &find_contours(&img, Mode::External));
            }
        }
        assert_eq!(h.0, 15846093614550182868);
    }

    /// つながった塊の番号と面積（4方向・8方向）が、OpenCV と同じことを確かめたときの記録から変わらないこと
    #[test]
    fn test_connected_components_stay_recorded() {
        let mut h = Fnv::new();
        for (i, &(w, ht)) in [
            (64, 48),
            (200, 150),
            (1, 1),
            (3, 2),
            (17, 5),
            (5, 17),
            (256, 192),
        ]
        .iter()
        .enumerate()
        {
            for density in [3, 30, 300] {
                let img = blobs(w, ht, i as u64 * 5 + density as u64, density);
                let mask: Vec<u8> = img.data.iter().map(|&v| (v != 0) as u8).collect();
                for eight in [false, true] {
                    let (labels, areas) = connected_components(&mask, w, ht, eight);
                    h.i32s(&labels);
                    h.i32s(&areas.iter().map(|&a| a as i32).collect::<Vec<_>>());
                }
            }
        }
        assert_eq!(h.0, 14216115578955044583);
    }

    /// 最小の傾いた四角（整数の点・小数の点）が、OpenCV と同じことを確かめたときの記録から変わらないこと
    #[test]
    fn test_min_area_rect_stays_recorded() {
        let mut h = Fnv::new();
        for p in polygons(11) {
            hash_rect(&mut h, &min_area_rect(&p));
            let pf: Vec<(f32, f32)> = p
                .iter()
                .map(|&(x, y)| (x as f32 * 0.75 + 0.3, y as f32 * 1.25))
                .collect();
            hash_rect(&mut h, &min_area_rect_f(&pf));
        }
        assert_eq!(h.0, 17610365392974966350);
    }

    /// 傾いた四角の4つの角・面積・周の長さが、OpenCV と同じことを確かめたときの記録から変わらないこと
    #[test]
    fn test_corners_area_length_stay_recorded() {
        let mut h = Fnv::new();
        for p in polygons(12) {
            let c = corners(&min_area_rect(&p));
            h.f32s(&c.iter().flat_map(|&(x, y)| [x, y]).collect::<Vec<_>>());
            h.f64s(&[contour_area(&c), arc_length_closed(&c)]);
        }
        assert_eq!(h.0, 159170911739767517);
    }

    /// 多角形を塗った結果が、OpenCV と1画素も違わないことを確かめたときの記録から変わらないこと
    #[test]
    fn test_fill_poly_stays_recorded() {
        let mut h = Fnv::new();
        for p in polygons(13) {
            let w = p.iter().map(|q| q.0).max().unwrap() + 1;
            let ht = p.iter().map(|q| q.1).max().unwrap() + 1;
            let mut img = Image::new(w as usize, ht as usize, 1);
            fill_poly(&mut img, &p, 1);
            h.bytes(&img.data);
        }
        assert_eq!(h.0, 3640298867769970952);
    }

    /// 輪郭を囲む四角が、OpenCV と同じことを確かめたときの記録から変わらないこと
    #[test]
    fn test_bounding_rect_stays_recorded() {
        let mut h = Fnv::new();
        for c in find_contours(&blobs(200, 150, 3, 30), Mode::List) {
            let (x, y, w, ht) = bounding_rect(&c);
            h.i32s(&[x, y, w, ht]);
        }
        assert_eq!(h.0, 1426648812375941262);
    }

    fn square_image(hole: bool) -> Image {
        let mut img = Image::new(20, 20, 1);
        for y in 5..15 {
            for x in 5..15 {
                let inner = (8..12).contains(&x) && (8..12).contains(&y);
                img.data[y * 20 + x] = if hole && inner { 0 } else { 255 };
            }
        }
        img
    }

    /// 塗りつぶした正方形の輪郭は、4つの角の点だけで表され、囲む四角は正方形そのものになること
    #[test]
    fn test_square_contour_is_four_corners() {
        let cs = find_contours(&square_image(false), Mode::External);
        assert_eq!(cs.len(), 1);
        let mut pts = cs[0].clone();
        pts.sort();
        assert_eq!(pts, vec![(5, 5), (5, 14), (14, 5), (14, 14)]);
        assert_eq!(bounding_rect(&cs[0]), (5, 5, 10, 10));
    }

    /// 穴のあいた正方形は、外側だけなら輪郭1つ、全部なら外側と穴の2つになること
    #[test]
    fn test_hole_is_found_only_in_list_mode() {
        let img = square_image(true);
        assert_eq!(find_contours(&img, Mode::External).len(), 1);
        assert_eq!(find_contours(&img, Mode::List).len(), 2);
    }

    /// 何も無い画像では輪郭が見つからないこと
    #[test]
    fn test_empty_image_has_no_contours() {
        assert!(find_contours(&Image::new(10, 10, 1), Mode::List).is_empty());
    }

    /// 斜めにだけ接している2画素は、8方向ならひとつの塊、4方向なら別々の塊になり、面積は背景から順に数えること
    #[test]
    fn test_diagonal_pixels_connect_only_in_eight_way() {
        let mask = vec![1, 0, 0, 1];
        assert_eq!(connected_components(&mask, 2, 2, true).1, vec![2, 2]);
        assert_eq!(connected_components(&mask, 2, 2, false).1, vec![2, 1, 1]);
    }

    /// 軸にそろった長方形の点の最小の傾いた四角は、その長方形の中心・大きさになること
    #[test]
    fn test_min_area_rect_of_axis_aligned_rectangle() {
        let r = min_area_rect(&[(0, 0), (10, 0), (10, 4), (0, 4)]);
        assert_eq!((r.cx, r.cy), (5.0, 2.0));
        let (long, short) = (r.w.max(r.h), r.w.min(r.h));
        assert_eq!((long, short), (10.0, 4.0));
    }

    /// 1辺10の正方形の面積は 100、周の長さは 40 になること
    #[test]
    fn test_square_area_and_length() {
        let sq = [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)];
        assert_eq!(contour_area(&sq), 100.0);
        assert_eq!(arc_length_closed(&sq), 40.0);
    }

    /// 長方形の角の点で塗ると、辺の上の画素も含めて長方形の中がすべて塗られ、外は塗られないこと
    #[test]
    fn test_fill_poly_fills_rectangle_including_edges() {
        let mut img = Image::new(10, 8, 1);
        fill_poly(&mut img, &[(2, 1), (6, 1), (6, 5), (2, 5)], 7);
        for y in 0..8 {
            for x in 0..10 {
                let inside = (2..=6).contains(&x) && (1..=5).contains(&y);
                assert_eq!(
                    img.data[y * 10 + x],
                    if inside { 7 } else { 0 },
                    "({x},{y})"
                );
            }
        }
    }
}
