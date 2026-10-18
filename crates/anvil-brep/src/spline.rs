//! Non-uniform rational B-spline curves, for edges that no line, circle or ellipse
//! describes: where two cylinders cross, say, or curves read from other systems.
//!
//! A spline whose ends meet is closed, and its parameter then wraps around with a
//! period of its domain's length, the way an angle wraps around a circle.

use crate::{KernelError, Result, tol};
use glam::DVec3;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

/// Highest degree evaluated. STEP files rarely go past 9.
pub const MAX_DEGREE: usize = 15;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Spline {
    pub degree: usize,
    /// Full knot vector, `points.len() + degree + 1` values.
    pub knots: Vec<f64>,
    pub points: Vec<DVec3>,
    /// Empty when not rational.
    #[serde(default)]
    pub weights: Vec<f64>,
    /// True when the curve's ends meet, so its parameter wraps.
    pub closed: bool,
    /// Parameters and points spread along the curve, for closest-point searches.
    #[serde(skip)]
    table: OnceLock<Vec<(f64, DVec3)>>,
}

impl PartialEq for Spline {
    fn eq(&self, other: &Self) -> bool {
        self.degree == other.degree
            && self.knots == other.knots
            && self.points == other.points
            && self.weights == other.weights
            && self.closed == other.closed
    }
}

impl Spline {
    /// Checks the data and decides whether the curve is closed.
    pub fn new(degree: usize, knots: Vec<f64>, points: Vec<DVec3>, weights: Vec<f64>) -> Result<Spline> {
        let bad = |m: &str| Err(KernelError::Unsupported(format!("spline curve: {m}")));
        if degree == 0 || degree > MAX_DEGREE {
            return bad("degree out of range");
        }
        if points.len() < degree + 1 || knots.len() != points.len() + degree + 1 {
            return bad("knot and control point counts disagree");
        }
        if knots.iter().any(|k| !k.is_finite()) || knots.windows(2).any(|w| w[1] < w[0]) {
            return bad("knots are not increasing");
        }
        if points.iter().any(|p| !tol::is_finite3(*p)) {
            return Err(KernelError::NonFinite);
        }
        if !weights.is_empty()
            && (weights.len() != points.len() || weights.iter().any(|w| w.is_nan() || *w <= 0.0))
        {
            return bad("weights must be positive, one per control point");
        }
        let (lo, hi) = (knots[degree], knots[points.len()]);
        if hi - lo <= tol::DEGENERATE {
            return bad("empty parameter range");
        }
        let mut s = Spline { degree, knots, points, weights, closed: false, table: OnceLock::new() };
        let size = s.points.iter().map(|p| p.length()).fold(1.0, f64::max);
        s.closed = (s.eval_raw(lo) - s.eval_raw(hi)).length() < 1e-9 * size;
        Ok(s)
    }

    /// A cubic curve through `points` with the given unit `tangents`, each piece a
    /// Hermite cubic over its chord length. For a closed curve the first point is
    /// not repeated at the end.
    pub fn hermite(points: &[DVec3], tangents: &[DVec3], closed: bool) -> Result<Spline> {
        if points.len() < 2 || points.len() != tangents.len() {
            return Err(KernelError::Unsupported("spline curve: too few points".into()));
        }
        let mut pts: Vec<DVec3> = points.to_vec();
        let mut tans: Vec<DVec3> = tangents.to_vec();
        if closed {
            pts.push(points[0]);
            tans.push(tangents[0]);
        }
        let mut knots = vec![0.0; 4];
        let mut control = vec![pts[0]];
        let mut s = 0.0;
        for i in 0..pts.len() - 1 {
            let h = (pts[i + 1] - pts[i]).length();
            if h <= tol::DEGENERATE {
                return Err(KernelError::Unsupported("spline curve: repeated point".into()));
            }
            // The handle length that makes a circular arc almost exact; it tends to
            // h / 3 as the piece straightens.
            let phi = tans[i].angle_between(tans[i + 1]);
            let handle =
                if phi < 1e-6 { h / 3.0 } else { 2.0 / 3.0 * h * (phi / 4.0).tan() / (phi / 2.0).sin() };
            control.push(pts[i] + tans[i] * handle);
            control.push(pts[i + 1] - tans[i + 1] * handle);
            control.push(pts[i + 1]);
            s += h;
            let m = if i + 2 == pts.len() { 4 } else { 3 };
            knots.extend(std::iter::repeat_n(s, m));
        }
        let mut spline = Spline::new(3, knots, control, Vec::new())?;
        spline.closed = closed;
        Ok(spline)
    }

