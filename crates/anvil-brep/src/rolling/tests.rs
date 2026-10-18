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
    let (tool, convex, contacts) = rolling_tool(solid, edge, Blend::Fillet(r), 0).expect("tool");
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

/// A stadium: two half circles of radius `r` about (0, 0) and (`len`, 0), joined by
/// straight sides.
fn stadium(r: f64, len: f64) -> Loop2 {
    Loop2::new(vec![
        Segment::Line { a: DVec2::new(0.0, -r), b: DVec2::new(len, -r) },
        Segment::Arc { center: DVec2::new(len, 0.0), radius: r, start: -0.5 * PI, sweep: PI },
        Segment::Line { a: DVec2::new(len, r), b: DVec2::new(0.0, r) },
        Segment::Arc { center: DVec2::ZERO, radius: r, start: 0.5 * PI, sweep: PI },
    ])
}

#[test]
fn run_matches_the_closed_form_round_a_slot() {
    // A slot's top outline (lines and half circles running smoothly into each other):
    // one ball rolled all the way round gives what the cylinders and tori give.
    let slot = prism(&[stadium(4.0, 10.0)], &Frame::world(), 0.0, 5.0);
    let edges: Vec<EdgeId> = [
        DVec3::new(5.0, -4.0, 5.0),
        DVec3::new(14.0, 0.0, 5.0),
        DVec3::new(5.0, 4.0, 5.0),
        DVec3::new(-4.0, 0.0, 5.0),
    ]
    .map(|p| edge_near(&slot, p, 1e-6).expect("edge"))
    .to_vec();
    let tagged: Vec<(EdgeId, u32)> = edges.iter().map(|e| (*e, 0)).collect();
    let closed =
        volume(&crate::blend::blend_chain(&slot, &tagged, Blend::Fillet(1.0), 1).expect("closed form"));
    let (tool, convex, contacts) = rolling_run_tool(&slot, &edges, Blend::Fillet(1.0), 0).expect("tool");
    assert!(convex);
    assert_eq!(tool.faces.iter().filter(|f| matches!(f.surface, Surface::BSpline(_))).count(), 5);
    let rolled = volume(&boolean_with_contacts(&slot, &tool, BoolOp::Subtract, &contacts).expect("boolean"));
    // The rounds agree to the fit (1e-7 mm, a few millionths of a cubic millimetre here);
    // the volumes, tessellated from a B-spline on one side and cylinders and tori on the
    // other, to about 1e-6.
    assert!((rolled - closed).abs() / closed < 1e-5, "{rolled} vs {closed}");
}

#[test]
fn slot_boss_root_on_a_shaft() {
    // A slot-shaped boss standing across a shaft of radius 10: its root runs along the
    // shaft beside the straight sides and round it in saddles at the ends, concave and
    // smooth all the way round. One ball rolls along all four edges.
    let shaft = prism(
        &[Loop2::new(vec![Segment::circle(DVec2::ZERO, 10.0)])],
        &Frame::new(DVec3::new(-20.0, 0.0, 0.0), DVec3::X, DVec3::Y).expect("frame"),
        0.0,
        50.0,
    );
    let boss = prism(&[stadium(3.0, 8.0)], &Frame::world(), 0.0, 16.0);
    let part = crate::boolean::boolean(&shaft, &boss, BoolOp::Union).expect("union").remove(0);
    // The root's edges: on the shaft, shared with the boss's sides.
    let shaft_face = |f: &crate::topo::Face| matches!(f.surface, Surface::Cylinder { radius, .. } if (radius - 10.0).abs() < 1e-9);
    let root: Vec<EdgeId> = (0..part.edges.len())
        .map(|i| EdgeId(i as u32))
        .filter(|&id| {
            let faces = part.edge_faces(id);
            // Up where the boss stands, not round the shaft's end faces.
            let high = part.edge(id).is_some_and(|e| e.curve.eval(0.5 * (e.t0 + e.t1)).z > 5.0);
            faces.len() == 2
                && high
                && faces.iter().any(|f| part.face(*f).is_some_and(shaft_face))
                && faces.iter().any(|f| part.face(*f).is_some_and(|x| !shaft_face(x)))
        })
        .collect();
    assert_eq!(root.len(), 4);
    let before = rough_volume(std::slice::from_ref(&part));
    let run = crate::blend::smooth_run(&part, root[0], &root);
    assert_eq!(run.len(), 4);
    let tagged: Vec<(EdgeId, u32)> = run.iter().map(|e| (*e, 0)).collect();
    let out = crate::blend::blend_chain(&part, &tagged, Blend::Fillet(1.0), 1).expect("fillet");
    let added = rough_volume(&out) - before;
    // Along the straight sides the faces meet at about 90 degrees (the shaft's top is
    // nearly flat there), round the ends less sharply: between the corner a flat-on-flat
    // round fills at 120 degrees and at 90, along a root about 2 * 8 + 2 pi 3 long.
    let length = 16.0 + 2.0 * PI * 3.0;
    let (least, most) = ((1.0 / (PI / 3.0).tan() - PI / 6.0) * length, (1.0 - PI / 4.0) * length * 1.2);
    assert!(added > least && added < most, "{added} not in {least}..{most}");
    assert_eq!(
        out[0].faces.iter().filter(|f| f.owner == 1 && matches!(f.surface, Surface::BSpline(_))).count(),
        1
    );
    round_is_a_rolling_ball(&out[0], 1, 1.0);
}

