//! Writes the format fixtures that `format_fixtures.rs` checks: parts, a template and an
//! assembly, each saved by AnvilCAD itself, with the values a rebuild gives recorded in a
//! `<file>.expected.json` beside it.
//!
//! Fixtures are only ever generated on purpose, into a new folder named after the build
//! that saves them, never over the committed ones: they are there to prove that files
//! saved by that build still open. When a format version goes up, save a new set with
//! the first build that writes it, and add the new feature types here when there are
//! any. Run it as (the folder is relative to `crates/anvil-model`, where cargo runs
//! tests)
//!
//! ```text
//! ANVIL_FIXTURE_DIR=tests/fixtures/<build> \
//! ANVIL_FIXTURE_BUILD="<release or commit, and how it was run>" \
//! cargo test -p anvil-model --test fixture_generator generate -- --ignored
//! ```
//!
//! To save fixtures with an older build, copy this file and `common/` into that build's
//! `crates/anvil-model/tests/` and run the same command there. It uses only APIs the
//! oldest such build (v0.1.0) has; keep it that way, or note what changed.
//!
//! `rerecord_expected_values` measures committed fixtures again with this build, for a
//! change that is meant to move rebuilt geometry; the files themselves never change.
//!
//! No part here comes from SOLIDWORKS or any other CAD system: every feature is built
//! from AnvilCAD's own API with round numbers.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use anvil_kernel::{v2, v3, EdgeRef, FaceId, Frame};
use anvil_model::assembly::{Assembly, MateEntity, MateRef, Parts, ASSEMBLY_FORMAT, ASSEMBLY_FORMAT_VERSION};
use anvil_model::mates::{Alignment, MateKind};
use anvil_model::*;
use anvil_sketch::{Constraint, Geom, Sketch, ORIGIN};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

fn val(x: f64) -> Dim {
    Dim::Value(x)
}

fn var(doc: &mut Document, name: &str, expr: &str) {
    doc.variables.push(GlobalVariable { name: name.into(), expr: expr.into(), comment: String::new() });
}

fn material(name: &str) -> Option<Material> {
    let m = Material::library().into_iter().find(|m| m.name == name);
    assert!(m.is_some(), "no material {name}");
    m
}

/// A `w` x `h` rectangle centred on the sketch origin, its width and height dimensioned.
/// Returns the width and height constraints.
fn centred_rect(s: &mut Sketch, w: f64, h: f64) -> (usize, usize) {
    let l = s.add_rectangle(v2(-w / 2.0, -h / 2.0), v2(w / 2.0, h / 2.0));
    let (&Geom::Line { a, .. }, &Geom::Line { a: c, .. }) = (&s.entities[l[0]].geom, &s.entities[l[2]].geom)
    else {
        panic!("rectangle sides are lines")
    };
    let diag = s.add_line(a, c);
    s.entities[diag].construction = true;
    s.add_constraint(Constraint::Midpoint { p: ORIGIN, line: diag });
    let wc = s.add_constraint(Constraint::Length { line: l[0], value: w });
    let hc = s.add_constraint(Constraint::Length { line: l[1], value: h });
    (wc, hc)
}

fn sketch(doc: &mut Document, plane: PlaneRef, f: impl FnOnce(&mut Sketch)) -> u32 {
    let mut sf = SketchFeature::new(plane);
    f(&mut sf.sketch);
    doc.add_feature(FeatureKind::Sketch(sf))
}

fn circle(s: &mut Sketch, x: f64, y: f64, d: f64) -> usize {
    let c = s.add_point(v2(x, y));
    let e = s.add_circle(c, d / 2.0);
    s.add_constraint(Constraint::Diameter { entity: e, value: d });
    e
}

fn boss(doc: &mut Document, sketch: u32, depth: f64) -> u32 {
    doc.add_feature(FeatureKind::Extrude(ExtrudeFeature { sketch, depth: val(depth), ..Default::default() }))
}

fn through_cut(doc: &mut Document, sketch: u32) -> u32 {
    doc.add_feature(FeatureKind::Extrude(ExtrudeFeature {
        sketch,
        end: EndCondition::ThroughAll,
        depth: val(0.0),
        op: BodyOp::Cut,
        ..Default::default()
    }))
}

