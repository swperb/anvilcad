//! Plane geometry of sketch curves: intersections, projections and parameters.
//!
//! The editing tools (trim, extend, fillet, offset) work on the curves an entity
//! describes rather than on its points, so they share these helpers.

use crate::sketch::{angle_in_ccw_range, arc_sweep, EntityId, Geom, Sketch};
use anvil_kernel::{v2, Vec2};
use std::f64::consts::TAU;

const EPS: f64 = 1e-9;

/// The curve an entity describes, in sketch coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Curve2 {
    /// The segment from `a` to `b`.
    Line { a: Vec2, b: Vec2 },
    /// The full circle.
    Circle { c: Vec2, r: f64 },
    /// The counter-clockwise arc from angle `start` sweeping `sweep` (0, 2π] radians.
    Arc { c: Vec2, r: f64, start: f64, sweep: f64 },
}

impl Curve2 {
    pub fn of(sk: &Sketch, e: EntityId) -> Option<Curve2> {
        Some(match sk.entities.get(e)?.geom {
            Geom::Line { a, b } => Curve2::Line { a: sk.pos(a), b: sk.pos(b) },
            Geom::Circle { c, r } => Curve2::Circle { c: sk.pos(c), r },
            Geom::Arc { c, a, b } => {
                let (cc, pa, pb) = (sk.pos(c), sk.pos(a), sk.pos(b));
                let start = (pa - cc).angle();
                Curve2::Arc { c: cc, r: pa.distance(cc), start, sweep: arc_sweep(start, (pb - cc).angle()) }
            }
        })
    }

    /// Parameter of the point on the curve nearest `p`: a fraction of the way along a
    /// line (unclamped), an angle for a circle, and the swept angle from the start for an
    /// arc (unclamped, in `[-π, 2π)` so points just before the start come out negative).
    pub fn param(&self, p: Vec2) -> f64 {
        match *self {
            Curve2::Line { a, b } => {
                let d = b - a;
                let l2 = d.dot(d);
                if l2 < EPS * EPS {
                    0.0
                } else {
                    (p - a).dot(d) / l2
                }
            }
            Curve2::Circle { c, .. } => (p - c).angle().rem_euclid(TAU),
            Curve2::Arc { c, start, sweep, .. } => {
                let t = ((p - c).angle() - start).rem_euclid(TAU);
                // Past the end, a point is nearer the start if it is within the gap's
                // second half.
                if t > sweep && t - sweep > 0.5 * (TAU - sweep) {
                    t - TAU
                } else {
                    t
                }
            }
        }
    }

    /// The point at parameter `t` (see [`Curve2::param`]).
    pub fn at(&self, t: f64) -> Vec2 {
        match *self {
            Curve2::Line { a, b } => a + (b - a) * t,
            Curve2::Circle { c, r } => c + Vec2::from_angle(t) * r,
            Curve2::Arc { c, r, start, .. } => c + Vec2::from_angle(start + t) * r,
        }
    }

    /// The parameter range the entity covers.
    pub fn range(&self) -> (f64, f64) {
        match *self {
            Curve2::Line { .. } => (0.0, 1.0),
            Curve2::Circle { .. } => (0.0, TAU),
            Curve2::Arc { sweep, .. } => (0.0, sweep),
        }
    }

    /// True when the point (assumed on the underlying line or circle) lies on the entity.
    pub fn contains(&self, p: Vec2, tol: f64) -> bool {
        match *self {
            Curve2::Line { a, b } => {
                let t = self.param(p);
                let len = a.distance(b).max(EPS);
                t >= -tol / len && t <= 1.0 + tol / len
            }
            Curve2::Circle { .. } => true,
            Curve2::Arc { c, r, start, sweep } => {
                let slack = tol / r.max(EPS);
                angle_in_ccw_range((p - c).angle(), start - slack, start + sweep + slack)
            }
        }
    }

    /// Unit tangent at parameter `t`, in the direction of increasing parameter.
    pub fn tangent(&self, t: f64) -> Vec2 {
        match *self {
            Curve2::Line { a, b } => (b - a).normalized(),
            Curve2::Circle { .. } => Vec2::from_angle(t).perp(),
            Curve2::Arc { start, .. } => Vec2::from_angle(start + t).perp(),
        }
    }
}

