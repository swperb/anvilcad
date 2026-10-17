use super::*;
use crate::blend::{Blend, blend_edge, edge_near};
use crate::boolean::{BoolOp, boolean_with_contacts};
use crate::extrude::extrude;
use crate::profile::{Loop2, Segment, build_regions};
use crate::{mass, tess};
use glam::DVec2;
use std::f64::consts::PI;

fn volume(solids: &[Solid]) -> f64 {
    volume_at(solids, tess::TessParams { chord: 1e-4, max_angle: 0.01 })
}

/// Volume tessellated well enough for a check good to a few percent (finely tessellating
/// a round of many sections takes seconds).
fn rough_volume(solids: &[Solid]) -> f64 {
    volume_at(solids, tess::TessParams { chord: 2e-3, max_angle: 0.05 })
}

fn volume_at(solids: &[Solid], params: tess::TessParams) -> f64 {
    solids
        .iter()
        .map(|s| {
            check::validate(s).expect("valid");
            let m = tess::tessellate(s, &params).expect("tess");
            assert!(m.is_watertight(), "not watertight");
            mass::properties(&m).volume
        })
        .sum()
}

fn prism(loops: &[Loop2], frame: &Frame, z0: f64, z1: f64) -> Solid {
    let r = build_regions(loops).expect("region");
    extrude(frame, &r[0], z0, z1).expect("prism")
}

fn square(s: f64) -> Loop2 {
    let p = [DVec2::ZERO, DVec2::new(s, 0.0), DVec2::new(s, s), DVec2::new(0.0, s)];
    Loop2::new((0..4).map(|i| Segment::Line { a: p[i], b: p[(i + 1) % 4] }).collect())
}

/// The rolling-ball tool applied directly (bypassing the closed-form blends).
fn roll(solid: &Solid, edge: EdgeId, r: f64) -> Vec<Solid> {
    let (tool, convex, contacts) = rolling_tool(solid, edge, r, 0).expect("tool");
    let op = if convex { BoolOp::Subtract } else { BoolOp::Union };
    boolean_with_contacts(solid, &tool, op, &contacts).expect("boolean")
}

#[test]
fn matches_the_closed_form_on_a_straight_edge() {
    // On a cube's edge the rolling ball sweeps a cylinder: the same as the prism tool.
    let cube = prism(&[square(10.0)], &Frame::world(), 0.0, 10.0);
    let e = edge_near(&cube, DVec3::new(5.0, 0.0, 10.0), 1e-6).expect("edge");
    let rolled = volume(&roll(&cube, e, 2.0));
    let want = 1000.0 - (4.0 - PI) * 10.0;
    assert!((rolled - want).abs() / want < 1e-6, "{rolled} vs {want}");
}

#[test]
fn matches_the_closed_form_on_a_slanted_edge() {
    // A wedge's sloping top meets its side at 60 degrees: still a cylinder's round, set
    // back r / tan(phi / 2) on each face.
    let wedge = Loop2::new(vec![
        Segment::Line { a: DVec2::ZERO, b: DVec2::new(10.0, 0.0) },
        Segment::Line { a: DVec2::new(10.0, 0.0), b: DVec2::new(10.0, 4.0) },
        Segment::Line { a: DVec2::new(10.0, 4.0), b: DVec2::new(0.0, 4.0 + 10.0 * (PI / 6.0).tan()) },
        Segment::Line { a: DVec2::new(0.0, 4.0 + 10.0 * (PI / 6.0).tan()), b: DVec2::ZERO },
    ]);
    let w = prism(&[wedge], &Frame::world(), 0.0, 8.0);
    let e = edge_near(&w, DVec3::new(10.0, 4.0, 4.0), 1e-6).expect("edge");
    let closed = volume(&blend_edge(&w, e, Blend::Fillet(1.5), 1, 0).expect("prism tool"));
    let rolled = volume(&roll(&w, e, 1.5));
    assert!((rolled - closed).abs() / closed < 1e-6, "{rolled} vs {closed}");
}

