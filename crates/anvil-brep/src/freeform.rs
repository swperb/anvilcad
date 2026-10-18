//! Intersection curves of a freeform (B-spline) patch with any other surface.
//!
//! Starting points come from the patch's sample grid: the signed distance to the other
//! surface is evaluated at every node, and wherever it changes sign along a grid edge a
//! bisection finds a point on both surfaces. From each start not already on a traced
//! curve, the curve is followed both ways by marching: a step along the curve's tangent
//! (the cross product of the two normals), then Newton steps in the patch's parameters
//! that bring the point back onto the other surface within the plane across the
//! tangent. Steps shrink where the curve bends and grow where it is straight. A curve
//! ends where it closes on itself or leaves the patch, and at the patch's edge its end is
//! found exactly on that edge. The points are joined into cubic splines with the exact
//! tangents, and pieces are split until the spline stays on both surfaces.
//!
//! Tangential contacts (where the other surface touches the patch without crossing it)
//! have no sign change and are not found.

use crate::geom::{Curve, Surface};
use crate::patch::{SplineSurface, Wrap};
use crate::spline::Spline;
use crate::{KernelError, Result, tol};
use glam::{DVec2, DVec3};
use std::sync::Arc;

/// How far a fitted curve may stray from either surface (mm).
const FIT: f64 = 1e-7;

/// Most points one curve may have.
const MAX_POINTS: usize = 20_000;

/// Sine of the angle between the normals under which the surfaces are taken to touch
/// rather than cross: the curve is then too ill-conditioned to follow (a fillet meeting
/// the face it blends into, say), and the intersection is refused.
const TOUCH: f64 = 1e-3;

/// Signed distance from `p` to a surface, positive on the side of its natural normal.
pub(crate) fn signed(s: &Surface, p: DVec3) -> f64 {
    match s {
        Surface::Plane { frame } => (p - frame.origin).dot(frame.z),
        Surface::Cylinder { frame, radius } => {
            let l = frame.to_local(p);
            DVec2::new(l.x, l.y).length() - radius
        }
        Surface::Cone { .. } => s.cone_offset(p),
        Surface::Torus { .. } => s.torus_offset(p),
        Surface::Sphere { frame, radius } => (p - frame.origin).length() - radius,
        Surface::BSpline(sp) => {
            let st = sp.closest(p);
            (p - sp.eval_st(st)).dot(sp.normal_st(st))
        }
    }
}

/// A torus as an exact rational B-spline patch: the tube's circle (as nine weighted
/// control points) swept round the axis's circle (nine more), weights multiplied.
pub fn torus_patch(frame: &crate::geom::Frame, major: f64, minor: f64) -> Result<SplineSurface> {
    let w = std::f64::consts::FRAC_1_SQRT_2;
    // The unit circle's nine control points and weights.
    let ring = [
        (1.0, 0.0, 1.0),
        (1.0, 1.0, w),
        (0.0, 1.0, 1.0),
        (-1.0, 1.0, w),
        (-1.0, 0.0, 1.0),
        (-1.0, -1.0, w),
        (0.0, -1.0, 1.0),
        (1.0, -1.0, w),
        (1.0, 0.0, 1.0),
    ];
    let knots = vec![0.0, 0.0, 0.0, 0.25, 0.25, 0.5, 0.5, 0.75, 0.75, 1.0, 1.0, 1.0];
    let mut points = Vec::with_capacity(81);
    let mut weights = Vec::with_capacity(81);
    for &(ax, ay, wa) in &ring {
        for &(cx, cy, wt) in &ring {
            let rho = major + minor * cx;
            points.push(frame.origin + (frame.x * ax + frame.y * ay) * rho + frame.z * (minor * cy));
            weights.push(wa * wt);
        }
    }
    SplineSurface::new((2, 2), knots.clone(), knots, (9, 9), points, weights)
}

