use super::*;
use crate::{mass, tess};
use glam::DVec3;
use std::f64::consts::PI;

/// A closed polygon of lines.
fn polygon(pts: &[DVec2]) -> Loop2 {
    Loop2::new((0..pts.len()).map(|i| Segment::Line { a: pts[i], b: pts[(i + 1) % pts.len()] }).collect())
}

fn region(loops: &[Loop2]) -> Region {
    let mut r = build_regions(loops).expect("region");
    assert_eq!(r.len(), 1);
    r.remove(0)
}

/// Revolves about the sketch's y axis (sketch x is the distance from it).
fn turn(r: &Region, angle: f64) -> Solid {
    revolve(&Frame::world(), r, DVec2::ZERO, DVec2::Y, angle).expect("revolve")
}

/// Finely tessellated volume, checked watertight.
fn volume(s: &Solid) -> f64 {
    let m = tess::tessellate(s, &tess::TessParams { chord: 1e-4, max_angle: 0.01 }).expect("tess");
    assert!(m.is_watertight(), "not watertight");
    mass::properties(&m).volume
}

fn close(a: f64, b: f64, rel: f64) -> bool {
    (a - b).abs() <= rel * b.abs().max(1.0)
}

fn pts(p: &[(f64, f64)]) -> Vec<DVec2> {
    p.iter().map(|&(x, y)| DVec2::new(x, y)).collect()
}

#[test]
fn tube_full_and_partial() {
    // Radii 2 to 5, height 3: cylinders inside and out, flat rings top and bottom.
    let r = region(&[polygon(&pts(&[(2.0, 0.0), (5.0, 0.0), (5.0, 3.0), (2.0, 3.0)]))]);
    let s = turn(&r, TAU);
    assert!(s.faces.iter().any(|f| matches!(f.surface, Surface::Cylinder { .. })));
    assert!(close(volume(&s), 63.0 * PI, 1e-4), "{}", volume(&s));
    let q = turn(&r, 0.5 * PI);
    assert!(close(volume(&q), 63.0 * PI / 4.0, 1e-4), "{}", volume(&q));
    assert!(
        q.faces.iter().any(|f| f.tag == FaceTag::StartCap)
            && q.faces.iter().any(|f| f.tag == FaceTag::EndCap)
    );
}

#[test]
fn turned_towards_the_sketch_normal() {
    // A positive quarter turn of a profile on +x carries it towards +z; a negative
    // one towards -z, and a profile on -x mirrors that.
    let r = region(&[polygon(&pts(&[(2.0, 0.0), (5.0, 0.0), (5.0, 3.0), (2.0, 3.0)]))]);
    let up = turn(&r, 0.5 * PI);
    assert!(up.vertices.iter().all(|v| v.point.z >= -1e-9 && v.point.x >= -1e-9));
    let down = turn(&r, -0.5 * PI);
    assert!(down.vertices.iter().all(|v| v.point.z <= 1e-9 && v.point.x >= -1e-9));
    let left = region(&[polygon(&pts(&[(-5.0, 0.0), (-2.0, 0.0), (-2.0, 3.0), (-5.0, 3.0)]))]);
    let l = turn(&left, 0.5 * PI);
    assert!(l.vertices.iter().all(|v| v.point.z >= -1e-9 && v.point.x <= 1e-9));
    assert!(close(volume(&l), 63.0 * PI / 4.0, 1e-4));
}

#[test]
fn solid_cylinder_from_a_profile_on_the_axis() {
    let r = region(&[polygon(&pts(&[(0.0, 0.0), (2.0, 0.0), (2.0, 5.0), (0.0, 5.0)]))]);
    let s = turn(&r, TAU);
    assert_eq!(s.faces.len(), 3);
    assert!(close(volume(&s), 20.0 * PI, 1e-4), "{}", volume(&s));
    let h = turn(&r, PI);
    assert!(close(volume(&h), 10.0 * PI, 1e-4), "{}", volume(&h));
}

