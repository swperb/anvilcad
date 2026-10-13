//! General intersection curves of a cylinder or cone with another plane, cylinder
//! or cone, for the cases with no conic answer.
//!
//! A cylinder or cone is made of straight lines (generators), one for each angle
//! around its axis. Each generator meets the other surface where a quadratic in
//! its height is zero, so sweeping the angle traces the whole intersection exactly:
//! where the quadratic has two roots for every angle the curve is two closed loops,
//! and where its roots appear and vanish together the curve is one loop per range
//! of angles, turning back at the ends. Points found this way lie on both surfaces
//! to rounding error. They are joined into cubic splines whose tangents are the
//! cross product of the two normals, refined until the spline itself stays on both
//! surfaces.

use crate::geom::{Curve, Surface};
use crate::spline::Spline;
use crate::{KernelError, Result};
use glam::DVec3;
use std::f64::consts::{PI, TAU};
use std::sync::Arc;

/// How far a fitted spline may stray from either surface (mm).
const FIT: f64 = 1e-7;

/// Angles sampled around the swept surface when looking for the curve.
const SAMPLES: usize = 720;

/// The intersection curves of `a` and `b`, at least one of which must be a
/// cylinder or cone. Tries sweeping each in turn.
pub fn intersect(a: &Surface, b: &Surface) -> Result<Vec<Curve>> {
    if matches!(a, Surface::Torus { .. }) || matches!(b, Surface::Torus { .. }) {
        return Err(KernelError::Unsupported("traced intersections with a torus".into()));
    }
    let sweepable = |s: &Surface| matches!(s, Surface::Cylinder { .. } | Surface::Cone { .. });
    let radius = |s: &Surface| match s {
        Surface::Cylinder { radius, .. } | Surface::Cone { radius, .. } => *radius,
        Surface::Plane { .. } | Surface::Torus { .. } | Surface::Sphere { .. } => f64::INFINITY,
    };
    // The narrower surface first: its generators are the likelier to all hit the other.
    let order = if radius(a) <= radius(b) { [(a, b), (b, a)] } else { [(b, a), (a, b)] };
    let mut last = KernelError::Unsupported("intersection of these surfaces".into());
    for (s, o) in order {
        if !sweepable(s) {
            continue;
        }
        match sweep(s, o) {
            Ok(curves) => return Ok(curves),
            Err(e) => last = e,
        }
    }
    Err(last)
}

/// The generator of `s` at angle `theta`: a point and a direction whose `v`
/// multiple is the height along the axis.
fn generator(s: &Surface, theta: f64) -> (DVec3, DVec3) {
    let f = s.frame();
    let radial = f.x * theta.cos() + f.y * theta.sin();
    match *s {
        Surface::Cylinder { radius, .. } => (f.origin + radial * radius, f.z),
        Surface::Cone { radius, half_angle, .. } => {
            (f.origin + radial * radius, f.z + radial * half_angle.tan())
        }
        Surface::Plane { .. } | Surface::Torus { .. } | Surface::Sphere { .. } => (f.origin, f.z),
    }
}

/// True when height `v` on the generator lies on the swept surface's own nappe.
fn on_sweep_nappe(s: &Surface, v: f64) -> bool {
    match *s {
        Surface::Cone { radius, half_angle, .. } => radius + v * half_angle.tan() > 0.0,
        _ => true,
    }
}

/// True when `p` lies on the nappe of `s` its faces use.
fn on_nappe(s: &Surface, p: DVec3) -> bool {
    match *s {
        Surface::Cone { frame, radius, half_angle } => {
            radius + frame.to_local(p).z * half_angle.tan() > -1e-9
        }
        _ => true,
    }
}