    pub fn domain(&self) -> (f64, f64) {
        (self.knots[self.degree], self.knots[self.points.len()])
    }

    pub fn period(&self) -> Option<f64> {
        let (lo, hi) = self.domain();
        self.closed.then_some(hi - lo)
    }

    /// Brings a parameter into the domain: wrapped on a closed curve, clamped otherwise.
    fn wrap(&self, t: f64) -> f64 {
        let (lo, hi) = self.domain();
        if self.closed { lo + (t - lo).rem_euclid(hi - lo) } else { t.clamp(lo, hi) }
    }

    pub fn eval(&self, t: f64) -> DVec3 {
        self.eval_raw(self.wrap(t))
    }

    fn eval_raw(&self, t: f64) -> DVec3 {
        let p = self.degree;
        let (span, basis) = basis(p, &self.knots, self.points.len(), t);
        let mut acc = DVec3::ZERO;
        let mut w = 0.0;
        for (j, bj) in basis.iter().take(p + 1).enumerate() {
            let i = span - p + j;
            let wi = self.weights.get(i).copied().unwrap_or(1.0);
            acc += self.points[i] * (bj * wi);
            w += bj * wi;
        }
        if w.abs() < 1e-300 { acc } else { acc / w }
    }

    /// The piece of the curve from `a` to `b` (within the domain, `a < b`) as a clamped
    /// spline of its own, exactly: each end is inserted as a knot until the curve passes
    /// through a control point there (Boehm's algorithm, on weighted points).
    pub fn piece(&self, a: f64, b: f64) -> Result<Spline> {
        let (lo, hi) = self.domain();
        let p = self.degree;
        if a.partial_cmp(&b) != Some(std::cmp::Ordering::Less) || a < lo || b > hi {
            return Err(KernelError::Unsupported("spline piece: range outside the curve".into()));
        }
        let mut knots = self.knots.clone();
        // Weighted points (w P, w).
        let mut pts: Vec<(DVec3, f64)> = self
            .points
            .iter()
            .enumerate()
            .map(|(i, q)| {
                let w = self.weights.get(i).copied().unwrap_or(1.0);
                (*q * w, w)
            })
            .collect();
        for x in [a, b] {
            let have = knots.iter().filter(|k| **k == x).count();
            for _ in have..p {
                let n = pts.len();
                let k = match knots[p..n].partition_point(|v| *v <= x) {
                    0 => p,
                    j => p + j - 1,
                };
                let mut next = Vec::with_capacity(n + 1);
                next.extend_from_slice(&pts[..=k - p]);
                for i in k - p + 1..=k {
                    let den = knots[i + p] - knots[i];
                    let al = if den.abs() < 1e-300 { 0.0 } else { (x - knots[i]) / den };
                    let (pi, wi) = pts[i];
                    let (pj, wj) = pts[i - 1];
                    next.push((pi * al + pj * (1.0 - al), wi * al + wj * (1.0 - al)));
                }
                next.extend_from_slice(&pts[k..]);
                pts = next;
                knots.insert(k + 1, x);
            }
        }
        // The control point the curve passes through at x.
        let n = pts.len();
        let at = |x: f64| -> usize {
            if x == lo {
                0
            } else if x == hi {
                n - 1
            } else {
                knots.iter().position(|k| *k == x).map_or(0, |i| i.saturating_sub(1))
            }
        };
        let (ia, ib) = (at(a), at(b));
        if ib <= ia {
            return Err(KernelError::Unsupported("spline piece: degenerate range".into()));
        }
        let mut piece_knots = vec![a; p + 1];
        piece_knots.extend(knots.iter().filter(|k| **k > a && **k < b));
        piece_knots.extend(std::iter::repeat_n(b, p + 1));
        let slice = &pts[ia..=ib];
        let rational = !self.weights.is_empty();
        let points = slice.iter().map(|(q, w)| *q / *w).collect();
        let weights = if rational { slice.iter().map(|(_, w)| *w).collect() } else { Vec::new() };
        Spline::new(p, piece_knots, points, weights)
    }

