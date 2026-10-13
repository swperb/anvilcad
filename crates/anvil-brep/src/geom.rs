//! Exact curve and surface geometry.

use crate::spline::Spline;
use crate::{KernelError, Result, tol};
use glam::{DVec2, DVec3};
use serde::{Deserialize, Serialize};
use std::f64::consts::TAU;
use std::sync::Arc;

/// A right-handed orthonormal coordinate frame.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Frame {
    pub origin: DVec3,
    pub x: DVec3,
    pub y: DVec3,
    pub z: DVec3,
}

impl Frame {
    /// Builds a frame from an origin, a normal (z) and a direction for x.
    /// `x_hint` is projected into the plane normal to `normal`.
    pub fn new(origin: DVec3, normal: DVec3, x_hint: DVec3) -> Result<Self> {
        if !tol::is_finite3(origin) || !tol::is_finite3(normal) || !tol::is_finite3(x_hint) {
            return Err(KernelError::NonFinite);
        }
        let z = normal.try_normalize().ok_or(KernelError::InvalidFrame)?;
        let x = (x_hint - z * x_hint.dot(z)).try_normalize().ok_or(KernelError::InvalidFrame)?;
        let y = z.cross(x);
        Ok(Self { origin, x, y, z })
    }

    pub const fn world() -> Self {
        Self { origin: DVec3::ZERO, x: DVec3::X, y: DVec3::Y, z: DVec3::Z }
    }

    pub fn to_world(&self, p: DVec2) -> DVec3 {
        self.origin + self.x * p.x + self.y * p.y
    }

    pub fn to_world3(&self, p: DVec3) -> DVec3 {
        self.origin + self.x * p.x + self.y * p.y + self.z * p.z
    }

    pub fn to_local(&self, p: DVec3) -> DVec3 {
        let d = p - self.origin;
        DVec3::new(d.dot(self.x), d.dot(self.y), d.dot(self.z))
    }

    /// Maps a point given in this frame's coordinates to world coordinates; used
    /// as a rigid motion when the frame is orthonormal.
    pub fn apply_point(&self, p: DVec3) -> DVec3 {
        self.to_world3(p)
    }

    /// Rotates a direction by this frame's axes.
    pub fn apply_dir(&self, d: DVec3) -> DVec3 {
        self.x * d.x + self.y * d.y + self.z * d.z
    }

    /// This frame moved by the rigid motion `m`.
    pub fn moved(&self, m: &Frame) -> Frame {
        Frame {
            origin: m.apply_point(self.origin),
            x: m.apply_dir(self.x),
            y: m.apply_dir(self.y),
            z: m.apply_dir(self.z),
        }
    }

    /// The same frame moved `dz` along its normal.
    pub fn offset(&self, dz: f64) -> Self {
        Self { origin: self.origin + self.z * dz, ..*self }
    }