#[test]
fn slot_boss_root_at_larger_radii() {
    // Larger rounds on the same root (its run starts half way along an edge, so the
    // round's seam is clear of the joins between the boss's faces).
    let shaft = prism(
        &[Loop2::new(vec![Segment::circle(DVec2::ZERO, 10.0)])],
        &Frame::new(DVec3::new(-20.0, 0.0, 0.0), DVec3::X, DVec3::Y).expect("frame"),
        0.0,
        50.0,
    );
    let boss = prism(&[stadium(3.0, 8.0)], &Frame::world(), 0.0, 16.0);
    let part = crate::boolean::boolean(&shaft, &boss, BoolOp::Union).expect("union").remove(0);
    let shaft_face = |f: &crate::topo::Face| matches!(f.surface, Surface::Cylinder { radius, .. } if (radius - 10.0).abs() < 1e-9);
    let root: Vec<EdgeId> = (0..part.edges.len())
        .map(|i| EdgeId(i as u32))
        .filter(|&id| {
            let faces = part.edge_faces(id);
            let high = part.edge(id).is_some_and(|e| e.curve.eval(0.5 * (e.t0 + e.t1)).z > 5.0);
            faces.len() == 2
                && high
                && faces.iter().any(|f| part.face(*f).is_some_and(shaft_face))
                && faces.iter().any(|f| part.face(*f).is_some_and(|x| !shaft_face(x)))
        })
        .collect();
    let tagged: Vec<(EdgeId, u32)> = root.iter().map(|e| (*e, 0)).collect();
    for r in [1.5, 2.0] {
        let out = crate::blend::blend_chain(&part, &tagged, Blend::Fillet(r), 1).expect("fillet");
        assert!(rough_volume(&out) > rough_volume(std::slice::from_ref(&part)));
        round_is_a_rolling_ball(&out[0], 1, r);
    }
}

/// The swept chamfer tool applied directly (bypassing the closed-form chamfers).
fn bevel(solid: &Solid, edge: EdgeId, c: f64) -> Vec<Solid> {
    let (tool, convex, contacts) = rolling_tool(solid, edge, Blend::Chamfer(c), 0).expect("tool");
    let op = if convex { BoolOp::Subtract } else { BoolOp::Union };
    boolean_with_contacts(solid, &tool, op, &contacts).expect("boolean")
}

/// Checks the bevel owned by `owner`: every section across it is straight, and its two
/// edges lie the chamfer's distance `c` from `edge` (the edge it replaced).
fn bevel_is_straight(solid: &Solid, owner: u32, edge: &crate::topo::Edge, c: f64) {
    let face = solid
        .faces
        .iter()
        .find(|f| f.owner == owner && matches!(f.surface, Surface::BSpline(_)))
        .expect("bevel");
    let Surface::BSpline(sp) = &face.surface else { unreachable!() };
    let ((s0, s1), _) = sp.domain();
    for k in 0..20 {
        let s = s0 + (s1 - s0) * (k as f64 + 0.5) / 20.0;
        let at = |t: f64| sp.eval_st(glam::DVec2::new(s, t));
        let (a, m, b) = (at(0.0), at(0.5), at(1.0));
        assert!((m - (a + b) * 0.5).length() < 1e-9, "not straight");
        for p in [a, b] {
            let d = edge.curve.distance_to(p, edge.t0, edge.t1);
            assert!((d - c).abs() < 1e-6, "{d} from the edge");
        }
    }
}

