//! Loft tests against the volumes in `docs/re/sweep-and-loft.md`, statements 20 to 24
//! and 28.

use super::*;
use crate::boolean::{BoolOp, boolean};
use crate::{mass, tess};
use std::f64::consts::SQRT_2;

fn volume(s: &Solid) -> f64 {
    check::validate(s).expect("valid");
    let m = tess::tessellate(s, &tess::TessParams { chord: 1e-4, max_angle: 0.01 }).expect("tess");
    assert!(m.is_watertight(), "not watertight");
    mass::properties(&m).volume
}

fn close(v: f64, want: f64, rel: f64) -> bool {
    (v - want).abs() <= rel * want.abs()
}

fn polygon(pts: &[DVec2]) -> Loop2 {
    Loop2::new((0..pts.len()).map(|i| Segment::Line { a: pts[i], b: pts[(i + 1) % pts.len()] }).collect())
}

/// A square of side `w` about the origin, turned `turn` radians, counter-clockwise.
fn square(w: f64, turn: f64) -> Loop2 {
    let h = 0.5 * w;
    let pts: Vec<DVec2> = [(-h, -h), (h, -h), (h, h), (-h, h)]
        .iter()
        .map(|&(x, y)| DVec2::from_angle(turn).rotate(DVec2::new(x, y)))
        .collect();
    polygon(&pts)
}

fn at(z: f64) -> Frame {
    Frame { origin: DVec3::new(0.0, 0.0, z), ..Frame::world() }
}

fn ring(z: f64, outline: Loop2) -> Section {
    Section::Loop { frame: at(z), outline }
}

fn circle(r: f64) -> Loop2 {
    Loop2::new(vec![Segment::circle(DVec2::ZERO, r)])
}

fn cuboid(lo: DVec3, hi: DVec3) -> Solid {
    let r = build_regions(&[polygon(&[
        DVec2::new(lo.x, lo.y),
        DVec2::new(hi.x, lo.y),
        DVec2::new(hi.x, hi.y),
        DVec2::new(lo.x, hi.y),
    ])])
    .expect("region");
    crate::extrude::extrude(&Frame::world(), &r[0], lo.z, hi.z).expect("box")
}

fn frustum() -> Solid {
    loft_ruled(&ring(0.0, square(20.0, 0.0)), &ring(30.0, square(10.0, 0.0)), [None, None]).expect("loft")
}

#[test]
fn square_frustum_has_planar_faces() {
    // Statement 20: 30/3 (400 + 100 + 200).
    let s = frustum();
    assert_eq!(s.faces.len(), 6);
    assert!(s.faces.iter().all(|f| matches!(f.surface, Surface::Plane { .. })));
    assert!(close(volume(&s), 7000.0, 1e-9), "{}", volume(&s));
    assert!(s.face_by_tag(FaceTag::StartCap).is_some() && s.face_by_tag(FaceTag::EndCap).is_some());
    for k in 0..4 {
        assert!(s.face_by_tag(FaceTag::Side { contour: 0, segment: k }).is_some());
    }
}

#[test]
fn coaxial_circles_give_a_cone_frustum() {
    // Statement 21: pi 30/3 (100 + 50 + 25), side a cone of half-angle atan(5/30).
    let s = loft_ruled(&ring(0.0, circle(10.0)), &ring(30.0, circle(5.0)), [None, None]).expect("loft");
    assert_eq!(s.faces.len(), 3);
    let cone = s.faces.iter().find_map(|f| match f.surface {
        Surface::Cone { half_angle, apex: false, .. } => Some(half_angle),
        _ => None,
    });
    let half = cone.expect("a cone side");
    assert!((half - (5.0f64 / 30.0).atan()).abs() < 1e-12);
    let want = 1750.0 * PI;
    assert!(close(volume(&s), want, 1e-4), "{} vs {want}", volume(&s));
    // Equal circles give a cylinder.
    let c = loft_ruled(&ring(0.0, circle(4.0)), &ring(10.0, circle(4.0)), [None, None]).expect("loft");
    assert!(c.faces.iter().any(|f| matches!(f.surface, Surface::Cylinder { .. })));
    assert!(close(volume(&c), 160.0 * PI, 1e-4), "{}", volume(&c));
}

