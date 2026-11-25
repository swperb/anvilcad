//! Spline sketch entities and their relations (docs/re/sketch-entities.md, "Splines").

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use anvil_kernel::{v2, Vec2};
use anvil_sketch::spline::{self, Spline2};
use anvil_sketch::{Constraint, Geom, PointId, Sketch, SketchError};

fn pts_of(sk: &Sketch, e: usize) -> Vec<PointId> {
    match &sk.entities[e].geom {
        Geom::Spline { pts, .. } | Geom::ControlSpline { pts } => pts.clone(),
        _ => panic!("not a spline"),
    }
}

fn fix_all(sk: &mut Sketch, ids: &[PointId]) {
    for &p in ids {
        let q = sk.pos(p);
        sk.add_constraint(Constraint::Fix { p, x: q.x, y: q.y });
    }
}

fn curve(sk: &Sketch, e: usize) -> Spline2 {
    spline::of(sk, e).expect("spline geometry")
}

/// The arch from (0, 0) through control points (0, 30) and (40, 30) to (40, 0), closed by
/// its base, encloses 3/5 of its 40 by 30 box: 720.
#[test]
fn control_spline_profile_has_exact_area() {
    let mut sk = Sketch::new();
    let s =
        sk.add_control_spline(&[v2(0.0, 0.0), v2(0.0, 30.0), v2(40.0, 30.0), v2(40.0, 0.0)]).expect("spline");
    let p = pts_of(&sk, s);
    sk.add_line(p[3], p[0]);
    let r = sk.solve();
    assert!(r.converged, "{r:?}");
    // Four free vertices; the base adds none of its own.
    assert_eq!(r.dof, 8, "{r:?}");
    let profiles = sk.profiles().expect("closed");
    assert_eq!(profiles.len(), 1);
    assert!((profiles[0].area() - 720.0).abs() < 1e-9, "{}", profiles[0].area());
}

/// A spline through points is one cubic span per pair of points; closed by a line it
/// makes a profile whose area is the exact area of those spans.
#[test]
fn through_spline_profile() {
    let mut sk = Sketch::new();
    let s = sk.add_spline(&[v2(0.0, 0.0), v2(10.0, 15.0), v2(25.0, 12.0), v2(40.0, 0.0)]).expect("spline");
    let p = pts_of(&sk, s);
    sk.add_line(p[3], p[0]);
    let r = sk.solve();
    assert_eq!(r.dof, 8, "{r:?}");
    let sp = curve(&sk, s);
    assert_eq!(sp.spans.len(), 3);
    // The area by Green's theorem, numerically on a fine polyline, agrees with the exact
    // area the kernel profile reports.
    let poly = sp.polyline(400);
    let green: f64 = poly.windows(2).map(|w| 0.5 * (w[0].x * w[1].y - w[1].x * w[0].y)).sum();
    let area = sk.profiles().expect("closed")[0].area();
    assert!((area - green.abs()).abs() < 1e-3, "{area} vs {green}");
}

/// sketch-entities.md statement 36: through (0, 0), (20, 10), (40, 0) with natural ends,
/// each span is y = 15u - 5u^3 over x = 20u, so the spline and its base enclose 250.
#[test]
fn natural_spline_area_has_a_closed_form() {
    let mut sk = Sketch::new();
    let s = sk.add_spline(&[v2(0.0, 0.0), v2(20.0, 10.0), v2(40.0, 0.0)]).expect("spline");
    let p = pts_of(&sk, s);
    sk.add_line(p[2], p[0]);
    let area = sk.profiles().expect("closed")[0].area();
    assert!((area - 250.0).abs() < 1e-9, "{area}");
    // Halfway along the first span: x = 10, y = 15/2 - 5/8.
    let q = curve(&sk, s).at(0, 0.5);
    assert!(q.distance(v2(10.0, 7.5 - 0.625)) < 1e-9, "{q:?}");
}

/// The builders refuse what makes no spline (sketch-entities.md, "Dimensions and errors").
#[test]
fn degenerate_splines_are_refused() {
    let mut sk = Sketch::new();
    let three = [v2(0.0, 0.0), v2(1.0, 1.0), v2(2.0, 0.0)];
    assert!(matches!(sk.add_control_spline(&three), Err(SketchError::OutOfRange(_))));
    assert!(matches!(sk.add_spline(&[v2(1.0, 1.0)]), Err(SketchError::OutOfRange(_))));
    let doubled = [v2(0.0, 0.0), v2(1.0, 1.0), v2(1.0, 1.0), v2(2.0, 0.0)];
    assert_eq!(sk.add_spline(&doubled), Err(SketchError::Degenerate));
    let closed = [v2(0.0, 0.0), v2(1.0, 1.0), v2(2.0, 0.0), v2(0.0, 0.0)];
    assert_eq!(sk.add_control_spline(&closed), Err(SketchError::Degenerate));
    assert_eq!(sk.entities.len(), 0);
    assert_eq!(sk.points.len(), 1, "nothing but the origin was added");
}

