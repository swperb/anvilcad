//! Seams for faces that wrap all the way around a cylinder or cone.
//!
//! A face bounded by two loops that each go once around the axis (a band) has no
//! edge where its angle wraps, so it cannot be laid out in parameter space.
//! [`add_seams`] cuts every such band along a generator line, splitting the loops'
//! edges where the line crosses them, and joins the two loops into one.

use crate::geom::{Curve, Frame, Surface};
use crate::intersect::{CurveHit, curve_surface};
use crate::topo::{Coedge, Edge, Loop, Solid};
use crate::{EdgeId, KernelError, Result, VertexId, tol};
use glam::{DVec2, DVec3};

/// Points closer than this are the same vertex when placing a seam.
const SNAP: f64 = tol::LINEAR * 10.0;

/// Gives every band face a seam. Faces that do not wrap are left alone.
pub fn add_seams(solid: &mut Solid) -> Result<()> {
    for fi in 0..solid.faces.len() {
        seam_face(solid, fi)?;
    }
    Ok(())
}

/// Parameter-space polyline of a loop and the number of times it goes around.
fn loop_uv(solid: &Solid, surface: &Surface, l: &Loop, period: f64) -> (Vec<DVec2>, i64) {
    let mut uv: Vec<DVec2> = Vec::new();
    for c in &l.coedges {
        let Some(e) = solid.edge(c.edge) else { continue };
        let mut pts = e.curve.sample(e.t0, e.t1, tol::CHORD, tol::MAX_ANGLE);
        if c.reversed {
            pts.reverse();
        }
        pts.pop();
        for p in pts {
            let mut q = surface.uv(p);
            if let Some(prev) = uv.last() {
                q.x += period * ((prev.x - q.x) / period).round();
            }
            uv.push(q);
        }
    }
    let turns = match (uv.first(), uv.last()) {
        (Some(a), Some(b)) => {
            let mut close = a.x - b.x;
            close -= period * (close / period).round();
            ((b.x - a.x + close) / period).round() as i64
        }
        _ => 0,
    };
    (uv, turns)
}

