//! Linear extrusion of a planar region into a solid.

use crate::geom::{Curve, Frame, Surface};
use crate::profile::{Loop2, Region, Segment};
use crate::topo::{Coedge, Edge, EdgeId, Face, FaceTag, Loop, Solid};
use crate::{KernelError, Result, check, tol};
use glam::{DVec2, DVec3};

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
                Segment::Ellipse { .. } => elliptic_side(&bottom, seg, height)?,
            };
            solid.add_face(Face {
                surface,
                reversed,
                outer: Loop {
                    coedges: vec![fwd(r.bottom[k]), fwd(r.vertical[next]), rev(r.top[k]), rev(r.vertical[k])],
                },
                inners: Vec::new(),
                tag: FaceTag::Side { contour: ci as u32, segment: contour.tag(k) },
                owner: 0,
            });
        }
    }

    check::validate(&solid)?;
    Ok(solid)
}

/// The 3D curve for a 2D segment placed in `frame`, with its parameter range.
pub(crate) fn segment_curve(frame: &Frame, seg: &Segment) -> (Curve, f64, f64) {
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
        Segment::Ellipse { center, a, b, rotation, start, sweep } => {
            let u = DVec2::from_angle(rotation);
            let f = Frame {
                origin: frame.to_world(center),
                x: frame.x * u.x + frame.y * u.y,
                y: frame.x * -u.y + frame.y * u.x,
                z: frame.z,
            };
            (Curve::Ellipse { frame: f, a, b }, start, start + sweep)
        }
    }
}

/// The side an elliptic arc in `bottom` sweeps when extruded `height` along its normal,
/// and whether its natural normal points into the solid. There is no elliptic cylinder
/// among the surfaces, so the side is the exact rational patch ruled between the arc at
/// the bottom and at the top: the arc as a rational quadratic in pieces of at most a
/// quarter turn (the circle's, carried over by the ellipse's affine map).
fn elliptic_side(bottom: &Frame, seg: &Segment, height: f64) -> Result<(Surface, bool)> {
    let Segment::Ellipse { center, a, b, rotation, start, sweep } = *seg else {
        return Err(KernelError::InvalidInput("not an elliptic arc".into()));
    };
    let u = DVec2::from_angle(rotation);
    let (ua, vb) = (u * a, u.perp() * b);
    let at = |t: f64, scale: f64| bottom.to_world(center + (ua * t.cos() + vb * t.sin()) * scale);
    let k = crate::loft::quarter_pieces(sweep);
    let phi = sweep / k as f64;
    let w = (0.5 * phi).cos();
    let mut row = vec![at(start, 1.0)];
    let mut weights = vec![1.0];
    for j in 0..k {
        let t0 = start + phi * j as f64;
        // The middle point of a piece, off the curve where the end tangents meet: the
        // circle's (cos, sin) of the middle angle over cos(phi / 2), mapped.
        let m = t0 + 0.5 * phi;
        row.push(bottom.to_world(center + (ua * m.cos() + vb * m.sin()) / w));
        row.push(at(t0 + phi, 1.0));
        weights.extend([w, 1.0]);
    }
    let top: Vec<DVec3> = row.iter().map(|p| *p + bottom.z * height).collect();
    let surface = crate::loft::ruled_patch(2, crate::loft::bezier_knots(2, k), &row, &top, &weights)?;
    // Material is to the left of travel, so outward is travel x the extrusion direction.
    let mid = bottom.to_world(seg.midpoint()) + bottom.z * (0.5 * height);
    let travel = seg.travel_at(0.5);
    let outward = (bottom.x * travel.x + bottom.y * travel.y).cross(bottom.z);
    let reversed = surface.normal_at(mid).dot(outward) < 0.0;
    Ok((surface, reversed))
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
    fn ellipses_extrude_exactly() {
        let measure = |s: &Solid| {
            check::validate(s).expect("valid");
            let m = mass::exact_properties(s).expect("measure");
            (m.volume, m.area)
        };
        let rel = |x: f64, want: f64| ((x - want) / want).abs();
        // brep-bench's geometry/elliptic-cylinder: semi-axes 5 and 3, 10 high. Volume
        // 150 pi; area 30 pi + 200 E(16/25).
        let ellipse = Loop2::new(vec![Segment::ellipse(DVec2::ZERO, 5.0, 3.0, 0.0)]);
        let r = build_regions(&[ellipse]).expect("valid");
        let s = extrude(&Frame::world(), &r[0], 0.0, 10.0).expect("extrudes");
        assert_eq!(s.faces.len(), 3);
        assert!(s.edges.iter().any(|e| matches!(e.curve, crate::geom::Curve::Ellipse { .. })));
        let (v, a) = measure(&s);
        assert!(rel(v, 150.0 * PI) < 1e-8 && rel(a, 349.517_768_241_675_07) < 1e-8, "{v} {a}");
        // brep-bench's geometry/elliptic-hole: a block 24 x 16 x 8 with a hole of semi-axes 6
        // and 3 through it. Volume 3072 - 144 pi; area 1408 - 36 pi + 192 E(3/4).
        let hole = Loop2::new(vec![Segment::ellipse(DVec2::new(12.0, 8.0), 6.0, 3.0, 0.0)]);
        let r = build_regions(&[rect(24.0, 16.0), hole]).expect("valid");
        let s = extrude(&Frame::world(), &r[0], 0.0, 8.0).expect("extrudes");
        let (v, a) = measure(&s);
        assert!(rel(v, 3072.0 - 144.0 * PI) < 1e-8 && rel(a, 1_527.425_421_763_911_6) < 1e-8, "{v} {a}");
        // Half an ellipse turned a little, closed by its axis: a D-shaped prism of volume
        // 2 (15 pi / 2), its flat side 6 by 2.
        let u = DVec2::from_angle(0.4);
        let half = Loop2::new(vec![
            Segment::Ellipse {
                center: DVec2::ZERO,
                a: 5.0,
                b: 3.0,
                rotation: 0.4,
                start: -PI / 2.0,
                sweep: PI,
            },
            Segment::Line { a: u.perp() * 3.0, b: -u.perp() * 3.0 },
        ]);
        let r = build_regions(&[half]).expect("valid");
        let s = extrude(&Frame::world(), &r[0], 0.0, 2.0).expect("extrudes");
        let (v, _) = measure(&s);
        assert!(rel(v, 15.0 * PI) < 1e-8, "{v}");
        // Operations that take only lines and arcs so far say so.
        let ellipse = Loop2::new(vec![Segment::ellipse(DVec2::new(10.0, 0.0), 5.0, 3.0, 0.0)]);
        let r = build_regions(&[ellipse]).expect("valid");
        let err =
            crate::revolve::revolve(&Frame::world(), &r[0], DVec2::ZERO, DVec2::Y, PI).expect_err("revolve");
        assert!(matches!(err, crate::KernelError::Unsupported(_)), "{err}");
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
