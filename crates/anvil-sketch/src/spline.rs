//! Spline geometry: the curves of [`Geom::Spline`] and [`Geom::ControlSpline`]
//! (`docs/re/sketch-entities.md`, "Splines").
//!
//! Both are cubic and held as a chain of Bézier spans, which is how they reach the kernel
//! (`anvil_kernel::profile::Seg2::Cubic`) and STEP (`B_SPLINE_CURVE_WITH_KNOTS`).
//!
//! - A **spline** runs through its points. It is the C2 cubic interpolant with one span
//!   between each pair of points, parametrised by chord length. Each end is natural (no
//!   bending there) unless it has a handle, whose position is the end span's inner
//!   control point: the end leaves towards the handle, as fast as the handle is far.
//! - A **control point spline** is the clamped cubic B-spline of its control vertices,
//!   with uniform knots. It runs through its first and last vertex and leaves each end
//!   along the polygon's first and last side. With four vertices it is one Bézier span.

use crate::sketch::{EntityId, Geom, PointId, Sketch};
use anvil_kernel::profile::{cubic_derivative, cubic_point, cubic_second_derivative, GAUSS5};
use anvil_kernel::Vec2;

/// The degree of both kinds of spline.
pub const DEGREE: usize = 3;

/// Fewest points of a spline, and fewest control vertices of a control point spline.
pub const MIN_THROUGH: usize = 2;
pub const MIN_CONTROL: usize = DEGREE + 1;

/// A spline as cubic Bézier spans, each starting where the one before ends.
#[derive(Clone, Debug, PartialEq)]
pub struct Spline2 {
    pub spans: Vec<[Vec2; 4]>,
}

impl Spline2 {
    /// The spline through `pts` with optional handles at its start and end. `None` when
    /// there are fewer than two points, two neighbours coincide, or a value is not finite.
    pub fn through(pts: &[Vec2], start_handle: Option<Vec2>, end_handle: Option<Vec2>) -> Option<Spline2> {
        let n = pts.len();
        if n < MIN_THROUGH
            || !all_finite(pts)
            || ![start_handle, end_handle].iter().flatten().all(|h| finite(*h))
        {
            return None;
        }
        // Chord lengths; neighbours that coincide leave no span.
        let h: Vec<f64> = pts.windows(2).map(|w| w[0].distance(w[1])).collect();
        let scale = h.iter().sum::<f64>();
        if h.iter().any(|&x| x <= 1e-12 * scale.max(1.0)) {
            return None;
        }
        let d = interpolant_derivatives(pts, &h, start_handle, end_handle);
        let spans = (0..n - 1)
            .map(|i| [pts[i], pts[i] + d[i] * (h[i] / 3.0), pts[i + 1] - d[i + 1] * (h[i] / 3.0), pts[i + 1]])
            .collect();
        Some(Spline2 { spans })
    }

    /// The clamped cubic B-spline of control vertices `pts`, uniform knots. `None` with
    /// fewer than four vertices, or when its ends coincide or a value is not finite.
    pub fn control(pts: &[Vec2]) -> Option<Spline2> {
        let n = pts.len();
        if n < MIN_CONTROL || !all_finite(pts) {
            return None;
        }
        let spans = n - DEGREE;
        // Knots 0,0,0,0, 1, 2, ..., spans-1, spans,spans,spans,spans.
        let mut knots = vec![0.0; DEGREE + 1];
        knots.extend((1..spans).map(|k| k as f64));
        knots.extend(vec![spans as f64; DEGREE + 1]);
        let mut ctrl = pts.to_vec();
        // Raise each interior knot to multiplicity 3 (Boehm's insertion); the control
        // points then are the Bézier spans' end to end.
        for k in 1..spans {
            for _ in 0..DEGREE - 1 {
                insert_knot(&mut knots, &mut ctrl, k as f64);
            }
        }
        let spans =
            (0..spans).map(|s| [ctrl[3 * s], ctrl[3 * s + 1], ctrl[3 * s + 2], ctrl[3 * s + 3]]).collect();
        Some(Spline2 { spans })
    }

    pub fn start(&self) -> Vec2 {
        self.spans.first().map_or(Vec2::ZERO, |s| s[0])
    }