/// Coefficients (a, b, c) of `a v^2 + b v + c`, zero where `o + d v` meets `s`.
fn quadric(s: &Surface, o: DVec3, d: DVec3) -> (f64, f64, f64) {
    match *s {
        Surface::Plane { frame } => (0.0, frame.z.dot(d), frame.z.dot(o - frame.origin)),
        // Never swept against: `intersect` refuses tori first.
        Surface::Torus { .. } => (0.0, 0.0, 1.0),
        Surface::Sphere { frame, radius } => {
            let r = o - frame.origin;
            (d.dot(d), 2.0 * d.dot(r), r.dot(r) - radius * radius)
        }
        Surface::Cylinder { frame, radius } => {
            let lo = frame.to_local(o);
            let ld = DVec3::new(d.dot(frame.x), d.dot(frame.y), d.dot(frame.z));
            (
                ld.x * ld.x + ld.y * ld.y,
                2.0 * (lo.x * ld.x + lo.y * ld.y),
                lo.x * lo.x + lo.y * lo.y - radius * radius,
            )
        }
        Surface::Cone { frame, radius, half_angle } => {
            let k = half_angle.tan();
            let lo = frame.to_local(o);
            let ld = DVec3::new(d.dot(frame.x), d.dot(frame.y), d.dot(frame.z));
            let (w0, w1) = (radius + k * lo.z, k * ld.z);
            (
                ld.x * ld.x + ld.y * ld.y - w1 * w1,
                2.0 * (lo.x * ld.x + lo.y * ld.y - w0 * w1),
                lo.x * lo.x + lo.y * lo.y - w0 * w0,
            )
        }
    }
}

/// A closed curve traced over a parameter `sigma` in `[0, span)`.
struct Trace<'a> {
    sweep: &'a Surface,
    other: &'a Surface,
    /// Maps `sigma` to (angle, which root: -1 or +1).
    at: Box<dyn Fn(f64) -> (f64, f64) + 'a>,
    span: f64,
    linear: bool,
}

impl Trace<'_> {
    fn point(&self, sigma: f64) -> (DVec3, f64) {
        let (theta, sign) = (self.at)(sigma);
        let (o, d) = generator(self.sweep, theta);
        let (a, b, c) = quadric(self.other, o, d);
        let v = if self.linear {
            -c / b
        } else {
            let disc = (b * b - 4.0 * a * c).max(0.0);
            (-b + sign * disc.sqrt()) / (2.0 * a)
        };
        (o + d * v, v)
    }

    fn tangent(&self, p: DVec3, sigma: f64) -> DVec3 {
        let t = self.sweep.normal_at(p).cross(self.other.normal_at(p));
        // Orient along increasing sigma; fall back to a difference where the
        // surfaces touch.
        let h = 1e-6 * self.span;
        let ahead = self.point(sigma + h).0 - self.point(sigma - h).0;
        match t.try_normalize() {
            Some(t) if t.dot(ahead) >= 0.0 => t,
            Some(t) => -t,
            None => ahead.normalize_or(DVec3::X),
        }
    }

    fn off(&self, p: DVec3) -> f64 {
        self.sweep.distance(p).max(self.other.distance(p))
    }

    /// The traced curve as a closed spline.
    fn spline(&self) -> Result<Curve> {
        let n = if self.linear { 64 } else { 96 };
        let mut sigmas: Vec<f64> = (0..n).map(|i| self.span * i as f64 / n as f64).collect();
        sigmas.push(self.span);
        let mut out: Vec<(DVec3, DVec3)> = Vec::new();
        for w in sigmas.windows(2) {
            self.refine(w[0], w[1], 0, &mut out)?;
        }
        if out.len() > 50_000 {
            return Err(KernelError::Unsupported("intersection curve needs too many points".into()));
        }
        let (pts, tans): (Vec<DVec3>, Vec<DVec3>) = out.into_iter().unzip();
        Ok(Curve::Spline(Arc::new(Spline::hermite(&pts, &tans, true)?)))
    }

    /// Adds the point at `s0` and enough points before `s1` for the fitted cubic
    /// to stay on both surfaces.
    fn refine(&self, s0: f64, s1: f64, depth: u32, out: &mut Vec<(DVec3, DVec3)>) -> Result<()> {
        let (p0, v0) = self.point(s0);
        let (p1, _) = self.point(s1);
        if !on_sweep_nappe(self.sweep, v0) || !on_nappe(self.other, p0) {
            return Err(KernelError::Unsupported("intersection curve passes through a cone's apex".into()));
        }
        let (t0, t1) = (self.tangent(p0, s0), self.tangent(p1, s1));
        let h = (p1 - p0).length();
        let phi = t0.angle_between(t1);
        let handle = if phi < 1e-6 { h / 3.0 } else { 2.0 / 3.0 * h * (phi / 4.0).tan() / (phi / 2.0).sin() };
        let (c1, c2) = (p0 + t0 * handle, p1 - t1 * handle);
        let bez = |u: f64| {
            let w = 1.0 - u;
            p0 * (w * w * w) + c1 * (3.0 * w * w * u) + c2 * (3.0 * w * u * u) + p1 * (u * u * u)
        };
        let err = [0.25, 0.5, 0.75].iter().map(|u| self.off(bez(*u))).fold(0.0, f64::max);
        if err > FIT && depth < 14 && h > 1e-9 {
            let m = 0.5 * (s0 + s1);
            self.refine(s0, m, depth + 1, out)?;
            self.refine(m, s1, depth + 1, out)
        } else {
            if h > 1e-9 || out.is_empty() {
                out.push((p0, t0));
            }
            Ok(())
        }
    }
}