/// Points where the underlying (infinite) line or full circle of `a` meets that of `b`.
pub fn intersect_unbounded(a: &Curve2, b: &Curve2) -> Vec<Vec2> {
    match (support(a), support(b)) {
        (Support::Line(p, d), Support::Line(q, e)) => {
            let den = d.cross(e);
            if den.abs() < EPS * d.length() * e.length() {
                return Vec::new();
            }
            let t = (q - p).cross(e) / den;
            vec![p + d * t]
        }
        (Support::Line(p, d), Support::Circle(c, r)) | (Support::Circle(c, r), Support::Line(p, d)) => {
            line_circle(p, d, c, r)
        }
        (Support::Circle(c1, r1), Support::Circle(c2, r2)) => circle_circle(c1, r1, c2, r2),
    }
}

/// Points where entity `a` meets entity `b`, within both entities' extents.
pub fn intersect(a: &Curve2, b: &Curve2, tol: f64) -> Vec<Vec2> {
    intersect_unbounded(a, b).into_iter().filter(|p| a.contains(*p, tol) && b.contains(*p, tol)).collect()
}

enum Support {
    Line(Vec2, Vec2),
    Circle(Vec2, f64),
}

fn support(c: &Curve2) -> Support {
    match *c {
        Curve2::Line { a, b } => Support::Line(a, b - a),
        Curve2::Circle { c, r } | Curve2::Arc { c, r, .. } => Support::Circle(c, r),
    }
}

fn line_circle(p: Vec2, d: Vec2, c: Vec2, r: f64) -> Vec<Vec2> {
    let len = d.length();
    if len < EPS {
        return Vec::new();
    }
    let u = d / len;
    let foot = p + u * (c - p).dot(u);
    let h2 = r * r - foot.distance(c).powi(2);
    if h2 < -EPS * r.max(1.0) {
        Vec::new()
    } else if h2 <= EPS * r.max(1.0) {
        vec![foot]
    } else {
        let h = h2.sqrt();
        vec![foot - u * h, foot + u * h]
    }
}

fn circle_circle(c1: Vec2, r1: f64, c2: Vec2, r2: f64) -> Vec<Vec2> {
    let d = c1.distance(c2);
    if d < EPS || d > r1 + r2 + EPS || d < (r1 - r2).abs() - EPS {
        return Vec::new();
    }
    let a = (r1 * r1 - r2 * r2 + d * d) / (2.0 * d);
    let h2 = r1 * r1 - a * a;
    let u = (c2 - c1) / d;
    let m = c1 + u * a;
    if h2 <= EPS * r1.max(1.0) {
        return vec![m];
    }
    let h = h2.max(0.0).sqrt();
    vec![m + u.perp() * h, m - u.perp() * h]
}

/// Point on the entity nearest `p`.
pub fn nearest_on(c: &Curve2, p: Vec2) -> Vec2 {
    match *c {
        Curve2::Line { .. } => c.at(c.param(p).clamp(0.0, 1.0)),
        Curve2::Circle { c: o, r } => o + (p - o).normalized() * r,
        Curve2::Arc { .. } => {
            let (lo, hi) = c.range();
            c.at(c.param(p).clamp(lo, hi))
        }
    }
}

/// The circle tangent to two lines meeting at a corner, of radius `r`, on the side
/// the corner opens to. `a` and `b` are the far ends of the two lines and `corner` is
/// the shared point. Returns the centre and the two tangent points (on `a`'s line
/// first), or `None` when the lines are parallel or too short.
pub fn corner_fillet(a: Vec2, corner: Vec2, b: Vec2, r: f64) -> Option<(Vec2, Vec2, Vec2)> {
    let (u, w) = ((a - corner).normalized(), (b - corner).normalized());
    let cos = u.dot(w).clamp(-1.0, 1.0);
    let half = 0.5 * cos.acos();
    if half < 1e-6 || (std::f64::consts::FRAC_PI_2 - half) < 1e-9 {
        return None;
    }
    let setback = r / half.tan();
    if setback > a.distance(corner) + EPS || setback > b.distance(corner) + EPS {
        return None;
    }
    let bis = (u + w).normalized();
    let center = corner + bis * (r / half.sin());
    Some((center, corner + u * setback, corner + w * setback))
}

/// Twice the signed area of triangle (a, b, c); positive when counter-clockwise.
pub fn orient(a: Vec2, b: Vec2, c: Vec2) -> f64 {
    (b - a).cross(c - a)
}

/// Rotates `p` about `c` by `angle` radians.
pub fn rotate_about(p: Vec2, c: Vec2, angle: f64) -> Vec2 {
    let (s, co) = angle.sin_cos();
    let d = p - c;
    c + v2(d.x * co - d.y * s, d.x * s + d.y * co)
}