    pub fn end(&self) -> Vec2 {
        self.spans.last().map_or(Vec2::ZERO, |s| s[3])
    }

    /// The first and second derivatives (in the end span's own parameter) at the start
    /// (`at_end` false) or the end, both pointing the way the spline runs.
    pub fn end_derivatives(&self, at_end: bool) -> (Vec2, Vec2) {
        let (Some(first), Some(last)) = (self.spans.first(), self.spans.last()) else {
            return (Vec2::ZERO, Vec2::ZERO);
        };
        if at_end {
            (cubic_derivative(last, 1.0), cubic_second_derivative(last, 1.0))
        } else {
            (cubic_derivative(first, 0.0), cubic_second_derivative(first, 0.0))
        }
    }

    /// The unit tangent at the start or the end, pointing the way the spline runs.
    pub fn end_tangent(&self, at_end: bool) -> Vec2 {
        let (d, _) = self.end_derivatives(at_end);
        if d.length() > 1e-12 {
            return d.normalized();
        }
        // An end whose inner control point sits on it leaves along the next one.
        let s = if at_end { self.spans.last() } else { self.spans.first() };
        match s {
            Some(s) if at_end => (s[3] - s[1]).normalized(),
            Some(s) => (s[2] - s[0]).normalized(),
            None => Vec2::ZERO,
        }
    }

    /// The curvature vector at the start or the end: towards the centre of curvature,
    /// of length one over the radius. It does not depend on the way the spline runs.
    pub fn end_curvature(&self, at_end: bool) -> Vec2 {
        let (d1, d2) = self.end_derivatives(at_end);
        curvature_vector(d1, d2)
    }

    /// The length along the spline.
    pub fn length(&self) -> f64 {
        self.spans.iter().map(span_length).sum()
    }

    /// The point `t` (0 to 1) of the way along span `i`.
    pub fn at(&self, i: usize, t: f64) -> Vec2 {
        self.spans.get(i).map_or(Vec2::ZERO, |s| cubic_point(s, t))
    }

    /// Points along the spline, `per_span` steps to a span, from start to end inclusive.
    pub fn polyline(&self, per_span: usize) -> Vec<Vec2> {
        let per = per_span.max(1);
        let mut out = Vec::with_capacity(self.spans.len() * per + 1);
        for s in &self.spans {
            for k in 0..per {
                out.push(cubic_point(s, k as f64 / per as f64));
            }
        }
        out.push(self.end());
        out
    }

    /// The span and parameter of the point of the spline nearest `p`, and its distance.
    pub fn nearest(&self, p: Vec2) -> (usize, f64, f64) {
        let mut best = (0, 0.0, f64::INFINITY);
        for (i, s) in self.spans.iter().enumerate() {
            // A coarse search, then Newton steps on (B(t) - p) . B'(t) = 0.
            let n = 16;
            let mut t = (0..=n)
                .map(|k| k as f64 / n as f64)
                .min_by(|a, b| cubic_point(s, *a).distance(p).total_cmp(&cubic_point(s, *b).distance(p)))
                .unwrap_or(0.0);
            for _ in 0..12 {
                let (q, d1, d2) = (cubic_point(s, t), cubic_derivative(s, t), cubic_second_derivative(s, t));
                let f = (q - p).dot(d1);
                let df = d1.dot(d1) + (q - p).dot(d2);
                if df.abs() < 1e-18 {
                    break;
                }
                t = (t - f / df).clamp(0.0, 1.0);
            }
            let d = cubic_point(s, t).distance(p);
            if d < best.2 {
                best = (i, t, d);
            }
        }
        best
    }

    /// The distance from `p` to the spline.
    pub fn distance(&self, p: Vec2) -> f64 {
        self.nearest(p).2
    }

    /// The distance from `p` to the spline, positive to the left of its direction of
    /// travel. Zero on the curve, and smooth across it, as a solver residual needs.
    pub fn signed_distance(&self, p: Vec2) -> f64 {
        let (i, t, d) = self.nearest(p);
        let Some(s) = self.spans.get(i) else { return d };
        let tangent = cubic_derivative(s, t);
        if tangent.cross(p - cubic_point(s, t)) < 0.0 {
            -d
        } else {
            d
        }
    }
}

