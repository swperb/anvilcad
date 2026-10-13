//! Surface-surface and curve-surface intersection.

use crate::geom::{Curve, Frame, Surface};
use crate::{KernelError, Result};
use glam::{DVec2, DVec3};
use std::f64::consts::PI;

/// Directions closer than this to parallel are treated as parallel.
const PARALLEL: f64 = 1e-9;

#[derive(Debug, Clone, PartialEq)]
pub enum Ssi {
    /// The surfaces do not meet.
    None,
    /// The surfaces are the same point set.
    Coincident,
    /// Intersection curves, unbounded (lines) or closed (circles, ellipses).
    Curves(Vec<Curve>),
}

/// Intersects two surfaces. `tol` decides tangency and coincidence.
pub fn surfaces(a: &Surface, b: &Surface, tol: f64) -> Result<Ssi> {
    if a.coincident(b, tol) {
        return Ok(Ssi::Coincident);
    }
    match (a, b) {
        (Surface::Plane { frame: fa }, Surface::Plane { frame: fb }) => Ok(plane_plane(fa, fb)),
        (Surface::Plane { frame: fp }, Surface::Cylinder { frame, radius })
        | (Surface::Cylinder { frame, radius }, Surface::Plane { frame: fp }) => {
            plane_cylinder(fp, frame, *radius, tol)
        }
        (Surface::Cylinder { frame: fa, radius: ra }, Surface::Cylinder { frame: fb, radius: rb }) => {
            cylinder_cylinder(fa, *ra, fb, *rb, tol)
        }
        (Surface::Plane { frame: fp }, Surface::Cone { frame, radius, half_angle })
        | (Surface::Cone { frame, radius, half_angle }, Surface::Plane { frame: fp }) => {
            plane_cone(fp, frame, *radius, *half_angle, tol)
        }
        (Surface::Cylinder { frame: fc, radius: rc }, Surface::Cone { frame, radius, half_angle })
        | (Surface::Cone { frame, radius, half_angle }, Surface::Cylinder { frame: fc, radius: rc }) => {
            // A cylinder is a cone that does not open.
            coaxial_cones(frame, *radius, *half_angle, fc, *rc, 0.0, tol)
        }
        (
            Surface::Cone { frame: fa, radius: ra, half_angle: ha },
            Surface::Cone { frame: fb, radius: rb, half_angle: hb },
        ) => coaxial_cones(fa, *ra, *ha, fb, *rb, *hb, tol),
        (Surface::Torus { frame, major, minor, .. }, other)
        | (other, Surface::Torus { frame, major, minor, .. }) => {
            torus_with(frame, *major, *minor, other, tol)
        }
    }
}

/// A surface of revolution seen in a half-plane through an axis, with coordinates
/// (distance from the axis, height along it).
#[derive(Debug, Clone, Copy)]
enum Meridian {
    Line { p: DVec2, d: DVec2 },
    Circle { c: DVec2, r: f64 },
}

/// The meridian of `s` about the axis of `axis`, if `s` is a plane across that
/// axis or a surface of revolution about it.
fn meridian(axis: &Frame, s: &Surface, tol: f64) -> Option<Meridian> {
    let f = s.frame();
    let d = f.origin - axis.origin;
    let z0 = d.dot(axis.z);
    let parallel = axis.z.cross(f.z).length() < PARALLEL;
    let on_axis = (d - axis.z * z0).length() < tol;
    let sign = axis.z.dot(f.z).signum();
    match *s {
        Surface::Plane { .. } if parallel => Some(Meridian::Line { p: DVec2::new(0.0, z0), d: DVec2::X }),
        Surface::Cylinder { radius, .. } if parallel && on_axis => {
            Some(Meridian::Line { p: DVec2::new(radius, 0.0), d: DVec2::Y })
        }
        Surface::Cone { radius, half_angle, .. } if parallel && on_axis => {
            Some(Meridian::Line { p: DVec2::new(radius, z0), d: DVec2::new(half_angle.tan() * sign, 1.0) })
        }
        Surface::Torus { major, minor, .. } if parallel && on_axis => {
            Some(Meridian::Circle { c: DVec2::new(major, z0), r: minor })
        }
        _ => None,
    }
}

