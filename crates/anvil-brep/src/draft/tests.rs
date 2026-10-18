//! Testable statements 10 to 17 of `docs/re/rib-draft-dome.md`, and a few more.

use super::*;
use crate::boolean::{BoolOp, boolean};
use crate::extrude::extrude;
use crate::profile::{Loop2, Segment, build_regions};
use crate::{mass, tess};
use glam::DVec2;
use std::f64::consts::PI;

fn prism(loops: &[Loop2], z0: f64, z1: f64) -> Solid {
    let r = build_regions(loops).expect("region");
    extrude(&Frame::world(), &r[0], z0, z1).expect("prism")
}

fn polygon(pts: &[(f64, f64)]) -> Loop2 {
    let p: Vec<DVec2> = pts.iter().map(|&(x, y)| DVec2::new(x, y)).collect();
    Loop2::new((0..p.len()).map(|i| Segment::Line { a: p[i], b: p[(i + 1) % p.len()] }).collect())
}

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Loop2 {
    polygon(&[(x0, y0), (x1, y0), (x1, y1), (x0, y1)])
}

fn volume(s: &Solid) -> f64 {
    check::validate(s).expect("valid");
    let m = tess::tessellate(s, &tess::TessParams { chord: 1e-4, max_angle: 0.01 }).expect("tess");
    assert!(m.is_watertight(), "not watertight");
    mass::properties(&m).volume
}

fn close(a: f64, b: f64, rel: f64) -> bool {
    (a - b).abs() <= rel * b.abs().max(1.0)
}

/// The planar faces whose outward normal is horizontal: the sides of a box.
fn sides(s: &Solid) -> Vec<usize> {
    (0..s.faces.len())
        .filter(|&f| matches!(s.faces[f].surface, Surface::Plane { .. }))
        .filter(|&f| s.faces[f].normal_at(DVec3::ZERO).z.abs() < 1e-9)
        .collect()
}

/// The face whose outward normal is `n` and which passes through `p`.
fn face_at(s: &Solid, p: DVec3, n: DVec3) -> usize {
    (0..s.faces.len())
        .find(|&f| s.faces[f].surface.distance(p) < 1e-9 && s.faces[f].normal_at(p).dot(n) > 1.0 - 1e-9)
        .expect("face")
}

/// The extent along x of the vertices at height `z`.
fn width_at(s: &Solid, z: f64) -> f64 {
    let xs: Vec<f64> =
        s.vertices.iter().filter(|v| (v.point.z - z).abs() < 1e-9).map(|v| v.point.x).collect();
    assert!(!xs.is_empty(), "no vertices at z = {z}");
    xs.iter().copied().fold(f64::MIN, f64::max) - xs.iter().copied().fold(f64::MAX, f64::min)
}

fn deg(a: f64) -> f64 {
    a.to_radians()
}

fn plane_at(z: f64) -> Frame {
    Frame::world().offset(z)
}

#[test]
fn box_sides_drafted_about_its_base() {
    // Statement 10: a 20 x 20 x 10 box, its four sides drafted 5 degrees about z = 0.
    let b = prism(&[rect(0.0, 0.0, 20.0, 20.0)], 0.0, 10.0);
    let d = draft_faces(&b, &sides(&b), &Frame::world(), deg(5.0)).expect("draft");
    let a = 20.0 - 20.0 * deg(5.0).tan();
    assert!(close(a, 18.250227, 1e-7));
    assert!(close(width_at(&d, 10.0), a, 1e-12), "{}", width_at(&d, 10.0));
    assert!(close(width_at(&d, 0.0), 20.0, 1e-12));
    let want = 10.0 / 3.0 * (400.0 + a * a + 20.0 * a);
    assert!(close(want, 3660.251034, 1e-9));
    assert!(close(volume(&d), want, 1e-9), "{} vs {want}", volume(&d));
    // Same faces, in the same places, with the same names.
    assert_eq!(d.faces.len(), 6);
    assert_eq!((d.edges.len(), d.vertices.len()), (b.edges.len(), b.vertices.len()));
    for (x, y) in b.faces.iter().zip(&d.faces) {
        assert_eq!((x.tag, x.owner, x.reversed), (y.tag, y.owner, y.reversed));
    }

    // Statement 11: Reverse (a negative angle) widens it.
    let d = draft_faces(&b, &sides(&b), &Frame::world(), -deg(5.0)).expect("draft");
    let a = 20.0 + 20.0 * deg(5.0).tan();
    assert!(close(width_at(&d, 10.0), a, 1e-12) && close(a, 21.749773, 1e-7));
    assert!(close(volume(&d), 4360.160342, 1e-9), "{}", volume(&d));
}

