//! Issue #174: failed sketches, extrudes, revolves, sweeps, lofts, thin walls and ribs say
//! what went wrong and what to try, built through a real rebuild and checked word for word.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use anvil_kernel::v2;
use anvil_model::rib::RibFeature;
use anvil_model::sweep::SweepFeature;
use anvil_model::*;

/// The cause and suggestion of feature `id`'s error, after checking the whole message.
fn error(doc: &mut Document, id: u32) -> (String, String) {
    let st = doc.rebuild();
    let e =
        st.errors.get(&id).cloned().unwrap_or_else(|| panic!("feature {id} built; errors: {:?}", st.errors));
    let name = doc.feature(id).map(|f| f.name.clone()).unwrap();
    let m = e.message();
    assert!(m.starts_with(&format!("{name} could not be built: ")), "{m}");
    assert!(e.suggestion.ends_with('.') && !e.cause.ends_with('.'), "{m}");
    for t in ["{", "}", "::", "Some(", "\""] {
        assert!(!m.contains(t), "{m}");
    }
    (e.cause, e.suggestion)
}

fn rect_sketch(doc: &mut Document, plane: PlaneRef) -> u32 {
    let mut sf = SketchFeature::new(plane);
    sf.sketch.add_rectangle(v2(0.0, 0.0), v2(40.0, 30.0));
    doc.add_feature(FeatureKind::Sketch(sf))
}

fn extrude(doc: &mut Document, sketch: u32, f: impl FnOnce(&mut ExtrudeFeature)) -> u32 {
    let mut e = ExtrudeFeature {
        sketch,
        end: EndCondition::Blind,
        depth: Dim::Value(20.0),
        reverse: false,
        op: BodyOp::Boss,
        ..Default::default()
    };
    f(&mut e);
    doc.add_feature(FeatureKind::Extrude(e))
}

fn pair(c: &str, s: &str) -> (String, String) {
    (c.into(), s.into())
}

#[test]
fn extrude_errors_say_what_to_try() {
    let mut doc = Document::new();
    let sk = rect_sketch(&mut doc, PlaneRef::Top);
    let zero = extrude(&mut doc, sk, |e| e.depth = Dim::Value(0.0));
    assert_eq!(
        error(&mut doc, zero),
        pair("the depth is 0 mm, and must be greater than zero", "Give it a depth greater than 0.")
    );

    let mut doc = Document::new();
    let sk = rect_sketch(&mut doc, PlaneRef::Top);
    let through = extrude(&mut doc, sk, |e| e.end = EndCondition::ThroughAll);
    assert_eq!(
        error(&mut doc, through),
        pair(
            "there is no body yet for Through All to pass through",
            "Use Blind with a depth, or move this feature after one that makes a body."
        )
    );

    let mut doc = Document::new();
    let sk = rect_sketch(&mut doc, PlaneRef::Top);
    let up_to = extrude(&mut doc, sk, |e| e.end = EndCondition::UpToSurface);
    assert_eq!(
        error(&mut doc, up_to),
        pair(
            "no plane or face is picked for it to end at",
            "Edit the feature and pick the plane or face to end at."
        )
    );

    let mut doc = Document::new();
    let sk = rect_sketch(&mut doc, PlaneRef::Top);
    let steep = extrude(&mut doc, sk, |e| e.draft = Some(Draft { angle: Dim::Value(95.0), outward: false }));
    assert_eq!(
        error(&mut doc, steep),
        pair(
            "the draft angle must be between 0 and 90 degrees",
            "Give the draft an angle between 0 and 90 degrees, or turn off Draft."
        )
    );
}

#[test]
fn a_feature_on_a_failed_or_deleted_sketch_says_so() {
    let mut doc = Document::new();
    let sk = rect_sketch(&mut doc, PlaneRef::Top);
    // Two conflicting lengths for the rectangle's bottom line.
    if let Some(FeatureKind::Sketch(sf)) = doc.feature_mut(sk).map(|f| &mut f.kind) {
        let bottom = 0;
        sf.sketch.add_constraint(anvil_sketch::Constraint::Length { line: bottom, value: 40.0 });
        sf.sketch.add_constraint(anvil_sketch::Constraint::Length { line: bottom, value: 50.0 });
    }
    let ex = extrude(&mut doc, sk, |_| {});
    let (cause, suggestion) = error(&mut doc, sk);
    assert!(cause.starts_with("it is over defined: "), "{cause}");
    assert!(suggestion.starts_with("Edit the sketch: the conflicting relations"), "{suggestion}");
    assert_eq!(
        error(&mut doc, ex),
        pair(
            "the sketch it uses failed or is suppressed",
            "Fix or unsuppress that sketch first; a failed one is marked in the tree."
        )
    );

    let mut doc = Document::new();
    let ex = extrude(&mut doc, 999, |_| {});
    assert_eq!(
        error(&mut doc, ex),
        pair(
            "the sketch it uses has been deleted",
            "Edit the feature and pick another sketch, or undo the deletion."
        )
    );
}