/// Where two meridians cross.
fn meridian_points(a: Meridian, b: Meridian, tol: f64) -> Vec<DVec2> {
    use Meridian::*;
    let line_circle = |p: DVec2, d: DVec2, c: DVec2, r: f64| -> Vec<DVec2> {
        let d = d.normalize();
        let foot = p + d * (c - p).dot(d);
        let h2 = r * r - (foot - c).length_squared();
        if h2 < -tol * tol {
            Vec::new()
        } else if h2 <= tol * tol {
            vec![foot]
        } else {
            let h = h2.sqrt();
            vec![foot + d * h, foot - d * h]
        }
    };
    match (a, b) {
        (Line { p: p1, d: d1 }, Line { p: p2, d: d2 }) => {
            let den = d1.perp_dot(d2);
            if den.abs() < PARALLEL {
                return Vec::new();
            }
            vec![p1 + d1 * ((p2 - p1).perp_dot(d2) / den)]
        }
        (Line { p, d }, Circle { c, r }) | (Circle { c, r }, Line { p, d }) => line_circle(p, d, c, r),
        (Circle { c: c1, r: r1 }, Circle { c: c2, r: r2 }) => {
            let v = c2 - c1;
            let dist = v.length();
            if dist < tol || dist > r1 + r2 + tol || dist < (r1 - r2).abs() - tol {
                return Vec::new();
            }
            let x = (dist * dist + r1 * r1 - r2 * r2) / (2.0 * dist);
            let u = v / dist;
            let h2 = r1 * r1 - x * x;
            if h2 <= tol * tol {
                vec![c1 + u * x]
            } else {
                let h = h2.sqrt();
                vec![c1 + u * x + u.perp() * h, c1 + u * x - u.perp() * h]
            }
        }
    }
}

/// A torus with any other surface. Planes across its axis and surfaces of
/// revolution about it meet it in circles, planes through its axis in two circles
/// round the tube; anything else is not supported yet.
fn torus_with(t: &Frame, major: f64, minor: f64, other: &Surface, tol: f64) -> Result<Ssi> {
    let torus = Meridian::Circle { c: DVec2::ZERO + DVec2::new(major, 0.0), r: minor };
    if let Some(m) = meridian(t, other, tol) {
        let curves = meridian_points(torus, m, tol)
            .into_iter()
            .filter(|q| q.x > tol)
            .map(|q| Curve::Circle { frame: t.with_origin(t.origin + t.z * q.y), radius: q.x })
            .collect::<Vec<_>>();
        return Ok(if curves.is_empty() { Ssi::None } else { Ssi::Curves(curves) });
    }
    if let Surface::Plane { frame: p } = other
        && p.z.dot(t.z).abs() < PARALLEL
        && (t.origin - p.origin).dot(p.z).abs() < tol
    {
        // Through the axis: the tube's cross-sections on either side.
        let side = p.z.cross(t.z).normalize();
        let circles = [side, -side]
            .into_iter()
            .map(|s| Curve::Circle {
                frame: Frame { origin: t.origin + s * major, x: s, y: t.z, z: s.cross(t.z) },
                radius: minor,
            })
            .collect();
        return Ok(Ssi::Curves(circles));
    }
    Err(KernelError::Unsupported("intersection of a torus with a surface not on its axis".into()))
}

fn plane_cone(plane: &Frame, cone: &Frame, r: f64, half_angle: f64, tol: f64) -> Result<Ssi> {
    let n = plane.z;
    let axis = cone.z;
    let cos = n.dot(axis);
    let tan = half_angle.tan();
    if (1.0 - cos.abs()) < PARALLEL {
        // Perpendicular to the axis: one circle, unless the plane misses the nappe.
        let h = (plane.origin - cone.origin).dot(axis);
        let radius = r + h * tan;
        if radius <= tol {
            return Ok(Ssi::None);
        }
        let center = cone.origin + axis * h;
        return Ok(Ssi::Curves(vec![Curve::Circle { frame: cone.with_origin(center), radius }]));
    }
    let apex = crate::geom::cone_apex(cone, r, half_angle);
    if cos.abs() < PARALLEL && (apex - plane.origin).dot(n).abs() < tol {
        // A plane through the axis: the two generators in it.
        let side = n.cross(axis).normalize();
        let (c, s) = (half_angle.cos(), half_angle.sin());
        return Ok(Ssi::Curves(vec![
            Curve::Line { origin: apex, dir: axis * c + side * s },
            Curve::Line { origin: apex, dir: axis * c - side * s },
        ]));
    }
    // At an angle: an ellipse when the plane cuts every generator, traced in general.
    Ok(Ssi::Curves(crate::ssi::intersect(
        &Surface::Plane { frame: *plane },
        &cone_surface(cone, r, half_angle),
    )?))
}

