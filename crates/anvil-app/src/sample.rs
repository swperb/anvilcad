//! A sample part that exercises most features. Used by Help > Open Sample Part.

use anvil_kernel::{v2, EdgeRef, FaceId};
use anvil_model::*;
use anvil_sketch::{Constraint, Geom, Sketch, ORIGIN};

/// Rectangle centered on the origin with driven width/height.
fn centered_rect(s: &mut Sketch, w: f64, h: f64) -> (usize, usize) {
    let l = s.add_rectangle(v2(-w / 2.0, -h / 2.0), v2(w / 2.0, h / 2.0));
    let (Geom::Line { a, .. }, Geom::Line { a: c, .. }) = (s.entities[l[0]].geom, s.entities[l[2]].geom)
    else {
        unreachable!()
    };
    let diag = s.add_line(a, c);
    s.entities[diag].construction = true;
    s.add_constraint(Constraint::Midpoint { p: ORIGIN, line: diag });
    let wc = s.add_constraint(Constraint::Length { line: l[0], value: w });
    let hc = s.add_constraint(Constraint::Length { line: l[1], value: h });
    (wc, hc)
}

fn circle(s: &mut Sketch, x: f64, y: f64, d: f64) -> usize {
    let c = s.add_point(v2(x, y));
    let e = s.add_circle(c, d / 2.0);
    s.add_constraint(Constraint::Diameter { entity: e, value: d });
    s.add_constraint(Constraint::HDistance { p: ORIGIN, q: c, value: x });
    s.add_constraint(Constraint::VDistance { p: ORIGIN, q: c, value: y });
    e
}

