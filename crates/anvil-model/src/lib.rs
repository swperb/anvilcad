//! AnvilCAD parametric part model.
//!
//! A [`Document`] is an ordered feature tree (sketches, extrudes, revolves, reference
//! planes, patterns) plus global variables, a material and custom properties.
//! [`Document::rebuild`] regenerates the solid.

pub mod document;
pub mod equations;
pub mod exact;
pub mod rebuild;

pub use document::*;
pub use rebuild::{BuildState, MassReport};

#[cfg(test)]
mod tests {
    use super::*;
    use anvil_kernel::{v2, FaceId};
    use anvil_sketch::{Constraint, ORIGIN};
    use std::f64::consts::PI;

    /// 40 x 20 rectangle centered on the origin, fully dimensioned.
    fn plate_sketch(doc: &mut Document, plane: PlaneRef) -> u32 {
        let mut sf = SketchFeature::new(plane);
        let s = &mut sf.sketch;
        let l = s.add_rectangle(v2(-20.0, -10.0), v2(20.0, 10.0));
        let mid = s.add_point(v2(0.0, 0.0));
        s.add_constraint(Constraint::Coincident { p: mid, q: ORIGIN });
        let ws = s.add_constraint(Constraint::Length { line: l[0], value: 40.0 });
        s.add_constraint(Constraint::Length { line: l[1], value: 20.0 });
        // Center the rectangle with a construction diagonal.
        let Some(anvil_sketch::Geom::Line { a, .. }) = s.entities.get(l[0]).map(|e| e.geom) else {
            unreachable!()
        };
        let Some(anvil_sketch::Geom::Line { a: c, .. }) = s.entities.get(l[2]).map(|e| e.geom) else {
            unreachable!()
        };
        let diag = s.add_line(a, c);
        s.entities[diag].construction = true;
        s.add_constraint(Constraint::Midpoint { p: mid, line: diag });
        sf.set_driven(ws, Some("width".into()));
        doc.add_feature(FeatureKind::Sketch(sf))
    }

    fn sample_doc() -> (Document, u32, u32) {
        let mut doc = Document::new();
        doc.variables.push(GlobalVariable {
            name: "width".into(),
            expr: "40".into(),
            comment: String::new(),
        });
        let sk = plate_sketch(&mut doc, PlaneRef::Front);
        let ex = doc.add_feature(FeatureKind::Extrude(ExtrudeFeature {
            sketch: sk,
            end: EndCondition::Blind,
            depth: Dim::Value(10.0),
            reverse: false,
            op: BodyOp::Boss,
        }));
        (doc, sk, ex)
    }

    #[test]
    fn extrude_and_variable_change() {
        let (mut doc, sk, _) = sample_doc();
        let st = doc.rebuild();
        assert!(st.errors.is_empty(), "{:?}", st.errors);
        assert_eq!(st.sketch_status[&sk].dof, 0);
        assert!((st.body.mass_properties().volume - 8000.0).abs() < 1e-6);

        doc.variables[0].expr = "60".into();
        let st = doc.rebuild();
        assert!((st.body.mass_properties().volume - 12000.0).abs() < 1e-6);
        assert_eq!(doc.feature(1).unwrap().name, "Sketch1");
        assert_eq!(doc.feature(2).unwrap().name, "Boss-Extrude1");
    }