fn seam_face(solid: &mut Solid, fi: usize) -> Result<()> {
    let face = &solid.faces[fi];
    let surface = face.surface.clone();
    let Some(period) = surface.u_period() else {
        return Ok(());
    };
    let loops: Vec<Loop> = face.loops().cloned().collect();
    let info: Vec<(Vec<DVec2>, i64)> = loops.iter().map(|l| loop_uv(solid, &surface, l, period)).collect();
    let wrapping: Vec<usize> = (0..loops.len()).filter(|&i| info[i].1 != 0).collect();
    match wrapping.len() {
        0 => return Ok(()),
        2 => {}
        n => {
            return Err(KernelError::Unsupported(format!(
                "a face that wraps around its axis with {n} boundary loops"
            )));
        }
    }
    let (ia, ib) = (wrapping[0], wrapping[1]);

    // Holes in the band, as ranges of u; the seam keeps as far from them as it can.
    let holes: Vec<(f64, f64)> = (0..loops.len())
        .filter(|i| !wrapping.contains(i))
        .map(|i| {
            info[i].0.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), p| (a.min(p.x), b.max(p.x)))
        })
        .collect();
    let clearance = |u: f64| -> f64 {
        holes
            .iter()
            .map(|&(lo, hi)| {
                let mid = 0.5 * (lo + hi);
                let mut d = u - mid;
                d -= period * (d / period).round();
                d.abs() - 0.5 * (hi - lo)
            })
            .fold(f64::INFINITY, f64::min)
    };
    let vertex_u = |l: &Loop| -> Vec<(VertexId, f64)> {
        l.coedges
            .iter()
            .filter_map(|c| solid.coedge_vertices(*c))
            .filter_map(|(v, _)| solid.vertex(v).map(|p| (v, surface.uv(p.point).x)))
            .collect()
    };
    let (va, vb) = (vertex_u(&loops[ia]), vertex_u(&loops[ib]));
    let same = |a: f64, b: f64| {
        let mut d = a - b;
        d -= period * (d / period).round();
        d.abs() < 1e-9 * period.max(1.0)
    };
    let margin = 1e-3 * period;
    // Best: a vertex on each loop at the same angle, clear of every hole.
    let aligned = va
        .iter()
        .filter(|(_, u)| vb.iter().any(|(_, w)| same(*u, *w)))
        .map(|(_, u)| *u)
        .filter(|u| clearance(*u) > margin)
        .max_by(|a, b| clearance(*a).total_cmp(&clearance(*b)));
    let u = match aligned {
        Some(u) => u,
        None => {
            let candidates =
                va.iter().chain(&vb).map(|(_, u)| *u).chain((0..64).map(|k| period * k as f64 / 64.0));
            candidates.max_by(|a, b| clearance(*a).total_cmp(&clearance(*b))).unwrap_or(0.0)
        }
    };

    let (pa, pb, seam) = if let Surface::BSpline(patch) = &surface {
        // A wrapping patch: the seam is the patch's curve across the wrap, at the u
        // where the first loop is crossed; the piece between the two crossings, its ends
        // set exactly on them.
        let pa = vertex_at_u(solid, fi, ia, &surface, u, period)?;
        let a = solid
            .vertex(pa)
            .map(|v| v.point)
            .ok_or_else(|| KernelError::Topology("seam vertex missing".into()))?;
        let ua = surface.uv(a).x;
        let pb = vertex_at_u(solid, fi, ib, &surface, ua, period)?;
        let b = solid
            .vertex(pb)
            .map(|v| v.point)
            .ok_or_else(|| KernelError::Topology("seam vertex missing".into()))?;
        let iso = patch.iso(ua).ok_or_else(|| KernelError::Topology("patch does not wrap".into()))?;
        let (lo, hi) = iso.domain();
        let (ta, tb) = (iso.closest(a, lo, hi), iso.closest(b, lo, hi));
        let mut piece = iso.piece(ta.min(tb), ta.max(tb))?;
        let last = piece.points.len() - 1;
        let (first_end, last_end) = if ta <= tb { (a, b) } else { (b, a) };
        piece.points[0] = first_end;
        piece.points[last] = last_end;
        let piece = crate::spline::Spline::new(
            piece.degree,
            piece.knots.clone(),
            piece.points.clone(),
            piece.weights.clone(),
        )?;
        let (t0, t1) = if ta <= tb { (ta.min(tb), ta.max(tb)) } else { (ta.max(tb), ta.min(tb)) };
        let curve = Curve::Spline(std::sync::Arc::new(piece));
        (pa, pb, solid.add_edge(Edge { curve, start: pa, end: pb, t0, t1 }))
    } else {
        let theta = u / surface_scale(&surface);
        let frame = *surface.frame();
        let radial = frame.x * theta.cos() + frame.y * theta.sin();
        let cut = Surface::Plane { frame: Frame::new(frame.origin, frame.z.cross(radial), frame.z)? };
        let pa = vertex_at(solid, fi, ia, &cut, radial)?;
        let pb = vertex_at(solid, fi, ib, &cut, radial)?;
        let (Some(a), Some(b)) = (solid.vertex(pa).map(|v| v.point), solid.vertex(pb).map(|v| v.point))
        else {
            return Err(KernelError::Topology("seam vertex missing".into()));
        };
        (pa, pb, solid.add_edge(seam_edge(&surface, radial, a, b, pa, pb)?))
    };

    let face = &solid.faces[fi];
    let mut loops: Vec<Loop> = face.loops().cloned().collect();
    let la = rotate_to(solid, &loops[ia], pa)?;
    let lb = rotate_to(solid, &loops[ib], pb)?;
    let mut joined = la.coedges;
    joined.push(Coedge { edge: seam, reversed: false });
    joined.extend(lb.coedges);
    joined.push(Coedge { edge: seam, reversed: true });
    let (hi, lo) = (ia.max(ib), ia.min(ib));
    loops.remove(hi);
    loops.remove(lo);
    let face = &mut solid.faces[fi];
    face.outer = Loop { coedges: joined };
    face.inners = loops;
    Ok(())
}