fn sweep(s: &Surface, other: &Surface) -> Result<Vec<Curve>> {
    let thetas: Vec<f64> = (0..SAMPLES).map(|i| TAU * i as f64 / SAMPLES as f64).collect();
    let coeffs: Vec<(f64, f64, f64)> = thetas
        .iter()
        .map(|t| {
            let (o, d) = generator(s, *t);
            quadric(other, o, d)
        })
        .collect();
    let scale = coeffs.iter().map(|c| c.0.abs().max(c.1.abs())).fold(0.0, f64::max).max(1e-300);
    let small = |x: f64| x.abs() < 1e-12 * scale;
    let unbounded = || Err(KernelError::Unsupported("intersection curve is not closed".into()));

    let linear = coeffs.iter().all(|c| small(c.0));
    if linear {
        if coeffs.iter().any(|c| small(c.1)) {
            return unbounded();
        }
        let trace = Trace { sweep: s, other, at: Box::new(|sig| (TAU * sig, 1.0)), span: 1.0, linear: true };
        return Ok(vec![trace.spline()?]);
    }
    let sign_a = coeffs[0].0.signum();
    if coeffs.iter().any(|c| small(c.0) || c.0.signum() != sign_a) {
        return unbounded();
    }

    let disc_at = |theta: f64| {
        let (o, d) = generator(s, theta);
        let (a, b, c) = quadric(other, o, d);
        (b * b - 4.0 * a * c) / (scale * scale)
    };
    let disc: Vec<f64> = coeffs.iter().map(|(a, b, c)| (b * b - 4.0 * a * c) / (scale * scale)).collect();
    let mut curves = Vec::new();
    if disc.iter().all(|d| *d > 0.0) {
        // Two roots at every angle: two loops around the axis.
        for sign in [-1.0, 1.0] {
            let trace = Trace {
                sweep: s,
                other,
                at: Box::new(move |sig| (TAU * sig, sign)),
                span: 1.0,
                linear: false,
            };
            curves.push(trace.spline()?);
        }
        return Ok(curves);
    }
    // Angles where the roots appear or vanish, found by bisection.
    let mut changes: Vec<(f64, bool)> = Vec::new(); // (angle, entering a range with roots)
    for i in 0..SAMPLES {
        let j = (i + 1) % SAMPLES;
        let (d0, d1) = (disc[i], disc[j]);
        if (d0 > 0.0) != (d1 > 0.0) {
            let (mut lo, mut hi) = (thetas[i], if j == 0 { TAU } else { thetas[j] });
            let rising = d1 > 0.0;
            for _ in 0..80 {
                let m = 0.5 * (lo + hi);
                if (disc_at(m) > 0.0) == rising {
                    hi = m;
                } else {
                    lo = m;
                }
            }
            changes.push((if rising { lo } else { hi }, rising));
        }
    }
    if changes.is_empty() {
        return Ok(Vec::new());
    }
    // Pair each rise with the next fall around the circle.
    let start = changes.iter().position(|c| c.1).unwrap_or(0);
    changes.rotate_left(start);
    for pair in changes.chunks(2) {
        let [(a, true), (b, false)] = *pair else { continue };
        let b = if b <= a { b + TAU } else { b };
        if b - a < 1e-9 {
            continue; // A touch, not a crossing.
        }
        // sigma in [0, 2): down one root and back along the other. The cosine
        // spacing makes points crowd where the roots meet and the curve turns.
        let at = move |sig: f64| {
            let (s, sign) = if sig < 1.0 { (sig, -1.0) } else { (2.0 - sig, 1.0) };
            (a + (b - a) * 0.5 * (1.0 - (PI * s).cos()), sign)
        };
        let trace = Trace { sweep: s, other, at: Box::new(at), span: 2.0, linear: false };
        curves.push(trace.spline()?);
    }
    Ok(curves)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::Frame;

    fn cyl(origin: DVec3, axis: DVec3, radius: f64) -> Surface {
        let hint = if axis.x.abs() < 0.9 { DVec3::X } else { DVec3::Y };
        Surface::Cylinder { frame: Frame::new(origin, axis, hint).expect("frame"), radius }
    }

    fn on_both(c: &Curve, a: &Surface, b: &Surface) {
        let Curve::Spline(s) = c else { panic!("not a spline") };
        let (lo, hi) = s.domain();
        for i in 0..500 {
            let p = s.eval(lo + (hi - lo) * i as f64 / 500.0);
            assert!(a.distance(p) < 1e-6 && b.distance(p) < 1e-6, "{} {}", a.distance(p), b.distance(p));
        }
    }

    #[test]
    fn small_cylinder_through_a_large_one() {
        // A 2 mm hole drilled across a 5 mm shaft: two loops, entry and exit.
        let shaft = cyl(DVec3::ZERO, DVec3::X, 5.0);
        let hole = cyl(DVec3::new(1.0, 0.0, 0.0), DVec3::Z, 2.0);
        let cs = intersect(&shaft, &hole).expect("curves");
        assert_eq!(cs.len(), 2);
        for c in &cs {
            on_both(c, &shaft, &hole);
        }
    }

    #[test]
    fn cylinders_crossing_partly() {
        // Axes offset so each cuts only part of the other: one loop that turns back.
        let a = cyl(DVec3::ZERO, DVec3::X, 3.0);
        let b = cyl(DVec3::new(0.0, 2.5, 0.0), DVec3::Z, 1.5);
        let cs = intersect(&a, &b).expect("curves");
        assert_eq!(cs.len(), 1);
        on_both(&cs[0], &a, &b);
    }

    #[test]
    fn oblique_plane_through_a_cone() {
        let cone = Surface::Cone { frame: Frame::world(), radius: 2.0, half_angle: 0.3 };
        let plane = Surface::Plane {
            frame: Frame::new(DVec3::new(0.0, 0.0, 1.0), DVec3::new(0.3, 0.0, 1.0), DVec3::X).expect("frame"),
        };
        let cs = intersect(&cone, &plane).expect("curves");
        assert_eq!(cs.len(), 1);
        on_both(&cs[0], &cone, &plane);
        // A steep plane meets the cone in an open hyperbola, which is refused.
        let steep = Surface::Plane {
            frame: Frame::new(DVec3::new(1.0, 0.0, 0.0), DVec3::X, DVec3::Z).expect("frame"),
        };
        assert!(intersect(&cone, &steep).is_err());
    }

    #[test]
    fn cylinder_into_a_cone() {
        let cone = Surface::Cone { frame: Frame::world(), radius: 6.0, half_angle: 0.4 };
        let pin = cyl(DVec3::new(0.0, 0.0, 2.0), DVec3::X, 1.0);
        let cs = intersect(&cone, &pin).expect("curves");
        assert_eq!(cs.len(), 2);
        for c in &cs {
            on_both(c, &cone, &pin);
        }
    }
}