fn cone_surface(frame: &Frame, radius: f64, half_angle: f64) -> Surface {
    Surface::Cone { frame: *frame, radius, half_angle }
}

/// Two surfaces of revolution about the same axis, each given as a cone (a
/// cylinder has `half_angle` 0): they meet in circles where their radii agree.
fn coaxial_cones(a: &Frame, ra: f64, ha: f64, b: &Frame, rb: f64, hb: f64, tol: f64) -> Result<Ssi> {
    let d = b.origin - a.origin;
    if a.z.cross(b.z).length() > PARALLEL || (d - a.z * d.dot(a.z)).length() > tol {
        let sa = cone_surface(a, ra, ha);
        let sb =
            if hb == 0.0 { Surface::Cylinder { frame: *b, radius: rb } } else { cone_surface(b, rb, hb) };
        return Ok(Ssi::Curves(crate::ssi::intersect(&sa, &sb)?));
    }
    // Heights along a's axis: b's radius is rb + sign * (z - z0) * tan(hb).
    let sign = a.z.dot(b.z).signum();
    let z0 = d.dot(a.z);
    let (ta, tb) = (ha.tan(), hb.tan() * sign);
    let slope = ta - tb;
    if slope.abs() < PARALLEL {
        return Ok(Ssi::None);
    }
    let z = (rb - z0 * tb - ra) / slope;
    let radius = ra + z * ta;
    if radius <= tol || rb + (z - z0) * tb <= tol {
        return Ok(Ssi::None);
    }
    Ok(Ssi::Curves(vec![Curve::Circle { frame: a.with_origin(a.origin + a.z * z), radius }]))
}

fn plane_plane(a: &Frame, b: &Frame) -> Ssi {
    let dir = a.z.cross(b.z);
    let len = dir.length();
    if len < PARALLEL {
        return Ssi::None;
    }
    let dir = dir / len;
    // Point on both planes closest to a's origin.
    let da = a.origin.dot(a.z);
    let db = b.origin.dot(b.z);
    let n1n2 = a.z.dot(b.z);
    let det = 1.0 - n1n2 * n1n2;
    let c1 = (da - db * n1n2) / det;
    let c2 = (db - da * n1n2) / det;
    let p = a.z * c1 + b.z * c2;
    let origin = p + dir * (a.origin - p).dot(dir);
    Ssi::Curves(vec![Curve::Line { origin, dir }])
}

fn plane_cylinder(plane: &Frame, cyl: &Frame, r: f64, tol: f64) -> Result<Ssi> {
    let n = plane.z;
    let axis = cyl.z;
    let cos = n.dot(axis);
    if cos.abs() < PARALLEL {
        // Plane parallel to the axis: zero, one or two lines.
        let d = (cyl.origin - plane.origin).dot(n);
        if d.abs() > r + tol {
            return Ok(Ssi::None);
        }
        let foot = cyl.origin - n * d;
        let side = n.cross(axis).normalize();
        let h2 = r * r - d * d;
        if h2 <= tol * tol || (r - d.abs()) < tol {
            return Ok(Ssi::Curves(vec![Curve::Line { origin: foot, dir: axis }]));
        }
        let h = h2.sqrt();
        return Ok(Ssi::Curves(vec![
            Curve::Line { origin: foot + side * h, dir: axis },
            Curve::Line { origin: foot - side * h, dir: axis },
        ]));
    }
    // Where the axis pierces the plane.
    let t = (plane.origin - cyl.origin).dot(n) / cos;
    let center = cyl.origin + axis * t;
    if (1.0 - cos.abs()) < PARALLEL {
        // Perpendicular: a circle parameterised like the cylinder itself.
        return Ok(Ssi::Curves(vec![Curve::Circle { frame: cyl.with_origin(center), radius: r }]));
    }
    let minor = axis.cross(n).normalize();
    let major = n.cross(minor).normalize();
    let frame = Frame { origin: center, x: major, y: minor, z: major.cross(minor) };
    Ok(Ssi::Curves(vec![Curve::Ellipse { frame, a: r / cos.abs(), b: r }]))
}