/// The seam from `a` to `b` at angle `radial` around the axis: a straight generator
/// on a cylinder or cone, an arc round the tube on a torus.
fn seam_edge(s: &Surface, radial: DVec3, a: DVec3, b: DVec3, pa: VertexId, pb: VertexId) -> Result<Edge> {
    match *s {
        Surface::Torus { frame, major, minor, v_mid } => {
            let centre = frame.origin + radial * major;
            let circle = Frame { origin: centre, x: radial, y: frame.z, z: radial.cross(frame.z) };
            // Tube angles taken on the face's own branch, so the arc stays on the face.
            let angle = |p: DVec3| s.uv(p).y / minor + v_mid;
            let curve = Curve::Circle { frame: circle, radius: minor };
            Ok(Edge { curve, start: pa, end: pb, t0: angle(a), t1: angle(b) })
        }
        _ => {
            let len = (b - a).length();
            let dir =
                (b - a).try_normalize().ok_or_else(|| KernelError::Topology("band of zero height".into()))?;
            Ok(Edge { curve: Curve::Line { origin: a, dir }, start: pa, end: pb, t0: 0.0, t1: len })
        }
    }
}

/// Scale from angle to `u` on a surface that wraps.
fn surface_scale(s: &Surface) -> f64 {
    match s {
        Surface::Cylinder { radius, .. } | Surface::Cone { radius, .. } => *radius,
        Surface::Torus { major, .. } => *major,
        Surface::Plane { .. } | Surface::Sphere { .. } | Surface::BSpline(_) => 1.0,
    }
}

/// The vertex where loop `li` of face `fi` crosses the half-plane `cut` on the
/// side of `radial`, splitting the crossed edge when no vertex is there.
fn vertex_at(solid: &mut Solid, fi: usize, li: usize, cut: &Surface, radial: DVec3) -> Result<VertexId> {
    let origin = cut.frame().origin;
    let l = solid.faces[fi].loops().nth(li).cloned().unwrap_or_default();
    let mut best: Option<(f64, EdgeId, f64)> = None;
    for c in &l.coedges {
        let Some(e) = solid.edge(c.edge) else { continue };
        let CurveHit::Params(ts) = curve_surface(&e.curve, e.t0, e.t1, cut, tol::LINEAR) else {
            continue;
        };
        for t in ts {
            let p = e.curve.eval(t);
            let side = (p - origin).dot(radial);
            if side > 0.0 && best.is_none_or(|b| side > b.0) {
                best = Some((side, c.edge, t));
            }
        }
    }
    let (_, edge, t) =
        best.ok_or_else(|| KernelError::Topology("seam does not cross a band boundary".into()))?;
    split_edge(solid, edge, t)
}

/// The vertex where loop `li` of face `fi` crosses the line `u` = `u` (on any turn) in
/// the surface's parameters, splitting the crossed edge when no vertex is there: found
/// along the loop's samples, then by bisection on the edge.
fn vertex_at_u(
    solid: &mut Solid,
    fi: usize,
    li: usize,
    surface: &Surface,
    u: f64,
    period: f64,
) -> Result<VertexId> {
    let l = solid.faces[fi].loops().nth(li).cloned().unwrap_or_default();
    // Signed offset of a point's u from the line, on the nearest turn.
    let off = |p: DVec3| {
        let d = surface.uv(p).x - u;
        d - period * (d / period).round()
    };
    for c in &l.coedges {
        let Some(e) = solid.edge(c.edge).cloned() else { continue };
        let n = 64;
        let ts: Vec<f64> = (0..=n).map(|k| e.t0 + (e.t1 - e.t0) * k as f64 / n as f64).collect();
        let offs: Vec<f64> = ts.iter().map(|t| off(e.curve.eval(*t))).collect();
        for k in 0..n {
            let (a, b) = (offs[k], offs[k + 1]);
            // A real crossing changes sign by a little, not by a whole turn.
            if a == 0.0 {
                return split_edge(solid, c.edge, ts[k]);
            }
            if a.signum() != b.signum() && (a - b).abs() < 0.5 * period {
                let (mut lo, mut hi, mut flo) = (ts[k], ts[k + 1], a);
                for _ in 0..60 {
                    let m = 0.5 * (lo + hi);
                    let fm = off(e.curve.eval(m));
                    if fm.signum() == flo.signum() {
                        lo = m;
                        flo = fm;
                    } else {
                        hi = m;
                    }
                }
                return split_edge(solid, c.edge, 0.5 * (lo + hi));
            }
        }
    }
    Err(KernelError::Topology("seam does not cross a band boundary".into()))
}