#[test]
fn square_to_a_point_is_a_pyramid() {
    // Statement 22: 400 30 / 3, four planar triangles.
    let s =
        loft_ruled(&ring(0.0, square(20.0, 0.0)), &Section::Point(DVec3::new(0.0, 0.0, 30.0)), [None, None])
            .expect("loft");
    assert_eq!(s.faces.len(), 5);
    assert_eq!(s.vertices.len(), 5);
    assert!(s.faces.iter().all(|f| matches!(f.surface, Surface::Plane { .. })));
    assert!(close(volume(&s), 4000.0, 1e-9), "{}", volume(&s));
    // The same with the point first: an upside-down pyramid.
    let s =
        loft_ruled(&Section::Point(DVec3::new(0.0, 0.0, -30.0)), &ring(0.0, square(20.0, 0.0)), [None, None])
            .expect("loft");
    assert!(close(volume(&s), 4000.0, 1e-9), "{}", volume(&s));
    assert!(s.face_by_tag(FaceTag::EndCap).is_some() && s.face_by_tag(FaceTag::StartCap).is_none());
}

#[test]
fn circle_to_a_point_on_its_axis_is_a_cone() {
    let s = loft_ruled(&ring(0.0, circle(10.0)), &Section::Point(DVec3::new(0.0, 0.0, 30.0)), [None, None])
        .expect("loft");
    assert_eq!(s.faces.len(), 2);
    assert!(s.faces.iter().any(|f| matches!(f.surface, Surface::Cone { apex: true, .. })));
    assert!(close(volume(&s), 1000.0 * PI, 1e-4), "{}", volume(&s));
    // Off the axis there is no exact pointed surface to build.
    let off = loft_ruled(&ring(0.0, circle(10.0)), &Section::Point(DVec3::new(3.0, 0.0, 30.0)), [None, None]);
    assert!(matches!(off, Err(KernelError::Unsupported(_))), "{off:?}");
}

#[test]
fn half_disc_to_a_point_on_its_axis() {
    // A D shape: a half circle closed by its diameter, lofted to a point above its centre.
    // Half a cone: pi 100 30 / 6.
    let d = Loop2::new(vec![
        Segment::Arc { center: DVec2::ZERO, radius: 10.0, start: -0.5 * PI, sweep: PI },
        Segment::Line { a: DVec2::new(0.0, 10.0), b: DVec2::new(0.0, -10.0) },
    ]);
    let s =
        loft_ruled(&ring(0.0, d), &Section::Point(DVec3::new(0.0, 0.0, 30.0)), [None, None]).expect("loft");
    assert!(s.faces.iter().any(|f| matches!(f.surface, Surface::Cone { apex: true, .. })));
    let want = PI * 100.0 * 30.0 / 6.0;
    assert!(close(volume(&s), want, 1e-4), "{} vs {want}", volume(&s));
}

#[test]
fn square_to_circle_splits_the_circle_in_quarters() {
    // Statement 23: h/3 (a^2 + pi r^2 + 4 sqrt2 a r / pi).
    let s = loft_ruled(&ring(0.0, square(20.0, 0.0)), &ring(30.0, circle(10.0)), [None, None]).expect("loft");
    assert_eq!(s.faces.len(), 6);
    let end = s.face_by_tag(FaceTag::EndCap).and_then(|f| s.face(f)).expect("end cap");
    assert_eq!(end.outer.coedges.len(), 4);
    assert_eq!(s.faces.iter().filter(|f| matches!(f.surface, Surface::BSpline(_))).count(), 4);
    let want = 4000.0 + 1000.0 * PI + 8000.0 * SQRT_2 / PI;
    assert!((want - 10742.858).abs() < 1e-3);
    assert!(close(volume(&s), want, 1e-5), "{} vs {want}", volume(&s));
    // The matched circle is split at +-45 and +-135 degrees.
    let [_, b] = match_sections(&ring(0.0, square(20.0, 0.0)), &ring(30.0, circle(10.0)), [None, None])
        .expect("match");
    assert_eq!(b.segment_count(), 4);
    for k in 0..4 {
        let p = b.point_at(k, 0.0);
        let a = p.y.atan2(p.x).to_degrees();
        assert!(((a - 45.0).rem_euclid(90.0)).min(90.0 - (a - 45.0).rem_euclid(90.0)) < 1e-9, "{a}");
    }
    // The mid-height section of the matched rulings has the stated area.
    let [a, b] = match_sections(&ring(0.0, square(20.0, 0.0)), &ring(30.0, circle(10.0)), [None, None])
        .expect("match");
    let mut pts = Vec::new();
    for k in 0..4 {
        for j in 0..2000 {
            let f = j as f64 / 2000.0;
            let p = (a.point_at(k, f) + b.point_at(k, f)) * 0.5;
            pts.push(DVec2::new(p.x, p.y));
        }
    }
    let area: f64 = (0..pts.len()).map(|i| pts[i].perp_dot(pts[(i + 1) % pts.len()])).sum::<f64>() * 0.5;
    let mid = (400.0 + 100.0 * PI + 2.0 * 4.0 * SQRT_2 * 200.0 / PI) / 4.0;
    assert!(close(area, mid, 1e-5), "{area} vs {mid}");
}