fn end_cap(feature: u32) -> FaceId {
    FaceId { feature, index: FaceId::END_CAP }
}

/// A block with a chamfer, fillets, holes, pockets, patterns and a mirror: Sketch,
/// Extrude (boss and cut), Plane, Chamfer, Fillet, Hole, LinearPattern, Mirror and
/// SketchPattern. Driven by global variables, with a material and custom properties.
fn prismatic() -> (Document, u32) {
    let mut doc = Document::new();
    doc.material = material("6061 Alloy");
    doc.custom_properties[0].value = "Fixture: prismatic block".into();
    doc.custom_properties[1].value = "FX-0001".into();
    var(&mut doc, "width", "80");
    var(&mut doc, "thickness", "12");

    let mut sf = SketchFeature::new(PlaneRef::Front);
    let (wc, _) = centred_rect(&mut sf.sketch, 80.0, 50.0);
    sf.set_driven(wc, Some("width".into()));
    let base_sk = doc.add_feature(FeatureKind::Sketch(sf));
    let base = doc.add_feature(FeatureKind::Extrude(ExtrudeFeature {
        sketch: base_sk,
        depth: Dim::Expr("thickness".into()),
        ..Default::default()
    }));
    let top = PlaneRef::Face(end_cap(base));
    let side = |k: u32| FaceId::side(base, k);

    // Chamfer the top edge of the left side; round the two right-hand vertical corners.
    doc.add_feature(FeatureKind::Chamfer(ChamferFeature {
        edges: vec![EdgeRef::new(end_cap(base), side(3))],
        distance: val(1.5),
        kind: ChamferKind::EqualDistance,
        tangent_propagation: false,
    }));
    doc.add_feature(FeatureKind::Fillet(FilletFeature {
        edges: vec![EdgeRef::new(side(0), side(1)), EdgeRef::new(side(1), side(2))],
        radius: val(4.0),
        profile: FilletProfile::Circular,
        tangent_propagation: false,
    }));

    // A drilled hole, patterned along x.
    let hole_sk = sketch(&mut doc, top, |s| {
        s.add_point(v2(-25.0, 15.0));
    });
    let hole = doc.add_feature(FeatureKind::Hole(HoleFeature {
        sketch: hole_sk,
        kind: HoleKind::Simple,
        diameter: val(6.0),
        end: HoleEnd::ThroughAll,
        depth: val(0.0),
        drill_angle: None,
        near_sink: None,
        reverse: false,
        size: None,
    }));
    doc.add_feature(FeatureKind::LinearPattern(LinearPatternFeature {
        features: vec![hole],
        direction: Axis::X,
        spacing: val(10.0),
        count: val(3.0),
        ..Default::default()
    }));

    // A pocket, mirrored in a plane offset from the Right plane.
    let pocket_sk = sketch(&mut doc, top, |s| {
        s.add_rectangle(v2(12.0, -10.0), v2(26.0, 6.0));
    });
    let pocket = doc.add_feature(FeatureKind::Extrude(ExtrudeFeature {
        sketch: pocket_sk,
        depth: val(4.0),
        op: BodyOp::Cut,
        ..Default::default()
    }));
    let plane = doc.add_feature(FeatureKind::Plane(PlaneFeature {
        base: PlaneRef::Right,
        offset: val(5.0),
        ..Default::default()
    }));
    doc.add_feature(FeatureKind::Mirror(MirrorFeature {
        plane: PlaneRef::Feature(plane),
        features: vec![pocket],
        bodies: false,
        plane2: None,
        seed_only: false,
    }));

    // A square through-cut, copied to the points of a sketch.
    let slot_sk = sketch(&mut doc, top, |s| {
        s.add_rectangle(v2(28.0, 12.0), v2(32.0, 16.0));
    });
    let slot = through_cut(&mut doc, slot_sk);
    let pts = sketch(&mut doc, PlaneRef::Front, |s| {
        for (x, y) in [(30.0, -16.0), (-30.0, -16.0)] {
            s.add_point(v2(x, y));
        }
    });
    doc.add_feature(FeatureKind::SketchPattern(SketchPatternFeature {
        features: vec![slot],
        sketch: pts,
        reference: None,
    }));
    (doc, base)
}

