//! Offsetting a solid's faces, keeping its shape: the basis of shells.
//!
//! Each face moves along its outward normal by its own distance, onto an exact offset
//! surface: a plane moves, a cylinder, sphere or torus tube changes radius, a cone keeps
//! its axis and angle and widens. Each edge is rebuilt where its two moved faces meet:
//! their intersection curve nearest the old edge, or, where the faces are tangent (a
//! fillet running into a flat) or the edge is a seam, the old edge carried along the
//! faces' normal. Each vertex is solved onto all of its moved faces and edges. Faces,
//! edges and vertices keep their places in the solid, so the result has the same
//! topology; an offset that would change it (a round smaller than the offset on its
//! convex side, a feature thinner than the offset, faces drifting apart) is refused.
//!
//! The behaviour is specified in `docs/re/shell-and-fillet.md`.

use crate::freeform::signed;
use crate::geom::{Curve, Frame, Surface};
use crate::intersect::{self, Ssi};
use crate::topo::{Edge, Solid};
use crate::{KernelError, Result, check, tol};
use glam::DVec3;
use std::f64::consts::TAU;

fn refuse<T>(msg: impl Into<String>) -> Result<T> {
    Err(KernelError::Unsupported(msg.into()))
}

/// The solid with face `f` moved `distance[f]` along its outward normal (negative moves
/// it inwards). Faces, edges and vertices keep their indices.
pub fn offset_faces(solid: &Solid, distance: &[f64]) -> Result<Solid> {
    if distance.len() != solid.faces.len() || distance.iter().any(|d| !d.is_finite()) {
        return Err(KernelError::InvalidInput("one finite offset per face is needed".into()));
    }
    let size = solid.bounds().map_or(1.0, |(lo, hi)| (hi - lo).length().max(1.0));
    let close = 1e-7 * size;

    // Moved surfaces.
    let mut out = solid.clone();
    for (f, face) in out.faces.iter_mut().enumerate() {
        let s = if face.reversed { -distance[f] } else { distance[f] };
        face.surface = moved(&face.surface, s)?;
    }

    // The faces on each edge.
    let mut faces_of: Vec<Vec<usize>> = vec![Vec::new(); solid.edges.len()];
    for (fi, face) in solid.faces.iter().enumerate() {
        for c in face.loops().flat_map(|l| &l.coedges) {
            faces_of[c.edge.index()].push(fi);
        }
    }

    // New curves, before their ends are known.
    let mut curves: Vec<Curve> = Vec::with_capacity(solid.edges.len());
    for (ei, e) in solid.edges.iter().enumerate() {
        let fs = &faces_of[ei];
        let [fa, fb] = fs[..] else {
            return Err(KernelError::Topology(format!("edge {ei} is not used by two faces")));
        };
        let mid = e.curve.eval(0.5 * (e.t0 + e.t1));
        let (na, nb) = (solid.faces[fa].normal_at(mid), solid.faces[fb].normal_at(mid));
        let tangent = fa == fb || na.cross(nb).length() < 1e-6;
        let curve = if tangent {
            if (distance[fa] - distance[fb]).abs() > close {
                return refuse("faces that meet smoothly would be offset by different amounts");
            }
            carried(e, &solid.faces[fa], distance[fa])?
        } else {
            let guess = mid + na * distance[fa] + nb * distance[fb];
            let Ssi::Curves(cs) =
                intersect::surfaces(&out.faces[fa].surface, &out.faces[fb].surface, tol::LINEAR)?
            else {
                return refuse("two offset faces no longer meet");
            };
            cs.into_iter()
                .min_by(|x, y| x.distance_to_curve(guess).total_cmp(&y.distance_to_curve(guess)))
                .ok_or_else(|| KernelError::Unsupported("two offset faces no longer meet".into()))?
        };
        curves.push(curve);
    }

    // New vertices: on every moved face and edge around them.
    let mut around: Vec<(Vec<usize>, Vec<usize>)> = vec![(Vec::new(), Vec::new()); solid.vertices.len()];
    for (ei, e) in solid.edges.iter().enumerate() {
        for v in [e.start.index(), e.end.index()] {
            around[v].1.push(ei);
            around[v].0.extend(faces_of[ei].iter().copied());
        }
    }
    for (vi, (faces, edges)) in around.iter_mut().enumerate() {
        faces.sort_unstable();
        faces.dedup();
        edges.dedup();
        let p = solid.vertices[vi].point;
        let guess = p + faces
            .iter()
            .map(|&f| solid.faces[f].normal_at(p) * distance[f])
            .fold(DVec3::ZERO, |a, b| a + b)
            / (faces.len().max(1) as f64).sqrt();
        let x =
            settle(guess, faces.iter().map(|&f| &out.faces[f].surface), edges.iter().map(|&e| &curves[e]));
        let off_face = faces.iter().map(|&f| signed(&out.faces[f].surface, x).abs()).fold(0.0, f64::max);
        let off_edge = edges.iter().map(|&e| curves[e].distance_to_curve(x)).fold(0.0, f64::max);
        if off_face > close || off_edge > close {
            return refuse("a corner of the offset faces does not meet (the offset changes the shape)");
        }
        out.vertices[vi].point = x;
    }

    // Edges between their new ends, running the same way as before.
    for (ei, e) in solid.edges.iter().enumerate() {
        let c = curves[ei].clone();
        let (a, b) = (out.vertices[e.start.index()].point, out.vertices[e.end.index()].point);
        let old_mid = e.curve.eval(0.5 * (e.t0 + e.t1));
        let mid = old_mid
            + faces_of[ei]
                .iter()
                .map(|&f| solid.faces[f].normal_at(old_mid) * distance[f])
                .fold(DVec3::ZERO, |x, y| x + y)
                / (faces_of[ei].len() as f64).sqrt();
        let (t0, t1) = span(&c, a, b, e, mid)?;
        out.edges[ei] = Edge { curve: c, start: e.start, end: e.end, t0, t1 };
    }

    check::validate(&out)?;
    // The faces must not have turned over: each keeps its orientation round its edges.
    for (f, (old, new)) in solid.faces.iter().zip(&out.faces).enumerate() {
        if let (Some(a), Some(b)) = (turning(solid, f, &old.surface), turning(&out, f, &new.surface))
            && a * b < 0.0
        {
            return refuse(
                "an offset face turns inside out (the offset is larger than a round or a feature)",
            );
        }
    }
    Ok(out)
}

