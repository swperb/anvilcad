//! Neutral-plane draft: chosen faces tilt by an angle so a moulded part releases.
//!
//! The behaviour is specified in `docs/re/rib-draft-dome.md` (section "Neutral plane
//! draft" and testable statements 10 to 17). The neutral plane is a frame whose `z` is
//! the direction of pull. Each drafted face turns about its hinge, the line (or circle)
//! where it meets the neutral plane, until it makes the draft angle with the pull:
//!
//! - a plane square to the neutral plane becomes a plane through its hinge line, leaning
//!   by the angle;
//! - a cylinder, or a cone, whose axis lies along the pull becomes a cone through its
//!   circle in the neutral plane, with the angle as its half-angle.
//!
//! A positive angle closes the faces in on the body as they go in the direction of pull,
//! so the body narrows that way (the specification's decision for Reverse off); a
//! negative angle widens it (Reverse on).
//!
//! Neighbouring faces that are not drafted keep their surfaces and are trimmed or
//! extended to the moved ones. As in [`crate::offset::offset_faces`], each edge on a
//! drafted face is rebuilt where its two faces now meet (their intersection nearest the
//! old edge, or, where two drafted faces run smoothly into each other, the drafted
//! generator line), and each vertex is solved onto all of its faces and edges. A draft
//! makes no new faces: faces, edges and vertices keep their indices, tags and owners.
//! Faces the kernel cannot draft exactly, and drafts that would change the topology,
//! are refused.

use crate::geom::{Curve, Frame, Surface};
use crate::intersect::{self, Ssi};
use crate::offset::{settle, span, turning};
use crate::topo::{Edge, Solid};
use crate::{KernelError, Result, check, tol};
use glam::DVec3;

fn refuse<T>(msg: impl Into<String>) -> Result<T> {
    Err(KernelError::Unsupported(msg.into()))
}

fn invalid<T>(msg: impl Into<String>) -> Result<T> {
    Err(KernelError::InvalidInput(msg.into()))
}

/// Directions closer than this (as a sine) to square or to parallel count as such.
const SQUARE: f64 = 1e-9;