/// Tangent at a shared end between a fixed line and a control point spline: the spline's
/// second vertex comes onto the line (sketch-entities.md, "Handles, tangency and
/// curvature", Decision).
#[test]
fn control_spline_tangent_to_a_line() {
    let mut sk = Sketch::new();
    let s =
        sk.add_control_spline(&[v2(0.0, 0.0), v2(5.0, 10.0), v2(30.0, 10.0), v2(40.0, 0.0)]).expect("spline");
    let p = pts_of(&sk, s);
    let a = sk.add_point(v2(-20.0, -5.0));
    let line = sk.add_line(a, p[0]);
    fix_all(&mut sk, &[a, p[0], p[3]]);
    sk.add_relation(Constraint::Tangent { line, entity: s });
    let r = sk.solve();
    assert!(r.converged, "{r:?}");
    let dir = (sk.pos(p[0]) - sk.pos(a)).normalized();
    let t = curve(&sk, s).end_tangent(false);
    assert!(dir.cross(t).abs() < 1e-8 && dir.dot(t) > 0.0, "{dir:?} {t:?}");
    // The second vertex is on the line through the first two points.
    assert!(dir.cross(sk.pos(p[1]) - sk.pos(p[0])).abs() < 1e-7);
}

/// Tangency at the end of a spline through points gives that end a handle; with the
/// through points fixed only the handle turns.
#[test]
fn through_spline_tangency_turns_a_handle() {
    let mut sk = Sketch::new();
    let s = sk.add_spline(&[v2(0.0, 0.0), v2(20.0, 15.0), v2(40.0, 5.0)]).expect("spline");
    let p = pts_of(&sk, s);
    let a = sk.add_point(v2(-30.0, 0.0));
    let line = sk.add_line(a, p[0]);
    fix_all(&mut sk, &[a, p[0], p[1], p[2]]);
    let before: Vec<Vec2> = p.iter().map(|&q| sk.pos(q)).collect();
    sk.add_relation(Constraint::Tangent { line, entity: s });
    let Geom::Spline { handles, .. } = &sk.entities[s].geom else { panic!("not a spline") };
    let h = handles[0].expect("a handle at the start");
    assert!(handles[1].is_none());
    // The handle starts where the natural end's control point was: nothing moved yet.
    let r = sk.solve();
    assert!(r.converged, "{r:?}");
    for (q, b) in p.iter().zip(&before) {
        assert!(sk.pos(*q).distance(*b) < 1e-9, "through points stay put");
    }
    // The handle came down onto the line's direction, ahead of the start.
    assert!(sk.pos(h).y.abs() < 1e-8 && sk.pos(h).x > 0.0, "{:?}", sk.pos(h));
    let t = curve(&sk, s).end_tangent(false);
    assert!(t.y.abs() < 1e-8 && t.x > 0.0, "{t:?}");
    // The handle's distance is left free: one freedom (2 handle coordinates, less the
    // tangency).
    assert_eq!(r.dof, 1, "{r:?}");
}

/// Equal curvature between two control point splines sharing an end: tangent there, and
/// bending alike.
#[test]
fn equal_curvature_between_splines() {
    let mut sk = Sketch::new();
    let s1 =
        sk.add_control_spline(&[v2(0.0, 0.0), v2(10.0, 0.0), v2(20.0, 10.0), v2(30.0, 10.0)]).expect("first");
    let p1 = pts_of(&sk, s1);
    let s2 = sk
        .add_control_spline(&[v2(30.0, 10.0), v2(38.0, 12.0), v2(45.0, 5.0), v2(50.0, 0.0)])
        .expect("second");
    let p2 = pts_of(&sk, s2);
    sk.merge_points(p1[3], p2[0]).expect("share the end");
    let p2 = pts_of(&sk, s2);
    let p1 = pts_of(&sk, s1);
    fix_all(&mut sk, &p1);
    fix_all(&mut sk, &[p2[3]]);
    sk.add_relation(Constraint::EqualCurvature { a: s1, b: s2 });
    let r = sk.solve();
    assert!(r.converged, "{r:?}");
    let (c1, c2) = (curve(&sk, s1), curve(&sk, s2));
    let (t1, t2) = (c1.end_tangent(true), c2.end_tangent(false));
    assert!(t1.cross(t2).abs() < 1e-8 && t1.dot(t2) > 0.0, "{t1:?} {t2:?}");
    let (k1, k2) = (c1.end_curvature(true), c2.end_curvature(false));
    assert!(k1.distance(k2) < 1e-9, "{k1:?} {k2:?}");
    // The first spline bends at its end with curvature 2/3 |d1 x d2| / |d1|^3 of its last
    // three vertices: (10, 0) -> 2/3 * 100 / 1000.
    assert!((k1.length() - 2.0 / 30.0).abs() < 1e-9, "{k1:?}");
}