#[test]
fn box_drafted_about_its_top_and_its_middle() {
    let b = prism(&[rect(0.0, 0.0, 20.0, 20.0)], 0.0, 10.0);
    // Statement 12: about the top face (normal +z, out of the body) the box widens
    // towards its bottom.
    let d = draft_faces(&b, &sides(&b), &plane_at(10.0), deg(5.0)).expect("draft");
    assert!(close(width_at(&d, 10.0), 20.0, 1e-12));
    assert!(close(width_at(&d, 0.0), 21.749773, 1e-7));
    assert!(close(volume(&d), 4360.160342, 1e-9), "{}", volume(&d));
    // Statement 14: about z = 5 the faces cross the neutral plane and tilt as a whole.
    let d = draft_faces(&b, &sides(&b), &plane_at(5.0), deg(5.0)).expect("draft");
    assert!(close(width_at(&d, 10.0), 19.125113, 1e-7));
    assert!(close(width_at(&d, 0.0), 20.874887, 1e-7));
    let want = 4000.0 + 1000.0 / 3.0 * deg(5.0).tan().powi(2);
    assert!(close(want, 4002.551422, 1e-9));
    assert!(close(volume(&d), want, 1e-9), "{} vs {want}", volume(&d));
}

#[test]
fn one_side_drafted() {
    // Statement 13: only the face x = 20; the top and the sides y = 0, 20 are trimmed.
    let b = prism(&[rect(0.0, 0.0, 20.0, 20.0)], 0.0, 10.0);
    let f = face_at(&b, DVec3::new(20.0, 10.0, 5.0), DVec3::X);
    let d = draft_faces(&b, &[f], &Frame::world(), deg(5.0)).expect("draft");
    assert!(close(width_at(&d, 10.0), 19.125113, 1e-7));
    let want = 4000.0 - 1000.0 * deg(5.0).tan();
    assert!(close(want, 3912.511336, 1e-9));
    assert!(close(volume(&d), want, 1e-9), "{} vs {want}", volume(&d));
}

#[test]
fn cylinder_becomes_a_cone() {
    // Statement 15: radius 10, height 20, drafted 5 degrees about z = 0.
    let c = prism(&[Loop2::new(vec![Segment::circle(DVec2::ZERO, 10.0)])], 0.0, 20.0);
    let side = (0..c.faces.len()).find(|&f| matches!(c.faces[f].surface, Surface::Cylinder { .. }));
    let side = side.expect("side");
    let d = draft_faces(&c, &[side], &Frame::world(), deg(5.0)).expect("draft");
    let Surface::Cone { half_angle, .. } = d.faces[side].surface else { panic!("not a cone") };
    assert!(close(half_angle, deg(5.0), 1e-12));
    let r1 = 10.0 - 20.0 * deg(5.0).tan();
    assert!(close(r1, 8.250227, 1e-7));
    let want = PI * 20.0 / 3.0 * (100.0 + 10.0 * r1 + r1 * r1);
    assert!(close(want, 5247.894568, 1e-9));
    assert!(close(volume(&d), want, 3e-5), "{} vs {want}", volume(&d));
    assert_eq!(d.faces.len(), c.faces.len());
    // The top circle is exact, and the solid is the one a drafted extrusion builds.
    let top = d.edges.iter().find_map(|e| match e.curve {
        Curve::Circle { frame, radius } if (frame.origin.z - 20.0).abs() < 1e-9 => Some(radius),
        _ => None,
    });
    assert!(close(top.expect("top circle"), r1, 1e-12));
    let region = build_regions(&[Loop2::new(vec![Segment::circle(DVec2::ZERO, 10.0)])]).expect("region");
    let tapered = crate::taper::extrude_drafted(&Frame::world(), &region[0], 0.0, 20.0, -deg(5.0).tan())
        .expect("taper");
    assert!(close(volume(&d), volume(&tapered), 1e-9), "{} vs {}", volume(&d), volume(&tapered));
    // Drafting the cone again about z = 20 makes it 10 degrees there, keeping its top.
    let again = draft_faces(&d, &[side], &plane_at(20.0), deg(10.0)).expect("again");
    let r0 = r1 + 20.0 * deg(10.0).tan();
    let want = PI * 20.0 / 3.0 * (r0 * r0 + r0 * r1 + r1 * r1);
    assert!(close(volume(&again), want, 3e-5), "{} vs {want}", volume(&again));
}