/// The intersection curves of patch `p` with surface `q`.
pub fn intersect(p: &SplineSurface, q: &Surface) -> Result<Vec<Curve>> {
    let tracer = Tracer::new(p, q);
    let mut traced: Vec<Vec<DVec3>> = Vec::new();
    let mut curves = Vec::new();
    for seed in tracer.seeds() {
        let x = p.eval_st(seed);
        // Past the other patch's edge its signed distance can still change sign.
        if tracer.outside_other(x) {
            continue;
        }
        if traced.iter().any(|l| near_polyline(l, x, tracer.near)) {
            continue;
        }
        let Some((points, closed)) = tracer.trace(seed)? else { continue };
        // A trace that goes nowhere is a point where the surfaces only touch (two rounds
        // tangent to the same face where they meet at a corner), which divides no face.
        let length: f64 = points.windows(2).map(|w| (w[1].1 - w[0].1).length()).sum();
        if points.len() < 2 || length <= tol::DEGENERATE {
            continue;
        }
        curves.push(tracer.fit(&points, closed)?);
        traced.push(points.iter().map(|q| q.1).collect());
    }
    Ok(curves)
}

/// Where `f` changes sign near `u0` in `[a, b]` (`f0` is its value at `u0`): bracketed by
/// widening from `u0`, then bisected.
fn bisect_near(f: &dyn Fn(f64) -> f64, f0: f64, u0: f64, a: f64, b: f64) -> Option<f64> {
    let mut w = (b - a) * 1e-5;
    let mut other = None;
    while w <= 0.1 * (b - a) && other.is_none() {
        other = [(u0 - w).max(a), (u0 + w).min(b)].into_iter().find(|u| f(*u).signum() != f0.signum());
        w *= 2.0;
    }
    let (mut l, mut r) = (u0, other?);
    for _ in 0..80 {
        let m = 0.5 * (l + r);
        if f(m).signum() == f0.signum() {
            l = m;
        } else {
            r = m;
        }
    }
    Some(0.5 * (l + r))
}

fn near_polyline(l: &[DVec3], x: DVec3, tol: f64) -> bool {
    l.windows(2).any(|w| {
        let ab = w[1] - w[0];
        let t = ((x - w[0]).dot(ab) / ab.length_squared().max(1e-300)).clamp(0.0, 1.0);
        (w[0] + ab * t - x).length() < tol
    })
}

/// A point on the curve: patch parameters and position.
type Node = (DVec2, DVec3);

struct Tracer<'a> {
    p: &'a SplineSurface,
    q: &'a Surface,
    /// Largest and smallest marching steps (mm).
    max_step: f64,
    min_step: f64,
    /// Distance under which a start is taken to lie on a traced curve.
    near: f64,
}