/// A point held on a spline lands on the curve, and a length dimension drives the
/// spline's length.
#[test]
fn point_on_spline_and_spline_length() {
    let mut sk = Sketch::new();
    let s =
        sk.add_control_spline(&[v2(0.0, 0.0), v2(0.0, 30.0), v2(40.0, 30.0), v2(40.0, 0.0)]).expect("spline");
    let p = pts_of(&sk, s);
    fix_all(&mut sk, &[p[0], p[3]]);
    let q = sk.add_point(v2(20.0, 30.0));
    sk.add_constraint(Constraint::PointOnCircle { p: q, entity: s });
    let r = sk.solve();
    assert!(r.converged, "{r:?}");
    assert!(curve(&sk, s).distance(sk.pos(q)) < 1e-8);

    // The measured length of the arch, then 10 more.
    let len = sk.measure(&Constraint::ArcLength { entity: s, value: 0.0 }).expect("length");
    let polyline: f64 = curve(&sk, s).polyline(2000).windows(2).map(|w| w[0].distance(w[1])).sum();
    assert!((len - polyline).abs() < 1e-4, "{len} vs {polyline}");
    sk.add_constraint(Constraint::ArcLength { entity: s, value: len + 10.0 });
    let r = sk.solve();
    assert!(r.converged, "{r:?}");
    assert!((curve(&sk, s).length() - (len + 10.0)).abs() < 1e-7);
}

/// A tangent arc can start from a spline's end, which then has a handle.
#[test]
fn tangent_arc_from_a_spline() {
    let mut sk = Sketch::new();
    let s = sk.add_spline(&[v2(0.0, 0.0), v2(20.0, 10.0), v2(40.0, 0.0)]).expect("spline");
    let p = pts_of(&sk, s);
    let arc = sk
        .add_tangent_arc(s, p[2], v2(50.0, -20.0), anvil_sketch::entities::TangentZone::Forward)
        .expect("arc");
    let r = sk.solve();
    assert!(r.converged, "{r:?}");
    let t = curve(&sk, s).end_tangent(true);
    let c = sk.pos(arc.center);
    assert!(t.dot(sk.pos(p[2]) - c).abs() < 1e-7, "the radius is square to the spline's end");
}

/// Splines survive a save and reload, handles included.
#[test]
fn splines_round_trip_through_json() {
    let mut sk = Sketch::new();
    let s = sk.add_spline(&[v2(0.0, 0.0), v2(20.0, 10.0), v2(40.0, 0.0)]).expect("spline");
    let p = pts_of(&sk, s);
    sk.ensure_spline_handle(s, p[0]).expect("handle");
    sk.add_control_spline(&[v2(0.0, 0.0), v2(5.0, 5.0), v2(10.0, 5.0), v2(15.0, 0.0)]).expect("spline");
    let json = serde_json::to_string(&sk).expect("save");
    let back: Sketch = serde_json::from_str(&json).expect("load");
    assert_eq!(back, sk);
}

/// An open spline leaves two loose ends; closed by a line it leaves none.
#[test]
fn spline_ends_count_as_open_ends() {
    let mut sk = Sketch::new();
    let s = sk.add_spline(&[v2(5.0, 0.0), v2(20.0, 10.0), v2(40.0, 0.0)]).expect("spline");
    let p = pts_of(&sk, s);
    assert_eq!(sk.open_ends(), vec![p[0], p[2]]);
    sk.add_line(p[2], p[0]);
    assert!(sk.open_ends().is_empty());
    // A whole ellipse has no ends to leave open.
    sk.add_ellipse_sized(v2(100.0, 0.0), 10.0, 5.0, 0.0).expect("ellipse");
    assert!(sk.open_ends().is_empty());
}