/// The solid with each face in `faces` drafted by `angle` radians about the plane of
/// `neutral`, pulling along `neutral.z`. A positive angle narrows the body in the
/// direction of pull, a negative one widens it; its size must be strictly between 0 and
/// a right angle. Faces, edges and vertices keep their indices, tags and owners; only
/// surfaces, curves and points change. The returned solid has passed
/// [`check::validate`].
pub fn draft_faces(solid: &Solid, faces: &[usize], neutral: &Frame, angle: f64) -> Result<Solid> {
    if !angle.is_finite() || angle == 0.0 || angle.abs() >= std::f64::consts::FRAC_PI_2 - 1e-12 {
        return invalid("a draft angle must be strictly between 0 and 90 degrees");
    }
    if faces.is_empty() {
        return invalid("no faces to draft");
    }
    if !tol::is_finite3(neutral.origin) || !tol::is_finite3(neutral.z) {
        return Err(KernelError::NonFinite);
    }
    let pull = neutral.z.try_normalize().ok_or(KernelError::InvalidFrame)?;
    let size = solid.bounds().map_or(1.0, |(lo, hi)| (hi - lo).length().max(1.0));
    let close = 1e-7 * size;

    let mut drafted = vec![false; solid.faces.len()];
    for &f in faces {
        let Some(slot) = drafted.get_mut(f) else {
            return invalid(format!("face {f} is not in the solid"));
        };
        *slot = true;
    }

    // Drafted surfaces.
    let mut out = solid.clone();
    for (f, face) in out.faces.iter_mut().enumerate() {
        if drafted[f] {
            face.surface = drafted_surface(&face.surface, face.reversed, neutral.origin, pull, angle)?;
        }
    }

    // The faces on each edge.
    let mut faces_of: Vec<Vec<usize>> = vec![Vec::new(); solid.edges.len()];
    for (fi, face) in solid.faces.iter().enumerate() {
        for c in face.loops().flat_map(|l| &l.coedges) {
            if let Some(list) = faces_of.get_mut(c.edge.index()) {
                list.push(fi);
            }
        }
    }

    // New curves for the edges on drafted faces, before their ends are known.
    let mut curves: Vec<Curve> = Vec::with_capacity(solid.edges.len());
    let mut changed = vec![false; solid.edges.len()];
    for (ei, e) in solid.edges.iter().enumerate() {
        let [fa, fb] = faces_of[ei][..] else {
            return Err(KernelError::Topology(format!("edge {ei} is not used by two faces")));
        };
        if !drafted[fa] && !drafted[fb] {
            curves.push(e.curve.clone());
            continue;
        }
        changed[ei] = true;
        let mid = e.curve.eval(0.5 * (e.t0 + e.t1));
        let (na, nb) = (solid.faces[fa].normal_at(mid), solid.faces[fb].normal_at(mid));
        let smooth = fa == fb || na.cross(nb).length() < 1e-6;
        let curve = if smooth {
            if !(drafted[fa] && drafted[fb]) {
                return refuse("a face that meets a drafted face smoothly must be drafted with it");
            }
            drafted_line(e, na, neutral.origin, pull, angle)?
        } else {
            let (sa, sb) = (&out.faces[fa].surface, &out.faces[fb].surface);
            let guess = settle(mid, [sa, sb].into_iter(), std::iter::empty());
            let Ssi::Curves(cs) = intersect::surfaces(sa, sb, tol::LINEAR)? else {
                return refuse("two drafted faces no longer meet in an edge");
            };
            cs.into_iter()
                .min_by(|x, y| x.distance_to_curve(guess).total_cmp(&y.distance_to_curve(guess)))
                .ok_or_else(|| KernelError::Unsupported("two drafted faces no longer meet".into()))?
        };
        curves.push(curve);
    }

    // New vertices: on every face and edge around them, where one of those faces moved.
    let mut around: Vec<(Vec<usize>, Vec<usize>)> = vec![(Vec::new(), Vec::new()); solid.vertices.len()];
    for (ei, e) in solid.edges.iter().enumerate() {
        for v in [e.start.index(), e.end.index()] {
            let Some((fs, es)) = around.get_mut(v) else {
                return Err(KernelError::Topology(format!("edge {ei} names a missing vertex")));
            };
            es.push(ei);
            fs.extend(faces_of[ei].iter().copied());
        }
    }
    let mut moved_vertex = vec![false; solid.vertices.len()];
    for (vi, (fs, es)) in around.iter_mut().enumerate() {
        fs.sort_unstable();
        fs.dedup();
        es.sort_unstable();
        es.dedup();
        if !fs.iter().any(|&f| drafted[f]) {
            continue;
        }
        moved_vertex[vi] = true;
        let x = settle(
            solid.vertices[vi].point,
            fs.iter().map(|&f| &out.faces[f].surface),
            es.iter().map(|&e| &curves[e]),
        );
        let off_face = fs.iter().map(|&f| out.faces[f].surface.distance(x)).fold(0.0, f64::max);
        let off_edge = es.iter().map(|&e| curves[e].distance_to_curve(x)).fold(0.0, f64::max);
        if off_face > close || off_edge > close {
            return refuse("a corner of the drafted faces does not meet (the draft changes the shape)");
        }
        out.vertices[vi].point = x;
    }

    // Edges between their new ends, running the same way as before.
    for (ei, e) in solid.edges.iter().enumerate() {
        if !changed[ei] && !moved_vertex[e.start.index()] && !moved_vertex[e.end.index()] {
            continue;
        }
        let c = curves[ei].clone();
        let (a, b) = (out.vertices[e.start.index()].point, out.vertices[e.end.index()].point);
        // An open edge must keep running the same way: a draft so steep that faces
        // cross turns an edge round (or shrinks it to nothing).
        if e.start != e.end {
            let (oa, ob) = (solid.vertices[e.start.index()].point, solid.vertices[e.end.index()].point);
            if (b - a).dot(ob - oa) <= close * (ob - oa).length() {
                return refuse("an edge turns round or vanishes (the draft angle is too large for the part)");
            }
        }
        let old_mid = e.curve.eval(0.5 * (e.t0 + e.t1));
        let mid = c.eval(c.param_of(old_mid));
        let (t0, t1) = span(&c, a, b, e, mid)?;
        out.edges[ei] = Edge { curve: c, start: e.start, end: e.end, t0, t1 };
    }

    // A cone is framed at its narrow end, opening along its axis.
    for (f, face) in out.faces.iter_mut().enumerate() {
        if drafted[f]
            && let Surface::Cone { frame, radius, half_angle, apex: false } = face.surface
        {
            let lowest = face
                .loops()
                .flat_map(|l| &l.coedges)
                .filter_map(|c| out.edges.get(c.edge.index()))
                .flat_map(|e| (0..=8).map(move |k| e.curve.eval(e.t0 + (e.t1 - e.t0) * k as f64 / 8.0)))
                .map(|p| (p - frame.origin).dot(frame.z))
                .fold(f64::INFINITY, f64::min);
            if !lowest.is_finite() {
                continue;
            }
            let r = radius + lowest * half_angle.tan();
            if r <= tol::LINEAR {
                return refuse("a drafted round face would reach its cone's tip");
            }
            face.surface = Surface::Cone {
                frame: frame.with_origin(frame.origin + frame.z * lowest),
                radius: r,
                half_angle,
                apex: false,
            };
        }
    }

    check::validate(&out)?;
    // The faces must not have turned over: each keeps its orientation round its edges.
    for (f, (old, new)) in solid.faces.iter().zip(&out.faces).enumerate() {
        if let (Some(a), Some(b)) = (turning(solid, f, &old.surface), turning(&out, f, &new.surface))
            && a * b < 0.0
        {
            return refuse("a face turns inside out (the draft angle is too large for the part)");
        }
    }
    Ok(out)
}