#[test]
fn chamfer_matches_the_closed_form_on_flat_faces() {
    // A cube's edge, and a 60 degree wedge's: set back the same along both faces as the
    // prism tool's chamfer.
    let cube = prism(&[square(10.0)], &Frame::world(), 0.0, 10.0);
    let e = edge_near(&cube, DVec3::new(5.0, 0.0, 10.0), 1e-6).expect("edge");
    let want = 1000.0 - 4.5 * 10.0;
    assert!((volume(&bevel(&cube, e, 3.0)) - want).abs() / want < 1e-9);
    let wedge = Loop2::new(vec![
        Segment::Line { a: DVec2::ZERO, b: DVec2::new(10.0, 0.0) },
        Segment::Line { a: DVec2::new(10.0, 0.0), b: DVec2::new(10.0, 4.0) },
        Segment::Line { a: DVec2::new(10.0, 4.0), b: DVec2::new(0.0, 4.0 + 10.0 * (PI / 6.0).tan()) },
        Segment::Line { a: DVec2::new(0.0, 4.0 + 10.0 * (PI / 6.0).tan()), b: DVec2::ZERO },
    ]);
    let w = prism(&[wedge], &Frame::world(), 0.0, 8.0);
    let e = edge_near(&w, DVec3::new(10.0, 4.0, 4.0), 1e-6).expect("edge");
    let closed = volume(&blend_edge(&w, e, Blend::Chamfer(1.5), 1, 0).expect("prism tool"));
    let swept = volume(&bevel(&w, e, 1.5));
    assert!((swept - closed).abs() / closed < 1e-9, "{swept} vs {closed}");
}

#[test]
fn chamfer_a_hole_slanting_through_a_plate() {
    // The slanted hole's elliptical rim, between a plane and a cylinder with no common
    // axis: the bevel is straight across, set back 1 from the rim on both faces.
    let plate = prism(&[square(40.0)], &Frame::world(), 0.0, 6.0);
    let axis = DVec3::new((PI / 6.0).sin(), 0.0, (PI / 6.0).cos());
    let f = Frame::new(DVec3::new(20.0, 20.0, 3.0), axis, DVec3::Y).expect("frame");
    let hole = prism(&[Loop2::new(vec![Segment::circle(DVec2::ZERO, 4.0)])], &f, -20.0, 20.0);
    let holed = crate::boolean::boolean(&plate, &hole, BoolOp::Subtract).expect("hole").remove(0);
    let top = holed
        .edges
        .iter()
        .position(|e| matches!(e.curve, Curve::Ellipse { frame, .. } if (frame.origin.z - 6.0).abs() < 1e-9))
        .map(|i| EdgeId(i as u32))
        .expect("top edge");
    let rim = holed.edges[top.index()].clone();
    let before = rough_volume(std::slice::from_ref(&holed));
    let out = blend_edge(&holed, top, Blend::Chamfer(1.0), 1, 0).expect("chamfer");
    // Somewhere between a right-angled corner's half square and twice it, round a rim a
    // little longer than the hole's circumference.
    let taken = before - rough_volume(&out);
    let around = 2.0 * PI * 4.0;
    assert!(taken > 0.5 * around * 0.8 && taken < 0.5 * around * 2.0, "{taken}");
    bevel_is_straight(&out[0], 1, &rim, 1.0);
}

#[test]
fn chamfer_a_slot_boss_root_on_a_shaft() {
    // The concave run round a slot-shaped boss on a shaft, bevelled as one face.
    let shaft = prism(
        &[Loop2::new(vec![Segment::circle(DVec2::ZERO, 10.0)])],
        &Frame::new(DVec3::new(-20.0, 0.0, 0.0), DVec3::X, DVec3::Y).expect("frame"),
        0.0,
        50.0,
    );
    let boss = prism(&[stadium(3.0, 8.0)], &Frame::world(), 0.0, 16.0);
    let part = crate::boolean::boolean(&shaft, &boss, BoolOp::Union).expect("union").remove(0);
    let shaft_face = |f: &crate::topo::Face| matches!(f.surface, Surface::Cylinder { radius, .. } if (radius - 10.0).abs() < 1e-9);
    let root: Vec<EdgeId> = (0..part.edges.len())
        .map(|i| EdgeId(i as u32))
        .filter(|&id| {
            let faces = part.edge_faces(id);
            let high = part.edge(id).is_some_and(|e| e.curve.eval(0.5 * (e.t0 + e.t1)).z > 5.0);
            faces.len() == 2
                && high
                && faces.iter().any(|f| part.face(*f).is_some_and(shaft_face))
                && faces.iter().any(|f| part.face(*f).is_some_and(|x| !shaft_face(x)))
        })
        .collect();
    let tagged: Vec<(EdgeId, u32)> = root.iter().map(|e| (*e, 0)).collect();
    let before = rough_volume(std::slice::from_ref(&part));
    let out = crate::blend::blend_chain(&part, &tagged, Blend::Chamfer(1.0), 1).expect("chamfer");
    assert!(rough_volume(&out) > before);
    assert_eq!(
        out[0].faces.iter().filter(|f| f.owner == 1 && matches!(f.surface, Surface::BSpline(_))).count(),
        1
    );
}

