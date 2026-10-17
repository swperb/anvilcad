//! Revolution of a planar region about an axis in its plane into a solid.
//!
//! A profile of lines and arcs sweeps surfaces the kernel already has: a line across
//! the axis sweeps a plane, one along it a cylinder, a slanted one a cone (pointed where
//! it meets the axis); an arc centred on the axis sweeps a sphere, any other arc a
//! torus. The solid is built like an extrusion whose path is a circle: every profile
//! vertex off the axis sweeps a circle (an arc when the turn is partial), and a partial
//! turn has the profile itself as a flat start and end cap. Points on the axis stay
//! put. A full turn closes the periodic faces (cylinders, cones, tori) with a seam at
//! the profile; planes, spheres and pointed cones are laid out without one and are
//! bounded by their circles alone.

use crate::extrude::segment_curve;
use crate::geom::{Curve, Frame, Surface};
use crate::profile::{Loop2, Region, Segment, build_regions};
use crate::topo::{Coedge, Edge, EdgeId, Face, FaceTag, Loop, Solid, VertexId};
use crate::{KernelError, Result, check, tol};
use glam::DVec2;
use std::f64::consts::{PI, TAU};

/// Revolves `region` (in the 2D coordinates of `frame`) about the line through
/// `axis_point` along `axis_dir` (also in those coordinates) by `angle` radians. A
/// positive angle turns the profile towards `frame.z`; a full turn (or more) has no
/// caps. The region must lie on one side of the axis, touching it at most. The
/// returned solid has passed [`check::validate`].
pub fn revolve(
    frame: &Frame,
    region: &Region,
    axis_point: DVec2,
    axis_dir: DVec2,
    angle: f64,
) -> Result<Solid> {
    if !angle.is_finite() || angle.abs() < 1e-9 {
        return Err(KernelError::InvalidDistance);
    }
    let d = axis_dir
        .try_normalize()
        .ok_or_else(|| KernelError::InvalidInput("the axis has no direction".into()))?;
    let full = angle.abs() >= TAU - 1e-9;
    let theta = if full { TAU } else { angle.abs() };

    // Which side of the axis the profile lies on, and how far it reaches.
    let loops: Vec<&Loop2> = std::iter::once(&region.outer).chain(&region.holes).collect();
    let across = |p: DVec2| d.perp_dot(p - axis_point);
    let mut side = 0.0f64;
    let mut reach = 0.0f64;
    for l in &loops {
        for s in &l.segments {
            for p in segment_extremes(s, d) {
                let r = across(p);
                reach = reach.max(r.abs()).max((p - axis_point).length());
                if r.abs() > 1e-9 * (1.0 + reach) {
                    if side == 0.0 {
                        side = r.signum();
                    } else if r.signum() != side {
                        return Err(KernelError::InvalidInput(
                            "the profile crosses the axis of revolution".into(),
                        ));
                    }
                }
            }
        }
    }
    if side == 0.0 {
        return Err(KernelError::EmptyProfile);
    }
    if full && !region.holes.is_empty() {
        return Err(KernelError::Unsupported(
            "a hole in a profile turned all the way round would leave a sealed hollow".into(),
        ));
    }
    let on_axis = 1e-9 * (1.0 + reach);

    // The axis frame: z along the axis, x towards the profile at angle 0, y the way the
    // turn goes (towards `frame.z` for a positive angle).
    let x = (frame.x * (-d.y) + frame.y * d.x) * side;
    let y = frame.z * angle.signum();
    let axis = Frame { origin: frame.to_world(axis_point), x, y, z: x.cross(y) };

    // The profile in the meridian plane: (height along the axis, distance from it). As
    // a frame (x along the axis, y away from it, z the way the turn goes) it is the
    // "bottom" of an extrusion along the turn.
    let meridian = Frame { origin: axis.origin, x: axis.z, y: axis.x, z: axis.y };
    let to_q = |p: DVec2| {
        let w = frame.to_world(p) - axis.origin;
        let q = DVec2::new(w.dot(axis.z), w.dot(axis.x));
        DVec2::new(q.x, if q.y.abs() <= on_axis { 0.0 } else { q.y })
    };
    let mapped: Vec<Loop2> = loops
        .iter()
        .map(|l| {
            let mut segs = Vec::new();
            let mut tags = Vec::new();
            for (k, s) in l.segments.iter().enumerate() {
                for piece in meridian_segment(s, &to_q, full, on_axis) {
                    segs.push(piece);
                    tags.push(l.tag(k));
                }
            }
            Loop2::with_tags(segs, tags)
        })
        .collect();
    // Counter-clockwise outer loop and clockwise holes in the meridian plane, material
    // on the left of travel.
    let regions = build_regions(&mapped)?;
    let [region] = regions.as_slice() else {
        return Err(KernelError::InvalidInput("the profile is not one region".into()));
    };
    let turned = |f: &Frame, a: f64| -> Frame {
        let (c, s) = (a.cos(), a.sin());
        Frame { origin: f.origin, x: f.x, y: axis.x * c + axis.y * s, z: axis.y * c - axis.x * s }
    };
    let end_plane = turned(&meridian, theta);

    let mut solid = Solid::default();
    let contours: Vec<&Loop2> = std::iter::once(&region.outer).chain(region.holes.iter()).collect();
    struct Ring {
        /// Profile edges at angle 0 and at the end of the turn (the same edge for a
        /// segment on the axis, or for a seam).
        start: Vec<Option<EdgeId>>,
        end: Vec<Option<EdgeId>>,
        /// The circle (arc) each vertex sweeps, if it is off the axis.
        sweep: Vec<Option<EdgeId>>,
    }
    let zero = |y: f64| y.abs() <= on_axis;
    let axis_line =
        |seg: &Segment| matches!(seg, Segment::Line { .. }) && zero(seg.start().y) && zero(seg.end().y);
    let mut rings = Vec::with_capacity(contours.len());
    for contour in &contours {
        let n = contour.segments.len();
        let q: Vec<DVec2> = contour.segments.iter().map(Segment::start).collect();
        let at = |q: DVec2, a: f64| axis.origin + axis.z * q.x + (axis.x * a.cos() + axis.y * a.sin()) * q.y;
        // A full turn uses no point on the axis (pointed faces have no vertex at their
        // tip), so it makes none.
        let v0: Vec<Option<VertexId>> =
            q.iter().map(|p| (!full || !zero(p.y)).then(|| solid.add_vertex(at(*p, 0.0)))).collect();
        let v1: Vec<Option<VertexId>> = q
            .iter()
            .zip(&v0)
            .map(|(p, v)| if full || zero(p.y) { *v } else { Some(solid.add_vertex(at(*p, theta))) })
            .collect();
        let vertex = |v: Option<VertexId>| {
            v.ok_or_else(|| KernelError::Topology("a profile edge ends on the axis".into()))
        };
        let mut sweep: Vec<Option<EdgeId>> = Vec::with_capacity(n);
        for k in 0..n {
            sweep.push(if zero(q[k].y) {
                None
            } else {
                let (a, b) = (vertex(v0[k])?, vertex(v1[k])?);
                Some(solid.add_edge(Edge {
                    curve: Curve::Circle {
                        frame: axis.with_origin(axis.origin + axis.z * q[k].x),
                        radius: q[k].y,
                    },
                    start: a,
                    end: b,
                    t0: 0.0,
                    t1: theta,
                }))
            });
        }
        let mut start = Vec::with_capacity(n);
        let mut end = Vec::with_capacity(n);
        for (k, seg) in contour.segments.iter().enumerate() {
            let next = (k + 1) % n;
            let on = axis_line(seg);
            // A full turn needs the profile edge only as the seam of a periodic face.
            let needed = !full || (!on && periodic(&side_surface(&axis, seg, theta, full, on_axis)?));
            if !needed {
                start.push(None);
                end.push(None);
                continue;
            }
            let (c0, t0, t1) = segment_curve(&meridian, seg);
            let e0 =
                solid.add_edge(Edge { curve: c0, start: vertex(v0[k])?, end: vertex(v0[next])?, t0, t1 });
            let e1 = if full || on {
                e0
            } else {
                let (c1, t0, t1) = segment_curve(&end_plane, seg);
                solid.add_edge(Edge { curve: c1, start: vertex(v1[k])?, end: vertex(v1[next])?, t0, t1 })
            };
            start.push(Some(e0));
            end.push(Some(e1));
        }
        rings.push(Ring { start, end, sweep });
    }

    let fwd = |edge| Coedge { edge, reversed: false };
    let rev = |edge| Coedge { edge, reversed: true };

    if !full {
        // End cap: the profile's orientation matches its outward normal, the way the
        // turn goes. The start cap faces back, so its loops run backwards.
        let mut ends =
            rings.iter().map(|r| Loop { coedges: r.end.iter().filter_map(|e| e.map(fwd)).collect() });
        solid.add_face(Face {
            surface: Surface::Plane { frame: end_plane },
            reversed: false,
            outer: ends.next().unwrap_or_default(),
            inners: ends.collect(),
            tag: FaceTag::EndCap,
            owner: 0,
        });
        let mut starts =
            rings.iter().map(|r| Loop { coedges: r.start.iter().rev().filter_map(|e| e.map(rev)).collect() });
        solid.add_face(Face {
            surface: Surface::Plane { frame: meridian },
            reversed: true,
            outer: starts.next().unwrap_or_default(),
            inners: starts.collect(),
            tag: FaceTag::StartCap,
            owner: 0,
        });
    }

    // Side faces, one per segment off the axis. Material is on the left of the
    // profile's travel, so the outward normal is on its right.
    for (ci, (contour, r)) in contours.iter().zip(&rings).enumerate() {
        let n = contour.segments.len();
        for (k, seg) in contour.segments.iter().enumerate() {
            let next = (k + 1) % n;
            if axis_line(seg) {
                continue;
            }
            let surface = side_surface(&axis, seg, theta, full, on_axis)?;
            let (outer, inners) = if full && !periodic(&surface) {
                // Bounded by its circles alone: the one farther out in the face's layout
                // is the outer loop.
                let mut loops: Vec<(f64, Loop)> = Vec::new();
                if let Some(e) = r.sweep[next] {
                    loops.push((layout_reach(&surface, &solid, e), Loop { coedges: vec![fwd(e)] }));
                }
                if let Some(e) = r.sweep[k] {
                    loops.push((layout_reach(&surface, &solid, e), Loop { coedges: vec![rev(e)] }));
                }
                loops.sort_by(|a, b| b.0.total_cmp(&a.0));
                let mut it = loops.into_iter().map(|l| l.1);
                (it.next().unwrap_or_default(), it.collect())
            } else {
                let mut coedges = Vec::with_capacity(4);
                coedges.extend(r.start[k].map(fwd));
                coedges.extend(r.sweep[next].map(fwd));
                coedges.extend(r.end[k].map(rev));
                coedges.extend(r.sweep[k].map(rev));
                (Loop { coedges }, Vec::new())
            };
            // Outward: right of the travel at the segment's middle, turned half way.
            let (mid, travel) = midpoint_and_tangent(seg);
            let half = if full { 0.0 } else { 0.5 * theta };
            let place = turned(&meridian, half);
            let p = place.to_world(mid);
            let out = place.to_world(DVec2::new(travel.y, -travel.x)) - place.origin;
            let reversed = surface.normal_at(p).dot(out) < 0.0;
            solid.add_face(Face {
                surface,
                reversed,
                outer,
                inners,
                tag: FaceTag::Side { contour: ci as u32, segment: contour.tag(k) },
                owner: 0,
            });
        }
    }

    check::validate(&solid)?;
    Ok(solid)
}

