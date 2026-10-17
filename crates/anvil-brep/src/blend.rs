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
//! The behaviour is specified in `docs/re/shell-and-fillet.md`.

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
    let size = match blend {
        Blend::Fillet(r) | Blend::Chamfer(r) => r,
    };
    if !(size.is_finite() && size > 0.0) {
        return invalid("the size must be a positive number");
    }
    let e = solid.edge(edge).ok_or_else(|| KernelError::InvalidInput("the edge does not exist".into()))?;
    if let Curve::Circle { frame, radius } = e.curve {
        return blend_round_edge(solid, edge, &frame, radius, blend, owner, tag_base);
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
    let [(fa, _, wa), (fb, nb, wb)] = sides[..] else {
        return invalid("the edge is not shared by exactly two faces");
    };
    let bend = wa.dot(nb);
    if bend.abs() < 1e-9 {
        return invalid("the faces at this edge are tangent; there is nothing to blend");
    }
    let convex = bend < 0.0;
    let phi = wa.dot(wb).clamp(-1.0, 1.0).acos();
    let setback = match blend {
        Blend::Fillet(r) => r / (phi / 2.0).tan(),
        Blend::Chamfer(c) => c,
    };

    // The blend must fit on both faces.
    let mid = (p0 + p1) * 0.5;
    for (f, w) in [(fa, wa), (fb, wb)] {
        if !boolean::face_contains(solid, f, mid + w * setback * 0.999) {
            return invalid(format!(
                "a {} of {size} does not fit on the faces next to this edge",
                match blend {
                    Blend::Fillet(_) => "fillet",
                    Blend::Chamfer(_) => "chamfer",
                }
            ));
        }
    }

    // Cross-section in a frame with z along the edge and x into face A.
    let frame = Frame::new(p0, d, wa)?;
    let to2 = |v: DVec3| DVec2::new(v.dot(frame.x), v.dot(frame.y));
    let a = to2(wa * setback);
    let b = to2(wb * setback);
    let mut segs = vec![Segment::Line { a: DVec2::ZERO, b: a }];
    match blend {
        Blend::Fillet(r) => {
            let center = to2((wa + wb).normalize() * (r / (phi / 2.0).sin()));
            let arc = [true, false]
                .into_iter()
                .filter_map(|ccw| Segment::arc_from_center(center, a, b, ccw))
                .find(|s| matches!(s, Segment::Arc { sweep, .. } if sweep.abs() < PI))
                .ok_or_else(|| KernelError::InvalidInput("degenerate fillet".into()))?;
            segs.push(arc);
        }
        Blend::Chamfer(_) => segs.push(Segment::Line { a, b }),
    }
    segs.push(Segment::Line { a: b, b: DVec2::ZERO });
    let tags = vec![tag_base, tag_base + 2, tag_base + 4];
    let regions = build_regions(&[Loop2::with_tags(segs, tags)])?;

    // Extend past an end only where the far side is open (convex) or solid
    // (concave), so the extension changes nothing but avoids slivers.
    let margin = setback * 2.0;
    let probe = (wa + wb) * setback * 0.25;
    let extend = |end: DVec3, out: DVec3| -> Result<f64> {
        let inside = boolean::contains_point(solid, end + out * (margin * 0.5) + probe)?;
        Ok(if inside != convex { margin } else { 0.0 })
    };
    let z0 = -extend(p0, -d)?;
    let z1 = len + extend(p1, d)?;
    let region = regions.first().ok_or_else(|| KernelError::InvalidInput("degenerate blend".into()))?;
    let mut tool = extrude::extrude(&frame, region, z0, z1)?;
    tool.set_owner(owner);
    let op = if convex { BoolOp::Subtract } else { BoolOp::Union };
    boolean::boolean(solid, &tool, op)
}