#[test]
fn hole_slanting_through_a_plate() {
    // A hole leaning 30 degrees through a 6 thick plate: its edge on the top face is an
    // ellipse between faces with no common axis, rounded by the rolling ball.
    let plate = prism(&[square(40.0)], &Frame::world(), 0.0, 6.0);
    let axis = DVec3::new((PI / 6.0).sin(), 0.0, (PI / 6.0).cos());
    let f = Frame::new(DVec3::new(20.0, 20.0, 3.0), axis, DVec3::Y).expect("frame");
    let hole = prism(&[Loop2::new(vec![Segment::circle(DVec2::ZERO, 4.0)])], &f, -20.0, 20.0);
    let holed = crate::boolean::boolean(&plate, &hole, BoolOp::Subtract).expect("hole").remove(0);
    let before = rough_volume(std::slice::from_ref(&holed));
    let top = holed
        .edges
        .iter()
        .position(|e| matches!(e.curve, Curve::Ellipse { frame, .. } if (frame.origin.z - 6.0).abs() < 1e-9))
        .map(|i| EdgeId(i as u32))
        .expect("top edge");
    let out = blend_edge(&holed, top, Blend::Fillet(1.0), 1, 0).expect("fillet");
    let after = rough_volume(&out);
    assert!(after < before && after > before - 2.0 * PI * 7.0, "{before} -> {after}");
    round_is_a_rolling_ball(&out[0], 1, 1.0);
}

/// Checks the round owned by `owner`: every cross-section is a circular arc of the
/// radius `r`, and along both its edges it meets the faces there tangentially.
fn round_is_a_rolling_ball(solid: &Solid, owner: u32, r: f64) {
    let round =
        solid.faces.iter().find(|f| f.owner == 1 && matches!(f.surface, Surface::BSpline(_))).expect("round");
    let Surface::BSpline(sp) = &round.surface else { unreachable!() };
    let ((s0, s1), _) = sp.domain();
    // Inside the patch: at its corners the normal is taken one-sidedly both ways.
    for k in 0..20 {
        let s = s0 + (s1 - s0) * (k as f64 + 0.5) / 20.0;
        let pts: Vec<DVec3> = (0..=8).map(|j| sp.eval_st(glam::DVec2::new(s, j as f64 / 8.0))).collect();
        // The centre: the point at distance r from the arc's ends along the normals.
        let n0 = sp.normal_st(glam::DVec2::new(s, 0.0));
        let n1 = sp.normal_st(glam::DVec2::new(s, 1.0));
        // The centre, a radius in from either edge along the normal.
        let sign = if (pts[4] - pts[0]).dot(n0) > 0.0 { 1.0 } else { -1.0 };
        let c = pts[0] + n0 * (sign * r);
        // The patch's normals come from finite differences (good to about 1e-6), so the
        // checks that use them are held to 1e-5.
        assert!((pts[8] + n1 * (sign * r) - c).length() < 1e-5);
        for p in &pts {
            assert!(((*p - c).length() - r).abs() < 1e-5, "{}", (*p - c).length());
        }
        // Tangent to the faces it lands on: another face's normal there is parallel to
        // the round's.
        for (p, n) in [(pts[0], n0), (pts[8], n1)] {
            let other = solid
                .faces
                .iter()
                .filter(|f| f.owner != owner && f.surface.distance(p) < 1e-6)
                .map(|f| f.surface.normal_at(p).cross(n).length())
                .fold(f64::MAX, f64::min);
            assert!(other < 1e-5, "not tangent: {other}");
        }
    }
}

