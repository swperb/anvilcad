//! Solid construction from profiles: extrusion and revolution.

use crate::math::{v3, Frame, Vec2, Vec3};
use crate::profile::{Loop2, Profile, Seg2};
use crate::solid::{FaceId, FaceTag, Polygon, Solid, SurfaceKind};
use crate::triangulate::triangulate;
use std::f64::consts::TAU;

/// Chord tolerance used when faceting curved geometry (mm).
pub const CHORD_TOL: f64 = 0.01;

#[derive(Debug, Clone, PartialEq)]
pub enum BuildError {
    EmptyProfile,
    ZeroDepth,
    ZeroAngle,
    ProfileCrossesAxis,
}

impl std::fmt::Display for BuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BuildError::EmptyProfile => write!(f, "the sketch has no closed profile"),
            BuildError::ZeroDepth => write!(f, "depth must be non-zero"),
            BuildError::ZeroAngle => write!(f, "angle must be non-zero"),
            BuildError::ProfileCrossesAxis => {
                write!(f, "the profile crosses the axis of revolution")
            }
        }
    }
}

impl std::error::Error for BuildError {}

fn plane_tag(feature: u32, index: u32) -> FaceTag {
    FaceTag { id: FaceId { feature, index }, surface: SurfaceKind::Plane }
}

/// Cap polygons (triangles) for a profile placed by `place`.
fn cap(profile: &Profile, tag: FaceTag, flip: bool, place: impl Fn(Vec2) -> Vec3, out: &mut Vec<Polygon>) {
    let outer = profile.outer.polyline(CHORD_TOL);
    let holes: Vec<Vec<Vec2>> = profile.holes.iter().map(|h| h.polyline(CHORD_TOL)).collect();
    let (pts, tris) = triangulate(&outer, &holes);
    for t in tris {
        let mut vs = vec![place(pts[t[0]]), place(pts[t[1]]), place(pts[t[2]])];
        if flip {
            vs.reverse();
        }
        out.extend(Polygon::new(vs, tag));
    }
}

/// Extrudes profiles lying in `frame`'s XY plane from z = `z0` to z = `z1`.
/// Face ids are derived from `feature` and each segment's sketch entity.
pub fn extrude(
    profiles: &[Profile],
    frame: &Frame,
    z0: f64,
    z1: f64,
    feature: u32,
) -> Result<Solid, BuildError> {
    if profiles.is_empty() {
        return Err(BuildError::EmptyProfile);
    }
    if (z1 - z0).abs() < 1e-9 {
        return Err(BuildError::ZeroDepth);
    }
    let (lo, hi) = (z0.min(z1), z0.max(z1));
    // The end cap is the face away from the sketch plane.
    let (lo_idx, hi_idx) =
        if z1 >= z0 { (FaceId::START_CAP, FaceId::END_CAP) } else { (FaceId::END_CAP, FaceId::START_CAP) };
    let mut polys = Vec::new();
    for prof in profiles {
        let mut prof = prof.clone();
        prof.normalize_orientation();
        let at = |z: f64| move |p: Vec2| frame.to_world3(v3(p.x, p.y, z));
        cap(&prof, plane_tag(feature, lo_idx), true, at(lo), &mut polys);
        cap(&prof, plane_tag(feature, hi_idx), false, at(hi), &mut polys);
        for lp in prof.loops() {
            side_walls(lp, frame, lo, hi, feature, &mut polys);
        }
    }
    Ok(Solid::new(polys))
}

fn side_walls(lp: &Loop2, frame: &Frame, lo: f64, hi: f64, feature: u32, out: &mut Vec<Polygon>) {
    let samples = lp.samples(CHORD_TOL);
    let n = samples.len();
    for i in 0..n {
        let (a, b) = (samples[i].p, samples[(i + 1) % n].p);
        let seg = &lp.segs[samples[i].seg];
        let id = FaceId::side(feature, seg.source());
        let surface = match *seg {
            Seg2::Line { .. } => SurfaceKind::Plane,
            Seg2::Arc { center, radius, .. } => {
                SurfaceKind::Cylinder { axis_origin: frame.to_world(center), axis_dir: frame.z, radius }
            }
        };
        let w = |p: Vec2, z: f64| frame.to_world3(v3(p.x, p.y, z));
        out.extend(Polygon::new(vec![w(a, lo), w(b, lo), w(b, hi), w(a, hi)], FaceTag { id, surface }));
    }
}