#[test]
fn pointed_cones() {
    // A right triangle on the axis sweeps a cone with its tip on the axis.
    let r = region(&[polygon(&pts(&[(0.0, 0.0), (3.0, 0.0), (0.0, 4.0)]))]);
    let s = turn(&r, TAU);
    assert!(s.faces.iter().any(|f| matches!(f.surface, Surface::Cone { apex: true, .. })));
    assert!(close(volume(&s), 12.0 * PI, 1e-4), "{}", volume(&s));
    assert!(close(volume(&turn(&r, 0.75 * PI)), 12.0 * PI * 0.375, 1e-4));
    // Upside down, and a triangle touching the axis at one corner (two tips there).
    let r = region(&[polygon(&pts(&[(0.0, 4.0), (3.0, 4.0), (0.0, 0.0)]))]);
    assert!(close(volume(&turn(&r, TAU)), 12.0 * PI, 1e-4));
    let r = region(&[polygon(&pts(&[(0.0, 1.0), (3.0, 0.0), (3.0, 2.0)]))]);
    let s = turn(&r, TAU);
    assert!(close(volume(&s), 12.0 * PI, 1e-4), "{}", volume(&s));
    // A frustum: a cone that does not reach the axis.
    let r = region(&[polygon(&pts(&[(1.0, 0.0), (4.0, 0.0), (2.0, 3.0), (1.0, 3.0)]))]);
    let want = PI * 3.0 / 3.0 * (16.0 + 8.0 + 4.0) - PI * 3.0;
    assert!(close(volume(&turn(&r, TAU)), want, 1e-4), "{} vs {want}", volume(&turn(&r, TAU)));
}

#[test]
fn spheres() {
    // A half disc on the axis: a ball, split into two caps for a full turn.
    let half = Loop2::new(vec![
        Segment::Arc { center: DVec2::ZERO, radius: 3.0, start: -0.5 * PI, sweep: PI },
        Segment::Line { a: DVec2::new(0.0, 3.0), b: DVec2::new(0.0, -3.0) },
    ]);
    let r = region(&[half]);
    let s = turn(&r, TAU);
    assert_eq!(s.faces.len(), 2);
    assert!(close(volume(&s), 36.0 * PI, 1e-4), "{}", volume(&s));
    // A wedge of it (both poles on its faces).
    assert!(close(volume(&turn(&r, 0.5 * PI)), 9.0 * PI, 1e-4));
    assert!(close(volume(&turn(&r, 1.5 * PI)), 27.0 * PI, 1e-4));
    // A dome on a cylinder: the arc reaches the axis at its top only.
    let dome = Loop2::new(vec![
        Segment::Line { a: DVec2::new(0.0, 0.0), b: DVec2::new(3.0, 0.0) },
        Segment::Line { a: DVec2::new(3.0, 0.0), b: DVec2::new(3.0, 2.0) },
        Segment::Arc { center: DVec2::new(0.0, 2.0), radius: 3.0, start: 0.0, sweep: 0.5 * PI },
        Segment::Line { a: DVec2::new(0.0, 5.0), b: DVec2::new(0.0, 0.0) },
    ]);
    let want = 18.0 * PI + 18.0 * PI;
    assert!(close(volume(&turn(&region(&[dome]), TAU)), want, 1e-4));
}