/// The swept tool for any blend, applied directly.
fn swept(solid: &Solid, edge: EdgeId, blend: Blend) -> Vec<Solid> {
    let (tool, convex, contacts) = rolling_tool(solid, edge, blend, 0).expect("tool");
    let op = if convex { BoolOp::Subtract } else { BoolOp::Union };
    boolean_with_contacts(solid, &tool, op, &contacts).expect("boolean")
}

/// The face of `s` with a plane through `p` square to `n`.
fn plane_face(s: &Solid, p: DVec3, n: DVec3) -> usize {
    s.faces
        .iter()
        .position(|f| matches!(f.surface, Surface::Plane { frame } if frame.z.cross(n).length() < 1e-9 && (p - frame.origin).dot(frame.z).abs() < 1e-9))
        .expect("plane face")
}

#[test]
fn angle_and_offset_chamfers_on_flat_faces() {
    // A cube's top edge, 3 along the top at 30 degrees: down the side 3 tan 30.
    let cube = prism(&[square(10.0)], &Frame::world(), 0.0, 10.0);
    let e = edge_near(&cube, DVec3::new(5.0, 0.0, 10.0), 1e-6).expect("edge");
    let top = plane_face(&cube, DVec3::new(0.0, 0.0, 10.0), DVec3::Z);
    let angle = Blend::ChamferAngle { distance: 3.0, angle: PI / 6.0, along: top };
    let want = 1000.0 - 0.5 * 3.0 * 3.0 * (PI / 6.0).tan() * 10.0;
    for out in [blend_edge(&cube, e, angle, 1, 0).expect("closed form"), swept(&cube, e, angle)] {
        assert!((volume(&out) - want).abs() / want < 1e-9, "{} vs {want}", volume(&out));
    }
    // At right angles an offset face chamfer is the equal-distance one.
    let want = 1000.0 - 4.5 * 10.0;
    for out in [
        blend_edge(&cube, e, Blend::ChamferOffset(3.0), 1, 0).expect("closed form"),
        swept(&cube, e, Blend::ChamferOffset(3.0)),
    ] {
        assert!((volume(&out) - want).abs() / want < 1e-9, "{} vs {want}", volume(&out));
    }
    // A wedge's edge where its side and sloping top meet at 120 degrees: both kinds the
    // same from the prism tool and the sweep.
    let wedge = Loop2::new(vec![
        Segment::Line { a: DVec2::ZERO, b: DVec2::new(10.0, 0.0) },
        Segment::Line { a: DVec2::new(10.0, 0.0), b: DVec2::new(10.0, 4.0) },
        Segment::Line { a: DVec2::new(10.0, 4.0), b: DVec2::new(0.0, 4.0 + 10.0 * (PI / 6.0).tan()) },
        Segment::Line { a: DVec2::new(0.0, 4.0 + 10.0 * (PI / 6.0).tan()), b: DVec2::ZERO },
    ]);
    let w = prism(&[wedge], &Frame::world(), 0.0, 8.0);
    let e = edge_near(&w, DVec3::new(10.0, 4.0, 4.0), 1e-6).expect("edge");
    let side = plane_face(&w, DVec3::new(10.0, 0.0, 0.0), DVec3::X);
    // On faces meeting at 120 degrees an offset face chamfer of 1 sets back cot 60.
    let offset = Blend::ChamferOffset(1.0);
    let equal = Blend::Chamfer(1.0 / (PI / 3.0).tan());
    let want = volume(&blend_edge(&w, e, equal, 1, 0).expect("equal"));
    for blend in [offset, Blend::ChamferAngle { distance: 1.5, angle: 0.5, along: side }] {
        let closed = volume(&blend_edge(&w, e, blend, 1, 0).expect("closed form"));
        let sweep = volume(&swept(&w, e, blend));
        assert!((sweep - closed).abs() / closed < 1e-9, "{blend:?}: {sweep} vs {closed}");
        if blend == offset {
            assert!((closed - want).abs() / want < 1e-9, "{closed} vs {want}");
        }
    }
}