/// Reflects `p` in the line through `a` and `b`.
pub fn reflect(p: Vec2, a: Vec2, b: Vec2) -> Vec2 {
    let d = (b - a).normalized();
    let q = p - a;
    a + d * (2.0 * q.dot(d)) - q
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::{FRAC_PI_2, PI};

    fn close(a: Vec2, b: Vec2) -> bool {
        a.distance(b) < 1e-9
    }

    #[test]
    fn line_line() {
        let a = Curve2::Line { a: v2(0.0, 0.0), b: v2(10.0, 0.0) };
        let b = Curve2::Line { a: v2(3.0, -2.0), b: v2(3.0, 5.0) };
        assert_eq!(intersect(&a, &b, 1e-9).len(), 1);
        assert!(close(intersect(&a, &b, 1e-9)[0], v2(3.0, 0.0)));
        let c = Curve2::Line { a: v2(12.0, -2.0), b: v2(12.0, 5.0) };
        assert!(intersect(&a, &c, 1e-9).is_empty());
        assert_eq!(intersect_unbounded(&a, &c).len(), 1);
    }

    #[test]
    fn line_circle_and_arc() {
        let l = Curve2::Line { a: v2(-10.0, 0.0), b: v2(10.0, 0.0) };
        let c = Curve2::Circle { c: v2(0.0, 0.0), r: 5.0 };
        let mut pts = intersect(&l, &c, 1e-9);
        pts.sort_by(|p, q| p.x.total_cmp(&q.x));
        assert!(close(pts[0], v2(-5.0, 0.0)) && close(pts[1], v2(5.0, 0.0)));
        // Upper half arc meets the x axis only at its ends; a lower arc from -170 to -10
        // degrees misses it.
        let upper = Curve2::Arc { c: v2(0.0, 0.0), r: 5.0, start: 0.0, sweep: PI };
        assert_eq!(intersect(&l, &upper, 1e-9).len(), 2);
        let lower =
            Curve2::Arc { c: v2(0.0, 0.0), r: 5.0, start: -170f64.to_radians(), sweep: 160f64.to_radians() };
        assert!(intersect(&l, &lower, 1e-9).is_empty());
    }

    #[test]
    fn circle_circle_points() {
        let a = Curve2::Circle { c: v2(0.0, 0.0), r: 5.0 };
        let b = Curve2::Circle { c: v2(8.0, 0.0), r: 5.0 };
        let pts = intersect(&a, &b, 1e-9);
        assert_eq!(pts.len(), 2);
        for p in pts {
            assert!((p.x - 4.0).abs() < 1e-9 && (p.y.abs() - 3.0).abs() < 1e-9);
        }
        let touching = Curve2::Circle { c: v2(10.0, 0.0), r: 5.0 };
        assert_eq!(intersect(&a, &touching, 1e-9).len(), 1);
    }

    #[test]
    fn arc_params_wrap_sensibly() {
        let arc = Curve2::Arc { c: v2(0.0, 0.0), r: 1.0, start: 0.0, sweep: FRAC_PI_2 };
        assert!((arc.param(v2(0.0, 1.0)) - FRAC_PI_2).abs() < 1e-12);
        // Just clockwise of the start comes out negative.
        assert!(arc.param(Vec2::from_angle(-0.1)) < 0.0);
        assert!(arc.param(Vec2::from_angle(FRAC_PI_2 + 0.1)) > FRAC_PI_2);
    }

    #[test]
    fn fillet_of_a_right_angle() {
        let (c, ta, tb) = corner_fillet(v2(40.0, 0.0), v2(0.0, 0.0), v2(0.0, 20.0), 5.0).unwrap();
        assert!(close(c, v2(5.0, 5.0)) && close(ta, v2(5.0, 0.0)) && close(tb, v2(0.0, 5.0)));
        assert!(corner_fillet(v2(4.0, 0.0), v2(0.0, 0.0), v2(0.0, 20.0), 5.0).is_none());
    }

    #[test]
    fn reflect_and_rotate() {
        assert!(close(reflect(v2(3.0, 1.0), v2(0.0, 0.0), v2(0.0, 5.0)), v2(-3.0, 1.0)));
        assert!(close(rotate_about(v2(2.0, 0.0), v2(1.0, 0.0), FRAC_PI_2), v2(1.0, 1.0)));
    }
}