fn cylinder_cylinder(a: &Frame, ra: f64, b: &Frame, rb: f64, tol: f64) -> Result<Ssi> {
    let cross = a.z.cross(b.z);
    if cross.length() > PARALLEL {
        return crossing_cylinders(a, ra, b, rb, tol);
    }
    // Parallel axes: intersect the circular cross-sections.
    let la = DVec2::ZERO;
    let lb3 = a.to_local(b.origin);
    let lb = DVec2::new(lb3.x, lb3.y);
    let dv = lb - la;
    let d = dv.length();
    if d < tol {
        return Ok(Ssi::None); // Coaxial with different radii.
    }
    if d > ra + rb + tol || d < (ra - rb).abs() - tol {
        return Ok(Ssi::None);
    }
    let x = (d * d + ra * ra - rb * rb) / (2.0 * d);
    let h2 = ra * ra - x * x;
    let u = dv / d;
    let base = u * x;
    let pts: Vec<DVec2> = if h2 <= tol * tol {
        vec![base]
    } else {
        let h = h2.sqrt();
        vec![base + u.perp() * h, base - u.perp() * h]
    };
    Ok(Ssi::Curves(pts.into_iter().map(|p| Curve::Line { origin: a.to_world(p), dir: a.z }).collect()))
}

/// Cylinders whose axes are not parallel. The case that edge blends need,
/// equal radii with intersecting axes, meets in two plane ellipses: the planes
/// through the axes' crossing point with normals `a1 - a2` and `a1 + a2`.
/// Anything else is a general quartic curve.
fn crossing_cylinders(a: &Frame, ra: f64, b: &Frame, rb: f64, tol: f64) -> Result<Ssi> {
    // Anything but equal radii with meeting axes is a quartic, traced in general.
    let general = || {
        Ok(Ssi::Curves(crate::ssi::intersect(
            &Surface::Cylinder { frame: *a, radius: ra },
            &Surface::Cylinder { frame: *b, radius: rb },
        )?))
    };
    if (ra - rb).abs() > tol {
        return general();
    }
    let n = a.z.cross(b.z);
    let d = b.origin - a.origin;
    if d.dot(n).abs() / n.length() > tol {
        return general();
    }
    // Crossing point: a.origin + a.z * s with s from the closest-point solve.
    let s = d.cross(b.z).dot(n) / n.length_squared();
    let p = a.origin + a.z * s;
    let mut curves = Vec::new();
    for normal in [a.z - b.z, a.z + b.z] {
        let Ok(frame) = Frame::new(p, normal, a.z.cross(normal).normalize_or(a.x)) else {
            continue;
        };
        if let Ssi::Curves(cs) = plane_cylinder(&frame, a, ra, tol)? {
            curves.extend(cs);
        }
    }
    Ok(Ssi::Curves(curves))
}

/// Result of intersecting a curve piece with a surface.
#[derive(Debug, Clone, PartialEq)]
pub enum CurveHit {
    /// Parameters, within the piece's range, where the curve crosses or
    /// touches the surface.
    Params(Vec<f64>),
    /// The whole curve lies on the surface.
    LiesOn,
}

/// Intersects the piece of `curve` between `t0` and `t1` with `surface`.
pub fn curve_surface(curve: &Curve, t0: f64, t1: f64, surface: &Surface, tol: f64) -> CurveHit {
    let (lo, hi) = (t0.min(t1), t0.max(t1));
    let eps = match curve {
        Curve::Line { .. } => tol,
        _ => 1e-9,
    };
    let in_range = |t: &f64| *t >= lo - eps && *t <= hi + eps;
    // Lies on: check a few samples.
    let lies_on = (0..=4).all(|i| {
        let t = lo + (hi - lo) * i as f64 / 4.0;
        surface.distance(curve.eval(t)) < tol
    });
    if lies_on {
        return CurveHit::LiesOn;
    }
    let params: Vec<f64> = match (curve, surface) {
        (Curve::Line { origin, dir }, Surface::Plane { frame }) => {
            let den = dir.dot(frame.z);
            if den.abs() < PARALLEL { Vec::new() } else { vec![(frame.origin - *origin).dot(frame.z) / den] }
        }
        (Curve::Line { origin, dir }, Surface::Cylinder { frame, radius }) => {
            line_cylinder(*origin, *dir, frame, *radius, tol)
        }
        (Curve::Line { origin, dir }, Surface::Cone { frame, radius, half_angle }) => {
            line_cone(*origin, *dir, frame, *radius, *half_angle, tol)
        }
        (Curve::Circle { frame: cf, radius }, Surface::Plane { frame }) => {
            // (c + r cos t x + r sin t y - o) . n = 0
            let n = frame.z;
            trig_roots(radius * cf.x.dot(n), radius * cf.y.dot(n), (cf.origin - frame.origin).dot(n), tol)
        }
        (Curve::Ellipse { frame: cf, a, b }, Surface::Plane { frame }) => {
            let n = frame.z;
            trig_roots(a * cf.x.dot(n), b * cf.y.dot(n), (cf.origin - frame.origin).dot(n), tol)
        }
        (_, Surface::Cylinder { .. } | Surface::Cone { .. }) => numeric_roots(curve, lo, hi, surface, tol),
        (Curve::Line { origin, dir }, Surface::Torus { frame, major, minor, .. }) => {
            // Only the stretch of the line inside the torus's bounding sphere can hit it.
            let (o, r) = (*origin - frame.origin, major + minor);
            let b = o.dot(*dir);
            let disc = b * b - (o.length_squared() - r * r);
            if disc <= 0.0 {
                Vec::new()
            } else {
                let (a, c) = ((-b - disc.sqrt()).max(lo), (-b + disc.sqrt()).min(hi));
                if a < c { numeric_roots(curve, a, c, surface, tol) } else { Vec::new() }
            }
        }
        (_, Surface::Torus { .. }) => numeric_roots(curve, lo, hi, surface, tol),
        (Curve::Spline(_), Surface::Plane { .. }) => numeric_roots(curve, lo, hi, surface, tol),
    };
    let mut out: Vec<f64> = Vec::new();
    for t in params {
        // Bring periodic parameters into the piece's range.
        let t = if curve.is_periodic() { curve.param_in_range(curve.eval(t), lo, hi) } else { t };
        if in_range(&t) && surface.distance(curve.eval(t)) < tol * 10.0 {
            let t = t.clamp(lo, hi);
            if !out.iter().any(|o| (curve.eval(*o) - curve.eval(t)).length() < tol) {
                out.push(t);
            }
        }
    }
    CurveHit::Params(out)
}