#[test]
fn angle_and_offset_chamfers_on_a_slanted_hole() {
    // The slanted hole's rim: an angle-distance bevel starts 1 from the rim on the plate
    // and leaves it at exactly the angle; an offset face bevel's ends are where a ball of
    // the distance rolling round the rim touches the plate and the hole.
    let plate = prism(&[square(40.0)], &Frame::world(), 0.0, 6.0);
    let axis = DVec3::new((PI / 6.0).sin(), 0.0, (PI / 6.0).cos());
    let f = Frame::new(DVec3::new(20.0, 20.0, 3.0), axis, DVec3::Y).expect("frame");
    let hole = prism(&[Loop2::new(vec![Segment::circle(DVec2::ZERO, 4.0)])], &f, -20.0, 20.0);
    let holed = crate::boolean::boolean(&plate, &hole, BoolOp::Subtract).expect("hole").remove(0);
    let rim_id = holed
        .edges
        .iter()
        .position(|e| matches!(e.curve, Curve::Ellipse { frame, .. } if (frame.origin.z - 6.0).abs() < 1e-9))
        .map(|i| EdgeId(i as u32))
        .expect("rim");
    let rim = holed.edges[rim_id.index()].clone();
    let top = plane_face(&holed, DVec3::new(0.0, 0.0, 6.0), DVec3::Z);
    let angle = 0.4;
    let out = blend_edge(&holed, rim_id, Blend::ChamferAngle { distance: 1.0, angle, along: top }, 1, 0)
        .expect("angle");
    let bevel = out[0]
        .faces
        .iter()
        .find(|f| f.owner == 1 && matches!(f.surface, Surface::BSpline(_)))
        .expect("bevel");
    let Surface::BSpline(sp) = &bevel.surface else { unreachable!() };
    let ((s0, s1), _) = sp.domain();
    for k in 0..20 {
        let s = s0 + (s1 - s0) * (k as f64 + 0.5) / 20.0;
        let (a, b) = (sp.eval_st(glam::DVec2::new(s, 0.0)), sp.eval_st(glam::DVec2::new(s, 1.0)));
        assert!((a.z - 6.0).abs() < 1e-6, "side A is the plate");
        assert!((rim.curve.distance_to(a, rim.t0, rim.t1) - 1.0).abs() < 1e-6);
        let leaving = ((b - a).normalize().z.abs()).asin();
        assert!((leaving - angle).abs() < 1e-6, "{leaving} vs {angle}");
    }
    let out = blend_edge(&holed, rim_id, Blend::ChamferOffset(1.0), 1, 0).expect("offset");
    assert!(rough_volume(&out) < rough_volume(std::slice::from_ref(&holed)));
}

/// The round face of a blended solid (the B-spline face owned by `owner`).
fn round_patch(solid: &Solid, owner: u32) -> Arc<SplineSurface> {
    let f = solid.faces.iter().find(|f| f.owner == owner && matches!(f.surface, Surface::BSpline(_)));
    match f.map(|f| &f.surface) {
        Some(Surface::BSpline(sp)) => sp.clone(),
        _ => panic!("no round"),
    }
}

/// The area cut from a right-angled corner (at the origin, faces along x and y) by the
/// profile `curve` from (r, 0) to (0, r): the polygon of the corner and many points of
/// the curve.
fn corner_area(curve: impl Fn(f64) -> DVec2) -> f64 {
    let n = 200_000;
    let mut pts = vec![DVec2::ZERO];
    pts.extend((0..=n).map(|k| curve(k as f64 / n as f64)));
    (0..pts.len()).map(|k| pts[k].perp_dot(pts[(k + 1) % pts.len()])).sum::<f64>().abs() / 2.0
}