impl<'a> Tracer<'a> {
    fn new(p: &'a SplineSurface, q: &'a Surface) -> Tracer<'a> {
        let (lo, hi) = p.bounds();
        let size = (hi - lo).length().max(1e-6);
        let max_step = size / 40.0;
        Tracer { p, q, max_step, min_step: size * 1e-7, near: max_step * 0.5 }
    }

    fn f(&self, st: DVec2) -> f64 {
        signed(self.q, self.p.eval_st(st))
    }

    /// Patch parameters where the other surface crosses the sample grid's edges.
    fn seeds(&self) -> Vec<DVec2> {
        let ((ns, nt), params, _) = self.p.samples();
        let at = |i: usize, j: usize| i * (nt + 1) + j;
        let values: Vec<f64> = params.iter().map(|st| self.f(*st)).collect();
        let mut out = Vec::new();
        let mut edge = |a: usize, b: usize| {
            let (fa, fb) = (values[a], values[b]);
            if fa == 0.0 {
                out.push(params[a]);
            } else if fa.signum() != fb.signum() && fb != 0.0 {
                let (mut lo, mut hi, mut flo) = (params[a], params[b], fa);
                for _ in 0..60 {
                    let m = (lo + hi) * 0.5;
                    let fm = self.f(m);
                    if fm.signum() == flo.signum() {
                        lo = m;
                        flo = fm;
                    } else {
                        hi = m;
                    }
                }
                out.push((lo + hi) * 0.5);
            }
        };
        for i in 0..=ns {
            for j in 0..=nt {
                if i < ns {
                    edge(at(i, j), at(i + 1, j));
                }
                if j < nt {
                    edge(at(i, j), at(i, j + 1));
                }
            }
        }
        out
    }

    /// Unit tangent of the curve at `x` (patch parameters `st`), or `None` where the
    /// surfaces touch.
    fn tangent(&self, st: DVec2, x: DVec3) -> Option<DVec3> {
        self.p.normal_st(st).cross(self.q.normal_at(x)).try_normalize()
    }

    /// The point on both surfaces in the plane through `pred` across `t`, near `st`.
    fn settle(&self, st: DVec2, pred: DVec3, t: DVec3) -> Option<Node> {
        let mut st = self.p.closest_from(pred, st);
        for _ in 0..30 {
            let x = self.p.eval_st(st);
            let (g1, g2) = (signed(self.q, x), (x - pred).dot(t));
            let tol = 1e-12 * (1.0 + x.length());
            if g1.abs() < tol && g2.abs() < tol {
                return Some((st, x));
            }
            let (ps, pt) = self.p.partials(st);
            let nq = self.q.normal_at(x);
            let (a, b, c, d) = (nq.dot(ps), nq.dot(pt), t.dot(ps), t.dot(pt));
            let det = a * d - b * c;
            if det.abs() < 1e-300 {
                return None;
            }
            let step = DVec2::new((d * g1 - b * g2) / det, (a * g2 - c * g1) / det);
            st = self.p.keep_in(st - step);
        }
        let x = self.p.eval_st(st);
        (signed(self.q, x).abs() < 1e-9 * (1.0 + x.length())).then_some((st, x))
    }

    /// Which patch edge `st` lies on, if any: (fixed coordinate, value).
    fn on_edge(&self, st: DVec2) -> Option<(usize, f64)> {
        let ((s0, s1), (t0, t1)) = self.p.domain();
        let eps = |a: f64, b: f64| 1e-12 * (b - a).abs().max(1e-300);
        if !self.p.closed.0 {
            for v in [s0, s1] {
                if (st.x - v).abs() <= eps(s0, s1) {
                    return Some((0, v));
                }
            }
        }
        if !self.p.closed.1 {
            for v in [t0, t1] {
                if (st.y - v).abs() <= eps(t0, t1) {
                    return Some((1, v));
                }
            }
        }
        None
    }

    /// Where the curve crosses the patch edge that `from` is heading out over: the root
    /// of the signed distance along that edge, near `from`.
    fn edge_end(&self, from: DVec2, dir: DVec2) -> Option<Node> {
        let ((s0, s1), (t0, t1)) = self.p.domain();
        // Which edge: the first the parameter direction reaches.
        let mut best: Option<(f64, usize, f64)> = None;
        for (k, lo, hi, closed) in [(0usize, s0, s1, self.p.closed.0), (1, t0, t1, self.p.closed.1)] {
            if closed {
                continue;
            }
            let (x, d) = if k == 0 { (from.x, dir.x) } else { (from.y, dir.y) };
            if d.abs() < 1e-300 {
                continue;
            }
            let target = if d > 0.0 { hi } else { lo };
            let dist = (target - x) / d;
            if dist >= 0.0 && best.is_none_or(|b| dist < b.0) {
                best = Some((dist, k, target));
            }
        }
        let (_, k, v) = best?;
        let (a, b) = if k == 0 { (t0, t1) } else { (s0, s1) };
        let along = |u: f64| if k == 0 { DVec2::new(v, u) } else { DVec2::new(u, v) };
        let u0 = if k == 0 { from.y } else { from.x };
        let fu = |u: f64| self.f(along(u));
        // Bracket a sign change, widening from the nearest point on the edge.
        let mut w = (b - a) * 1e-4;
        let f0 = fu(u0);
        if f0 == 0.0 {
            let st = along(u0);
            return Some((st, self.p.eval_st(st)));
        }
        while w <= (b - a) {
            for u in [(u0 - w).max(a), (u0 + w).min(b)] {
                if fu(u).signum() != f0.signum() {
                    let (mut lo, mut hi, mut flo) = (u0, u, f0);
                    for _ in 0..60 {
                        let m = 0.5 * (lo + hi);
                        let fm = fu(m);
                        if fm.signum() == flo.signum() {
                            lo = m;
                            flo = fm;
                        } else {
                            hi = m;
                        }
                    }
                    let st = along(0.5 * (lo + hi));
                    return Some((st, self.p.eval_st(st)));
                }
            }
            w *= 2.0;
        }
        None
    }

    /// True when the other surface is a patch and `x` lies beyond its edge (its closest
    /// point on that patch is on the edge, not straight below `x`).
    fn outside_other(&self, x: DVec3) -> bool {
        let Surface::BSpline(q) = self.q else { return false };
        let st = q.closest(x);
        let y = q.eval_st(st);
        let off = x - y;
        // On the patch, the offset runs along the normal; past an edge it does not.
        off.length() > 1e-9 && off.cross(q.normal_st(st)).length() > 1e-6 * off.length().max(1e-9)
    }

    /// When the other surface is a patch and `x` has run off its edge, the point where
    /// that edge (an iso-curve of the other patch) crosses this patch.
    fn past_other_edge(&self, x: DVec3) -> Option<Node> {
        let Surface::BSpline(q) = self.q else { return None };
        let st = q.closest(x);
        let ((s0, s1), (t0, t1)) = q.domain();
        let edge =
            |v: f64, a: f64, b: f64| (v - a).abs() <= 1e-12 * (b - a) || (v - b).abs() <= 1e-12 * (b - a);
        let (fixed, value, u0) = if !q.closed.0 && edge(st.x, s0, s1) {
            (Wrap::S, st.x, st.y)
        } else if !q.closed.1 && edge(st.y, t0, t1) {
            (Wrap::T, st.y, st.x)
        } else {
            return None;
        };
        let line = q.iso_line(fixed, value)?;
        let (a, b) = line.domain();
        // Signed distance to this patch along the other's edge.
        let fu = |u: f64| {
            let y = line.eval(u);
            let pst = self.p.closest(y);
            (y - self.p.eval_st(pst)).dot(self.p.normal_st(pst))
        };
        let f0 = fu(u0);
        let mut w = (b - a) * 1e-4;
        while w <= (b - a) {
            for u in [(u0 - w).max(a), (u0 + w).min(b)] {
                if fu(u).signum() != f0.signum() {
                    let (mut lo, mut hi, mut flo) = (u0, u, f0);
                    for _ in 0..60 {
                        let m = 0.5 * (lo + hi);
                        let fm = fu(m);
                        if fm.signum() == flo.signum() {
                            lo = m;
                            flo = fm;
                        } else {
                            hi = m;
                        }
                    }
                    let y = line.eval(0.5 * (lo + hi));
                    return Some((self.p.closest(y), y));
                }
            }
            w *= 2.0;
        }
        None
    }

    /// Follows the curve through `seed` both ways. Returns its points in order and
    /// whether it closes on itself.
    fn trace(&self, seed: DVec2) -> Result<Option<(Vec<Node>, bool)>> {
        let x0 = self.p.eval_st(seed);
        let Some(t) = self.tangent(seed, x0) else { return Ok(None) };
        let Some(start) = self.settle(seed, x0, t) else { return Ok(None) };
        let Some(t0) = self.tangent(start.0, start.1) else { return Ok(None) };
        // A start at a single point where the surfaces touch, on an edge (the corner where
        // two rounds tangent to one face meet): the curves through it are traced from
        // other starts, and the point itself divides nothing.
        if self.touches(start) && self.touch_end(start).is_some_and(|e| (e.1 - start.1).length() < self.near)
        {
            return Ok(None);
        }
        self.check_crossing(start)?;
        let (forward, closed) = self.march(start, t0)?;
        if closed {
            return Ok(Some((forward, true)));
        }
        let (mut back, _) = self.march(start, -t0)?;
        back.reverse();
        back.pop(); // The start, which `forward` also holds.
        back.extend(forward);
        Ok(Some((back, false)))
    }

    /// Whether the surfaces touch rather than cross at `n` (see [`TOUCH`]).
    fn touches(&self, n: Node) -> bool {
        self.p.normal_st(n.0).cross(self.q.normal_at(n.1)).length() < TOUCH
    }

    /// Refuses a point where the surfaces touch rather than cross.
    fn check_crossing(&self, n: Node) -> Result<()> {
        if self.touches(n) {
            return Err(KernelError::Unsupported("the surfaces touch along their intersection".into()));
        }
        Ok(())
    }

    /// The point near `n` on one of this patch's edges where the other surface touches it:
    /// where the other surface's signed distance along the edge has a double root (it
    /// reaches zero without changing sign), or where the edge leaves the other surface,
    /// when that is a patch whose own edge passes there (the contact edges of two rounds
    /// meeting at a corner). Kept only where the distance is zero, the surfaces touch,
    /// and they touch at that point alone.
    fn touch_end(&self, n: Node) -> Option<Node> {
        let ((s0, s1), (t0, t1)) = self.p.domain();
        let mut best: Option<(f64, Node)> = None;
        for (k, lo, hi, closed) in [(0usize, s0, s1, self.p.closed.0), (1, t0, t1, self.p.closed.1)] {
            if closed {
                continue;
            }
            let (a, b) = if k == 0 { (t0, t1) } else { (s0, s1) };
            for v in [lo, hi] {
                let along = |u: f64| if k == 0 { DVec2::new(v, u) } else { DVec2::new(u, v) };
                let u0 = if k == 0 { n.0.y } else { n.0.x };
                // Only an edge that passes close by.
                if (self.p.eval_st(along(u0)) - n.1).length() > self.max_step {
                    continue;
                }
                let h = (b - a) * 1e-7;
                let rate = |u: f64| self.f(along(u + h)) - self.f(along(u - h));
                let leaves = |u: f64| self.outside_other(self.p.eval_st(along(u)));
                let mut candidates = Vec::new();
                for (test, at0) in [
                    (&rate as &dyn Fn(f64) -> f64, rate(u0)),
                    (&|u: f64| if leaves(u) { 1.0 } else { -1.0 }, if leaves(u0) { 1.0 } else { -1.0 }),
                ] {
                    if let Some(u) = bisect_near(test, at0, u0, a, b) {
                        candidates.push(u);
                    }
                }
                for u in candidates {
                    let st = along(u);
                    let x = self.p.eval_st(st);
                    let off = self.f(st).abs();
                    // Touching at this one point, not along the edge: away from it either
                    // way the distance is not zero, or the edge has left the other patch.
                    let scale = 1e-9 * (1.0 + x.length());
                    let d = (b - a) * 1e-3;
                    let isolated = [u - d, u + d]
                        .iter()
                        .filter(|v| **v >= a && **v <= b)
                        .all(|v| self.f(along(*v)).abs() > scale || leaves(*v));
                    if off < scale && isolated && self.touches((st, x)) && best.is_none_or(|b| off < b.0) {
                        best = Some((off, (st, x)));
                    }
                }
            }
        }
        best.map(|b| b.1)
    }

    /// Marches from `start` along `dir`. Returns the points (beginning with `start`) and
    /// whether the curve came back round to it.
    fn march(&self, start: Node, dir: DVec3) -> Result<(Vec<Node>, bool)> {
        let mut out = vec![start];
        let mut cur = start;
        let mut tan = dir;
        let mut h = self.max_step * 0.25;
        let mut travelled = 0.0;
        while out.len() < MAX_POINTS {
            let pred = cur.1 + tan * h;
            let next = self.settle(cur.0, pred, tan).and_then(|n| {
                let t = self.tangent(n.0, n.1)?;
                let t = if t.dot(tan) < 0.0 { -t } else { t };
                Some((n, t))
            });
            let Some((n, t)) = next else {
                if h > self.min_step {
                    h *= 0.5;
                    continue;
                }
                break;
            };
            // Too sharp a turn for this step: shorten it.
            let turn = tan.angle_between(t);
            if turn > 0.15 && h > self.min_step {
                h *= 0.5;
                continue;
            }
            // Leaving the other surface, when it is a patch too: end exactly where its
            // edge crosses this patch.
            if let Some(end) = self.past_other_edge(n.1) {
                out.push(end);
                break;
            }
            // Leaving the patch: end exactly on its edge.
            if self.on_edge(n.0).is_some() {
                let dir_st = n.0 - cur.0;
                if let Some(end) = self.edge_end(cur.0, dir_st) {
                    out.push(end);
                }
                break;
            }
            let step = (n.1 - cur.1).length();
            travelled += step;
            // Back at the start after going round: a closed curve.
            if out.len() > 3 && travelled > 4.0 * h && (n.1 - start.1).length() < 0.75 * h.max(step) {
                return Ok((out, true));
            }
            // Running into a point where the surfaces touch (two rounds tangent to the
            // same face where they meet at a corner): the curve ends there, on an edge.
            if self.touches(n) {
                let Some(end) = self.touch_end(n) else {
                    return Err(KernelError::Unsupported(
                        "the surfaces touch along their intersection".into(),
                    ));
                };
                out.push(end);
                break;
            }
            out.push(n);
            cur = n;
            tan = t;
            if turn < 0.05 {
                h = (h * 1.5).min(self.max_step);
            }
        }
        Ok((out, false))
    }

    fn off(&self, x: DVec3) -> f64 {
        let st = self.p.closest(x);
        (self.p.eval_st(st) - x).length().max(signed(self.q, x).abs())
    }

    /// Cubic spline through the points with the curve's tangents, with pieces split
    /// until it stays on both surfaces.
    fn fit(&self, points: &[Node], closed: bool) -> Result<Curve> {
        let mut nodes: Vec<(Node, DVec3)> = Vec::with_capacity(points.len());
        for n in points {
            let t = self.tangent(n.0, n.1).unwrap_or(DVec3::X);
            nodes.push((*n, t));
        }
        // Orient every tangent along the run of the points.
        for i in 0..nodes.len() {
            let ahead = if i + 1 < nodes.len() {
                nodes[i + 1].0.1 - nodes[i].0.1
            } else if i > 0 {
                nodes[i].0.1 - nodes[i - 1].0.1
            } else {
                nodes[i].1
            };
            if nodes[i].1.dot(ahead) < 0.0 {
                nodes[i].1 = -nodes[i].1;
            }
        }
        let mut out: Vec<(DVec3, DVec3)> = Vec::new();
        let n = nodes.len();
        let pieces = if closed { n } else { n - 1 };
        for k in 0..pieces {
            let (a, b) = (nodes[k], nodes[(k + 1) % n]);
            self.refine(a, b, 0, &mut out);
        }
        if !closed {
            let last = nodes[n - 1];
            out.push((last.0.1, last.1));
        }
        if out.len() > 50_000 {
            return Err(KernelError::Unsupported("intersection curve needs too many points".into()));
        }
        let (pts, tans): (Vec<DVec3>, Vec<DVec3>) = out.into_iter().unzip();
        Ok(Curve::Spline(Arc::new(Spline::hermite(&pts, &tans, closed)?)))
    }

    /// Adds `a` and, where the cubic from `a` to `b` strays, points between.
    fn refine(&self, a: (Node, DVec3), b: (Node, DVec3), depth: u32, out: &mut Vec<(DVec3, DVec3)>) {
        let ((sa, p0), t0) = a;
        let ((_, p1), t1) = b;
        let h = (p1 - p0).length();
        let phi = t0.angle_between(t1);
        let handle = if phi < 1e-6 { h / 3.0 } else { 2.0 / 3.0 * h * (phi / 4.0).tan() / (phi / 2.0).sin() };
        let (c1, c2) = (p0 + t0 * handle, p1 - t1 * handle);
        let bez = |u: f64| {
            let w = 1.0 - u;
            p0 * (w * w * w) + c1 * (3.0 * w * w * u) + c2 * (3.0 * w * u * u) + p1 * (u * u * u)
        };
        let mid = bez(0.5);
        let strays = || [0.25, 0.5, 0.75].iter().any(|u| self.off(bez(*u)) > FIT);
        if depth < 12 && h > 1e-9 && strays() {
            let chord = (p1 - p0).try_normalize().unwrap_or(t0);
            if let Some(m) = self.settle(sa, mid, chord)
                && let Some(tm) = self.tangent(m.0, m.1)
            {
                let tm = if tm.dot(t0 + t1) < 0.0 { -tm } else { tm };
                self.refine(a, (m, tm), depth + 1, out);
                self.refine((m, tm), b, depth + 1, out);
                return;
            }
        }
        if h > 1e-9 || out.is_empty() {
            out.push((p0, t0));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::Frame;

    fn on_both(c: &Curve, p: &SplineSurface, q: &Surface) {
        let Curve::Spline(s) = c else { panic!("not a spline") };
        let (lo, hi) = s.domain();
        for i in 0..=200 {
            let x = s.eval(lo + (hi - lo) * i as f64 / 200.0);
            let on_p = (p.eval_st(p.closest(x)) - x).length();
            let on_q = signed(q, x).abs();
            assert!(on_p < 1e-6 && on_q < 1e-6, "{on_p:e} {on_q:e} at {x:?}");
        }
    }

    #[test]
    fn plane_through_a_bump_is_a_closed_loop() {
        let p = crate::patch::tests::bump();
        let plane = Surface::Plane { frame: Frame { origin: DVec3::new(0.0, 0.0, 3.0), ..Frame::world() } };
        let cs = intersect(&p, &plane).expect("curves");
        assert_eq!(cs.len(), 1);
        assert!(cs[0].is_periodic());
        on_both(&cs[0], &p, &plane);
    }

    #[test]
    fn cylinder_across_the_bump_edge_ends_on_the_patch_edge() {
        // A vertical cylinder straddling the patch's x = 0 edge: an open curve whose ends
        // lie exactly on that edge.
        let p = crate::patch::tests::bump();
        let cyl =
            Surface::Cylinder { frame: Frame::world().with_origin(DVec3::new(0.0, 10.0, 0.0)), radius: 4.0 };
        let cs = intersect(&p, &cyl).expect("curves");
        assert_eq!(cs.len(), 1);
        assert!(!cs[0].is_periodic());
        on_both(&cs[0], &p, &cyl);
        let Curve::Spline(s) = &cs[0] else { unreachable!() };
        let (lo, hi) = s.domain();
        for t in [lo, hi] {
            assert!(s.eval(t).x.abs() < 1e-6, "{:?}", s.eval(t));
        }
    }

    #[test]
    fn torus_patch_is_exact() {
        let frame = Frame::world().with_origin(DVec3::new(1.0, 2.0, 3.0));
        let p = torus_patch(&frame, 6.0, 1.5).expect("patch");
        let torus = Surface::Torus { frame, major: 6.0, minor: 1.5, v_mid: 0.0 };
        let ((s0, s1), (t0, t1)) = p.domain();
        for i in 0..=20 {
            for j in 0..=20 {
                let x =
                    p.eval_st(DVec2::new(s0 + (s1 - s0) * i as f64 / 20.0, t0 + (t1 - t0) * j as f64 / 20.0));
                assert!(torus.distance(x) < 1e-12, "{x:?}");
            }
        }
    }

    #[test]
    fn torus_cut_off_its_axis() {
        // A fillet-like torus crossed by a cylinder whose axis misses the torus's.
        let torus = Surface::Torus { frame: Frame::world(), major: 6.0, minor: 1.5, v_mid: 0.0 };
        let pin = Surface::Cylinder {
            frame: Frame::new(DVec3::new(6.0, 0.0, 0.0), DVec3::X, DVec3::Y).expect("frame"),
            radius: 0.8,
        };
        let r = crate::intersect::surfaces(&torus, &pin, 1e-6);
        let Ok(crate::intersect::Ssi::Curves(cs)) = r else { panic!("expected curves, got {r:?}") };
        assert!(!cs.is_empty());
        for c in &cs {
            let Curve::Spline(s) = c else { panic!("not a spline") };
            let (lo, hi) = s.domain();
            for i in 0..=100 {
                let x = s.eval(lo + (hi - lo) * i as f64 / 100.0);
                assert!(torus.distance(x) < 1e-6 && pin.distance(x) < 1e-6, "{x:?}");
            }
        }
    }

    #[test]
    fn two_patches_cross() {
        let p = crate::patch::tests::bump();
        let m = Frame { origin: DVec3::new(15.0, -5.0, 3.0), x: DVec3::Y, y: DVec3::NEG_X, z: DVec3::Z };
        let q = Surface::BSpline(Arc::new(p.moved(&m)));
        let cs = intersect(&p, &q).expect("curves");
        assert!(!cs.is_empty());
        let Surface::BSpline(qs) = &q else { unreachable!() };
        for c in &cs {
            on_both(c, &p, &q);
            // Each end lies on an edge of one patch or the other.
            let Curve::Spline(s) = c else { unreachable!() };
            let (lo, hi) = s.domain();
            for t in [lo, hi] {
                let x = s.eval(t);
                let on_edge = |sp: &SplineSurface| {
                    let st = sp.closest(x);
                    let ((s0, s1), (t0, t1)) = sp.domain();
                    let e = |v: f64, a: f64, b: f64| (v - a).abs() < 1e-9 || (v - b).abs() < 1e-9;
                    e(st.x, s0, s1) || e(st.y, t0, t1)
                };
                assert!(on_edge(&p) || on_edge(qs), "end {x:?} is on neither patch's edge");
            }
        }
    }
}