/// Splits `edge` at parameter `t`, or returns the end vertex already there.
fn split_edge(solid: &mut Solid, edge: EdgeId, t: f64) -> Result<VertexId> {
    let e = solid.edge(edge).cloned().ok_or_else(|| KernelError::Topology("edge missing".into()))?;
    let p = e.curve.eval(t);
    for v in [e.start, e.end] {
        if solid.vertex(v).is_some_and(|x| (x.point - p).length() < SNAP) {
            return Ok(v);
        }
    }
    let v = solid.add_vertex(p);
    let tail = solid.add_edge(Edge { start: v, t0: t, ..e.clone() });
    let head = &mut solid.edges[edge.index()];
    head.end = v;
    head.t1 = t;
    for face in &mut solid.faces {
        for l in std::iter::once(&mut face.outer).chain(face.inners.iter_mut()) {
            let mut out = Vec::with_capacity(l.coedges.len() + 1);
            for c in &l.coedges {
                if c.edge != edge {
                    out.push(*c);
                } else if c.reversed {
                    out.push(Coedge { edge: tail, reversed: true });
                    out.push(*c);
                } else {
                    out.push(*c);
                    out.push(Coedge { edge: tail, reversed: false });
                }
            }
            l.coedges = out;
        }
    }
    Ok(v)
}