fn line_cylinder(o: DVec3, d: DVec3, cyl: &Frame, r: f64, tol: f64) -> Vec<f64> {
    let lo = cyl.to_local(o);
    let ld = DVec3::new(d.dot(cyl.x), d.dot(cyl.y), d.dot(cyl.z));
    let a = ld.x * ld.x + ld.y * ld.y;
    if a < PARALLEL {
        return Vec::new();
    }
    let b = 2.0 * (lo.x * ld.x + lo.y * ld.y);
    let c = lo.x * lo.x + lo.y * lo.y - r * r;
    let disc = b * b - 4.0 * a * c;
    // Distance from the axis to the line, for tangency.
    let closest = -b / (2.0 * a);
    let p = lo + ld * closest;
    let dist = DVec2::new(p.x, p.y).length();
    if dist > r + tol {
        return Vec::new();
    }
    if disc <= 0.0 || (r - dist) < tol {
        return vec![closest];
    }
    let sq = disc.sqrt();
    vec![(-b - sq) / (2.0 * a), (-b + sq) / (2.0 * a)]
}

fn line_cone(o: DVec3, d: DVec3, cone: &Frame, r: f64, half_angle: f64, tol: f64) -> Vec<f64> {
    // x^2 + y^2 = (r + k z)^2 on the line o + d t, in the cone's frame.
    let k = half_angle.tan();
    let lo = cone.to_local(o);
    let ld = DVec3::new(d.dot(cone.x), d.dot(cone.y), d.dot(cone.z));
    let (w0, w1) = (r + k * lo.z, k * ld.z);
    let a = ld.x * ld.x + ld.y * ld.y - w1 * w1;
    let b = 2.0 * (lo.x * ld.x + lo.y * ld.y - w0 * w1);
    let c = lo.x * lo.x + lo.y * lo.y - w0 * w0;
    let mut roots = Vec::new();
    if a.abs() < PARALLEL {
        if b.abs() > PARALLEL {
            roots.push(-c / b);
        }
    } else {
        let disc = b * b - 4.0 * a * c;
        if disc >= 0.0 {
            let sq = disc.sqrt();
            roots.push((-b - sq) / (2.0 * a));
            if sq > 1e-12 {
                roots.push((-b + sq) / (2.0 * a));
            }
        } else if disc > -tol * tol * a.abs() {
            roots.push(-b / (2.0 * a));
        }
    }
    // Keep hits on the cone's own nappe, not its mirror past the apex.
    roots.retain(|t| w0 + w1 * t > -tol);
    roots
}

/// Roots of `a cos t + b sin t + c = 0` in `(-PI, PI]`.
fn trig_roots(a: f64, b: f64, c: f64, tol: f64) -> Vec<f64> {
    let r = (a * a + b * b).sqrt();
    if r < PARALLEL {
        return Vec::new();
    }
    let phi = b.atan2(a);
    let k = -c / r;
    if k.abs() > 1.0 + tol / r.max(1.0) {
        return Vec::new();
    }
    let k = k.clamp(-1.0, 1.0);
    let alpha = k.acos();
    if alpha < 1e-7 || (PI - alpha) < 1e-7 {
        return vec![phi + alpha];
    }
    vec![phi + alpha, phi - alpha]
}