/// The drafted surface of a face lying on `s` (outward normal opposite the natural one
/// when `reversed`), hinged on the plane through `origin` across `pull`. The natural
/// normal keeps its side, so the face's `reversed` flag still holds.
fn drafted_surface(s: &Surface, reversed: bool, origin: DVec3, pull: DVec3, angle: f64) -> Result<Surface> {
    let out_sign = if reversed { -1.0 } else { 1.0 };
    let (sin, cos) = angle.sin_cos();
    // Height above the neutral plane.
    let height = |p: DVec3| (p - origin).dot(pull);
    match *s {
        Surface::Plane { frame } => {
            let n = frame.z * out_sign;
            let along = n.dot(pull);
            if along.abs() > 1.0 - SQUARE {
                return invalid("a face parallel to the neutral plane cannot be drafted");
            }
            if along.abs() > SQUARE {
                return refuse("drafting a plane that is not square to the neutral plane");
            }
            // The hinge: the plane's origin slid along the pull onto the neutral plane.
            let hinge = frame.origin - pull * height(frame.origin);
            let tilted = n * cos + pull * sin;
            Ok(Surface::Plane { frame: Frame::new(hinge, tilted * out_sign, frame.x)? })
        }
        Surface::Cylinder { frame, radius } => cone_about(&frame, radius, 0.0, reversed, origin, pull, angle),
        Surface::Cone { frame, radius, half_angle, apex: false } => {
            cone_about(&frame, radius, half_angle, reversed, origin, pull, angle)
        }
        Surface::Cone { apex: true, .. } => refuse("drafting a pointed cone"),
        Surface::Sphere { .. } | Surface::Torus { .. } | Surface::BSpline(_) => {
            refuse("drafting faces other than planes, cylinders and cones")
        }
    }
}

/// The cone through the circle where the round surface (a cylinder when `half_angle`
/// is 0) about `frame.z` meets the neutral plane, making `angle` with the pull.
fn cone_about(
    frame: &Frame,
    radius: f64,
    half_angle: f64,
    reversed: bool,
    origin: DVec3,
    pull: DVec3,
    angle: f64,
) -> Result<Surface> {
    let dot = frame.z.dot(pull);
    if dot.abs() < 1.0 - SQUARE {
        return refuse("drafting a round face whose axis is not along the pull");
    }
    // Where the axis crosses the neutral plane, and the radius there.
    let t = -(frame.origin - origin).dot(pull) / dot;
    let centre = frame.origin + frame.z * t;
    let r = radius + t * half_angle.tan();
    if r <= tol::LINEAR {
        return refuse("a round face does not reach the neutral plane");
    }
    // Outward normal h cos(angle) + pull sin(angle), with h the outward radial direction;
    // the natural normal radial cos(a) - z sin(a) of a cone along z matches it (up to the
    // face's side) when z = -side * sign(angle) * pull.
    let side = if reversed { -1.0 } else { 1.0 };
    let z = pull * (-side * angle.signum());
    let x = (frame.x - z * frame.x.dot(z)).try_normalize().ok_or(KernelError::InvalidFrame)?;
    Ok(Surface::Cone {
        frame: Frame { origin: centre, x, y: z.cross(x), z },
        radius: r,
        half_angle: angle.abs(),
        apex: false,
    })
}

/// The drafted image of a straight edge rising across the neutral plane on a drafted
/// face (or between two drafted faces meeting smoothly) whose outward normal there is
/// `n`, the edge lying in the plane of `n` and the pull (a generator of a cylinder or
/// cone, or a line up a plane): the generator through the edge's hinge point, leaning
/// by the angle.
fn drafted_line(e: &Edge, n: DVec3, origin: DVec3, pull: DVec3, angle: f64) -> Result<Curve> {
    let not_rising =
        || refuse("drafting a smooth or seam edge that does not rise straight from the neutral plane");
    let Curve::Line { origin: o, dir } = e.curve else {
        return not_rising();
    };
    let Some(h) = (n - pull * n.dot(pull)).try_normalize() else {
        return invalid("a face parallel to the neutral plane cannot be drafted");
    };
    let rise = dir.dot(pull);
    if rise.abs() < SQUARE || dir.dot(pull.cross(h)).abs() > SQUARE {
        return not_rising();
    }
    let hinge = o - dir * ((o - origin).dot(pull) / rise);
    let (sin, cos) = angle.sin_cos();
    let lean = pull * cos - h * sin;
    // Keep the old direction of travel.
    let lean = if rise < 0.0 { -lean } else { lean };
    Ok(Curve::Line { origin: hinge, dir: lean })
}

#[cfg(test)]
mod tests;
