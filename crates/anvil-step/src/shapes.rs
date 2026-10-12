//! Curves and surfaces as STEP describes them, with evaluation and point inversion.
//!
//! Lengths are already scaled to millimetres when these are built.

use anvil_brep::Frame;
use glam::{DVec2, DVec3};
use std::f64::consts::{PI, TAU};

/// A non-uniform rational B-spline basis: degree, full knot vector, and weights
/// (empty for non-rational).
#[derive(Debug, Clone, PartialEq)]
pub struct Basis {
    pub degree: usize,
    pub knots: Vec<f64>,
}

impl Basis {
    /// Expands STEP's distinct knots plus multiplicities into a full knot vector.
    pub fn new(degree: usize, mults: &[i64], knots: &[f64]) -> Option<Basis> {
        let mut full = Vec::new();
        for (m, k) in mults.iter().zip(knots) {
            if *m < 0 || *m > 64 {
                return None;
            }
            full.extend(std::iter::repeat_n(*k, *m as usize));
        }
        if degree == 0 || degree > 32 || full.len() < 2 * (degree + 1) {
            return None;
        }
        if full.windows(2).any(|w| w[1] < w[0]) || full.iter().any(|k| !k.is_finite()) {
            return None;
        }
        Some(Basis { degree, knots: full })
    }

    /// Number of control points this basis expects.
    pub fn count(&self) -> usize {
        self.knots.len() - self.degree - 1
    }

    pub fn domain(&self) -> (f64, f64) {
        (self.knots[self.degree], self.knots[self.count()])
    }