/// A tube turned about the y axis with radial holes: Revolve, Axis, CircularPattern,
/// Point and CoordSys.
fn turned() -> Document {
    let mut doc = Document::new();
    doc.material = material("Brass");
    doc.custom_properties[0].value = "Fixture: turned sleeve".into();

    let mut sf = SketchFeature::new(PlaneRef::Front);
    let s = &mut sf.sketch;
    let (a, b) = (s.add_point(v2(0.0, 0.0)), s.add_point(v2(0.0, 10.0)));
    let axis = s.add_line(a, b);
    s.entities[axis].construction = true;
    s.add_rectangle(v2(8.0, 0.0), v2(15.0, 30.0));
    let sk = doc.add_feature(FeatureKind::Sketch(sf));
    doc.add_feature(FeatureKind::Revolve(RevolveFeature {
        sketch: sk,
        axis,
        angle: val(360.0),
        op: BodyOp::Boss,
        ..Default::default()
    }));

    // The y axis, as where the Right and Front planes meet.
    let y_axis = doc.add_feature(FeatureKind::Axis(AxisFeature {
        kind: AxisKind::TwoPlanes(PlaneRef::Right, PlaneRef::Front),
        reverse: false,
    }));
    // A radial hole through the wall at +x, then four of them round the axis.
    let hole_sk = sketch(&mut doc, PlaneRef::Right, |s| {
        circle(s, 0.0, 15.0, 4.0);
    });
    let hole = through_cut(&mut doc, hole_sk);
    doc.add_feature(FeatureKind::CircularPattern(CircularPatternFeature {
        features: vec![hole],
        axis: Axis::Feature(y_axis),
        total_angle: val(360.0),
        count: val(4.0),
        ..Default::default()
    }));
    doc.add_feature(FeatureKind::Point(PointFeature::CenterOfMass));
    doc.add_feature(FeatureKind::CoordSys(CoordSysFeature {
        position: [0.0, 30.0, 0.0],
        rotation: [0.0, 0.0, 90.0],
        ..Default::default()
    }));
    doc
}

/// A drafted block with a domed boss: Draft and Dome.
fn drafted() -> Document {
    let mut doc = Document::new();
    doc.custom_properties[0].value = "Fixture: drafted block".into();
    let s = sketch(&mut doc, PlaneRef::Front, |s| {
        s.add_rectangle(v2(0.0, 0.0), v2(40.0, 40.0));
    });
    let base = boss(&mut doc, s, 10.0);
    doc.add_feature(FeatureKind::Draft(DraftFeature {
        neutral: PlaneRef::Front,
        faces: (0..4).map(|k| FaceId::side(base, k)).collect(),
        angle: val(5.0),
        reverse: false,
        propagation: Propagation::None,
    }));
    let s = sketch(&mut doc, PlaneRef::Face(end_cap(base)), |s| {
        circle(s, 20.0, 20.0, 12.0);
    });
    let post = boss(&mut doc, s, 8.0);
    doc.add_feature(FeatureKind::Dome(DomeFeature {
        faces: vec![end_cap(post)],
        height: val(3.0),
        reverse: false,
        elliptical: false,
    }));
    doc
}