#[test]
fn tori() {
    // A circle off the axis: a ring torus, 2 pi^2 R r^2.
    let c = Loop2::new(vec![Segment::circle(DVec2::new(5.0, 1.0), 2.0)]);
    let r = region(&[c]);
    let s = turn(&r, TAU);
    // Two faces, the tube's outer and inner halves. (A torus this fine meets the
    // tessellator's cap on grid points, so its volume is good to 1e-3 here.)
    assert_eq!(s.faces.len(), 2);
    assert!(close(volume(&s), 40.0 * PI * PI, 1e-3), "{}", volume(&s));
    assert!(close(volume(&turn(&r, PI)), 20.0 * PI * PI, 1e-3), "{}", volume(&turn(&r, PI)));
    // A rounded corner: a rectangle with one corner filleted, partly turned.
    let fillet = Loop2::new(vec![
        Segment::Line { a: DVec2::new(2.0, 0.0), b: DVec2::new(5.0, 0.0) },
        Segment::Line { a: DVec2::new(5.0, 0.0), b: DVec2::new(5.0, 2.0) },
        Segment::Arc { center: DVec2::new(4.0, 2.0), radius: 1.0, start: 0.0, sweep: 0.5 * PI },
        Segment::Line { a: DVec2::new(4.0, 3.0), b: DVec2::new(2.0, 3.0) },
        Segment::Line { a: DVec2::new(2.0, 3.0), b: DVec2::new(2.0, 0.0) },
    ]);
    // Pappus: the volume is the turn times the area's first moment about the axis.
    // The rectangle (area 9, centroid 3.5 out) loses the corner outside the arc: the
    // unit square at 4..5 less a quarter disc, whose moment about the line through the
    // arc's centre is 1/2 - 1/3.
    let moment = 9.0 * 3.5 - ((1.0 - PI / 4.0) * 4.0 + 1.0 / 6.0);
    let s = turn(&region(&[fillet]), TAU);
    assert!(s.faces.iter().any(|f| matches!(f.surface, Surface::Torus { .. })));
    assert!(close(volume(&s), TAU * moment, 1e-4), "{} vs {}", volume(&s), TAU * moment);
}

#[test]
fn partial_turn_of_a_ring_with_a_hole() {
    // A square ring partly turned: its hole becomes a tunnel through the caps.
    let outer = polygon(&pts(&[(2.0, 0.0), (6.0, 0.0), (6.0, 4.0), (2.0, 4.0)]));
    let hole = polygon(&pts(&[(3.0, 1.0), (5.0, 1.0), (5.0, 3.0), (3.0, 3.0)]));
    let r = region(&[outer, hole]);
    let s = turn(&r, PI);
    // Area 12, centroid at 4 from the axis: half a turn sweeps 12 * 4 * pi.
    assert!(close(volume(&s), 48.0 * PI, 1e-4), "{}", volume(&s));
    assert_eq!(check::genus(&s).expect("valid"), 1);
    assert!(matches!(
        revolve(&Frame::world(), &r, DVec2::ZERO, DVec2::Y, TAU),
        Err(KernelError::Unsupported(_))
    ));
}

#[test]
fn refuses_bad_profiles() {
    let across = region(&[polygon(&pts(&[(-1.0, 0.0), (2.0, 0.0), (2.0, 2.0), (-1.0, 2.0)]))]);
    assert!(revolve(&Frame::world(), &across, DVec2::ZERO, DVec2::Y, TAU).is_err());
    // A circle reaching past the axis's side? No: one whose circle reaches the axis.
    let c = region(&[Loop2::new(vec![
        Segment::Arc { center: DVec2::new(1.5, 0.0), radius: 1.0, start: -0.5 * PI, sweep: PI },
        Segment::Line { a: DVec2::new(1.5, 1.0), b: DVec2::new(1.5, -1.0) },
    ])]);
    assert!(revolve(&Frame::world(), &c, DVec2::new(0.0, 0.0), DVec2::Y, TAU).is_ok());
    assert!(revolve(&Frame::world(), &across, DVec2::ZERO, DVec2::Y, 0.0).is_err());
}