/// `s` moved `d` along its natural normal.
fn moved(s: &Surface, d: f64) -> Result<Surface> {
    let positive = |r: f64| {
        if r > tol::LINEAR { Ok(r) } else { refuse("the offset is larger than a curved face's radius") }
    };
    Ok(match *s {
        Surface::Plane { frame } => Surface::Plane { frame: frame.with_origin(frame.origin + frame.z * d) },
        Surface::Cylinder { frame, radius } => Surface::Cylinder { frame, radius: positive(radius + d)? },
        Surface::Sphere { frame, radius } => Surface::Sphere { frame, radius: positive(radius + d)? },
        Surface::Torus { frame, major, minor, v_mid } => {
            Surface::Torus { frame, major, minor: positive(minor + d)?, v_mid }
        }
        Surface::Cone { frame, radius, half_angle, apex } => {
            // The normal leans back from the axis by the half angle, so the radius at the
            // frame's origin grows by d / cos.
            let r = radius + d / half_angle.cos();
            if apex && r <= tol::LINEAR {
                return refuse("the offset passes a cone's tip");
            }
            Surface::Cone { frame, radius: positive(r)?, half_angle, apex }
        }
        Surface::BSpline(_) => return refuse("offsets of freeform faces"),
    })
}

/// An edge carried `d` along the outward normal of `face` (a seam, or an edge between
/// faces meeting smoothly): a line stays a line, a circle a circle.
fn carried(e: &Edge, face: &crate::topo::Face, d: f64) -> Result<Curve> {
    let at = |t: f64| {
        let p = e.curve.eval(t);
        p + face.normal_at(p) * d
    };
    let check = |c: &Curve, ts: &[f64]| {
        ts.iter().all(|t| c.distance_to_curve(at(*t)) < 1e-9 * (1.0 + at(*t).length()))
    };
    match e.curve {
        Curve::Line { dir, .. } => {
            let c = Curve::Line { origin: at(e.t0), dir };
            if check(&c, &[e.t1, 0.5 * (e.t0 + e.t1)]) {
                Ok(c)
            } else {
                refuse("an offset edge is not straight")
            }
        }
        Curve::Circle { frame, .. } => {
            let (p, q, r) = (at(0.0), at(TAU / 3.0), at(2.0 * TAU / 3.0));
            let c = circle_through(p, q, r, &frame)
                .ok_or_else(|| KernelError::Unsupported("an offset edge is not round".into()))?;
            if check(&c, &[1.0, 2.5, 4.0, 5.5]) { Ok(c) } else { refuse("an offset edge is not round") }
        }
        _ => refuse("offsets along curved edges other than lines and circles"),
    }
}

