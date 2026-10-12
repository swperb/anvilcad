//! Fillets and chamfers on straight edges between planar faces.
//!
//! A blend is made with an exact boolean: a prism whose cross-section is the
//! material between the edge and the fillet arc (or chamfer line) is cut away
//! from a convex edge, or added to a concave one. The prism is extended past
//! an edge end only where nothing lies beyond it, so it never bites into
//! neighbouring geometry.

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

/// Blends one edge. New faces are tagged with `owner`.
pub fn blend_edge(solid: &Solid, edge: EdgeId, blend: Blend, owner: u32) -> Result<Vec<Solid>> {
    let size = match blend {
        Blend::Fillet(r) | Blend::Chamfer(r) => r,
    };
    if !(size.is_finite() && size > 0.0) {
        return invalid("the size must be a positive number");
    }
    let e = solid.edge(edge).ok_or_else(|| KernelError::InvalidInput("the edge does not exist".into()))?;
    let Curve::Line { .. } = e.curve else {
        return Err(KernelError::Unsupported("fillets and chamfers on curved edges".into()));
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
    let regions = build_regions(&[Loop2::new(segs)])?;

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
        let r = blend_edge(&c, edge(&c, DVec3::new(5.0, 0.0, 10.0)), Blend::Fillet(2.0), 7).expect("fillet");
        assert_eq!(r.len(), 1);
        let exact = 1000.0 - (4.0 - PI) * 10.0;
        assert!((volume(&r) - exact).abs() < 0.05, "{}", volume(&r));
        // The round face belongs to the fillet.
        assert!(r[0].faces.iter().any(|f| f.owner == 7 && matches!(f.surface, Surface::Cylinder { .. })));
    }

    #[test]
    fn chamfer_one_cube_edge() {
        let c = cube(10.0);
        let r =
            blend_edge(&c, edge(&c, DVec3::new(10.0, 5.0, 10.0)), Blend::Chamfer(3.0), 1).expect("chamfer");
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
            s = blend_edge(&s, e, Blend::Fillet(2.0), 1).expect("fillet").remove(0);
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
            s = blend_edge(&s, e, Blend::Chamfer(2.0), 1).expect("chamfer").remove(0);
        }
        let v = volume(std::slice::from_ref(&s));
        assert!(v < 1000.0 - 3.0 * 2.0 * 6.0 && v > 1000.0 - 3.0 * 2.0 * 10.0, "{v}");
    }

    #[test]
    fn fillet_three_edges_at_a_corner() {
        let mut s = cube(10.0);
        for mid in [DVec3::new(5.0, 0.0, 10.0), DVec3::new(0.0, 5.0, 10.0), DVec3::new(0.0, 0.0, 5.0)] {
            let e = edge(&s, mid);
            s = blend_edge(&s, e, Blend::Fillet(2.0), 1).expect("fillet").remove(0);
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
        let r = blend_edge(&l, edge(&l, DVec3::new(4.0, 4.0, 2.5)), Blend::Fillet(2.0), 1).expect("fillet");
        assert_eq!(r.len(), 1);
        let exact = before + (4.0 - PI) * 5.0;
        assert!((volume(&r) - exact).abs() < 0.05, "{}", volume(&r));
    }

    #[test]
    fn too_large_and_curved_edges_are_refused() {
        let c = cube(10.0);
        let e = edge(&c, DVec3::new(5.0, 0.0, 10.0));
        assert!(matches!(blend_edge(&c, e, Blend::Fillet(20.0), 1), Err(KernelError::InvalidInput(_))));
        assert!(blend_edge(&c, e, Blend::Fillet(-1.0), 1).is_err());
        let l = Loop2::new(vec![Segment::circle(DVec2::ZERO, 3.0)]);
        let r = build_regions(&[l]).expect("region");
        let cyl = extrude::extrude(&Frame::world(), &r[0], 0.0, 5.0).expect("cyl");
        let rim = edge(&cyl, DVec3::new(3.0, 0.0, 5.0));
        assert!(matches!(blend_edge(&cyl, rim, Blend::Fillet(1.0), 1), Err(KernelError::Unsupported(_))));
    }
}
