//! Plane geometry of sketch ellipses and partial ellipses.
//!
//! A sketch ellipse is held by three points: its centre and the ends of its two semi-axes
//! (see `docs/re/sketch-entities.md`, "Ellipses"). [`Ellipse2`] is the curve those points
//! describe, in the kernel's convention: the point at parameter `t` is
//! `c + u r1 cos t + v r2 sin t`, with `u` the unit vector towards the first axis end and
//! `v` that turned +90°.

use crate::sketch::{arc_sweep, EntityId, Geom, Sketch};
use anvil_kernel::profile::{ellipse_derivative, ellipse_point};
use anvil_kernel::{v2, Vec2};
use std::f64::consts::TAU;

/// An ellipse in sketch coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ellipse2 {
    pub c: Vec2,
    /// Semi-axis along `u`, the direction at angle `rotation`.
    pub r1: f64,
    /// Semi-axis along `u` turned +90°.
    pub r2: f64,
    pub rotation: f64,
}

impl Ellipse2 {
    /// The ellipse with centre `c` and semi-axis ends `axis1` and `axis2`. The second
    /// radius is `axis2`'s distance from the line of the first axis, so a point that is
    /// not quite square to it still gives the right size.
    pub fn from_points(c: Vec2, axis1: Vec2, axis2: Vec2) -> Ellipse2 {
        let d = axis1 - c;
        let r1 = d.length();
        let u = if r1 > 1e-12 { d / r1 } else { v2(1.0, 0.0) };
        Ellipse2 { c, r1, r2: u.cross(axis2 - c).abs(), rotation: u.angle() }
    }

    /// Unit vectors along the first and second axes.
    pub fn axes(&self) -> (Vec2, Vec2) {
        let u = Vec2::from_angle(self.rotation);
        (u, u.perp())
    }

    /// `p` in the ellipse's own axes.
    pub fn local(&self, p: Vec2) -> Vec2 {
        let (u, v) = self.axes();
        let d = p - self.c;
        v2(d.dot(u), d.dot(v))
    }

    pub fn at(&self, t: f64) -> Vec2 {
        ellipse_point(self.c, self.r1, self.r2, self.rotation, t)
    }

    /// The derivative of [`Ellipse2::at`] with respect to `t`.
    pub fn derivative(&self, t: f64) -> Vec2 {
        ellipse_derivative(self.r1, self.r2, self.rotation, t)
    }

    /// The parameter of the point of the ellipse on the same ray from the centre as `p`,
    /// scaled to a circle (the eccentric anomaly): exact for points on the curve. In
    /// `[0, 2π)`.
    pub fn param(&self, p: Vec2) -> f64 {
        let q = self.local(p);
        (q.y / self.r2.max(1e-300)).atan2(q.x / self.r1.max(1e-300)).rem_euclid(TAU)
    }

    /// The point of the ellipse in the direction of `p` from the centre.
    pub fn project_radially(&self, p: Vec2) -> Vec2 {
        let q = self.local(p);
        let s = ((q.x / self.r1).powi(2) + (q.y / self.r2).powi(2)).sqrt();
        if s < 1e-12 || !s.is_finite() {
            return self.at(0.0);
        }
        self.c + (p - self.c) / s
    }

    /// Zero on the curve, about the distance from it near it, and exactly the radial
    /// distance for a circle: `(√((x/r1)² + (y/r2)²) − 1)·√(r1 r2)`.
    pub fn residual(&self, p: Vec2) -> f64 {
        let q = self.local(p);
        let (r1, r2) = (self.r1.max(1e-12), self.r2.max(1e-12));
        (((q.x / r1).powi(2) + (q.y / r2).powi(2)).sqrt() - 1.0) * (r1 * r2).sqrt()
    }

    /// The outward normal (not unit) at a point on the curve: the gradient of the implicit
    /// form there.
    pub fn normal_at(&self, p: Vec2) -> Vec2 {
        let q = self.local(p);
        let (u, v) = self.axes();
        let (r1, r2) = (self.r1.max(1e-12), self.r2.max(1e-12));
        u * (q.x / (r1 * r1)) + v * (q.y / (r2 * r2))
    }

    /// The distance from the centre to the tangent line square to the unit vector `n`.
    pub fn support(&self, n: Vec2) -> f64 {
        let (u, v) = self.axes();
        ((self.r1 * n.dot(u)).powi(2) + (self.r2 * n.dot(v)).powi(2)).sqrt()
    }