/// Revolves every region of a profile, one solid per region.
pub fn revolve_regions(
    frame: &Frame,
    regions: &[Region],
    axis_point: DVec2,
    axis_dir: DVec2,
    angle: f64,
) -> Result<Vec<Solid>> {
    regions.iter().map(|r| revolve(frame, r, axis_point, axis_dir, angle)).collect()
}

/// Points of a segment that bound how far it goes from the axis: its ends, and for an
/// arc the points of its circle square to `d` that it passes through.
fn segment_extremes(s: &Segment, d: DVec2) -> Vec<DVec2> {
    let mut out = vec![s.start(), s.end()];
    if let Segment::Arc { center, radius, start, sweep } = *s {
        let base = d.y.atan2(d.x) + 0.5 * PI;
        for k in 0..4 {
            let a = base + 0.5 * PI * k as f64;
            let rel = if sweep >= 0.0 { (a - start).rem_euclid(TAU) } else { (start - a).rem_euclid(TAU) };
            if rel <= sweep.abs() {
                out.push(center + DVec2::from_angle(a) * radius);
            }
        }
    }
    out
}

/// A segment carried into the meridian plane by `to_q` (an isometry, possibly a
/// mirror). Some arcs are split in two: a whole circle, whose torus would wrap round
/// its tube as well as its axis (the layout wraps only round the axis), and for a full
/// turn an arc centred on the axis that runs from it back to it, since a sphere face
/// needs a boundary.
fn meridian_segment(s: &Segment, to_q: &dyn Fn(DVec2) -> DVec2, full: bool, on_axis: f64) -> Vec<Segment> {
    match *s {
        Segment::Line { a, b } => vec![Segment::Line { a: to_q(a), b: to_q(b) }],
        Segment::Arc { center, radius, start, sweep } => {
            let c = to_q(center);
            // The image of the direction at angle `a`, as an angle.
            let o = to_q(center + DVec2::X) - c;
            let o2 = to_q(center + DVec2::Y) - c;
            let mirrored = o.perp_dot(o2) < 0.0;
            let image = |a: f64| {
                let v = o * a.cos() + o2 * a.sin();
                v.y.atan2(v.x)
            };
            let (start, sweep) = (image(start), if mirrored { -sweep } else { sweep });
            let c = DVec2::new(c.x, if c.y.abs() <= on_axis { 0.0 } else { c.y });
            let arc = |start: f64, sweep: f64| Segment::Arc { center: c, radius, start, sweep };
            let ends_on_axis = |seg: &Segment| seg.start().y.abs() <= on_axis && seg.end().y.abs() <= on_axis;
            let whole = arc(start, sweep);
            let pole_to_pole = full && c.y == 0.0 && ends_on_axis(&whole);
            if sweep.abs() >= TAU - 1e-9 || pole_to_pole {
                vec![arc(start, 0.5 * sweep), arc(start + 0.5 * sweep, 0.5 * sweep)]
            } else {
                vec![whole]
            }
        }
    }
}

