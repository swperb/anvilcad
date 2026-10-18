//! The relation statements of `docs/re/sketch-tools.md` ("Relations", 51 to 57).

use anvil_kernel::{v2, Vec2};
use anvil_sketch::{Constraint, EntityId, Geom, PointId, Sketch, ORIGIN};

fn line(s: &mut Sketch, a: Vec2, b: Vec2) -> EntityId {
    let (p, q) = (s.add_point(a), s.add_point(b));
    s.add_line(p, q)
}

fn ends(s: &Sketch, e: EntityId) -> (PointId, PointId) {
    match s.entities[e].geom {
        Geom::Line { a, b } | Geom::Arc { a, b, .. } => (a, b),
        Geom::Circle { .. } => panic!("no ends"),
    }
}

fn arc(s: &mut Sketch, c: Vec2, r: f64) -> EntityId {
    let pc = s.add_point(c);
    let pa = s.add_point(c + v2(r, 0.0));
    let pb = s.add_point(c + v2(0.0, r));
    s.add_arc(pc, pa, pb)
}

fn centre(s: &Sketch, e: EntityId) -> Vec2 {
    match s.entities[e].geom {
        Geom::Arc { c, .. } | Geom::Circle { c, .. } => s.pos(c),
        Geom::Line { .. } => panic!("no centre"),
    }
}

/// The freedoms a relation removes from the sketch as it stands.
fn removes(s: &mut Sketch, c: Constraint) -> usize {
    let before = s.solve().dof;
    s.add_constraint(c);
    let r = s.solve();
    assert!(r.converged, "{c:?}: {r:?}");
    before - r.dof
}

#[test]
fn s51_collinear() {
    let mut s = Sketch::new();
    let a = line(&mut s, v2(0.0, 0.0), v2(10.0, 0.0));
    let b = line(&mut s, v2(20.0, 1.0), v2(30.0, 3.0));
    assert_eq!(removes(&mut s, Constraint::Collinear { a, b }), 2);
    let ((p0, p1), (q0, q1)) = (ends(&s, a), ends(&s, b));
    let (o, d) = (s.pos(p0), (s.pos(p1) - s.pos(p0)).normalized());
    for p in [p0, p1, q0, q1] {
        assert!(d.cross(s.pos(p) - o).abs() < 1e-9);
    }
}

#[test]
fn s52_coradial() {
    let mut s = Sketch::new();
    let a = arc(&mut s, v2(0.0, 0.0), 5.0);
    let b = arc(&mut s, v2(1.0, 1.0), 7.0);
    assert_eq!(removes(&mut s, Constraint::Coradial { a, b }), 3);
    assert!(centre(&s, a).distance(centre(&s, b)) < 1e-9);
    assert!((s.radius(a) - s.radius(b)).abs() < 1e-9);
}

#[test]
fn s53_symmetric_points() {
    let mut s = Sketch::new();
    let axis = line(&mut s, v2(0.0, -10.0), v2(0.0, 10.0));
    let (a0, a1) = ends(&s, axis);
    s.add_constraint(Constraint::Fix { p: a0, x: 0.0, y: -10.0 });
    s.add_constraint(Constraint::Fix { p: a1, x: 0.0, y: 10.0 });
    let p = s.add_point(v2(3.0, 2.0));
    let q = s.add_point(v2(-5.0, 4.0));
    assert_eq!(removes(&mut s, Constraint::Symmetric { p, q, line: axis }), 2);
    let (pp, qq) = (s.pos(p), s.pos(q));
    assert!((pp.x + qq.x).abs() < 1e-9 && (pp.y - qq.y).abs() < 1e-9, "{pp:?} {qq:?}");
}

#[test]
fn s54_intersection() {
    let mut s = Sketch::new();
    let a = line(&mut s, v2(0.0, 0.0), v2(10.0, 0.0));
    let b = line(&mut s, v2(7.0, -5.0), v2(7.0, 5.0));
    for e in [a, b] {
        let (p, q) = ends(&s, e);
        for k in [p, q] {
            let at = s.pos(k);
            s.add_constraint(Constraint::Fix { p: k, x: at.x, y: at.y });
        }
    }
    let p = s.add_point(v2(3.0, 4.0));
    assert_eq!(removes(&mut s, Constraint::Intersection { p, a, b }), 2);
    assert!(s.pos(p).distance(v2(7.0, 0.0)) < 1e-9);
}

#[test]
fn s55_fixed_line_keeps_its_infinite_line() {
    let mut s = Sketch::new();
    let l = line(&mut s, v2(0.0, 0.0), v2(10.0, 0.0));
    assert_eq!(removes(&mut s, Constraint::FixLine { line: l, x: 0.0, y: 0.0, degrees: 0.0 }), 2);
    let (_, b) = ends(&s, l);
    assert!(s.drag_point(b, v2(14.0, 3.0)).converged);
    let p = s.pos(b);
    assert!(p.y.abs() < 1e-9 && (p.x - 14.0).abs() < 1e-6, "{p:?}");
}