#[test]
fn tilted_sketch_and_axis() {
    // The same tube in a tilted sketch, about a slanted axis away from the origin.
    let f = Frame::new(DVec3::new(5.0, -2.0, 7.0), DVec3::new(1.0, 1.0, 1.0), DVec3::X).expect("frame");
    let axis_dir = DVec2::new(1.0, 2.0).normalize();
    let across = axis_dir.perp();
    let o = DVec2::new(3.0, -1.0);
    let p = |r: f64, h: f64| o + across * r + axis_dir * h;
    let l = polygon(&[p(2.0, 0.0), p(5.0, 0.0), p(5.0, 3.0), p(2.0, 3.0)]);
    let s = revolve(&f, &region(&[l]), o, axis_dir, TAU).expect("revolve");
    assert!(close(volume(&s), 63.0 * PI, 1e-4), "{}", volume(&s));
}

#[test]
fn revolved_bodies_in_booleans() {
    use crate::boolean::{BoolOp, boolean};
    use crate::extrude::extrude;
    let bar = |c: DVec3, axis: DVec3, r: f64, z0: f64, z1: f64| {
        let hint = if axis.x.abs() < 0.9 { DVec3::X } else { DVec3::Y };
        let f = Frame::new(c, axis, hint).expect("frame");
        let reg = build_regions(&[Loop2::new(vec![Segment::circle(DVec2::ZERO, r)])]).expect("region");
        extrude(&f, &reg[0], z0, z1).expect("bar")
    };
    let fine = |s: &[Solid]| -> f64 {
        s.iter()
            .map(|x| {
                check::validate(x).expect("valid");
                volume(x)
            })
            .sum()
    };
    // A flange (radii 2 to 5, 3 thick, about the y axis) with a bolt hole along y.
    let flange = turn(&region(&[polygon(&pts(&[(2.0, 0.0), (5.0, 0.0), (5.0, 3.0), (2.0, 3.0)]))]), TAU);
    let bolt = bar(DVec3::new(3.5, 0.0, 0.0), DVec3::Y, 0.5, -1.0, 4.0);
    let d = boolean(&flange, &bolt, BoolOp::Subtract).expect("subtract");
    assert!(close(fine(&d), 63.0 * PI - 0.75 * PI, 1e-4), "{}", fine(&d));
    // A ball drilled across its axis: the drill meets both caps.
    let half = Loop2::new(vec![
        Segment::Arc { center: DVec2::ZERO, radius: 3.0, start: -0.5 * PI, sweep: PI },
        Segment::Line { a: DVec2::new(0.0, 3.0), b: DVec2::new(0.0, -3.0) },
    ]);
    let ball = turn(&region(&[half]), TAU);
    let drill = bar(DVec3::new(-5.0, 0.0, 0.0), DVec3::X, 1.0, 0.0, 10.0);
    let d = boolean(&ball, &drill, BoolOp::Subtract).expect("subtract");
    let i = boolean(&ball, &drill, BoolOp::Intersect).expect("intersect");
    assert!(close(fine(&d) + fine(&i), 36.0 * PI, 1e-4), "{} + {}", fine(&d), fine(&i));
    // The hole through a ball: a cylinder of radius 1 and length 2 sqrt(8), plus two caps.
    let h = 8f64.sqrt();
    let cap = PI * (3.0 - h).powi(2) * (9.0 - (3.0 - h)) / 3.0;
    assert!(close(fine(&i), PI * 2.0 * h + 2.0 * cap, 1e-4), "{}", fine(&i));
    // A torus (about the y axis, its tube's middle at y = 0) cut in half by a slab
    // above that plane: the cut runs across both of its faces.
    let ring = turn(&region(&[Loop2::new(vec![Segment::circle(DVec2::new(5.0, 0.0), 2.0)])]), TAU);
    let slab = bar(DVec3::ZERO, DVec3::Y, 20.0, 0.0, 5.0);
    let i = boolean(&ring, &slab, BoolOp::Intersect).expect("intersect");
    let d = boolean(&ring, &slab, BoolOp::Subtract).expect("subtract");
    let whole = 40.0 * PI * PI;
    assert!(close(fine(&i) + fine(&d), whole, 1e-3), "{} + {}", fine(&i), fine(&d));
    assert!(close(fine(&i), 0.5 * whole, 1e-3), "{}", fine(&i));
}