/// The same loop, starting with the coedge that leaves `v`.
fn rotate_to(solid: &Solid, l: &Loop, v: VertexId) -> Result<Loop> {
    let k = l
        .coedges
        .iter()
        .position(|c| solid.coedge_vertices(*c).is_some_and(|(s, _)| s == v))
        .ok_or_else(|| KernelError::Topology("seam vertex is not on its loop".into()))?;
    let mut coedges = l.coedges.clone();
    coedges.rotate_left(k);
    Ok(Loop { coedges })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::check;
    use crate::tess::{TessParams, tessellate};
    use crate::topo::{Face, FaceTag};
    use std::f64::consts::{PI, TAU};

    /// A cylinder (or cone) solid on the z axis from z = 0 (radius 2) to z = 3, with
    /// plane caps and a band side face that has no seam, its two circles starting at
    /// different angles.
    pub(crate) fn band_solid(half_angle: f64) -> Solid {
        let mut s = Solid::default();
        let (r, h) = (2.0, 3.0);
        let frame = Frame::world();
        let top_r = r + h * half_angle.tan();
        let bottom = Frame::world();
        let top_frame = Frame { origin: DVec3::new(0.0, 0.0, h), ..Frame::world() };
        let (a0, a1) = (0.3f64, 2.0f64);
        let pa = bottom.origin + DVec3::new(a0.cos(), a0.sin(), 0.0) * r;
        let pb = top_frame.origin + DVec3::new(a1.cos(), a1.sin(), 0.0) * top_r;
        let va = s.add_vertex(pa);
        let vb = s.add_vertex(pb);
        let ea = s.add_edge(Edge {
            curve: Curve::Circle { frame: bottom, radius: r },
            start: va,
            end: va,
            t0: a0,
            t1: a0 + TAU,
        });
        let eb = s.add_edge(Edge {
            curve: Curve::Circle { frame: top_frame, radius: top_r },
            start: vb,
            end: vb,
            t0: a1,
            t1: a1 + TAU,
        });
        let surface = if half_angle == 0.0 {
            Surface::Cylinder { frame, radius: r }
        } else {
            Surface::Cone { frame, radius: r, half_angle, apex: false }
        };
        let plane = |f: Frame| Surface::Plane { frame: f };
        let tag = |i| FaceTag::Imported { index: i };
        // Bottom cap faces down: its loop runs clockwise seen from above.
        s.add_face(Face {
            surface: plane(bottom),
            reversed: true,
            outer: Loop { coedges: vec![Coedge { edge: ea, reversed: true }] },
            inners: vec![],
            tag: tag(0),
            owner: 0,
        });
        s.add_face(Face {
            surface: plane(top_frame),
            reversed: false,
            outer: Loop { coedges: vec![Coedge { edge: eb, reversed: false }] },
            inners: vec![],
            tag: tag(1),
            owner: 0,
        });
        s.add_face(Face {
            surface,
            reversed: false,
            outer: Loop { coedges: vec![Coedge { edge: ea, reversed: false }] },
            inners: vec![Loop { coedges: vec![Coedge { edge: eb, reversed: true }] }],
            tag: tag(2),
            owner: 0,
        });
        s
    }

    /// A ring on the z axis: the half disc (rho - major)^2 + z^2 <= minor^2, rho >= major,
    /// swept round. A torus band outside, a cylinder band inside, neither with a seam.
    pub(crate) fn half_round_ring(major: f64, minor: f64) -> Solid {
        let mut s = Solid::default();
        let circle = |z: f64| Frame { origin: DVec3::new(0.0, 0.0, z), ..Frame::world() };
        let (a0, a1) = (0.4f64, 2.5f64);
        let vb = s.add_vertex(circle(-minor).origin + DVec3::new(a0.cos(), a0.sin(), 0.0) * major);
        let vt = s.add_vertex(circle(minor).origin + DVec3::new(a1.cos(), a1.sin(), 0.0) * major);
        let curve = |z: f64| Curve::Circle { frame: circle(z), radius: major };
        let eb = s.add_edge(Edge { curve: curve(-minor), start: vb, end: vb, t0: a0, t1: a0 + TAU });
        let et = s.add_edge(Edge { curve: curve(minor), start: vt, end: vt, t0: a1, t1: a1 + TAU });
        let tag = |i| FaceTag::Imported { index: i };
        let c = |edge, reversed| Loop { coedges: vec![Coedge { edge, reversed }] };
        s.add_face(Face {
            surface: Surface::Torus { frame: Frame::world(), major, minor, v_mid: 0.0 },
            reversed: false,
            outer: c(eb, false),
            inners: vec![c(et, true)],
            tag: tag(0),
            owner: 0,
        });
        s.add_face(Face {
            surface: Surface::Cylinder { frame: Frame::world(), radius: major },
            reversed: true,
            outer: c(eb, true),
            inners: vec![c(et, false)],
            tag: tag(1),
            owner: 0,
        });
        s
    }

    #[test]
    fn torus_band_gets_a_seam() {
        let (major, minor) = (6.0, 1.5);
        let mut s = half_round_ring(major, minor);
        check::validate(&s).expect("valid before");
        add_seams(&mut s).expect("seam");
        check::validate(&s).expect("valid after");
        // Pappus: half-disc area times the path of its centroid.
        let exact = 0.5 * PI * minor * minor * TAU * (major + 4.0 * minor / (3.0 * PI));
        assert!((volume(&s) - exact).abs() / exact < 1e-3, "{} vs {exact}", volume(&s));
    }

    fn volume(s: &Solid) -> f64 {
        let m = tessellate(s, &TessParams { chord: 1e-4, max_angle: 0.02 }).expect("tessellate");
        assert!(m.is_watertight());
        crate::mass::properties(&m).volume
    }

    #[test]
    fn cylinder_band_gets_a_seam() {
        let mut s = band_solid(0.0);
        check::validate(&s).expect("valid before");
        add_seams(&mut s).expect("seam");
        check::validate(&s).expect("valid after");
        assert!(s.faces[2].inners.is_empty());
        let exact = PI * 4.0 * 3.0;
        assert!((volume(&s) - exact).abs() / exact < 1e-3, "{}", volume(&s));
    }

    #[test]
    fn cone_band_gets_a_seam() {
        let half_angle = 0.25f64;
        let mut s = band_solid(half_angle);
        add_seams(&mut s).expect("seam");
        check::validate(&s).expect("valid");
        let (r1, r2, h) = (2.0, 2.0 + 3.0 * half_angle.tan(), 3.0);
        let exact = PI * h / 3.0 * (r1 * r1 + r1 * r2 + r2 * r2);
        assert!((volume(&s) - exact).abs() / exact < 1e-3, "{}", volume(&s));
    }
}