#[test]
fn hole_widens_in_the_direction_of_pull() {
    // A block with a hole of radius 5 through it, the hole's wall drafted 5 degrees
    // about z = 0: less material towards the top, so the hole opens upwards.
    let block = prism(&[rect(0.0, 0.0, 20.0, 20.0)], 0.0, 10.0);
    let hole = prism(&[Loop2::new(vec![Segment::circle(DVec2::new(10.0, 10.0), 5.0)])], -1.0, 11.0);
    let holed = boolean(&block, &hole, BoolOp::Subtract).expect("hole").remove(0);
    let wall = (0..holed.faces.len()).find(|&f| matches!(holed.faces[f].surface, Surface::Cylinder { .. }));
    let d = draft_faces(&holed, &[wall.expect("wall")], &Frame::world(), deg(5.0)).expect("draft");
    let r1 = 5.0 + 10.0 * deg(5.0).tan();
    let want = 4000.0 - PI * 10.0 / 3.0 * (25.0 + 5.0 * r1 + r1 * r1);
    assert!(close(volume(&d), want, 3e-5), "{} vs {want}", volume(&d));
}

#[test]
fn rounded_corners_draft_as_a_tangent_chain() {
    // A 20 x 10 rectangle with corners rounded to radius 3, 10 high, all its sides
    // (planes and quarter cylinders, meeting smoothly) drafted 5 degrees about z = 0.
    // Each section is the profile offset in by z tan 5, of area A - P d + pi d^2.
    let (w, h, r) = (20.0, 10.0, 3.0);
    let arc = |cx: f64, cy: f64, start: f64| Segment::Arc {
        center: DVec2::new(cx, cy),
        radius: r,
        start,
        sweep: 0.5 * PI,
    };
    let profile = Loop2::new(vec![
        Segment::Line { a: DVec2::new(r, 0.0), b: DVec2::new(w - r, 0.0) },
        arc(w - r, r, -0.5 * PI),
        Segment::Line { a: DVec2::new(w, r), b: DVec2::new(w, h - r) },
        arc(w - r, h - r, 0.0),
        Segment::Line { a: DVec2::new(w - r, h), b: DVec2::new(r, h) },
        arc(r, h - r, 0.5 * PI),
        Segment::Line { a: DVec2::new(0.0, h - r), b: DVec2::new(0.0, r) },
        arc(r, r, PI),
    ]);
    let p = prism(&[profile], 0.0, 10.0);
    let walls: Vec<usize> = (0..p.faces.len())
        .filter(|&f| match p.faces[f].surface {
            Surface::Plane { frame } => frame.z.z.abs() < 1e-9,
            _ => true,
        })
        .collect();
    assert_eq!(walls.len(), 8);
    let d = draft_faces(&p, &walls, &Frame::world(), deg(5.0)).expect("draft");
    let t = deg(5.0).tan();
    let area = w * h - (4.0 - PI) * r * r;
    let perimeter = 2.0 * (w + h) - 8.0 * r + 2.0 * PI * r;
    let want = area * 10.0 - perimeter * t * 50.0 + PI * t * t * 1000.0 / 3.0;
    assert!(close(volume(&d), want, 3e-5), "{} vs {want}", volume(&d));
    // Drafting only some of a smooth chain would tear it.
    assert!(matches!(
        draft_faces(&p, &walls[..1], &Frame::world(), deg(5.0)),
        Err(KernelError::Unsupported(_))
    ));
}

