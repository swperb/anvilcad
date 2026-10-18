//! Fillets and chamfers on straight edges between planar faces, and on whole circular
//! edges between faces round about the circle's axis (planes square to it, cylinders and
//! cones on it).
//!
//! A blend is made with an exact boolean: a tool whose cross-section is the material
//! between the edge and the fillet arc (or chamfer line) is cut away from a convex edge,
//! or added to a concave one. Along a straight edge the tool is a prism, extended past
//! an edge end only where nothing lies beyond it, so it never bites into neighbouring
//! geometry. Round a circular edge it is the cross-section revolved about the axis (a
//! torus or cone face for the round or bevel); its straight sides are pushed off the
//! faces (out of the part at a convex edge, into it at a concave one) so that they cross
//! them rather than lie on them.
//!
//! Edges that run smoothly one into the next are blended together ([`blend_chain`]): each
//! edge's tool stops where its edge runs on, and the tools are united first.
//!
//! The behaviour is specified in `docs/re/shell-and-fillet.md` and
//! `docs/re/fillet-curved-edges.md`.

use crate::boolean::{self, BoolOp};
use crate::geom::{Curve, Frame, Surface};
use crate::profile::{Loop2, Segment, build_regions};
use crate::topo::{EdgeId, Solid};
use crate::{KernelError, Result, extrude};
use glam::{DVec2, DVec3};
use std::f64::consts::PI;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Blend {
    /// Round with this radius.
    Fillet(f64),
    /// Equal-distance bevel, set back this far along each face.
    Chamfer(f64),
    /// Bevel set back `distance` along face `along` (an index into the solid's faces)
    /// and leaving that face at `angle` radians.
    ChamferAngle { distance: f64, angle: f64, along: usize },
    /// Offset face bevel: the two faces offset `distance` into the part (out of it at a
    /// concave edge) meet where the bevel's ends are dropped back square to each face, as
    /// a fillet of that radius would touch them.
    ChamferOffset(f64),
}

impl Blend {
    /// The size the user gave: radius or distance.
    fn size(&self) -> f64 {
        match *self {
            Blend::Fillet(r) | Blend::Chamfer(r) | Blend::ChamferOffset(r) => r,
            Blend::ChamferAngle { distance, .. } => distance,
        }
    }

    fn name(&self) -> &'static str {
        if matches!(self, Blend::Fillet(_)) { "fillet" } else { "chamfer" }
    }

    /// How far the blend sets back along faces `fa` and `fb` where they meet at the angle
    /// `phi` (on the corner's side), as on flat faces.
    fn setbacks(&self, phi: f64, fa: usize, fb: usize) -> Result<(f64, f64)> {
        Ok(match *self {
            Blend::Fillet(r) | Blend::ChamferOffset(r) => (r / (phi / 2.0).tan(), r / (phi / 2.0).tan()),
            Blend::Chamfer(c) => (c, c),
            Blend::ChamferAngle { distance, angle, along } => {
                if !(angle > 0.0 && angle + phi < PI) {
                    return invalid(
                        "the chamfer's angle must be more than 0 and leave the corner's other face",
                    );
                }
                // The triangle of the edge and the bevel's ends: angle phi at the edge,
                // `angle` at the end on face `along` (law of sines).
                let other = distance * angle.sin() / (phi + angle).sin();
                if along == fa {
                    (distance, other)
                } else if along == fb {
                    (other, distance)
                } else {
                    return invalid("the face the chamfer's distance is measured along is not at this edge");
                }
            }
        })
    }
}

fn invalid<T>(msg: impl Into<String>) -> Result<T> {
    Err(KernelError::InvalidInput(msg.into()))
}

/// Blends one edge. New faces are tagged with `owner`, and the tool's three side
/// faces get the tags `tag_base`, `tag_base + 2` (the round or bevel face) and
/// `tag_base + 4`, so callers can give blend faces stable names.
pub fn blend_edge(
    solid: &Solid,
    edge: EdgeId,
    blend: Blend,
    owner: u32,
    tag_base: u32,
) -> Result<Vec<Solid>> {
    blend_chain(solid, &[(edge, tag_base)], blend, owner)
}

