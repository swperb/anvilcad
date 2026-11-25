//! Issue #69: ellipses and partial ellipses in sketches reach the features exactly
//! (`docs/re/sketch-entities.md`, "Ellipses", testable statements 28 to 31).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use anvil_kernel::{v2, Vec2};
use anvil_model::*;
use std::f64::consts::{FRAC_PI_4, PI};

fn extrude(doc: &mut Document, sketch: u32, depth: f64) -> u32 {
    doc.add_feature(FeatureKind::Extrude(ExtrudeFeature {
        sketch,
        end: EndCondition::Blind,
        depth: Dim::Value(depth),
        reverse: false,
        op: BodyOp::Boss,
        ..Default::default()
    }))
}

/// Exact volume and centroid of the part's only exact body.
fn exact_mass(st: &BuildState) -> anvil_brep::mass::MassProperties {
    let exact = st.exact.as_ref().unwrap_or_else(|| panic!("not exact: {:?}", st.faceted_from));
    assert_eq!(exact.len(), 1);
    anvil_brep::mass::exact_properties(&exact[0]).expect("mass")
}

fn near(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol * b.abs().max(1.0)
}

#[test]
fn extruded_ellipse_is_exact() {
    let mut doc = Document::new();
    let mut sf = SketchFeature::new(PlaneRef::Front);
    sf.sketch.add_ellipse(v2(0.0, 0.0), v2(30.0, 0.0), v2(12.0, 20.0)).unwrap();
    let sk = doc.add_feature(FeatureKind::Sketch(sf));
    let ex = extrude(&mut doc, sk, 10.0);
    let st = doc.rebuild();
    assert!(st.errors.is_empty(), "{:?}", st.errors);
    assert!(st.exact_features.contains(&ex));
    let m = exact_mass(&st);
    assert!(near(m.volume, 6000.0 * PI, 1e-8), "{}", m.volume);
    assert!(m.centroid.distance(glam::DVec3::new(0.0, 0.0, 5.0)) < 1e-7, "{:?}", m.centroid);
    // Side area: the perimeter of the ellipse times the depth. Ramanujan's second
    // approximation is good to far below the tolerance for these radii.
    let (a, b) = (30.0f64, 20.0f64);
    let h = ((a - b) / (a + b)).powi(2);
    let perimeter = PI * (a + b) * (1.0 + 3.0 * h / (10.0 + (4.0 - 3.0 * h).sqrt()));
    assert!(near(m.area, 2.0 * 600.0 * PI + perimeter * 10.0, 1e-7), "{}", m.area);
}

#[test]
fn rotated_ellipse_with_a_hole_is_exact() {
    // An ellipse at 45 degrees with radii 20 and 10 about (5, 5), with a circular hole of
    // radius 3 at its centre: area 200π − 9π.
    let mut doc = Document::new();
    let mut sf = SketchFeature::new(PlaneRef::Front);
    let c = v2(5.0, 5.0);
    sf.sketch.add_ellipse_sized(c, 20.0, 10.0, FRAC_PI_4).unwrap();
    let pc = sf.sketch.add_point(c);
    sf.sketch.add_circle(pc, 3.0);
    let sk = doc.add_feature(FeatureKind::Sketch(sf));
    extrude(&mut doc, sk, 4.0);
    let st = doc.rebuild();
    assert!(st.errors.is_empty(), "{:?}", st.errors);
    let m = exact_mass(&st);
    assert!(near(m.volume, 191.0 * PI * 4.0, 1e-8), "{}", m.volume);
    assert!(m.centroid.distance(glam::DVec3::new(5.0, 5.0, 2.0)) < 1e-7, "{:?}", m.centroid);
}

/// A half ellipse of radii 30 and 20, closed by its long axis: area 300π, centroid
/// 4·20/(3π) above the axis.
fn half_ellipse(doc: &mut Document) -> u32 {
    let mut sf = SketchFeature::new(PlaneRef::Front);
    let s = &mut sf.sketch;
    let ids = s.add_elliptic_arc(v2(0.0, 0.0), 30.0, 20.0, 0.0, v2(1.0, 0.0), v2(-1.0, 0.0)).unwrap();
    let (a, b) = ids.ends.unwrap();
    s.add_line(b, a);
    assert!(s.solve().converged);
    doc.add_feature(FeatureKind::Sketch(sf))
}

#[test]
fn extruded_half_ellipse_is_exact() {
    let mut doc = Document::new();
    let sk = half_ellipse(&mut doc);
    extrude(&mut doc, sk, 2.0);
    let st = doc.rebuild();
    assert!(st.errors.is_empty(), "{:?}", st.errors);
    let m = exact_mass(&st);
    assert!(near(m.volume, 600.0 * PI, 1e-8), "{}", m.volume);
    let cy = 80.0 / (3.0 * PI);
    assert!(m.centroid.distance(glam::DVec3::new(0.0, cy, 1.0)) < 1e-7, "{:?} vs {cy}", m.centroid);
}

