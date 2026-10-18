//! The testable statements of `docs/re/sketch-tools.md` for the editing tools (numbered
//! as there). Units are millimetres and degrees; "rectangle 40 × 20" has corners (0, 0),
//! (40, 0), (40, 20) and (0, 20).

use anvil_kernel::profile::Seg2;
use anvil_kernel::{v2, Vec2};
use anvil_sketch::tools::{ChamferKind, FilletOptions, TrimOptions};
use anvil_sketch::transform::{
    circular_pattern_center, CapEnds, CircularPattern, LinearPattern, OffsetOptions, Transform2,
    TransformOptions,
};
use anvil_sketch::{Constraint, EntityId, Geom, PointId, Sketch, SketchError, ORIGIN};
use std::f64::consts::PI;

fn near(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

fn close(a: Vec2, b: Vec2) -> bool {
    a.distance(b) < 1e-6
}

/// The 40 × 20 rectangle: bottom, right, top and left lines, and its corners from (0, 0)
/// counter-clockwise.
fn rect() -> (Sketch, [EntityId; 4], [PointId; 4]) {
    let mut s = Sketch::new();
    let l = s.add_rectangle(v2(0.0, 0.0), v2(40.0, 20.0));
    let c = l.map(|e| match s.entities[e].geom {
        Geom::Line { a, .. } => a,
        _ => unreachable!(),
    });
    (s, l, c)
}

fn line(s: &mut Sketch, a: Vec2, b: Vec2) -> EntityId {
    let (p, q) = (s.add_point(a), s.add_point(b));
    s.add_line(p, q)
}

fn ends(s: &Sketch, e: EntityId) -> (Vec2, Vec2) {
    match s.entities[e].geom {
        Geom::Line { a, b } | Geom::Arc { a, b, .. } => (s.pos(a), s.pos(b)),
        Geom::Circle { .. } => panic!("no ends"),
    }
}

fn end_ids(s: &Sketch, e: EntityId) -> (PointId, PointId) {
    match s.entities[e].geom {
        Geom::Line { a, b } | Geom::Arc { a, b, .. } => (a, b),
        Geom::Circle { .. } => panic!("no ends"),
    }
}

fn centre(s: &Sketch, e: EntityId) -> Vec2 {
    match s.entities[e].geom {
        Geom::Arc { c, .. } | Geom::Circle { c, .. } => s.pos(c),
        Geom::Line { .. } => panic!("no centre"),
    }
}

fn has_ends(s: &Sketch, e: EntityId, p: Vec2, q: Vec2) -> bool {
    let (a, b) = ends(s, e);
    (close(a, p) && close(b, q)) || (close(a, q) && close(b, p))
}

fn area(s: &Sketch) -> f64 {
    let p = s.profiles().expect("closed profile");
    assert_eq!(p.len(), 1, "{p:?}");
    p[0].area()
}

fn perimeter(s: &Sketch) -> f64 {
    let loops = s.loops().unwrap();
    loops[0]
        .segs
        .iter()
        .map(|seg| match *seg {
            Seg2::Line { a, b, .. } => a.distance(b),
            Seg2::Arc { radius, sweep, .. } => radius * sweep.abs(),
        })
        .sum()
}

fn arc_len(s: &Sketch, e: EntityId) -> f64 {
    s.measure(&Constraint::ArcLength { entity: e, value: 0.0 }).unwrap()
}

fn fillet(r: f64) -> FilletOptions {
    FilletOptions { radius: r, ..Default::default() }
}

// ----- Fillet -----------------------------------------------------------------------

#[test]
fn s1_s2_fillet_one_corner() {
    let (mut s, l, c) = rect();
    let f = s.fillet_corner(c[0], fillet(5.0)).unwrap();
    let arc = f.entities[0];
    assert!(near(area(&s), 800.0 - (25.0 - 25.0 * PI / 4.0), 1e-9));
    assert!(near(area(&s), 794.6350, 1e-4));
    assert!(close(centre(&s, arc), v2(5.0, 5.0)));
    assert!(has_ends(&s, arc, v2(5.0, 0.0), v2(0.0, 5.0)));
    assert!(near(s.line_length(l[0]), 35.0, 1e-12) && near(s.line_length(l[3]), 15.0, 1e-12));
    assert!(near(arc_len(&s, arc), 7.8540, 1e-4));
    assert!(near(perimeter(&s), 117.8540, 1e-4));
    let r = s.solve();
    assert!(r.converged, "{r:?}");
}

#[test]
fn s3_fillet_four_corners_in_one_run() {
    let (mut s, _, c) = rect();
    let f = s.fillet_corners(&c, fillet(5.0), None).unwrap();
    assert!(near(area(&s), 778.5398, 1e-4));
    assert!(near(perimeter(&s), 80.0 + 10.0 * PI, 1e-9));
    let radii = s.constraints.iter().filter(|k| matches!(k, Constraint::Radius { .. })).count();
    let equals: Vec<_> = s.constraints.iter().filter(|k| matches!(k, Constraint::Equal { .. })).collect();
    assert_eq!(radii, 1);
    assert_eq!(equals.len(), 3);
    for e in equals {
        assert!(matches!(e, Constraint::Equal { a, .. } if *a == f.entities[0]));
    }
    assert!(s.solve().converged);

    let (mut s, _, c) = rect();
    s.fillet_corners(&c, FilletOptions { dimension_each: true, ..fillet(5.0) }, None).unwrap();
    let radii: Vec<f64> = s
        .constraints
        .iter()
        .filter(|k| matches!(k, Constraint::Radius { .. }))
        .map(|k| k.value().unwrap())
        .collect();
    assert_eq!(radii, vec![5.0; 4]);
}

#[test]
fn s4_constrained_corner_becomes_a_virtual_sharp() {
    let (mut s, l, c) = rect();
    let dim = s.add_constraint(Constraint::HDistance { p: c[0], q: c[1], value: 40.0 });
    s.add_constraint(Constraint::Fix { p: c[0], x: 0.0, y: 0.0 });
    let f = s.fillet_corner(c[0], fillet(5.0)).unwrap();
    assert!(f.renumbering.is_identity());
    assert_eq!(f.virtual_sharps, vec![c[0]]);
    assert!(close(s.pos(c[0]), v2(0.0, 0.0)));
    assert!(s.entities_using(c[0]).is_empty(), "no longer an end point");
    assert!(s.constraints.contains(&Constraint::PointOnLine { p: c[0], line: l[0] }));
    assert!(s.constraints.contains(&Constraint::PointOnLine { p: c[0], line: l[3] }));
    assert!(s.solve().converged);
    s.constraints[dim].set_value(50.0);
    assert!(s.solve().converged);
    let (_, right_end) = ends(&s, l[0]);
    assert!(near(right_end.x, 50.0, 1e-7));
    assert!(near(ends(&s, l[1]).1.x, 50.0, 1e-7));
}

#[test]
fn s5_s6_unconstrained_corner_goes() {
    let (mut s, l, c) = rect();
    let n = s.points.len();
    let f = s.fillet_corner(c[0], fillet(5.0)).unwrap();
    assert_eq!(s.points.len(), n + 2);
    assert!(f.virtual_sharps.is_empty());
    assert_eq!(s.entities.len(), 5);
    let arc = f.entities[0];
    let (a, b) = end_ids(&s, arc);
    let bottom_start = end_ids(&s, l[0]).0;
    let left_end = end_ids(&s, l[3]).1;
    assert!([a, b].contains(&bottom_start) && [a, b].contains(&left_end));
}

#[test]
fn s7_fillet_of_a_sixty_degree_corner() {
    let mut s = Sketch::new();
    let o = s.add_point(v2(0.0, 0.0));
    let p = s.add_point(v2(30.0, 0.0));
    let apex = Vec2::from_angle(PI / 3.0) * 30.0;
    let q = s.add_point(apex);
    s.add_line(o, p);
    s.add_line(p, q);
    s.add_line(q, o);
    let before = area(&s);
    let f = s.fillet_corner(o, fillet(5.0)).unwrap();
    let arc = f.entities[0];
    assert!(close(centre(&s, arc), v2(5.0 / (PI / 6.0).tan(), 5.0)));
    assert!(near(centre(&s, arc).x, 8.6603, 1e-4));
    let (a, b) = ends(&s, arc);
    assert!(near(a.length().min(b.length()), 8.6603, 1e-4), "set-back");
    assert!(near(centre(&s, arc).length(), 10.0, 1e-9));
    assert!(near(arc_len(&s, arc), 10.4720, 1e-4));
    assert!(near(before - area(&s), 17.1213, 1e-4), "{}", before - area(&s));
}

#[test]
fn s8_line_to_arc_fillet() {
    let mut s = Sketch::new();
    let a = s.add_point(v2(-20.0, 0.0));
    let x = s.add_point(v2(0.0, 0.0));
    let l = s.add_line(a, x);
    let c = s.add_point(v2(20.0, 0.0));
    let top = s.add_point(v2(20.0, 20.0));
    let arc = s.add_arc(c, top, x);
    let f = s.fillet_corner(x, fillet(5.0)).unwrap();
    let fa = f.entities[0];
    let k = 20.0 - 10.0 * 6f64.sqrt();
    assert!(close(centre(&s, fa), v2(k, 5.0)), "{:?}", centre(&s, fa));
    assert!(close(ends(&s, l).1, v2(k, 0.0)));
    assert!(close(ends(&s, arc).1, v2(20.0 - 8.0 * 6f64.sqrt(), 4.0)));
    assert!(near(ends(&s, arc).1.x, 0.4041, 1e-4));
    assert!(s.constraints.contains(&Constraint::Tangent { line: l, entity: fa }));
    assert!(s.constraints.contains(&Constraint::TangentCircles { a: arc, b: fa, inside: false }));
    let r = s.solve();
    assert!(r.converged && r.max_residual < 1e-9, "{r:?}");
}

#[test]
fn s9_fillet_of_curves_that_do_not_touch() {
    let mut s = Sketch::new();
    let a = line(&mut s, v2(0.0, 0.0), v2(30.0, 0.0));
    let b = line(&mut s, v2(40.0, 5.0), v2(40.0, 20.0));
    let f = s.fillet_entities(a, v2(10.0, 0.0), b, v2(40.0, 15.0), fillet(5.0), None).unwrap();
    let arc = f.entities[0];
    assert!(close(centre(&s, arc), v2(35.0, 5.0)));
    assert!(has_ends(&s, arc, v2(35.0, 0.0), v2(40.0, 5.0)));
    assert!(has_ends(&s, a, v2(0.0, 0.0), v2(35.0, 0.0)));
    assert!(has_ends(&s, b, v2(40.0, 5.0), v2(40.0, 20.0)));
}

#[test]
fn s10_too_large_a_fillet_is_refused() {
    let (mut s, _, c) = rect();
    let before = s.clone();
    assert_eq!(s.fillet_corner(c[0], fillet(25.0)), Err(SketchError::TooLarge));
    assert_eq!(s, before);
    // One bad corner in a run leaves the others alone too.
    assert_eq!(s.fillet_corners(&[c[1], c[0]], fillet(25.0), None), Err(SketchError::TooLarge));
    assert_eq!(s, before);
}

// ----- Chamfer ----------------------------------------------------------------------

#[test]
fn s11_s12_s13_chamfers() {
    let (mut s, l, c) = rect();
    let r = s.chamfer_corner(c[0], Some(l[0]), ChamferKind::Distances(5.0, 10.0), true).unwrap();
    assert!(has_ends(&s, r.entities[0], v2(5.0, 0.0), v2(0.0, 10.0)));
    assert!(near(s.line_length(r.entities[0]), 125f64.sqrt(), 1e-12));
    assert!(near(area(&s), 775.0, 1e-9));

    let (mut s, l, c) = rect();
    let r = s.chamfer_corner(c[0], Some(l[0]), ChamferKind::Equal(5.0), true).unwrap();
    assert!(has_ends(&s, r.entities[0], v2(5.0, 0.0), v2(0.0, 5.0)));
    assert!(near(s.line_length(r.entities[0]), 7.0711, 1e-4));
    assert!(near(area(&s), 787.5, 1e-9));

    let (mut s, l, c) = rect();
    let r = s
        .chamfer_corner(c[0], Some(l[0]), ChamferKind::AngleDistance { distance: 5.0, degrees: 30.0 }, true)
        .unwrap();
    assert!(has_ends(&s, r.entities[0], v2(5.0, 0.0), v2(0.0, 5.0 * (PI / 6.0).tan())));
    assert!(near(s.line_length(r.entities[0]), 5.7735, 1e-4));
    assert!(near(area(&s), 792.7831, 1e-4));
}

#[test]
fn chamfer_keeps_a_constrained_corner_with_its_dimensions() {
    let (mut s, l, c) = rect();
    s.add_constraint(Constraint::Fix { p: c[0], x: 0.0, y: 0.0 });
    let kind = ChamferKind::AngleDistance { distance: 5.0, degrees: 30.0 };
    let r = s.chamfer_corner(c[0], Some(l[0]), kind, true).unwrap();
    assert_eq!(r.virtual_sharps, vec![c[0]]);
    let angle = s.constraints.iter().find(|k| matches!(k, Constraint::RayAngle { .. })).copied().unwrap();
    assert!(near(s.measure(&angle).unwrap(), 30.0, 1e-9));
    assert!(s.constraints.iter().any(|k| matches!(k, Constraint::Distance { value, .. } if *value == 5.0)));
    assert!(s.solve().converged);
    // Chamfers of more than the line allows are refused.
    let (mut s, l, c) = rect();
    assert_eq!(
        s.chamfer_corner(c[0], Some(l[3]), ChamferKind::Equal(20.0), true),
        Err(SketchError::TooLarge)
    );
}

// ----- Offset -----------------------------------------------------------------------

fn offset(d: f64) -> OffsetOptions {
    OffsetOptions { distance: d, ..Default::default() }
}

fn bounds(s: &Sketch, ids: &[EntityId]) -> (Vec2, Vec2) {
    let mut lo = v2(f64::MAX, f64::MAX);
    let mut hi = v2(f64::MIN, f64::MIN);
    for &e in ids {
        let (a, b) = ends(s, e);
        for p in [a, b] {
            lo = v2(lo.x.min(p.x), lo.y.min(p.y));
            hi = v2(hi.x.max(p.x), hi.y.max(p.y));
        }
    }
    (lo, hi)
}

#[test]
fn s14_s15_s16_rectangle_offsets() {
    let (mut s, l, _) = rect();
    let r = s.offset_entities(&[l[0]], offset(5.0)).unwrap();
    assert_eq!(r.entities.len(), 4);
    assert!(r.entities.iter().all(|&e| s.entities[e].is_line()), "sharp corners");
    let (lo, hi) = bounds(&s, &r.entities);
    assert!(close(lo, v2(-5.0, -5.0)) && close(hi, v2(45.0, 25.0)));
    assert!(s.solve().converged);

    let (mut s, l, _) = rect();
    let r = s.offset_entities(&[l[2]], OffsetOptions { reverse: true, ..offset(5.0) }).unwrap();
    let (lo, hi) = bounds(&s, &r.entities);
    assert!(close(lo, v2(5.0, 5.0)) && close(hi, v2(35.0, 15.0)));

    let (mut s, l, _) = rect();
    let before = s.clone();
    assert_eq!(
        s.offset_entities(&[l[0]], OffsetOptions { reverse: true, ..offset(10.0) }),
        Err(SketchError::TooLarge)
    );
    assert_eq!(s, before);
}

#[test]
fn offset_profiles_and_areas() {
    // The outer offset with the original as construction is one 50 x 30 profile.
    let (mut s, l, _) = rect();
    s.offset_entities(&[l[0]], OffsetOptions { base_construction: true, ..offset(5.0) }).unwrap();
    assert!(near(area(&s), 1500.0, 1e-9));
    // s21: and the originals are construction.
    for e in l {
        assert!(s.entities[e].construction);
    }
    let (mut s, l, _) = rect();
    s.offset_entities(&[l[0]], OffsetOptions { reverse: true, base_construction: true, ..offset(5.0) })
        .unwrap();
    assert!(near(area(&s), 300.0, 1e-9));
}

#[test]
fn s17_circle_offsets() {
    let mut s = Sketch::new();
    let c = s.add_point(v2(3.0, 4.0));
    let circle = s.add_circle(c, 10.0);
    let r = s.offset_entities(&[circle], offset(5.0)).unwrap();
    assert!(near(s.radius(r.entities[0]), 15.0, 1e-12) && close(centre(&s, r.entities[0]), v2(3.0, 4.0)));
    let r = s.offset_entities(&[circle], OffsetOptions { reverse: true, ..offset(5.0) }).unwrap();
    assert!(near(s.radius(r.entities[0]), 5.0, 1e-12));
    assert_eq!(
        s.offset_entities(&[circle], OffsetOptions { reverse: true, ..offset(10.0) }),
        Err(SketchError::TooLarge)
    );
    assert!(s.solve().converged);
}

#[test]
fn s18_offset_follows_its_dimension_and_its_original() {
    let (mut s, l, c) = rect();
    s.add_constraint(Constraint::Fix { p: c[0], x: 0.0, y: 0.0 });
    let w = s.add_constraint(Constraint::Length { line: l[0], value: 40.0 });
    s.add_constraint(Constraint::Length { line: l[1], value: 20.0 });
    let r = s.offset_entities(&[l[0]], offset(5.0)).unwrap();
    assert_eq!(r.dimensions.len(), 1);
    let d = r.dimensions[0];
    assert!(near(s.constraints[d].value().unwrap().abs(), 5.0, 1e-12));
    let st = s.solve();
    assert!(st.converged && st.dof == 0, "{st:?}");
    s.constraints[d].set_value(8.0);
    assert!(s.solve().converged);
    let (lo, hi) = bounds(&s, &r.entities);
    assert!(near(hi.x - lo.x, 56.0, 1e-7) && near(hi.y - lo.y, 36.0, 1e-7));
    s.constraints[d].set_value(5.0);
    s.constraints[w].set_value(60.0);
    assert!(s.solve().converged);
    let (lo, hi) = bounds(&s, &r.entities);
    assert!(near(hi.x - lo.x, 70.0, 1e-7) && near(hi.y - lo.y, 30.0, 1e-7));
    // A typed minus sign moves the offset to the other side.
    s.constraints[d].set_value(-5.0);
    assert!(s.solve().converged);
    let (lo, hi) = bounds(&s, &r.entities);
    assert!(near(hi.x - lo.x, 50.0, 1e-7) && near(hi.y - lo.y, 10.0, 1e-7));
}

#[test]
fn s19_bidirectional_offset_with_caps() {
    let mut s = Sketch::new();
    let l = line(&mut s, v2(0.0, 0.0), v2(40.0, 0.0));
    let opts =
        OffsetOptions { bidirectional: true, caps: CapEnds::Arcs, base_construction: true, ..offset(5.0) };
    s.offset_entities(&[l], opts).unwrap();
    assert!(near(area(&s), 400.0 + 25.0 * PI, 1e-9));
    assert!(near(area(&s), 478.5398, 1e-4));
    let st = s.solve();
    assert!(st.converged, "{st:?}");

    let mut s = Sketch::new();
    let l = line(&mut s, v2(0.0, 0.0), v2(40.0, 0.0));
    let opts =
        OffsetOptions { bidirectional: true, caps: CapEnds::Lines, base_construction: true, ..offset(5.0) };
    s.offset_entities(&[l], opts).unwrap();
    assert!(near(area(&s), 400.0, 1e-9));
    assert!(s.solve().converged);
}

#[test]
fn s20_bidirectional_offsets_have_their_own_dimensions() {
    let mut s = Sketch::new();
    let l = line(&mut s, v2(0.0, 0.0), v2(40.0, 0.0));
    let (a, b) = end_ids(&s, l);
    s.add_constraint(Constraint::Fix { p: a, x: 0.0, y: 0.0 });
    s.add_constraint(Constraint::Fix { p: b, x: 40.0, y: 0.0 });
    let r = s.offset_entities(&[l], OffsetOptions { bidirectional: true, ..offset(5.0) }).unwrap();
    assert_eq!(r.dimensions.len(), 2);
    s.constraints[r.dimensions[0]].set_value(3.0);
    s.constraints[r.dimensions[1]].set_value(7.0);
    let st = s.solve();
    assert!(st.converged && st.dof == 0, "{st:?}");
    let mut ys: Vec<f64> = r.entities.iter().map(|&e| ends(&s, e).0.y).collect();
    ys.sort_by(f64::total_cmp);
    assert!(
        (near(ys[0], -7.0, 1e-7) && near(ys[1], 3.0, 1e-7))
            || (near(ys[0], -3.0, 1e-7) && near(ys[1], 7.0, 1e-7)),
        "{ys:?}"
    );
}

#[test]
fn s22_offset_of_a_line_and_tangent_arc() {
    let mut s = Sketch::new();
    let p0 = s.add_point(v2(0.0, 0.0));
    let p1 = s.add_point(v2(20.0, 0.0));
    let p2 = s.add_point(v2(30.0, 10.0));
    let c = s.add_point(v2(20.0, 10.0));
    let l = s.add_line(p0, p1);
    let a = s.add_arc(c, p1, p2);
    let r = s.offset_entities(&[l], offset(2.0)).unwrap();
    let (nl, na) = (r.entities[0], r.entities[1]);
    assert!(near(ends(&s, nl).0.y, 2.0, 1e-12) && near(ends(&s, nl).1.y, 2.0, 1e-12));
    assert!(close(centre(&s, na), v2(20.0, 10.0)) && near(s.radius(na), 8.0, 1e-12));
    let shared = end_ids(&s, nl).1;
    assert!(close(s.pos(shared), v2(20.0, 2.0)));
    assert!(end_ids(&s, na).0 == shared || end_ids(&s, na).1 == shared);
    let _ = a;
    let st = s.solve();
    assert!(st.converged, "{st:?}");
}

// ----- Mirror -----------------------------------------------------------------------

fn axis_x0(s: &mut Sketch) -> EntityId {
    let a = line(s, v2(0.0, -10.0), v2(0.0, 10.0));
    s.entities[a].construction = true;
    a
}

#[test]
fn s25_mirrored_circle_follows() {
    let mut s = Sketch::new();
    let axis = axis_x0(&mut s);
    let (p, q) = end_ids(&s, axis);
    s.add_constraint(Constraint::Fix { p, x: 0.0, y: -10.0 });
    s.add_constraint(Constraint::Fix { p: q, x: 0.0, y: 10.0 });
    let c = s.add_point(v2(10.0, 5.0));
    let circle = s.add_circle(c, 3.0);
    let (m, _) = s.mirror_entities(&[circle], axis, true).unwrap();
    assert!(close(centre(&s, m[0]), v2(-10.0, 5.0)) && near(s.radius(m[0]), 3.0, 1e-12));
    assert!(s.constraints.iter().any(|k| matches!(k, Constraint::Symmetric { p, .. } if *p == c)));
    assert!(s.constraints.contains(&Constraint::Equal { a: circle, b: m[0] }));
    assert!(s.drag_point(c, v2(12.0, 6.0)).converged);
    assert!(close(centre(&s, m[0]), v2(-12.0, 6.0)), "{:?}", centre(&s, m[0]));
}

#[test]
fn s26_s27_mirror_images() {
    let mut s = Sketch::new();
    let axis = line(&mut s, v2(0.0, 0.0), v2(10.0, 10.0));
    let l = line(&mut s, v2(5.0, 0.0), v2(5.0, 10.0));
    let (m, _) = s.mirror_entities(&[l], axis, true).unwrap();
    assert!(has_ends(&s, m[0], v2(0.0, 5.0), v2(10.0, 5.0)));

    let mut s = Sketch::new();
    let axis = axis_x0(&mut s);
    let c = s.add_point(v2(10.0, 0.0));
    let a = s.add_point(v2(15.0, 0.0));
    let b = s.add_point(v2(10.0, 5.0));
    let arc = s.add_arc(c, a, b);
    let (m, _) = s.mirror_entities(&[arc], axis, true).unwrap();
    assert!(close(centre(&s, m[0]), v2(-10.0, 0.0)));
    let (ma, mb) = ends(&s, m[0]);
    assert!(close(ma, v2(-10.0, 5.0)) && close(mb, v2(-15.0, 0.0)), "{ma:?} {mb:?}");
    assert!(near(arc_len(&s, m[0]), 5.0 * PI / 2.0, 1e-9));
}

#[test]
fn s28_mirror_without_copy_replaces() {
    let mut s = Sketch::new();
    let axis = axis_x0(&mut s);
    let l = line(&mut s, v2(5.0, 0.0), v2(15.0, 0.0));
    s.add_constraint(Constraint::Horizontal { line: l });
    s.add_constraint(Constraint::Length { line: l, value: 10.0 });
    let n = s.entities.len();
    let (m, _) = s.mirror_entities(&[l], axis, false).unwrap();
    assert_eq!(s.entities.len(), n);
    assert!(has_ends(&s, m[0], v2(-5.0, 0.0), v2(-15.0, 0.0)));
    assert!(s.constraints.contains(&Constraint::Horizontal { line: m[0] }));
    assert!(s.constraints.contains(&Constraint::Length { line: m[0], value: 10.0 }));
}

#[test]
fn s29_points_on_the_mirror_line_are_shared() {
    let mut s = Sketch::new();
    let axis = axis_x0(&mut s);
    let o = s.add_point(v2(0.0, 0.0));
    let p = s.add_point(v2(10.0, 0.0));
    let q = s.add_point(v2(10.0, 10.0));
    let a = s.add_line(o, p);
    let b = s.add_line(p, q);
    let (m, _) = s.mirror_entities(&[a, b], axis, true).unwrap();
    let mut pts: Vec<PointId> = [a, b, m[0], m[1]].iter().flat_map(|&e| s.entities[e].points()).collect();
    pts.sort_unstable();
    pts.dedup();
    assert_eq!(pts.len(), 5);
}

// ----- Patterns ---------------------------------------------------------------------

fn seed_circle(s: &mut Sketch, at: Vec2) -> EntityId {
    let c = s.add_point(at);
    s.add_circle(c, 2.0)
}

fn circle_centres(s: &Sketch) -> Vec<Vec2> {
    s.entities
        .iter()
        .filter_map(|e| match e.geom {
            Geom::Circle { c, .. } if !e.construction => Some(s.pos(c)),
            _ => None,
        })
        .collect()
}

fn has_centre(cs: &[Vec2], p: Vec2) -> bool {
    cs.iter().any(|&c| c.distance(p) < 1e-4)
}

#[test]
fn s30_s31_linear_pattern() {
    let mut s = Sketch::new();
    let seed = seed_circle(&mut s, v2(0.0, 0.0));
    let p =
        LinearPattern { count: [3, 2], spacing: [10.0, 15.0], degrees: [0.0, 90.0], ..Default::default() };
    let r = s.linear_pattern(&[seed], &p).unwrap();
    assert_eq!(r.instances.len(), 5);
    let cs = circle_centres(&s);
    assert_eq!(cs.len(), 6);
    for q in [(0.0, 0.0), (10.0, 0.0), (20.0, 0.0), (0.0, 15.0), (10.0, 15.0), (20.0, 15.0)] {
        assert!(has_centre(&cs, v2(q.0, q.1)), "{q:?}");
    }
    assert!(s.solve().converged);

    let mut s = Sketch::new();
    let seed = seed_circle(&mut s, v2(0.0, 0.0));
    let p = LinearPattern { skip: vec![(1, 1)], ..p };
    s.linear_pattern(&[seed], &p).unwrap();
    let cs = circle_centres(&s);
    assert_eq!(cs.len(), 5);
    assert!(!has_centre(&cs, v2(10.0, 15.0)));
}

#[test]
fn s32_linear_pattern_at_an_angle() {
    let mut s = Sketch::new();
    let seed = seed_circle(&mut s, v2(0.0, 0.0));
    let p =
        LinearPattern { count: [2, 1], spacing: [10.0, 10.0], degrees: [10.0, 90.0], ..Default::default() };
    s.linear_pattern(&[seed], &p).unwrap();
    assert!(has_centre(&circle_centres(&s), v2(9.8481, 1.7365)));
}

#[test]
fn s33_pattern_follows_its_spacing() {
    let mut s = Sketch::new();
    let seed = seed_circle(&mut s, v2(0.0, 0.0));
    if let Geom::Circle { c, .. } = s.entities[seed].geom {
        s.add_constraint(Constraint::Coincident { p: c, q: ORIGIN });
    }
    s.add_constraint(Constraint::Radius { entity: seed, value: 2.0 });
    let p =
        LinearPattern { count: [3, 2], spacing: [10.0, 15.0], degrees: [0.0, 90.0], ..Default::default() };
    let r = s.linear_pattern(&[seed], &p).unwrap();
    let st = s.solve();
    assert!(st.converged && st.dof == 0, "a fully defined seed gives a fully defined pattern: {st:?}");
    s.constraints[r.dimensions[0]].set_value(12.0);
    assert!(s.solve().converged);
    let cs = circle_centres(&s);
    for q in [(0.0, 0.0), (12.0, 0.0), (24.0, 0.0), (0.0, 15.0), (12.0, 15.0), (24.0, 15.0)] {
        assert!(has_centre(&cs, v2(q.0, q.1)), "{q:?} {cs:?}");
    }
    // Instances keep the seed's size.
    for e in &r.instances {
        assert!(near(s.radius(e[0]), 2.0, 1e-7));
    }
}

#[test]
fn s34_s35_s36_s37_circular_patterns() {
    let run = |count: u32, reverse: bool| {
        let mut s = Sketch::new();
        let seed = seed_circle(&mut s, v2(20.0, 0.0));
        let p = CircularPattern { count, reverse, ..Default::default() };
        s.circular_pattern(&[seed], &p).unwrap();
        assert!(s.solve().converged);
        circle_centres(&s)
    };
    let cs = run(4, false);
    assert_eq!(cs.len(), 4);
    for q in [(20.0, 0.0), (0.0, 20.0), (-20.0, 0.0), (0.0, -20.0)] {
        assert!(has_centre(&cs, v2(q.0, q.1)), "{q:?}");
    }
    let cs = run(6, false);
    assert!(close(cs[1], v2(10.0, 17.3205)) || has_centre(&cs, v2(10.0, 17.3205)));
    let mut s = Sketch::new();
    let seed = seed_circle(&mut s, v2(20.0, 0.0));
    let r = s
        .circular_pattern(&[seed], &CircularPattern { count: 4, reverse: true, ..Default::default() })
        .unwrap();
    assert!(close(centre(&s, r.instances[0][0]), v2(0.0, -20.0)));
    let c = circular_pattern_center(v2(20.0, 0.0), 20.0, 180.0);
    assert!(close(c, v2(0.0, 0.0)));
}

#[test]
fn circular_pattern_follows_its_angle() {
    let mut s = Sketch::new();
    let seed = seed_circle(&mut s, v2(20.0, 0.0));
    if let Geom::Circle { c, .. } = s.entities[seed].geom {
        s.add_constraint(Constraint::Fix { p: c, x: 20.0, y: 0.0 });
    }
    let r = s
        .circular_pattern(&[seed], &CircularPattern { count: 3, total_degrees: 90.0, ..Default::default() })
        .unwrap();
    assert!(close(centre(&s, r.instances[1][0]), v2(0.0, 20.0)));
    s.constraints[r.dimensions[1]].set_value(30.0);
    assert!(s.solve().converged);
    assert!(close(centre(&s, r.instances[1][0]), Vec2::from_angle(PI / 3.0) * 20.0));
}

// ----- Move, copy, rotate, scale ----------------------------------------------------

fn dimensioned_rect() -> (Sketch, [EntityId; 4], [PointId; 4], [usize; 2]) {
    let (mut s, l, c) = rect();
    let w = s.add_constraint(Constraint::Length { line: l[0], value: 40.0 });
    let h = s.add_constraint(Constraint::Length { line: l[1], value: 20.0 });
    (s, l, c, [w, h])
}

#[test]
fn s38_s39_move_and_copy() {
    let (mut s, l, c, d) = dimensioned_rect();
    s.transform_entities(&l, Transform2::Translate(v2(10.0, -5.0)), TransformOptions::default()).unwrap();
    assert!(s.solve().converged);
    assert!(close(s.pos(c[0]), v2(10.0, -5.0)) && close(s.pos(c[2]), v2(50.0, 15.0)));
    assert_eq!(s.constraints[d[0]].value(), Some(40.0));
    assert_eq!(s.constraints[d[1]].value(), Some(20.0));

    let (mut s, l, _, _) = dimensioned_rect();
    let opts = TransformOptions { copy: true, ..Default::default() };
    let (new, _) = s.transform_entities(&l, Transform2::Translate(v2(10.0, -5.0)), opts).unwrap();
    assert_eq!(s.entities.len(), 8);
    assert!(near(s.line_length(new[0]), 40.0, 1e-12) && near(s.line_length(new[1]), 20.0, 1e-12));
    assert!(s.constraints.contains(&Constraint::Length { line: new[0], value: 40.0 }));
    assert!(s.solve().converged);
}

#[test]
fn s40_rotate() {
    let r = Transform2::Rotate { center: v2(0.0, 0.0), degrees: 30.0 }.apply(v2(10.0, 0.0));
    assert!(close(r, v2(10.0 * (PI / 6.0).cos(), 5.0)));
    assert!(near(r.x, 8.6603, 1e-4) && near(r.y, 5.0, 1e-12));
    let (mut s, l, c, _) = dimensioned_rect();
    s.transform_entities(
        &l,
        Transform2::Rotate { center: v2(0.0, 0.0), degrees: 90.0 },
        TransformOptions::default(),
    )
    .unwrap();
    let st = s.solve();
    assert!(st.converged, "{st:?}");
    for (p, q) in c.iter().zip([v2(0.0, 0.0), v2(0.0, 40.0), v2(-20.0, 40.0), v2(-20.0, 0.0)]) {
        assert!(close(s.pos(*p), q), "{:?} {q:?}", s.pos(*p));
    }
    assert!(s.constraints.contains(&Constraint::Vertical { line: l[0] }));
}

#[test]
fn s41_scale() {
    let (mut s, l, c, d) = dimensioned_rect();
    s.transform_entities(
        &l,
        Transform2::Scale { about: v2(0.0, 0.0), factor: 0.5 },
        TransformOptions::default(),
    )
    .unwrap();
    assert!(s.solve().converged);
    assert!(close(s.pos(c[2]), v2(20.0, 10.0)));
    assert!(near(area(&s), 200.0, 1e-9));
    assert_eq!(s.constraints[d[0]].value(), Some(20.0));
    assert_eq!(s.constraints[d[1]].value(), Some(10.0));
}

#[test]
fn moving_breaks_relations_to_the_rest_unless_kept() {
    let (mut s, l, c) = rect();
    s.add_constraint(Constraint::Fix { p: c[0], x: 0.0, y: 0.0 });
    let free = line(&mut s, v2(0.0, 0.0), v2(-10.0, 0.0));
    let (fa, _) = end_ids(&s, free);
    let n = s.merge_points(c[0], fa).unwrap();
    let c0 = n.point(c[0]).unwrap();
    // The corner is shared with an unselected line and fixed: without "Keep relations"
    // the rectangle leaves both behind.
    s.transform_entities(&l, Transform2::Translate(v2(5.0, 5.0)), TransformOptions::default()).unwrap();
    assert!(s.solve().converged);
    assert!(close(s.pos(c0), v2(0.0, 0.0)));
    assert!(close(ends(&s, l[0]).0, v2(5.0, 5.0)));
}

// ----- Trim, extend, split ----------------------------------------------------------

fn cross() -> (Sketch, EntityId, EntityId) {
    let mut s = Sketch::new();
    let h = line(&mut s, v2(-10.0, 0.0), v2(10.0, 0.0));
    let v = line(&mut s, v2(0.0, -10.0), v2(0.0, 10.0));
    (s, h, v)
}

#[test]
fn s42_power_trim() {
    let (mut s, h, v) = cross();
    let r = s.trim_at(h, v2(5.0, 0.0), TrimOptions::default()).unwrap();
    assert_eq!(r.kept, vec![h]);
    assert!(has_ends(&s, h, v2(-10.0, 0.0), v2(0.0, 0.0)));
    assert!(near(s.line_length(h), 10.0, 1e-12));
    let (_, cut) = end_ids(&s, h);
    assert!(s.constraints.contains(&Constraint::PointOnLine { p: cut, line: v }));
    assert_eq!(s.points.len(), 5, "the trimmed-off end point is gone, the cut point is new");
}

#[test]
fn s43_trim_of_a_line_that_crosses_nothing() {
    let mut s = Sketch::new();
    let a = line(&mut s, v2(0.0, 0.0), v2(10.0, 0.0));
    line(&mut s, v2(0.0, 5.0), v2(10.0, 5.0));
    let r = s.trim_at(a, v2(3.0, 0.0), TrimOptions::default()).unwrap();
    assert!(r.kept.is_empty());
    assert_eq!(s.entities.len(), 1);
}

#[test]
fn s44_trim_a_circle_into_an_arc() {
    let mut s = Sketch::new();
    let c = s.add_point(v2(0.0, 0.0));
    let circle = s.add_circle(c, 10.0);
    line(&mut s, v2(5.0, -20.0), v2(5.0, 20.0));
    s.trim_at(circle, v2(10.0, 0.0), TrimOptions::default()).unwrap();
    let (a, b) = ends(&s, circle);
    assert!(close(a, v2(5.0, 75f64.sqrt())) && close(b, v2(5.0, -75f64.sqrt())), "{a:?} {b:?}");
    assert!(near(arc_len(&s, circle), 10.0 * 240f64.to_radians(), 1e-9));
    assert!(close(centre(&s, circle), v2(0.0, 0.0)) && near(s.radius(circle), 10.0, 1e-12));
}

#[test]
fn s45_trim_the_middle() {
    let mut s = Sketch::new();
    let l = line(&mut s, v2(0.0, 3.0), v2(30.0, 3.0));
    s.add_constraint(Constraint::Horizontal { line: l });
    line(&mut s, v2(10.0, 0.0), v2(10.0, 10.0));
    line(&mut s, v2(20.0, 0.0), v2(20.0, 10.0));
    let r = s.trim_at(l, v2(15.0, 3.0), TrimOptions::default()).unwrap();
    assert_eq!(r.kept.len(), 2);
    assert!(has_ends(&s, r.kept[0], v2(0.0, 3.0), v2(10.0, 3.0)));
    assert!(has_ends(&s, r.kept[1], v2(20.0, 3.0), v2(30.0, 3.0)));
    for e in r.kept {
        assert!(s.constraints.contains(&Constraint::Horizontal { line: e }));
    }
    assert!(s.solve().converged);
}

#[test]
fn s46_corner_trim() {
    let mut s = Sketch::new();
    let a = line(&mut s, v2(0.0, 0.0), v2(10.0, 0.0));
    let b = line(&mut s, v2(15.0, 5.0), v2(15.0, 20.0));
    let (x, _) = s.trim_corner(a, v2(1.0, 0.0), b, v2(15.0, 19.0)).unwrap();
    assert!(close(s.pos(x), v2(15.0, 0.0)));
    assert!(has_ends(&s, a, v2(0.0, 0.0), v2(15.0, 0.0)));
    assert!(has_ends(&s, b, v2(15.0, 0.0), v2(15.0, 20.0)));
    assert!(near(s.line_length(a), 15.0, 1e-12) && near(s.line_length(b), 20.0, 1e-12));
    assert_eq!(s.entities_ending_at(x).len(), 2);
}

#[test]
fn s47_trim_away_inside_and_outside() {
    let setup = || {
        let mut s = Sketch::new();
        let b1 = line(&mut s, v2(0.0, -5.0), v2(0.0, 5.0));
        let b2 = line(&mut s, v2(10.0, -5.0), v2(10.0, 5.0));
        let t = line(&mut s, v2(-5.0, 3.0), v2(15.0, 3.0));
        (s, [b1, b2], t)
    };
    let (mut s, b, t) = setup();
    let r = s.trim_away(b, &[t], true).unwrap();
    assert_eq!(r.kept.len(), 2);
    assert!(has_ends(&s, r.kept[0], v2(-5.0, 3.0), v2(0.0, 3.0)));
    assert!(has_ends(&s, r.kept[1], v2(10.0, 3.0), v2(15.0, 3.0)));
    let (mut s, b, t) = setup();
    let r = s.trim_away(b, &[t], false).unwrap();
    assert_eq!(r.kept, vec![t]);
    assert!(has_ends(&s, t, v2(0.0, 3.0), v2(10.0, 3.0)));
    assert!(s.solve().converged);
    // A curve that crosses only one boundary is refused.
    let (mut s, b, _) = setup();
    let short = line(&mut s, v2(5.0, -2.0), v2(15.0, -2.0));
    let before = s.clone();
    assert!(s.trim_away(b, &[short], true).is_err());
    assert_eq!(s, before);
}

#[test]
fn s48_keep_trimmed_part_as_construction() {
    let (mut s, h, _) = cross();
    let r =
        s.trim_at(h, v2(5.0, 0.0), TrimOptions { keep_as_construction: true, ..Default::default() }).unwrap();
    let k = r.construction[0];
    assert!(s.entities[k].construction);
    assert!(has_ends(&s, k, v2(0.0, 0.0), v2(10.0, 0.0)));
}

#[test]
fn s49_extend() {
    let mut s = Sketch::new();
    let l = line(&mut s, v2(0.0, 0.0), v2(5.0, 0.0));
    let wall = line(&mut s, v2(12.0, -5.0), v2(12.0, 5.0));
    let (p, _) = s.extend(l, v2(4.0, 0.0)).unwrap();
    assert!(has_ends(&s, l, v2(0.0, 0.0), v2(12.0, 0.0)));
    assert!(s.constraints.contains(&Constraint::PointOnLine { p, line: wall }));
    // Nothing further in the way.
    let before = s.clone();
    assert_eq!(s.extend(l, v2(11.0, 0.0)), Err(SketchError::NoIntersection));
    assert_eq!(s, before);
}

#[test]
fn s50_split_and_join() {
    let mut s = Sketch::new();
    let l = line(&mut s, v2(0.0, 0.0), v2(40.0, 0.0));
    let (p, piece) = s.split_at(l, v2(10.0, 0.3)).unwrap();
    assert!(close(s.pos(p), v2(10.0, 0.0)));
    assert!(near(s.line_length(l), 10.0, 1e-12) && near(s.line_length(piece), 30.0, 1e-12));
    assert!(s.constraints.contains(&Constraint::Collinear { a: l, b: piece }));
    assert!(s.can_join_at(p));
    let (joined, _) = s.join_at(p).unwrap();
    assert_eq!(s.entities.len(), 1);
    assert!(has_ends(&s, joined, v2(0.0, 0.0), v2(40.0, 0.0)));
    assert!(s.constraints.iter().all(|c| !matches!(c, Constraint::Collinear { .. })));
}

#[test]
fn split_and_rejoin_a_circle() {
    let mut s = Sketch::new();
    let c = s.add_point(v2(0.0, 0.0));
    let circle = s.add_circle(c, 10.0);
    s.add_constraint(Constraint::Radius { entity: circle, value: 10.0 });
    let (a, _, piece) = s.split_circle(circle, v2(10.0, 0.0), v2(-10.0, 0.0)).unwrap();
    assert!(near(arc_len(&s, circle), 10.0 * PI, 1e-9) && near(arc_len(&s, piece), 10.0 * PI, 1e-9));
    let st = s.solve();
    assert!(st.converged, "{st:?}");
    let (joined, _) = s.join_at(a).unwrap();
    assert!(matches!(s.entities[joined].geom, Geom::Circle { r, .. } if near(r, 10.0, 1e-12)));
    assert_eq!(s.entities.len(), 1);
}

#[test]
fn construction_toggle() {
    let (mut s, l, _) = rect();
    assert!(s.toggle_construction(&l[..2]).unwrap());
    assert!(s.entities[l[0]].construction && s.entities[l[1]].construction);
    assert!(!s.toggle_construction(&l[..2]).unwrap());
    assert_eq!(s.toggle_construction(&[99]), Err(SketchError::Missing));
}

#[test]
fn fillet_a_centre_rectangle_corner_keeps_the_diagonal() {
    use anvil_sketch::entities::RectOptions;
    let mut s = Sketch::new();
    let r = s.add_center_rectangle(v2(0.0, 0.0), v2(20.0, 10.0), RectOptions::default()).unwrap();
    let f = s.fillet_corner(r.corners[0], fillet(3.0)).unwrap();
    // The diagonal still ends at the corner, so it stays as a virtual sharp.
    assert_eq!(f.virtual_sharps, vec![r.corners[0]]);
    let st = s.solve();
    assert!(st.converged, "{st:?}");
    assert!(near(area(&s), 800.0 - (9.0 - 9.0 * PI / 4.0), 1e-7));
}

#[test]
fn offset_dimension_is_drawn_between_the_lines() {
    let (mut s, l, _) = rect();
    let r = s.offset_entities(&[l[0]], offset(5.0)).unwrap();
    let c = s.constraints[r.dimensions[0]];
    let at = anvil_sketch::dimension::anchor(&s, &c).unwrap();
    // Halfway between the bottom line and its offset, under the bottom line's middle.
    assert!(close(at, v2(20.0, -2.5)), "{at:?}");
    let g = anvil_sketch::dimension::graphic(&s, &c, at);
    assert_eq!(g.arrows.len(), 2);
}