#[test]
fn sphere_band_measures_true() {
    // A ball of 5 bored through by 2: the band of the sphere between the bore's rims holds
    // a point near wherever its layout is projected from, so its parameters stretch to
    // many times the radius. Tessellated on a grid over them, it came out 3e-4 small at
    // any fineness (the grid hit its point budget and widened). Ball less the bore and
    // its two caps: 4 pi 125 / 3 - pi 4 2h - 2 pi (5 - h)^2 (15 - (5 - h)) / 3, h = sqrt 21.
    let h = 21f64.sqrt();
    let b = (h / 5.0).asin();
    let band = Loop2::new(vec![
        Segment::Arc { center: DVec2::ZERO, radius: 5.0, start: -b, sweep: 2.0 * b },
        Segment::Line { a: DVec2::new(2.0, h), b: DVec2::new(2.0, -h) },
    ]);
    let s = turn(&region(&[band]), 2.0 * PI);
    let want = 4.0 * PI * 125.0 / 3.0 - PI * 8.0 * h - 2.0 * PI * (5.0 - h).powi(2) * (10.0 + h) / 3.0;
    let v = volume(&s);
    assert!(close(v, want, 1e-4), "{v} vs {want}");
    // A whole ball, too, and within the display tolerance at the default settings.
    let ball = Loop2::new(vec![
        Segment::Arc { center: DVec2::ZERO, radius: 5.0, start: -0.5 * PI, sweep: PI },
        Segment::Line { a: DVec2::new(0.0, 5.0), b: DVec2::new(0.0, -5.0) },
    ]);
    let ball = turn(&region(&[ball]), 2.0 * PI);
    assert!(close(volume(&ball), 4.0 * PI * 125.0 / 3.0, 1e-4));
    let m = tess::tessellate(&ball, &tess::TessParams::default()).expect("tess");
    let sag = m
        .triangles
        .iter()
        .map(|t| {
            5.0 - ((m.positions[t[0] as usize] + m.positions[t[1] as usize] + m.positions[t[2] as usize])
                / 3.0)
                .length()
        })
        .fold(0.0, f64::max);
    assert!(sag <= crate::tol::CHORD, "{sag}");
}

/// Exact volume and area, against closed forms to 1e-8 (as `mass`'s own tests), and the
/// tessellation's volume (watertight) to 1e-3: a tube this fine meets the tessellator's
/// cap on grid points, as in `tori`.
fn measures(s: &Solid, v: f64, a: f64) {
    let m = mass::exact_properties(s).expect("exact");
    assert!(close(m.volume, v, 1e-8), "volume {} vs {v}", m.volume);
    assert!(close(m.area, a, 1e-8), "area {} vs {a}", m.area);
    let t = volume(s);
    assert!(close(t, v, 1e-3), "tessellated volume {t} vs {v}");
}

/// The arc of the circle about (`cx`, 0) of radius `r` from the axis at (0, -h) to
/// (0, h) on the side x >= 0, closed by the axis: the apple of a spindle torus when the
/// centre is on that side, the lemon when it is across the axis.
fn axis_to_axis(cx: f64, r: f64) -> Region {
    let h = (r * r - cx * cx).sqrt();
    let a = h.atan2(-cx);
    region(&[Loop2::new(vec![
        Segment::Arc { center: DVec2::new(cx, 0.0), radius: r, start: -a, sweep: 2.0 * a },
        Segment::Line { a: DVec2::new(0.0, h), b: DVec2::new(0.0, -h) },
    ])])
}