#[test]
fn ellipse_exports_to_step_as_an_ellipse() {
    let mut doc = Document::new();
    let mut sf = SketchFeature::new(PlaneRef::Front);
    sf.sketch.add_ellipse(v2(0.0, 0.0), v2(30.0, 0.0), v2(0.0, 20.0)).unwrap();
    let sk = doc.add_feature(FeatureKind::Sketch(sf));
    extrude(&mut doc, sk, 10.0);
    let st = doc.rebuild();
    let text = anvil_model::step::exact_step(&st, "ellipse").expect("exact STEP");
    let ellipses: Vec<&str> = text.lines().filter(|l| l.contains("ELLIPSE(")).collect();
    assert_eq!(ellipses.len(), 2, "one ellipse at each cap: {ellipses:?}");
    for l in &ellipses {
        assert!(l.contains(",30.") && l.contains(",20."), "{l}");
    }
    assert!(text.contains("SURFACE_OF_LINEAR_EXTRUSION") || text.contains("B_SPLINE_SURFACE"), "side face");
    // Read back: still exact, same volume, and the cap edges are still ellipses.
    let imp = anvil_step::import(&text).expect("import");
    let body = imp.bodies[0].exact.as_ref().unwrap_or_else(|| panic!("{:?}", imp.bodies[0].not_exact));
    let v = anvil_brep::mass::exact_properties(body).expect("mass").volume;
    assert!(near(v, 6000.0 * PI, 1e-7), "{v}");
    let curves =
        body.edges.iter().filter(|e| matches!(e.curve, anvil_brep::geom::Curve::Ellipse { .. })).count();
    assert_eq!(curves, 2);
}

#[test]
fn revolved_ellipse_builds_on_the_faceted_kernel() {
    // An ellipse about (40, 0) with radii 10 and 5, turned about the y axis: Pappus gives
    // 2π · 40 · 50π. The exact kernel does not revolve ellipses yet (#143), so the part is
    // built faceted, close to the exact value.
    let mut doc = Document::new();
    let mut sf = SketchFeature::new(PlaneRef::Front);
    let s = &mut sf.sketch;
    s.add_ellipse_sized(v2(40.0, 0.0), 10.0, 5.0, 0.0).unwrap();
    let (a, b) = (s.add_point(v2(0.0, -20.0)), s.add_point(v2(0.0, 20.0)));
    let axis = s.add_line(a, b);
    s.entities[axis].construction = true;
    let sk = doc.add_feature(FeatureKind::Sketch(sf));
    let rv = doc.add_feature(FeatureKind::Revolve(RevolveFeature {
        sketch: sk,
        axis,
        angle: Dim::Value(360.0),
        op: BodyOp::Boss,
        ..Default::default()
    }));
    let st = doc.rebuild();
    assert!(st.errors.is_empty(), "{:?}", st.errors);
    let want = 2.0 * PI * 40.0 * 50.0 * PI;
    let v = st.body.mass_properties().volume;
    assert!((v - want).abs() / want < 0.01, "{v} vs {want}");
    if st.exact.is_none() {
        assert_eq!(st.faceted_from.map(|(f, _)| f), Some(rv));
    }
}

#[test]
fn thin_extrude_of_an_ellipse_says_why_it_fails() {
    let mut doc = Document::new();
    let mut sf = SketchFeature::new(PlaneRef::Front);
    sf.sketch.add_ellipse_sized(v2(0.0, 0.0), 10.0, 5.0, 0.0).unwrap();
    let sk = doc.add_feature(FeatureKind::Sketch(sf));
    let ex = doc.add_feature(FeatureKind::Extrude(ExtrudeFeature {
        sketch: sk,
        end: EndCondition::Blind,
        depth: Dim::Value(10.0),
        op: BodyOp::Boss,
        thin: Some(ThinWall {
            kind: ThinKind::OneDirection,
            thickness: Dim::Value(1.0),
            thickness2: Dim::Value(1.0),
            reverse: false,
            fillet: None,
            cap: None,
        }),
        ..Default::default()
    }));
    let st = doc.rebuild();
    let e = st.errors.get(&ex).cloned().expect("a thin ellipse fails");
    let m = e.message();
    assert!(m.contains("ellipses cannot be offset yet"), "{m}");
    assert!(e.suggestion.contains("lines, arcs and circles"), "{m}");
}

#[test]
fn moved_sketch_keeps_its_exact_area() {
    // Dragging the centre of a dimensioned ellipse moves it whole.
    let mut sf = SketchFeature::new(PlaneRef::Front);
    let s = &mut sf.sketch;
    let ids = s.add_ellipse(v2(0.0, 0.0), v2(20.0, 0.0), v2(0.0, 10.0)).unwrap();
    s.add_constraint(anvil_sketch::Constraint::Distance { p: ids.center, q: ids.axis1, value: 20.0 });
    s.add_constraint(anvil_sketch::Constraint::Distance { p: ids.center, q: ids.axis2, value: 10.0 });
    s.add_constraint(anvil_sketch::Constraint::HorizontalPoints { p: ids.center, q: ids.axis1 });
    let r = s.drag_point(ids.center, v2(5.0, 5.0));
    assert!(r.converged, "{r:?}");
    assert_eq!(r.dof, 2);
    let p = s.profiles().unwrap();
    assert!(near(p[0].area(), 200.0 * PI, 1e-9), "{}", p[0].area());
    assert!(s.pos(ids.center).distance(v2(5.0, 5.0)) < 1e-6);
    let _: Vec2 = s.pos(ids.axis1);
}