    /// The parameter in `[t0, t0 + sweep]` of the curve point nearest `p`: a coarse scan
    /// refined by Newton's method on the squared distance.
    pub fn nearest_param(&self, p: Vec2, t0: f64, sweep: f64) -> f64 {
        let n = 64;
        let mut best = (f64::INFINITY, t0);
        for i in 0..=n {
            let t = t0 + sweep * i as f64 / n as f64;
            let d = self.at(t).distance(p);
            if d < best.0 {
                best = (d, t);
            }
        }
        let mut t = best.1;
        for _ in 0..16 {
            let (q, d1) = (self.at(t), self.derivative(t));
            let d2 = self.c - q; // second derivative: -(q - c)
            let g = (q - p).dot(d1);
            let h = d1.dot(d1) + (q - p).dot(d2);
            if h.abs() < 1e-300 {
                break;
            }
            let step = g / h;
            t = (t - step).clamp(t0, t0 + sweep);
            if step.abs() < 1e-14 {
                break;
            }
        }
        t
    }

    /// The distance from `p` to the part of the curve from `t0` sweeping `sweep`.
    pub fn distance(&self, p: Vec2, t0: f64, sweep: f64) -> f64 {
        self.at(self.nearest_param(p, t0, sweep)).distance(p)
    }

    /// Points along the part from `t0` sweeping `sweep`, ends included, with chords
    /// within `tol` of the curve.
    pub fn polyline(&self, t0: f64, sweep: f64, tol: f64) -> Vec<Vec2> {
        let n = anvil_kernel::profile::ellipse_segments(self.r1, self.r2, sweep, tol);
        (0..=n).map(|i| self.at(t0 + sweep * i as f64 / n as f64)).collect()
    }
}

/// The ellipse and the parameter range an ellipse entity covers: `(ellipse, start, sweep)`
/// with a whole ellipse from 0 sweeping 2π and a partial one counter-clockwise from its
/// start point to its end point.
pub fn of(sk: &Sketch, e: EntityId) -> Option<(Ellipse2, f64, f64)> {
    match sk.entities.get(e)?.geom {
        Geom::Ellipse { c, axis1, axis2 } => {
            Some((Ellipse2::from_points(sk.pos(c), sk.pos(axis1), sk.pos(axis2)), 0.0, TAU))
        }
        Geom::EllipseArc { c, axis1, axis2, a, b } => {
            let el = Ellipse2::from_points(sk.pos(c), sk.pos(axis1), sk.pos(axis2));
            let ta = el.param(sk.pos(a));
            Some((el, ta, arc_sweep(ta, el.param(sk.pos(b)))))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::{FRAC_PI_2, PI};

    #[test]
    fn points_params_and_residuals() {
        let el = Ellipse2::from_points(v2(1.0, 2.0), v2(31.0, 2.0), v2(1.0, 22.0));
        assert!((el.r1 - 30.0).abs() < 1e-12 && (el.r2 - 20.0).abs() < 1e-12);
        assert!(el.at(FRAC_PI_2).distance(v2(1.0, 22.0)) < 1e-12);
        assert!((el.param(v2(-29.0, 2.0)) - PI).abs() < 1e-12);
        assert!(el.residual(el.at(0.7)).abs() < 1e-12);
        assert!((el.residual(v2(1.0, 27.0)) - 0.25 * 600f64.sqrt()).abs() < 1e-12);
        // The support distance square to the second axis is the second radius.
        assert!((el.support(v2(0.0, 1.0)) - 20.0).abs() < 1e-12);
        // A point outside, nearest a quadrant point.
        assert!((el.distance(v2(1.0, 30.0), 0.0, TAU) - 8.0).abs() < 1e-9);
        assert!(el.project_radially(v2(1.0, 100.0)).distance(v2(1.0, 22.0)) < 1e-12);
    }

    #[test]
    fn second_point_off_square_still_gives_its_radius() {
        let el = Ellipse2::from_points(v2(0.0, 0.0), v2(0.0, 10.0), v2(-5.0, 3.0));
        assert!((el.r2 - 5.0).abs() < 1e-12);
        assert!((el.rotation - FRAC_PI_2).abs() < 1e-12);
    }
}