/// The faces at an edge, each with its outward normal at `p` and the direction from
/// the edge into it there (`tangent` is the edge's direction at `p`).
fn edge_sides(solid: &Solid, edge: EdgeId, p: DVec3, tangent: DVec3) -> Vec<(usize, DVec3, DVec3)> {
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

/// Blends a whole circular edge (see the module notes).
fn blend_round_edge(
    solid: &Solid,
    edge: EdgeId,
    circle: &Frame,
    radius: f64,
    blend: Blend,
    owner: u32,
    tag_base: u32,
) -> Result<Vec<Solid>> {
    let e = solid.edge(edge).ok_or_else(|| KernelError::InvalidInput("the edge does not exist".into()))?;
    if ((e.t1 - e.t0).abs() - 2.0 * PI).abs() > 1e-9 {
        return Err(KernelError::Unsupported("fillets and chamfers on part of a circle".into()));
    }
    let (o, a) = (circle.origin, circle.z);
    let p = e.curve.eval(e.t0);
    let tangent = (circle.y * e.t0.cos() - circle.x * e.t0.sin()) * (e.t1 - e.t0).signum();
    let sides = edge_sides(solid, edge, p, tangent);
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
    let size = match blend {
        Blend::Fillet(r) | Blend::Chamfer(r) => r,
    };
    let bend = wa.dot(nb);
    if bend.abs() < 1e-9 {
        return invalid("the faces at this edge are tangent; there is nothing to blend");
    }
    let convex = bend < 0.0;
    let phi = wa.dot(wb).clamp(-1.0, 1.0).acos();
    let setback = match blend {
        Blend::Fillet(r) => r / (phi / 2.0).tan(),
        Blend::Chamfer(c) => c,
    };
    let too_big = || {
        invalid(format!(
            "a {} of {size} does not fit on the faces next to this edge",
            match blend {
                Blend::Fillet(_) => "fillet",
                Blend::Chamfer(_) => "chamfer",
            }
        ))
    };
    for (f, w) in [(fa, wa), (fb, wb)] {
        if !boolean::face_contains(solid, f, p + w * setback * 0.999) {
            return too_big();
        }
    }

    // The cross-section in the meridian half-plane through `p`: x away from the axis,
    // y along it, the edge at (radius, 0).
    let radial = (p - o).normalize();
    let frame = Frame { origin: o, x: radial, y: a, z: radial.cross(a) };
    let to2 = |v: DVec3| DVec2::new(v.dot(radial), v.dot(a));
    let c = DVec2::new(radius, 0.0);
    let (wa2, wb2, na2, nb2) = (to2(wa), to2(wb), to2(na), to2(nb));
    let pa = c + wa2 * setback;
    let pb = c + wb2 * setback;
    let round = match blend {
        Blend::Fillet(r) => {
            let center = c + (wa2 + wb2).normalize() * (r / (phi / 2.0).sin());
            [true, false]
                .into_iter()
                .filter_map(|ccw| Segment::arc_from_center(center, pa, pb, ccw))
                .find(|s| matches!(s, Segment::Arc { sweep, .. } if sweep.abs() < PI))
                .ok_or_else(|| KernelError::InvalidInput("degenerate fillet".into()))?
        }
        Blend::Chamfer(_) => Segment::Line { a: pa, b: pb },
    };
    // The sides pushed off the faces: out of the part at a convex edge (where the tool
    // cuts), into it at a concave one (where the tool adds).
    let push = setback * 0.5 * if convex { 1.0 } else { -1.0 };
    let (pb2, pc2, pa2) = (pb + nb2 * push, c + (na2 + nb2) * push, pa + na2 * push);
    let segs = vec![
        round,
        Segment::Line { a: pb, b: pb2 },
        Segment::Line { a: pb2, b: pc2 },
        Segment::Line { a: pc2, b: pa2 },
        Segment::Line { a: pa2, b: pa },
    ];
    let tags = vec![tag_base + 2, tag_base + 4, tag_base + 4, tag_base, tag_base];
    let regions = build_regions(&[Loop2::with_tags(segs, tags)])?;
    let region = regions.first().ok_or_else(|| KernelError::InvalidInput("degenerate blend".into()))?;
    let mut tool = match crate::revolve::revolve(&frame, region, DVec2::ZERO, DVec2::Y, 2.0 * PI) {
        Ok(t) => t,
        // The cross-section reaching the axis: the blend is wider than the circle.
        Err(KernelError::InvalidInput(_) | KernelError::Unsupported(_)) => return too_big(),
        Err(err) => return Err(err),
    };
    tool.set_owner(owner);
    let op = if convex { BoolOp::Subtract } else { BoolOp::Union };
    boolean::boolean(solid, &tool, op)
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
}