    /// The curve moved by a rigid motion given as a point map.
    pub fn mapped(&self, f: impl Fn(DVec3) -> DVec3) -> Spline {
        Spline { points: self.points.iter().map(|p| f(*p)).collect(), table: OnceLock::new(), ..self.clone() }
    }

    fn table(&self) -> &[(f64, DVec3)] {
        self.table.get_or_init(|| {
            let per = 2 * self.degree + 2;
            let mut out = Vec::new();
            let n = self.points.len();
            for i in self.degree..n {
                let (a, b) = (self.knots[i], self.knots[i + 1]);
                if b > a {
                    for k in 0..per {
                        let t = a + (b - a) * k as f64 / per as f64;
                        out.push((t, self.eval_raw(t)));
                    }
                }
            }
            let hi = self.knots[n];
            out.push((hi, self.eval_raw(hi)));
            out
        })
    }

    /// The parameter in `[lo, hi]` of the point closest to `p`.
    pub fn closest(&self, p: DVec3, lo: f64, hi: f64) -> f64 {
        let (lo, hi) = (lo.min(hi), lo.max(hi));
        let (dlo, dhi) = self.domain();
        let table = self.table();
        let step = (dhi - dlo) / table.len().max(1) as f64 * 2.0;
        // Candidates: the table, shifted by whole periods into the range, and the ends.
        let shifts: Vec<f64> = match self.period() {
            Some(per) => {
                let first = ((lo - dhi) / per).floor() as i64;
                let last = ((hi - dlo) / per).ceil() as i64;
                (first..=last.min(first + 8)).map(|k| per * k as f64).collect()
            }
            None => vec![0.0],
        };
        let mut best = (lo, (self.eval(lo) - p).length_squared());
        let d_hi = (self.eval(hi) - p).length_squared();
        if d_hi < best.1 {
            best = (hi, d_hi);
        }
        for sh in &shifts {
            for (t, q) in table {
                let t = t + sh;
                if t < lo || t > hi {
                    continue;
                }
                let d = (*q - p).length_squared();
                if d < best.1 {
                    best = (t, d);
                }
            }
        }
        // Golden-section refinement around the best candidate, reaching at least to the
        // samples either side of it (spans differ in length, so the table is not evenly
        // spread and an average step can fall short of them).
        let samples = || shifts.iter().flat_map(|sh| table.iter().map(move |(t, _)| t + sh));
        let below = samples().filter(|t| *t < best.0).fold(f64::MIN, f64::max);
        let above = samples().filter(|t| *t > best.0).fold(f64::MAX, f64::min);
        let left = if below > f64::MIN { step.max(best.0 - below) } else { step };
        let right = if above < f64::MAX { step.max(above - best.0) } else { step };
        let (mut a, mut b) = ((best.0 - left).max(lo), (best.0 + right).min(hi));
        let f = |t: f64| (self.eval(t) - p).length_squared();
        for _ in 0..80 {
            let m1 = a + (b - a) * 0.381_966;
            let m2 = a + (b - a) * 0.618_034;
            if f(m1) < f(m2) {
                b = m2;
            } else {
                a = m1;
            }
        }
        let t = 0.5 * (a + b);
        if f(t) < best.1 { t } else { best.0 }
    }