/// The length of one span: five-point Gauss on eight pieces.
fn span_length(s: &[Vec2; 4]) -> f64 {
    let n = 8;
    let mut sum = 0.0;
    for k in 0..n {
        for (x, w) in GAUSS5 {
            sum += w * cubic_derivative(s, (k as f64 + x) / n as f64).length();
        }
    }
    sum / n as f64
}

/// The curvature vector of a curve whose first and second derivatives are `d1` and `d2`.
pub fn curvature_vector(d1: Vec2, d2: Vec2) -> Vec2 {
    let s2 = d1.dot(d1);
    if s2 < 1e-24 {
        return Vec2::ZERO;
    }
    (d2 - d1 * (d2.dot(d1) / s2)) / s2
}

fn finite(p: Vec2) -> bool {
    p.x.is_finite() && p.y.is_finite()
}

fn all_finite(pts: &[Vec2]) -> bool {
    pts.iter().all(|p| finite(*p))
}

/// The first derivatives (in chord length) of the C2 cubic through `pts` at each point.
/// Interior points: h_i D_(i-1) + 2 (h_(i-1) + h_i) D_i + h_(i-1) D_(i+1) =
/// 3 (h_i δ_(i-1) + h_(i-1) δ_i), with δ_i the chord direction scaled by its length's
/// reciprocal. A natural end has 2 D_0 + D_1 = 3 δ_0; an end with a handle has D_0 three
/// times the handle's offset over the first chord. Solved by the tridiagonal algorithm.
fn interpolant_derivatives(pts: &[Vec2], h: &[f64], start: Option<Vec2>, end: Option<Vec2>) -> Vec<Vec2> {
    let n = pts.len();
    let delta: Vec<Vec2> = (0..n - 1).map(|i| (pts[i + 1] - pts[i]) / h[i]).collect();
    // Rows: lower[i] D_(i-1) + diag[i] D_i + upper[i] D_(i+1) = rhs[i].
    let mut lower = vec![0.0; n];
    let mut diag = vec![0.0; n];
    let mut upper = vec![0.0; n];
    let mut rhs = vec![Vec2::ZERO; n];
    match start {
        Some(hd) => {
            diag[0] = 1.0;
            rhs[0] = (hd - pts[0]) * (3.0 / h[0]);
        }
        None => {
            diag[0] = 2.0;
            upper[0] = 1.0;
            rhs[0] = delta[0] * 3.0;
        }
    }
    for i in 1..n - 1 {
        lower[i] = h[i];
        diag[i] = 2.0 * (h[i - 1] + h[i]);
        upper[i] = h[i - 1];
        rhs[i] = (delta[i - 1] * h[i] + delta[i] * h[i - 1]) * 3.0;
    }
    match end {
        Some(hd) => {
            diag[n - 1] = 1.0;
            lower[n - 1] = 0.0;
            rhs[n - 1] = (pts[n - 1] - hd) * (3.0 / h[n - 2]);
        }
        None => {
            lower[n - 1] = 1.0;
            diag[n - 1] = 2.0;
            rhs[n - 1] = delta[n - 2] * 3.0;
        }
    }
    // Forward sweep, then back substitution. The system is diagonally dominant.
    for i in 1..n {
        let m = lower[i] / diag[i - 1];
        diag[i] -= m * upper[i - 1];
        rhs[i] = rhs[i] - rhs[i - 1] * m;
    }
    let mut d = vec![Vec2::ZERO; n];
    d[n - 1] = rhs[n - 1] / diag[n - 1];
    for i in (0..n - 1).rev() {
        d[i] = (rhs[i] - d[i + 1] * upper[i]) / diag[i];
    }
    d
}

/// Inserts knot `u` once into a cubic B-spline (Boehm's algorithm).
fn insert_knot(knots: &mut Vec<f64>, ctrl: &mut Vec<Vec2>, u: f64) {
    let p = DEGREE;
    // The span holding u: knots[k] <= u < knots[k + 1], taking the last such k.
    let k = match (p..knots.len() - p - 1).rev().find(|&i| knots[i] <= u) {
        Some(k) => k,
        None => return,
    };
    let mut q = Vec::with_capacity(ctrl.len() + 1);
    for i in 0..=ctrl.len() {
        if i + p <= k {
            q.push(ctrl[i]);
        } else if i > k {
            q.push(ctrl[i - 1]);
        } else {
            let den = knots[i + p] - knots[i];
            let a = if den.abs() < 1e-300 { 0.0 } else { (u - knots[i]) / den };
            q.push(ctrl[i - 1] * (1.0 - a) + ctrl[i] * a);
        }
    }
    knots.insert(k + 1, u);
    *ctrl = q;
}