/// Blends edges that run smoothly one into the next (the top edges of an extruded slot,
/// say) in one step: each edge's tool is made against the solid as it is, stopping
/// exactly where its edge runs on into the next, and the tools are united and applied
/// once. Blending them one at a time instead would lose the joins: once an edge is
/// blended, its neighbour no longer runs on into anything. Each edge comes with its
/// tag base (see [`blend_edge`]).
pub fn blend_chain(solid: &Solid, edges: &[(EdgeId, u32)], blend: Blend, owner: u32) -> Result<Vec<Solid>> {
    // Closed-form tools where every edge has one; otherwise (a fillet with an edge between
    // faces not round about a common axis) one rolling-ball round along the whole run.
    let tools: Result<Vec<(Solid, bool)>> =
        edges.iter().map(|&(edge, tag_base)| analytic_tool(solid, edge, blend, tag_base)).collect();
    let (mut tool, convex, contacts) = match tools {
        Ok(tools) => {
            let mut acc: Option<(Solid, bool)> = None;
            for (t, convex) in tools {
                acc = Some(match acc {
                    None => (t, convex),
                    Some((a, c)) => {
                        if c != convex {
                            return Err(KernelError::Unsupported(
                                "blending a smooth run of edges that are convex in part and concave in part"
                                    .into(),
                            ));
                        }
                        let mut joined = boolean::boolean(&a, &t, BoolOp::Union)?;
                        if joined.len() != 1 {
                            return invalid("the blends along these edges do not join up");
                        }
                        (joined.remove(0), c)
                    }
                });
            }
            let (t, c) = acc.ok_or_else(|| KernelError::InvalidInput("no edge to blend".into()))?;
            (t, c, Vec::new())
        }
        Err(KernelError::Unsupported(_)) => {
            let ids: Vec<EdgeId> = edges.iter().map(|e| e.0).collect();
            let tag_base = edges.first().map_or(0, |e| e.1);
            crate::rolling::rolling_run_tool(solid, &ids, blend, tag_base)?
        }
        Err(e) => return Err(e),
    };
    tool.set_owner(owner);
    let op = if convex { BoolOp::Subtract } else { BoolOp::Union };
    boolean::boolean_with_contacts(solid, &tool, op, &contacts)
}

/// Every edge among `candidates` joined to `edge` through smooth runs, `edge` first.
pub fn smooth_run(solid: &Solid, edge: EdgeId, candidates: &[EdgeId]) -> Vec<EdgeId> {
    let mut run = vec![edge];
    let mut k = 0;
    while k < run.len() {
        let e = run[k];
        for &c in candidates {
            if !run.contains(&c) && joined_smoothly(solid, e, c) {
                run.push(c);
            }
        }
        k += 1;
    }
    run
}

/// Whether edges `a` and `b` share an end where one runs smoothly on into the other.
fn joined_smoothly(solid: &Solid, a: EdgeId, b: EdgeId) -> bool {
    let (Some(ea), Some(eb)) = (solid.edge(a), solid.edge(b)) else { return false };
    let leaving = |e: &crate::topo::Edge, t: f64, toward: f64| {
        let h = (toward - t) * 1e-6;
        (e.curve.eval(t + h) - e.curve.eval(t)).normalize_or_zero()
    };
    let ends = |e: &crate::topo::Edge| [(e.start, leaving(e, e.t0, e.t1)), (e.end, leaving(e, e.t1, e.t0))];
    ends(ea).iter().any(|(va, da)| ends(eb).iter().any(|(vb, db)| va == vb && da.dot(*db) < -1.0 + 1e-6))
}