#[test]
fn twisted_squares_make_ruled_sides() {
    // Statement 24: 2000 + 500 sqrt 2, four doubly ruled sides.
    let s = loft_ruled(&ring(0.0, square(10.0, 0.0)), &ring(30.0, square(10.0, 0.25 * PI)), [None, None])
        .expect("loft");
    assert_eq!(s.faces.iter().filter(|f| matches!(f.surface, Surface::BSpline(_))).count(), 4);
    let want = 2000.0 + 500.0 * SQRT_2;
    assert!(close(volume(&s), want, 1e-4), "{} vs {want}", volume(&s));
}

#[test]
fn winding_frames_and_start_points_are_aligned() {
    // The top square drawn clockwise from another corner, in a frame facing down: the
    // loops are turned the same way and started at matching corners, so the result is
    // still statement 20's frustum.
    let down = Frame { origin: DVec3::new(0.0, 0.0, 30.0), x: DVec3::X, y: DVec3::NEG_Y, z: DVec3::NEG_Z };
    let cw = polygon(&[
        DVec2::new(5.0, 5.0),
        DVec2::new(5.0, -5.0),
        DVec2::new(-5.0, -5.0),
        DVec2::new(-5.0, 5.0),
    ]);
    let top = Section::Loop { frame: down, outline: cw };
    let bottom = ring(0.0, square(20.0, 0.0));
    let s = loft_ruled(&bottom, &top, [None, None]).expect("loft");
    assert!(s.faces.iter().all(|f| matches!(f.surface, Surface::Plane { .. })));
    assert!(close(volume(&s), 7000.0, 1e-9), "{}", volume(&s));
    // Matched starts: corner (10, 10) below and (5, 5) above, both turning the same way.
    let [a, b] = match_sections(&bottom, &top, [None, None]).expect("match");
    assert!((a.point_at(0, 0.0) - DVec3::new(10.0, 10.0, 0.0)).length() < 1e-12);
    assert!((b.point_at(0, 0.0) - DVec3::new(5.0, 5.0, 30.0)).length() < 1e-12);
    assert!((b.point_at(1, 0.0) - DVec3::new(-5.0, 5.0, 30.0)).length() < 1e-12);
    // Lofting downwards (sections in the other order) gives the same solid.
    let s = loft_ruled(&top, &bottom, [None, None]).expect("loft");
    assert!(close(volume(&s), 7000.0, 1e-9), "{}", volume(&s));

    // Picking the top square's next corner shifts the matching by a quarter turn:
    // the midsection is (I + R/2) P / 2, of area 400 * 5/16, so 30/6 (400 + 500 + 100).
    let pick = Some(DVec3::new(-5.0, 5.0, 30.0));
    let s = loft_ruled(&bottom, &ring(30.0, square(10.0, 0.0)), [None, pick]).expect("loft");
    assert_eq!(s.faces.iter().filter(|f| matches!(f.surface, Surface::BSpline(_))).count(), 4);
    assert!(close(volume(&s), 5000.0, 1e-4), "{}", volume(&s));
    // A pick on the circle sets where it starts.
    let [_, c] = match_sections(&bottom, &ring(30.0, circle(5.0)), [None, Some(DVec3::new(0.0, 9.0, 30.0))])
        .expect("match");
    assert!((c.point_at(0, 0.0) - DVec3::new(0.0, 5.0, 30.0)).length() < 1e-12);
}