    /// Index of the knot span containing `t`, and the non-zero basis values there.
    fn eval(&self, t: f64) -> (usize, Vec<f64>) {
        let p = self.degree;
        let n = self.count();
        let (lo, hi) = self.domain();
        let t = t.clamp(lo, hi);
        // Span search: largest i in [p, n-1] with knots[i] <= t.
        let mut span = p;
        for i in p..n {
            if self.knots[i] <= t {
                span = i;
            }
        }
        while span > p && self.knots[span] >= self.knots[span + 1] {
            span -= 1;
        }
        // Cox-de Boor, triangular scheme.
        let mut left = vec![0.0; p + 1];
        let mut right = vec![0.0; p + 1];
        let mut basis = vec![0.0; p + 1];
        basis[0] = 1.0;
        for j in 1..=p {
            left[j] = t - self.knots[span + 1 - j];
            right[j] = self.knots[span + j] - t;
            let mut saved = 0.0;
            for r in 0..j {
                let den = right[r + 1] + left[j - r];
                let tmp = if den.abs() < 1e-300 { 0.0 } else { basis[r] / den };
                basis[r] = saved + right[r + 1] * tmp;
                saved = left[j - r] * tmp;
            }
            basis[j] = saved;
        }
        (span, basis)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct BSplineCurve {
    pub basis: Basis,
    pub points: Vec<DVec3>,
    /// Empty when not rational.
    pub weights: Vec<f64>,
}

impl BSplineCurve {
    pub fn eval(&self, t: f64) -> DVec3 {
        let (span, b) = self.basis.eval(t);
        let p = self.basis.degree;
        let mut acc = DVec3::ZERO;
        let mut w = 0.0;
        for (j, bj) in b.iter().enumerate() {
            let i = span - p + j;
            let wi = self.weights.get(i).copied().unwrap_or(1.0);
            if let Some(cp) = self.points.get(i) {
                acc += *cp * (bj * wi);
                w += bj * wi;
            }
        }
        if w.abs() < 1e-300 { acc } else { acc / w }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct BSplineSurface {
    pub u: Basis,
    pub v: Basis,
    /// Control net, `points[i][j]` for u index i and v index j.
    pub points: Vec<Vec<DVec3>>,
    /// Empty when not rational, otherwise the same shape as `points`.
    pub weights: Vec<Vec<f64>>,
}

impl BSplineSurface {
    pub fn eval(&self, uv: DVec2) -> DVec3 {
        let (su, bu) = self.u.eval(uv.x);
        let (sv, bv) = self.v.eval(uv.y);
        let (pu, pv) = (self.u.degree, self.v.degree);
        let mut acc = DVec3::ZERO;
        let mut w = 0.0;
        for (a, ba) in bu.iter().enumerate() {
            let i = su - pu + a;
            for (b, bb) in bv.iter().enumerate() {
                let j = sv - pv + b;
                let wij = self.weights.get(i).and_then(|r| r.get(j)).copied().unwrap_or(1.0);
                if let Some(cp) = self.points.get(i).and_then(|r| r.get(j)) {
                    let k = ba * bb * wij;
                    acc += *cp * k;
                    w += k;
                }
            }
        }
        if w.abs() < 1e-300 { acc } else { acc / w }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Curve {
    Line { origin: DVec3, dir: DVec3 },
    Circle { frame: Frame, radius: f64 },
    Ellipse { frame: Frame, a: f64, b: f64 },
    BSpline(BSplineCurve),
    Polyline(Vec<DVec3>),
}

impl Curve {
    /// Parameter range over which the curve is defined (lines are unbounded).
    pub fn domain(&self) -> (f64, f64) {
        match self {
            Curve::Line { .. } => (f64::NEG_INFINITY, f64::INFINITY),
            Curve::Circle { .. } | Curve::Ellipse { .. } => (0.0, TAU),
            Curve::BSpline(c) => c.basis.domain(),
            Curve::Polyline(p) => (0.0, (p.len().max(1) - 1) as f64),
        }
    }

    pub fn is_periodic(&self) -> bool {
        matches!(self, Curve::Circle { .. } | Curve::Ellipse { .. })
    }

    pub fn eval(&self, t: f64) -> DVec3 {
        match self {
            Curve::Line { origin, dir } => *origin + *dir * t,
            Curve::Circle { frame, radius } => {
                frame.origin + (frame.x * t.cos() + frame.y * t.sin()) * *radius
            }
            Curve::Ellipse { frame, a, b } => {
                frame.origin + frame.x * (a * t.cos()) + frame.y * (b * t.sin())
            }
            Curve::BSpline(c) => c.eval(t),
            Curve::Polyline(p) => {
                if p.is_empty() {
                    return DVec3::ZERO;
                }
                let t = t.clamp(0.0, (p.len() - 1) as f64);
                let i = (t.floor() as usize).min(p.len().saturating_sub(2));
                let f = t - i as f64;
                match (p.get(i), p.get(i + 1)) {
                    (Some(a), Some(b)) => a.lerp(*b, f),
                    (Some(a), None) => *a,
                    _ => DVec3::ZERO,
                }
            }
        }
    }

    /// The parameter of the point on the curve nearest `p`.
    pub fn invert(&self, p: DVec3) -> f64 {
        match self {
            Curve::Line { origin, dir } => (p - *origin).dot(*dir) / dir.length_squared().max(1e-300),
            Curve::Circle { frame, .. } => {
                let l = frame.to_local(p);
                l.y.atan2(l.x).rem_euclid(TAU)
            }
            Curve::Ellipse { frame, a, b } => {
                let l = frame.to_local(p);
                (l.y / b).atan2(l.x / a).rem_euclid(TAU)
            }
            _ => {
                let (lo, hi) = self.domain();
                minimize_1d(lo, hi, 200, |t| (self.eval(t) - p).length_squared())
            }
        }
    }

    /// Points from parameter `t0` to `t1` (either order), chord error under `chord`.
    pub fn sample(&self, t0: f64, t1: f64, chord: f64) -> Vec<DVec3> {
        let n = match self {
            Curve::Line { .. } => 1,
            Curve::Circle { radius, .. } => arc_steps(*radius, (t1 - t0).abs(), chord),
            Curve::Ellipse { a, b, .. } => arc_steps(a.max(*b), (t1 - t0).abs(), chord),
            Curve::Polyline(p) => return polyline_between(p, t0, t1),
            Curve::BSpline(c) => {
                // Enough samples per knot span to follow the curve closely.
                let spans = c.basis.knots.windows(2).filter(|w| w[1] > w[0]).count().max(1);
                let (lo, hi) = c.basis.domain();
                let frac = ((t1 - t0).abs() / (hi - lo).max(1e-300)).clamp(0.0, 1.0);
                ((spans as f64 * 12.0 * frac).ceil() as usize).clamp(2, 4000)
            }
        };
        (0..=n).map(|i| self.eval(t0 + (t1 - t0) * i as f64 / n as f64)).collect()
    }
}

fn polyline_between(p: &[DVec3], t0: f64, t1: f64) -> Vec<DVec3> {
    if p.is_empty() {
        return Vec::new();
    }
    let last = (p.len() - 1) as f64;
    let (a, b) = (t0.clamp(0.0, last), t1.clamp(0.0, last));
    let eval = |t: f64| Curve::Polyline(p.to_vec()).eval(t);
    let mut out = vec![eval(a)];
    if a <= b {
        let mut i = a.floor() as usize + 1;
        while (i as f64) < b {
            out.push(p[i]);
            i += 1;
        }
    } else {
        let mut i = a.ceil() as i64 - 1;
        while i as f64 > b && i >= 0 {
            out.push(p[i as usize]);
            i -= 1;
        }
    }
    out.push(eval(b));
    out
}

/// Number of chords for an arc of `sweep` radians.
fn arc_steps(radius: f64, sweep: f64, chord: f64) -> usize {
    let mut step = PI / 36.0;
    if chord > 0.0 && chord < radius {
        step = step.min(2.0 * (1.0 - chord / radius).acos());
    }
    ((sweep / step).ceil() as usize).clamp(1, 4000)
}

/// Minimizes `f` on [lo, hi]: coarse scan, then golden-section refinement.
pub fn minimize_1d(lo: f64, hi: f64, n: usize, f: impl Fn(f64) -> f64) -> f64 {
    if !(lo.is_finite() && hi.is_finite()) || hi <= lo {
        return lo;
    }
    let mut best = lo;
    let mut best_v = f64::INFINITY;
    for i in 0..=n {
        let t = lo + (hi - lo) * i as f64 / n as f64;
        let v = f(t);
        if v < best_v {
            best_v = v;
            best = t;
        }
    }
    let step = (hi - lo) / n as f64;
    let (mut a, mut b) = ((best - step).max(lo), (best + step).min(hi));
    for _ in 0..80 {
        let m1 = a + (b - a) * 0.382;
        let m2 = a + (b - a) * 0.618;
        if f(m1) < f(m2) {
            b = m2;
        } else {
            a = m1;
        }
    }
    0.5 * (a + b)
}

#[derive(Debug, Clone, PartialEq)]
pub enum Surface {
    Plane {
        frame: Frame,
    },
    Cylinder {
        frame: Frame,
        radius: f64,
    },
    /// Radius `radius` in the placement plane, opening at `semi_angle`.
    Cone {
        frame: Frame,
        radius: f64,
        semi_angle: f64,
    },
    Sphere {
        frame: Frame,
        radius: f64,
    },
    Torus {
        frame: Frame,
        major: f64,
        minor: f64,
    },
    BSpline(BSplineSurface),
}

impl Surface {
    /// Which parameters wrap around (u, v).
    pub fn periodic(&self) -> (bool, bool) {
        match self {
            Surface::Cylinder { .. } | Surface::Cone { .. } | Surface::Sphere { .. } => (true, false),
            Surface::Torus { .. } => (true, true),
            _ => (false, false),
        }
    }

    pub fn eval(&self, uv: DVec2) -> DVec3 {
        let (u, v) = (uv.x, uv.y);
        match self {
            Surface::Plane { frame } => frame.to_world(uv),
            Surface::Cylinder { frame, radius } => {
                frame.origin + (frame.x * u.cos() + frame.y * u.sin()) * *radius + frame.z * v
            }
            Surface::Cone { frame, radius, semi_angle } => {
                let r = radius + v * semi_angle.tan();
                frame.origin + (frame.x * u.cos() + frame.y * u.sin()) * r + frame.z * v
            }
            Surface::Sphere { frame, radius } => {
                frame.origin
                    + (frame.x * u.cos() + frame.y * u.sin()) * (radius * v.cos())
                    + frame.z * (radius * v.sin())
            }
            Surface::Torus { frame, major, minor } => {
                frame.origin
                    + (frame.x * u.cos() + frame.y * u.sin()) * (major + minor * v.cos())
                    + frame.z * (minor * v.sin())
            }
            Surface::BSpline(s) => s.eval(uv),
        }
    }

    /// The natural normal (du x dv direction), by finite differences.
    pub fn normal(&self, uv: DVec2) -> DVec3 {
        let h = match self {
            Surface::BSpline(s) => {
                let (a, b) = s.u.domain();
                ((b - a) * 1e-5).max(1e-9)
            }
            _ => 1e-5,
        };
        let hv = match self {
            Surface::BSpline(s) => {
                let (a, b) = s.v.domain();
                ((b - a) * 1e-5).max(1e-9)
            }
            Surface::Plane { .. } | Surface::Cylinder { .. } | Surface::Cone { .. } => 1e-3,
            _ => 1e-5,
        };
        let du = self.eval(uv + DVec2::new(h, 0.0)) - self.eval(uv - DVec2::new(h, 0.0));
        let dv = self.eval(uv + DVec2::new(0.0, hv)) - self.eval(uv - DVec2::new(0.0, hv));
        du.cross(dv).normalize_or_zero()
    }

    /// Parameters of the surface point nearest `p`. Angles are principal values;
    /// callers unwrap periodic parameters themselves.
    pub fn invert(&self, p: DVec3) -> DVec2 {
        match self {
            Surface::Plane { frame } => {
                let l = frame.to_local(p);
                DVec2::new(l.x, l.y)
            }
            Surface::Cylinder { frame, .. } | Surface::Cone { frame, .. } => {
                let l = frame.to_local(p);
                DVec2::new(l.y.atan2(l.x), l.z)
            }
            Surface::Sphere { frame, .. } => {
                let l = frame.to_local(p);
                let rho = DVec2::new(l.x, l.y).length();
                DVec2::new(l.y.atan2(l.x), l.z.atan2(rho))
            }
            Surface::Torus { frame, major, .. } => {
                let l = frame.to_local(p);
                let rho = DVec2::new(l.x, l.y).length();
                DVec2::new(l.y.atan2(l.x), l.z.atan2(rho - major))
            }
            Surface::BSpline(s) => invert_bspline(s, p),
        }
    }

    /// Approximate lengths of a unit step in u and v near `uv`, so parameter space
    /// can be scaled to millimetres for triangulation.
    pub fn metric(&self, uv: DVec2) -> DVec2 {
        let h = 1e-4;
        let du =
            (self.eval(uv + DVec2::new(h, 0.0)) - self.eval(uv - DVec2::new(h, 0.0))).length() / (2.0 * h);
        let dv =
            (self.eval(uv + DVec2::new(0.0, h)) - self.eval(uv - DVec2::new(0.0, h))).length() / (2.0 * h);
        DVec2::new(du.max(1e-6), dv.max(1e-6))
    }
}

fn invert_bspline(s: &BSplineSurface, p: DVec3) -> DVec2 {
    let (u0, u1) = s.u.domain();
    let (v0, v1) = s.v.domain();
    // Coarse grid, then Gauss-Newton on the distance with numeric derivatives.
    let n = 24;
    let mut best = DVec2::new(u0, v0);
    let mut best_d = f64::INFINITY;
    for i in 0..=n {
        for j in 0..=n {
            let uv = DVec2::new(u0 + (u1 - u0) * i as f64 / n as f64, v0 + (v1 - v0) * j as f64 / n as f64);
            let d = (s.eval(uv) - p).length_squared();
            if d < best_d {
                best_d = d;
                best = uv;
            }
        }
    }
    let mut uv = best;
    let hu = (u1 - u0).max(1e-12) * 1e-6;
    let hv = (v1 - v0).max(1e-12) * 1e-6;
    for _ in 0..30 {
        let f = s.eval(uv) - p;
        let su = (s.eval(uv + DVec2::new(hu, 0.0)) - s.eval(uv - DVec2::new(hu, 0.0))) / (2.0 * hu);
        let sv = (s.eval(uv + DVec2::new(0.0, hv)) - s.eval(uv - DVec2::new(0.0, hv))) / (2.0 * hv);
        let (a, b, c) = (su.dot(su), su.dot(sv), sv.dot(sv));
        let (g1, g2) = (su.dot(f), sv.dot(f));
        let det = a * c - b * b;
        if det.abs() < 1e-300 {
            break;
        }
        let step = DVec2::new((c * g1 - b * g2) / det, (a * g2 - b * g1) / det);
        let next = DVec2::new((uv.x - step.x).clamp(u0, u1), (uv.y - step.y).clamp(v0, v1));
        if (s.eval(next) - p).length_squared() > f.length_squared() {
            break;
        }
        let moved = (next - uv).length();
        uv = next;
        if moved < 1e-14 {
            break;
        }
    }
    uv
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame() -> Frame {
        Frame::new(DVec3::new(1.0, 2.0, 3.0), DVec3::new(0.0, 0.3, 1.0), DVec3::X).expect("frame")
    }

    #[test]
    fn bspline_reproduces_a_quarter_circle() {
        // Rational quadratic Bezier for a quarter of the unit circle.
        let w = std::f64::consts::FRAC_1_SQRT_2;
        let c = BSplineCurve {
            basis: Basis::new(2, &[3, 3], &[0.0, 1.0]).expect("basis"),
            points: vec![DVec3::X, DVec3::new(1.0, 1.0, 0.0), DVec3::Y],
            weights: vec![1.0, w, 1.0],
        };
        for i in 0..=10 {
            let p = c.eval(i as f64 / 10.0);
            assert!((p.length() - 1.0).abs() < 1e-12, "{p:?}");
        }
        let t = Curve::BSpline(c.clone()).invert(DVec3::new(0.6, 0.8, 0.0));
        assert!((c.eval(t) - DVec3::new(0.6, 0.8, 0.0)).length() < 1e-9);
    }

    #[test]
    fn bad_knots_are_rejected() {
        assert!(Basis::new(2, &[3, 3], &[1.0, 0.0]).is_none());
        assert!(Basis::new(3, &[1, 1], &[0.0, 1.0]).is_none());
        assert!(Basis::new(2, &[-1, 3], &[0.0, 1.0]).is_none());
    }

    #[test]
    fn analytic_surfaces_invert_their_own_points() {
        let f = frame();
        let surfaces = [
            Surface::Plane { frame: f },
            Surface::Cylinder { frame: f, radius: 3.0 },
            Surface::Cone { frame: f, radius: 2.0, semi_angle: 0.3 },
            Surface::Sphere { frame: f, radius: 4.0 },
            Surface::Torus { frame: f, major: 5.0, minor: 1.5 },
        ];
        for s in &surfaces {
            for uv in [DVec2::new(0.4, 0.2), DVec2::new(-2.0, 0.7), DVec2::new(2.9, -0.5)] {
                let p = s.eval(uv);
                let back = s.eval(s.invert(p));
                assert!((back - p).length() < 1e-9, "{s:?} {uv:?}");
                assert!(s.normal(uv).length() > 0.5);
            }
        }
    }

    #[test]
    fn bspline_surface_inverts() {
        // Bilinear patch with a bump in the middle control point.
        let pts: Vec<Vec<DVec3>> = (0..3)
            .map(|i| {
                (0..3)
                    .map(|j| DVec3::new(i as f64, j as f64, if i == 1 && j == 1 { 1.0 } else { 0.0 }))
                    .collect()
            })
            .collect();
        let s = BSplineSurface {
            u: Basis::new(2, &[3, 3], &[0.0, 1.0]).expect("u"),
            v: Basis::new(2, &[3, 3], &[0.0, 1.0]).expect("v"),
            points: pts,
            weights: Vec::new(),
        };
        let target = s.eval(DVec2::new(0.3, 0.7));
        let uv = Surface::BSpline(s.clone()).invert(target);
        assert!((s.eval(uv) - target).length() < 1e-8);
    }

    #[test]
    fn curve_sampling_hits_the_ends() {
        let c = Curve::Circle { frame: frame(), radius: 2.0 };
        let pts = c.sample(0.5, 2.0, 0.01);
        assert!((pts[0] - c.eval(0.5)).length() < 1e-12);
        assert!((pts[pts.len() - 1] - c.eval(2.0)).length() < 1e-12);
        let poly = Curve::Polyline(vec![DVec3::ZERO, DVec3::X, DVec3::new(1.0, 1.0, 0.0)]);
        let back = poly.sample(1.5, 0.5, 0.01);
        assert_eq!(back.len(), 3);
        assert!((back[1] - DVec3::X).length() < 1e-12);
    }
}