    /// Points from `t0` to `t1` (in that order) whose chords stay within `chord` of
    /// the curve and turn by at most `max_angle`.
    pub fn sample(&self, t0: f64, t1: f64, chord: f64, max_angle: f64) -> Vec<DVec3> {
        let (lo, hi) = (t0.min(t1), t0.max(t1));
        let mut ts = vec![lo];
        let (dlo, dhi) = self.domain();
        let period = self.period();
        for (t, _) in self.table() {
            let shifts: Vec<f64> = match period {
                Some(per) => {
                    let first = ((lo - dhi) / per).floor() as i64;
                    let last = ((hi - dlo) / per).ceil() as i64;
                    (first..=last.min(first + 8)).map(|k| per * k as f64).collect()
                }
                None => vec![0.0],
            };
            for sh in shifts {
                let t = t + sh;
                if t > lo + 1e-12 && t < hi - 1e-12 {
                    ts.push(t);
                }
            }
        }
        ts.push(hi);
        ts.sort_by(f64::total_cmp);
        ts.dedup_by(|a, b| (*a - *b).abs() < 1e-12);
        let chord = if chord > 0.0 { chord } else { f64::INFINITY };
        let mut out = vec![self.eval(ts[0])];
        for w in ts.windows(2) {
            self.refine(w[0], w[1], chord, max_angle.max(1e-3), 0, &mut out);
        }
        if t0 > t1 {
            out.reverse();
        }
        out
    }

    fn refine(&self, a: f64, b: f64, chord: f64, max_angle: f64, depth: u32, out: &mut Vec<DVec3>) {
        let (pa, pb) = (self.eval(a), self.eval(b));
        let m = 0.5 * (a + b);
        let pm = self.eval(m);
        let deviation = (pm - 0.5 * (pa + pb)).length();
        let turn = (pm - pa).angle_between(pb - pm);
        if depth < 10 && (deviation > chord || turn > max_angle) {
            self.refine(a, m, chord, max_angle, depth + 1, out);
            self.refine(m, b, chord, max_angle, depth + 1, out);
        } else {
            out.push(pb);
        }
    }
}

/// The knot span holding `t` and the `degree + 1` basis functions not zero there, for
/// a B-spline with `n` control points (Cox-de Boor). `t` is clamped to the domain.
pub(crate) fn basis(degree: usize, knots: &[f64], n: usize, t: f64) -> (usize, [f64; MAX_DEGREE + 1]) {
    basis_free(degree, knots, n, t.clamp(knots[degree], knots[n]))
}

