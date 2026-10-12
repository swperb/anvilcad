//! Linear extrusion of a planar region into a solid.

use crate::geom::{Curve, Frame, Surface};
use crate::profile::{Loop2, Region, Segment};
use crate::topo::{Coedge, Edge, EdgeId, Face, FaceTag, Loop, Solid};
use crate::{KernelError, Result, check, tol};

/// Extrudes `region` (in the 2D coordinates of `frame`) between the planes
/// `z0` and `z1` measured along `frame.z`. The order of `z0` and `z1` does not
/// matter. The returned solid has passed [`check::validate`].
pub fn extrude(frame: &Frame, region: &Region, z0: f64, z1: f64) -> Result<Solid> {
    if !z0.is_finite() || !z1.is_finite() {
        return Err(KernelError::InvalidDistance);
    }
    let (z0, z1) = (z0.min(z1), z0.max(z1));
    if z1 - z0 < tol::LINEAR {
        return Err(KernelError::InvalidDistance);
    }
    let height = z1 - z0;
    let bottom = frame.offset(z0);
    let top = frame.offset(z1);

    let mut solid = Solid::default();
    let contours: Vec<&Loop2> = std::iter::once(&region.outer).chain(region.holes.iter()).collect();

    // Edges per contour, indexed by segment.
    struct Rings {
        bottom: Vec<EdgeId>,
        top: Vec<EdgeId>,
        vertical: Vec<EdgeId>,
    }
    let mut rings = Vec::with_capacity(contours.len());

    for contour in &contours {
        let n = contour.segments.len();
        let vb: Vec<_> =
            contour.segments.iter().map(|s| solid.add_vertex(bottom.to_world(s.start()))).collect();
        let vt: Vec<_> = contour.segments.iter().map(|s| solid.add_vertex(top.to_world(s.start()))).collect();
        let mut r = Rings {
            bottom: Vec::with_capacity(n),
            top: Vec::with_capacity(n),
            vertical: Vec::with_capacity(n),
        };
        for (k, seg) in contour.segments.iter().enumerate() {
            let next = (k + 1) % n;
            let (cb, t0, t1) = segment_curve(&bottom, seg);
            let (ct, _, _) = segment_curve(&top, seg);
            r.bottom.push(solid.add_edge(Edge { curve: cb, start: vb[k], end: vb[next], t0, t1 }));
            r.top.push(solid.add_edge(Edge { curve: ct, start: vt[k], end: vt[next], t0, t1 }));
            r.vertical.push(solid.add_edge(Edge {
                curve: Curve::Line { origin: bottom.to_world(seg.start()), dir: frame.z },
                start: vb[k],
                end: vt[k],
                t0: 0.0,
                t1: height,
            }));
        }
        rings.push(r);
    }

    let fwd = |edge| Coedge { edge, reversed: false };
    let rev = |edge| Coedge { edge, reversed: true };

    // End cap: profile orientation already matches the +z outward normal.
    let mut top_loops = rings.iter().map(|r| Loop { coedges: r.top.iter().copied().map(fwd).collect() });
    solid.add_face(Face {
        surface: Surface::Plane { frame: top },
        reversed: false,
        outer: top_loops.next().unwrap_or_default(),
        inners: top_loops.collect(),
        tag: FaceTag::EndCap,
        owner: 0,
    });

    // Start cap faces -z, so every loop runs backwards.
    let mut bottom_loops =
        rings.iter().map(|r| Loop { coedges: r.bottom.iter().rev().copied().map(rev).collect() });
    solid.add_face(Face {
        surface: Surface::Plane { frame: bottom },
        reversed: true,
        outer: bottom_loops.next().unwrap_or_default(),
        inners: bottom_loops.collect(),
        tag: FaceTag::StartCap,
        owner: 0,
    });

    // Side faces. Material is on the left of each contour's direction of
    // travel, so the outward normal is on the right: travel x frame.z.
    for (ci, (contour, r)) in contours.iter().zip(&rings).enumerate() {
        let n = contour.segments.len();
        for (k, seg) in contour.segments.iter().enumerate() {
            let next = (k + 1) % n;
            let (surface, reversed) = match *seg {
                Segment::Line { a, b } => {
                    let d = bottom.to_world(b) - bottom.to_world(a);
                    let normal = d.cross(frame.z);
                    let f = Frame::new(bottom.to_world(a), normal, d)?;
                    (Surface::Plane { frame: f }, false)
                }
                Segment::Arc { center, radius, sweep, .. } => (
                    Surface::Cylinder { frame: bottom.with_origin(bottom.to_world(center)), radius },
                    sweep < 0.0,
                ),
            };
            solid.add_face(Face {
                surface,
                reversed,
                outer: Loop {
                    coedges: vec![fwd(r.bottom[k]), fwd(r.vertical[next]), rev(r.top[k]), rev(r.vertical[k])],
                },
                inners: Vec::new(),
                tag: FaceTag::Side { contour: ci as u32, segment: k as u32 },
                owner: 0,
            });
        }
    }

    check::validate(&solid)?;
    Ok(solid)
}