#[test]
fn horn_torus() {
    // brep-bench's geometry/horn-torus: a circle of radius 5 touching the axis.
    // Volume 2 pi^2 R r^2 = 250 pi^2, area 4 pi^2 R r = 100 pi^2.
    let r = region(&[Loop2::new(vec![Segment::circle(DVec2::new(5.0, 0.0), 5.0)])]);
    let s = turn(&r, TAU);
    // The two halves of the tube, each pinched to the pole at the origin.
    assert_eq!(s.faces.len(), 2);
    assert!(s.faces.iter().all(|f| f.surface.is_pinched_torus()));
    assert_eq!(check::genus(&s).expect("valid"), 0);
    measures(&s, 250.0 * PI * PI, 100.0 * PI * PI);
    // Any circle start, and a partial turn (with caps touching at the pole).
    let r = region(&[Loop2::new(vec![Segment::Arc {
        center: DVec2::new(5.0, 0.0),
        radius: 5.0,
        start: 1.0,
        sweep: -TAU,
    }])]);
    let s = turn(&r, 0.5 * PI);
    measures(&s, 62.5 * PI * PI, 25.0 * PI * PI + 2.0 * 25.0 * PI);
}

#[test]
fn torus_nearly_horn() {
    // brep-bench's geometry/torus-nearly-horn: a ring torus whose hole is 0.02 across.
    let (big, small) = (5.0, 4.99);
    let r = region(&[Loop2::new(vec![Segment::circle(DVec2::new(big, 0.0), small)])]);
    let s = turn(&r, TAU);
    assert!(s.faces.iter().all(|f| !f.surface.is_pinched_torus()));
    measures(&s, 2.0 * PI * PI * big * small * small, 4.0 * PI * PI * big * small);
}

#[test]
fn spindle_torus_apple() {
    // brep-bench's geometry/spindle-torus: middle circle 3, tube 5. The outer arc,
    // axis to axis, turned: 2 pi (75 (pi - t) + 236 / 3), area 20 pi (3 (pi - t) + 4),
    // t = acos(3 / 5).
    let t = (0.6f64).acos();
    let r = axis_to_axis(3.0, 5.0);
    let s = turn(&r, TAU);
    assert!(s.faces.iter().all(|f| f.surface.is_pinched_torus()));
    measures(&s, TAU * (75.0 * (PI - t) + 236.0 / 3.0), 20.0 * PI * (3.0 * (PI - t) + 4.0));
    // A third of a turn: a third of the volume, the caps the disc's part x >= 0, of area
    // pi r^2 less the segment cut off, r^2 t - R sqrt(r^2 - R^2).
    let s = turn(&r, TAU / 3.0);
    let cap = 25.0 * PI - (25.0 * t - 12.0);
    measures(
        &s,
        TAU * (75.0 * (PI - t) + 236.0 / 3.0) / 3.0,
        20.0 * PI * (3.0 * (PI - t) + 4.0) / 3.0 + 2.0 * cap,
    );
}

#[test]
fn lemon() {
    // brep-bench's geometry/lemon: the segment of the disc of radius 5 about (-3, 0)
    // cut off by the axis, turned: 2 pi (236 / 3 - 75 t), area 20 pi (4 - 3 t).
    let t = (0.6f64).acos();
    let r = axis_to_axis(-3.0, 5.0);
    let s = turn(&r, TAU);
    assert!(s.faces.iter().all(|f| matches!(f.surface, Surface::Torus { major, .. } if major == -3.0)));
    measures(&s, TAU * (236.0 / 3.0 - 75.0 * t), 20.0 * PI * (4.0 - 3.0 * t));
    let s = turn(&r, -0.5 * PI);
    let cap = 25.0 * t - 12.0;
    measures(&s, TAU * (236.0 / 3.0 - 75.0 * t) / 4.0, 5.0 * PI * (4.0 - 3.0 * t) + 2.0 * cap);
}