    /// The same frame with its origin moved to the world point `origin`.
    pub fn with_origin(&self, origin: DVec3) -> Self {
        Self { origin, ..*self }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Curve {
    /// `origin + dir * t`, `dir` unit length.
    Line { origin: DVec3, dir: DVec3 },
    /// `frame.origin + radius * (cos t * frame.x + sin t * frame.y)`.
    Circle { frame: Frame, radius: f64 },
    /// `frame.origin + a cos t * frame.x + b sin t * frame.y`.
    Ellipse { frame: Frame, a: f64, b: f64 },
    /// A B-spline, for intersection curves and curves read from other systems.
    Spline(Arc<Spline>),
}

impl Curve {
    pub fn eval(&self, t: f64) -> DVec3 {
        match self {
            Curve::Line { origin, dir } => *origin + *dir * t,
            Curve::Circle { frame, radius } => {
                frame.origin + (frame.x * t.cos() + frame.y * t.sin()) * *radius
            }
            Curve::Ellipse { frame, a, b } => {
                frame.origin + frame.x * (a * t.cos()) + frame.y * (b * t.sin())
            }
            Curve::Spline(s) => s.eval(t),
        }
    }

    /// The curve moved by the rigid motion `m`.
    pub fn moved(&self, m: &Frame) -> Curve {
        match *self {
            Curve::Spline(ref s) => Curve::Spline(Arc::new(s.mapped(|p| m.apply_point(p)))),
            Curve::Line { origin, dir } => {
                Curve::Line { origin: m.apply_point(origin), dir: m.apply_dir(dir) }
            }
            Curve::Circle { frame, radius } => Curve::Circle { frame: frame.moved(m), radius },
            Curve::Ellipse { frame, a, b } => Curve::Ellipse { frame: frame.moved(m), a, b },
        }
    }

    /// Circles, ellipses and closed splines are periodic in their parameter.
    pub fn is_periodic(&self) -> bool {
        self.period().is_some()
    }

    /// How far the parameter goes before the curve repeats, if it does.
    pub fn period(&self) -> Option<f64> {
        match self {
            Curve::Line { .. } => None,
            Curve::Circle { .. } | Curve::Ellipse { .. } => Some(TAU),
            Curve::Spline(s) => s.period(),
        }
    }

    /// Largest radius of curvature scale, for choosing sample counts.
    fn size(&self) -> f64 {
        match self {
            Curve::Line { .. } | Curve::Spline(_) => 0.0,
            Curve::Circle { radius, .. } => *radius,
            Curve::Ellipse { a, b, .. } => a.max(*b),
        }
    }

    /// Samples the curve on `[t0, t1]` (inclusive). Lines give two points.
    pub fn sample(&self, t0: f64, t1: f64, chord: f64, max_angle: f64) -> Vec<DVec3> {
        match self {
            Curve::Line { .. } => vec![self.eval(t0), self.eval(t1)],
            Curve::Spline(s) => s.sample(t0, t1, chord, max_angle),
            _ => {
                let n = arc_steps(self.size(), (t1 - t0).abs(), chord, max_angle);
                (0..=n)
                    .map(
                        |i| {
                            if i == n {
                                self.eval(t1)
                            } else {
                                self.eval(t0 + (t1 - t0) * i as f64 / n as f64)
                            }
                        },
                    )
                    .collect()
            }
        }
    }

    /// The parameter of a point on (or near) the curve. Circles and ellipses
    /// return a value in `(-PI, PI]`, splines one in their domain.
    pub fn param_of(&self, p: DVec3) -> f64 {
        match self {
            Curve::Line { origin, dir } => (p - *origin).dot(*dir),
            Curve::Circle { frame, .. } => {
                let l = frame.to_local(p);
                l.y.atan2(l.x)
            }
            Curve::Ellipse { frame, a, b } => {
                let l = frame.to_local(p);
                (l.y / b).atan2(l.x / a)
            }
            Curve::Spline(s) => {
                let (lo, hi) = s.domain();
                s.closest(p, lo, hi)
            }
        }
    }

    /// The parameter of `p`, shifted by whole periods to fall inside (or as
    /// close as possible to) the range between `t0` and `t1`.
    pub fn param_in_range(&self, p: DVec3, t0: f64, t1: f64) -> f64 {
        let t = self.param_of(p);
        let Some(period) = self.period() else {
            return t;
        };
        let (lo, hi) = (t0.min(t1), t0.max(t1));
        let mut best = t;
        let mut best_err = f64::INFINITY;
        for k in -3..=3 {
            let c = t + period * k as f64;
            let err = if c < lo {
                lo - c
            } else if c > hi {
                c - hi
            } else {
                0.0
            };
            if err < best_err {
                best_err = err;
                best = c;
            }
        }
        best
    }

    /// Distance from `p` to the piece of the curve between `t0` and `t1`.
    pub fn distance_to(&self, p: DVec3, t0: f64, t1: f64) -> f64 {
        let (lo, hi) = (t0.min(t1), t0.max(t1));
        match self {
            Curve::Line { .. } => {
                let t = self.param_of(p).clamp(lo, hi);
                (self.eval(t) - p).length()
            }
            Curve::Circle { .. } => {
                let t = self.param_in_range(p, lo, hi);
                if t >= lo && t <= hi {
                    (self.eval(t) - p).length()
                } else {
                    (self.eval(lo) - p).length().min((self.eval(hi) - p).length())
                }
            }
            Curve::Ellipse { .. } => {
                // No closed form: coarse search then golden-section refinement.
                let n = 96;
                let mut best_t = lo;
                let mut best = f64::INFINITY;
                for i in 0..=n {
                    let t = lo + (hi - lo) * i as f64 / n as f64;
                    let d = (self.eval(t) - p).length_squared();
                    if d < best {
                        best = d;
                        best_t = t;
                    }
                }
                let step = (hi - lo) / n as f64;
                let (mut a, mut b) = ((best_t - step).max(lo), (best_t + step).min(hi));
                for _ in 0..60 {
                    let m1 = a + (b - a) * 0.382;
                    let m2 = a + (b - a) * 0.618;
                    if (self.eval(m1) - p).length_squared() < (self.eval(m2) - p).length_squared() {
                        b = m2;
                    } else {
                        a = m1;
                    }
                }
                (self.eval(0.5 * (a + b)) - p).length().min(best.sqrt())
            }
            Curve::Spline(s) => (s.eval(s.closest(p, lo, hi)) - p).length(),
        }
    }

    /// Distance from `p` to the whole (unbounded or closed) curve.
    pub fn distance_to_curve(&self, p: DVec3) -> f64 {
        match self {
            Curve::Line { origin, dir } => {
                let d = p - *origin;
                (d - *dir * d.dot(*dir)).length()
            }
            Curve::Spline(s) => {
                let (lo, hi) = s.domain();
                self.distance_to(p, lo, hi)
            }
            _ => self.distance_to(p, -std::f64::consts::PI, std::f64::consts::PI),
        }
    }
}
/// Number of chords needed to approximate an arc of `sweep` radians.
pub fn arc_steps(radius: f64, sweep: f64, chord: f64, max_angle: f64) -> usize {
    let mut step = max_angle.max(1e-3);
    if chord > 0.0 && chord < radius {
        step = step.min(2.0 * (1.0 - chord / radius).acos());
    }
    let n = (sweep / step).ceil();
    let min = if sweep >= TAU - 1e-12 { 8.0 } else { 1.0 };
    n.clamp(min, 4096.0) as usize
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Surface {
    /// The plane through `frame.origin` with normal `frame.z`.
    Plane { frame: Frame },
    /// The cylinder with axis `frame.z` through `frame.origin`. Its natural
    /// normal points away from the axis.
    Cylinder { frame: Frame, radius: f64 },
    /// The cone with axis `frame.z` whose radius is `radius` at `frame.origin` and
    /// grows by `tan(half_angle)` per unit along the axis. Its natural normal points
    /// away from the axis. `radius` is kept positive (the origin is never the apex).
    Cone { frame: Frame, radius: f64, half_angle: f64 },
}

impl Surface {
    /// The surface's natural normal at a point on (or near) it.
    pub fn normal_at(&self, p: DVec3) -> DVec3 {
        match self {
            Surface::Plane { frame } => frame.z,
            Surface::Cylinder { frame, .. } => {
                let l = frame.to_local(p);
                (frame.x * l.x + frame.y * l.y).normalize_or(frame.x)
            }
            Surface::Cone { frame, half_angle, .. } => {
                let l = frame.to_local(p);
                let radial = (frame.x * l.x + frame.y * l.y).normalize_or(frame.x);
                radial * half_angle.cos() - frame.z * half_angle.sin()
            }
        }
    }

    pub fn frame(&self) -> &Frame {
        match self {
            Surface::Plane { frame } | Surface::Cylinder { frame, .. } | Surface::Cone { frame, .. } => frame,
        }
    }

    /// The surface moved by the rigid motion `m`.
    pub fn moved(&self, m: &Frame) -> Surface {
        match *self {
            Surface::Plane { frame } => Surface::Plane { frame: frame.moved(m) },
            Surface::Cylinder { frame, radius } => Surface::Cylinder { frame: frame.moved(m), radius },
            Surface::Cone { frame, radius, half_angle } => {
                Surface::Cone { frame: frame.moved(m), radius, half_angle }
            }
        }
    }

    /// Distance from a point to the surface.
    pub fn distance(&self, p: DVec3) -> f64 {
        match self {
            Surface::Plane { frame } => (p - frame.origin).dot(frame.z).abs(),
            Surface::Cylinder { frame, radius } => {
                let l = frame.to_local(p);
                (DVec2::new(l.x, l.y).length() - radius).abs()
            }
            Surface::Cone { .. } => self.cone_offset(p).abs(),
        }
    }

    /// Signed distance from a cone (positive away from the axis), measured to the
    /// nappe the cone's faces lie on.
    pub fn cone_offset(&self, p: DVec3) -> f64 {
        match self {
            Surface::Cone { frame, radius, half_angle } => {
                let l = frame.to_local(p);
                let rho = DVec2::new(l.x, l.y).length();
                (rho - radius - l.z * half_angle.tan()) * half_angle.cos()
            }
            _ => 0.0,
        }
    }

    /// Radius of a cone at height `v` along its axis.
    fn cone_radius(radius: f64, half_angle: f64, v: f64) -> f64 {
        radius + v * half_angle.tan()
    }

    /// Parameter-space coordinates scaled to model units. On a cylinder `u`
    /// is arc length around the axis (principal angle times radius) and `v`
    /// is height along it.
    pub fn uv(&self, p: DVec3) -> DVec2 {
        match self {
            Surface::Plane { frame } => {
                let l = frame.to_local(p);
                DVec2::new(l.x, l.y)
            }
            Surface::Cylinder { frame, radius } => {
                let l = frame.to_local(p);
                DVec2::new(l.y.atan2(l.x) * radius, l.z)
            }
            // `u` is the angle scaled by the radius at the origin, so generators are
            // straight lines and circles are horizontal lines in parameter space.
            Surface::Cone { frame, radius, .. } => {
                let l = frame.to_local(p);
                DVec2::new(l.y.atan2(l.x) * radius, l.z)
            }
        }
    }

    pub fn eval_uv(&self, uv: DVec2) -> DVec3 {
        match self {
            Surface::Plane { frame } => frame.to_world(uv),
            Surface::Cylinder { frame, radius } => {
                let t = uv.x / radius;
                frame.origin + (frame.x * t.cos() + frame.y * t.sin()) * *radius + frame.z * uv.y
            }
            Surface::Cone { frame, radius, half_angle } => {
                let t = uv.x / radius;
                let r = Self::cone_radius(*radius, *half_angle, uv.y);
                frame.origin + (frame.x * t.cos() + frame.y * t.sin()) * r + frame.z * uv.y
            }
        }
    }

    /// Period of `u`, if the surface wraps around.
    pub fn u_period(&self) -> Option<f64> {
        match self {
            Surface::Plane { .. } => None,
            Surface::Cylinder { radius, .. } | Surface::Cone { radius, .. } => Some(TAU * radius),
        }
    }

    /// True when both describe the same point set.
    pub fn coincident(&self, other: &Surface, tol: f64) -> bool {
        match (self, other) {
            (Surface::Plane { frame: a }, Surface::Plane { frame: b }) => {
                a.z.cross(b.z).length() < 1e-9 && (b.origin - a.origin).dot(a.z).abs() < tol
            }
            (Surface::Cylinder { frame: a, radius: ra }, Surface::Cylinder { frame: b, radius: rb }) => {
                let d = b.origin - a.origin;
                a.z.cross(b.z).length() < 1e-9
                    && (d - a.z * d.dot(a.z)).length() < tol
                    && (ra - rb).abs() < tol
            }
            (
                Surface::Cone { frame: a, radius: ra, half_angle: ha },
                Surface::Cone { frame: b, radius: rb, half_angle: hb },
            ) => {
                // Same axis line, same apex and same opening.
                let (aa, ab) = (cone_apex(a, *ra, *ha), cone_apex(b, *rb, *hb));
                a.z.dot(b.z) > 0.0
                    && a.z.cross(b.z).length() < 1e-9
                    && (aa - ab).length() < tol
                    && (ha - hb).abs() < 1e-9
            }
            _ => false,
        }
    }
}

/// The apex of a cone (where its radius reaches zero).
pub fn cone_apex(frame: &Frame, radius: f64, half_angle: f64) -> DVec3 {
    frame.origin - frame.z * (radius / half_angle.tan())
}