/// The 3D curve for a 2D segment placed in `frame`, with its parameter range.
fn segment_curve(frame: &Frame, seg: &Segment) -> (Curve, f64, f64) {
    match *seg {
        Segment::Line { a, b } => {
            let pa = frame.to_world(a);
            let d = frame.to_world(b) - pa;
            let len = d.length();
            (Curve::Line { origin: pa, dir: d / len }, 0.0, len)
        }
        Segment::Arc { center, radius, start, sweep } => {
            (Curve::Circle { frame: frame.with_origin(frame.to_world(center)), radius }, start, start + sweep)
        }
    }
}

/// Extrudes every region of a profile, one solid per region.
pub fn extrude_regions(frame: &Frame, regions: &[Region], z0: f64, z1: f64) -> Result<Vec<Solid>> {
    regions.iter().map(|r| extrude(frame, r, z0, z1)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::build_regions;
    use crate::{mass, tess};
    use glam::{DVec2, DVec3};
    use proptest::prelude::*;
    use std::f64::consts::PI;

    fn polygon(pts: &[DVec2]) -> Loop2 {
        Loop2::new((0..pts.len()).map(|i| Segment::Line { a: pts[i], b: pts[(i + 1) % pts.len()] }).collect())
    }

    fn rect(w: f64, h: f64) -> Loop2 {
        polygon(&[DVec2::ZERO, DVec2::new(w, 0.0), DVec2::new(w, h), DVec2::new(0.0, h)])
    }

    fn volume(s: &Solid) -> f64 {
        let mesh = tess::tessellate(s, &tess::TessParams::default()).expect("tessellates");
        assert!(mesh.is_watertight());
        mass::properties(&mesh).volume
    }

    #[test]
    fn box_counts_and_volume() {
        let regions = build_regions(&[rect(20.0, 10.0)]).expect("valid");
        let s = extrude(&Frame::world(), &regions[0], 0.0, 5.0).expect("extrudes");
        assert_eq!((s.vertices.len(), s.edges.len(), s.faces.len()), (8, 12, 6));
        assert_eq!(check::genus(&s).expect("valid"), 0);
        assert!((volume(&s) - 1000.0).abs() < 1e-9);
    }

    #[test]
    fn negative_direction_is_the_same_solid_mirrored() {
        let regions = build_regions(&[rect(2.0, 3.0)]).expect("valid");
        let s = extrude(&Frame::world(), &regions[0], 0.0, -4.0).expect("extrudes");
        assert!((volume(&s) - 24.0).abs() < 1e-9);
        assert!(s.vertices.iter().all(|v| v.point.z <= 1e-12));
    }

    #[test]
    fn cylinder() {
        let c = Loop2::new(vec![Segment::circle(DVec2::new(1.0, 2.0), 5.0)]);
        let regions = build_regions(&[c]).expect("valid");
        let s = extrude(&Frame::world(), &regions[0], 0.0, 10.0).expect("extrudes");
        assert_eq!((s.vertices.len(), s.edges.len(), s.faces.len()), (2, 3, 3));
        assert_eq!(check::genus(&s).expect("valid"), 0);
        let exact = PI * 25.0 * 10.0;
        assert!((volume(&s) - exact).abs() / exact < 0.005);
    }

    #[test]
    fn plate_with_hole_has_genus_one() {
        let hole = Loop2::new(vec![Segment::circle(DVec2::new(10.0, 5.0), 2.0)]);
        let regions = build_regions(&[rect(20.0, 10.0), hole]).expect("valid");
        let s = extrude(&Frame::world(), &regions[0], 0.0, 3.0).expect("extrudes");
        assert_eq!(check::genus(&s).expect("valid"), 1);
        let exact = (200.0 - PI * 4.0) * 3.0;
        assert!((volume(&s) - exact).abs() / exact < 0.005);
    }

    #[test]
    fn concave_slot_with_cw_arc() {
        // A rectangle with a semicircular bite taken out of the top edge.
        let l = Loop2::new(vec![
            Segment::Line { a: DVec2::new(0.0, 0.0), b: DVec2::new(10.0, 0.0) },
            Segment::Line { a: DVec2::new(10.0, 0.0), b: DVec2::new(10.0, 5.0) },
            Segment::Line { a: DVec2::new(10.0, 5.0), b: DVec2::new(7.0, 5.0) },
            Segment::Arc { center: DVec2::new(5.0, 5.0), radius: 2.0, start: 0.0, sweep: -PI },
            Segment::Line { a: DVec2::new(3.0, 5.0), b: DVec2::new(0.0, 5.0) },
            Segment::Line { a: DVec2::new(0.0, 5.0), b: DVec2::new(0.0, 0.0) },
        ]);
        let regions = build_regions(&[l]).expect("valid");
        let s = extrude(&Frame::world(), &regions[0], 0.0, 1.0).expect("extrudes");
        let exact = 50.0 - PI * 2.0;
        assert!((volume(&s) - exact).abs() / exact < 0.005);
        // The bite's cylinder face points toward its axis.
        let bite =
            s.face_by_tag(FaceTag::Side { contour: 0, segment: 3 }).and_then(|f| s.face(f)).expect("face");
        assert!(bite.reversed);
        let n = bite.normal_at(DVec3::new(5.0, 3.0, 0.5));
        assert!((n - DVec3::Y).length() < 1e-9);
    }

    #[test]
    fn tilted_frame() {
        let f = Frame::new(DVec3::new(5.0, -2.0, 7.0), DVec3::new(1.0, 1.0, 1.0), DVec3::X).expect("frame");
        let regions = build_regions(&[rect(3.0, 4.0)]).expect("valid");
        let s = extrude(&f, &regions[0], -1.0, 2.0).expect("extrudes");
        assert!((volume(&s) - 36.0).abs() < 1e-9);
    }

    #[test]
    fn rejects_zero_depth() {
        let regions = build_regions(&[rect(1.0, 1.0)]).expect("valid");
        assert_eq!(extrude(&Frame::world(), &regions[0], 0.0, 0.0), Err(KernelError::InvalidDistance));
        assert_eq!(
            extrude(&Frame::world(), &regions[0], 0.0, f64::INFINITY),
            Err(KernelError::InvalidDistance)
        );
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(200))]

        #[test]
        fn regular_polygons_with_holes(
            sides in 3usize..24,
            radius in 0.5f64..500.0,
            rot in 0.0f64..std::f64::consts::TAU,
            hole_frac in 0.05f64..0.6,
            depth in prop_oneof![-200.0f64..-0.01, 0.01f64..200.0],
            cw in any::<bool>(),
        ) {
            let pts: Vec<DVec2> = (0..sides)
                .map(|i| DVec2::from_angle(rot + std::f64::consts::TAU * i as f64 / sides as f64) * radius)
                .collect();
            let mut outer = polygon(&pts);
            if cw { outer = outer.reversed(); }
            let inradius = radius * (PI / sides as f64).cos();
            let hr = inradius * hole_frac;
            let hole = Loop2::new(vec![Segment::circle(DVec2::ZERO, hr)]);
            let regions = build_regions(&[outer, hole]).expect("valid");
            prop_assert_eq!(regions.len(), 1);
            let s = extrude(&Frame::world(), &regions[0], 0.0, depth).expect("extrudes");
            prop_assert_eq!(check::genus(&s).expect("valid"), 1);
            let poly_area = 0.5 * sides as f64 * radius * radius * (std::f64::consts::TAU / sides as f64).sin();
            let exact = (poly_area - PI * hr * hr) * depth.abs();
            let v = volume(&s);
            prop_assert!((v - exact).abs() / exact < 0.01, "volume {v} vs {exact}");
        }
    }
}