/// The circle through three points, with its frame turned like `like`.
fn circle_through(p: DVec3, q: DVec3, r: DVec3, like: &Frame) -> Option<Curve> {
    let (a, b) = (q - p, r - p);
    let n = a.cross(b);
    let nn = n.length_squared();
    if nn < 1e-24 {
        return None;
    }
    let center = p + (b * a.length_squared() - a * b.length_squared()).cross(n) * (0.5 / nn);
    let z = if n.dot(like.z) < 0.0 { -n.normalize() } else { n.normalize() };
    let x = (like.x - z * like.x.dot(z)).try_normalize()?;
    Some(Curve::Circle {
        frame: Frame { origin: center, x, y: z.cross(x), z },
        radius: (p - center).length(),
    })
}

/// The point near `x` on all the surfaces and curves: Gauss-Newton on the surfaces'
/// signed distances (damped, so meeting tangentially it moves no further than needed),
/// then alternating projections onto the curves.
fn settle<'a>(
    mut x: DVec3,
    surfaces: impl Iterator<Item = &'a Surface> + Clone,
    curves: impl Iterator<Item = &'a Curve> + Clone,
) -> DVec3 {
    for _ in 0..50 {
        // Normal equations (J^T J + lambda) dx = -J^T r.
        let mut m = glam::DMat3::ZERO;
        let mut g = DVec3::ZERO;
        for s in surfaces.clone() {
            let n = s.normal_at(x);
            let r = signed(s, x);
            m += glam::DMat3::from_cols(n * n.x, n * n.y, n * n.z);
            g += n * r;
        }
        let lambda = 1e-9 * (1.0 + m.x_axis.x + m.y_axis.y + m.z_axis.z);
        let m = m + glam::DMat3::from_diagonal(DVec3::splat(lambda));
        if m.determinant().abs() < 1e-300 {
            break;
        }
        let dx = -(m.inverse() * g);
        x += dx;
        if dx.length() < 1e-15 * (1.0 + x.length()) {
            break;
        }
    }
    let cs: Vec<&Curve> = curves.collect();
    if !cs.is_empty() {
        for _ in 0..100 {
            let next =
                cs.iter().map(|c| c.eval(c.param_of(x))).fold(DVec3::ZERO, |a, b| a + b) / cs.len() as f64;
            let moved = (next - x).length();
            x = next;
            if moved < 1e-15 * (1.0 + x.length()) {
                break;
            }
        }
    }
    x
}

/// Parameters on `c` from `a` to `b`, running the way the old edge `e` ran (past the
/// point near `mid` for an open edge; round the circle the same way for a closed one).
fn span(c: &Curve, a: DVec3, b: DVec3, e: &Edge, mid: DVec3) -> Result<(f64, f64)> {
    let t0 = c.param_of(a);
    let t1 = c.param_of(b);
    let Some(period) = c.period() else { return Ok((t0, t1)) };
    if e.start == e.end {
        // A whole loop: go round the way the old edge went, judged by the direction
        // from its start towards its middle.
        let ahead = e.curve.eval(e.t0 + (e.t1 - e.t0) * 0.25) - e.curve.eval(e.t0);
        let h = period * 1e-3;
        let dir = c.eval(t0 + h) - c.eval(t0);
        return Ok(if dir.dot(ahead) >= 0.0 { (t0, t0 + period) } else { (t0, t0 - period) });
    }
    let best = [-period, 0.0, period]
        .into_iter()
        .map(|k| t1 + k)
        .filter(|t| (t - t0).abs() < period)
        .min_by(|x, y| {
            let d = |t: f64| (c.eval(0.5 * (t0 + t)) - mid).length();
            d(*x).total_cmp(&d(*y))
        })
        .ok_or_else(|| KernelError::Topology("an offset edge has no span".into()))?;
    Ok((t0, best))
}

/// The signed area round a face's outer loop in its surface's layout, or `None` where
/// that is not meaningful (a loop going round a periodic surface).
fn turning(solid: &Solid, f: usize, surface: &Surface) -> Option<f64> {
    let face = solid.faces.get(f)?;
    let period = surface.u_period();
    let mut uv: Vec<glam::DVec2> = Vec::new();
    for c in &face.outer.coedges {
        let e = solid.edge(c.edge)?;
        for k in 0..16 {
            let s = k as f64 / 16.0;
            let s = if c.reversed { 1.0 - s } else { s };
            let mut q = surface.uv(e.curve.eval(e.t0 + (e.t1 - e.t0) * s));
            if let (Some(per), Some(last)) = (period, uv.last()) {
                q.x += per * ((last.x - q.x) / per).round();
            }
            uv.push(q);
        }
    }
    if let (Some(per), Some(first), Some(last)) = (period, uv.first(), uv.last())
        && (first.x - last.x).abs() > 0.5 * per
    {
        return None;
    }
    let area: f64 = (0..uv.len()).map(|i| uv[i].perp_dot(uv[(i + 1) % uv.len()])).sum();
    (area.abs() > 1e-12).then_some(area)
}

#[cfg(test)]
mod tests;
