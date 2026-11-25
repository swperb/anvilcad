//! Issue #207: spline sketch entities reach the features exactly, and STEP keeps them as
//! B-spline curves (`docs/re/sketch-entities.md`, "Splines", "Features and export").

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use anvil_kernel::v2;
use anvil_model::*;
use anvil_sketch::Geom;

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

fn exact_mass(st: &BuildState) -> anvil_brep::mass::MassProperties {
    let exact = st.exact.as_ref().unwrap_or_else(|| panic!("not exact: {:?}", st.faceted_from));
    assert_eq!(exact.len(), 1);
    anvil_brep::mass::exact_properties(&exact[0]).expect("mass")
}

fn near(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol * b.abs().max(1.0)
}

fn spline_points(sk: &anvil_sketch::Sketch, e: usize) -> Vec<usize> {
    match &sk.entities[e].geom {
        Geom::Spline { pts, .. } | Geom::ControlSpline { pts } => pts.clone(),
        _ => panic!("not a spline"),
    }
}

/// The arch over control points (0, 0), (0, h), (w, h), (w, 0), closed by its base:
/// area 3 w h / 5 and centroid height 9 h / 28.
fn arch(doc: &mut Document, w: f64, h: f64) -> u32 {
    let mut sf = SketchFeature::new(PlaneRef::Front);
    let s = &mut sf.sketch;
    let e = s.add_control_spline(&[v2(0.0, 0.0), v2(0.0, h), v2(w, h), v2(w, 0.0)]).unwrap();
    let p = spline_points(s, e);
    s.add_line(p[3], p[0]);
    assert!(s.solve().converged);
    doc.add_feature(FeatureKind::Sketch(sf))
}

#[test]
fn extruded_control_spline_is_exact() {
    let mut doc = Document::new();
    let sk = arch(&mut doc, 40.0, 30.0);
    let ex = extrude(&mut doc, sk, 10.0);
    let st = doc.rebuild();
    assert!(st.errors.is_empty(), "{:?}", st.errors);
    assert!(st.exact_features.contains(&ex));
    let m = exact_mass(&st);
    assert!(near(m.volume, 7200.0, 1e-8), "{}", m.volume);
    let want = glam::DVec3::new(20.0, 9.0 * 30.0 / 28.0, 5.0);
    assert!(m.centroid.distance(want) < 1e-6, "{:?} vs {want:?}", m.centroid);
}

/// A spline through four points closed by a line: the exact volume is the profile's exact
/// area (the kernel's closed form for its Bézier spans) times the depth, and the side of
/// each span is its own B-spline face.
#[test]
fn extruded_through_spline_is_exact() {
    let mut doc = Document::new();
    let mut sf = SketchFeature::new(PlaneRef::Front);
    let s = &mut sf.sketch;
    let e = s.add_spline(&[v2(0.0, 0.0), v2(10.0, 15.0), v2(25.0, 12.0), v2(40.0, 0.0)]).unwrap();
    let p = spline_points(s, e);
    s.add_line(p[3], p[0]);
    let area = s.profiles().unwrap()[0].area();
    let sk = doc.add_feature(FeatureKind::Sketch(sf));
    extrude(&mut doc, sk, 5.0);
    let st = doc.rebuild();
    assert!(st.errors.is_empty(), "{:?}", st.errors);
    let m = exact_mass(&st);
    assert!(near(m.volume, 5.0 * area, 1e-8), "{} vs {}", m.volume, 5.0 * area);
    let exact = st.exact.as_ref().unwrap();
    let bsplines =
        exact[0].faces.iter().filter(|f| matches!(f.surface, anvil_brep::geom::Surface::BSpline(_))).count();
    assert_eq!(bsplines, 3, "one side face per span");
}

#[test]
fn spline_exports_to_step_as_a_b_spline_curve() {
    let mut doc = Document::new();
    let sk = arch(&mut doc, 40.0, 30.0);
    extrude(&mut doc, sk, 10.0);
    let st = doc.rebuild();
    let text = anvil_model::step::exact_step(&st, "spline").expect("exact STEP");
    let curves: Vec<&str> = text.lines().filter(|l| l.contains("B_SPLINE_CURVE_WITH_KNOTS(")).collect();
    assert_eq!(curves.len(), 2, "one at each cap: {curves:?}");
    for l in &curves {
        assert!(l.contains("'',3,("), "cubic: {l}");
    }
    assert!(text.contains("B_SPLINE_SURFACE_WITH_KNOTS("), "the side face");
    // Read back: still exact, same volume, and the cap edges are still B-spline curves.
    let imp = anvil_step::import(&text).expect("import");
    let body = imp.bodies[0].exact.as_ref().unwrap_or_else(|| panic!("{:?}", imp.bodies[0].not_exact));
    let v = anvil_brep::mass::exact_properties(body).expect("mass").volume;
    assert!(near(v, 7200.0, 1e-7), "{v}");
    let splines: Vec<&anvil_brep::spline::Spline> = body
        .edges
        .iter()
        .filter_map(|e| match &e.curve {
            anvil_brep::geom::Curve::Spline(s) => Some(&**s),
            _ => None,
        })
        .collect();
    assert_eq!(splines.len(), 2);
    for sp in splines {
        assert_eq!(sp.degree, 3);
        assert_eq!(sp.points.len(), 4);
    }
}

#[test]
fn revolved_spline_builds_on_the_faceted_kernel() {
    // The exact kernel does not revolve splines yet (#143): the part is built faceted and
    // says which feature made it so, with no error.
    let mut doc = Document::new();
    let mut sf = SketchFeature::new(PlaneRef::Front);
    let s = &mut sf.sketch;
    let e = s.add_control_spline(&[v2(10.0, 0.0), v2(10.0, 30.0), v2(50.0, 30.0), v2(50.0, 0.0)]).unwrap();
    let p = spline_points(s, e);
    s.add_line(p[3], p[0]);
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
    // Pappus: 2π times the centroid's distance from the axis times the area. The arch's
    // area is 3/5 of 40 by 30 and its centroid is at x = 30.
    let want = 2.0 * std::f64::consts::PI * 30.0 * 720.0;
    let v = st.body.mass_properties().volume;
    assert!((v - want).abs() / want < 0.01, "{v} vs {want}");
    assert!(st.exact.is_none());
    assert_eq!(st.faceted_from.map(|(f, _)| f), Some(rv));
}

#[test]
fn thin_extrude_of_a_spline_says_why_it_fails() {
    let mut doc = Document::new();
    let mut sf = SketchFeature::new(PlaneRef::Front);
    sf.sketch.add_spline(&[v2(0.0, 0.0), v2(10.0, 8.0), v2(20.0, 0.0)]).unwrap();
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
    let e = st.errors.get(&ex).cloned().expect("a thin spline fails");
    let m = e.message();
    assert!(m.contains("splines cannot be offset yet"), "{m}");
    assert!(e.suggestion.contains("lines, arcs and circles"), "{m}");
}
