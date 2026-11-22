//! The Part Check contract: the committed schema matches the types, and a synthetic part
//! built by AnvilCAD gives the committed golden result.
//!
//! Set `ANVIL_BLESS=1` to rewrite `schema/` and `fixtures/` after an intended change, then
//! review the diff: any change to the schema file is a contract change for Kerfa.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use anvil_kernel::{v2, v3};
use anvil_model::{
    BodyOp, Dim, Document, EndCondition, ExtrudeFeature, FeatureKind, PlaneRef, SketchFeature,
};
use anvil_partcheck::{
    Feature, FlagSeverity, PartCheckError, PartCheckResult, Request, SCHEMA_ID, SCHEMA_VERSION, TimeStatus,
    check_step, json_schema,
};
use std::path::PathBuf;

fn here(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(rel)
}

fn bless() -> bool {
    std::env::var_os("ANVIL_BLESS").is_some()
}

/// Compares `text` with the committed file, or writes it when blessing.
fn golden(rel: &str, text: &str) {
    let path = here(rel);
    if bless() {
        std::fs::write(&path, text).unwrap();
        return;
    }
    let want = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("{rel} is missing; run with ANVIL_BLESS=1 to create it"));
    assert_eq!(
        want.replace("\r\n", "\n"),
        text,
        "{rel} is out of date; run with ANVIL_BLESS=1 and review the diff"
    );
}

#[test]
fn committed_schema_matches_the_types() {
    let text = serde_json::to_string_pretty(&json_schema()).unwrap() + "\n";
    golden("schema/part-check.v1.schema.json", &text);
}

/// 80 x 60 x 20 plate centred on the origin, with a 40 x 30 pocket 8 deep in the middle
/// of the top face and one 6 mm through hole near a corner. Entirely synthetic.
fn plate_step() -> String {
    let mut doc = Document::new();
    let mut base = SketchFeature::new(PlaneRef::Front);
    base.sketch.add_rectangle(v2(-40.0, -30.0), v2(40.0, 30.0));
    let base = doc.add_feature(FeatureKind::Sketch(base));
    doc.add_feature(FeatureKind::Extrude(ExtrudeFeature {
        sketch: base,
        end: EndCondition::Blind,
        depth: Dim::Value(20.0),
        op: BodyOp::Boss,
        ..Default::default()
    }));
    let st = doc.rebuild();
    assert!(st.errors.is_empty(), "{:?}", st.errors);
    let top = st.body.face_at(v3(0.0, 0.0, 20.0), 1e-6).expect("top face");

    let mut pocket = SketchFeature::new(PlaneRef::Face(top));
    pocket.sketch.add_rectangle(v2(-20.0, -15.0), v2(20.0, 15.0));
    let pocket = doc.add_feature(FeatureKind::Sketch(pocket));
    doc.add_feature(FeatureKind::Extrude(ExtrudeFeature {
        sketch: pocket,
        end: EndCondition::Blind,
        depth: Dim::Value(8.0),
        op: BodyOp::Cut,
        ..Default::default()
    }));

    let mut hole = SketchFeature::new(PlaneRef::Face(top));
    let c = hole.sketch.add_point(v2(30.0, 20.0));
    hole.sketch.add_circle(c, 3.0);
    let hole = doc.add_feature(FeatureKind::Sketch(hole));
    doc.add_feature(FeatureKind::Extrude(ExtrudeFeature {
        sketch: hole,
        end: EndCondition::ThroughAll,
        depth: Dim::Value(1.0),
        op: BodyOp::Cut,
        ..Default::default()
    }));
    let st = doc.rebuild();
    assert!(st.errors.is_empty(), "{:?}", st.errors);
    anvil_model::step::exact_step(&st, "partcheck-plate").expect("the plate builds exactly")
}

fn request(material: &str, quantity: u32) -> Request {
    Request { material: material.into(), quantity, file_name: Some("plate-pocket-hole.step".into()) }
}