/// The tool for a blend in closed form: a prism along a straight edge, the corner
/// revolved round a circular one.
fn analytic_tool(solid: &Solid, edge: EdgeId, blend: Blend, tag_base: u32) -> Result<(Solid, bool)> {
    let size = blend.size();
    if !(size.is_finite() && size > 0.0) {
        return invalid("the size must be a positive number");
    }
    let e = solid.edge(edge).ok_or_else(|| KernelError::InvalidInput("the edge does not exist".into()))?;
    if let Curve::Circle { frame, radius } = e.curve {
        return round_edge_tool(solid, edge, &frame, radius, blend, tag_base);
    }
    let Curve::Line { .. } = e.curve else {
        return Err(KernelError::Unsupported(
            "fillets and chamfers on curved edges other than circles".into(),
        ));
    };
    let p0 = e.curve.eval(e.t0);
    let p1 = e.curve.eval(e.t1);
    let len = (p1 - p0).length();
    let d = (p1 - p0) / len;

    // The two faces meeting at the edge, with the direction pointing from
    // the edge into each face.
    let mut sides: Vec<(usize, DVec3, DVec3)> = Vec::new();
    for (fi, face) in solid.faces.iter().enumerate() {
        for l in face.loops() {
            for c in &l.coedges {
                if c.edge != edge {
                    continue;
                }
                let Surface::Plane { frame } = face.surface else {
                    return Err(KernelError::Unsupported("fillets and chamfers next to curved faces".into()));
                };
                let n = if face.reversed { -frame.z } else { frame.z };
                let travel = if c.reversed { -d } else { d };
                // Loops run counter-clockwise about the outward normal, so
                // the face lies to the left of its edges.
                sides.push((fi, n, n.cross(travel).normalize()));
            }
        }
    }
    let [(fa, na, wa), (fb, nb, wb)] = sides[..] else {
        return invalid("the edge is not shared by exactly two faces");
    };
    let bend = wa.dot(nb);
    if bend.abs() < 1e-9 {
        return invalid("the faces at this edge are tangent; there is nothing to blend");
    }
    let convex = bend < 0.0;
    let phi = wa.dot(wb).clamp(-1.0, 1.0).acos();
    let (sa, sb) = blend.setbacks(phi, fa, fb)?;
    let setback = sa.max(sb);

    // The blend must fit on both faces.
    let mid = (p0 + p1) * 0.5;
    for (f, w, s) in [(fa, wa, sa), (fb, wb, sb)] {
        if !boolean::face_contains(solid, f, mid + w * s * 0.999) {
            return invalid(format!(
                "a {} of {size} does not fit on the faces next to this edge",
                blend.name()
            ));
        }
    }

    // Cross-section in a frame with z along the edge and x into face A.
    let frame = Frame::new(p0, d, wa)?;
    let to2 = |v: DVec3| DVec2::new(v.dot(frame.x), v.dot(frame.y));
    let a = to2(wa * sa);
    let b = to2(wb * sb);
    let round = match blend {
        Blend::Fillet(r) => {
            let center = to2((wa + wb).normalize() * (r / (phi / 2.0).sin()));
            [true, false]
                .into_iter()
                .filter_map(|ccw| Segment::arc_from_center(center, a, b, ccw))
                .find(|s| matches!(s, Segment::Arc { sweep, .. } if sweep.abs() < PI))
                .ok_or_else(|| KernelError::InvalidInput("degenerate fillet".into()))?
        }
        _ => Segment::Line { a, b },
    };
    let regions = build_regions(&[pushed_section(
        round,
        a,
        b,
        DVec2::ZERO,
        to2(na),
        to2(nb),
        setback,
        convex,
        tag_base,
    )])?;

    // Extend past an end only where the far side is open (convex) or solid
    // (concave), so the extension changes nothing but avoids slivers.
    let margin = setback * 2.0;
    let probe = (wa + wb) * setback * 0.25;
    let extend = |end: DVec3, out: DVec3| -> Result<f64> {
        // Where the edge runs smoothly on into another, that edge's blend carries on
        // from here, so this one stops exactly at the end.
        if runs_on(solid, edge, end, out) {
            return Ok(0.0);
        }
        let inside = boolean::contains_point(solid, end + out * (margin * 0.5) + probe)?;
        Ok(if inside != convex { margin } else { 0.0 })
    };
    let z0 = -extend(p0, -d)?;
    let z1 = len + extend(p1, d)?;
    let region = regions.first().ok_or_else(|| KernelError::InvalidInput("degenerate blend".into()))?;
    let tool = extrude::extrude(&frame, region, z0, z1)?;
    Ok((tool, convex))
}

/// The faces at an edge, each with its outward normal at `p` and the direction from
/// the edge into it there (`tangent` is the edge's direction at `p`).
pub(crate) fn edge_sides(
    solid: &Solid,
    edge: EdgeId,
    p: DVec3,
    tangent: DVec3,
) -> Vec<(usize, DVec3, DVec3)> {
    let mut sides = Vec::new();
    for (fi, face) in solid.faces.iter().enumerate() {
        for c in face.loops().flat_map(|l| &l.coedges) {
            if c.edge != edge {
                continue;
            }
            let n = face.normal_at(p);
            let travel = if c.reversed { -tangent } else { tangent };
            // Loops run counter-clockwise about the outward normal, so the face lies to
            // the left of its edges.
            sides.push((fi, n, n.cross(travel).normalize_or_zero()));
        }
    }
    sides
}

/// Whether `s` is a plane square to the axis through `o` along `a`, or a cylinder or
/// cone on that axis: a surface whose meridian is a straight line.
fn coaxial_line_surface(s: &Surface, o: DVec3, a: DVec3) -> bool {
    let on_axis = |f: &Frame| {
        let d = f.origin - o;
        f.z.cross(a).length() < 1e-9 && (d - a * d.dot(a)).length() < 1e-9 * (1.0 + d.length())
    };
    match s {
        Surface::Plane { frame } => frame.z.cross(a).length() < 1e-9,
        Surface::Cylinder { frame, .. } | Surface::Cone { frame, .. } => on_axis(frame),
        _ => false,
    }
}

/// A blend tool's cross-section: the `round` (or bevel) from `pa` on face A to `pb` on
/// face B, closed by sides pushed off the faces (whose outward normals in the section
/// are `na` and `nb`, meeting at the edge `c`): out of the part at a convex edge, where
/// the tool cuts, into it at a concave one, where the tool adds. Lying on the faces
/// instead, the tool's sides would coincide with them, and where tools meet a curved
/// round could touch a flat side at a point; pushed off, every side crosses the faces.
#[allow(clippy::too_many_arguments)]
fn pushed_section(
    round: Segment,
    pa: DVec2,
    pb: DVec2,
    c: DVec2,
    na: DVec2,
    nb: DVec2,
    setback: f64,
    convex: bool,
    tag_base: u32,
) -> Loop2 {
    let push = setback * 0.5 * if convex { 1.0 } else { -1.0 };
    let (pb2, pc2, pa2) = (pb + nb * push, c + (na + nb) * push, pa + na * push);
    let segs = vec![
        round,
        Segment::Line { a: pb, b: pb2 },
        Segment::Line { a: pb2, b: pc2 },
        Segment::Line { a: pc2, b: pa2 },
        Segment::Line { a: pa2, b: pa },
    ];
    Loop2::with_tags(segs, vec![tag_base + 2, tag_base + 4, tag_base + 4, tag_base, tag_base])
}