#[test]
fn tags_follow_the_first_section() {
    // Split pieces keep their parent's tag; the second section's tags are not used.
    let tagged = Loop2::with_tags(square(20.0, 0.0).segments, vec![11, 12, 13, 14]);
    let s = loft_ruled(&ring(0.0, tagged), &ring(30.0, circle(10.0)), [None, None]).expect("loft");
    let mut tags: Vec<u32> = s
        .faces
        .iter()
        .filter_map(|f| match f.tag {
            FaceTag::Side { contour: 0, segment } => Some(segment),
            _ => None,
        })
        .collect();
    tags.sort_unstable();
    assert_eq!(tags, vec![11, 12, 13, 14]);
    // A circle first: its one tag goes to every piece.
    let c = Loop2::with_tags(circle(10.0).segments, vec![7]);
    let s = loft_ruled(&ring(0.0, c), &ring(30.0, square(20.0, 0.0)), [None, None]).expect("loft");
    let sides = s.faces.iter().filter(|f| f.tag == FaceTag::Side { contour: 0, segment: 7 }).count();
    assert_eq!(sides, 4);
}

#[test]
fn offset_circles_make_an_oblique_cylinder() {
    // Every section is a circle of radius 5: 25 pi 10 (Cavalieri).
    let top = Section::Loop {
        frame: Frame { origin: DVec3::new(3.0, 0.0, 10.0), ..Frame::world() },
        outline: circle(5.0),
    };
    let s = loft_ruled(&ring(0.0, circle(5.0)), &top, [None, None]).expect("loft");
    assert!(s.faces.iter().any(|f| matches!(f.surface, Surface::BSpline(_))));
    assert!(close(volume(&s), 250.0 * PI, 1e-4), "{}", volume(&s));
}

#[test]
fn rotated_circles_rule_a_band() {
    // A circle of radius 10 lofted to one of radius 5 started a quarter turn later: each
    // section (1-s) 10 u(t) + s 5 u(t + pi/2) is a circle of radius squared
    // 100 (1-s)^2 + 25 s^2, so the volume is 30 pi (100 + 25) / 3. Equal sweeps: the
    // side is an exact rational patch.
    let pick = Some(DVec3::new(0.0, 9.0, 30.0));
    let s = loft_ruled(&ring(0.0, circle(10.0)), &ring(30.0, circle(5.0)), [None, pick]).expect("loft");
    assert!(s.faces.iter().any(|f| matches!(&f.surface, Surface::BSpline(p) if p.degree == (2, 1))));
    let want = 30.0 * PI * 125.0 / 3.0;
    assert!(close(volume(&s), want, 1e-4), "{} vs {want}", volume(&s));
}

/// Area of the section a fraction `s` of the way between two matched parallel loops,
/// from dense samples of the matched points.
fn blend_area(a: &Section, b: &Section, s: f64) -> f64 {
    let mut pts = Vec::new();
    for k in 0..a.segment_count().max(b.segment_count()) {
        for j in 0..4000 {
            let f = j as f64 / 4000.0;
            let p = a.point_at(k, f) * (1.0 - s) + b.point_at(k, f) * s;
            pts.push(DVec2::new(p.x, p.y));
        }
    }
    (0..pts.len()).map(|i| pts[i].perp_dot(pts[(i + 1) % pts.len()])).sum::<f64>() * 0.5
}

#[test]
fn arcs_of_different_sweep_are_ruled_by_arc_length() {
    // A D (half circle and diameter) lofted to a circle: the circle is split where the
    // D's arc ends by arc length, so a 180 degree arc meets a longer one and the line a
    // shorter one. The volume between parallel sections is h/6 (A0 + 4 Am + A1).
    let d = Loop2::new(vec![
        Segment::Arc { center: DVec2::ZERO, radius: 10.0, start: -0.5 * PI, sweep: PI },
        Segment::Line { a: DVec2::new(0.0, 10.0), b: DVec2::new(0.0, -10.0) },
    ]);
    let (a, b) = (ring(0.0, d), ring(20.0, circle(8.0)));
    let s = loft_ruled(&a, &b, [None, None]).expect("loft");
    assert_eq!(
        s.faces.iter().filter(|f| matches!(&f.surface, Surface::BSpline(p) if p.degree == (5, 1))).count(),
        2
    );
    let [ma, mb] = match_sections(&a, &b, [None, None]).expect("match");
    let want = 20.0 / 6.0 * (50.0 * PI + 4.0 * blend_area(&ma, &mb, 0.5) + 64.0 * PI);
    assert!(close(volume(&s), want, 1e-4), "{} vs {want}", volume(&s));
}