#[test]
fn spindle_arc_off_the_axis() {
    // A half disc of radius 1 whose centre is 0.8 from the axis, flat side in: its
    // circle crosses the axis but the arc does not reach it. Pappus: 2 pi A (0.8 + 4 /
    // (3 pi)) with A = pi / 2.
    let r = region(&[Loop2::new(vec![
        Segment::Arc { center: DVec2::new(0.8, 0.0), radius: 1.0, start: -0.5 * PI, sweep: PI },
        Segment::Line { a: DVec2::new(0.8, 1.0), b: DVec2::new(0.8, -1.0) },
    ])]);
    let s = turn(&r, TAU);
    assert!(s.faces.iter().any(|f| f.surface.is_pinched_torus()));
    // The arc turned (2 pi (0.8 pi + 2)) and the cylinder inside (2 pi 0.8 * 2).
    let v = TAU * 0.5 * PI * (0.8 + 4.0 / (3.0 * PI));
    measures(&s, v, TAU * (0.8 * PI + 2.0) + TAU * 1.6);
}

#[test]
fn horn_arc_through_its_pole() {
    // A half disc of a horn torus's circle, its arc passing the pole, closed by the
    // chord x = 5 from (5, -5) to (5, 5): split at the pole, the arc sweeps two pointed
    // faces. Its volume is a half of the horn torus's less... by Pappus: area 25 pi / 2,
    // centroid 5 - 20 / (3 pi) from the axis.
    let r = region(&[Loop2::new(vec![
        Segment::Arc { center: DVec2::new(5.0, 0.0), radius: 5.0, start: 0.5 * PI, sweep: PI },
        Segment::Line { a: DVec2::new(5.0, -5.0), b: DVec2::new(5.0, 5.0) },
    ])]);
    let s = turn(&r, TAU);
    assert_eq!(s.faces.iter().filter(|f| f.surface.is_pinched_torus()).count(), 2);
    let v = TAU * 12.5 * PI * (5.0 - 20.0 / (3.0 * PI));
    // The inner half of the tube turned, 2 pi r (pi R - 2 r), and the cylinder.
    measures(&s, v, TAU * 5.0 * (5.0 * PI - 10.0) + TAU * 5.0 * 10.0);
}

#[test]
fn pinched_tori_mirror_scale_and_refuse_the_rest() {
    let t = (0.6f64).acos();
    let lemon = turn(&axis_to_axis(-3.0, 5.0), TAU);
    let v = TAU * (236.0 / 3.0 - 75.0 * t);
    let a = 20.0 * PI * (4.0 - 3.0 * t);
    // Mirrored and scaled, measured again.
    let m = crate::mirror::mirrored(&lemon, DVec3::new(1.0, 0.0, 0.0), DVec3::new(1.0, 1.0, 0.0))
        .expect("mirror");
    measures(&m, v, a);
    let s = crate::scale::scaled(&lemon, DVec3::ZERO, &Frame::world(), [2.0; 3]).expect("scale");
    measures(&s, 8.0 * v, 4.0 * a);
    // What does not handle a horn or spindle torus yet says so.
    let block = crate::extrude::extrude(
        &Frame::world(),
        &region(&[polygon(&pts(&[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)]))]),
        0.0,
        1.0,
    )
    .expect("block");
    let unsupported = |r: Result<Vec<Solid>>| matches!(r, Err(KernelError::Unsupported(_)));
    assert!(unsupported(crate::boolean::boolean(&lemon, &block, crate::boolean::BoolOp::Union)));
    assert!(unsupported(crate::boolean::boolean(&block, &lemon, crate::boolean::BoolOp::Subtract)));
    let apple = turn(&axis_to_axis(3.0, 5.0), TAU);
    let equator = crate::blend::edge_near(&apple, DVec3::new(8.0, 0.0, 0.0), 1e-6).expect("equator");
    assert!(unsupported(crate::blend::blend_edge(&apple, equator, crate::blend::Blend::Fillet(0.5), 1, 0)));
    let grown = crate::offset::offset_faces(&apple, &vec![0.5; apple.faces.len()]);
    assert!(matches!(grown, Err(KernelError::Unsupported(_))), "{grown:?}");
    assert!(crate::shell::shell(&apple, &[0], 0.5).is_err());
}
