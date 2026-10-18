//! AnvilCAD parametric part model.
//!
//! A [`Document`] is an ordered feature tree (sketches, extrudes, revolves, reference
//! planes, patterns) plus global variables, a material and custom properties.
//! [`Document::rebuild`] regenerates the solid. A part can have several configurations
//! (see [`config`]).

pub mod config;
pub mod dfm;
pub mod document;
pub mod equations;
pub mod exact;
mod extrude;
pub mod hole;
pub mod material;
mod pattern;
pub mod rebuild;
mod revolve;
pub mod step;
pub mod thin;

pub use config::{ConfigError, ConfigParam, ConfigValue, Configuration, ConfiguredParam, DimSlot};
pub use document::*;
pub use hole::{HoleEnd, HoleFeature, HoleKind, HoleSize, Sink};
pub use material::{Material, MaterialClass};
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
            ..Default::default()
        }));
        (doc, sk, ex)
    }

    /// Export a part exactly, import it back, and keep modelling on the imported body:
    /// a hole sketched on an imported face is cut on the exact kernel.
    #[test]
    fn step_round_trip_stays_exact() {
        let (mut doc, _, _) = sample_doc();
        let st = doc.rebuild();
        let text = crate::step::exact_step(&st, "plate").expect("the plate is exact");
        let (mut part, report) = crate::step::document_from_step(&text, "plate.step").expect("import");
        assert_eq!((report.bodies, report.exact), (1, 1), "{}", report.summary());
        let st = part.rebuild();
        assert!(st.errors.is_empty(), "{:?}", st.errors);
        assert!((st.body.mass_properties().volume - 8000.0).abs() < 1e-6);

        let face = st.body.face_at(anvil_kernel::v3(5.0, 5.0, 10.0), 1e-6).expect("top face");
        let mut sf = SketchFeature::new(PlaneRef::Face(face));
        let c = sf.sketch.add_point(v2(0.0, 0.0));
        sf.sketch.add_circle(c, 3.0);
        let sk = part.add_feature(FeatureKind::Sketch(sf));
        let cut = part.add_feature(FeatureKind::Extrude(ExtrudeFeature {
            sketch: sk,
            end: EndCondition::ThroughAll,
            depth: Dim::Value(1.0),
            reverse: false,
            op: BodyOp::Cut,
            ..Default::default()
        }));
        let st = part.rebuild();
        assert!(st.errors.is_empty(), "{:?}", st.errors);
        assert!(st.exact_features.contains(&cut), "the cut fell back: {:?}", st.faceted_from);
        let v = st.body.mass_properties().volume;
        let exact = 8000.0 - PI * 9.0 * 10.0;
        assert!((v - exact).abs() / exact < 1e-3, "{v}");
        // Exporting again keeps the exact geometry, hole included.
        let again = crate::step::exact_step(&st, "plate").expect("still exact");
        assert!(again.contains("CYLINDRICAL_SURFACE"));
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
        }));
        doc.add_feature(FeatureKind::CircularPattern(CircularPatternFeature {
            features: vec![cut],
            axis: Axis::Z,
            total_angle: 360.0.into(),
            count: 6.0.into(),
            ..Default::default()
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
            ..Default::default()
        }));
        let st = doc.rebuild();
        assert!(st.errors.is_empty(), "{:?}", st.errors);
        let v = st.body.mass_properties().volume;
        let expected = PI * (225.0 - 25.0) * 20.0;
        assert!((v - expected).abs() / expected < 5e-3, "{v} vs {expected}");
        // Built by the exact kernel: cylinders and flat rings, true to the profile.
        assert_eq!(st.faceted_from, None);
        let exact = st.exact.as_ref().expect("exact bodies");
        let fine = anvil_brep::tess::TessParams { chord: 1e-4, max_angle: 0.01 };
        let v: f64 = exact
            .iter()
            .map(|b| {
                anvil_brep::mass::properties(&anvil_brep::tess::tessellate(b, &fine).expect("tess")).volume
            })
            .sum();
        assert!((v - expected).abs() / expected < 1e-5, "{v} vs {expected}");
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
        ImportedBodyFeature {
            source: "test".into(),
            replaces: vec!["Fillet1".into()],
            vertices: v,
            faces,
            exact: None,
            adds: false,
        }
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
            ..Default::default()
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
            ..Default::default()
        }));
        doc.add_feature(FeatureKind::CircularPattern(CircularPatternFeature {
            features: vec![ex],
            axis: Axis::Line { origin: anvil_kernel::v3(5.0, 0.0, 0.0), dir: anvil_kernel::Vec3::Z },
            total_angle: Dim::Value(360.0),
            count: Dim::Value(4.0),
            ..Default::default()
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
mod exact_shell {
    use super::*;
    use anvil_kernel::{v2, EdgeRef, FaceId};

    /// A 40 x 30 x 20 box with its four upright edges rounded (radius 5), opened at the
    /// top with walls 2 thick: built on the exact kernel, the inside rounds have radius 3.
    #[test]
    fn rounded_box_shells_exactly() {
        let mut doc = Document::new();
        let mut sf = SketchFeature::new(PlaneRef::Top);
        sf.sketch.add_rectangle(v2(0.0, 0.0), v2(40.0, 30.0));
        let sk = doc.add_feature(FeatureKind::Sketch(sf));
        let base = doc.add_feature(FeatureKind::Extrude(ExtrudeFeature {
            sketch: sk,
            end: EndCondition::Blind,
            depth: Dim::Value(20.0),
            reverse: false,
            op: BodyOp::Boss,
            ..Default::default()
        }));
        let side = |e: u32| FaceId::side(base, e);
        let corners = (0..4).map(|i| EdgeRef::new(side(i), side((i + 1) % 4))).collect();
        doc.add_feature(FeatureKind::Fillet(FilletFeature {
            edges: corners,
            radius: Dim::Value(5.0),
            profile: FilletProfile::Circular,
            tangent_propagation: false,
        }));
        let top = FaceId { feature: base, index: FaceId::END_CAP };
        let shell = doc
            .add_feature(FeatureKind::Shell(ShellFeature { remove: vec![top], thickness: Dim::Value(2.0) }));
        let st = doc.rebuild();
        assert!(st.errors.is_empty(), "{:?}", st.errors);
        assert_eq!(st.faceted_from, None);
        assert!(st.exact_features.contains(&shell));
        // Outside: 40 x 30 less four corners (25 - 25 pi / 4), 20 tall. Inside: 36 x 26
        // less four corners (9 - 9 pi / 4), 18 deep.
        let pi = std::f64::consts::PI;
        let want =
            (1200.0 - 4.0 * (25.0 - 25.0 * pi / 4.0)) * 20.0 - (936.0 - 4.0 * (9.0 - 9.0 * pi / 4.0)) * 18.0;
        let exact = st.exact.as_ref().expect("exact");
        let fine = anvil_brep::tess::TessParams { chord: 1e-4, max_angle: 0.01 };
        let v: f64 = exact
            .iter()
            .map(|b| {
                anvil_brep::mass::properties(&anvil_brep::tess::tessellate(b, &fine).expect("tess")).volume
            })
            .sum();
        assert!((v - want).abs() / want < 1e-5, "{v} vs {want}");
        // The cavity's faces are the shell's, numbered after the faces they follow.
        assert!(exact[0].faces.iter().any(|f| f.owner == shell));
    }
}

#[cfg(test)]
mod exact_round_fillets {
    use super::*;
    use anvil_kernel::{v2, EdgeRef, FaceId};

    /// A slot (two half circles of radius 4, 10 apart) extruded 5 tall, all four top
    /// edges filleted with radius 1 in one feature: the lines and arcs run smoothly into
    /// each other, so they are blended together on the exact kernel and the rounds join.
    #[test]
    fn slot_outline_fillets_exactly() {
        let mut doc = Document::new();
        let mut sf = SketchFeature::new(PlaneRef::Top);
        let s = &mut sf.sketch;
        let p: Vec<usize> = [v2(0.0, -4.0), v2(10.0, -4.0), v2(10.0, 4.0), v2(0.0, 4.0)]
            .iter()
            .map(|&q| s.add_point(q))
            .collect();
        let (c1, c0) = (s.add_point(v2(10.0, 0.0)), s.add_point(v2(0.0, 0.0)));
        let entities = [
            s.add_line(p[0], p[1]),
            s.add_arc(c1, p[1], p[2]),
            s.add_line(p[2], p[3]),
            s.add_arc(c0, p[3], p[0]),
        ];
        let sk = doc.add_feature(FeatureKind::Sketch(sf));
        let base = doc.add_feature(FeatureKind::Extrude(ExtrudeFeature {
            sketch: sk,
            end: EndCondition::Blind,
            depth: Dim::Value(5.0),
            reverse: false,
            op: BodyOp::Boss,
            ..Default::default()
        }));
        let top = FaceId { feature: base, index: FaceId::END_CAP };
        let edges = entities.iter().map(|&e| EdgeRef::new(top, FaceId::side(base, e as u32))).collect();
        let fillet = doc.add_feature(FeatureKind::Fillet(FilletFeature {
            edges,
            radius: Dim::Value(1.0),
            profile: FilletProfile::Circular,
            tangent_propagation: false,
        }));
        let st = doc.rebuild();
        assert!(st.errors.is_empty(), "{:?}", st.errors);
        assert_eq!(st.faceted_from, None);
        assert!(st.exact_features.contains(&fillet));
        // Pappus: the corner a radius 1 fillet takes off (area 1 - pi/4, centroid
        // (10 - 3 pi) / (3 (4 - pi)) in from the edge), along the sides and round the ends.
        let pi = std::f64::consts::PI;
        let (area, inset) = (1.0 - pi / 4.0, (10.0 - 3.0 * pi) / (3.0 * (4.0 - pi)));
        let want = (80.0 + 16.0 * pi) * 5.0 - area * (20.0 + 2.0 * pi * (4.0 - inset));
        let fine = anvil_brep::tess::TessParams { chord: 1e-4, max_angle: 0.01 };
        let v: f64 = st
            .exact
            .as_ref()
            .expect("exact")
            .iter()
            .map(|b| {
                anvil_brep::mass::properties(&anvil_brep::tess::tessellate(b, &fine).expect("tess")).volume
            })
            .sum();
        assert!((v - want).abs() / want < 5e-5, "{v} vs {want}");
    }
}

#[cfg(test)]
mod exact_rolling_fillets {
    use super::*;
    use anvil_kernel::{v2, EdgeRef, FaceId};

    /// A boss of radius 12 with a hole of radius 4 drilled across it: the hole's rims on
    /// the boss are saddle-shaped, between faces with no common axis, so their fillet is
    /// a rolling-ball round, and the part stays on the exact kernel.
    #[test]
    fn cross_hole_rims_fillet_exactly() {
        let mut doc = Document::new();
        let mut sf = SketchFeature::new(PlaneRef::Top);
        let c = sf.sketch.add_point(v2(0.0, 0.0));
        let boss_circle = sf.sketch.add_circle(c, 12.0);
        let sk = doc.add_feature(FeatureKind::Sketch(sf));
        let boss = doc.add_feature(FeatureKind::Extrude(ExtrudeFeature {
            sketch: sk,
            end: EndCondition::Blind,
            depth: Dim::Value(40.0),
            reverse: false,
            op: BodyOp::Boss,
            ..Default::default()
        }));
        let mut sf = SketchFeature::new(PlaneRef::Front);
        let c = sf.sketch.add_point(v2(0.0, 20.0));
        let hole_circle = sf.sketch.add_circle(c, 4.0);
        let sk = doc.add_feature(FeatureKind::Sketch(sf));
        let hole = doc.add_feature(FeatureKind::Extrude(ExtrudeFeature {
            sketch: sk,
            end: EndCondition::ThroughAllBoth,
            depth: Dim::Value(0.0),
            reverse: false,
            op: BodyOp::Cut,
            ..Default::default()
        }));
        let before = doc.rebuild().body.mass_properties().volume;
        let rim =
            EdgeRef::new(FaceId::side(boss, boss_circle as u32), FaceId::side(hole, hole_circle as u32));
        let fillet = doc.add_feature(FeatureKind::Fillet(FilletFeature {
            edges: vec![rim],
            radius: Dim::Value(1.0),
            profile: FilletProfile::Circular,
            tangent_propagation: false,
        }));
        let st = doc.rebuild();
        assert_eq!(st.faceted_from, None);
        assert!(st.errors.is_empty(), "{:?}", st.errors);
        assert!(st.exact_features.contains(&fillet));
        // Both rims rounded. Round each the faces meet at 90 degrees at the hole's top
        // and bottom and 70.5 at its sides; a unit round takes off cot(phi / 2) -
        // (pi - phi) / 2 per unit length there (as between flat faces; the boss's curved
        // wall adds about a tenth, so the measure is allowed 20% over).
        let pi = std::f64::consts::PI;
        let n = 20_000;
        let one: f64 = (0..n)
            .map(|k| {
                let a = 2.0 * pi * (k as f64 + 0.5) / n as f64;
                let x = 4.0 * a.sin();
                let y = (144.0 - x * x).sqrt();
                let speed =
                    (16.0 * a.cos().powi(2) * (1.0 + x * x / (y * y)) + 16.0 * a.sin().powi(2)).sqrt();
                let phi = pi - (-(a.sin().powi(2)) / 3.0).acos();
                let corner = 1.0 / (phi / 2.0).tan() - (pi - phi) / 2.0;
                corner * speed * (2.0 * pi / n as f64) * (4.25 / 4.0)
            })
            .sum();
        let taken = before - st.body.mass_properties().volume;
        assert!(taken > 2.0 * one && taken < 2.4 * one, "{taken} vs about {}", 2.0 * one);
        assert_eq!(st.body.open_edges(), 0);
    }
}

#[cfg(test)]
mod exact_freeform_chamfers {
    use super::*;
    use anvil_kernel::{v2, EdgeRef, FaceId};

    /// The rims of a hole drilled across a boss, chamfered: their bevel is swept along
    /// the saddle-shaped edges, and the part stays on the exact kernel.
    #[test]
    fn cross_hole_rims_chamfer_exactly() {
        let mut doc = Document::new();
        let mut sf = SketchFeature::new(PlaneRef::Top);
        let c = sf.sketch.add_point(v2(0.0, 0.0));
        let boss_circle = sf.sketch.add_circle(c, 12.0);
        let sk = doc.add_feature(FeatureKind::Sketch(sf));
        let boss = doc.add_feature(FeatureKind::Extrude(ExtrudeFeature {
            sketch: sk,
            end: EndCondition::Blind,
            depth: Dim::Value(40.0),
            reverse: false,
            op: BodyOp::Boss,
            ..Default::default()
        }));
        let mut sf = SketchFeature::new(PlaneRef::Front);
        let c = sf.sketch.add_point(v2(0.0, 20.0));
        let hole_circle = sf.sketch.add_circle(c, 4.0);
        let sk = doc.add_feature(FeatureKind::Sketch(sf));
        let hole = doc.add_feature(FeatureKind::Extrude(ExtrudeFeature {
            sketch: sk,
            end: EndCondition::ThroughAllBoth,
            depth: Dim::Value(0.0),
            reverse: false,
            op: BodyOp::Cut,
            ..Default::default()
        }));
        let before = doc.rebuild().body.mass_properties().volume;
        let rim =
            EdgeRef::new(FaceId::side(boss, boss_circle as u32), FaceId::side(hole, hole_circle as u32));
        let chamfer = doc.add_feature(FeatureKind::Chamfer(ChamferFeature {
            edges: vec![rim],
            distance: Dim::Value(1.0),
            kind: ChamferKind::EqualDistance,
            tangent_propagation: false,
        }));
        let st = doc.rebuild();
        assert_eq!(st.faceted_from, None);
        assert!(st.errors.is_empty(), "{:?}", st.errors);
        assert!(st.exact_features.contains(&chamfer));
        // Each rim loses about half a unit square (the bevel's triangle where the faces
        // meet at right angles; more where they meet more sharply) along about 26 mm.
        let taken = before - st.body.mass_properties().volume;
        assert!(taken > 2.0 * 0.5 * 26.0 * 0.8 && taken < 2.0 * 0.5 * 26.0 * 2.0, "{taken}");
        assert_eq!(st.body.open_edges(), 0);
    }
}

#[cfg(test)]
mod chamfer_kinds {
    use super::*;
    use anvil_kernel::{v2, EdgeRef, FaceId};

    /// A 20 x 10 x 10 block with its front top edge chamfered.
    fn block(kind: ChamferKind, distance: f64) -> (Document, u32) {
        let mut doc = Document::new();
        let mut sf = SketchFeature::new(PlaneRef::Top);
        let sides = sf.sketch.add_rectangle(v2(0.0, 0.0), v2(20.0, 10.0));
        let sk = doc.add_feature(FeatureKind::Sketch(sf));
        let base = doc.add_feature(FeatureKind::Extrude(ExtrudeFeature {
            sketch: sk,
            end: EndCondition::Blind,
            depth: Dim::Value(10.0),
            reverse: false,
            op: BodyOp::Boss,
            ..Default::default()
        }));
        let top = FaceId { feature: base, index: FaceId::END_CAP };
        let edge = EdgeRef::new(top, FaceId::side(base, sides[0] as u32));
        let id = doc.add_feature(FeatureKind::Chamfer(ChamferFeature {
            edges: vec![edge],
            distance: Dim::Value(distance),
            kind,
            tangent_propagation: false,
        }));
        (doc, id)
    }

    fn exact_volume(st: &BuildState) -> f64 {
        let fine = anvil_brep::tess::TessParams { chord: 1e-4, max_angle: 0.01 };
        st.exact
            .as_ref()
            .expect("exact")
            .iter()
            .map(|b| {
                anvil_brep::mass::properties(&anvil_brep::tess::tessellate(b, &fine).expect("tess")).volume
            })
            .sum()
    }

    /// How far the bevel sets back on the block's top and front faces: the nearest corner
    /// on each face's plane to the line where the two planes meet (the edge it replaced).
    fn setbacks(st: &BuildState, doc: &Document) -> (f64, f64) {
        let base = doc.features.iter().find(|f| matches!(f.kind, FeatureKind::Extrude(_))).expect("base").id;
        let sides = match &doc.features[0].kind {
            FeatureKind::Sketch(sf) => sf.sketch.entities.iter().position(|e| !e.construction).expect("side"),
            _ => unreachable!(),
        };
        let body = &st.exact.as_ref().expect("exact")[0];
        let plane = |name: FaceId| {
            let f = body.faces.iter().find(|f| exact::face_id(f) == name).expect("face");
            match f.surface {
                anvil_brep::Surface::Plane { frame } => frame,
                _ => panic!("not flat"),
            }
        };
        let (top, front) = (
            plane(FaceId { feature: base, index: FaceId::END_CAP }),
            plane(FaceId::side(base, sides as u32)),
        );
        let dir = top.z.cross(front.z).normalize();
        let off = |p: glam::DVec3| {
            // The point of the line where both planes hold, then the distance square to it.
            let q = p - dir * (p - top.origin).dot(dir);
            let (a, b) = (top.z, front.z);
            let (ha, hb) = (top.origin.dot(a), front.origin.dot(b));
            let ab = a.dot(b);
            let det = 1.0 - ab * ab;
            let ca = (ha - hb * ab) / det;
            let cb = (hb - ha * ab) / det;
            let line = a * ca + b * cb;
            let r = q - line;
            (r - dir * r.dot(dir)).length()
        };
        let on = |frame: &anvil_brep::Frame, p: glam::DVec3| (p - frame.origin).dot(frame.z).abs() < 1e-9;
        let near = |frame: &anvil_brep::Frame| {
            body.vertices.iter().filter(|v| on(frame, v.point)).map(|v| off(v.point)).fold(f64::MAX, f64::min)
        };
        (near(&top), near(&front))
    }

    #[test]
    fn angle_distance_measures_along_the_chosen_face() {
        // 3 along one face at 30 degrees: 3 tan 30 along the other. Flipped, the 3 runs
        // along the other face.
        let pi = std::f64::consts::PI;
        let short = 3.0 * (pi / 6.0).tan();
        let mut seen = Vec::new();
        for flip in [false, true] {
            let kind = ChamferKind::AngleDistance { angle: Dim::Value(30.0), flip };
            let (mut doc, id) = block(kind, 3.0);
            let st = doc.rebuild();
            assert!(st.errors.is_empty(), "{:?}", st.errors);
            assert_eq!(st.faceted_from, None);
            assert!(st.exact_features.contains(&id));
            let want = 2000.0 - 0.5 * 3.0 * short * 20.0;
            assert!((exact_volume(&st) - want).abs() / want < 1e-9, "{}", exact_volume(&st));
            let (top, front) = setbacks(&st, &doc);
            assert!(
                ((top - 3.0).abs() < 1e-9 && (front - short).abs() < 1e-9)
                    || ((top - short).abs() < 1e-9 && (front - 3.0).abs() < 1e-9),
                "{top} {front}"
            );
            seen.push(top);
        }
        // Flipping moves the 3 to the other face.
        assert!((seen[0] - seen[1]).abs() > 1.0, "{seen:?}");
    }

    #[test]
    fn offset_face_on_a_square_edge_is_equal_distance() {
        let (mut a, _) = block(ChamferKind::OffsetFace, 2.0);
        let (mut b, _) = block(ChamferKind::EqualDistance, 2.0);
        let (va, vb) = (exact_volume(&a.rebuild()), exact_volume(&b.rebuild()));
        assert!((va - vb).abs() < 1e-9 && (va - (2000.0 - 2.0 * 20.0)).abs() < 1e-9, "{va} {vb}");
    }

    #[test]
    fn older_parts_load_as_equal_distance() {
        let (doc, id) = block(ChamferKind::OffsetFace, 2.0);
        let mut json: serde_json::Value = serde_json::from_str(&doc.to_json()).expect("json");
        // Remove the kind, as a part saved before it existed would have none.
        fn strip(v: &mut serde_json::Value) {
            match v {
                serde_json::Value::Object(m) => {
                    if m.contains_key("distance") && m.contains_key("edges") {
                        m.remove("kind");
                    }
                    m.values_mut().for_each(strip);
                }
                serde_json::Value::Array(a) => a.iter_mut().for_each(strip),
                _ => {}
            }
        }
        strip(&mut json);
        let back = Document::from_json(&json.to_string()).expect("load");
        let f = back.features.iter().find(|f| f.id == id).expect("chamfer");
        assert!(matches!(&f.kind, FeatureKind::Chamfer(c) if c.kind == ChamferKind::EqualDistance));
    }
}

#[cfg(test)]
mod exact_rolling_runs {
    use super::*;
    use anvil_kernel::{v2, EdgeRef, FaceId};

    /// A slot-shaped boss standing across a shaft: its root runs along the shaft beside
    /// the boss's straight sides and round it in saddles at the ends, smooth all the way
    /// round. Filleted in one feature, one rolling ball goes round all four edges and the
    /// part stays on the exact kernel.
    #[test]
    fn slot_boss_root_on_a_shaft_fillets_exactly() {
        let mut doc = Document::new();
        // The shaft: a circle of radius 10 on the right plane, extruded both ways.
        let mut sf = SketchFeature::new(PlaneRef::Right);
        let c = sf.sketch.add_point(v2(0.0, 0.0));
        let shaft_circle = sf.sketch.add_circle(c, 10.0);
        let sk = doc.add_feature(FeatureKind::Sketch(sf));
        let shaft = doc.add_feature(FeatureKind::Extrude(ExtrudeFeature {
            sketch: sk,
            end: EndCondition::MidPlane,
            depth: Dim::Value(50.0),
            reverse: false,
            op: BodyOp::Boss,
            ..Default::default()
        }));
        // The boss: a slot (half circles of radius 3, 8 apart) on the top plane, 16 tall.
        let mut sf = SketchFeature::new(PlaneRef::Top);
        let s = &mut sf.sketch;
        let p: Vec<usize> = [v2(0.0, -3.0), v2(8.0, -3.0), v2(8.0, 3.0), v2(0.0, 3.0)]
            .iter()
            .map(|&q| s.add_point(q))
            .collect();
        let (c1, c0) = (s.add_point(v2(8.0, 0.0)), s.add_point(v2(0.0, 0.0)));
        let entities = [
            s.add_line(p[0], p[1]),
            s.add_arc(c1, p[1], p[2]),
            s.add_line(p[2], p[3]),
            s.add_arc(c0, p[3], p[0]),
        ];
        let sk = doc.add_feature(FeatureKind::Sketch(sf));
        let boss = doc.add_feature(FeatureKind::Extrude(ExtrudeFeature {
            sketch: sk,
            end: EndCondition::Blind,
            depth: Dim::Value(16.0),
            reverse: false,
            op: BodyOp::Boss,
            ..Default::default()
        }));
        let before = doc.rebuild();
        assert_eq!(before.faceted_from, None);
        let shaft_face = FaceId::side(shaft, shaft_circle as u32);
        let edges =
            entities.iter().map(|&e| EdgeRef::new(shaft_face, FaceId::side(boss, e as u32))).collect();
        let fillet = doc.add_feature(FeatureKind::Fillet(FilletFeature {
            edges,
            radius: Dim::Value(1.0),
            profile: FilletProfile::Circular,
            tangent_propagation: false,
        }));
        let st = doc.rebuild();
        assert_eq!(st.faceted_from, None);
        assert!(st.errors.is_empty(), "{:?}", st.errors);
        assert!(st.exact_features.contains(&fillet));
        let exact = st.exact.as_ref().expect("exact");
        let rounds = exact[0]
            .faces
            .iter()
            .filter(|f| f.owner == fillet && matches!(f.surface, anvil_brep::Surface::BSpline(_)))
            .count();
        assert_eq!(rounds, 1);
        // The round adds material: more than a 120 degree corner's worth, less than a
        // right angle's, along the root.
        let added = st.body.mass_properties().volume - before.body.mass_properties().volume;
        let pi = std::f64::consts::PI;
        let length = 16.0 + 2.0 * pi * 3.0;
        assert!(
            added > (1.0 / (pi / 3.0).tan() - pi / 6.0) * length && added < (1.0 - pi / 4.0) * length * 1.2,
            "{added}"
        );
        assert_eq!(st.body.open_edges(), 0);
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
            ..Default::default()
        }));
        let st = doc.rebuild();
        assert!(st.errors.is_empty(), "{:?}", st.errors);
        let v = st.body.mass_properties().volume;
        let expected = std::f64::consts::PI * 25.0 * 10.0;
        assert!((v - expected).abs() / expected < 5e-3, "{v}");
        assert_eq!(st.faceted_from, None);
        assert_eq!(st.exact.as_ref().map(|b| b.len()), Some(1));
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

#[cfg(test)]
mod fillet_profiles {
    use super::*;
    use anvil_kernel::{v2, EdgeRef, FaceId};

    /// A 20 x 10 x 10 block with its front top edge filleted, radius 2.
    fn block(profile: FilletProfile) -> (Document, u32) {
        let mut doc = Document::new();
        let mut sf = SketchFeature::new(PlaneRef::Top);
        let sides = sf.sketch.add_rectangle(v2(0.0, 0.0), v2(20.0, 10.0));
        let sk = doc.add_feature(FeatureKind::Sketch(sf));
        let base = doc.add_feature(FeatureKind::Extrude(ExtrudeFeature {
            sketch: sk,
            end: EndCondition::Blind,
            depth: Dim::Value(10.0),
            reverse: false,
            op: BodyOp::Boss,
            ..Default::default()
        }));
        let top = FaceId { feature: base, index: FaceId::END_CAP };
        let edge = EdgeRef::new(top, FaceId::side(base, sides[0] as u32));
        let id = doc.add_feature(FeatureKind::Fillet(FilletFeature {
            edges: vec![edge],
            radius: Dim::Value(2.0),
            profile,
            tangent_propagation: false,
        }));
        (doc, id)
    }

    fn exact_volume(st: &BuildState) -> f64 {
        let fine = anvil_brep::tess::TessParams { chord: 1e-4, max_angle: 0.01 };
        st.exact
            .as_ref()
            .expect("exact")
            .iter()
            .map(|b| {
                anvil_brep::mass::properties(&anvil_brep::tess::tessellate(b, &fine).expect("tess")).volume
            })
            .sum()
    }

    /// The area a profile cuts from the right-angled corner, the corner at the origin and
    /// the contacts at (2, 0) and (0, 2): Green's theorem on many points of the curve.
    fn corner_area(curve: impl Fn(f64) -> (f64, f64)) -> f64 {
        let n = 200_000;
        let mut pts = vec![(0.0, 0.0)];
        pts.extend((0..=n).map(|k| curve(k as f64 / n as f64)));
        let m = pts.len();
        (0..m).map(|k| pts[k].0 * pts[(k + 1) % m].1 - pts[(k + 1) % m].0 * pts[k].1).sum::<f64>().abs() / 2.0
    }

    #[test]
    fn conic_and_curvature_continuous_fillets_stay_exact() {
        // Rho 0.7: the conic of weight 7/3 from (2, 0) to (0, 2) about the corner.
        let w = 0.7 / 0.3;
        let conic = |t: f64| {
            let (b0, b1, b2) = ((1.0 - t) * (1.0 - t), 2.0 * t * (1.0 - t) * w, t * t);
            let s = b0 + b1 + b2;
            (2.0 * b0 / s, 2.0 * b2 / s)
        };
        // The quartic with its second and fourth control points half way to the corner and
        // its middle one on it (the faces are flat, so it leaves them with no curvature).
        let quartic = |t: f64| {
            let u = 1.0 - t;
            (2.0 * u.powi(4) + 4.0 * t * u.powi(3), 2.0 * t.powi(4) + 4.0 * t.powi(3) * u)
        };
        let circular = 4.0 - std::f64::consts::PI;
        for (profile, area) in [
            (FilletProfile::Circular, circular),
            (FilletProfile::ConicRho { rho: Dim::Value(0.7) }, corner_area(conic)),
            (FilletProfile::CurvatureContinuous, corner_area(quartic)),
        ] {
            let (mut doc, id) = block(profile.clone());
            let st = doc.rebuild();
            assert!(st.errors.is_empty(), "{:?}", st.errors);
            assert_eq!(st.faceted_from, None);
            assert!(st.exact_features.contains(&id));
            let want = 2000.0 - 20.0 * area;
            let v = exact_volume(&st);
            assert!((v - want).abs() / want < 1e-6, "{profile:?}: {v} vs {want}");
        }
    }

    #[test]
    fn a_rho_out_of_range_reports_why() {
        let (mut doc, id) = block(FilletProfile::ConicRho { rho: Dim::Value(0.3) });
        let st = doc.rebuild();
        let err = st.errors.get(&id).expect("error");
        assert!(err.contains("rho"), "{err}");
    }

    #[test]
    fn older_parts_load_as_circular() {
        let (doc, id) = block(FilletProfile::CurvatureContinuous);
        let mut json: serde_json::Value = serde_json::from_str(&doc.to_json()).expect("json");
        // Remove the profile, as a part saved before it existed would have none.
        fn strip(v: &mut serde_json::Value) {
            match v {
                serde_json::Value::Object(m) => {
                    if m.contains_key("radius") && m.contains_key("edges") {
                        m.remove("profile");
                    }
                    m.values_mut().for_each(strip);
                }
                serde_json::Value::Array(a) => a.iter_mut().for_each(strip),
                _ => {}
            }
        }
        strip(&mut json);
        let back = Document::from_json(&json.to_string()).expect("load");
        let f = back.features.iter().find(|f| f.id == id).expect("fillet");
        assert!(matches!(&f.kind, FeatureKind::Fillet(f) if f.profile == FilletProfile::Circular));
    }
}

#[cfg(test)]
mod tangent_propagation {
    use super::*;
    use anvil_kernel::{v2, EdgeRef, FaceId};

    /// A slot (half circles of radius 4, 10 apart) extruded 5 tall, and its top edges.
    fn slot() -> (Document, Vec<EdgeRef>) {
        let mut doc = Document::new();
        let mut sf = SketchFeature::new(PlaneRef::Top);
        let s = &mut sf.sketch;
        let p: Vec<usize> = [v2(0.0, -4.0), v2(10.0, -4.0), v2(10.0, 4.0), v2(0.0, 4.0)]
            .iter()
            .map(|&q| s.add_point(q))
            .collect();
        let (c1, c0) = (s.add_point(v2(10.0, 0.0)), s.add_point(v2(0.0, 0.0)));
        let entities = [
            s.add_line(p[0], p[1]),
            s.add_arc(c1, p[1], p[2]),
            s.add_line(p[2], p[3]),
            s.add_arc(c0, p[3], p[0]),
        ];
        let sk = doc.add_feature(FeatureKind::Sketch(sf));
        let base = doc.add_feature(FeatureKind::Extrude(ExtrudeFeature {
            sketch: sk,
            end: EndCondition::Blind,
            depth: Dim::Value(5.0),
            reverse: false,
            op: BodyOp::Boss,
            ..Default::default()
        }));
        let top = FaceId { feature: base, index: FaceId::END_CAP };
        (doc, entities.iter().map(|&e| EdgeRef::new(top, FaceId::side(base, e as u32))).collect())
    }

    fn exact_volume(st: &BuildState) -> f64 {
        let fine = anvil_brep::tess::TessParams { chord: 1e-4, max_angle: 0.01 };
        st.exact
            .as_ref()
            .expect("exact")
            .iter()
            .map(|b| {
                anvil_brep::mass::properties(&anvil_brep::tess::tessellate(b, &fine).expect("tess")).volume
            })
            .sum()
    }

    #[test]
    fn one_edge_of_a_slot_blends_the_whole_outline() {
        // One straight top edge selected, with propagation: the same as all four.
        let blends = |all: bool, propagate: bool, chamfer: bool| {
            let (mut doc, edges) = slot();
            let edges = if all { edges } else { vec![edges[0]] };
            let id = doc.add_feature(if chamfer {
                FeatureKind::Chamfer(ChamferFeature {
                    edges,
                    distance: Dim::Value(1.0),
                    kind: ChamferKind::EqualDistance,
                    tangent_propagation: propagate,
                })
            } else {
                FeatureKind::Fillet(FilletFeature {
                    edges,
                    radius: Dim::Value(1.0),
                    profile: FilletProfile::Circular,
                    tangent_propagation: propagate,
                })
            });
            let st = doc.rebuild();
            assert!(st.errors.is_empty(), "{:?}", st.errors);
            assert!(st.exact_features.contains(&id), "{all} {propagate} {chamfer}: {:?}", st.faceted_from);
            exact_volume(&st)
        };
        for chamfer in [false, true] {
            let all = blends(true, false, chamfer);
            let one = blends(false, true, chamfer);
            let alone = blends(false, false, chamfer);
            assert!((one - all).abs() < 1e-9 * all, "{one} vs {all}");
            // Without propagation the one edge alone is blended: 10 long.
            assert!(alone > one + 1.0, "{alone} {one}");
        }
    }

    #[test]
    fn a_later_round_carries_on_round_an_earlier_one() {
        // A 10 cube: its front top edge rounded 3, then its right top edge rounded 1 with
        // propagation, which carries the round on round the first one's end and down the
        // front right edge (see the kernel's tests for the closed form).
        let mut doc = Document::new();
        let mut sf = SketchFeature::new(PlaneRef::Top);
        let sides = sf.sketch.add_rectangle(v2(0.0, 0.0), v2(10.0, 10.0));
        let sk = doc.add_feature(FeatureKind::Sketch(sf));
        let base = doc.add_feature(FeatureKind::Extrude(ExtrudeFeature {
            sketch: sk,
            end: EndCondition::Blind,
            depth: Dim::Value(10.0),
            reverse: false,
            op: BodyOp::Boss,
            ..Default::default()
        }));
        let top = FaceId { feature: base, index: FaceId::END_CAP };
        let edge = |k: usize| EdgeRef::new(top, FaceId::side(base, sides[k] as u32));
        for (k, r) in [(0, 3.0), (1, 1.0)] {
            doc.add_feature(FeatureKind::Fillet(FilletFeature {
                edges: vec![edge(k)],
                radius: Dim::Value(r),
                profile: FilletProfile::Circular,
                tangent_propagation: true,
            }));
        }
        let st = doc.rebuild();
        assert!(st.errors.is_empty(), "{:?}", st.errors);
        assert_eq!(st.faceted_from, None);
        let pi = std::f64::consts::PI;
        let area = |s: f64| s * s * (1.0 - pi / 4.0);
        let inset = |s: f64| s * (10.0 - 3.0 * pi) / (3.0 * (4.0 - pi));
        let want = 1000.0 - 10.0 * area(3.0) - 14.0 * area(1.0) - 0.5 * pi * area(1.0) * (3.0 - inset(1.0));
        let v = exact_volume(&st);
        assert!((v - want).abs() / want < 3e-6, "{v} vs {want}");
    }

    #[test]
    fn older_parts_load_without_propagation() {
        let (mut doc, edges) = slot();
        let id = doc.add_feature(FeatureKind::Fillet(FilletFeature {
            edges,
            radius: Dim::Value(1.0),
            profile: FilletProfile::Circular,
            tangent_propagation: true,
        }));
        let mut json: serde_json::Value = serde_json::from_str(&doc.to_json()).expect("json");
        fn strip(v: &mut serde_json::Value) {
            match v {
                serde_json::Value::Object(m) => {
                    m.remove("tangent_propagation");
                    m.values_mut().for_each(strip);
                }
                serde_json::Value::Array(a) => a.iter_mut().for_each(strip),
                _ => {}
            }
        }
        strip(&mut json);
        let back = Document::from_json(&json.to_string()).expect("load");
        let f = back.features.iter().find(|f| f.id == id).expect("fillet");
        assert!(matches!(&f.kind, FeatureKind::Fillet(f) if !f.tangent_propagation));
    }
}