/// Whether another edge leaves the vertex at `end` (an end of `edge`) in the direction
/// `out` that `edge` would carry on in: the edges join smoothly there.
pub(crate) fn runs_on(solid: &Solid, edge: EdgeId, end: DVec3, out: DVec3) -> bool {
    solid.edges.iter().enumerate().any(|(i, e)| {
        if i == edge.index() {
            return false;
        }
        let leaving = |t: f64, toward: f64| {
            let h = (toward - t) * 1e-6;
            (e.curve.eval(t + h) - e.curve.eval(t)).normalize_or_zero()
        };
        let starts =
            solid.vertex(e.start).is_some_and(|v| (v.point - end).length() < 1e-9 * (1.0 + end.length()));
        let ends =
            solid.vertex(e.end).is_some_and(|v| (v.point - end).length() < 1e-9 * (1.0 + end.length()));
        (starts && leaving(e.t0, e.t1).dot(out) > 1.0 - 1e-6)
            || (ends && leaving(e.t1, e.t0).dot(out) > 1.0 - 1e-6)
    })
}

/// Blends a circular edge, whole or an arc (see the module notes).
fn round_edge_tool(
    solid: &Solid,
    edge: EdgeId,
    circle: &Frame,
    radius: f64,
    blend: Blend,
    tag_base: u32,
) -> Result<(Solid, bool)> {
    let e = solid.edge(edge).ok_or_else(|| KernelError::InvalidInput("the edge does not exist".into()))?;
    let (o, a) = (circle.origin, circle.z);
    let dir = (e.t1 - e.t0).signum();
    let sweep = (e.t1 - e.t0).abs();
    let whole = (sweep - 2.0 * PI).abs() <= 1e-9;
    // Position and direction of travel round the circle at parameter `t`.
    let at = |t: f64| o + (circle.x * t.cos() + circle.y * t.sin()) * radius;
    let along = |t: f64| (circle.y * t.cos() - circle.x * t.sin()) * dir;
    let mid = 0.5 * (e.t0 + e.t1);
    let p = at(mid);
    let sides = edge_sides(solid, edge, p, along(mid));
    let [(fa, na, wa), (fb, nb, wb)] = sides[..] else {
        return invalid("the edge is not shared by exactly two faces");
    };
    for f in [fa, fb] {
        if !coaxial_line_surface(&solid.faces[f].surface, o, a) {
            return Err(KernelError::Unsupported(
                "fillets and chamfers on circular edges next to faces not round about the same axis".into(),
            ));
        }
    }
    let size = blend.size();
    let bend = wa.dot(nb);
    if bend.abs() < 1e-9 {
        return invalid("the faces at this edge are tangent; there is nothing to blend");
    }
    let convex = bend < 0.0;
    let phi = wa.dot(wb).clamp(-1.0, 1.0).acos();
    let (sa, sb) = blend.setbacks(phi, fa, fb)?;
    let setback = sa.max(sb);
    let too_big =
        || invalid(format!("a {} of {size} does not fit on the faces next to this edge", blend.name()));
    for (f, w, s) in [(fa, wa, sa), (fb, wb, sb)] {
        if !boolean::face_contains(solid, f, p + w * s * 0.999) {
            return too_big();
        }
    }

    // The cross-section in the meridian half-plane through `p`: x away from the axis,
    // y along it, the edge at (radius, 0).
    let radial = (p - o).normalize();
    let to2 = |v: DVec3| DVec2::new(v.dot(radial), v.dot(a));
    let c = DVec2::new(radius, 0.0);
    let (wa2, wb2, na2, nb2) = (to2(wa), to2(wb), to2(na), to2(nb));
    let pa = c + wa2 * sa;
    let pb = c + wb2 * sb;
    let round = match blend {
        Blend::Fillet(r) => {
            let center = c + (wa2 + wb2).normalize() * (r / (phi / 2.0).sin());
            [true, false]
                .into_iter()
                .filter_map(|ccw| Segment::arc_from_center(center, pa, pb, ccw))
                .find(|s| matches!(s, Segment::Arc { sweep, .. } if sweep.abs() < PI))
                .ok_or_else(|| KernelError::InvalidInput("degenerate fillet".into()))?
        }
        _ => Segment::Line { a: pa, b: pb },
    };
    let regions = build_regions(&[pushed_section(round, pa, pb, c, na2, nb2, setback, convex, tag_base)])?;
    let region = regions.first().ok_or_else(|| KernelError::InvalidInput("degenerate blend".into()))?;
    // How far round the tool turns: the whole circle, or the arc, carried on past an
    // end (by the same margin as a straight edge's) only where nothing lies beyond it,
    // and never where the edge runs smoothly on into the next.
    let (start, turn) = if whole {
        (e.t0, 2.0 * PI)
    } else {
        let margin = setback * 2.0;
        let extend = |t: f64, out: DVec3| -> Result<f64> {
            let end = at(t);
            if runs_on(solid, edge, end, out) {
                return Ok(0.0);
            }
            let ends = edge_sides(solid, edge, end, along(t));
            let [(_, _, ua), (_, _, ub)] = ends[..] else { return Ok(0.0) };
            let probe = end + out * (margin * 0.5) + (ua + ub) * setback * 0.25;
            let inside = boolean::contains_point(solid, probe)?;
            Ok(if inside != convex { margin / radius } else { 0.0 })
        };
        let before = extend(e.t0, -along(e.t0))?;
        let after = extend(e.t1, along(e.t1))?;
        let turn = (sweep + before + after).min(2.0 * PI - 1e-6);
        (e.t0 - dir * before, turn)
    };
    let x0 = (at(start) - o).normalize();
    let frame = Frame { origin: o, x: x0, y: a, z: x0.cross(a) };
    // A positive turn of the revolve goes towards frame.z, which is backwards along
    // the circle's parameter.
    let tool = match crate::revolve::revolve(&frame, region, DVec2::ZERO, DVec2::Y, -dir * turn) {
        Ok(t) => t,
        // The cross-section reaching the axis: the blend is wider than the circle.
        Err(KernelError::InvalidInput(_) | KernelError::Unsupported(_)) => return too_big(),
        Err(err) => return Err(err),
    };
    Ok((tool, convex))
}