    #[test]
    fn cut_on_top_face_through_all() {
        let (mut doc, _, ex) = sample_doc();
        let top = FaceId { feature: ex, index: FaceId::END_CAP };
        let mut sf = SketchFeature::new(PlaneRef::Face(top));
        let c = sf.sketch.add_point(v2(0.0, 0.0));
        sf.sketch.add_constraint(Constraint::Coincident { p: c, q: ORIGIN });
        sf.sketch.add_circle(c, 5.0);
        let hole_sk = doc.add_feature(FeatureKind::Sketch(sf));
        doc.add_feature(FeatureKind::Extrude(ExtrudeFeature {
            sketch: hole_sk,
            end: EndCondition::ThroughAll,
            depth: Dim::Value(0.0),
            reverse: false,
            op: BodyOp::Cut,
        }));
        let st = doc.rebuild();
        assert!(st.errors.is_empty(), "{:?}", st.errors);
        let v = st.body.mass_properties().volume;
        let expected = 8000.0 - PI * 25.0 * 10.0;
        assert!((v - expected).abs() / expected < 2e-3, "{v} vs {expected}");
        // The sketch plane sits on the top face.
        assert!((st.frames[&hole_sk].origin.z - 10.0).abs() < 1e-9);
    }

    #[test]
    fn circular_pattern_of_holes() {
        let mut doc = Document::new();
        // Disc of radius 30, thickness 5.
        let mut sf = SketchFeature::new(PlaneRef::Front);
        let c = sf.sketch.add_point(v2(0.0, 0.0));
        sf.sketch.add_constraint(Constraint::Coincident { p: c, q: ORIGIN });
        sf.sketch.add_circle(c, 30.0);
        let disc = doc.add_feature(FeatureKind::Sketch(sf));
        doc.add_feature(FeatureKind::Extrude(ExtrudeFeature {
            sketch: disc,
            end: EndCondition::Blind,
            depth: 5.0.into(),
            reverse: false,
            op: BodyOp::Boss,
        }));
        // One hole at x = 20, patterned 6 times about Z.
        let mut hf = SketchFeature::new(PlaneRef::Front);
        let hc = hf.sketch.add_point(v2(20.0, 0.0));
        hf.sketch.add_circle(hc, 3.0);
        let hs = doc.add_feature(FeatureKind::Sketch(hf));
        let cut = doc.add_feature(FeatureKind::Extrude(ExtrudeFeature {
            sketch: hs,
            end: EndCondition::ThroughAllBoth,
            depth: 0.0.into(),
            reverse: false,
            op: BodyOp::Cut,
        }));
        doc.add_feature(FeatureKind::CircularPattern(CircularPatternFeature {
            features: vec![cut],
            axis: Axis::Z,
            total_angle: 360.0.into(),
            count: 6.0.into(),
        }));
        let st = doc.rebuild();
        assert!(st.errors.is_empty(), "{:?}", st.errors);
        let v = st.body.mass_properties().volume;
        let expected = 5.0 * PI * (900.0 - 6.0 * 9.0);
        assert!((v - expected).abs() / expected < 5e-3, "{v} vs {expected}");
    }

    #[test]
    fn error_isolated_and_dependents_deleted() {
        let (mut doc, sk, ex) = sample_doc();
        doc.variables[0].expr = "oops +".into();
        let st = doc.rebuild();
        assert!(st.errors.contains_key(&sk));
        assert!(st.errors.contains_key(&ex));

        let gone = doc.delete_feature(sk);
        assert!(gone.contains(&ex));
        assert!(doc.features.is_empty());
    }

    #[test]
    fn rollback_and_suppress() {
        let (mut doc, _, ex) = sample_doc();
        doc.rollback = Some(1);
        assert!(doc.rebuild().body.is_empty());
        doc.rollback = None;
        doc.feature_mut(ex).unwrap().suppressed = true;
        assert!(doc.rebuild().body.is_empty());
    }

    #[test]
    fn save_load_roundtrip() {
        let (mut doc, _, _) = sample_doc();
        doc.material = Some(Material::library().remove(0));
        doc.custom_properties[0].value = "Mounting plate".into();
        let json = doc.to_json();
        let back = Document::from_json(&json).unwrap();
        assert_eq!(doc, back);
        let st = doc.rebuild();
        let m = st.mass_report(&doc).mass.unwrap();
        assert!((m - 8000.0 * 7.8 / 1000.0).abs() < 1e-6);
        assert!(Document::from_json("{\"format\":\"other\"}").is_err());
    }