/// The middle of a segment and its unit direction of travel there.
fn midpoint_and_tangent(s: &Segment) -> (DVec2, DVec2) {
    match *s {
        Segment::Line { a, b } => (0.5 * (a + b), (b - a).normalize_or_zero()),
        Segment::Arc { center, radius, start, sweep } => {
            let m = start + 0.5 * sweep;
            let t = DVec2::new(-m.sin(), m.cos()) * sweep.signum();
            (center + DVec2::from_angle(m) * radius, t)
        }
    }
}

/// Whether a surface's layout wraps round its axis (so a full turn needs a seam).
fn periodic(s: &Surface) -> bool {
    matches!(s, Surface::Cylinder { .. } | Surface::Torus { .. } | Surface::Cone { apex: false, .. })
}

/// How far an edge reaches in a surface's layout (the larger circle bounds the face).
fn layout_reach(s: &Surface, solid: &Solid, e: EdgeId) -> f64 {
    let Some(edge) = solid.edge(e) else { return 0.0 };
    (0..8)
        .map(|k| s.uv(edge.curve.eval(edge.t0 + (edge.t1 - edge.t0) * k as f64 / 8.0)).length())
        .fold(0.0, f64::max)
}

/// The surface a meridian segment sweeps about `axis`, laid out so that the face it
/// bounds has no pole or seam inside it.
fn side_surface(axis: &Frame, seg: &Segment, theta: f64, full: bool, on_axis: f64) -> Result<Surface> {
    let at_height = |h: f64| axis.with_origin(axis.origin + axis.z * h);
    // The axis frame upside down (still right-handed), for surfaces opening downwards.
    let flipped = |h: f64| Frame { origin: axis.origin + axis.z * h, x: axis.x, y: -axis.y, z: -axis.z };
    match *seg {
        Segment::Line { a, b } => {
            let (dh, dr) = (b.x - a.x, b.y - a.y);
            let len = (dh * dh + dr * dr).sqrt();
            if dh.abs() <= tol::LINEAR * 1e-3 * len.max(1.0) {
                return Ok(Surface::Plane { frame: at_height(a.x) });
            }
            if dr.abs() <= tol::LINEAR * 1e-3 * len.max(1.0) {
                return Ok(Surface::Cylinder { frame: at_height(a.x), radius: a.y });
            }
            // A cone, set up to open along its frame's z from the end nearer the axis.
            let (near, far) = if a.y <= b.y { (a, b) } else { (b, a) };
            let up = far.x > near.x;
            let half_angle = ((far.y - near.y) / (far.x - near.x).abs()).atan();
            let apex = near.y.abs() <= on_axis;
            // A pointed cone has its frame at the wide end; any other at the narrow end.
            let base = if apex { far } else { near };
            let frame = if up { at_height(base.x) } else { flipped(base.x) };
            Ok(Surface::Cone { frame, radius: base.y, half_angle, apex })
        }
        Segment::Arc { center, radius, start, sweep } => {
            if center.y == 0.0 {
                // The layout projects from the point opposite frame.z, which must be
                // off the face: a pole the face holds, or the middle of a partial turn.
                let o = axis.origin + axis.z * center.x;
                let frame = if full {
                    let pole_below = |p: DVec2| p.y.abs() <= on_axis && p.x < center.x;
                    if pole_below(seg.start()) || pole_below(seg.end()) {
                        Frame { origin: o, x: axis.x, y: -axis.y, z: -axis.z }
                    } else {
                        axis.with_origin(o)
                    }
                } else {
                    let m = 0.5 * theta;
                    let z = axis.x * m.cos() + axis.y * m.sin();
                    Frame { origin: o, x: axis.z, y: z.cross(axis.z), z }
                };
                return Ok(Surface::Sphere { frame, radius });
            }
            if center.y <= radius + tol::LINEAR {
                return Err(KernelError::Unsupported(
                    "an arc whose circle reaches the axis sweeps a self-intersecting torus".into(),
                ));
            }
            // Tube angle phi of the arc's middle: distance from the axis is
            // major + minor cos(phi), height minor sin(phi).
            let m = start + 0.5 * sweep;
            let v_mid = 0.5 * PI - m;
            let v_mid = (v_mid + PI).rem_euclid(TAU) - PI;
            Ok(Surface::Torus { frame: at_height(center.x), major: center.y, minor: radius, v_mid })
        }
    }
}

#[cfg(test)]
mod tests;
