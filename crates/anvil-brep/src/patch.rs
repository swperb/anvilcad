//! Rational B-spline surface patches, for freeform faces (blends, styled shapes, and
//! anything else other systems describe this way).
//!
//! A patch maps its two parameters (s, t) to space. The kernel's parameters for it are
//! those scaled to millimetres by the patch's average stretch, so lengths in parameter
//! space are roughly true and triangulation sees real proportions. Points are mapped
//! back to parameters by a closest-point search: the nearest of a grid of samples,
//! refined by Gauss-Newton steps.

use crate::geom::Frame;
use crate::spline::{MAX_DEGREE, basis, basis_free};
use crate::{KernelError, Result, tol};
use glam::{DVec2, DVec3};
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SplineSurface {
    pub degree: (usize, usize),
    pub knots_s: Vec<f64>,
    pub knots_t: Vec<f64>,
    /// Number of control points along s and t.
    pub count: (usize, usize),
    /// Control net, `points[i * count.1 + j]` for s index i and t index j.
    pub points: Vec<DVec3>,
    /// Empty when not rational, otherwise one per control point.
    #[serde(default)]
    pub weights: Vec<f64>,
    /// Millimetres per unit of s and of t, on average over the patch.
    pub scale: (f64, f64),
    /// Which parameter, if any, wraps: the patch closes on itself along it (an extruded
    /// or revolved closed profile, say). The kernel's `u` is laid along it.
    #[serde(default)]
    pub wraps: Option<Wrap>,
    /// Whether the patch closes on itself along s and along t. Both can (a torus); the
    /// kernel's `u` follows only `wraps`, but closest-point searches wrap round either.
    #[serde(default)]
    pub closed: (bool, bool),
    /// Reference frame (moves with the patch); the patch has no axis of its own.
    pub frame: Frame,
    #[serde(skip)]
    grid: OnceLock<Grid>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Wrap {
    S,
    T,
}

/// Samples over the patch, for seeding closest-point searches and ray hits.
#[derive(Debug, Clone)]
struct Grid {
    n: (usize, usize),
    params: Vec<DVec2>,
    points: Vec<DVec3>,
}

impl PartialEq for SplineSurface {
    fn eq(&self, o: &Self) -> bool {
        self.degree == o.degree
            && self.knots_s == o.knots_s
            && self.knots_t == o.knots_t
            && self.count == o.count
            && self.points == o.points
            && self.weights == o.weights
            && self.frame == o.frame
    }
}

impl SplineSurface {
    pub fn new(
        degree: (usize, usize),
        knots_s: Vec<f64>,
        knots_t: Vec<f64>,
        count: (usize, usize),
        points: Vec<DVec3>,
        weights: Vec<f64>,
    ) -> Result<SplineSurface> {
        let bad = |m: &str| Err(KernelError::Unsupported(format!("spline surface: {m}")));
        let (p, q) = degree;
        let (n, m) = count;
        if p == 0 || q == 0 || p > MAX_DEGREE || q > MAX_DEGREE {
            return bad("degree out of range");
        }
        if n < p + 1 || m < q + 1 || points.len() != n * m {
            return bad("control net does not match its degrees");
        }
        if knots_s.len() != n + p + 1 || knots_t.len() != m + q + 1 {
            return bad("knot and control point counts disagree");
        }
        for k in [&knots_s, &knots_t] {
            if k.iter().any(|x| !x.is_finite()) || k.windows(2).any(|w| w[1] < w[0]) {
                return bad("knots are not increasing");
            }
        }
        if points.iter().any(|p| !tol::is_finite3(*p)) {
            return Err(KernelError::NonFinite);
        }
        if !weights.is_empty()
            && (weights.len() != points.len() || weights.iter().any(|w| w.is_nan() || *w <= 0.0))
        {
            return bad("weights must be positive, one per control point");
        }
        let mut s = SplineSurface {
            degree,
            knots_s,
            knots_t,
            count,
            points,
            weights,
            scale: (1.0, 1.0),
            wraps: None,
            closed: (false, false),
            frame: Frame::world(),
            grid: OnceLock::new(),
        };
        let ((s0, s1), (t0, t1)) = s.domain();
        if s1 - s0 <= tol::DEGENERATE || t1 - t0 <= tol::DEGENERATE {
            return bad("empty parameter range");
        }
        // Average stretch, from differences over a few rows and columns.
        let k = 8;
        let (mut ls, mut lt) = (0.0, 0.0);
        for a in 0..=k {
            let f = a as f64 / k as f64;
            let row = (0..=k)
                .map(|b| s.eval_st(DVec2::new(s0 + (s1 - s0) * b as f64 / k as f64, t0 + (t1 - t0) * f)));
            let col = (0..=k)
                .map(|b| s.eval_st(DVec2::new(s0 + (s1 - s0) * f, t0 + (t1 - t0) * b as f64 / k as f64)));
            ls += polyline_length(row);
            lt += polyline_length(col);
        }
        let n = (k + 1) as f64;
        s.scale = (((ls / n) / (s1 - s0)).max(1e-9), ((lt / n) / (t1 - t0)).max(1e-9));
        // Closed along a parameter when its two edges coincide all the way along.
        let size = s.points.iter().map(|p| p.length()).fold(1.0, f64::max);
        let meets = |a: &dyn Fn(f64) -> DVec2, b: &dyn Fn(f64) -> DVec2| {
            (0..=16).all(|k| {
                let f = k as f64 / 16.0;
                (s.eval_st(a(f)) - s.eval_st(b(f))).length() < 1e-9 * size
            })
        };
        let along_t = |f: f64| t0 + (t1 - t0) * f;
        let along_s = |f: f64| s0 + (s1 - s0) * f;
        s.closed = (
            meets(&|f| DVec2::new(s0, along_t(f)), &|f| DVec2::new(s1, along_t(f))),
            meets(&|f| DVec2::new(along_s(f), t0), &|f| DVec2::new(along_s(f), t1)),
        );
        s.wraps = match s.closed {
            (true, _) => Some(Wrap::S),
            (false, true) => Some(Wrap::T),
            _ => None,
        };
        Ok(s)
    }

    pub fn domain(&self) -> ((f64, f64), (f64, f64)) {
        let (p, q) = self.degree;
        let (n, m) = self.count;
        ((self.knots_s[p], self.knots_s[n]), (self.knots_t[q], self.knots_t[m]))
    }

    /// The point at patch parameters (s, t), clamped to the domain.
    pub fn eval_st(&self, st: DVec2) -> DVec3 {
        let (p, q) = self.degree;
        let (n, m) = self.count;
        // Just past an edge the patch carries on smoothly (an imported face's boundary
        // can lie a little beyond its patch).
        let (span_s, bs) = basis_free(p, &self.knots_s, n, st.x);
        let (span_t, bt) = basis_free(q, &self.knots_t, m, st.y);
        let mut acc = DVec3::ZERO;
        let mut w = 0.0;
        for (a, ba) in bs.iter().take(p + 1).enumerate() {
            let i = span_s - p + a;
            for (b, bb) in bt.iter().take(q + 1).enumerate() {
                let j = span_t - q + b;
                let idx = i * m + j;
                let k = ba * bb * self.weights.get(idx).copied().unwrap_or(1.0);
                acc += self.points[idx] * k;
                w += k;
            }
        }
        if w.abs() < 1e-300 { acc } else { acc / w }
    }

    /// Kernel parameters (millimetre-scaled, from the domain's corner) of (s, t), and back.
    /// Kernel parameters: lengths from the domain's corner, with `u` along the
    /// wrapping parameter when there is one (then `v` runs along minus s).
    pub fn to_uv(&self, st: DVec2) -> DVec2 {
        let ((s0, _), (t0, _)) = self.domain();
        let q = DVec2::new((st.x - s0) * self.scale.0, (st.y - t0) * self.scale.1);
        // Turned a quarter rather than swapped, so the layout keeps its handedness
        // (counter-clockwise stays about the natural normal).
        if self.wraps == Some(Wrap::T) { DVec2::new(q.y, -q.x) } else { q }
    }

    /// Patch parameters of kernel parameters; the wrapping one comes back into its
    /// domain from any number of turns away.
    pub fn to_st(&self, uv: DVec2) -> DVec2 {
        let ((s0, s1), (t0, t1)) = self.domain();
        let q = if self.wraps == Some(Wrap::T) { DVec2::new(-uv.y, uv.x) } else { uv };
        let mut st = DVec2::new(s0 + q.x / self.scale.0, t0 + q.y / self.scale.1);
        match self.wraps {
            Some(Wrap::S) => st.x = s0 + (st.x - s0).rem_euclid(s1 - s0),
            Some(Wrap::T) => st.y = t0 + (st.y - t0).rem_euclid(t1 - t0),
            None => {}
        }
        st
    }

    /// How far `u` goes before the patch repeats, if it wraps.
    pub fn period(&self) -> Option<f64> {
        let ((s0, s1), (t0, t1)) = self.domain();
        match self.wraps? {
            Wrap::S => Some((s1 - s0) * self.scale.0),
            Wrap::T => Some((t1 - t0) * self.scale.1),
        }
    }

    /// The curve across the wrapping direction at kernel parameter `u`: the patch with
    /// its wrapping parameter held fixed (exactly, as a spline in the other parameter).
    pub fn iso(&self, u: f64) -> Option<crate::spline::Spline> {
        let wrap = self.wraps?;
        let st = self.to_st(DVec2::new(u, 0.0));
        self.iso_line(wrap, if wrap == Wrap::S { st.x } else { st.y })
    }

    /// The patch with parameter `fixed` held at `value`, exactly, as a spline in the
    /// other parameter (whose values it keeps).
    pub fn iso_line(&self, fixed: Wrap, value: f64) -> Option<crate::spline::Spline> {
        let (p, q) = self.degree;
        let (n, m) = self.count;
        let w = |i: usize, j: usize| self.weights.get(i * m + j).copied().unwrap_or(1.0);
        let rational = !self.weights.is_empty();
        let (deg, knots, pts): (usize, Vec<f64>, Vec<(DVec3, f64)>) = match fixed {
            Wrap::S => {
                let (span, b) = basis(p, &self.knots_s, n, value);
                let pts = (0..m)
                    .map(|j| {
                        (0..=p).fold((DVec3::ZERO, 0.0), |(acc, wa), a| {
                            let i = span - p + a;
                            let k = b[a] * w(i, j);
                            (acc + self.points[i * m + j] * k, wa + k)
                        })
                    })
                    .collect();
                (q, self.knots_t.clone(), pts)
            }
            Wrap::T => {
                let (span, b) = basis(q, &self.knots_t, m, value);
                let pts = (0..n)
                    .map(|i| {
                        (0..=q).fold((DVec3::ZERO, 0.0), |(acc, wa), a| {
                            let j = span - q + a;
                            let k = b[a] * w(i, j);
                            (acc + self.points[i * m + j] * k, wa + k)
                        })
                    })
                    .collect();
                (p, self.knots_s.clone(), pts)
            }
        };
        let points = pts.iter().map(|(a, wa)| *a / *wa).collect();
        let weights = if rational { pts.iter().map(|(_, wa)| *wa).collect() } else { Vec::new() };
        crate::spline::Spline::new(deg, knots, points, weights).ok()
    }

    /// Partial derivatives at (s, t), by central differences (one-sided at the edges).
    pub fn partials(&self, st: DVec2) -> (DVec3, DVec3) {
        let ((s0, s1), (t0, t1)) = self.domain();
        let (hs, ht) = ((s1 - s0) * 1e-6, (t1 - t0) * 1e-6);
        let d = |a: DVec2, b: DVec2, h: f64| (self.eval_st(b) - self.eval_st(a)) / h;
        // Within the domain, one-sided at its edges; past them, on the extrapolated patch.
        let within = |x: f64, h: f64, lo: f64, hi: f64| {
            if x < lo || x > hi { (x - h, x + h) } else { ((x - h).max(lo), (x + h).min(hi)) }
        };
        let (sa, sb) = within(st.x, hs, s0, s1);
        let (ta, tb) = within(st.y, ht, t0, t1);
        (
            d(DVec2::new(sa, st.y), DVec2::new(sb, st.y), (sb - sa).max(1e-300)),
            d(DVec2::new(st.x, ta), DVec2::new(st.x, tb), (tb - ta).max(1e-300)),
        )
    }

    /// The natural normal (along ds x dt) at (s, t). Where the patch pinches to a point
    /// the derivatives vanish, so the normal comes from a step inside.
    pub fn normal_st(&self, st: DVec2) -> DVec3 {
        let ((s0, s1), (t0, t1)) = self.domain();
        let mid = DVec2::new(0.5 * (s0 + s1), 0.5 * (t0 + t1));
        for f in [0.0, 1e-4, 1e-3, 1e-2] {
            let (a, b) = self.partials(st + (mid - st) * f);
            if let Some(n) = a.cross(b).try_normalize() {
                return n;
            }
        }
        self.frame.z
    }

    fn grid(&self) -> &Grid {
        self.grid.get_or_init(|| {
            let ((s0, s1), (t0, t1)) = self.domain();
            // About eight samples per knot span each way, within reason.
            let spans = |k: &[f64]| k.windows(2).filter(|w| w[1] > w[0]).count().max(1);
            let ns = (8 * spans(&self.knots_s)).clamp(16, 96);
            let nt = (8 * spans(&self.knots_t)).clamp(16, 96);
            let mut params = Vec::with_capacity((ns + 1) * (nt + 1));
            for i in 0..=ns {
                for j in 0..=nt {
                    params.push(DVec2::new(
                        s0 + (s1 - s0) * i as f64 / ns as f64,
                        t0 + (t1 - t0) * j as f64 / nt as f64,
                    ));
                }
            }
            let points = params.iter().map(|st| self.eval_st(*st)).collect();
            Grid { n: (ns, nt), params, points }
        })
    }

    /// The sample grid: its size in cells along s and t, then each node's parameters and
    /// point, node (i, j) at index `i * (nt + 1) + j`.
    pub(crate) fn samples(&self) -> ((usize, usize), &[DVec2], &[DVec3]) {
        let g = self.grid();
        (g.n, &g.params, &g.points)
    }

    /// Parameters (s, t) of the patch point closest to `p`: the nearest few of a coarse
    /// pass over the samples, each searched around in full, then refined.
    pub fn closest(&self, p: DVec3) -> DVec2 {
        let g = self.grid();
        let (ns, nt) = g.n;
        let at = |i: usize, j: usize| i * (nt + 1) + j;
        let d = |k: usize| (g.points[k] - p).length_squared();
        let stride = 4;
        let mut coarse: Vec<(f64, usize, usize)> = Vec::new();
        for i in (0..=ns).step_by(stride) {
            for j in (0..=nt).step_by(stride) {
                coarse.push((d(at(i, j)), i, j));
            }
        }
        coarse.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut best = (f64::INFINITY, 0usize);
        for &(_, ci, cj) in coarse.iter().take(3) {
            for i in ci.saturating_sub(stride)..=(ci + stride).min(ns) {
                for j in cj.saturating_sub(stride)..=(cj + stride).min(nt) {
                    let k = at(i, j);
                    let dk = d(k);
                    if dk < best.0 {
                        best = (dk, k);
                    }
                }
            }
        }
        self.refine(p, g.params[best.1])
    }

    /// Like [`Self::closest`], starting from `seed` (the answer for a point nearby), and
    /// searching afresh only when that does not land on the patch.
    pub fn closest_from(&self, p: DVec3, seed: DVec2) -> DVec2 {
        let st = self.refine(p, seed);
        if (self.eval_st(st) - p).length() <= 1e-7 * (1.0 + p.length()) { st } else { self.closest(p) }
    }

    /// Like [`Self::closest`], but a point beyond an open edge finds its foot on the patch
    /// extrapolated a little past that edge (at most `MARGIN` of the domain). Laying out
    /// an imported face whose boundary strays past its patch keeps that boundary's shape
    /// rather than folding it onto the edge.
    pub fn closest_beyond(&self, p: DVec3) -> DVec2 {
        self.beyond(p, self.closest(p))
    }

    /// From `st`, the closest point to `p` within the domain, carries the search past an
    /// open edge it rests on (see [`Self::closest_beyond`]).
    pub fn beyond(&self, p: DVec3, mut st: DVec2) -> DVec2 {
        const MARGIN: f64 = 0.05;
        let ((s0, s1), (t0, t1)) = self.domain();
        let at_edge = |x: f64, lo: f64, hi: f64, closed: bool| {
            !closed && (x <= lo + 1e-12 * (hi - lo) || x >= hi - 1e-12 * (hi - lo))
        };
        if !at_edge(st.x, s0, s1, self.closed.0) && !at_edge(st.y, t0, t1, self.closed.1) {
            return st;
        }
        let (ms, mt) = (MARGIN * (s1 - s0), MARGIN * (t1 - t0));
        let bound = |x: DVec2| {
            DVec2::new(
                if self.closed.0 { x.x } else { x.x.clamp(s0 - ms, s1 + ms) },
                if self.closed.1 { x.y } else { x.y.clamp(t0 - mt, t1 + mt) },
            )
        };
        let mut err = (self.eval_st(st) - p).length_squared();
        for _ in 0..30 {
            let f = self.eval_st(st) - p;
            let (a, b) = self.partials(st);
            let (aa, ab, bb) = (a.dot(a), a.dot(b), b.dot(b));
            let det = aa * bb - ab * ab;
            if det.abs() < 1e-300 {
                break;
            }
            let (g1, g2) = (a.dot(f), b.dot(f));
            let step = DVec2::new((bb * g1 - ab * g2) / det, (aa * g2 - ab * g1) / det);
            let next = bound(st - step);
            let e = (self.eval_st(next) - p).length_squared();
            if e >= err {
                break;
            }
            let done = (next - st).length() < 1e-15 * (s1 - s0 + t1 - t0);
            st = next;
            err = e;
            if done {
                break;
            }
        }
        st
    }

    /// `st` brought into the domain: wrapped along a wrapping parameter, clamped otherwise.
    pub(crate) fn keep_in(&self, st: DVec2) -> DVec2 {
        let ((s0, s1), (t0, t1)) = self.domain();
        let fit = |x: f64, lo: f64, hi: f64, wraps: bool| {
            if wraps { lo + (x - lo).rem_euclid(hi - lo) } else { x.clamp(lo, hi) }
        };
        DVec2::new(fit(st.x, s0, s1, self.closed.0), fit(st.y, t0, t1, self.closed.1))
    }

    /// Gauss-Newton on the distance from `p`, from `st`, within the domain.
    fn refine(&self, p: DVec3, mut st: DVec2) -> DVec2 {
        let ((s0, s1), (t0, t1)) = self.domain();
        let mut err = (self.eval_st(st) - p).length_squared();
        for _ in 0..40 {
            let f = self.eval_st(st) - p;
            let (a, b) = self.partials(st);
            let (aa, ab, bb) = (a.dot(a), a.dot(b), b.dot(b));
            let det = aa * bb - ab * ab;
            if det.abs() < 1e-300 {
                break;
            }
            let (g1, g2) = (a.dot(f), b.dot(f));
            let mut step = DVec2::new((bb * g1 - ab * g2) / det, (aa * g2 - ab * g1) / det);
            // At an edge of the domain with the step pointing out, hold that parameter
            // and solve for the other alone (a clamped joint step points the wrong way).
            let out_s = !self.closed.0 && ((st.x <= s0 && step.x > 0.0) || (st.x >= s1 && step.x < 0.0));
            let out_t = !self.closed.1 && ((st.y <= t0 && step.y > 0.0) || (st.y >= t1 && step.y < 0.0));
            if out_s || out_t {
                step = DVec2::new(
                    if out_s || aa < 1e-300 { 0.0 } else { g1 / aa },
                    if out_t || bb < 1e-300 { 0.0 } else { g2 / bb },
                );
                if out_s && out_t {
                    break;
                }
            }
            // Halve the step until it helps.
            let mut k = 1.0;
            let mut moved = false;
            while k > 1e-6 {
                let next = self.keep_in(st - step * k);
                let e = (self.eval_st(next) - p).length_squared();
                if e < err {
                    moved = (next - st).length() > 1e-15 * (s1 - s0 + t1 - t0);
                    st = next;
                    err = e;
                    break;
                }
                k *= 0.5;
            }
            if !moved {
                break;
            }
        }
        st
    }

    /// The patch moved by a rigid motion.
    pub fn moved(&self, m: &Frame) -> SplineSurface {
        SplineSurface {
            points: self.points.iter().map(|p| m.apply_point(*p)).collect(),
            frame: self.frame.moved(m),
            grid: OnceLock::new(),
            ..self.clone()
        }
    }

    /// A box holding the whole patch (the control net's).
    pub fn bounds(&self) -> (DVec3, DVec3) {
        self.points
            .iter()
            .fold((DVec3::splat(f64::INFINITY), DVec3::splat(f64::NEG_INFINITY)), |(lo, hi), p| {
                (lo.min(*p), hi.max(*p))
            })
    }

    /// Parameters along `origin + dir * x` (for x in [lo, hi]) where the line meets the
    /// patch: candidates from the sample grid's triangles, refined by Newton steps.
    pub fn line_hits(&self, origin: DVec3, dir: DVec3, lo: f64, hi: f64) -> Vec<f64> {
        let g = self.grid();
        let (ns, nt) = g.n;
        let at = |i: usize, j: usize| i * (nt + 1) + j;
        let mut out: Vec<f64> = Vec::new();
        for i in 0..ns {
            for j in 0..nt {
                let quad = [at(i, j), at(i + 1, j), at(i + 1, j + 1), at(i, j + 1)];
                for tri in [[quad[0], quad[1], quad[2]], [quad[0], quad[2], quad[3]]] {
                    let [a, b, c] = tri.map(|k| g.points[k]);
                    let Some((x, w1, w2)) = ray_triangle(origin, dir, a, b, c) else { continue };
                    // Seed parameters from the triangle, then solve exactly.
                    let st =
                        g.params[tri[0]] * (1.0 - w1 - w2) + g.params[tri[1]] * w1 + g.params[tri[2]] * w2;
                    if let Some(x) = self.refine_hit(origin, dir, x, st)
                        && x >= lo - 1e-9
                        && x <= hi + 1e-9
                        && !out.iter().any(|y| (y - x).abs() < 1e-7)
                    {
                        out.push(x);
                    }
                }
            }
        }
        out
    }

    /// Newton on (x, s, t) for origin + dir x = S(s, t).
    fn refine_hit(&self, o: DVec3, d: DVec3, mut x: f64, mut st: DVec2) -> Option<f64> {
        for _ in 0..30 {
            let f = o + d * x - self.eval_st(st);
            if f.length() < 1e-10 * (1.0 + o.length()) {
                return Some(x);
            }
            let (a, b) = self.partials(st);
            // Solve [d, -a, -b] [dx, ds, dt] = -f.
            let m = glam::DMat3::from_cols(d, -a, -b);
            if m.determinant().abs() < 1e-300 {
                return None;
            }
            let step = m.inverse() * -f;
            x += step.x;
            st = self.keep_in(st + DVec2::new(step.y, step.z));
        }
        let f = o + d * x - self.eval_st(st);
        (f.length() < 1e-7 * (1.0 + o.length())).then_some(x)
    }

    /// The largest curvature found over the sample grid (1 / mm), for choosing
    /// tessellation steps.
    pub fn max_curvature(&self) -> f64 {
        let g = self.grid();
        let (ns, nt) = g.n;
        let at = |i: usize, j: usize| g.points[i * (nt + 1) + j];
        let mut k: f64 = 0.0;
        for i in 1..ns {
            for j in 0..=nt {
                k = k.max(bend(at(i - 1, j), at(i, j), at(i + 1, j)));
            }
        }
        for i in 0..=ns {
            for j in 1..nt {
                k = k.max(bend(at(i, j - 1), at(i, j), at(i, j + 1)));
            }
        }
        k
    }
}

/// Curvature of the circle through three points.
fn bend(a: DVec3, b: DVec3, c: DVec3) -> f64 {
    let (ab, bc, ca) = ((b - a).length(), (c - b).length(), (a - c).length());
    let area2 = (b - a).cross(c - a).length();
    if ab * bc * ca < 1e-300 { 0.0 } else { 2.0 * area2 / (ab * bc * ca) }
}

fn polyline_length(pts: impl Iterator<Item = DVec3>) -> f64 {
    let mut prev: Option<DVec3> = None;
    let mut len = 0.0;
    for p in pts {
        if let Some(q) = prev {
            len += (p - q).length();
        }
        prev = Some(p);
    }
    len
}

/// Where a line meets a triangle: (parameter, barycentric weights of b and c), with a
/// little slack at the edges so hits between triangles are not lost.
fn ray_triangle(o: DVec3, d: DVec3, a: DVec3, b: DVec3, c: DVec3) -> Option<(f64, f64, f64)> {
    let (e1, e2) = (b - a, c - a);
    let p = d.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-300 {
        return None;
    }
    let inv = 1.0 / det;
    let s = o - a;
    let w1 = s.dot(p) * inv;
    let q = s.cross(e1);
    let w2 = d.dot(q) * inv;
    let slack = 0.05;
    if w1 < -slack || w2 < -slack || w1 + w2 > 1.0 + slack {
        return None;
    }
    Some((e2.dot(q) * inv, w1.clamp(0.0, 1.0), w2.clamp(0.0, 1.0)))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A bicubic patch over [0, 30] x [0, 20] whose height is a gentle bump.
    pub(crate) fn bump() -> SplineSurface {
        let (n, m) = (5, 4);
        let mut points = Vec::new();
        for i in 0..n {
            for j in 0..m {
                let (x, y) = (30.0 * i as f64 / (n - 1) as f64, 20.0 * j as f64 / (m - 1) as f64);
                let z = if (1..n - 1).contains(&i) && (1..m - 1).contains(&j) { 6.0 } else { 0.0 };
                points.push(DVec3::new(x, y, z));
            }
        }
        let clamped = |count: usize, deg: usize| {
            let inner = count - deg - 1;
            let mut k = vec![0.0; deg + 1];
            k.extend((1..=inner).map(|i| i as f64 / (inner + 1) as f64));
            k.extend(vec![1.0; deg + 1]);
            k
        };
        SplineSurface::new((3, 3), clamped(n, 3), clamped(m, 3), (n, m), points, vec![]).expect("patch")
    }

    #[test]
    fn closest_point_and_parameters_round_trip() {
        let s = bump();
        for st in [DVec2::new(0.2, 0.3), DVec2::new(0.7, 0.9), DVec2::new(0.0, 0.5), DVec2::new(0.5, 0.5)] {
            let p = s.eval_st(st);
            let back = s.closest(p + s.normal_st(st) * 0.5);
            assert!((s.eval_st(back) - p).length() < 1e-6, "{st:?} -> {back:?}");
            assert!((s.to_st(s.to_uv(st)) - st).length() < 1e-12);
        }
        // Scaled parameters are roughly lengths: the patch is 30 by 20 and gently bent.
        let ((s0, s1), (t0, t1)) = s.domain();
        let (w, h) = ((s1 - s0) * s.scale.0, (t1 - t0) * s.scale.1);
        assert!((30.0..34.0).contains(&w) && (20.0..24.0).contains(&h), "{w} {h}");
    }

    #[test]
    fn lines_hit_the_patch() {
        let s = bump();
        let hits = s.line_hits(DVec3::new(12.0, 9.0, 50.0), DVec3::NEG_Z, 0.0, 100.0);
        assert_eq!(hits.len(), 1);
        let p = DVec3::new(12.0, 9.0, 50.0 - hits[0]);
        assert!((s.eval_st(s.closest(p)) - p).length() < 1e-8);
        assert!(s.line_hits(DVec3::new(50.0, 9.0, 50.0), DVec3::NEG_Z, 0.0, 100.0).is_empty());
    }
}