    #[test]
    fn revolve_feature() {
        let mut doc = Document::new();
        let mut sf = SketchFeature::new(PlaneRef::Front);
        let s = &mut sf.sketch;
        // Centerline along Y plus a 10 x 20 rectangle offset 5 from it -> tube.
        let a = s.add_point(v2(0.0, 0.0));
        let b = s.add_point(v2(0.0, 10.0));
        let axis = s.add_line(a, b);
        s.entities[axis].construction = true;
        s.add_rectangle(v2(5.0, 0.0), v2(15.0, 20.0));
        let sk = doc.add_feature(FeatureKind::Sketch(sf));
        doc.add_feature(FeatureKind::Revolve(RevolveFeature {
            sketch: sk,
            axis,
            angle: 360.0.into(),
            op: BodyOp::Boss,
        }));
        let st = doc.rebuild();
        assert!(st.errors.is_empty(), "{:?}", st.errors);
        let v = st.body.mass_properties().volume;
        let expected = PI * (225.0 - 25.0) * 20.0;
        assert!((v - expected).abs() / expected < 5e-3, "{v} vs {expected}");
    }
}

#[cfg(test)]
mod import_tests {
    use super::*;
    use anvil_kernel::{v2, FaceId};
    use anvil_sketch::{Constraint, ORIGIN};

    /// A 10 mm cube as triangles, one face per side.
    fn cube() -> ImportedBodyFeature {
        let v: Vec<[f64; 3]> = (0..8)
            .map(|i| [((i & 1) * 10) as f64, (((i >> 1) & 1) * 10) as f64, (((i >> 2) & 1) * 10) as f64])
            .collect();
        let quads = [[0, 2, 3, 1], [4, 5, 7, 6], [0, 1, 5, 4], [2, 6, 7, 3], [0, 4, 6, 2], [1, 3, 7, 5]];
        let faces = quads
            .iter()
            .map(|q| ImportedFace { planar: true, triangles: vec![[q[0], q[1], q[2]], [q[0], q[2], q[3]]] })
            .collect();
        ImportedBodyFeature { source: "test".into(), replaces: vec!["Fillet1".into()], vertices: v, faces }
    }

    #[test]
    fn imported_body_then_parametric_cut() {
        let mut doc = Document::new();
        let imp = doc.add_feature(FeatureKind::ImportedBody(cube()));
        let st = doc.rebuild();
        assert!(st.errors.is_empty(), "{:?}", st.errors);
        assert!((st.body.mass_properties().volume - 1000.0).abs() < 1e-9);

        // Sketch on the imported top face (z = 10, face index 1) and cut a hole through it.
        let top = FaceId { feature: imp, index: 1 };
        assert!(st.body.planar_face_frame(top).is_some());
        let mut sf = SketchFeature::new(PlaneRef::Face(top));
        let c = sf.sketch.add_point(v2(5.0, 5.0));
        sf.sketch.add_circle(c, 2.0);
        sf.sketch.add_constraint(Constraint::HDistance { p: ORIGIN, q: c, value: 5.0 });
        let sk = doc.add_feature(FeatureKind::Sketch(sf));
        doc.add_feature(FeatureKind::Extrude(ExtrudeFeature {
            sketch: sk,
            end: EndCondition::ThroughAll,
            depth: Dim::Value(0.0),
            reverse: false,
            op: BodyOp::Cut,
        }));
        let st = doc.rebuild();
        assert!(st.errors.is_empty(), "{:?}", st.errors);
        let v = st.body.mass_properties().volume;
        let hole = std::f64::consts::PI * 4.0 * 10.0;
        assert!((v - (1000.0 - hole)).abs() / 1000.0 < 0.01, "{v}");
        // Round-trips through the file format.
        assert_eq!(Document::from_json(&doc.to_json()).unwrap(), doc);
    }

