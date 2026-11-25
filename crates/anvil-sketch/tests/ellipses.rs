//! Ellipse and partial ellipse entities, their relations and dimensions
//! (`docs/re/sketch-entities.md`, "Ellipses").

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use anvil_kernel::{v2, Vec2};
use anvil_sketch::dimension::{infer, Pick};
use anvil_sketch::ellipse::{self, Ellipse2};
use anvil_sketch::transform::{Transform2, TransformOptions};
use anvil_sketch::{Constraint, Geom, Sketch, SketchError};
use std::f64::consts::PI;

fn near(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

fn curve(s: &Sketch, e: usize) -> Ellipse2 {
    ellipse::of(s, e).expect("an ellipse").0
}

/// Statement 28: three picks, 5 DOF, area 600π.
#[test]
fn ellipse_from_three_picks() {
    let mut s = Sketch::new();
    let ids = s.add_ellipse(v2(0.0, 0.0), v2(30.0, 0.0), v2(12.0, 20.0)).unwrap();
    assert!(s.pos(ids.axis2).distance(v2(0.0, 20.0)) < 1e-12);
    let r = s.solve();
    assert!(r.converged && r.dof == 5, "{r:?}");
    let el = curve(&s, ids.ellipse);
    assert!(near(el.r1, 30.0, 1e-12) && near(el.r2, 20.0, 1e-12));
    let p = s.profiles().unwrap();
    assert_eq!(p.len(), 1);
    assert!(near(p[0].area(), 600.0 * PI, 1e-9), "{}", p[0].area());
    // A third pick on the first axis, or a first axis of no length, makes nothing.
    assert_eq!(
        s.add_ellipse(v2(0.0, 0.0), v2(30.0, 0.0), v2(10.0, 0.0)).unwrap_err(),
        SketchError::Degenerate
    );
    assert_eq!(s.add_ellipse(v2(1.0, 1.0), v2(1.0, 1.0), v2(5.0, 5.0)).unwrap_err(), SketchError::Degenerate);
}

/// The solver keeps the second axis square to the first when the first is dragged.
#[test]
fn axes_stay_square() {
    let mut s = Sketch::new();
    let ids = s.add_ellipse(v2(0.0, 0.0), v2(30.0, 0.0), v2(0.0, 20.0)).unwrap();
    s.add_constraint(Constraint::Fix { p: ids.center, x: 0.0, y: 0.0 });
    let r = s.drag_point(ids.axis1, v2(20.0, 20.0));
    assert!(r.converged, "{r:?}");
    let (c, a1, a2) = (s.pos(ids.center), s.pos(ids.axis1), s.pos(ids.axis2));
    assert!((a1 - c).dot(a2 - c).abs() < 1e-7);
    assert!(a1.distance(v2(20.0, 20.0)) < 1e-6);
}

/// Statement 31: a partial ellipse closed by a line; 7 DOF for the partial ellipse alone.
#[test]
fn partial_ellipse_and_its_area() {
    let mut s = Sketch::new();
    // Picks: centre, first axis end, a third pick at 60° (setting radius 20 and the start),
    // and an end at 180°, swept counter-clockwise.
    let third = v2(10.0, 20.0);
    let ids = s.add_partial_ellipse(v2(0.0, 0.0), v2(30.0, 0.0), third, v2(-5.0, 0.0), 1.0).unwrap();
    let r = s.solve();
    assert!(r.converged && r.dof == 7, "{r:?}");
    let (a, b) = ids.ends.unwrap();
    let el = curve(&s, ids.ellipse);
    assert!(el.residual(s.pos(a)).abs() < 1e-9 && el.residual(s.pos(b)).abs() < 1e-9);
    assert!(s.pos(b).distance(v2(-30.0, 0.0)) < 1e-9);
    // The start lies on the ray from the centre through the third pick.
    assert!((s.pos(a) - v2(0.0, 0.0)).normalized().distance(third.normalized()) < 1e-12);

    // The same with the start on the axis: a half ellipse, 300π.
    let mut s = Sketch::new();
    let ids = s.add_elliptic_arc(v2(0.0, 0.0), 30.0, 20.0, 0.0, v2(1.0, 0.0), v2(-1.0, 0.0)).unwrap();
    let (a, b) = ids.ends.unwrap();
    s.add_line(b, a);
    let p = s.profiles().unwrap();
    assert!(near(p[0].area(), 300.0 * PI, 1e-9), "{}", p[0].area());
    // Swept clockwise, the picks give the other half.
    let mut s = Sketch::new();
    let ids =
        s.add_partial_ellipse(v2(0.0, 0.0), v2(30.0, 0.0), v2(30.0, 20.0), v2(-1.0, 0.0), -1.0).unwrap();
    let (a, b) = ids.ends.unwrap();
    assert!(s.pos(a).distance(v2(-30.0, 0.0)) < 1e-9, "stored counter-clockwise from the end");
    let el = curve(&s, ids.ellipse);
    let (_, t0, sweep) = ellipse::of(&s, ids.ellipse).unwrap();
    assert!(el.at(t0 + 0.5 * sweep).y < 0.0, "the lower half");
    assert!(s.pos(b).y > 0.0);
}

/// Statement 32: a point held on an ellipse stays on it when dragged away.
#[test]
fn point_on_ellipse() {
    let mut s = Sketch::new();
    let ids = s.add_ellipse_sized(v2(0.0, 0.0), 30.0, 20.0, 0.0).unwrap();
    for p in [ids.center, ids.axis1, ids.axis2] {
        let q = s.pos(p);
        s.add_constraint(Constraint::Fix { p, x: q.x, y: q.y });
    }
    let p = s.add_point(v2(0.0, 21.0));
    s.add_constraint(Constraint::PointOnCircle { p, entity: ids.ellipse });
    assert!(s.solve().converged);
    let r = s.drag_point(p, v2(40.0, 40.0));
    assert!(r.converged, "{r:?}");
    let el = curve(&s, ids.ellipse);
    assert!(el.residual(s.pos(p)).abs() < 1e-7);
    assert!(s.pos(p).x > 0.0 && s.pos(p).y > 0.0);
    assert_eq!(r.dof, 1, "the point slides round the curve");
}

/// Statement 33: lines tangent to an ellipse away from its ends sit at its support distance.
#[test]
fn line_tangent_to_an_ellipse() {
    let mut s = Sketch::new();
    let ids = s.add_ellipse_sized(v2(0.0, 0.0), 30.0, 20.0, 0.0).unwrap();
    for p in [ids.center, ids.axis1, ids.axis2] {
        let q = s.pos(p);
        s.add_constraint(Constraint::Fix { p, x: q.x, y: q.y });
    }
    let (a, b) = (s.add_point(v2(-40.0, 23.0)), s.add_point(v2(40.0, 23.0)));
    let h = s.add_line(a, b);
    s.add_constraint(Constraint::Horizontal { line: h });
    s.add_constraint(Constraint::Tangent { line: h, entity: ids.ellipse });
    let (c, d) = (s.add_point(v2(-33.0, -30.0)), s.add_point(v2(-33.0, 30.0)));
    let v = s.add_line(c, d);
    s.add_constraint(Constraint::Vertical { line: v });
    s.add_constraint(Constraint::Tangent { line: v, entity: ids.ellipse });
    // And a slanted one: tangent with normal n at distance √(30² n.x² + 20² n.y²).
    let (e, f) = (s.add_point(v2(0.0, -40.0)), s.add_point(v2(40.0, 0.0)));
    let k = s.add_line(e, f);
    s.add_constraint(Constraint::Angle { a: h, b: k, degrees: 45.0 });
    s.add_constraint(Constraint::Tangent { line: k, entity: ids.ellipse });
    let r = s.solve();
    assert!(r.converged, "{r:?}");
    assert!(near(s.pos(a).y, 20.0, 1e-7) && near(s.pos(b).y, 20.0, 1e-7));
    assert!(near(s.pos(c).x, -30.0, 1e-7));
    let n = (s.pos(f) - s.pos(e)).normalized().perp();
    let dist = n.dot(s.pos(e)).abs();
    assert!(near(dist, (450.0f64 + 200.0).sqrt(), 1e-6), "{dist}");
}

/// A line drawn from the end of a partial ellipse and made tangent there leaves along the
/// curve; an arc made tangent at the other end shares its normal.
#[test]
fn tangency_at_shared_ends() {
    let mut s = Sketch::new();
    let ids = s.add_elliptic_arc(v2(0.0, 0.0), 30.0, 20.0, 0.0, v2(1.0, 0.0), v2(0.0, 1.0)).unwrap();
    for p in [ids.center, ids.axis1, ids.axis2] {
        let q = s.pos(p);
        s.add_constraint(Constraint::Fix { p, x: q.x, y: q.y });
    }
    let (a, b) = ids.ends.unwrap();
    // Move the ends off the quadrants so the tangents are not axis-aligned.
    s.drag_point(a, v2(25.0, 12.0));
    s.drag_point(b, v2(-10.0, 19.0));
    let q = s.add_point(s.pos(b) + v2(-20.0, 5.0));
    let l = s.add_line(b, q);
    s.add_constraint(Constraint::Tangent { line: l, entity: ids.ellipse });
    let arc = s.add_tangent_arc(
        ids.ellipse,
        a,
        s.pos(a) + v2(5.0, -20.0),
        anvil_sketch::entities::TangentZone::Back,
    );
    let arc = arc.unwrap();
    let r = s.solve();
    assert!(r.converged, "{r:?}");
    let el = curve(&s, ids.ellipse);
    let line_dir = (s.pos(q) - s.pos(b)).normalized();
    assert!(line_dir.dot(el.normal_at(s.pos(b)).normalized()).abs() < 1e-7);
    let (ca, pa) = (s.pos(arc.center), s.pos(a));
    assert!((ca - pa).normalized().cross(el.normal_at(pa).normalized()).abs() < 1e-7);
    // A normal arc from an ellipse is refused, plainly.
    let e = s.add_tangent_arc(
        ids.ellipse,
        b,
        s.pos(b) + v2(0.0, 10.0),
        anvil_sketch::entities::TangentZone::Left,
    );
    assert!(matches!(e, Err(SketchError::Unsupported(_))), "{e:?}");
}

/// Statement 30: moving, rotating, scaling and mirroring keep an ellipse an ellipse.
#[test]
fn transforms_keep_the_shape() {
    let mut s = Sketch::new();
    let ids = s.add_ellipse_sized(v2(5.0, 5.0), 20.0, 10.0, PI / 4.0).unwrap();
    let opts = TransformOptions { copy: false, keep_relations: true, copies: 1 };
    s.transform_entities(&[ids.ellipse], Transform2::Rotate { center: v2(0.0, 0.0), degrees: 57.0 }, opts)
        .unwrap();
    s.transform_entities(&[ids.ellipse], Transform2::Scale { about: v2(0.0, 0.0), factor: 2.0 }, opts)
        .unwrap();
    let el = curve(&s, 0);
    assert!(near(el.r1, 40.0, 1e-9) && near(el.r2, 20.0, 1e-9));
    assert!(near(s.profiles().unwrap()[0].area(), 800.0 * PI, 1e-6));

    // A mirrored partial ellipse runs counter-clockwise and keeps its area.
    let mut s = Sketch::new();
    let ids = s.add_elliptic_arc(v2(10.0, 0.0), 6.0, 3.0, 0.3, v2(1.0, 0.0), v2(-1.0, 0.2)).unwrap();
    let (a, b) = ids.ends.unwrap();
    let chord = s.add_line(b, a);
    let area = s.profiles().unwrap()[0].area();
    let (m0, m1) = (s.add_point(v2(0.0, -5.0)), s.add_point(v2(0.0, 5.0)));
    let axis = s.add_line(m0, m1);
    s.entities[axis].construction = true;
    let (mirrored, _) = s.mirror_entities(&[ids.ellipse, chord], axis, true).unwrap();
    assert!(matches!(s.entities[mirrored[0]].geom, Geom::EllipseArc { .. }));
    assert!(s.solve().converged);
    let profiles = s.profiles().unwrap();
    assert_eq!(profiles.len(), 2);
    for p in &profiles {
        assert!(near(p.area(), area, 1e-9), "{} vs {area}", p.area());
    }
    let (_, _, sweep) = ellipse::of(&s, mirrored[0]).unwrap();
    let (_, _, sweep0) = ellipse::of(&s, ids.ellipse).unwrap();
    assert!(near(sweep, sweep0, 1e-9));
}

/// The Dimension tool on an ellipse dimensions the semi-axis nearer the pick.
#[test]
fn dimensioning_an_ellipse() {
    let mut s = Sketch::new();
    let ids = s.add_ellipse_sized(v2(0.0, 0.0), 30.0, 20.0, 0.0).unwrap();
    let c = infer(&s, &[Pick::Entity(ids.ellipse)], v2(28.0, 5.0)).unwrap();
    assert_eq!(c, Constraint::Distance { p: ids.center, q: ids.axis1, value: 30.0 });
    let c = infer(&s, &[Pick::Entity(ids.ellipse)], v2(3.0, 19.0)).unwrap();
    assert_eq!(c, Constraint::Distance { p: ids.center, q: ids.axis2, value: 20.0 });
    // Driving the second radius to 12, with the centre and first radius held, changes the
    // area to 360π.
    s.add_constraint(Constraint::Fix { p: ids.center, x: 0.0, y: 0.0 });
    s.add_constraint(Constraint::Distance { p: ids.center, q: ids.axis1, value: 30.0 });
    let mut c = c;
    c.set_value(12.0);
    s.add_constraint(c);
    assert!(s.solve().converged);
    assert!(near(s.profiles().unwrap()[0].area(), 360.0 * PI, 1e-6));
}

/// Tools that need curve intersections or offsets refuse an ellipse in plain words.
#[test]
fn editing_tools_refuse_ellipses() {
    let mut s = Sketch::new();
    let ids = s.add_ellipse_sized(v2(0.0, 0.0), 30.0, 20.0, 0.0).unwrap();
    let e = s.trim_at(ids.ellipse, v2(30.0, 0.0), Default::default()).unwrap_err();
    assert_eq!(e.to_string(), "this tool does not work on ellipses yet");
    let e = s.offset_entities(&[ids.ellipse], Default::default()).unwrap_err();
    assert_eq!(e.to_string(), "ellipses cannot be offset yet");
}

/// Sketches with ellipses save and load.
#[test]
fn ellipses_round_trip_through_json() {
    let mut s = Sketch::new();
    s.add_ellipse_sized(v2(1.0, 2.0), 3.0, 4.0, 0.5).unwrap();
    s.add_elliptic_arc(v2(1.0, 2.0), 3.0, 4.0, 0.5, v2(1.0, 0.0), v2(0.0, 1.0)).unwrap();
    let json = serde_json::to_string(&s).unwrap();
    let back: Sketch = serde_json::from_str(&json).unwrap();
    assert_eq!(back, s);
}

/// Picking and hit testing find the curve, not just its points.
#[test]
fn picking_an_ellipse() {
    let mut s = Sketch::new();
    let ids = s.add_ellipse_sized(v2(0.0, 0.0), 30.0, 20.0, 0.0).unwrap();
    let on = Vec2::from_angle(1.0);
    let q = curve(&s, ids.ellipse).at(1.0) + on * 0.3;
    assert_eq!(s.pick_entity(q, 0.5), Some(ids.ellipse));
    assert!(near(s.distance_to_entity(ids.ellipse, v2(0.0, 25.0)), 5.0, 1e-9));
}