/// An open box with a rib, a swept rod and a lofted frustum: Shell, Rib, Sweep and Loft.
fn swept() -> Document {
    let mut doc = Document::new();
    doc.custom_properties[0].value = "Fixture: shelled, ribbed, swept and lofted".into();
    let s = sketch(&mut doc, PlaneRef::Front, |s| {
        s.add_rectangle(v2(0.0, 0.0), v2(100.0, 60.0));
    });
    let tray = boss(&mut doc, s, 30.0);
    doc.add_feature(FeatureKind::Shell(ShellFeature { remove: vec![end_cap(tray)], thickness: val(5.0) }));
    let top = doc.add_feature(FeatureKind::Plane(PlaneFeature {
        base: PlaneRef::Front,
        offset: val(30.0),
        ..Default::default()
    }));
    let rib_sk = sketch(&mut doc, PlaneRef::Feature(top), |s| {
        let (p, q) = (s.add_point(v2(20.0, 30.0)), s.add_point(v2(80.0, 30.0)));
        s.add_line(p, q);
    });
    doc.add_feature(FeatureKind::Rib(RibFeature {
        sketch: rib_sk,
        thickness: val(4.0),
        sides: RibSides::Both,
        normal_to_sketch: true,
        flip: false,
        natural: false,
    }));

    // A rod swept along x, below the tray.
    let path = sketch(&mut doc, PlaneRef::Front, |s| {
        let (p, q) = (s.add_point(v2(0.0, -20.0)), s.add_point(v2(50.0, -20.0)));
        s.add_line(p, q);
    });
    let profile = sketch(&mut doc, PlaneRef::Right, |s| {
        let c = s.add_point(v2(0.0, -20.0));
        s.add_circle(c, 5.0);
    });
    doc.add_feature(FeatureKind::Sweep(SweepFeature {
        profile: Some(profile),
        path,
        diameter: None,
        op: BodyOp::Boss,
        direction: SweepDirection::Direction1,
        thin: None,
    }));

    // A frustum lofted between two squares, beside the tray.
    let lo = sketch(&mut doc, PlaneRef::Front, |s| {
        s.add_rectangle(v2(140.0, 20.0), v2(160.0, 40.0));
    });
    let up = doc.add_feature(FeatureKind::Plane(PlaneFeature {
        base: PlaneRef::Front,
        offset: val(25.0),
        ..Default::default()
    }));
    let hi = sketch(&mut doc, PlaneRef::Feature(up), |s| {
        s.add_rectangle(v2(145.0, 25.0), v2(155.0, 35.0));
    });
    doc.add_feature(FeatureKind::Loft(LoftFeature { profiles: vec![lo, hi], op: BodyOp::Boss }));
    doc
}

/// Separate bodies combined, moved, copied, split, deleted and scaled: Combine,
/// MoveCopy, Split, DeleteBody and Scale.
fn multibody() -> Document {
    let mut doc = Document::new();
    doc.custom_properties[0].value = "Fixture: multibody".into();
    let block = |doc: &mut Document, a: (f64, f64), b: (f64, f64), h: f64| {
        let s = sketch(doc, PlaneRef::Front, |s| {
            s.add_rectangle(v2(a.0, a.1), v2(b.0, b.1));
        });
        doc.add_feature(FeatureKind::Extrude(ExtrudeFeature {
            sketch: s,
            depth: val(h),
            merge: false,
            ..Default::default()
        }))
    };
    let body = |feature: u32| BodyRef { feature, index: 1 };
    let a = block(&mut doc, (0.0, 0.0), (10.0, 10.0), 10.0);
    let b = block(&mut doc, (5.0, 0.0), (15.0, 10.0), 10.0);
    doc.add_feature(FeatureKind::Combine(CombineFeature {
        op: CombineOp::Add,
        main: None,
        bodies: vec![body(a), body(b)],
    }));
    let d = block(&mut doc, (20.0, 0.0), (30.0, 10.0), 10.0);
    doc.add_feature(FeatureKind::MoveCopy(MoveCopyFeature {
        bodies: vec![body(d)],
        copies: 1,
        translate: [val(0.0), val(20.0), val(0.0)],
        rotate: [val(0.0), val(0.0), val(0.0)],
        origin: None,
    }));
    let e = block(&mut doc, (40.0, 0.0), (60.0, 10.0), 10.0);
    let cut = doc.add_feature(FeatureKind::Plane(PlaneFeature {
        base: PlaneRef::Right,
        offset: val(48.0),
        ..Default::default()
    }));
    let split = doc.add_feature(FeatureKind::Split(SplitFeature {
        planes: vec![PlaneRef::Feature(cut)],
        bodies: vec![body(e)],
    }));
    doc.add_feature(FeatureKind::DeleteBody(DeleteBodyFeature {
        bodies: vec![BodyRef { feature: split, index: 2 }],
        keep: false,
    }));
    doc.add_feature(FeatureKind::Scale(ScaleFeature {
        bodies: vec![body(d)],
        about: ScaleAbout::Centroid,
        factors: [val(0.5), val(0.5), val(0.5)],
    }));
    doc
}