    #[test]
    fn pattern_about_offset_axis() {
        // A 2 x 2 x 2 block at x = 10, rotated 4 times about a Z axis through (5, 0, 0).
        let mut doc = Document::new();
        let mut sf = SketchFeature::new(PlaneRef::Front);
        sf.sketch.add_rectangle(v2(9.0, -1.0), v2(11.0, 1.0));
        let sk = doc.add_feature(FeatureKind::Sketch(sf));
        let ex = doc.add_feature(FeatureKind::Extrude(ExtrudeFeature {
            sketch: sk,
            end: EndCondition::Blind,
            depth: Dim::Value(2.0),
            reverse: false,
            op: BodyOp::Boss,
        }));
        doc.add_feature(FeatureKind::CircularPattern(CircularPatternFeature {
            features: vec![ex],
            axis: Axis::Line { origin: anvil_kernel::v3(5.0, 0.0, 0.0), dir: anvil_kernel::Vec3::Z },
            total_angle: Dim::Value(360.0),
            count: Dim::Value(4.0),
        }));
        let st = doc.rebuild();
        assert!(st.errors.is_empty(), "{:?}", st.errors);
        assert!((st.body.mass_properties().volume - 32.0).abs() < 1e-6);
        // Instances sit 5 mm from the axis: bounding box spans x from 0 to 10 +/- 1.
        let b = st.body.bounds();
        assert!((b.min.x - -1.0).abs() < 1e-6 && (b.max.x - 11.0).abs() < 1e-6, "{b:?}");
    }
}

#[cfg(test)]
mod revolve_open_profile {
    use super::*;
    use anvil_kernel::v2;

    #[test]
    fn profile_closed_by_axis() {
        let mut doc = Document::new();
        let mut sf = SketchFeature::new(PlaneRef::Front);
        let s = &mut sf.sketch;
        // Open "C" shape whose ends sit on the centerline: (0,0)-(5,0)-(5,10)-(0,10).
        let p: Vec<usize> = [v2(0.0, 0.0), v2(5.0, 0.0), v2(5.0, 10.0), v2(0.0, 10.0)]
            .iter()
            .map(|&q| s.add_point(q))
            .collect();
        s.add_line(p[0], p[1]);
        s.add_line(p[1], p[2]);
        s.add_line(p[2], p[3]);
        let c0 = s.add_point(v2(0.0, -5.0));
        let c1 = s.add_point(v2(0.0, 15.0));
        let axis = s.add_line(c0, c1);
        s.entities[axis].construction = true;
        let sk = doc.add_feature(FeatureKind::Sketch(sf));
        doc.add_feature(FeatureKind::Revolve(RevolveFeature {
            sketch: sk,
            axis,
            angle: Dim::Value(360.0),
            op: BodyOp::Boss,
        }));
        let st = doc.rebuild();
        assert!(st.errors.is_empty(), "{:?}", st.errors);
        let v = st.body.mass_properties().volume;
        let expected = std::f64::consts::PI * 25.0 * 10.0;
        assert!((v - expected).abs() / expected < 5e-3, "{v}");
    }
}

#[cfg(test)]
mod probe_imported {
    use super::*;

    /// Reports orientation consistency of each imported body in ANVIL_FILE.
    #[test]
    #[ignore]
    fn imported_body_consistency() {
        let doc = Document::load(std::path::Path::new(&std::env::var("ANVIL_FILE").unwrap())).unwrap();
        for (i, f) in doc.features.iter().enumerate() {
            if let FeatureKind::ImportedBody(_) = f.kind {
                let mut d = doc.clone();
                d.rollback = Some(i + 1);
                let st = d.rebuild();
                let flipped_faces = st.body.polygons.len();
                println!(
                    "{}: polygons={flipped_faces} open_edges={} volume={:.1}",
                    f.name,
                    st.body.open_edges(),
                    st.body.mass_properties().volume
                );
            }
        }
    }
}