#[test]
fn lofts_unite_and_cut() {
    // A plinth under statement 20's frustum, meeting it face to face.
    let plinth = cuboid(DVec3::new(-15.0, -15.0, -10.0), DVec3::new(15.0, 15.0, 0.0));
    let u = boolean(&plinth, &frustum(), BoolOp::Union).expect("union");
    assert_eq!(u.len(), 1);
    assert!(close(volume(&u[0]), 9000.0 + 7000.0, 1e-9), "{}", volume(&u[0]));
    // Statement 28: the frustum cut from a 40 mm block holding it, flush with its base
    // (a cut wholly inside would leave a closed void, which the kernel refuses).
    let block = cuboid(DVec3::new(-20.0, -20.0, 0.0), DVec3::new(20.0, 20.0, 40.0));
    let d = boolean(&block, &frustum(), BoolOp::Subtract).expect("subtract");
    assert_eq!(d.len(), 1);
    assert!(close(volume(&d[0]), 57000.0, 1e-9), "{}", volume(&d[0]));
}

#[test]
fn freeform_loft_unites_with_a_plate() {
    // Statement 23's loft standing on a plate.
    let loft =
        loft_ruled(&ring(0.0, square(20.0, 0.0)), &ring(30.0, circle(10.0)), [None, None]).expect("loft");
    let plate = cuboid(DVec3::new(-20.0, -20.0, -5.0), DVec3::new(20.0, 20.0, 0.0));
    let u = boolean(&plate, &loft, BoolOp::Union).expect("union");
    assert_eq!(u.len(), 1);
    let want = 8000.0 + 4000.0 + 1000.0 * PI + 8000.0 * SQRT_2 / PI;
    assert!(close(volume(&u[0]), want, 1e-5), "{} vs {want}", volume(&u[0]));
}

#[test]
fn twisted_loft_cut_through_its_sides() {
    // A plane at mid-height cuts statement 24's ruled sides; by symmetry the half below
    // holds half the volume, (2000 + 500 sqrt 2) / 2.
    let loft = loft_ruled(&ring(0.0, square(10.0, 0.0)), &ring(30.0, square(10.0, 0.25 * PI)), [None, None])
        .expect("loft");
    let cap = cuboid(DVec3::new(-10.0, -10.0, 15.0), DVec3::new(10.0, 10.0, 40.0));
    let d = boolean(&loft, &cap, BoolOp::Subtract).expect("subtract");
    assert_eq!(d.len(), 1);
    let want = 1000.0 + 250.0 * SQRT_2;
    assert!(close(volume(&d[0]), want, 1e-4), "{} vs {want}", volume(&d[0]));
}

#[test]
fn bad_lofts_are_refused() {
    let p = Section::Point(DVec3::new(0.0, 0.0, 10.0));
    assert!(loft_ruled(&p, &Section::Point(DVec3::ZERO), [None, None]).is_err());
    // Both in one plane.
    assert!(loft_ruled(&ring(0.0, square(20.0, 0.0)), &ring(0.0, square(10.0, 0.0)), [None, None]).is_err());
    // A section reaching through the other's plane.
    let tilted = Section::Loop {
        frame: Frame::new(DVec3::new(0.0, 0.0, 2.0), DVec3::new(1.0, 0.0, 1.0), DVec3::Y).expect("frame"),
        outline: square(20.0, 0.0),
    };
    assert!(loft_ruled(&ring(0.0, square(20.0, 0.0)), &tilted, [None, None]).is_err());
    // A half turn of twist pinches every ruling through the axis.
    let twisted = loft_ruled(
        &ring(0.0, square(10.0, 0.0)),
        &ring(10.0, square(10.0, 0.0)),
        [Some(DVec3::new(5.0, 5.0, 0.0)), Some(DVec3::new(-5.0, -5.0, 10.0))],
    );
    assert!(twisted.is_err(), "{twisted:?}");
    // An open outline.
    let open = Loop2::new(vec![Segment::Line { a: DVec2::ZERO, b: DVec2::new(1.0, 0.0) }]);
    assert!(loft_ruled(&ring(0.0, open), &p, [None, None]).is_err());
    // A point in the frame.
    assert!(
        loft_ruled(
            &ring(0.0, square(20.0, 0.0)),
            &Section::Point(DVec3::new(f64::NAN, 0.0, 1.0)),
            [None, None]
        )
        .is_err()
    );
}