/// A body captured from mesh data, cut by a parametric feature: ImportedBody. The mesh
/// is a 10 mm cube written out here, not taken from any file.
fn imported() -> Document {
    let mut doc = Document::new();
    doc.custom_properties[0].value = "Fixture: imported body".into();
    let vertices: Vec<[f64; 3]> = (0..8)
        .map(|i| [((i & 1) * 10) as f64, (((i >> 1) & 1) * 10) as f64, (((i >> 2) & 1) * 10) as f64])
        .collect();
    let quads = [[0, 2, 3, 1], [4, 5, 7, 6], [0, 1, 5, 4], [2, 6, 7, 3], [0, 4, 6, 2], [1, 3, 7, 5]];
    let faces = quads
        .iter()
        .map(|q| ImportedFace { planar: true, triangles: vec![[q[0], q[1], q[2]], [q[0], q[2], q[3]]] })
        .collect();
    let cube = doc.add_feature(FeatureKind::ImportedBody(ImportedBodyFeature {
        source: "AnvilCAD format fixture".into(),
        replaces: vec!["Boss-Extrude1".into()],
        vertices,
        faces,
        exact: None,
        adds: false,
    }));
    // Imported face 1 is the top (z = 10).
    let s = sketch(&mut doc, PlaneRef::Face(FaceId { feature: cube, index: 1 }), |s| {
        circle(s, 5.0, 5.0, 4.0);
    });
    through_cut(&mut doc, s);
    doc
}

/// A plate in three configurations, two top-level and one derived, with a configured
/// variable, depth, suppression and material. The returned id is the base extrude.
fn configured() -> (Document, u32) {
    let mut doc = Document::new();
    doc.material = material("AISI 304");
    doc.custom_properties[0].value = "Fixture: configured plate".into();
    var(&mut doc, "len", "60");
    let mut sf = SketchFeature::new(PlaneRef::Front);
    let (wc, _) = centred_rect(&mut sf.sketch, 60.0, 30.0);
    sf.set_driven(wc, Some("len".into()));
    let s = doc.add_feature(FeatureKind::Sketch(sf));
    let base = boss(&mut doc, s, 10.0);
    let bore_sk = sketch(&mut doc, PlaneRef::Face(end_cap(base)), |s| {
        circle(s, 0.0, 0.0, 8.0);
    });
    let bore = through_cut(&mut doc, bore_sk);

    let len = doc.configure(ConfigParam::Variable { name: "len".into() }).unwrap();
    let depth = doc.configure(ConfigParam::Dimension { feature: base, slot: DimSlot::Depth }).unwrap();
    let bored = doc.configure(ConfigParam::Suppression { feature: bore }).unwrap();
    let mat = doc.configure(ConfigParam::Material).unwrap();

    let long = doc.add_configuration("Long", None).unwrap();
    doc.set_configured_value(len, long, Some(ConfigValue::Expr("90".into()))).unwrap();
    doc.set_configured_value(bored, long, Some(ConfigValue::Suppressed(true))).unwrap();
    doc.set_configured_value(mat, long, Some(ConfigValue::Material(material("6061 Alloy")))).unwrap();
    {
        let c = doc.configuration_mut(long).unwrap();
        c.description = "Long plate, no bore".into();
        c.properties.push(CustomProperty {
            name: "PartNo".into(),
            kind: PropertyKind::Text,
            value: "FX-0002-L".into(),
        });
    }
    let thin = doc.add_configuration("Long-Thin", Some(long)).unwrap();
    doc.set_configured_value(depth, thin, Some(ConfigValue::Dim(val(4.0)))).unwrap();
    doc.activate_configuration(long).unwrap();
    (doc, base)
}

/// A part template: custom properties, a material, variables and a starting sketch.
fn template() -> Document {
    let mut doc = Document::new();
    doc.material = material("Plain Carbon Steel");
    doc.custom_properties[3].value = "Fixture".into();
    doc.custom_properties.push(CustomProperty {
        name: "Weight".into(),
        kind: PropertyKind::Number,
        value: "0".into(),
    });
    var(&mut doc, "wall", "3");
    let mut sf = SketchFeature::new(PlaneRef::Front);
    centred_rect(&mut sf.sketch, 50.0, 50.0);
    let s = doc.add_feature(FeatureKind::Sketch(sf));
    doc.add_feature(FeatureKind::Extrude(ExtrudeFeature {
        sketch: s,
        depth: Dim::Expr("wall".into()),
        ..Default::default()
    }));
    doc
}