#[test]
fn boss_root_on_a_shaft() {
    // A boss of radius 3 standing on a shaft of radius 6 at right angles: where they meet
    // is a saddle-shaped closed edge, concave, and its round adds material all round.
    let shaft = prism(
        &[Loop2::new(vec![Segment::circle(DVec2::ZERO, 6.0)])],
        &Frame::new(DVec3::new(-15.0, 0.0, 0.0), DVec3::X, DVec3::Y).expect("frame"),
        0.0,
        30.0,
    );
    let boss = prism(&[Loop2::new(vec![Segment::circle(DVec2::ZERO, 3.0)])], &Frame::world(), 0.0, 10.0);
    let part = crate::boolean::boolean(&shaft, &boss, BoolOp::Union).expect("union").remove(0);
    let before = rough_volume(std::slice::from_ref(&part));
    let root = part
        .edges
        .iter()
        .position(|e| e.start == e.end && matches!(e.curve, Curve::Spline(_)))
        .map(|i| EdgeId(i as u32))
        .expect("root edge");
    let out = blend_edge(&part, root, Blend::Fillet(1.0), 1, 0).expect("fillet");
    let added = rough_volume(&out) - before;
    // Roughly: the corner a unit round fills where faces meet at an angle phi (on the open
    // side) is cot(phi / 2) - (pi - phi) / 2, swept round the boss. At angle theta round
    // it the boss's wall and the shaft meet with cos(pi - phi) = sin^2(theta) / 2: at
    // right angles fore and aft, opening to 120 degrees at the sides.
    let n = 3600;
    let rough: f64 = (0..n)
        .map(|k| {
            let th = 2.0 * PI * (k as f64 + 0.5) / n as f64;
            let phi = PI - (th.sin().powi(2) / 2.0).acos();
            let corner = 1.0 / (phi / 2.0).tan() - (PI - phi) / 2.0;
            // Swept a little outside the boss, where the corner's middle lies.
            corner * 3.3 * 2.0 * PI / n as f64
        })
        .sum();
    // The estimate treats each section as a corner between flat faces, but the shaft
    // curves away within the round's width at the sides, so it is only a sanity check;
    // the round's shape is checked exactly below.
    assert!((added - rough).abs() / rough < 0.15, "{added} vs about {rough}");
    round_is_a_rolling_ball(&out[0], 1, 1.0);
}

/// Roughly the volume a unit fillet takes off one rim of a hole of radius 4 drilled across
/// a boss of radius 12 through its axis: round the rim (angle `a` about the hole's axis)
/// the faces meet at an angle phi on the material's side, with cos(pi - phi) = -sin^2(a)/3
/// (90 degrees at the hole's top and bottom, 70.5 at its sides), and the corner a unit
/// round takes off is cot(phi / 2) - (pi - phi) / 2, swept along the rim a little outside
/// the hole. It treats each section as a corner between flat faces; the boss's wall curves
/// within the round's width, so this runs about a tenth low.
fn cross_hole_rim_estimate() -> f64 {
    let n = 20_000;
    (0..n)
        .map(|k| {
            let a = 2.0 * PI * (k as f64 + 0.5) / n as f64;
            let x = 4.0 * a.sin();
            let y = (144.0 - x * x).sqrt();
            let d = DVec3::new(4.0 * a.cos(), -x * 4.0 * a.cos() / y, -4.0 * a.sin());
            let phi = PI - (-(a.sin().powi(2)) / 3.0).acos();
            let corner = 1.0 / (phi / 2.0).tan() - (PI - phi) / 2.0;
            corner * d.length() * (2.0 * PI / n as f64) * (4.25 / 4.0)
        })
        .sum()
}

#[test]
fn cross_hole_rim_on_a_boss() {
    // A hole of radius 4 drilled across a boss of radius 12: its rims are saddle-shaped
    // and convex, sharper at the hole's sides than at its top and bottom.
    let boss = prism(&[Loop2::new(vec![Segment::circle(DVec2::ZERO, 12.0)])], &Frame::world(), 0.0, 40.0);
    let f = Frame::new(DVec3::new(0.0, -30.0, 20.0), DVec3::Y, DVec3::X).expect("frame");
    let hole = prism(&[Loop2::new(vec![Segment::circle(DVec2::ZERO, 4.0)])], &f, 0.0, 60.0);
    let part = crate::boolean::boolean(&boss, &hole, BoolOp::Subtract).expect("hole").remove(0);
    let before = rough_volume(std::slice::from_ref(&part));
    let rim = part
        .edges
        .iter()
        .position(|e| e.start == e.end && matches!(e.curve, Curve::Spline(_)))
        .map(|i| EdgeId(i as u32))
        .expect("rim");
    let out = blend_edge(&part, rim, Blend::Fillet(1.0), 1, 0).expect("fillet");
    let taken = before - rough_volume(&out);
    let rough = cross_hole_rim_estimate();
    assert!(taken > rough && taken < 1.2 * rough, "{taken} vs about {rough}");
    round_is_a_rolling_ball(&out[0], 1, 1.0);
}