/// Revolves profiles about an axis lying in the sketch plane, given by a point and
/// direction in sketch coordinates. `angle` is in radians; a full turn has no caps.
pub fn revolve(
    profiles: &[Profile],
    frame: &Frame,
    axis_point: Vec2,
    axis_dir: Vec2,
    angle: f64,
    feature: u32,
) -> Result<Solid, BuildError> {
    if profiles.is_empty() {
        return Err(BuildError::EmptyProfile);
    }
    if angle.abs() < 1e-9 {
        return Err(BuildError::ZeroAngle);
    }
    let angle = angle.clamp(-TAU, TAU);
    let full = angle.abs() >= TAU - 1e-9;
    let d = axis_dir.normalized();

    // All profile points must lie on one side of the axis.
    let mut side = 0.0f64;
    for prof in profiles {
        for lp in prof.loops() {
            for p in lp.polyline(CHORD_TOL) {
                let s = d.cross(p - axis_point);
                if s.abs() > 1e-7 {
                    if side == 0.0 {
                        side = s.signum();
                    } else if s.signum() != side {
                        return Err(BuildError::ProfileCrossesAxis);
                    }
                }
            }
        }
    }
    if side == 0.0 {
        return Err(BuildError::EmptyProfile);
    }

    let axis_o = frame.to_world(axis_point);
    let axis_w = (frame.x * d.x + frame.y * d.y).normalized();
    let radial = (frame.x * (-d.y) + frame.y * d.x) * side; // in-plane, toward the profile
    let normal = frame.z;
    let max_r = profiles
        .iter()
        .flat_map(|p| p.loops())
        .flat_map(|lp| lp.polyline(CHORD_TOL))
        .map(|p| d.cross(p - axis_point).abs())
        .fold(0.0, f64::max);
    let steps = crate::profile::arc_segments(max_r, angle, CHORD_TOL).max(3);

    // (axial, radial) coordinates of a sketch point.
    let to_hr = |p: Vec2| -> (f64, f64) {
        let q = p - axis_point;
        (q.dot(d), d.cross(q).abs())
    };
    let place = |h: f64, r: f64, t: f64| axis_o + axis_w * h + (radial * t.cos() + normal * t.sin()) * r;

    let mut polys = Vec::new();
    for prof in profiles {
        let mut prof = prof.clone();
        prof.normalize_orientation();
        for lp in prof.loops() {
            let samples = lp.samples(CHORD_TOL);
            let n = samples.len();
            for i in 0..n {
                let (h0, r0) = to_hr(samples[i].p);
                let (h1, r1) = to_hr(samples[(i + 1) % n].p);
                let seg = &lp.segs[samples[i].seg];
                let tag = FaceTag {
                    id: FaceId::side(feature, seg.source()),
                    surface: SurfaceKind::Revolved { axis_origin: axis_o, axis_dir: axis_w },
                };
                for s in 0..steps {
                    let t0 = angle * s as f64 / steps as f64;
                    let t1 = angle * (s + 1) as f64 / steps as f64;
                    let mut vs =
                        vec![place(h0, r0, t0), place(h1, r1, t0), place(h1, r1, t1), place(h0, r0, t1)];
                    // Collapse points on the axis into triangles.
                    vs.dedup_by(|a, b| a.distance(*b) < 1e-9);
                    if vs.len() > 1 && vs[0].distance(vs[vs.len() - 1]) < 1e-9 {
                        vs.pop();
                    }
                    polys.extend(Polygon::new(vs, tag));
                }
            }
        }
        if !full {
            let start = |p: Vec2| {
                let (h, r) = to_hr(p);
                place(h, r, 0.0)
            };
            let end = |p: Vec2| {
                let (h, r) = to_hr(p);
                place(h, r, angle)
            };
            cap(&prof, plane_tag(feature, FaceId::START_CAP), false, start, &mut polys);
            cap(&prof, plane_tag(feature, FaceId::END_CAP), true, end, &mut polys);
        }
    }

    // Orientation depends on the side and sweep direction; normalize so volume is positive.
    let mut solid = Solid::new(polys);
    if solid.mass_properties().volume < 0.0 {
        solid.flip();
    }
    Ok(solid)
}

/// Axis-aligned box primitive, mostly for tests and quick modelling.
pub fn block(min: Vec3, max: Vec3, feature: u32) -> Solid {
    let frame = Frame { origin: v3(0.0, 0.0, 0.0), ..Frame::WORLD };
    let prof = Profile::new(
        Loop2::rectangle(crate::math::v2(min.x, min.y), crate::math::v2(max.x, max.y), 0),
        vec![],
    );
    extrude(&[prof], &frame, min.z, max.z, feature).expect("valid box")
}