/// Roots of the signed distance to a curved surface along a curve, by sampling and
/// bisection. Tangent touches are found from sampled minima.
fn numeric_roots(curve: &Curve, lo: f64, hi: f64, surface: &Surface, tol: f64) -> Vec<f64> {
    let f = |t: f64| {
        let p = curve.eval(t);
        match surface {
            Surface::Cylinder { frame, radius } => {
                let l = frame.to_local(p);
                DVec2::new(l.x, l.y).length() - radius
            }
            Surface::Cone { .. } => surface.cone_offset(p),
            Surface::Torus { .. } => surface.torus_offset(p),
            Surface::Plane { frame } => (p - frame.origin).dot(frame.z),
        }
    };
    let n = if matches!(curve, Curve::Spline(_)) || matches!(surface, Surface::Torus { .. }) {
        1024
    } else {
        256
    };
    let span = match curve.period() {
        Some(per) if (hi - lo) >= per - 1e-12 => per,
        _ => hi - lo,
    };
    let ts: Vec<f64> = (0..=n).map(|i| lo + span * i as f64 / n as f64).collect();
    let fs: Vec<f64> = ts.iter().map(|t| f(*t)).collect();
    let mut roots = Vec::new();
    for i in 0..n {
        let (t0, t1, f0, f1) = (ts[i], ts[i + 1], fs[i], fs[i + 1]);
        if f0.abs() < tol {
            roots.push(t0);
        } else if f0.signum() != f1.signum() && f1.abs() >= tol {
            let (mut a, mut b, mut fa) = (t0, t1, f0);
            for _ in 0..80 {
                let m = 0.5 * (a + b);
                let fm = f(m);
                if fm.signum() == fa.signum() {
                    a = m;
                    fa = fm;
                } else {
                    b = m;
                }
            }
            roots.push(0.5 * (a + b));
        }
    }
    if fs[n].abs() < tol {
        roots.push(ts[n]);
    }
    roots
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::TAU;

    fn plane(origin: DVec3, n: DVec3) -> Surface {
        Surface::Plane {
            frame: Frame::new(origin, n, if n.x.abs() < 0.9 { DVec3::X } else { DVec3::Y }).expect("frame"),
        }
    }

    fn cyl(origin: DVec3, axis: DVec3, r: f64) -> Surface {
        Surface::Cylinder {
            frame: Frame::new(origin, axis, if axis.x.abs() < 0.9 { DVec3::X } else { DVec3::Y })
                .expect("frame"),
            radius: r,
        }
    }

    fn on_both(c: &Curve, a: &Surface, b: &Surface) {
        for i in 0..16 {
            let p = c.eval(-3.0 + 6.0 * i as f64 / 15.0);
            assert!(a.distance(p) < 1e-9 && b.distance(p) < 1e-9, "{p:?}");
        }
    }

    #[test]
    fn plane_plane_line_lies_on_both() {
        let a = plane(DVec3::new(0.0, 0.0, 2.0), DVec3::Z);
        let b = plane(DVec3::new(3.0, 0.0, 0.0), DVec3::new(1.0, 1.0, 0.0));
        let Ok(Ssi::Curves(c)) = surfaces(&a, &b, 1e-6) else { panic!() };
        assert_eq!(c.len(), 1);
        on_both(&c[0], &a, &b);
    }

    #[test]
    fn coplanar_and_parallel_planes() {
        let a = plane(DVec3::ZERO, DVec3::Z);
        let b = plane(DVec3::new(5.0, 1.0, 0.0), DVec3::NEG_Z);
        assert_eq!(surfaces(&a, &b, 1e-6), Ok(Ssi::Coincident));
        let c = plane(DVec3::new(0.0, 0.0, 1.0), DVec3::Z);
        assert_eq!(surfaces(&a, &c, 1e-6), Ok(Ssi::None));
    }

    #[test]
    fn plane_cylinder_cases() {
        let c = cyl(DVec3::ZERO, DVec3::Z, 2.0);
        for (p, n, count) in [
            (plane(DVec3::new(0.0, 0.0, 3.0), DVec3::Z), 0, 1),
            (plane(DVec3::new(1.0, 0.0, 0.0), DVec3::X), 1, 2),
            (plane(DVec3::new(2.0, 0.0, 0.0), DVec3::X), 2, 1),
            (plane(DVec3::ZERO, DVec3::new(1.0, 0.0, 1.0)), 3, 1),
        ] {
            let Ok(Ssi::Curves(cs)) = surfaces(&p, &c, 1e-6) else { panic!("case {n}") };
            assert_eq!(cs.len(), count, "case {n}");
            for curve in &cs {
                on_both(curve, &p, &c);
            }
        }
        let far = plane(DVec3::new(5.0, 0.0, 0.0), DVec3::X);
        assert_eq!(surfaces(&far, &c, 1e-6), Ok(Ssi::None));
    }

    #[test]
    fn parallel_cylinders() {
        let a = cyl(DVec3::ZERO, DVec3::Z, 2.0);
        let b = cyl(DVec3::new(3.0, 0.0, 5.0), DVec3::Z, 2.0);
        let Ok(Ssi::Curves(cs)) = surfaces(&a, &b, 1e-6) else { panic!() };
        assert_eq!(cs.len(), 2);
        for c in &cs {
            on_both(c, &a, &b);
        }
        // Unequal radii crossing: two traced loops where the thin one goes through.
        let skew = cyl(DVec3::new(0.0, 0.0, 3.0), DVec3::X, 1.0);
        let Ok(Ssi::Curves(cs)) = surfaces(&a, &skew, 1e-6) else { panic!() };
        assert_eq!(cs.len(), 2);
        // Equal radii, axes meeting at a right angle: two ellipses.
        let cross = cyl(DVec3::new(-5.0, 0.0, 3.0), DVec3::X, 2.0);
        let Ok(Ssi::Curves(cs)) = surfaces(&a, &cross, 1e-6) else { panic!() };
        assert_eq!(cs.len(), 2);
        for c in &cs {
            on_both(c, &a, &cross);
        }
    }

    fn cone(half_angle: f64) -> Surface {
        Surface::Cone { frame: Frame::world(), radius: 2.0, half_angle }
    }

    #[test]
    fn plane_cone_cases() {
        let c = cone(0.4);
        // Across the axis: a circle.
        let p = plane(DVec3::new(0.0, 0.0, 1.5), DVec3::Z);
        let Ok(Ssi::Curves(cs)) = surfaces(&p, &c, 1e-6) else { panic!() };
        assert_eq!(cs.len(), 1);
        on_both(&cs[0], &p, &c);
        // Past the apex there is nothing.
        let below = plane(DVec3::new(0.0, 0.0, -10.0), DVec3::Z);
        assert_eq!(surfaces(&below, &c, 1e-6), Ok(Ssi::None));
        // Through the axis: two generators.
        let through = plane(DVec3::ZERO, DVec3::X);
        let Ok(Ssi::Curves(cs)) = surfaces(&through, &c, 1e-6) else { panic!() };
        assert_eq!(cs.len(), 2);
        for curve in &cs {
            for t in [0.5, 2.0, 5.0] {
                let p = curve.eval(t);
                assert!(c.distance(p) < 1e-9 && through.distance(p) < 1e-9);
            }
        }
        // An oblique plane cutting every generator: one closed traced curve.
        let oblique = plane(DVec3::ZERO, DVec3::new(1.0, 0.0, 1.0));
        let Ok(Ssi::Curves(cs)) = surfaces(&oblique, &c, 1e-6) else { panic!() };
        assert_eq!(cs.len(), 1);
        assert!(matches!(cs[0], Curve::Spline(_)));
    }

    #[test]
    fn coaxial_cone_cases() {
        let c = cone(0.4);
        let cy = cyl(DVec3::new(0.0, 0.0, 7.0), DVec3::NEG_Z, 3.0);
        let Ok(Ssi::Curves(cs)) = surfaces(&c, &cy, 1e-6) else { panic!() };
        assert_eq!(cs.len(), 1);
        on_both(&cs[0], &c, &cy);
        let other = Surface::Cone {
            frame: Frame { origin: DVec3::new(0.0, 0.0, 4.0), x: DVec3::X, y: DVec3::NEG_Y, z: DVec3::NEG_Z },
            radius: 2.5,
            half_angle: 0.2,
        };
        let Ok(Ssi::Curves(cs)) = surfaces(&c, &other, 1e-6) else { panic!() };
        assert_eq!(cs.len(), 1);
        on_both(&cs[0], &c, &other);
        assert_eq!(surfaces(&c, &c, 1e-6), Ok(Ssi::Coincident));
    }

    #[test]
    fn torus_cases() {
        let t = Surface::Torus { frame: Frame::world(), major: 6.0, minor: 1.5, v_mid: 0.0 };
        let on_torus = |cs: &[Curve], other: &Surface| {
            for c in cs {
                for i in 0..12 {
                    let p = c.eval(i as f64 * 0.5);
                    assert!(t.distance(p) < 1e-9 && other.distance(p) < 1e-9, "{p:?}");
                }
            }
        };
        // Across the axis through the tube: an outer and an inner circle.
        let across = plane(DVec3::new(0.0, 0.0, 0.5), DVec3::Z);
        let Ok(Ssi::Curves(cs)) = surfaces(&across, &t, 1e-6) else { panic!() };
        assert_eq!(cs.len(), 2);
        on_torus(&cs, &across);
        assert_eq!(surfaces(&plane(DVec3::new(0.0, 0.0, 3.0), DVec3::Z), &t, 1e-6), Ok(Ssi::None));
        // Through the axis: the tube's two cross-sections.
        let through = plane(DVec3::ZERO, DVec3::Y);
        let Ok(Ssi::Curves(cs)) = surfaces(&t, &through, 1e-6) else { panic!() };
        assert_eq!(cs.len(), 2);
        on_torus(&cs, &through);
        // A coaxial cylinder and cone through the tube.
        let c = cyl(DVec3::ZERO, DVec3::Z, 6.5);
        let Ok(Ssi::Curves(cs)) = surfaces(&c, &t, 1e-6) else { panic!() };
        assert_eq!(cs.len(), 2);
        on_torus(&cs, &c);
        let k = Surface::Cone { frame: Frame::world(), radius: 6.0, half_angle: 0.3 };
        let Ok(Ssi::Curves(cs)) = surfaces(&t, &k, 1e-6) else { panic!() };
        assert_eq!(cs.len(), 2);
        on_torus(&cs, &k);
        // Off-axis surfaces are not supported yet.
        assert!(surfaces(&t, &cyl(DVec3::new(6.0, 0.0, 0.0), DVec3::X, 0.5), 1e-6).is_err());
        assert!(surfaces(&t, &plane(DVec3::ZERO, DVec3::new(1.0, 0.0, 1.0)), 1e-6).is_err());
    }

    #[test]
    fn line_cone_hits() {
        let c = cone(0.4);
        let line = Curve::Line { origin: DVec3::new(-10.0, 0.3, 1.0), dir: DVec3::X };
        let CurveHit::Params(ts) = curve_surface(&line, 0.0, 20.0, &c, 1e-6) else { panic!() };
        assert_eq!(ts.len(), 2);
        for t in ts {
            assert!(c.distance(line.eval(t)) < 1e-9);
        }
        // A line along the axis meets only the cone's own nappe, at its apex.
        let axis = Curve::Line { origin: DVec3::new(0.0, 0.0, -20.0), dir: DVec3::Z };
        let CurveHit::Params(ts) = curve_surface(&axis, 0.0, 40.0, &c, 1e-6) else { panic!() };
        assert!(ts.len() <= 1);
    }

    #[test]
    fn curve_surface_hits() {
        let line = Curve::Line { origin: DVec3::new(-10.0, 0.5, 0.0), dir: DVec3::X };
        let c = cyl(DVec3::ZERO, DVec3::Z, 2.0);
        let CurveHit::Params(ts) = curve_surface(&line, 0.0, 20.0, &c, 1e-6) else { panic!() };
        assert_eq!(ts.len(), 2);
        let circle = Curve::Circle { frame: Frame::world(), radius: 3.0 };
        let p = plane(DVec3::new(1.0, 0.0, 0.0), DVec3::X);
        let CurveHit::Params(ts) = curve_surface(&circle, 0.0, TAU, &p, 1e-6) else { panic!() };
        assert_eq!(ts.len(), 2);
        for t in ts {
            assert!((circle.eval(t).x - 1.0).abs() < 1e-9);
        }
        // A circle crossing a cylinder of another axis, found numerically.
        let side = cyl(DVec3::new(3.0, 0.0, 0.0), DVec3::Z, 1.0);
        let CurveHit::Params(ts) = curve_surface(&circle, 0.0, TAU, &side, 1e-6) else { panic!() };
        assert_eq!(ts.len(), 2);
        let on = plane(DVec3::ZERO, DVec3::Z);
        assert_eq!(curve_surface(&circle, 0.0, TAU, &on, 1e-6), CurveHit::LiesOn);
    }
}