pub fn sample_part() -> Document {
    let mut doc = Document::new();
    doc.material = Material::library().into_iter().find(|m| m.name == "6061 Alloy");
    doc.custom_properties[0].value = "Sample mounting block".into();
    doc.custom_properties[1].value = "FC-0001".into();
    doc.custom_properties[2].value = "A".into();
    for (n, e) in [("width", "80"), ("depth", "50"), ("thickness", "10"), ("boss_d", "24")] {
        doc.variables.push(GlobalVariable { name: n.into(), expr: e.into(), comment: String::new() });
    }

    // Base plate on the Top plane.
    let mut sf = SketchFeature::new(PlaneRef::Top);
    let (wc, hc) = centered_rect(&mut sf.sketch, 80.0, 50.0);
    sf.set_driven(wc, Some("width".into()));
    sf.set_driven(hc, Some("depth".into()));
    let base_sk = doc.add_feature(FeatureKind::Sketch(sf));
    let base = doc.add_feature(FeatureKind::Extrude(ExtrudeFeature {
        sketch: base_sk,
        end: EndCondition::Blind,
        depth: Dim::Expr("thickness".into()),
        reverse: false,
        op: BodyOp::Boss,
    }));
    let top = PlaneRef::Face(FaceId { feature: base, index: FaceId::END_CAP });

    // Corner holes.
    let mut sf = SketchFeature::new(top);
    for (x, y) in [(30.0, 17.0), (-30.0, 17.0), (30.0, -17.0), (-30.0, -17.0)] {
        circle(&mut sf.sketch, x, y, 6.6);
    }
    let holes_sk = doc.add_feature(FeatureKind::Sketch(sf));
    doc.add_feature(FeatureKind::Extrude(ExtrudeFeature {
        sketch: holes_sk,
        end: EndCondition::ThroughAll,
        depth: Dim::Value(0.0),
        reverse: false,
        op: BodyOp::Cut,
    }));

    // Central boss.
    let mut sf = SketchFeature::new(top);
    let c = sf.sketch.add_point(v2(0.0, 0.0));
    sf.sketch.add_constraint(Constraint::Coincident { p: c, q: ORIGIN });
    let e = sf.sketch.add_circle(c, 12.0);
    let dc = sf.sketch.add_constraint(Constraint::Diameter { entity: e, value: 24.0 });
    sf.set_driven(dc, Some("boss_d".into()));
    let boss_sk = doc.add_feature(FeatureKind::Sketch(sf));
    let boss = doc.add_feature(FeatureKind::Extrude(ExtrudeFeature {
        sketch: boss_sk,
        end: EndCondition::Blind,
        depth: Dim::Value(18.0),
        reverse: false,
        op: BodyOp::Boss,
    }));

    // Bore through the boss.
    let mut sf = SketchFeature::new(PlaneRef::Face(FaceId { feature: boss, index: FaceId::END_CAP }));
    let c = sf.sketch.add_point(v2(0.0, 0.0));
    sf.sketch.add_constraint(Constraint::Coincident { p: c, q: ORIGIN });
    let e = sf.sketch.add_circle(c, 6.0);
    sf.sketch.add_constraint(Constraint::Diameter { entity: e, value: 12.0 });
    let bore_sk = doc.add_feature(FeatureKind::Sketch(sf));
    doc.add_feature(FeatureKind::Extrude(ExtrudeFeature {
        sketch: bore_sk,
        end: EndCondition::ThroughAll,
        depth: Dim::Value(0.0),
        reverse: false,
        op: BodyOp::Cut,
    }));

    // Slots on the front edge, patterned.
    let mut sf = SketchFeature::new(PlaneRef::Front);
    let s = &mut sf.sketch;
    let l = s.add_rectangle(v2(-4.0, 0.0), v2(4.0, 4.0));
    let Geom::Line { a, .. } = s.entities[l[0]].geom else { unreachable!() };
    s.add_constraint(Constraint::Length { line: l[0], value: 8.0 });
    s.add_constraint(Constraint::Length { line: l[1], value: 4.0 });
    s.add_constraint(Constraint::HDistance { p: ORIGIN, q: a, value: -24.0 });
    s.add_constraint(Constraint::VDistance { p: ORIGIN, q: a, value: 0.0 });
    let slot_sk = doc.add_feature(FeatureKind::Sketch(sf));
    let slot = doc.add_feature(FeatureKind::Extrude(ExtrudeFeature {
        sketch: slot_sk,
        end: EndCondition::MidPlane,
        depth: Dim::Value(60.0),
        reverse: false,
        op: BodyOp::Cut,
    }));
    doc.add_feature(FeatureKind::LinearPattern(LinearPatternFeature {
        features: vec![slot],
        direction: Axis::X,
        spacing: Dim::Value(16.0),
        count: Dim::Value(3.0),
    }));

    // Round the plate's vertical corners, the boss rim, and the boss root.
    let side = |feature: u32, entity: u32| FaceId::side(feature, entity);
    let corners = (0..4).map(|i| EdgeRef::new(side(base, i), side(base, (i + 1) % 4))).collect();
    doc.add_feature(FeatureKind::Fillet(FilletFeature { edges: corners, radius: Dim::Value(6.0) }));
    let rim = EdgeRef::new(FaceId { feature: boss, index: FaceId::END_CAP }, side(boss, 0));
    doc.add_feature(FeatureKind::Fillet(FilletFeature { edges: vec![rim], radius: Dim::Value(1.5) }));
    let root = EdgeRef::new(FaceId { feature: base, index: FaceId::END_CAP }, side(boss, 0));
    doc.add_feature(FeatureKind::Fillet(FilletFeature { edges: vec![root], radius: Dim::Value(3.0) }));
    doc
}

#[cfg(test)]
mod tests {
    #[test]
    fn sample_rebuilds_cleanly() {
        let mut doc = super::sample_part();
        let st = doc.rebuild();
        assert!(st.errors.is_empty(), "{:?}", st.errors);
        for (id, s) in &st.sketch_status {
            assert!(s.converged, "sketch {id}: {s:?}");
        }
        assert!(st.body.mass_properties().volume > 30000.0);
    }
}

#[cfg(test)]
mod exact_tests {
    /// The sample part builds entirely on the exact kernel (its boss's rim and root are
    /// round edges, filleted with tori), and matches the volume the faceted kernel alone
    /// produced (38845.67 mm^3 before the exact kernel existed).
    #[test]
    fn sample_part_builds_exactly() {
        let mut doc = super::sample_part();
        let st = doc.rebuild();
        assert!(st.errors.is_empty(), "{:?}", st.errors);
        let exact: Vec<&str> = doc
            .features
            .iter()
            .filter(|f| st.exact_features.contains(&f.id))
            .map(|f| f.name.as_str())
            .collect();
        for name in
            ["Boss-Extrude1", "Cut-Extrude1", "Boss-Extrude2", "LPattern1", "Fillet1", "Fillet2", "Fillet3"]
        {
            assert!(exact.contains(&name), "{name} was not built exactly: {exact:?}");
        }
        assert_eq!(st.faceted_from, None);
        assert!(anvil_model::step::exact_step(&st, "sample").is_some());
        let v = st.body.mass_properties().volume;
        assert!((v - 38845.67).abs() / 38845.67 < 1e-3, "{v}");
        assert_eq!(st.body.open_edges(), 0);
    }