#[test]
fn s56_arc_to_arc_tangent() {
    let mut s = Sketch::new();
    let c1 = s.add_point(v2(0.0, 0.0));
    let a = s.add_circle(c1, 5.0);
    let c2 = s.add_point(v2(10.0, 0.0));
    let b = s.add_circle(c2, 3.0);
    s.add_constraint(Constraint::Coincident { p: c1, q: ORIGIN });
    s.add_constraint(Constraint::Radius { entity: a, value: 5.0 });
    s.add_constraint(Constraint::Radius { entity: b, value: 3.0 });
    s.add_constraint(Constraint::HorizontalPoints { p: c2, q: ORIGIN });
    assert!(!s.circles_inside(a, b));
    assert_eq!(removes(&mut s, Constraint::TangentCircles { a, b, inside: false }), 1);
    assert!(s.pos(c2).distance(v2(8.0, 0.0)) < 1e-9, "{:?}", s.pos(c2));
}

#[test]
fn s57_freedoms_each_relation_removes() {
    // Merge points: two free points become one.
    let mut s = Sketch::new();
    let p = s.add_point(v2(1.0, 2.0));
    let q = s.add_point(v2(4.0, 6.0));
    let before = s.solve().dof;
    s.merge_points(p, q).unwrap();
    assert_eq!(before - s.solve().dof, 2);
    assert_eq!(s.points.len(), 2);

    // Fix on a circle: centre and radius.
    let mut s = Sketch::new();
    let c = s.add_point(v2(1.0, 1.0));
    let circle = s.add_circle(c, 4.0);
    assert_eq!(removes(&mut s, Constraint::FixRound { entity: circle, x: 1.0, y: 1.0, r: 4.0 }), 3);
    // On an arc the ends still slide round the circle.
    let mut s = Sketch::new();
    let e = arc(&mut s, v2(0.0, 0.0), 5.0);
    assert_eq!(removes(&mut s, Constraint::FixRound { entity: e, x: 0.0, y: 0.0, r: 5.0 }), 3);
    assert_eq!(s.solve().dof, 2);
}

#[test]
fn merging_drops_relations_of_a_point_to_itself() {
    let mut s = Sketch::new();
    let a = line(&mut s, v2(0.0, 0.0), v2(10.0, 0.0));
    let b = line(&mut s, v2(10.0, 0.1), v2(10.0, 10.0));
    let (_, p) = ends(&s, a);
    let (q, _) = ends(&s, b);
    s.add_constraint(Constraint::Distance { p, q, value: 0.1 });
    let horiz = s.add_constraint(Constraint::Horizontal { line: a });
    let map = s.merge_points(p, q).unwrap();
    assert_eq!(s.constraints.len(), 2, "the distance between the merged points goes");
    assert_eq!(map.constraints[horiz], Some(1));
    assert_eq!(s.entities_ending_at(map.point(p).unwrap()).len(), 2);
}

#[test]
fn deleting_entities_keeps_lone_points() {
    let mut s = Sketch::new();
    let l = line(&mut s, v2(0.0, 0.0), v2(10.0, 0.0));
    let lone = s.add_point(v2(5.0, 5.0));
    s.add_constraint(Constraint::Fix { p: lone, x: 5.0, y: 5.0 });
    s.delete_entities(&[l]);
    assert_eq!(s.points.len(), 2, "the origin and the lone point");
    assert_eq!(s.constraints.len(), 2);
}

#[test]
fn new_relations_survive_serde() {
    let mut s = Sketch::new();
    let a = line(&mut s, v2(0.0, 0.0), v2(10.0, 0.0));
    let b = line(&mut s, v2(20.0, 1.0), v2(30.0, 3.0));
    s.add_constraint(Constraint::Collinear { a, b });
    s.add_constraint(Constraint::Offset { base: a, entity: b, value: 2.0 });
    s.add_constraint(Constraint::PatternPoint { seed: 1, inst: 3, d1: a, i: 2, d2: b, j: 0 });
    let json = serde_json::to_string(&s).unwrap();
    let back: Sketch = serde_json::from_str(&json).unwrap();
    assert_eq!(s, back);
    // A sketch saved before these relations existed still loads.
    let old =
        r#"{"points":[{"x":0.0,"y":0.0}],"entities":[],"constraints":[{"Fix":{"p":0,"x":0.0,"y":0.0}}]}"#;
    assert_eq!(serde_json::from_str::<Sketch>(old).unwrap(), Sketch::new());
}