/// Like [`basis`], but past the domain's ends the end spans' polynomials carry on (the
/// spline extrapolated smoothly).
pub(crate) fn basis_free(degree: usize, knots: &[f64], n: usize, t: f64) -> (usize, [f64; MAX_DEGREE + 1]) {
    let p = degree;
    // Span: the last i in [p, n - 1] with knots[i] <= t < knots[i + 1].
    let mut span = match knots[p..n].partition_point(|k| *k <= t) {
        0 => p,
        k => p + k - 1,
    };
    while span > p && knots[span] >= knots[span + 1] {
        span -= 1;
    }
    let mut left = [0.0; MAX_DEGREE + 1];
    let mut right = [0.0; MAX_DEGREE + 1];
    let mut out = [0.0; MAX_DEGREE + 1];
    out[0] = 1.0;
    for j in 1..=p {
        left[j] = t - knots[span + 1 - j];
        right[j] = knots[span + j] - t;
        let mut saved = 0.0;
        for r in 0..j {
            let den = right[r + 1] + left[j - r];
            let tmp = if den.abs() < 1e-300 { 0.0 } else { out[r] / den };
            out[r] = saved + right[r + 1] * tmp;
            saved = left[j - r] * tmp;
        }
        out[j] = saved;
    }
    (span, out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closest_point_on_a_curve_with_uneven_spans() {
        // One long straight span, then many short ones: the closest point on the long span
        // is found exactly, though the average sample spacing is much shorter than its own.
        let mut pts = vec![DVec3::new(-10.0, 0.0, 0.0)];
        pts.extend((0..200).map(|k| DVec3::new(k as f64 * 0.01, 0.0, 0.0)));
        let tans = vec![DVec3::X; pts.len()];
        let s = Spline::hermite(&pts, &tans, false).expect("spline");
        let (lo, hi) = s.domain();
        for x in [-9.3, -7.7, -2.6] {
            let p = DVec3::new(x, 1.0, 0.0);
            let q = s.eval(s.closest(p, lo, hi));
            assert!((q - DVec3::new(x, 0.0, 0.0)).length() < 1e-6, "{x}: {q}");
        }
    }

    #[test]
    fn hermite_quarter_circles_follow_the_circle() {
        let n = 16;
        let (pts, tans): (Vec<DVec3>, Vec<DVec3>) = (0..n)
            .map(|i| {
                let a = std::f64::consts::TAU * i as f64 / n as f64;
                (DVec3::new(a.cos(), a.sin(), 0.0) * 5.0, DVec3::new(-a.sin(), a.cos(), 0.0))
            })
            .unzip();
        let s = Spline::hermite(&pts, &tans, true).expect("spline");
        assert!(s.closed);
        let (lo, hi) = s.domain();
        for i in 0..200 {
            let p = s.eval(lo + (hi - lo) * i as f64 / 200.0);
            assert!((p.length() - 5.0).abs() < 1e-4, "{}", p.length());
        }
        // Parameters wrap, and closest points come back.
        assert!((s.eval(lo - 0.3) - s.eval(hi - 0.3)).length() < 1e-12);
        let q = DVec3::new(0.0, 7.0, 1.0);
        let t = s.closest(q, lo, hi);
        assert!((s.eval(t) - DVec3::new(0.0, 5.0, 0.0)).length() < 1e-3);
        let pts = s.sample(lo, hi, 0.01, 0.2);
        assert!(pts.len() > 30);
        assert!((pts[0] - pts[pts.len() - 1]).length() < 1e-12);
    }

    #[test]
    fn pieces_follow_the_curve_exactly() {
        let w = std::f64::consts::FRAC_1_SQRT_2;
        let arc = Spline::new(
            2,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 2.0, 2.0, 2.0],
            vec![
                DVec3::new(2.0, 0.0, 0.0),
                DVec3::new(2.0, 2.0, 0.0),
                DVec3::new(0.0, 2.0, 0.0),
                DVec3::new(-2.0, 2.0, 0.0),
                DVec3::new(-2.0, 0.0, 0.0),
            ],
            vec![1.0, w, 1.0, w, 1.0],
        )
        .expect("half circle");
        let piece = arc.piece(0.3, 1.6).expect("piece");
        assert_eq!(piece.domain(), (0.3, 1.6));
        assert!((piece.points[0] - arc.eval(0.3)).length() < 1e-12);
        assert!((piece.points[piece.points.len() - 1] - arc.eval(1.6)).length() < 1e-12);
        for i in 0..=20 {
            let t = 0.3 + 1.3 * i as f64 / 20.0;
            assert!((piece.eval(t) - arc.eval(t)).length() < 1e-12, "{t}");
        }
    }

    #[test]
    fn rational_quadratic_is_an_exact_arc() {
        // A quarter circle of radius 2 as a rational quadratic.
        let w = std::f64::consts::FRAC_1_SQRT_2;
        let s = Spline::new(
            2,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            vec![DVec3::new(2.0, 0.0, 0.0), DVec3::new(2.0, 2.0, 0.0), DVec3::new(0.0, 2.0, 0.0)],
            vec![1.0, w, 1.0],
        )
        .expect("spline");
        assert!(!s.closed);
        for i in 0..=20 {
            assert!((s.eval(i as f64 / 20.0).length() - 2.0).abs() < 1e-12);
        }
        assert!(Spline::new(2, vec![0.0; 5], vec![DVec3::ZERO; 3], vec![]).is_err());
    }
}