/// Rebuilds a part, refusing one that does not rebuild cleanly.
fn checked(mut doc: Document, name: &str) -> Document {
    for c in doc.configurations.clone() {
        let st = doc.rebuild_configuration(c.id).unwrap();
        assert!(st.errors.is_empty(), "{name} ({}): {:?}", c.name, st.errors);
        assert!(st.variable_errors.is_empty(), "{name} ({}): {:?}", c.name, st.variable_errors);
    }
    // Solve the sketches in place, as the app does before saving.
    doc.rebuild();
    doc
}

fn same_on_resave(file: &Path, resave: impl FnOnce(&Path)) -> bool {
    let tmp = file.with_extension("resave.tmp");
    resave(&tmp);
    let same = std::fs::read(file).unwrap() == std::fs::read(&tmp).unwrap();
    std::fs::remove_file(&tmp).unwrap();
    same
}

/// What a part file is expected to give when reopened.
fn part_expected(file: &Path, saved_by: &str, recorded_by: &str) -> Value {
    let doc = Document::load(file).unwrap();
    let raw: Value = serde_json::from_str(&std::fs::read_to_string(file).unwrap()).unwrap();
    let configurations: Vec<Value> = doc
        .configurations
        .iter()
        .map(|c| {
            let st = doc.rebuild_configuration(c.id).unwrap();
            let mut g = common::part_geometry(&st);
            g["name"] = json!(c.name);
            g
        })
        .collect();
    json!({
        "saved_by": saved_by,
        "recorded_by": recorded_by,
        "format": raw["format"],
        "format_version": raw["format_version"],
        "resaves_unchanged": same_on_resave(file, |p| doc.save(p).unwrap()),
        "active_configuration": doc.configuration(doc.active_configuration).map(|c| c.name.clone()),
        "configurations": configurations,
    })
}

/// What an assembly file is expected to give when reopened and its mates solved.
fn assembly_expected(file: &Path, saved_by: &str, recorded_by: &str) -> Value {
    let mut asm = Assembly::load(file).unwrap();
    let resaves = same_on_resave(file, |p| asm.save(p).unwrap());
    let mut parts = Parts::for_file(file);
    let status = asm.solve(&mut parts);
    let solids: Vec<anvil_kernel::Solid> =
        asm.bodies(&mut parts).into_iter().map(|(_, b)| b.unwrap()).collect();
    let bodies = solids.iter().map(common::pieces).sum();
    let refs: Vec<&anvil_kernel::Solid> = solids.iter().collect();
    json!({
        "saved_by": saved_by,
        "recorded_by": recorded_by,
        "format": ASSEMBLY_FORMAT,
        "format_version": ASSEMBLY_FORMAT_VERSION,
        "resaves_unchanged": resaves,
        "components": asm.components.len(),
        "mates": asm.mates.len(),
        "mates_satisfied": status.ok(),
        "geometry": common::geometry(&refs, bodies),
    })
}

fn expected_path(file: &Path) -> PathBuf {
    let mut name = file.as_os_str().to_owned();
    name.push(".expected.json");
    PathBuf::from(name)
}

fn write_expected(file: &Path, v: &Value) {
    std::fs::write(expected_path(file), serde_json::to_string_pretty(v).unwrap() + "\n").unwrap();
}

fn save_part(dir: &Path, file: &str, doc: Document, build: &str) {
    let doc = checked(doc, file);
    let path = dir.join(file);
    doc.save(&path).unwrap();
    write_expected(&path, &part_expected(&path, build, build));
}

