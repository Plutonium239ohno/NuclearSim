//! Barnes–Hut quadtree for long-range forces (mutual gravity + Coulomb).
//!
//! O(N log N) instead of O(N^2). Each node stores three monopoles:
//! mass, positive charge and negative charge, each with its own centre.
//! Keeping the charge signs separate matters: a neutral cluster of +/- charges
//! would otherwise look like "nothing" from afar *and* have an undefined centre.

#[derive(Clone, Copy, Default)]
struct Node {
    cx: f32,
    cy: f32,
    half: f32,
    m: f32,
    mx: f32,
    my: f32,
    qp: f32,
    qpx: f32,
    qpy: f32,
    qn: f32, // <= 0
    qnx: f32,
    qny: f32,
    /// Index of the first of 4 consecutive children; 0 = leaf.
    child: u32,
    start: u32,
    end: u32,
}

const LEAF_SIZE: usize = 12;
const MAX_DEPTH: u32 = 24;

#[derive(Default)]
pub struct Tree {
    nodes: Vec<Node>,
    idx: Vec<u32>,
    // Sources in tree order, so every node covers a contiguous range.
    x: Vec<f32>,
    y: Vec<f32>,
    m: Vec<f32>,
    q: Vec<f32>,
}

impl Tree {
    pub fn build(&mut self, sx: &[f32], sy: &[f32], sm: &[f32], sq: &[f32]) {
        let n = sx.len();
        self.nodes.clear();
        self.idx.clear();
        self.idx.extend(0..n as u32);
        if n == 0 {
            return;
        }
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for i in 0..n {
            x0 = x0.min(sx[i]);
            x1 = x1.max(sx[i]);
            y0 = y0.min(sy[i]);
            y1 = y1.max(sy[i]);
        }
        let half = ((x1 - x0).max(y1 - y0) * 0.5).max(1.0) * 1.001;
        self.nodes.push(Node {
            cx: (x0 + x1) * 0.5,
            cy: (y0 + y1) * 0.5,
            half,
            start: 0,
            end: n as u32,
            ..Default::default()
        });
        self.split(0, sx, sy, 0);

        let idx = &self.idx;
        self.x.clear();
        self.y.clear();
        self.m.clear();
        self.q.clear();
        self.x.extend(idx.iter().map(|&i| sx[i as usize]));
        self.y.extend(idx.iter().map(|&i| sy[i as usize]));
        self.m.extend(idx.iter().map(|&i| sm[i as usize]));
        self.q.extend(idx.iter().map(|&i| sq[i as usize]));
        self.aggregate(0);
    }

    fn split(&mut self, ni: usize, sx: &[f32], sy: &[f32], depth: u32) {
        let nd = self.nodes[ni];
        let (s, e) = (nd.start as usize, nd.end as usize);
        if e - s <= LEAF_SIZE || depth >= MAX_DEPTH {
            return;
        }
        // Partition indices into quadrants: [top-left, top-right, bottom-left, bottom-right].
        let slice = &mut self.idx[s..e];
        let mid = partition(slice, |i| sy[i as usize] < nd.cy);
        let (top, bot) = slice.split_at_mut(mid);
        let tl = partition(top, |i| sx[i as usize] < nd.cx);
        let bl = partition(bot, |i| sx[i as usize] < nd.cx);
        let bounds = [s, s + tl, s + mid, s + mid + bl, e];

        let h = nd.half * 0.5;
        let first = self.nodes.len();
        self.nodes[ni].child = first as u32;
        for c in 0..4 {
            let cx = if c % 2 == 0 { nd.cx - h } else { nd.cx + h };
            let cy = if c < 2 { nd.cy - h } else { nd.cy + h };
            self.nodes.push(Node {
                cx,
                cy,
                half: h,
                start: bounds[c] as u32,
                end: bounds[c + 1] as u32,
                ..Default::default()
            });
        }
        for c in 0..4 {
            self.split(first + c, sx, sy, depth + 1);
        }
    }