#[test]
fn boss_on_a_base() {
    // Statement 16, with the propagated faces listed by hand: a base [0, 40]^2 x [0, 10]
    // with a boss [15, 25]^2 x [10, 15], drafted 5 degrees about the base's top face.
    let base = prism(&[rect(0.0, 0.0, 40.0, 40.0)], 0.0, 10.0);
    let boss = prism(&[rect(15.0, 15.0, 25.0, 25.0)], 10.0, 15.0);
    let part = boolean(&base, &boss, BoolOp::Union).expect("union").remove(0);
    let walls = sides(&part);
    // The boss's walls are 5 from its axis, the base's 20.
    let (inner, outer): (Vec<usize>, Vec<usize>) = walls
        .iter()
        .partition(|&&f| part.faces[f].surface.distance(DVec3::new(20.0, 20.0, 12.5)) < 5.0 + 1e-9);
    assert_eq!((inner.len(), outer.len()), (4, 4), "{inner:?} {outer:?}");
    let neutral = plane_at(10.0);
    let a = deg(5.0);
    let d = draft_faces(&part, &inner, &neutral, a).expect("inner");
    assert!(close(width_at(&d, 15.0), 9.125113, 1e-7));
    assert!(close(volume(&d), 16457.531379, 1e-9), "{}", volume(&d));
    let d = draft_faces(&part, &outer, &neutral, a).expect("outer");
    assert!(close(width_at(&d, 0.0), 41.749773, 1e-7));
    assert!(close(volume(&d), 17210.114997, 1e-9), "{}", volume(&d));
    let d = draft_faces(&part, &walls, &neutral, a).expect("all");
    assert!(close(volume(&d), 17167.646376, 1e-9), "{}", volume(&d));
    assert_eq!(d.faces.len(), part.faces.len());
}

#[test]
fn bad_drafts_are_refused() {
    let b = prism(&[rect(0.0, 0.0, 20.0, 20.0)], 0.0, 10.0);
    let s = sides(&b);
    // Statement 17: 0 or 90 degrees, or a face parallel to the neutral plane.
    for a in [0.0, deg(90.0), -deg(90.0), f64::NAN] {
        assert!(matches!(draft_faces(&b, &s, &Frame::world(), a), Err(KernelError::InvalidInput(_))));
    }
    let top = face_at(&b, DVec3::new(10.0, 10.0, 10.0), DVec3::Z);
    assert!(matches!(draft_faces(&b, &[top], &Frame::world(), deg(5.0)), Err(KernelError::InvalidInput(_))));
    assert!(draft_faces(&b, &[], &Frame::world(), deg(5.0)).is_err());
    assert!(draft_faces(&b, &[99], &Frame::world(), deg(5.0)).is_err());
    // A face not square to the pull.
    let tilted = Frame::new(DVec3::ZERO, DVec3::new(0.3, 0.0, 1.0), DVec3::X).expect("frame");
    let f = face_at(&b, DVec3::new(20.0, 10.0, 5.0), DVec3::X);
    assert!(matches!(draft_faces(&b, &[f], &tilted, deg(5.0)), Err(KernelError::Unsupported(_))));
    // So steep the top would turn over: the sides cross before the top.
    assert!(matches!(draft_faces(&b, &s, &Frame::world(), deg(60.0)), Err(KernelError::Unsupported(_))));
}