#[test]
#[ignore = "writes fixture files; run on purpose with ANVIL_FIXTURE_DIR and ANVIL_FIXTURE_BUILD set"]
fn generate_fixtures() {
    let (Ok(dir), Ok(build)) = (std::env::var("ANVIL_FIXTURE_DIR"), std::env::var("ANVIL_FIXTURE_BUILD"))
    else {
        eprintln!("set ANVIL_FIXTURE_DIR and ANVIL_FIXTURE_BUILD to write fixtures");
        return;
    };
    let dir = PathBuf::from(dir);
    std::fs::create_dir_all(&dir).unwrap();

    let (block_part, block_base) = prismatic();
    save_part(&dir, "prismatic.anvprt", block_part, &build);
    save_part(&dir, "turned.anvprt", turned(), &build);
    save_part(&dir, "drafted.anvprt", drafted(), &build);
    save_part(&dir, "swept.anvprt", swept(), &build);
    save_part(&dir, "multibody.anvprt", multibody(), &build);
    save_part(&dir, "imported.anvprt", imported(), &build);
    let (plate, plate_base) = configured();
    save_part(&dir, "configured.anvprt", plate, &build);
    save_part(&dir, "template.anvprtdot", template(), &build);

    // An assembly of three of the parts above, one of them in a named configuration.
    let mut asm = Assembly::new();
    asm.custom_properties[0].value = "Fixture: assembly".into();
    let c = |id: u32| Some(id);
    let block = asm.add_component("prismatic.anvprt", None, Frame::WORLD);
    let plate = asm.add_component(
        "configured.anvprt",
        Some("Long"),
        Frame { origin: v3(120.0, 10.0, 5.0), ..Frame::WORLD },
    );
    let sleeve = asm.add_component(
        "turned.anvprt",
        None,
        Frame {
            origin: v3(0.0, 0.0, 40.0),
            x: v3(1.0, 0.0, 0.0),
            y: v3(0.0, 0.0, -1.0),
            z: v3(0.0, 1.0, 0.0),
        },
    );
    let face =
        |component: u32, face: FaceId| MateRef { component: c(component), entity: MateEntity::Face(face) };
    let plane =
        |component: u32, p: PlaneRef| MateRef { component: c(component), entity: MateEntity::Plane(p) };
    // The bottom faces meet, the Top planes line up and the Right planes are 100 apart.
    asm.add_mate(
        MateKind::Coincident,
        Alignment::Aligned,
        face(block, FaceId { feature: block_base, index: FaceId::START_CAP }),
        face(plate, FaceId { feature: plate_base, index: FaceId::START_CAP }),
    );
    asm.add_mate(
        MateKind::Coincident,
        Alignment::Aligned,
        plane(block, PlaneRef::Top),
        plane(plate, PlaneRef::Top),
    );
    asm.add_mate(
        MateKind::Distance { value: 100.0, flip: false },
        Alignment::Aligned,
        plane(block, PlaneRef::Right),
        plane(plate, PlaneRef::Right),
    );
    let lock = asm.lock(c(block), sleeve).unwrap();
    asm.add_mate(lock, Alignment::Closest, plane(block, PlaneRef::Front), plane(sleeve, PlaneRef::Front));
    let path = dir.join("assembly.anvasm");
    let mut parts = Parts::for_file(&path);
    let status = asm.solve(&mut parts);
    assert!(status.ok(), "{status:?}");
    asm.save(&path).unwrap();
    write_expected(&path, &assembly_expected(&path, &build, &build));
}

/// Measures the committed fixtures in `ANVIL_FIXTURE_DIR` again with this build and
/// rewrites their expected values, keeping who saved each file and any notes. Run it only
/// when a change is meant to alter rebuilt geometry (a finer tessellation, say), and say
/// in the pull request what moved and why.
#[test]
#[ignore = "rewrites expected values; run on purpose with ANVIL_FIXTURE_DIR and ANVIL_FIXTURE_BUILD set"]
fn rerecord_expected_values() {
    let (Ok(dir), Ok(build)) = (std::env::var("ANVIL_FIXTURE_DIR"), std::env::var("ANVIL_FIXTURE_BUILD"))
    else {
        eprintln!("set ANVIL_FIXTURE_DIR and ANVIL_FIXTURE_BUILD to re-record expected values");
        return;
    };
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().path()).collect();
    files.sort();
    for file in files {
        let ext = file.extension().and_then(|e| e.to_str()).unwrap_or("");
        if !matches!(ext, "anvprt" | "anvprtdot" | "anvasm") {
            continue;
        }
        let old: Value =
            serde_json::from_str(&std::fs::read_to_string(expected_path(&file)).unwrap()).unwrap();
        let saved_by = old["saved_by"].as_str().unwrap();
        let mut new = if ext == "anvasm" {
            assembly_expected(&file, saved_by, &build)
        } else {
            part_expected(&file, saved_by, &build)
        };
        if let Some(notes) = old.get("notes") {
            new["notes"] = notes.clone();
        }
        write_expected(&file, &new);
    }
}