/// The curve of spline entity `e`, if it is one, from point positions `pos`. The solver
/// passes its trial positions; everything else passes the sketch's.
pub fn of_geom(geom: &Geom, pos: &dyn Fn(PointId) -> Vec2) -> Option<Spline2> {
    match geom {
        Geom::Spline { pts, handles } => {
            let p: Vec<Vec2> = pts.iter().map(|&i| pos(i)).collect();
            Spline2::through(&p, handles[0].map(pos), handles[1].map(pos))
        }
        Geom::ControlSpline { pts } => {
            let p: Vec<Vec2> = pts.iter().map(|&i| pos(i)).collect();
            Spline2::control(&p)
        }
        _ => None,
    }
}

/// The curve of spline entity `e` in sketch `sk`.
pub fn of(sk: &Sketch, e: EntityId) -> Option<Spline2> {
    let n = sk.points.len();
    let geom = &sk.entities.get(e)?.geom;
    if geom_points(geom).iter().any(|&p| p >= n) {
        return None;
    }
    of_geom(geom, &|p| sk.pos(p))
}

fn geom_points(g: &Geom) -> Vec<PointId> {
    match g {
        Geom::Spline { pts, handles } => {
            pts.iter().copied().chain(handles.iter().flatten().copied()).collect()
        }
        Geom::ControlSpline { pts } => pts.clone(),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anvil_kernel::v2;

    fn close(a: Vec2, b: Vec2) -> bool {
        a.distance(b) < 1e-9
    }

    /// Four control vertices make one Bézier span: the vertices themselves.
    #[test]
    fn four_vertices_are_a_bezier_span() {
        let p = [v2(0.0, 0.0), v2(0.0, 30.0), v2(40.0, 30.0), v2(40.0, 0.0)];
        let s = Spline2::control(&p).expect("spline");
        assert_eq!(s.spans, vec![p]);
        // B(1/2) = (p0 + 3 p1 + 3 p2 + p3) / 8.
        assert!(close(s.at(0, 0.5), v2(20.0, 22.5)), "{:?}", s.at(0, 0.5));
        assert!(Spline2::control(&p[..3]).is_none());
    }

    /// A clamped uniform cubic B-spline of five vertices has two spans, joined with equal
    /// first and second derivatives.
    #[test]
    fn five_vertices_are_two_spans() {
        let p = [v2(0.0, 0.0), v2(10.0, 20.0), v2(30.0, 20.0), v2(40.0, 0.0), v2(60.0, 10.0)];
        let s = Spline2::control(&p).expect("spline");
        assert_eq!(s.spans.len(), 2);
        assert!(close(s.start(), p[0]) && close(s.end(), p[4]));
        // At the interior knot of knots 0,0,0,0,1,2,2,2,2 the basis values are
        // (0, 1/4, 1/2, 1/4, 0).
        let want = p[1] * 0.25 + p[2] * 0.5 + p[3] * 0.25;
        assert!(close(s.spans[0][3], want), "{:?} vs {want:?}", s.spans[0][3]);
        assert!(close(s.spans[1][0], want));
        // The spans join smoothly: equal derivatives at the joint.
        let (d0, d1) = (cubic_derivative(&s.spans[0], 1.0), cubic_derivative(&s.spans[1], 0.0));
        assert!(close(d0, d1), "{d0:?} {d1:?}");
        let (e0, e1) = (cubic_second_derivative(&s.spans[0], 1.0), cubic_second_derivative(&s.spans[1], 0.0));
        assert!(close(e0, e1), "{e0:?} {e1:?}");
        // It leaves its ends along the polygon's end sides.
        assert!(s.end_tangent(false).cross((p[1] - p[0]).normalized()).abs() < 1e-12);
        assert!(s.end_tangent(true).cross((p[4] - p[3]).normalized()).abs() < 1e-12);
    }

    /// Through two points with natural ends, the spline is the straight line.
    #[test]
    fn two_points_make_a_straight_spline() {
        let s = Spline2::through(&[v2(0.0, 0.0), v2(30.0, 40.0)], None, None).expect("spline");
        assert_eq!(s.spans.len(), 1);
        assert!(close(s.at(0, 0.5), v2(15.0, 20.0)));
        assert!((s.length() - 50.0).abs() < 1e-9);
        assert!(s.end_curvature(false).length() < 1e-12);
    }

    /// Through three points evenly spaced on a line with a bump, the spline is C2, passes
    /// through every point and has no bending at its natural ends.
    #[test]
    fn through_points_is_c2_with_natural_ends() {
        let pts = [v2(0.0, 0.0), v2(10.0, 10.0), v2(20.0, 0.0), v2(35.0, 5.0)];
        let s = Spline2::through(&pts, None, None).expect("spline");
        assert_eq!(s.spans.len(), 3);
        for (i, p) in pts.iter().enumerate().take(3) {
            assert!(close(s.spans[i][0], *p));
        }
        assert!(close(s.end(), pts[3]));
        // Natural ends: no second derivative.
        assert!(s.end_derivatives(false).1.length() < 1e-9);
        assert!(s.end_derivatives(true).1.length() < 1e-9);
        // C2 at each joint, in the chord-length parameter (span parameters scale by h).
        let h: Vec<f64> = pts.windows(2).map(|w| w[0].distance(w[1])).collect();
        for i in 0..2 {
            let (a, b) = (&s.spans[i], &s.spans[i + 1]);
            let (d0, d1) = (cubic_derivative(a, 1.0) / h[i], cubic_derivative(b, 0.0) / h[i + 1]);
            assert!(close(d0, d1), "joint {i}: {d0:?} {d1:?}");
            let (e0, e1) = (
                cubic_second_derivative(a, 1.0) / (h[i] * h[i]),
                cubic_second_derivative(b, 0.0) / (h[i + 1] * h[i + 1]),
            );
            assert!(e0.distance(e1) < 1e-9, "joint {i}: {e0:?} {e1:?}");
        }
        // Points that coincide make no spline.
        assert!(Spline2::through(&[v2(0.0, 0.0), v2(0.0, 0.0), v2(1.0, 0.0)], None, None).is_none());
    }

    /// A handle is the end span's inner control point.
    #[test]
    fn handles_set_the_end_spans() {
        let pts = [v2(0.0, 0.0), v2(30.0, 0.0)];
        let s = Spline2::through(&pts, Some(v2(0.0, 10.0)), Some(v2(30.0, 10.0))).expect("spline");
        assert_eq!(s.spans[0], [pts[0], v2(0.0, 10.0), v2(30.0, 10.0), pts[1]]);
        assert!(close(s.end_tangent(false), v2(0.0, 1.0)));
        assert!(close(s.end_tangent(true), v2(0.0, -1.0)));
    }

    /// The curvature of a cubic span at its start is 2/3 |p0p1 x p1p2| / |p0p1|^3.
    #[test]
    fn end_curvature_of_a_span() {
        let p = [v2(0.0, 0.0), v2(10.0, 0.0), v2(20.0, 10.0), v2(30.0, 10.0)];
        let s = Spline2::control(&p).expect("spline");
        let k = s.end_curvature(false);
        let want = 2.0 / 3.0 * (p[1] - p[0]).cross(p[2] - p[1]).abs() / 1000.0;
        assert!((k.length() - want).abs() < 1e-12 && k.y > 0.0, "{k:?} vs {want}");
    }

    #[test]
    fn nearest_point_and_signed_distance() {
        let p = [v2(0.0, 0.0), v2(0.0, 30.0), v2(40.0, 30.0), v2(40.0, 0.0)];
        let s = Spline2::control(&p).expect("spline");
        // The top of the arch is (20, 22.5), where the spline runs towards +x: a point above
        // is on its left.
        assert!((s.signed_distance(v2(20.0, 30.0)) - 7.5).abs() < 1e-9);
        assert!((s.signed_distance(v2(20.0, 20.0)) + 2.5).abs() < 1e-9);
        assert!(s.distance(v2(0.0, 0.0)) < 1e-12);
    }
}