    fn aggregate(&mut self, ni: usize) {
        let nd = self.nodes[ni];
        let (mut m, mut mx, mut my) = (0.0, 0.0, 0.0);
        let (mut qp, mut qpx, mut qpy) = (0.0, 0.0, 0.0);
        let (mut qn, mut qnx, mut qny) = (0.0, 0.0, 0.0);
        if nd.child == 0 {
            for i in nd.start as usize..nd.end as usize {
                let (x, y, mi, qi) = (self.x[i], self.y[i], self.m[i], self.q[i]);
                m += mi;
                mx += mi * x;
                my += mi * y;
                if qi > 0.0 {
                    qp += qi;
                    qpx += qi * x;
                    qpy += qi * y;
                } else if qi < 0.0 {
                    qn += qi;
                    qnx += qi * x;
                    qny += qi * y;
                }
            }
        } else {
            for c in 0..4 {
                let ci = nd.child as usize + c;
                self.aggregate(ci);
                let ch = &self.nodes[ci];
                m += ch.m;
                mx += ch.m * ch.mx;
                my += ch.m * ch.my;
                qp += ch.qp;
                qpx += ch.qp * ch.qpx;
                qpy += ch.qp * ch.qpy;
                qn += ch.qn;
                qnx += ch.qn * ch.qnx;
                qny += ch.qn * ch.qny;
            }
        }
        let n = &mut self.nodes[ni];
        n.m = m;
        (n.mx, n.my) = if m != 0.0 { (mx / m, my / m) } else { (n.cx, n.cy) };
        n.qp = qp;
        (n.qpx, n.qpy) = if qp != 0.0 { (qpx / qp, qpy / qp) } else { (n.cx, n.cy) };
        n.qn = qn;
        (n.qnx, n.qny) = if qn != 0.0 { (qnx / qn, qny / qn) } else { (n.cx, n.cy) };
    }

    /// Acceleration at (px, py). `g` = gravitational constant (source mass term),
    /// `kq` = k*q_i/m_i of the target. Positive coefficients attract.
    #[inline]
    pub fn accel(&self, px: f32, py: f32, g: f32, kq: f32, eps2: f32, theta: f32) -> (f32, f32) {
        if self.nodes.is_empty() {
            return (0.0, 0.0);
        }
        let theta2 = theta * theta;
        let (mut ax, mut ay) = (0.0f32, 0.0f32);
        let mut term = |x: f32, y: f32, coef: f32| {
            let dx = x - px;
            let dy = y - py;
            let r2 = dx * dx + dy * dy + eps2;
            let s = coef / (r2 * r2.sqrt());
            ax += s * dx;
            ay += s * dy;
        };

        let mut stack = [0u32; 128];
        let mut sp = 1usize;
        while sp > 0 {
            sp -= 1;
            let nd = &self.nodes[stack[sp] as usize];
            if nd.start == nd.end {
                continue;
            }
            // Conservative opening test: node size vs distance to the node's box.
            let bx = ((px - nd.cx).abs() - nd.half).max(0.0);
            let by = ((py - nd.cy).abs() - nd.half).max(0.0);
            let size = 2.0 * nd.half;
            if size * size < theta2 * (bx * bx + by * by) {
                if g != 0.0 && nd.m != 0.0 {
                    term(nd.mx, nd.my, g * nd.m);
                }
                if kq != 0.0 {
                    if nd.qp != 0.0 {
                        term(nd.qpx, nd.qpy, -kq * nd.qp);
                    }
                    if nd.qn != 0.0 {
                        term(nd.qnx, nd.qny, -kq * nd.qn);
                    }
                }
            } else if nd.child == 0 {
                for j in nd.start as usize..nd.end as usize {
                    term(self.x[j], self.y[j], g * self.m[j] - kq * self.q[j]);
                }
            } else {
                for c in 0..4 {
                    stack[sp] = nd.child + c;
                    sp += 1;
                }
            }
        }
        (ax, ay)
    }
}

/// In-place unstable partition; returns the number of elements satisfying `pred`.
fn partition(v: &mut [u32], pred: impl Fn(u32) -> bool) -> usize {
    let mut i = 0;
    let mut j = v.len();
    loop {
        while i < j && pred(v[i]) {
            i += 1;
        }
        while i < j && !pred(v[j - 1]) {
            j -= 1;
        }
        if i >= j {
            return i;
        }
        v.swap(i, j - 1);
        i += 1;
        j -= 1;
    }
}