/// The rational quadratic from `a` to `b` about corner `c` weighted `w`, at `t`.
fn conic(a: DVec2, c: DVec2, b: DVec2, w: f64, t: f64) -> DVec2 {
    let (b0, b1, b2) = ((1.0 - t) * (1.0 - t), 2.0 * t * (1.0 - t) * w, t * t);
    (a * b0 + c * b1 + b * b2) / (b0 + b1 + b2)
}

#[test]
fn conic_rho_round_on_a_cube_edge() {
    // A cube's top edge with r = 2: contacts 2 from the edge on each face, the conic's
    // corner on the edge. Rho 0.5 is a parabola, 0.8 a hyperbola nearer the corner.
    let cube = prism(&[square(10.0)], &Frame::world(), 0.0, 10.0);
    let e = edge_near(&cube, DVec3::new(5.0, 0.0, 10.0), 1e-6).expect("edge");
    for rho in [0.5, 0.8] {
        let blend = Blend::Profiled { radius: 2.0, profile: Profile::ConicRho(rho) };
        let w = rho / (1.0 - rho);
        let area = corner_area(|t| conic(DVec2::new(2.0, 0.0), DVec2::ZERO, DVec2::new(0.0, 2.0), w, t));
        let want = 1000.0 - 10.0 * area;
        for out in [swept(&cube, e, blend), blend_edge(&cube, e, blend, 1, 0).expect("fillet")] {
            let v = volume(&out);
            // Tessellating the curved round holds the volume to about 1e-6.
            assert!((v - want).abs() / want < 1e-6, "rho {rho}: {v} vs {want}");
        }
        // The shoulder point lies rho of the way from the chord's middle to the corner.
        let out = blend_edge(&cube, e, blend, 1, 0).expect("fillet");
        let sp = round_patch(&out[0], 1);
        let ((s0, s1), _) = sp.domain();
        let p = sp.eval_st(DVec2::new(0.5 * (s0 + s1), 0.5));
        let (m, corner) = (DVec2::new(1.0, 9.0), DVec2::new(0.0, 10.0));
        let want = m + (corner - m) * rho;
        assert!((DVec2::new(p.y, p.z) - want).length() < 1e-9, "{p} vs {want}");
    }
    // Rho outside the published range is refused.
    let bad = Blend::Profiled { radius: 2.0, profile: Profile::ConicRho(0.4) };
    assert!(matches!(blend_edge(&cube, e, bad, 1, 0), Err(KernelError::InvalidInput(_))));
}

#[test]
fn conic_radius_round_sets_the_shoulder_curvature() {
    let cube = prism(&[square(10.0)], &Frame::world(), 0.0, 10.0);
    let e = edge_near(&cube, DVec3::new(5.0, 0.0, 10.0), 1e-6).expect("edge");
    // A shoulder radius of 1 on contacts 2 from a right-angled corner.
    let blend = Blend::Profiled { radius: 2.0, profile: Profile::ConicRadius(1.0) };
    let out = blend_edge(&cube, e, blend, 1, 0).expect("fillet");
    let sp = round_patch(&out[0], 1);
    let ((s0, s1), _) = sp.domain();
    let s = 0.5 * (s0 + s1);
    // The curvature of the section at its middle, from its control points (the corner
    // weighted): the patch's weights and points along a section row.
    let k = 3 * (sp.count.0 / 6);
    let row = |j: usize| (sp.points[k * 3 + j], sp.weights[k * 3 + j]);
    let ((a, _), (c, w), (b, _)) = (row(0), row(1), row(2));
    assert!((shoulder_curvature(a, c, b, w) - 1.0).abs() < 1e-9);
    // And measured on the surface itself, by finite differences across it.
    let h = 1e-4;
    let at = |t: f64| sp.eval_st(DVec2::new(s, t));
    let (p0, p1, p2) = (at(0.5 - h), at(0.5), at(0.5 + h));
    let (d1, d2) = ((p2 - p0) / (2.0 * h), (p2 - p1 * 2.0 + p0) / (h * h));
    let measured = d1.cross(d2).length() / d1.length().powi(3);
    assert!((measured - 1.0).abs() < 1e-5, "{measured}");
    // The volume matches the conic of that weight.
    let area = corner_area(|t| conic(DVec2::new(2.0, 0.0), DVec2::ZERO, DVec2::new(0.0, 2.0), w, t));
    let want = 1000.0 - 10.0 * area;
    assert!((volume(&out) - want).abs() / want < 1e-6);
    // No conic between these contacts is that sharp at its shoulder, or that flat.
    for r in [1e-7, 1e9] {
        let blend = Blend::Profiled { radius: 2.0, profile: Profile::ConicRadius(r) };
        assert!(blend_edge(&cube, e, blend, 1, 0).is_err(), "{r}");
    }
}