    /// A vertical boss with a hole drilled across it: the hole meets the boss in
    /// traced intersection curves, so the part stays on the exact kernel.
    #[test]
    fn cross_drilled_boss_stays_exact() {
        use anvil_kernel::v2;
        use anvil_model::*;
        use anvil_sketch::{Constraint, ORIGIN};
        let (big, small, height) = (12.0, 4.0, 40.0);
        let mut doc = Document::new();
        let mut sf = SketchFeature::new(PlaneRef::Top);
        let c = sf.sketch.add_point(v2(0.0, 0.0));
        sf.sketch.add_constraint(Constraint::Coincident { p: c, q: ORIGIN });
        let e = sf.sketch.add_circle(c, big);
        sf.sketch.add_constraint(Constraint::Diameter { entity: e, value: 2.0 * big });
        let boss_sk = doc.add_feature(FeatureKind::Sketch(sf));
        doc.add_feature(FeatureKind::Extrude(ExtrudeFeature {
            sketch: boss_sk,
            end: EndCondition::Blind,
            depth: Dim::Value(height),
            reverse: false,
            op: BodyOp::Boss,
        }));
        let mut sf = SketchFeature::new(PlaneRef::Front);
        super::circle(&mut sf.sketch, 0.0, 20.0, 2.0 * small);
        let hole_sk = doc.add_feature(FeatureKind::Sketch(sf));
        doc.add_feature(FeatureKind::Extrude(ExtrudeFeature {
            sketch: hole_sk,
            end: EndCondition::MidPlane,
            depth: Dim::Value(60.0),
            reverse: false,
            op: BodyOp::Cut,
        }));
        let st = doc.rebuild();
        assert!(st.errors.is_empty(), "{:?}", st.errors);
        assert!(st.faceted_from.is_none(), "{:?}", st.faceted_from);
        // Volume shared by the two cylinders, integrated across the hole.
        let n = 100_000;
        let h = 2.0 * small / n as f64;
        let cross: f64 = (0..n)
            .map(|i| {
                let y = -small + h * (i as f64 + 0.5);
                4.0 * (small * small - y * y).sqrt() * (big * big - y * y).sqrt() * h
            })
            .sum();
        let want = std::f64::consts::PI * big * big * height - cross;
        let v = st.body.mass_properties().volume;
        assert!((v - want).abs() / want < 2e-3, "{v} vs {want}");
        assert_eq!(st.body.open_edges(), 0);
        let text = anvil_model::step::exact_step(&st, "boss").expect("exact STEP");
        if let Ok(dir) = std::env::var("ANVIL_DUMP_STEP") {
            let _ = std::fs::write(std::path::Path::new(&dir).join("cross-drilled-boss.step"), &text);
        }
    }

    /// A faceted STEP export (what a part not built exactly is written as) of the sample
    /// part imports back as the same, closed solid.
    #[test]
    fn faceted_step_export_imports_back() {
        let mut doc = super::sample_part();
        let st = doc.rebuild();
        let text = anvil_io::step::write(&st.body, "sample");
        if let Ok(dir) = std::env::var("ANVIL_DUMP_STEP") {
            let _ = std::fs::write(std::path::Path::new(&dir).join("sample-faceted.step"), &text);
        }
        let (mut part, report) = anvil_model::step::document_from_step(&text, "sample.step").expect("import");
        assert_eq!((report.bodies, report.exact), (1, 0));
        let back = part.rebuild();
        assert!(back.errors.is_empty(), "{:?}", back.errors);
        assert_eq!(back.body.open_edges(), 0);
        let (a, b) = (st.body.mass_properties().volume, back.body.mass_properties().volume);
        assert!((a - b).abs() / a < 1e-9, "{a} vs {b}");
    }
}