/// Cylinder primitive along +Z.
pub fn cylinder(center: Vec3, radius: f64, height: f64, feature: u32) -> Solid {
    let frame = Frame { origin: center, ..Frame::WORLD };
    let prof = Profile::new(Loop2::circle(Vec2::ZERO, radius, 0), vec![]);
    extrude(&[prof], &frame, 0.0, height, feature).expect("valid cylinder")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::v2;
    use std::f64::consts::PI;

    fn close(a: f64, b: f64, rel: f64) -> bool {
        (a - b).abs() <= rel * b.abs().max(1.0)
    }

    #[test]
    fn extruded_box_volume() {
        let s = block(v3(0.0, 0.0, 0.0), v3(10.0, 20.0, 30.0), 1);
        let m = s.mass_properties();
        assert!(close(m.volume, 6000.0, 1e-9), "{}", m.volume);
        assert!(close(m.surface_area, 2.0 * (200.0 + 300.0 + 600.0), 1e-9));
        assert!((m.centroid - v3(5.0, 10.0, 15.0)).length() < 1e-9);
    }

    #[test]
    fn negative_extrude_is_positive_volume() {
        let prof = Profile::new(Loop2::rectangle(v2(0.0, 0.0), v2(1.0, 1.0), 0), vec![]);
        let s = extrude(&[prof], &Frame::WORLD, 0.0, -5.0, 1).unwrap();
        assert!(close(s.mass_properties().volume, 5.0, 1e-9));
    }

    #[test]
    fn plate_with_hole() {
        let prof = Profile::new(
            Loop2::rectangle(v2(-10.0, -10.0), v2(10.0, 10.0), 0),
            vec![Loop2::circle(v2(0.0, 0.0), 3.0, 4)],
        );
        let s = extrude(&[prof], &Frame::WORLD, 0.0, 2.0, 1).unwrap();
        let v = s.mass_properties().volume;
        assert!(close(v, (400.0 - PI * 9.0) * 2.0, 1e-3), "{v}");
        // Hole wall is tagged as a cylinder from entity 4.
        assert!(
            s.polygons
                .iter()
                .any(|p| p.tag.id == FaceId::side(1, 4)
                    && matches!(p.tag.surface, SurfaceKind::Cylinder { .. }))
        );
    }

    #[test]
    fn revolve_cylinder() {
        // Rectangle from radius 0..5, height 0..10, revolved about the sketch Y axis.
        let prof = Profile::new(Loop2::rectangle(v2(0.0, 0.0), v2(5.0, 10.0), 0), vec![]);
        let s = revolve(&[prof], &Frame::WORLD, Vec2::ZERO, v2(0.0, 1.0), TAU, 1).unwrap();
        let v = s.mass_properties().volume;
        // Faceting under-approximates the true volume slightly.
        assert!(close(v, PI * 25.0 * 10.0, 5e-3), "{v}");
    }

    #[test]
    fn revolve_half_turn_tube() {
        let prof = Profile::new(Loop2::rectangle(v2(2.0, 0.0), v2(4.0, 1.0), 0), vec![]);
        let s = revolve(&[prof], &Frame::WORLD, Vec2::ZERO, v2(0.0, 1.0), PI, 1).unwrap();
        let v = s.mass_properties().volume;
        assert!(close(v, 0.5 * PI * (16.0 - 4.0), 1e-2), "{v}");
    }

    #[test]
    fn revolve_rejects_axis_crossing() {
        let prof = Profile::new(Loop2::rectangle(v2(-1.0, 0.0), v2(1.0, 1.0), 0), vec![]);
        assert_eq!(
            revolve(&[prof], &Frame::WORLD, Vec2::ZERO, v2(0.0, 1.0), TAU, 1).unwrap_err(),
            BuildError::ProfileCrossesAxis
        );
    }

    #[test]
    fn boolean_box_minus_cylinder() {
        let b = block(v3(0.0, 0.0, 0.0), v3(10.0, 10.0, 10.0), 1);
        let c = cylinder(v3(5.0, 5.0, -1.0), 2.0, 12.0, 2);
        let r = b.subtract(&c);
        let v = r.mass_properties().volume;
        let hole = c.mass_properties().volume / 12.0 * 10.0;
        assert!(close(v, 1000.0 - hole, 1e-6), "{v}");
        // Hole wall polygons are tagged with the cutting feature.
        assert!(r.polygons.iter().any(|p| p.tag.id.feature == 2));
        assert_eq!(r.open_edges(), 0, "result must be watertight");
    }

    #[test]
    fn boolean_union_and_intersection() {
        let a = block(v3(0.0, 0.0, 0.0), v3(10.0, 10.0, 10.0), 1);
        let b = block(v3(5.0, 5.0, 5.0), v3(15.0, 15.0, 15.0), 2);
        assert!(close(a.union(&b).mass_properties().volume, 2000.0 - 125.0, 1e-9));
        assert!(close(a.intersect(&b).mass_properties().volume, 125.0, 1e-9));
        assert!(close(a.subtract(&b).mass_properties().volume, 875.0, 1e-9));
    }

    #[test]
    fn coplanar_union() {
        // Two boxes sharing a face.
        let a = block(v3(0.0, 0.0, 0.0), v3(10.0, 10.0, 10.0), 1);
        let b = block(v3(10.0, 0.0, 0.0), v3(20.0, 10.0, 10.0), 2);
        assert!(close(a.union(&b).mass_properties().volume, 2000.0, 1e-9));
    }

    #[test]
    fn raycast_hits_top() {
        let b = block(v3(0.0, 0.0, 0.0), v3(10.0, 10.0, 10.0), 7);
        let hit = b.raycast(v3(5.0, 5.0, 100.0), v3(0.0, 0.0, -1.0)).unwrap();
        assert!((hit.t - 90.0).abs() < 1e-9);
        assert_eq!(hit.tag.id, FaceId { feature: 7, index: FaceId::END_CAP });
        let f = b.planar_face_frame(hit.tag.id).unwrap();
        assert!((f.z - Vec3::Z).length() < 1e-12);
        assert!((f.origin.z - 10.0).abs() < 1e-12);
    }
}