/// The curvature at the start of the Bezier curve with these control points.
fn start_curvature(p: &[DVec3]) -> f64 {
    let n = (p.len() - 1) as f64;
    let (d1, d2) = (p[1] - p[0], p[2] - p[1]);
    (n - 1.0) / n * d1.cross(d2).length() / d1.length().powi(3)
}

#[test]
fn curvature_continuous_round_on_a_cube_edge() {
    // Between flat faces the round leaves each face with no curvature at all.
    let cube = prism(&[square(10.0)], &Frame::world(), 0.0, 10.0);
    let e = edge_near(&cube, DVec3::new(5.0, 0.0, 10.0), 1e-6).expect("edge");
    let blend = Blend::Profiled { radius: 2.0, profile: Profile::CurvatureContinuous };
    let out = blend_edge(&cube, e, blend, 1, 0).expect("fillet");
    let sp = round_patch(&out[0], 1);
    assert_eq!(sp.degree.1, 4);
    for k in (0..sp.count.0).step_by(3) {
        let row: Vec<DVec3> = (0..5).map(|j| sp.points[k * 5 + j]).collect();
        let back: Vec<DVec3> = row.iter().rev().copied().collect();
        assert!(start_curvature(&row) < 1e-12 && start_curvature(&back) < 1e-12);
    }
    // The quartic from (2, 0) to (0, 2) with its second and fourth points half way to the
    // corner and its middle one on it.
    let quartic = |t: f64| {
        let p = [
            DVec2::new(2.0, 0.0),
            DVec2::new(1.0, 0.0),
            DVec2::ZERO,
            DVec2::new(0.0, 1.0),
            DVec2::new(0.0, 2.0),
        ];
        let c = [1.0, 4.0, 6.0, 4.0, 1.0];
        (0..5).map(|i| p[i] * (c[i] * t.powi(i as i32) * (1.0 - t).powi(4 - i as i32))).sum::<DVec2>()
    };
    let want = 1000.0 - 10.0 * corner_area(quartic);
    let v = volume(&out);
    assert!((v - want).abs() / want < 1e-6, "{v} vs {want}");
}

#[test]
fn curvature_continuous_round_matches_a_cylinder() {
    // A plate 4 thick with a ridge along z: a cylinder of radius 5 about (10, 6), rising
    // from the plate's top where it crosses y = 4. The edges where it rises are concave,
    // between the flat top and the cylinder, whose section square to them is a circle.
    let x = 21f64.sqrt();
    let (c, r) = (DVec2::new(10.0, 6.0), 5.0);
    let (p, q) = (DVec2::new(10.0 + x, 4.0), DVec2::new(10.0 - x, 4.0));
    let outline = Loop2::new(vec![
        Segment::Line { a: DVec2::ZERO, b: DVec2::new(20.0, 0.0) },
        Segment::Line { a: DVec2::new(20.0, 0.0), b: DVec2::new(20.0, 4.0) },
        Segment::Line { a: DVec2::new(20.0, 4.0), b: p },
        Segment::arc_from_center(c, p, q, true).expect("arc"),
        Segment::Line { a: q, b: DVec2::new(0.0, 4.0) },
        Segment::Line { a: DVec2::new(0.0, 4.0), b: DVec2::ZERO },
    ]);
    let ridge = prism(&[outline], &Frame::world(), 0.0, 10.0);
    let e = edge_near(&ridge, DVec3::new(10.0 + x, 4.0, 5.0), 1e-6).expect("edge");
    let before = volume(std::slice::from_ref(&ridge));
    let blend = Blend::Profiled { radius: 1.0, profile: Profile::CurvatureContinuous };
    let out = blend_edge(&ridge, e, blend, 1, 0).expect("fillet");
    let after = volume(&out);
    // Material is added in the corner, less than a circular round's.
    let circular = volume(&blend_edge(&ridge, e, Blend::Fillet(1.0), 1, 0).expect("fillet"));
    assert!(after > before && after < circular, "{before} {after} {circular}");
    // At each section the curvature where the round meets the cylinder is the
    // cylinder's (1 / 5), and where it meets the flat top, none.
    let sp = round_patch(&out[0], 1);
    for k in (0..sp.count.0).step_by(3) {
        let row: Vec<DVec3> = (0..5).map(|j| sp.points[k * 5 + j]).collect();
        let back: Vec<DVec3> = row.iter().rev().copied().collect();
        for ends in [&row, &back] {
            let on_cylinder = ((ends[0].truncate() - c).length() - r).abs() < 1e-9;
            let on_top = (ends[0].y - 4.0).abs() < 1e-9;
            assert!(on_cylinder != on_top, "{}", ends[0]);
            let want = if on_cylinder { 1.0 / r } else { 0.0 };
            assert!((start_curvature(ends) - want).abs() < 1e-7, "{} vs {want}", start_curvature(ends));
            if on_cylinder {
                // It bends the same way as the cylinder there, towards its axis.
                let t = (ends[1] - ends[0]).normalize();
                let off = (ends[2] - ends[0]) - t * (ends[2] - ends[0]).dot(t);
                let axis = c.extend(ends[0].z);
                assert!(off.dot(axis - ends[0]) > 0.0);
            }
        }
    }
}

