//! The testable statements of `docs/re/sketch-entities.md` (numbered as there) that the
//! sketch crate can check on its own. Lengths in mm, angles in degrees.

use anvil_kernel::{v2, Vec2};
use anvil_sketch::entities::{
    PolygonCircle, RectConstruction, RectOptions, SlotLength, SlotOptions, TangentZone, POLYGON_SIDES,
};
use anvil_sketch::{Constraint, EntityId, Geom, PointId, Sketch, SketchError, ORIGIN};
use std::f64::consts::PI;

fn near(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

fn close(a: Vec2, b: Vec2) -> bool {
    a.distance(b) < 1e-6
}

fn area(s: &Sketch) -> f64 {
    let p = s.profiles().expect("closed profile");
    assert_eq!(p.len(), 1);
    p[0].area()
}

fn ends(s: &Sketch, e: EntityId) -> (Vec2, Vec2) {
    match s.entities[e].geom {
        Geom::Line { a, b } | Geom::Arc { a, b, .. } => (s.pos(a), s.pos(b)),
        Geom::Circle { .. } => panic!("a circle has no ends"),
    }
}

fn centre(s: &Sketch, e: EntityId) -> Vec2 {
    match s.entities[e].geom {
        Geom::Arc { c, .. } | Geom::Circle { c, .. } => s.pos(c),
        Geom::Line { .. } => panic!("a line has no centre"),
    }
}

fn sweep_deg(s: &Sketch, e: EntityId) -> f64 {
    let (a, b) = ends(s, e);
    let c = centre(s, e);
    anvil_sketch::sketch::arc_sweep((a - c).angle(), (b - c).angle()).to_degrees()
}

fn dof(s: &mut Sketch) -> usize {
    let r = s.solve();
    assert!(r.converged, "{r:?}");
    r.dof
}

fn corner_set(s: &Sketch, corners: &[PointId]) -> Vec<Vec2> {
    corners.iter().map(|&p| s.pos(p)).collect()
}

#[test]
fn s1_corner_rectangle() {
    let mut s = Sketch::new();
    let r = s.add_corner_rectangle(v2(0.0, 0.0), v2(20.0, 10.0), RectOptions::default()).unwrap();
    let c = corner_set(&s, &r.corners);
    assert_eq!(c, vec![v2(0.0, 0.0), v2(20.0, 0.0), v2(20.0, 10.0), v2(0.0, 10.0)]);
    assert!(near(area(&s), 200.0, 1e-9));
    assert_eq!(dof(&mut s), 4);
}

#[test]
fn s2_center_rectangle_moves_with_its_centre() {
    let mut s = Sketch::new();
    let r = s.add_center_rectangle(v2(0.0, 0.0), v2(15.0, 10.0), RectOptions::default()).unwrap();
    for (p, q) in corner_set(&s, &r.corners).into_iter().zip([
        v2(-15.0, -10.0),
        v2(15.0, -10.0),
        v2(15.0, 10.0),
        v2(-15.0, 10.0),
    ]) {
        assert!(close(p, q), "{p:?} {q:?}");
    }
    assert!(near(area(&s), 600.0, 1e-9), "diagonals are not profile geometry");
    assert_eq!(r.construction.len(), 2);
    for &d in &r.construction {
        assert!(s.entities[d].construction);
        assert!(near(s.line_length(d), 36.0555, 1e-4));
    }
    assert_eq!(dof(&mut s), 4);
    // Hold the size and drag the centre: every corner moves by the same amount.
    s.add_constraint(Constraint::Length { line: r.sides[0], value: 30.0 });
    s.add_constraint(Constraint::Length { line: r.sides[1], value: 20.0 });
    let m = r.center.unwrap();
    assert!(s.drag_point(m, v2(5.0, 5.0)).converged);
    for (p, q) in corner_set(&s, &r.corners).into_iter().zip([
        v2(-10.0, -5.0),
        v2(20.0, -5.0),
        v2(20.0, 15.0),
        v2(-10.0, 15.0),
    ]) {
        assert!(close(p, q), "{p:?} {q:?}");
    }
}

#[test]
fn s3_three_point_center_rectangle() {
    let mut s = Sketch::new();
    // (6, 8) is 10 from the centre along (0.6, 0.8); the third pick is 5 off that line.
    let third = v2(6.0, 8.0) + v2(-0.8, 0.6) * 5.0;
    let r = s.add_3pt_center_rectangle(v2(0.0, 0.0), v2(6.0, 8.0), third, RectOptions::default()).unwrap();
    let c = corner_set(&s, &r.corners);
    for q in [v2(2.0, 11.0), v2(10.0, 5.0), v2(-2.0, -11.0), v2(-10.0, -5.0)] {
        assert!(c.iter().any(|p| close(*p, q)), "{q:?} missing from {c:?}");
    }
    assert!(near(s.line_length(r.sides[0]), 20.0, 1e-9));
    assert!(near(s.line_length(r.sides[1]), 10.0, 1e-9));
    assert!(near(area(&s), 200.0, 1e-9));
    assert_eq!(dof(&mut s), 5);
}

#[test]
fn s4_three_point_corner_rectangle() {
    let mut s = Sketch::new();
    let third = v2(30.0, 40.0) + v2(-0.8, 0.6) * 20.0;
    let r = s.add_3pt_corner_rectangle(v2(0.0, 0.0), v2(30.0, 40.0), third, RectOptions::default()).unwrap();
    assert!(near(s.line_length(r.sides[0]), 50.0, 1e-9));
    assert!(near(s.line_length(r.sides[1]), 20.0, 1e-9));
    assert!(near(area(&s), 1000.0, 1e-9));
    assert_eq!(dof(&mut s), 5);
}

#[test]
fn s5_parallelogram() {
    let mut s = Sketch::new();
    let r = s.add_parallelogram(v2(0.0, 0.0), v2(20.0, 0.0), v2(25.0, 10.0), RectOptions::default()).unwrap();
    assert!(close(s.pos(r.corners[3]), v2(5.0, 10.0)));
    assert!(near(area(&s), 200.0, 1e-9));
    assert_eq!(dof(&mut s), 6);
    assert_eq!(
        s.add_parallelogram(v2(0.0, 0.0), v2(20.0, 0.0), v2(30.0, 0.0), RectOptions::default()),
        Err(SketchError::Degenerate)
    );
}

#[test]
fn s6_shift_makes_a_square() {
    let mut s = Sketch::new();
    let opts = RectOptions { equal_sides: true, ..Default::default() };
    let r = s.add_corner_rectangle(v2(0.0, 0.0), v2(10.0, 7.0), opts).unwrap();
    assert!(s.solve().converged);
    assert!(near(s.line_length(r.sides[0]), s.line_length(r.sides[1]), 1e-7));
    assert_eq!(dof(&mut s), 3);
}

#[test]
fn rectangle_midpoint_construction_adds_no_freedom() {
    let mut s = Sketch::new();
    let opts = RectOptions { construction: RectConstruction::Midpoints, ..Default::default() };
    let r = s.add_center_rectangle(v2(0.0, 0.0), v2(15.0, 10.0), opts).unwrap();
    assert_eq!(r.construction.len(), 2);
    assert_eq!(dof(&mut s), 4);
    assert!(near(area(&s), 600.0, 1e-9));
}

#[test]
fn zero_size_rectangles_are_refused() {
    let mut s = Sketch::new();
    let before = s.clone();
    assert_eq!(
        s.add_center_rectangle(v2(0.0, 0.0), v2(15.0, 0.0), RectOptions::default()),
        Err(SketchError::Degenerate)
    );
    assert_eq!(
        s.add_3pt_corner_rectangle(v2(0.0, 0.0), v2(10.0, 0.0), v2(20.0, 0.0), RectOptions::default()),
        Err(SketchError::Degenerate)
    );
    assert_eq!(s, before);
}

fn slot_30x10(opts: SlotOptions) -> (Sketch, anvil_sketch::entities::SlotIds) {
    let mut s = Sketch::new();
    let slot = s.add_straight_slot(v2(0.0, 0.0), v2(30.0, 0.0), 10.0, opts).unwrap();
    (s, slot)
}

#[test]
fn s7_straight_slot() {
    let (mut s, slot) = slot_30x10(SlotOptions::default());
    for a in slot.ends {
        assert!(near(s.radius(a), 5.0, 1e-12));
    }
    let lines: Vec<(Vec2, Vec2)> = slot.sides.iter().map(|&l| ends(&s, l)).collect();
    assert!(lines.contains(&(v2(0.0, -5.0), v2(30.0, -5.0))));
    assert!(lines.contains(&(v2(30.0, 5.0), v2(0.0, 5.0))));
    let a = area(&s);
    assert!(near(a, 378.5398, 1e-4), "{a}");
    // Extruded 5 it has volume 1892.699.
    assert!(near(a * 5.0, 1892.699, 1e-3));
    let perimeter = 2.0 * 30.0 + PI * 10.0;
    assert!(near(perimeter, 91.416, 1e-3));
    let loops = s.loops().unwrap();
    let mut per = 0.0;
    for seg in &loops[0].segs {
        per += match *seg {
            anvil_kernel::profile::Seg2::Line { a, b, .. } => a.distance(b),
            anvil_kernel::profile::Seg2::Arc { radius, sweep, .. } => radius * sweep.abs(),
        };
    }
    assert!(near(per, 91.4159, 1e-4), "{per}");
    // Overall length 40.
    assert!(near(s.line_length(slot.centerline) + 2.0 * s.radius(slot.ends[0]), 40.0, 1e-12));
    assert_eq!(dof(&mut s), 5);
}

#[test]
fn s8_changing_the_slot_width() {
    let (mut s, slot) = slot_30x10(SlotOptions { dimensions: true, ..Default::default() });
    let width = slot.dimensions[1];
    assert!(matches!(s.constraints[width], Constraint::Diameter { .. }));
    let (c1, c2) = (centre(&s, slot.ends[0]), centre(&s, slot.ends[1]));
    s.constraints[width].set_value(20.0);
    assert!(s.solve().converged);
    assert!(close(centre(&s, slot.ends[0]), c1) && close(centre(&s, slot.ends[1]), c2));
    let a = area(&s);
    assert!(near(a, 30.0 * 20.0 + PI * 100.0, 1e-4), "{a}");
    assert!(near(a, 914.159, 1e-3));
}

#[test]
fn s9_centerpoint_straight_slot() {
    let mut s = Sketch::new();
    let slot = s.add_center_straight_slot(v2(0.0, 0.0), v2(15.0, 0.0), 6.0, SlotOptions::default()).unwrap();
    assert!(close(centre(&s, slot.ends[0]), v2(-15.0, 0.0)));
    assert!(close(centre(&s, slot.ends[1]), v2(15.0, 0.0)));
    assert!(s.constraints.contains(&Constraint::Midpoint { p: slot.center, line: slot.centerline }));
    assert!(close(s.pos(slot.center), v2(0.0, 0.0)));
}

#[test]
fn s10_overall_length() {
    let mut s = Sketch::new();
    let opts = SlotOptions { length: SlotLength::Overall, dimensions: true };
    let slot = s.add_straight_slot(v2(0.0, 0.0), v2(25.0, 0.0), 8.0, opts).unwrap();
    let (len, width) = (slot.dimensions[0], slot.dimensions[1]);
    assert!(matches!(s.constraints[len], Constraint::Distance { value, .. } if near(value, 33.0, 1e-12)));
    s.constraints[len].set_value(40.0);
    s.constraints[width].set_value(10.0);
    assert!(s.solve().converged);
    assert!(near(s.line_length(slot.centerline), 30.0, 1e-7));
    assert!(near(s.radius(slot.ends[0]), 5.0, 1e-7));
}

#[test]
fn s11_centerpoint_arc_slot() {
    let mut s = Sketch::new();
    let slot = s
        .add_centerpoint_arc_slot(
            v2(0.0, 0.0),
            v2(20.0, 0.0),
            v2(0.0, 7.0),
            0.5 * PI,
            6.0,
            SlotOptions::default(),
        )
        .unwrap();
    assert!(near(s.radius(slot.sides[0]), 23.0, 1e-12));
    assert!(near(s.radius(slot.sides[1]), 17.0, 1e-12));
    for e in slot.ends {
        assert!(near(s.radius(e), 3.0, 1e-12));
    }
    let a = area(&s);
    assert!(near(a, 69.0 * PI, 1e-6) && near(a, 216.770, 1e-3), "{a}");
    assert_eq!(dof(&mut s), 6);
    // The other way round: three quarters of a turn.
    let mut s = Sketch::new();
    let slot = s
        .add_centerpoint_arc_slot(
            v2(0.0, 0.0),
            v2(20.0, 0.0),
            v2(0.0, 7.0),
            -1.5 * PI,
            6.0,
            SlotOptions::default(),
        )
        .unwrap();
    assert!(near(sweep_deg(&s, slot.centerline), 270.0, 1e-9));
}

#[test]
fn s12_three_point_arc_slot() {
    let mut s = Sketch::new();
    let slot = s
        .add_3pt_arc_slot(v2(-20.0, 0.0), v2(20.0, 0.0), v2(0.0, 20.0), 4.0, SlotOptions::default())
        .unwrap();
    assert!(close(centre(&s, slot.centerline), v2(0.0, 0.0)));
    assert!(near(s.radius(slot.centerline), 20.0, 1e-12));
    assert!(near(sweep_deg(&s, slot.centerline), 180.0, 1e-9));
    let a = area(&s);
    assert!(near(a, 84.0 * PI, 1e-6) && near(a, 263.894, 1e-3), "{a}");
}

#[test]
fn s13_arc_slot_wider_than_its_diameter() {
    let mut s = Sketch::new();
    let r = s.add_centerpoint_arc_slot(
        v2(0.0, 0.0),
        v2(5.0, 0.0),
        v2(0.0, 5.0),
        1.0,
        10.0,
        SlotOptions::default(),
    );
    assert_eq!(r, Err(SketchError::Degenerate));
    let r = s.add_centerpoint_arc_slot(
        v2(0.0, 0.0),
        v2(5.0, 0.0),
        v2(0.0, 5.0),
        1.0,
        12.0,
        SlotOptions::default(),
    );
    assert_eq!(r, Err(SketchError::Degenerate));
    assert_eq!(s, Sketch::new());
}

#[test]
fn arc_slot_dimensions_drive_it() {
    let mut s = Sketch::new();
    let opts = SlotOptions { dimensions: true, ..Default::default() };
    let slot = s.add_centerpoint_arc_slot(v2(0.0, 0.0), v2(20.0, 0.0), v2(0.0, 7.0), 1.0, 6.0, opts).unwrap();
    assert_eq!(slot.dimensions.len(), 3);
    s.add_constraint(Constraint::Coincident { p: slot.center, q: ORIGIN });
    s.constraints[slot.dimensions[1]].set_value(60.0);
    s.constraints[slot.dimensions[2]].set_value(4.0);
    let r = s.solve();
    assert!(r.converged, "{r:?}");
    // Centre (2), radius, angle and width are held; the start angle is free.
    assert_eq!(r.dof, 1);
    assert!(near(sweep_deg(&s, slot.centerline), 60.0, 1e-6));
    assert!(near(area(&s), (PI / 3.0) * 20.0 * 4.0 + 4.0 * PI, 1e-6));
}

fn polygon_side(s: &Sketch, p: &anvil_sketch::entities::PolygonIds) -> f64 {
    s.line_length(p.sides[0])
}

#[test]
fn s14_to_s17_polygon_sizes() {
    // (sides, circle, diameter, side, area)
    let cases = [
        (6, PolygonCircle::Circumscribed, 20.0, 10.0, 259.808),
        (6, PolygonCircle::Inscribed, 20.0, 11.547, 346.410),
        (4, PolygonCircle::Inscribed, 20.0, 20.0, 400.0),
        (4, PolygonCircle::Circumscribed, 20.0, 14.142, 200.0),
        (3, PolygonCircle::Circumscribed, 20.0, 17.321, 129.904),
    ];
    for (n, kind, d, side, a) in cases {
        let mut s = Sketch::new();
        // Start from a different size and turn; the diameter dimension sets the size.
        let p = s.add_polygon(v2(1.0, 2.0), v2(9.0, 5.0), n, kind).unwrap();
        s.add_constraint(Constraint::Diameter { entity: p.circle, value: d });
        assert!(s.solve().converged);
        for &l in &p.sides {
            assert!(near(s.line_length(l), side, 1e-3), "{n} {kind:?}: {}", s.line_length(l));
        }
        assert!(near(area(&s), a, 1e-3), "{n} {kind:?}: {}", area(&s));
    }
}

#[test]
fn s18_polygon_freedoms_and_resize() {
    for n in POLYGON_SIDES {
        for kind in [PolygonCircle::Inscribed, PolygonCircle::Circumscribed] {
            let mut s = Sketch::new();
            s.add_polygon(v2(3.0, -2.0), v2(13.0, 1.0), n, kind).unwrap();
            assert_eq!(dof(&mut s), 4, "{n} sides, {kind:?}");
        }
    }
    let mut s = Sketch::new();
    assert!(matches!(
        s.add_polygon(v2(0.0, 0.0), v2(10.0, 0.0), 2, PolygonCircle::Inscribed),
        Err(SketchError::OutOfRange(_))
    ));
    assert!(matches!(
        s.add_polygon(v2(0.0, 0.0), v2(10.0, 0.0), 41, PolygonCircle::Inscribed),
        Err(SketchError::OutOfRange(_))
    ));
    let p = s.add_polygon(v2(0.0, 0.0), v2(10.0, 0.0), 6, PolygonCircle::Circumscribed).unwrap();
    let d = s.add_constraint(Constraint::Diameter { entity: p.circle, value: 20.0 });
    s.add_constraint(Constraint::Horizontal { line: p.sides[0] });
    s.add_constraint(Constraint::Coincident { p: p.center, q: ORIGIN });
    assert_eq!(dof(&mut s), 0, "a diameter, a horizontal side and the centre fully define it");
    s.constraints[d].set_value(30.0);
    assert!(s.solve().converged);
    assert!(near(polygon_side(&s, &p), 15.0, 1e-7));
}

#[test]
fn s19_centerpoint_arc_direction() {
    let mut s = Sketch::new();
    let a = s.add_centerpoint_arc(v2(0.0, 0.0), v2(10.0, 0.0), v2(0.0, 10.0), 0.5 * PI).unwrap();
    assert_eq!(ends(&s, a.arc), (v2(10.0, 0.0), v2(0.0, 10.0)));
    assert!(near(sweep_deg(&s, a.arc), 90.0, 1e-9));
    let b = s.add_centerpoint_arc(v2(0.0, 0.0), v2(10.0, 0.0), v2(0.0, 10.0), -1.5 * PI).unwrap();
    let (p, q) = ends(&s, b.arc);
    assert!(close(p, v2(0.0, 10.0)) && close(q, v2(10.0, 0.0)));
    assert!(near(sweep_deg(&s, b.arc), 270.0, 1e-9));
}

fn line_sketch() -> (Sketch, EntityId, PointId) {
    let mut s = Sketch::new();
    let a = s.add_point(v2(0.0, 0.0));
    let b = s.add_point(v2(10.0, 0.0));
    let l = s.add_line(a, b);
    (s, l, b)
}

#[test]
fn s20_tangent_arc_from_a_line() {
    let (mut s, l, b) = line_sketch();
    let t = s.leaving_direction(l, b).unwrap();
    assert_eq!(TangentZone::from_directions(t, v2(1.0, 0.2)), TangentZone::Forward);
    let arc = s.add_tangent_arc(l, b, v2(10.0, 10.0), TangentZone::Forward).unwrap();
    assert!(close(centre(&s, arc.arc), v2(10.0, 5.0)));
    assert!(near(s.radius(arc.arc), 5.0, 1e-12));
    assert!(near(sweep_deg(&s, arc.arc), 180.0, 1e-9));
    assert!(s.constraints.contains(&Constraint::Tangent { line: l, entity: arc.arc }));

    let (mut s, l, b) = line_sketch();
    let arc = s.add_tangent_arc(l, b, v2(15.0, 5.0), TangentZone::Forward).unwrap();
    assert!(close(centre(&s, arc.arc), v2(10.0, 5.0)));
    assert!(near(sweep_deg(&s, arc.arc), 90.0, 1e-9));
    let len = s.measure(&Constraint::ArcLength { entity: arc.arc, value: 0.0 }).unwrap();
    assert!(near(len, 7.854, 1e-3));
}

#[test]
fn s21_normal_arc_from_a_line() {
    let (mut s, l, b) = line_sketch();
    let t = s.leaving_direction(l, b).unwrap();
    assert_eq!(TangentZone::from_directions(t, v2(0.1, 1.0)), TangentZone::Left);
    let arc = s.add_tangent_arc(l, b, v2(15.0, 5.0), TangentZone::Left).unwrap();
    assert!(close(centre(&s, arc.arc), v2(15.0, 0.0)));
    assert!(near(s.radius(arc.arc), 5.0, 1e-12));
    assert!(s.constraints.contains(&Constraint::PointOnLine { p: arc.center, line: l }));
    assert!(s.solve().converged);
}

#[test]
fn s22_tangent_arc_stays_tangent_when_dragged() {
    let (mut s, l, b) = line_sketch();
    let arc = s.add_tangent_arc(l, b, v2(15.0, 5.0), TangentZone::Forward).unwrap();
    let free_end = if arc.start == b { arc.end } else { arc.start };
    assert!(s.drag_point(free_end, v2(18.0, 9.0)).converged);
    let c = centre(&s, arc.arc);
    let (p, q) = ends(&s, l);
    let dist = ((q - p).cross(c - p) / p.distance(q)).abs();
    assert!(near(dist, s.radius(arc.arc), 1e-7));
    assert!(close(s.pos(free_end), v2(18.0, 9.0)));
}

#[test]
fn tangent_arc_from_an_arc() {
    let mut s = Sketch::new();
    let first = s.add_centerpoint_arc(v2(0.0, 0.0), v2(10.0, 0.0), v2(0.0, 10.0), 1.0).unwrap();
    let arc = s.add_tangent_arc(first.arc, first.end, v2(-10.0, 20.0), TangentZone::Forward).unwrap();
    // Leaves (0, 10) heading in -x: a reverse curve centred at (0, 20).
    assert!(close(centre(&s, arc.arc), v2(0.0, 20.0)));
    assert!(s.constraints.iter().any(|c| matches!(c, Constraint::TangentCircles { inside: false, .. })));
    assert!(s.solve().converged);
}

#[test]
fn s23_three_point_arc() {
    let mut s = Sketch::new();
    let a = s.add_3pt_arc(v2(-10.0, 0.0), v2(10.0, 0.0), v2(0.0, 5.0)).unwrap();
    assert!(close(centre(&s, a.arc), v2(0.0, -7.5)));
    assert!(near(s.radius(a.arc), 12.5, 1e-12));
    assert!(near(sweep_deg(&s, a.arc), 106.260, 1e-3));
    let len = s.measure(&Constraint::ArcLength { entity: a.arc, value: 0.0 }).unwrap();
    assert!(near(len, 23.182, 1e-3), "{len}");
    let b = s.add_3pt_arc(v2(-10.0, 0.0), v2(10.0, 0.0), v2(0.0, 10.0)).unwrap();
    assert!(close(centre(&s, b.arc), v2(0.0, 0.0)));
    assert!(near(s.radius(b.arc), 10.0, 1e-12));
}

#[test]
fn s24_collinear_picks_make_nothing() {
    let mut s = Sketch::new();
    assert_eq!(s.add_3pt_arc(v2(0.0, 0.0), v2(10.0, 0.0), v2(5.0, 0.0)), Err(SketchError::Degenerate));
    assert_eq!(s.add_circle_3pt(v2(0.0, 0.0), v2(10.0, 0.0), v2(20.0, 0.0)), Err(SketchError::Degenerate));
    assert_eq!(s, Sketch::new());
}

#[test]
fn s25_perimeter_circle() {
    let mut s = Sketch::new();
    let (c, pc) = s.add_circle_3pt(v2(0.0, 0.0), v2(10.0, 0.0), v2(0.0, 10.0)).unwrap();
    assert!(close(s.pos(pc), v2(5.0, 5.0)));
    assert!(near(s.radius(c), 7.0711, 1e-4));
    let (c2, pc2) = s.add_circle_2pt(v2(0.0, 0.0), v2(10.0, 0.0)).unwrap();
    assert!(close(s.pos(pc2), v2(5.0, 0.0)) && near(s.radius(c2), 5.0, 1e-12));
}