#[test]
fn synthetic_plate_gives_the_golden_result() {
    let step = plate_step();
    golden("fixtures/plate-pocket-hole.step", &step);
    let r = check_step(step.as_bytes(), &request("6061", 10)).unwrap();

    assert_eq!(r.schema, SCHEMA_ID);
    assert_eq!(r.schema_version, SCHEMA_VERSION);
    assert_eq!(r.part.fingerprint.algorithm, "sha256");
    assert_eq!(r.part.fingerprint.value.len(), 64);
    assert_eq!((r.part.bodies, r.part.exact_bodies), (1, 1));
    assert!(r.request.material.matched.as_deref().unwrap().contains("6061"));
    assert_eq!(r.bounding_box.size, [80.0, 60.0, 20.0]);
    assert_eq!(r.stock.size, [86.0, 66.0, 22.0]);

    // Volume: plate less the pocket and the hole (the hole is faceted, so within 0.1 %).
    let exact = 80.0 * 60.0 * 20.0 - 40.0 * 30.0 * 8.0 - std::f64::consts::PI * 9.0 * 20.0;
    assert!((r.volume_mm3 - exact).abs() / exact < 1e-3, "{}", r.volume_mm3);

    let holes: Vec<_> = r.features.iter().filter(|f| matches!(f, Feature::Hole { .. })).collect();
    assert_eq!(holes, [&Feature::Hole { count: 1, diameter_mm: 6.0, depth_mm: 20.0, through: true }]);
    assert!(
        r.features.contains(&Feature::Pocket {
            count: 1,
            depth_mm: 8.0,
            area_mm2: 1200.0,
            open: false,
            vertical_walls: true
        }),
        "{:?}",
        r.features
    );

    let t = r.machine_time.as_ref().expect("an estimate");
    assert_eq!(t.status, TimeStatus::Unvalidated);
    assert!(t.per_part_min > 0.0);
    assert!((t.total_min - 10.0 * t.per_part_min).abs() < 0.01);

    // The engine version changes with every release and is not part of the contract, so
    // the golden file holds a placeholder rather than going out of date at each one.
    assert_eq!(r.engine.version, env!("CARGO_PKG_VERSION"));
    let mut stable = r.clone();
    stable.engine.version = "0.0.0".into();
    let json = serde_json::to_string_pretty(&stable).unwrap() + "\n";
    golden("fixtures/plate-pocket-hole.v1.json", &json);
}

#[test]
fn golden_result_reads_back_as_the_current_type() {
    let text = std::fs::read_to_string(here("fixtures/plate-pocket-hole.v1.json")).unwrap();
    let r: PartCheckResult = serde_json::from_str(&text).unwrap();
    assert_eq!(r.schema_version, SCHEMA_VERSION);
}

#[test]
fn same_file_same_fingerprint_and_result() {
    let step = plate_step();
    let a = check_step(step.as_bytes(), &request("6061", 1)).unwrap();
    let b = check_step(step.as_bytes(), &request("6061", 1)).unwrap();
    assert_eq!(a, b);
    let other = check_step(format!("{step}\n").as_bytes(), &request("6061", 1)).unwrap();
    assert_ne!(a.part.fingerprint, other.part.fingerprint);
}

#[test]
fn unknown_material_gives_flags_but_no_time() {
    let r = check_step(plate_step().as_bytes(), &request("unobtainium", 1)).unwrap();
    assert_eq!(r.request.material.matched, None);
    assert_eq!(r.mass_g, None);
    assert!(r.machine_time.is_none());
    assert!(r.warnings.iter().any(|w| w.contains("unobtainium")), "{:?}", r.warnings);
    assert!(!r.features.is_empty());
    // DFM flags carry only the three severities of the contract.
    assert!(
        r.dfm_flags
            .iter()
            .all(|f| matches!(f.severity, FlagSeverity::Error | FlagSeverity::Warning | FlagSeverity::Note))
    );
}

#[test]
fn bad_input_is_an_error_not_a_panic() {
    assert!(matches!(check_step(b"not a step file", &request("6061", 1)), Err(PartCheckError::Step(_))));
    assert!(matches!(check_step(&[0xff, 0xfe, 0x00], &request("6061", 1)), Err(PartCheckError::Step(_))));
    assert!(matches!(
        check_step(plate_step().as_bytes(), &request("6061", 0)),
        Err(PartCheckError::Quantity)
    ));
}