#[test]
fn rounds_on_edges_meeting_at_a_corner() {
    // Two top edges of a cube meeting at a corner, rounded one after the other with the
    // same profile. After the first, the second edge runs smoothly on into the first
    // round's edge down the side, so its round stops there, 2 from the corner (as for the
    // closed-form rounds): the rounds remove the profile's area along 10 and along 8.
    // Where they meet, both are tangent to the top face at one point.
    let cube = prism(&[square(10.0)], &Frame::world(), 0.0, 10.0);
    let w = 0.75 / 0.25;
    let quartic = |t: f64| {
        let p = [
            DVec2::new(2.0, 0.0),
            DVec2::new(1.0, 0.0),
            DVec2::ZERO,
            DVec2::new(0.0, 1.0),
            DVec2::new(0.0, 2.0),
        ];
        let c = [1.0, 4.0, 6.0, 4.0, 1.0];
        (0..5).map(|i| p[i] * (c[i] * t.powi(i as i32) * (1.0 - t).powi(4 - i as i32))).sum::<DVec2>()
    };
    let rho = |t: f64| conic(DVec2::new(2.0, 0.0), DVec2::ZERO, DVec2::new(0.0, 2.0), w, t);
    let circular = 4.0 - PI;
    let profiles = [
        (Blend::Fillet(2.0), circular),
        (Blend::Profiled { radius: 2.0, profile: Profile::ConicRho(0.75) }, corner_area(rho)),
        (Blend::Profiled { radius: 2.0, profile: Profile::CurvatureContinuous }, corner_area(quartic)),
    ];
    let e1 = edge_near(&cube, DVec3::new(5.0, 0.0, 10.0), 1e-6).expect("edge");
    let mut closed_form = 0.0;
    // Each profile first and second, and each meeting itself.
    for (i, j) in [(0, 0), (0, 1), (1, 2), (2, 2)] {
        let ((first, _), (second, area)) = (profiles[i], profiles[j]);
        {
            // A circular round swept (as on an edge with no closed form), then any.
            let a = if matches!(first, Blend::Fillet(_)) {
                swept(&cube, e1, first)
            } else {
                blend_edge(&cube, e1, first, 1, 0).expect("first")
            };
            let first_area = profiles.iter().find(|p| p.0 == first).map(|p| p.1).unwrap_or(0.0);
            let e2 = edge_near(&a[0], DVec3::new(10.0, 5.0, 10.0), 1e-6).expect("edge");
            let both = swept(&a[0], e2, second);
            assert_eq!(both.len(), 1);
            let v = volume(&both);
            let want = 1000.0 - 10.0 * first_area - 8.0 * area;
            assert!((v - want).abs() / want < 1e-6, "{first:?} then {second:?}: {v} vs {want}");
            if matches!((first, second), (Blend::Fillet(_), Blend::Fillet(_))) {
                closed_form = v;
            }
        }
    }
    // The swept circular rounds give what the closed-form ones do.
    let a = blend_edge(&cube, e1, Blend::Fillet(2.0), 1, 0).expect("first");
    let e2 = edge_near(&a[0], DVec3::new(10.0, 5.0, 10.0), 1e-6).expect("edge");
    let v = volume(&blend_edge(&a[0], e2, Blend::Fillet(2.0), 1, 8).expect("second"));
    assert!((v - closed_form).abs() / v < 1e-8, "{v} vs {closed_form}");
}