#[test]
fn revolve_errors_say_what_to_try() {
    let revolve = |axis: usize, angle: f64| {
        let mut doc = Document::new();
        let mut sf = SketchFeature::new(PlaneRef::Front);
        let lines = sf.sketch.add_rectangle(v2(5.0, 0.0), v2(15.0, 10.0));
        let (a, b) = (sf.sketch.add_point(v2(0.0, -5.0)), sf.sketch.add_point(v2(0.0, 15.0)));
        let axis_line = sf.sketch.add_line(a, b);
        let c = sf.sketch.add_point(v2(30.0, 5.0));
        let circle = sf.sketch.add_circle(c, 2.0);
        let sk = doc.add_feature(FeatureKind::Sketch(sf));
        let axis = if axis == 0 { axis_line } else { circle };
        let _ = lines;
        let id = doc.add_feature(FeatureKind::Revolve(RevolveFeature {
            sketch: sk,
            axis,
            angle: Dim::Value(angle),
            op: BodyOp::Boss,
            kind: RevolveKind::OneDirection,
            reverse: false,
            thin: None,
            flip_side: false,
        }));
        (doc, id)
    };
    let (mut doc, id) = revolve(0, 400.0);
    assert_eq!(
        error(&mut doc, id),
        pair(
            "the angle is 400 degrees, and must be more than 0 and at most 360",
            "Give it an angle from just above 0 to 360 degrees."
        )
    );
    let (mut doc, id) = revolve(1, 360.0);
    assert_eq!(
        error(&mut doc, id),
        pair(
            "its axis is not a line in its sketch",
            "Edit the feature and pick a straight line or centerline of the sketch as the axis."
        )
    );
}

#[test]
fn sweep_loft_thin_and_rib_errors_say_what_to_try() {
    let mut doc = Document::new();
    let path = rect_sketch(&mut doc, PlaneRef::Top);
    let sweep =
        doc.add_feature(FeatureKind::Sweep(SweepFeature { path, profile: None, ..Default::default() }));
    assert_eq!(
        error(&mut doc, sweep),
        pair(
            "no profile is picked",
            "Edit the feature and pick the profile's sketch, or turn on Round profile and give it a diameter."
        )
    );

    let mut doc = Document::new();
    let one = rect_sketch(&mut doc, PlaneRef::Top);
    let loft = doc.add_feature(FeatureKind::Loft(LoftFeature { profiles: vec![one], op: BodyOp::Boss }));
    assert_eq!(
        error(&mut doc, loft),
        pair(
            "a loft needs at least two profiles",
            "Edit the feature and pick two or more profile sketches, in order."
        )
    );

    let mut doc = Document::new();
    let sk = rect_sketch(&mut doc, PlaneRef::Top);
    let thin = extrude(&mut doc, sk, |e| {
        e.thin = Some(ThinWall {
            kind: ThinKind::OneDirection,
            thickness: Dim::Value(0.0),
            thickness2: Dim::Value(0.0),
            reverse: false,
            fillet: None,
            cap: None,
        })
    });
    assert_eq!(
        error(&mut doc, thin),
        pair(
            "the wall thickness must be greater than zero",
            "Give the thin wall a Thickness greater than 0."
        )
    );

    let mut doc = Document::new();
    let sk = rect_sketch(&mut doc, PlaneRef::Top);
    let rib = doc.add_feature(FeatureKind::Rib(RibFeature {
        sketch: sk,
        thickness: Dim::Value(2.0),
        sides: Default::default(),
        normal_to_sketch: false,
        flip: false,
        natural: false,
    }));
    assert_eq!(
        error(&mut doc, rib),
        pair(
            "there is no body yet for the rib to meet",
            "Move the rib after the features that make the part it braces."
        )
    );
}