/// Finds the edge closest to `p` (within `tol`).
pub fn edge_near(solid: &Solid, p: DVec3, tol: f64) -> Option<EdgeId> {
    solid
        .edges
        .iter()
        .enumerate()
        .map(|(i, e)| (i, e.curve.distance_to(p, e.t0, e.t1)))
        .filter(|(_, d)| *d < tol)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| EdgeId(i as u32))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{check, mass, tess};

    fn prism(pts: &[DVec2], h: f64) -> Solid {
        let l = Loop2::new(
            (0..pts.len()).map(|i| Segment::Line { a: pts[i], b: pts[(i + 1) % pts.len()] }).collect(),
        );
        let r = build_regions(&[l]).expect("region");
        extrude::extrude(&Frame::world(), &r[0], 0.0, h).expect("prism")
    }

    fn cube(s: f64) -> Solid {
        prism(&[DVec2::ZERO, DVec2::new(s, 0.0), DVec2::new(s, s), DVec2::new(0.0, s)], s)
    }

    fn volume(solids: &[Solid]) -> f64 {
        solids
            .iter()
            .map(|s| {
                check::validate(s).expect("valid");
                let m = tess::tessellate(s, &tess::TessParams::default()).expect("tess");
                assert!(m.is_watertight());
                mass::properties(&m).volume
            })
            .sum()
    }

    fn edge(s: &Solid, p: DVec3) -> EdgeId {
        edge_near(s, p, 1e-6).expect("edge")
    }

    #[test]
    fn fillet_one_cube_edge() {
        let c = cube(10.0);
        let r =
            blend_edge(&c, edge(&c, DVec3::new(5.0, 0.0, 10.0)), Blend::Fillet(2.0), 7, 0).expect("fillet");
        assert_eq!(r.len(), 1);
        let exact = 1000.0 - (4.0 - PI) * 10.0;
        assert!((volume(&r) - exact).abs() < 0.05, "{}", volume(&r));
        // The round face belongs to the fillet.
        assert!(r[0].faces.iter().any(|f| f.owner == 7 && matches!(f.surface, Surface::Cylinder { .. })));
    }

    #[test]
    fn chamfer_one_cube_edge() {
        let c = cube(10.0);
        let r = blend_edge(&c, edge(&c, DVec3::new(10.0, 5.0, 10.0)), Blend::Chamfer(3.0), 1, 0)
            .expect("chamfer");
        assert!((volume(&r) - (1000.0 - 4.5 * 10.0)).abs() < 1e-6);
    }

    #[test]
    fn fillet_all_four_top_edges() {
        let mut s = cube(10.0);
        for mid in [
            DVec3::new(5.0, 0.0, 10.0),
            DVec3::new(10.0, 5.0, 10.0),
            DVec3::new(5.0, 10.0, 10.0),
            DVec3::new(0.0, 5.0, 10.0),
        ] {
            let e = edge(&s, mid);
            s = blend_edge(&s, e, Blend::Fillet(2.0), 1, 0).expect("fillet").remove(0);
        }
        // Four straight runs of fillet section plus four mitred corners.
        let v = volume(std::slice::from_ref(&s));
        let section = 4.0 - PI;
        assert!(v < 1000.0 - section * 4.0 * 6.0 && v > 1000.0 - section * 40.0, "{v}");
        assert_eq!(check::genus(&s).expect("genus"), 0);
    }

    #[test]
    fn chamfer_three_edges_at_a_corner() {
        let mut s = cube(10.0);
        for mid in [DVec3::new(5.0, 0.0, 10.0), DVec3::new(0.0, 5.0, 10.0), DVec3::new(0.0, 0.0, 5.0)] {
            let e = edge(&s, mid);
            s = blend_edge(&s, e, Blend::Chamfer(2.0), 1, 0).expect("chamfer").remove(0);
        }
        let v = volume(std::slice::from_ref(&s));
        assert!(v < 1000.0 - 3.0 * 2.0 * 6.0 && v > 1000.0 - 3.0 * 2.0 * 10.0, "{v}");
    }

    #[test]
    fn fillet_three_edges_at_a_corner() {
        let mut s = cube(10.0);
        for mid in [DVec3::new(5.0, 0.0, 10.0), DVec3::new(0.0, 5.0, 10.0), DVec3::new(0.0, 0.0, 5.0)] {
            let e = edge(&s, mid);
            s = blend_edge(&s, e, Blend::Fillet(2.0), 1, 0).expect("fillet").remove(0);
        }
        let v = volume(std::slice::from_ref(&s));
        let section = 4.0 - PI;
        assert!(v < 1000.0 - section * 3.0 * 6.0 && v > 1000.0 - section * 30.0, "{v}");
    }

    #[test]
    fn fillet_a_concave_edge_adds_material() {
        // L-shaped prism: the inside corner edge runs along z at (4, 4).
        let l = prism(
            &[
                DVec2::ZERO,
                DVec2::new(10.0, 0.0),
                DVec2::new(10.0, 4.0),
                DVec2::new(4.0, 4.0),
                DVec2::new(4.0, 10.0),
                DVec2::new(0.0, 10.0),
            ],
            5.0,
        );
        let before = volume(std::slice::from_ref(&l));
        let r =
            blend_edge(&l, edge(&l, DVec3::new(4.0, 4.0, 2.5)), Blend::Fillet(2.0), 1, 0).expect("fillet");
        assert_eq!(r.len(), 1);
        let exact = before + (4.0 - PI) * 5.0;
        assert!((volume(&r) - exact).abs() < 0.05, "{}", volume(&r));
    }

    #[test]
    fn too_large_and_curved_edges_are_refused() {
        let c = cube(10.0);
        let e = edge(&c, DVec3::new(5.0, 0.0, 10.0));
        assert!(matches!(blend_edge(&c, e, Blend::Fillet(20.0), 1, 0), Err(KernelError::InvalidInput(_))));
        assert!(blend_edge(&c, e, Blend::Fillet(-1.0), 1, 0).is_err());
        let l = Loop2::new(vec![Segment::circle(DVec2::ZERO, 3.0)]);
        let r = build_regions(&[l]).expect("region");
        let cyl = extrude::extrude(&Frame::world(), &r[0], 0.0, 5.0).expect("cyl");
        // The rim fillets now, but not wider than the cylinder.
        let rim = edge(&cyl, DVec3::new(3.0, 0.0, 5.0));
        assert!(blend_edge(&cyl, rim, Blend::Fillet(1.0), 1, 0).is_ok());
        assert!(matches!(blend_edge(&cyl, rim, Blend::Fillet(3.5), 1, 0), Err(KernelError::InvalidInput(_))));
    }

    /// Volume finely tessellated, each solid checked valid and watertight.
    fn fine_volume(solids: &[Solid]) -> f64 {
        solids
            .iter()
            .map(|s| {
                check::validate(s).expect("valid");
                let m =
                    tess::tessellate(s, &tess::TessParams { chord: 1e-4, max_angle: 0.01 }).expect("tess");
                assert!(m.is_watertight());
                mass::properties(&m).volume
            })
            .sum()
    }

    fn cylinder(r: f64, h: f64) -> Solid {
        let l = Loop2::new(vec![Segment::circle(DVec2::ZERO, r)]);
        let reg = build_regions(&[l]).expect("region");
        extrude::extrude(&Frame::world(), &reg[0], 0.0, h).expect("cyl")
    }

    #[test]
    fn fillet_and_chamfer_a_cylinder_rim() {
        // A fillet of radius f round the top rim of a cylinder of radius R removes the
        // corner outside the arc (area f^2 (1 - pi / 4)) swept round its centroid, which
        // is f (10 - 3 pi) / (3 (4 - pi)) in from the edge (Pappus).
        let (big, f) = (3.0, 1.0);
        let cyl = cylinder(big, 5.0);
        let rim = edge(&cyl, DVec3::new(big, 0.0, 5.0));
        let r = blend_edge(&cyl, rim, Blend::Fillet(f), 7, 0).expect("fillet");
        let spandrel = f * f * (1.0 - PI / 4.0);
        let inset = f * (10.0 - 3.0 * PI) / (3.0 * (4.0 - PI));
        let x = big - inset;
        let want = PI * big * big * 5.0 - 2.0 * PI * x * spandrel;
        // Mostly round, so the tessellation's inscribed polygons (short by about the
        // square of the angle step over 6) set the tolerance.
        assert!((fine_volume(&r) - want).abs() / want < 5e-5, "{} vs {want}", fine_volume(&r));
        assert!(r[0].faces.iter().any(|f| f.owner == 7 && matches!(f.surface, Surface::Torus { .. })));
        // A chamfer: a right triangle of legs c, centroid c / 3 in from the rim.
        let c = 1.0;
        let r = blend_edge(&cyl, rim, Blend::Chamfer(c), 7, 0).expect("chamfer");
        let want = PI * big * big * 5.0 - 2.0 * PI * (big - c / 3.0) * (c * c / 2.0);
        assert!((fine_volume(&r) - want).abs() / want < 5e-5, "{} vs {want}", fine_volume(&r));
        assert!(r[0].faces.iter().any(|f| f.owner == 7 && matches!(f.surface, Surface::Cone { .. })));
    }

    #[test]
    fn fillet_a_hole_edge_and_a_boss_root() {
        use crate::boolean::{BoolOp, boolean};
        // A 20 x 20 x 6 block with a radius 3 hole: its top edge is convex, and filleting
        // it removes the same corner, now outside the edge's circle.
        let block = prism(
            &[
                DVec2::new(-10.0, -10.0),
                DVec2::new(10.0, -10.0),
                DVec2::new(10.0, 10.0),
                DVec2::new(-10.0, 10.0),
            ],
            6.0,
        );
        let hole = {
            let l = Loop2::new(vec![Segment::circle(DVec2::ZERO, 3.0)]);
            let reg = build_regions(&[l]).expect("region");
            extrude::extrude(&Frame::world(), &reg[0], -1.0, 7.0).expect("hole")
        };
        let holed = boolean(&block, &hole, BoolOp::Subtract).expect("hole").remove(0);
        let rim = edge(&holed, DVec3::new(3.0, 0.0, 6.0));
        let r = blend_edge(&holed, rim, Blend::Fillet(1.0), 1, 0).expect("fillet");
        let spandrel = 1.0 - PI / 4.0;
        let inset = (10.0 - 3.0 * PI) / (3.0 * (4.0 - PI));
        let x = 3.0 + inset;
        let want = (400.0 - 9.0 * PI) * 6.0 - 2.0 * PI * x * spandrel;
        assert!((fine_volume(&r) - want).abs() / want < 1e-5, "{} vs {want}", fine_volume(&r));
        // A boss on the block: the root edge is concave and its fillet adds material.
        let boss = {
            let l = Loop2::new(vec![Segment::circle(DVec2::new(0.0, 0.0), 4.0)]);
            let reg = build_regions(&[l]).expect("region");
            extrude::extrude(&Frame::world(), &reg[0], 6.0, 10.0).expect("boss")
        };
        let base = boolean(&block, &boss, BoolOp::Union).expect("union").remove(0);
        let root = edge(&base, DVec3::new(4.0, 0.0, 6.0));
        let r = blend_edge(&base, root, Blend::Fillet(1.0), 1, 0).expect("fillet");
        let x = 4.0 + inset;
        let want = 400.0 * 6.0 + 16.0 * PI * 4.0 + 2.0 * PI * x * spandrel;
        assert!((fine_volume(&r) - want).abs() / want < 1e-5, "{} vs {want}", fine_volume(&r));
    }

    /// The corner a fillet of radius 1 takes off (or fills): its area and how far its
    /// centroid lies in from the edge.
    fn spandrel() -> (f64, f64) {
        (1.0 - PI / 4.0, (10.0 - 3.0 * PI) / (3.0 * (4.0 - PI)))
    }

    /// A stadium: two half circles of radius `r` about (0, 0) and (`len`, 0), joined
    /// by straight sides.
    fn stadium(r: f64, len: f64) -> Loop2 {
        Loop2::new(vec![
            Segment::Line { a: DVec2::new(0.0, -r), b: DVec2::new(len, -r) },
            Segment::Arc { center: DVec2::new(len, 0.0), radius: r, start: -0.5 * PI, sweep: PI },
            Segment::Line { a: DVec2::new(len, r), b: DVec2::new(0.0, r) },
            Segment::Arc { center: DVec2::ZERO, radius: r, start: 0.5 * PI, sweep: PI },
        ])
    }

    #[test]
    fn fillet_an_arc_edge_between_flat_sides() {
        // A quarter disc of radius 10, 5 thick: filleting its round top edge takes the
        // corner off over a quarter turn, cut clean at the flat sides.
        let sector = Loop2::new(vec![
            Segment::Line { a: DVec2::ZERO, b: DVec2::new(10.0, 0.0) },
            Segment::Arc { center: DVec2::ZERO, radius: 10.0, start: 0.0, sweep: 0.5 * PI },
            Segment::Line { a: DVec2::new(0.0, 10.0), b: DVec2::ZERO },
        ]);
        let r = build_regions(&[sector]).expect("region");
        let s = extrude::extrude(&Frame::world(), &r[0], 0.0, 5.0).expect("sector");
        let mid = DVec3::new(10.0 * (PI / 4.0).cos(), 10.0 * (PI / 4.0).sin(), 5.0);
        let out = blend_edge(&s, edge(&s, mid), Blend::Fillet(1.0), 1, 0).expect("fillet");
        let (area, inset) = spandrel();
        let want = 25.0 * PI * 5.0 - area * 0.5 * PI * (10.0 - inset);
        assert!((fine_volume(&out) - want).abs() / want < 5e-5, "{} vs {want}", fine_volume(&out));
    }

    #[test]
    fn fillet_a_stadium_all_round() {
        // Its four top edges run smoothly one into the next, so they are blended as one
        // run: each edge's tool stops where its edge runs on, and the rounds meet
        // without a step. The corner comes off along the straight sides and round a
        // whole turn at the ends.
        let (r, len) = (4.0, 10.0);
        let reg = build_regions(&[stadium(r, len)]).expect("region");
        let mut s = extrude::extrude(&Frame::world(), &reg[0], 0.0, 5.0).expect("stadium");
        let edges: Vec<EdgeId> = [
            DVec3::new(0.5 * len, -r, 5.0),
            DVec3::new(len + r, 0.0, 5.0),
            DVec3::new(0.5 * len, r, 5.0),
            DVec3::new(-r, 0.0, 5.0),
        ]
        .map(|p| edge(&s, p))
        .to_vec();
        let run = smooth_run(&s, edges[0], &edges);
        assert_eq!(run.len(), 4);
        let tagged: Vec<(EdgeId, u32)> = run.iter().enumerate().map(|(k, e)| (*e, 8 * k as u32)).collect();
        s = blend_chain(&s, &tagged, Blend::Fillet(1.0), 1).expect("fillet").remove(0);
        let (area, inset) = spandrel();
        let whole = (2.0 * r * len + PI * r * r) * 5.0;
        let want = whole - area * (2.0 * len + 2.0 * PI * (r - inset));
        assert!(
            (fine_volume(std::slice::from_ref(&s)) - want).abs() / want < 5e-5,
            "{} vs {want}",
            fine_volume(std::slice::from_ref(&s))
        );
        assert_eq!(check::genus(&s).expect("valid"), 0);
    }

    #[test]
    fn fillet_a_stadium_pocket_floor() {
        // A stadium pocket 3 deep in a block: the floor's edges are concave, and their
        // fillets (one run) fill the corner along the sides and round the ends.
        use crate::boolean::{BoolOp, boolean};
        let (r, len) = (4.0, 10.0);
        let block = prism(
            &[
                DVec2::new(-10.0, -10.0),
                DVec2::new(20.0, -10.0),
                DVec2::new(20.0, 10.0),
                DVec2::new(-10.0, 10.0),
            ],
            6.0,
        );
        let reg = build_regions(&[stadium(r, len)]).expect("region");
        let pocket = extrude::extrude(&Frame::world(), &reg[0], 3.0, 7.0).expect("pocket");
        let mut s = boolean(&block, &pocket, BoolOp::Subtract).expect("pocket").remove(0);
        let edges: Vec<EdgeId> = [
            DVec3::new(0.5 * len, -r, 3.0),
            DVec3::new(len + r, 0.0, 3.0),
            DVec3::new(0.5 * len, r, 3.0),
            DVec3::new(-r, 0.0, 3.0),
        ]
        .map(|p| edge(&s, p))
        .to_vec();
        let run = smooth_run(&s, edges[0], &edges);
        assert_eq!(run.len(), 4);
        let tagged: Vec<(EdgeId, u32)> = run.iter().enumerate().map(|(k, e)| (*e, 8 * k as u32)).collect();
        s = blend_chain(&s, &tagged, Blend::Fillet(1.0), 1).expect("fillet").remove(0);
        let (area, inset) = spandrel();
        let before = 30.0 * 20.0 * 6.0 - (2.0 * r * len + PI * r * r) * 3.0;
        let want = before + area * (2.0 * len + 2.0 * PI * (r - inset));
        assert!(
            (fine_volume(std::slice::from_ref(&s)) - want).abs() / want < 1e-5,
            "{} vs {want}",
            fine_volume(std::slice::from_ref(&s))
        );
    }
}
