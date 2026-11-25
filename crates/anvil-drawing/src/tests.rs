//! Projected outlines of known parts, checked exactly, and the PDF they make.

use super::*;
use anvil_brep::profile::build_regions;
use anvil_brep::{Frame, Loop2, Segment};
use glam::{DVec2, DVec3};
use std::f64::consts::TAU;

const EPS: f64 = 1e-6;

fn frame(origin: DVec3, x: DVec3, y: DVec3) -> Frame {
    Frame { origin, x, y, z: x.cross(y) }
}

/// A 40 x 30 x 20 box from the origin: X 40, Y 30, Z 20.
fn block() -> Solid {
    let pts = [DVec2::new(0.0, 0.0), DVec2::new(40.0, 0.0), DVec2::new(40.0, 30.0), DVec2::new(0.0, 30.0)];
    let segs = (0..4).map(|i| Segment::Line { a: pts[i], b: pts[(i + 1) % 4] }).collect();
    let regions = build_regions(&[Loop2::new(segs)]).expect("region");
    let f = frame(DVec3::ZERO, DVec3::X, DVec3::Y);
    anvil_brep::extrude::extrude_regions(&f, &regions, 0.0, 20.0).expect("extrude").remove(0)
}

/// A cylinder of radius 20 standing 30 tall on the XZ plane (axis Y) with a through hole
/// of radius 8 down its axis.
fn tube() -> Solid {
    let circle =
        |r: f64| Loop2::new(vec![Segment::Arc { center: DVec2::ZERO, radius: r, start: 0.0, sweep: TAU }]);
    let regions = build_regions(&[circle(20.0), circle(8.0)]).expect("region");
    // The Top plane: x along X, y along -Z, normal +Y.
    let f = frame(DVec3::ZERO, DVec3::X, -DVec3::Z);
    anvil_brep::extrude::extrude_regions(&f, &regions, 0.0, 30.0).expect("extrude").remove(0)
}

fn view(solid: &Solid, o: Orientation) -> ViewGeometry {
    hlr::project(std::slice::from_ref(solid), &o.axes(), &Progress::new()).expect("projected")
}

/// A line as its two ends, lower end first, rounded so lines compare exactly.
fn key(a: DVec2, b: DVec2) -> [i64; 4] {
    let r = |v: f64| (v * 1e4).round() as i64;
    let (a, b) = if (a.x, a.y) <= (b.x, b.y) { (a, b) } else { (b, a) };
    [r(a.x), r(a.y), r(b.x), r(b.y)]
}

fn lines(g: &ViewGeometry, kind: LineKind) -> Vec<[i64; 4]> {
    let mut v: Vec<[i64; 4]> = g
        .lines
        .iter()
        .filter(|l| l.kind == kind)
        .map(|l| match &l.path {
            Path2::Line { a, b } => key(*a, *b),
            other => panic!("expected a line, got {other:?}"),
        })
        .collect();
    v.sort();
    v
}

fn k(a: (f64, f64), b: (f64, f64)) -> [i64; 4] {
    key(DVec2::new(a.0, a.1), DVec2::new(b.0, b.1))
}

fn sorted(mut v: Vec<[i64; 4]>) -> Vec<[i64; 4]> {
    v.sort();
    v
}

/// Checks that every line endpoint lies within EPS of the rounded value.
fn assert_exact(g: &ViewGeometry) {
    for l in &g.lines {
        if let Path2::Line { a, b } = l.path {
            for v in [a.x, a.y, b.x, b.y] {
                assert!((v * 1e4 - (v * 1e4).round()).abs() < EPS * 1e4, "{v} is not exact");
            }
        }
    }
}

#[test]
fn box_front_top_and_side_views_are_its_rectangles() {
    let b = block();
    let rect = |x0: f64, y0: f64, x1: f64, y1: f64| {
        sorted(vec![
            k((x0, y0), (x1, y0)),
            k((x1, y0), (x1, y1)),
            k((x0, y1), (x1, y1)),
            k((x0, y0), (x0, y1)),
        ])
    };
    for (o, want) in [
        (Orientation::Front, rect(0.0, 0.0, 40.0, 30.0)),
        // Top: X to the right, -Z up the page.
        (Orientation::Top, rect(0.0, -20.0, 40.0, 0.0)),
        // Right: -Z to the right; left: +Z to the right.
        (Orientation::Right, rect(-20.0, 0.0, 0.0, 30.0)),
        (Orientation::Left, rect(0.0, 0.0, 20.0, 30.0)),
    ] {
        let g = view(&b, o);
        assert_exact(&g);
        assert_eq!(lines(&g, LineKind::Visible), want, "{o:?} visible");
        // The edges behind lie on the outline, so no hidden line is left.
        assert!(lines(&g, LineKind::Hidden).is_empty(), "{o:?} hidden");
        assert!(g.lines.iter().all(|l| !matches!(l.kind, LineKind::Tangent | LineKind::HiddenTangent)));
    }
}

#[test]
fn box_isometric_has_nine_visible_and_three_hidden_edges() {
    let b = block();
    let g = view(&b, Orientation::Isometric);
    let ax = Orientation::Isometric.axes();
    let p = |x: f64, y: f64, z: f64| ax.project(DVec3::new(x, y, z));
    let e = |a: DVec2, b: DVec2| key(a, b);
    // The corner at the origin is the one hidden from (1, 1, 1).
    let hidden = sorted(vec![
        e(p(0.0, 0.0, 0.0), p(40.0, 0.0, 0.0)),
        e(p(0.0, 0.0, 0.0), p(0.0, 30.0, 0.0)),
        e(p(0.0, 0.0, 0.0), p(0.0, 0.0, 20.0)),
    ]);
    assert_eq!(lines(&g, LineKind::Hidden), hidden);
    let visible = lines(&g, LineKind::Visible);
    assert_eq!(visible.len(), 9, "{visible:?}");
    assert!(visible.contains(&e(p(40.0, 30.0, 20.0), p(0.0, 30.0, 20.0))));
    assert!(visible.contains(&e(p(40.0, 0.0, 0.0), p(40.0, 30.0, 0.0))));
}

#[test]
fn tube_front_view_is_a_rectangle_with_the_hole_hidden() {
    let g = view(&tube(), Orientation::Front);
    assert_exact(&g);
    // Silhouettes at x = +-20 and the cap circles seen edge-on.
    assert_eq!(
        lines(&g, LineKind::Visible),
        sorted(vec![
            k((-20.0, 0.0), (20.0, 0.0)),
            k((-20.0, 30.0), (20.0, 30.0)),
            k((-20.0, 0.0), (-20.0, 30.0)),
            k((20.0, 0.0), (20.0, 30.0)),
        ])
    );
    // The hole's silhouettes are hidden; its circles lie on the cap lines and are dropped.
    assert_eq!(
        lines(&g, LineKind::Hidden),
        sorted(vec![k((-8.0, 0.0), (-8.0, 30.0)), k((8.0, 0.0), (8.0, 30.0))])
    );
}

#[test]
fn tube_top_view_is_two_visible_circles() {
    let g = view(&tube(), Orientation::Top);
    let mut radii: Vec<f64> = Vec::new();
    for l in &g.lines {
        assert_eq!(l.kind, LineKind::Visible, "{l:?}");
        let Path2::Conic { c, u, v, t0, t1 } = l.path else { panic!("expected a circle, got {l:?}") };
        assert!(c.length() < EPS, "centre {c}");
        assert!((u.length() - v.length()).abs() < EPS && u.dot(v).abs() < EPS, "not a circle");
        radii.push(u.length());
        radii.push((t1 - t0).abs());
    }
    // Each circle may come as one arc or as the arcs between the cap's vertices; together
    // they cover each radius once round.
    let total = |r: f64| {
        g.lines
            .iter()
            .filter_map(|l| match l.path {
                Path2::Conic { u, t0, t1, .. } if (u.length() - r).abs() < EPS => Some((t1 - t0).abs()),
                _ => None,
            })
            .sum::<f64>()
    };
    assert!((total(20.0) - TAU).abs() < 1e-6, "outer circle covers {}", total(20.0));
    assert!((total(8.0) - TAU).abs() < 1e-6, "hole circle covers {}", total(8.0));
    assert_eq!(g.lines.len() as f64, radii.len() as f64 / 2.0);
}

#[test]
fn tube_isometric_has_straight_silhouettes_tangent_to_the_ellipses() {
    let g = view(&tube(), Orientation::Isometric);
    let ax = Orientation::Isometric.axes();
    // The outer silhouettes run up the cylinder where its normal is square to the eye.
    let w = DVec3::Y.cross(ax.eye).normalize();
    for s in [1.0, -1.0] {
        let a = ax.project(w * (20.0 * s));
        let b = ax.project(w * (20.0 * s) + DVec3::Y * 30.0);
        assert!(lines_of(&g).contains(&(LineKind::Visible, key(a, b))), "outer silhouette {s}");
    }
    // The top caps' circles are fully visible ellipses.
    let top_outer: f64 = g
        .lines
        .iter()
        .filter_map(|l| match l.path {
            Path2::Conic { c, u, t0, t1, .. }
                if l.kind == LineKind::Visible
                    && (c - ax.project(DVec3::Y * 30.0)).length() < EPS
                    && u.length() > 10.0 =>
            {
                Some((t1 - t0).abs())
            }
            _ => None,
        })
        .sum();
    assert!((top_outer - TAU).abs() < 1e-9, "top rim covers {top_outer}");
    // The bottom rim is half seen, half hidden, split where the silhouettes touch it. The
    // ray from that point grazes the cylinder, so the split is placed to within 1e-5 rad
    // (0.2 micrometres on this rim) rather than to rounding.
    let bottom: Vec<(LineKind, f64)> = g
        .lines
        .iter()
        .filter_map(|l| match l.path {
            Path2::Conic { c, u, t0, t1, .. } if c.length() < EPS && u.length() > 10.0 => {
                Some((l.kind, (t1 - t0).abs()))
            }
            _ => None,
        })
        .collect();
    let seen: f64 = bottom.iter().filter(|b| b.0 == LineKind::Visible).map(|b| b.1).sum();
    let hidden: f64 = bottom.iter().filter(|b| b.0 == LineKind::Hidden).map(|b| b.1).sum();
    assert!(
        (seen - std::f64::consts::PI).abs() < 1e-5 && (hidden - std::f64::consts::PI).abs() < 1e-5,
        "{bottom:?}"
    );
}

fn lines_of(g: &ViewGeometry) -> Vec<(LineKind, [i64; 4])> {
    g.lines
        .iter()
        .filter_map(|l| match l.path {
            Path2::Line { a, b } => Some((l.kind, key(a, b))),
            _ => None,
        })
        .collect()
}

#[test]
fn progress_runs_from_loading_to_done_across_the_views() {
    let p = Progress::new();
    assert_eq!(p.fraction(), None, "nothing projected yet");
    let d = Drawing::standard("p.anvprt".into(), Sheet::default());
    project_views(&d, &[block()], HatchPattern::General, &p).expect("views");
    assert_eq!(p.fraction(), Some(1.0));
    // Half way through the second of four views is three eighths done.
    p.set_stages(4);
    p.begin_stage(1);
    p.add_total_for_test(10);
    for _ in 0..5 {
        p.step_for_test();
    }
    assert_eq!(p.fraction(), Some(0.375));
}

#[test]
fn cancelling_stops_the_projection() {
    let p = Progress::new();
    p.cancel();
    assert_eq!(hlr::project(&[block()], &Orientation::Front.axes(), &p), Err(ProjectError::Cancelled));
}

#[test]
fn pdf_holds_vector_paths_and_no_image() {
    let sheet = Sheet { size: SheetSize::A4, scale: Scale::FULL, projection: Projection::ThirdAngle };
    let mut d = Drawing::standard("block.anvprt".into(), sheet);
    for v in &mut d.views {
        v.display = DisplayStyle::HiddenDashed;
    }
    let geometry =
        project_views(&d, &[tube()], HatchPattern::General, &Progress::new()).expect("views").geometry;
    let pic = layout(&d, &geometry);
    let bytes = pdf::write_pdf(&pic, "tube");
    let text = String::from_utf8(bytes).expect("the file is plain text");
    assert!(text.starts_with("%PDF-1.4"));
    assert!(!text.contains("/Image") && !text.contains("/XObject") && !text.contains(" Do"), "no images");
    // Every view's strokes are there: lines (l), curves (c) and dashes for hidden lines.
    let strokes = text.matches("\nS\n").count();
    assert_eq!(strokes, pic.strokes.len());
    assert!(text.matches(" c\n").count() >= 8, "circles are Bezier curves");
    assert!(text.contains("] 0 d"), "hidden lines are dashed");
    // The front view's left silhouette: 30 mm tall at 1:1 is 85.039 pt.
    let front =
        pic.views.iter().find(|v| d.view(v.id).map(|w| w.role) == Some(ViewRole::Front)).expect("front");
    let x = (front.origin.x - 20.0) * 72.0 / 25.4;
    let y0 = front.origin.y * 72.0 / 25.4;
    let mut s = String::new();
    s.push_str(&format!("{x:.3}"));
    let x_txt = s.trim_end_matches('0').trim_end_matches('.').to_string();
    assert!(text.contains(&format!("{x_txt} ")), "left silhouette at x = {x_txt} pt");
    assert!((front.hi.y - front.lo.y - 30.0).abs() < EPS);
    assert!(y0 > 0.0);
}

#[test]
fn views_follow_the_part_when_it_changes() {
    let d = Drawing::standard("p.anvprt".into(), Sheet::default());
    let small =
        project_views(&d, &[block()], HatchPattern::General, &Progress::new()).expect("views").geometry;
    let big = project_views(&d, &[tube()], HatchPattern::General, &Progress::new()).expect("views").geometry;
    let front = d.views[0].id;
    let width = |g: &HashMap<u32, ViewGeometry>| g[&front].bounds().map(|(lo, hi)| hi.x - lo.x);
    assert_eq!(width(&small).map(|w| (w * 1e6).round()), Some(40e6));
    assert_eq!(width(&big).map(|w| (w * 1e6).round()), Some(40e6));
    let pic = layout(&d, &big);
    assert_eq!(pic.views.len(), 4);
}

#[test]
fn part_files_load_to_exact_bodies() {
    let mut doc = anvil_model::Document::new();
    assert!(part_bodies(&doc.rebuild()).is_err(), "an empty part has no body");
    let dir = std::env::temp_dir().join(format!("anvil-drawing-load-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("dir");
    let missing = load_part(&dir.join("nope.anvprt"), None);
    assert!(missing.is_err_and(|e| e.0.contains("cannot open the part")));
    let _ = std::fs::remove_dir_all(&dir);
}

/// A 40 x 20 plate on the Front plane with an 8 mm hole, extruded 10 deep: its sketch has
/// a 40 and a 20 length and a diameter. Returns the part and the sketch's id.
fn plate(width: f64) -> (anvil_model::Document, u32) {
    use anvil_kernel::v2;
    use anvil_model::{
        BodyOp, CustomProperty, Dim, EndCondition, ExtrudeFeature, PlaneRef, PropertyKind, SketchFeature,
    };
    use anvil_sketch::{Constraint, Geom, ORIGIN};
    let mut doc = anvil_model::Document::new();
    let mut sf = SketchFeature::new(PlaneRef::Front);
    let s = &mut sf.sketch;
    let l = s.add_rectangle(v2(0.0, 0.0), v2(width, 20.0));
    let Some(Geom::Line { a, .. }) = s.entities.get(l[0]).map(|e| e.geom) else { unreachable!() };
    s.add_constraint(Constraint::Coincident { p: a, q: ORIGIN });
    s.add_constraint(Constraint::Length { line: l[0], value: width });
    s.add_constraint(Constraint::Length { line: l[1], value: 20.0 });
    let c = s.add_point(v2(width / 2.0, 10.0));
    let hole = s.add_circle(c, 4.0);
    s.add_constraint(Constraint::Diameter { entity: hole, value: 8.0 });
    let sk = doc.add_feature(FeatureKind::Sketch(sf));
    doc.add_feature(FeatureKind::Extrude(ExtrudeFeature {
        sketch: sk,
        end: EndCondition::Blind,
        depth: Dim::Value(10.0),
        reverse: false,
        op: BodyOp::Boss,
        ..Default::default()
    }));
    doc.custom_properties =
        vec![CustomProperty { name: "PartNo".into(), kind: PropertyKind::Text, value: "PL-40".into() }];
    (doc, sk)
}

fn drawn(doc: &anvil_model::Document, d: &Drawing) -> (PartData, HashMap<u32, ViewGeometry>) {
    let part = part_data(doc, d.configuration.as_deref(), "plate").expect("part");
    let g = project_views(d, &part.bodies, HatchPattern::General, &Progress::new()).expect("views").geometry;
    (part, g)
}

fn values(d: &Drawing, part: &PartData, g: &HashMap<u32, ViewGeometry>) -> Vec<(ViewRole, String)> {
    let pic = layout::layout_full(d, g, Some(part), "plate");
    let mut v: Vec<(ViewRole, String)> = pic
        .dims
        .iter()
        .map(|p| {
            let dim = d.dimension(p.id).expect("dimension");
            (d.view(dim.view).expect("view").role, p.text.text.clone())
        })
        .collect();
    v.sort_by(|a, b| (a.0 as u8, &a.1).cmp(&(b.0 as u8, &b.1)));
    v
}

#[test]
fn model_dimensions_match_the_model_and_follow_it() {
    let (mut doc, sk) = plate(40.0);
    let mut d = Drawing::standard("plate.anvprt".into(), Sheet::default());
    let (part, g) = drawn(&doc, &d);
    for dim in dims::model_dimensions(&d, &part, &g) {
        d.add_dimension(dim);
    }
    // The sketch's lengths and diameter go to the front view, which looks square onto the
    // Front plane; the depth goes to the top view, which sees it side on.
    assert_eq!(
        values(&d, &part, &g),
        vec![
            (ViewRole::Front, "20".into()),
            (ViewRole::Front, "40".into()),
            (ViewRole::Front, "\u{d8}8".into()),
            (ViewRole::Top, "10".into()),
        ]
    );
    // The values are what the views measure.
    let front = d.views[0].id;
    let (lo, hi) = g[&front].bounds().expect("front");
    assert!((hi.x - lo.x - 40.0).abs() < EPS && (hi.y - lo.y - 20.0).abs() < EPS);
    // Inserting again adds nothing.
    assert!(dims::model_dimensions(&d, &part, &g).is_empty());

    // The part gets wider: the same dimensions now say 60.
    if let Some(FeatureKind::Sketch(sf)) = doc.features.iter_mut().find(|f| f.id == sk).map(|f| &mut f.kind) {
        let ci = sf.sketch.constraints.iter().position(|c| c.value() == Some(40.0)).expect("width");
        sf.sketch.constraints[ci].set_value(60.0);
    }
    let (part, g) = drawn(&doc, &d);
    let v = values(&d, &part, &g);
    assert!(v.contains(&(ViewRole::Front, "60".into())), "{v:?}");
    let (lo, hi) = g[&front].bounds().expect("front");
    assert!((hi.x - lo.x - 60.0).abs() < EPS);

    // A dimension deleted from the sketch is no longer in the model.
    if let Some(FeatureKind::Sketch(sf)) = doc.features.iter_mut().find(|f| f.id == sk).map(|f| &mut f.kind) {
        let last = sf.sketch.constraints.len() - 1;
        sf.delete_constraint(last);
    }
    let (part, g) = drawn(&doc, &d);
    let pic = layout::layout_full(&d, &g, Some(&part), "plate");
    assert_eq!((pic.dims.len(), pic.dims_missing.len()), (3, 1));
}

#[test]
fn a_reference_dimension_measures_the_view_and_no_longer_attaches_when_it_moves() {
    let (doc, _) = plate(40.0);
    let mut d = Drawing::standard("plate.anvprt".into(), Sheet::default());
    let (part, g) = drawn(&doc, &d);
    let top = d.views.iter().find(|v| v.role == ViewRole::Top).expect("top").id;
    let found = dims::pickables(&g[&top]);
    // The top view of the plate: a 40 x 10 rectangle; pick its long edge.
    let edge = found.lines.iter().copied().find(|(a, b)| (a.distance(*b) - 40.0).abs() < EPS).expect("edge");
    let mid = (edge.0 + edge.1) * 0.5;
    let pick = dims::nearest(&found, mid + DVec2::new(0.0, 0.01), 0.5).expect("picked");
    let o = d.view(top).map(|v| v.role.orientation(d.sheet.projection)).expect("orientation");
    let dim = dims::reference_dimension(top, o, &[pick], mid + DVec2::new(0.0, 15.0)).expect("dimension");
    let id = d.add_dimension(dim);
    let pic = layout::layout_full(&d, &g, Some(&part), "plate");
    let p = pic.dims.iter().find(|p| p.id == id).expect("drawn");
    assert_eq!((p.text.text.as_str(), p.dangling), ("(40)", false));
    assert_eq!(p.arrows.len(), 2);
    // Saved and read back, it is the same.
    let back = Drawing::from_json(&d.to_json().expect("json")).expect("read");
    assert_eq!(back, d);

    // The plate gets wider: that edge is gone, so the dimension no longer attaches and is
    // left out of what is printed.
    let (doc, _) = plate(50.0);
    let (part, g) = drawn(&doc, &d);
    let pic = layout::layout_full(&d, &g, Some(&part), "plate");
    assert!(pic.dims.iter().any(|p| p.id == id && p.dangling));
    assert!(!pic.for_output().dims.iter().any(|p| p.id == id));
}

fn title_texts(d: &Drawing, part: &PartData) -> Vec<String> {
    layout::layout_full(d, &HashMap::new(), Some(part), "plate").notes.into_iter().map(|n| n.text).collect()
}

#[test]
fn the_title_block_follows_the_part_properties_and_configuration() {
    use anvil_model::{CustomProperty, PropertyKind};
    let (mut doc, _) = plate(40.0);
    let mut d = Drawing::standard("plate.anvprt".into(), Sheet::default());
    let part = part_data(&doc, None, "plate").expect("part");
    let t = title_texts(&d, &part);
    assert!(t.contains(&"PL-40".to_string()), "{t:?}");
    assert!(
        t.contains(&"1:1".to_string())
            && t.contains(&"First angle".to_string())
            && t.contains(&"A3".to_string())
    );
    assert!(t.contains(&"Default".to_string()), "the configuration's name: {t:?}");
    let pic = layout::layout_full(&d, &HashMap::new(), Some(&part), "plate");
    assert_eq!(pic.title_missing, ["Description", "Revision", "DrawnBy"]);

    // Changing the property changes the title block.
    doc.custom_properties[0].value = "PL-41".into();
    let part = part_data(&doc, None, "plate").expect("part");
    let t = title_texts(&d, &part);
    assert!(t.contains(&"PL-41".to_string()) && !t.contains(&"PL-40".to_string()), "{t:?}");

    // A configuration's own value wins when the drawing shows that configuration.
    let id = doc.add_configuration("Long", None).expect("configuration");
    if let Some(c) = doc.configuration_mut(id) {
        c.properties.push(CustomProperty {
            name: "partno".into(),
            kind: PropertyKind::Text,
            value: "PL-41L".into(),
        });
    }
    d.configuration = Some("Long".into());
    d.title_block.fields[0].text = "Plate, {Configuration} ({{{FileName}}})".into();
    let part = part_data(&doc, d.configuration.as_deref(), "plate").expect("part");
    let t = title_texts(&d, &part);
    assert!(t.contains(&"PL-41L".to_string()) && t.contains(&"Plate, Long ({plate})".to_string()), "{t:?}");
    d.configuration = Some("Gone".into());
    assert!(
        part_data(&doc, d.configuration.as_deref(), "plate").is_err_and(|e| e.0.contains("no configuration"))
    );
}

#[test]
fn a_drawing_round_trips_through_dxf() {
    let (doc, _) = plate(40.0);
    let mut d = Drawing::standard("plate.anvprt".into(), Sheet::default());
    for v in &mut d.views {
        v.display = DisplayStyle::HiddenDashed;
    }
    let (part, g) = drawn(&doc, &d);
    for dim in dims::model_dimensions(&d, &part, &g) {
        d.add_dimension(dim);
    }
    let dir = std::env::temp_dir().join(format!("anvil-drawing-dxf-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("dir");
    let path = dir.join("plate.dxf");
    export_dxf(&d, &g, Some(&part), &path, "plate").expect("written");
    let f = dxf::read_dxf(&std::fs::read_to_string(&path).expect("read")).expect("parsed");
    let pic = layout::layout_full(&d, &g, Some(&part), "plate").for_output();
    assert_eq!(f.version.as_deref(), Some("AC1009"));
    // One entity per sheet line, on its pen's layer.
    let drawn_on = |layer: &str| {
        f.entities.iter().filter(|e| e.layer() == layer && e.kind != "TEXT" && e.kind != "SOLID").count()
    };
    for pen in [Pen::Visible, Pen::Hidden, Pen::Frame, Pen::Title] {
        let want = pic.strokes.iter().filter(|s| s.pen == pen).count();
        assert_eq!(drawn_on(dxf::layer(pen)), want, "{pen:?}");
    }
    assert!(drawn_on("HIDDEN") > 0, "the hole is hidden in the side and top views");
    // The hole in the front view is an exact circle of radius 4 at its true place.
    let front = pic.views.iter().find(|v| v.id == d.views[0].id).expect("front");
    let s = d.sheet.scale.factor();
    let centre = front.origin + DVec2::new(20.0, 10.0) * s;
    assert!(f.entities.iter().any(|e| e.kind == "CIRCLE"
        && e.layer() == "VISIBLE"
        && e.point(10).is_some_and(|c| c.distance(centre) < 1e-5)
        && e.num(40).is_some_and(|r| (r - 4.0 * s).abs() < 1e-5)));
    // Dimensions: their text, two arrowheads each, on the dimension layer.
    let texts: Vec<&str> = f
        .entities
        .iter()
        .filter(|e| e.kind == "TEXT" && e.layer() == "DIMENSIONS")
        .filter_map(|e| e.get(1))
        .collect();
    let mut sorted = texts.clone();
    sorted.sort();
    assert_eq!(sorted, ["%%c8", "10", "20", "40"]);
    let arrows = f.entities.iter().filter(|e| e.kind == "SOLID").count();
    assert_eq!(arrows, pic.dims.iter().map(|d| d.arrows.len()).sum::<usize>());
    // And the title block's text.
    assert!(f.entities.iter().any(|e| e.kind == "TEXT" && e.layer() == "TITLE" && e.get(1) == Some("PL-40")));
    let _ = std::fs::remove_dir_all(&dir);
}

/// A box from `lo` to `hi`.
fn cuboid(lo: DVec3, hi: DVec3) -> Solid {
    let pts =
        [DVec2::new(lo.x, lo.y), DVec2::new(hi.x, lo.y), DVec2::new(hi.x, hi.y), DVec2::new(lo.x, hi.y)];
    let segs = (0..4).map(|i| Segment::Line { a: pts[i], b: pts[(i + 1) % 4] }).collect();
    let regions = build_regions(&[Loop2::new(segs)]).expect("region");
    anvil_brep::extrude::extrude_regions(&Frame::world(), &regions, lo.z, hi.z).expect("extrude").remove(0)
}

/// A cylinder of radius `r` round the axis through `c` along `axis`, from `z0` to `z1`
/// along it.
fn cylinder(c: DVec3, axis: DVec3, r: f64, z0: f64, z1: f64) -> Solid {
    let hint = if axis.x.abs() < 0.9 { DVec3::X } else { DVec3::Y };
    let f = Frame::new(c, axis, hint).expect("frame");
    let regions = build_regions(&[Loop2::new(vec![Segment::circle(DVec2::ZERO, r)])]).expect("region");
    anvil_brep::extrude::extrude_regions(&f, &regions, z0, z1).expect("extrude").remove(0)
}

fn minus(a: &Solid, b: &Solid) -> Solid {
    anvil_brep::boolean::boolean(a, b, anvil_brep::boolean::BoolOp::Subtract).expect("cut").remove(0)
}

/// The box round each hatch area, rounded, sorted.
fn hatch_boxes(g: &ViewGeometry) -> Vec<[i64; 4]> {
    let mut v: Vec<[i64; 4]> = g
        .hatch
        .iter()
        .map(|h| {
            let pts = h.loops.iter().flatten();
            let lo = pts.clone().fold(DVec2::splat(f64::INFINITY), |a, p| a.min(*p));
            let hi = pts.fold(DVec2::splat(f64::NEG_INFINITY), |a, p| a.max(*p));
            key(lo, hi)
        })
        .collect();
    v.sort();
    v
}

/// The area inside a hatch area's loops (outer less holes).
fn hatch_area(g: &ViewGeometry) -> f64 {
    g.hatch
        .iter()
        .flat_map(|h| h.loops.iter())
        .map(|l| {
            let n = l.len();
            0.5 * (0..n).map(|i| l[i].perp_dot(l[(i + 1) % n])).sum::<f64>()
        })
        .map(f64::abs)
        .sum()
}

fn section_of(solid: &Solid, def: &SectionDef) -> ViewGeometry {
    section::project_section(def, std::slice::from_ref(solid), HatchPattern::Steel, &Progress::new())
        .expect("not cancelled")
        .expect("cut")
}

#[test]
fn section_through_a_box_with_a_hole_shows_the_hole_split_in_two() {
    // 40 x 30 x 20 with an 8 mm hole down Z at (20, 15).
    let part = minus(
        &cuboid(DVec3::ZERO, DVec3::new(40.0, 30.0, 20.0)),
        &cylinder(DVec3::new(20.0, 15.0, 0.0), DVec3::Z, 4.0, -1.0, 21.0),
    );
    // On the front view, a horizontal line through the hole's centre, looking down.
    let def = SectionDef::new(1, &Orientation::Front.axes(), true, &[DVec2::new(20.0, 15.0)]);
    assert_eq!(def.orientation(), Some(Orientation::Top));
    let g = section_of(&part, &def);
    assert_exact(&g);
    // Seen from above (X right, -Z up), the cut at y = 15 is the 40 x 20 rectangle less the
    // hole's 8 mm slot: two cut faces, 16 x 20 each.
    assert_eq!(hatch_boxes(&g), sorted(vec![k((0.0, -20.0), (16.0, 0.0)), k((24.0, -20.0), (40.0, 0.0))]));
    assert!((hatch_area(&g) - 640.0).abs() < 1e-6, "{}", hatch_area(&g));
    let vis = lines(&g, LineKind::Visible);
    // The outline, and the hole's sides seen through the cut.
    for want in [
        k((0.0, -20.0), (0.0, 0.0)),
        k((16.0, -20.0), (16.0, 0.0)),
        k((24.0, -20.0), (24.0, 0.0)),
        k((40.0, -20.0), (40.0, 0.0)),
    ] {
        assert!(vis.contains(&want), "{want:?} in {vis:?}");
    }
    // Along the top and bottom, the lines cover 0 to 40 exactly.
    for v in [0.0, -20.0] {
        let along: f64 = g
            .lines
            .iter()
            .filter_map(|l| match l.path {
                Path2::Line { a, b }
                    if l.kind == LineKind::Visible && (a.y - v).abs() < EPS && (b.y - v).abs() < EPS =>
                {
                    Some((b.x - a.x).abs())
                }
                _ => None,
            })
            .sum();
        assert!((along - 40.0).abs() < 1e-6, "{v}: {along}");
    }
    assert!(lines(&g, LineKind::Hidden).is_empty(), "{:?}", lines(&g, LineKind::Hidden));
    // Hatching: steel's pairs at 45 degrees, only inside the two faces.
    let h = g.hatch.iter().flat_map(|a| section::hatch_lines(a, 1.0)).collect::<Vec<_>>();
    assert!(!h.is_empty());
    for l in &h {
        let Path2::Line { a, b } = l else { panic!("hatch lines are lines") };
        assert!(a.x.min(b.x) > -EPS && a.x.max(b.x) < 40.0 + EPS);
        assert!(
            !((a.x + b.x) * 0.5 > 16.0 + EPS && (a.x + b.x) * 0.5 < 24.0 - EPS),
            "no hatch in the hole: {l:?}"
        );
    }
}

#[test]
fn section_along_a_stepped_shaft_is_its_profile() {
    // Ø20 from x = 0 to 30, then Ø12 to x = 50, along X.
    let big = cylinder(DVec3::ZERO, DVec3::X, 10.0, 0.0, 30.0);
    let small = cylinder(DVec3::ZERO, DVec3::X, 6.0, 30.0, 50.0);
    let shaft = anvil_brep::boolean::boolean(&big, &small, anvil_brep::boolean::BoolOp::Union)
        .expect("union")
        .remove(0);
    let def = SectionDef::new(1, &Orientation::Front.axes(), true, &[DVec2::new(25.0, 0.0)]);
    let g = section_of(&shaft, &def);
    // The step's lines end where the small shaft's circle, behind the cut face, grazes the
    // view: placed to the kernel's 1e-5 mm, so compared at 1e-4 mm (`key` rounds there).
    let want = sorted(vec![
        k((0.0, -10.0), (30.0, -10.0)),
        k((30.0, -10.0), (30.0, -6.0)),
        k((30.0, -6.0), (50.0, -6.0)),
        k((50.0, -6.0), (50.0, 6.0)),
        k((30.0, 6.0), (50.0, 6.0)),
        k((30.0, 6.0), (30.0, 10.0)),
        k((0.0, 10.0), (30.0, 10.0)),
        k((0.0, -10.0), (0.0, 10.0)),
    ]);
    assert_eq!(lines(&g, LineKind::Visible), want);
    assert_eq!(hatch_boxes(&g), vec![k((0.0, -10.0), (50.0, 10.0))]);
    assert!((hatch_area(&g) - (30.0 * 20.0 + 20.0 * 12.0)).abs() < 1e-6, "{}", hatch_area(&g));
}

#[test]
fn an_offset_section_cuts_two_holes_and_hides_its_shoulder() {
    // 60 x 30 x 20 with 8 mm holes down Z at (15, 10) and (45, 20).
    let mut part = cuboid(DVec3::ZERO, DVec3::new(60.0, 30.0, 20.0));
    for (x, y) in [(15.0, 10.0), (45.0, 20.0)] {
        part = minus(&part, &cylinder(DVec3::new(x, y, 0.0), DVec3::Z, 4.0, -1.0, 21.0));
    }
    let def = SectionDef::new(
        1,
        &Orientation::Front.axes(),
        true,
        &[DVec2::new(15.0, 10.0), DVec2::new(45.0, 20.0)],
    );
    assert_eq!(def.jogs(), vec![30.0]);
    let g = section_of(&part, &def);
    // Four cut faces, each 11 x 20: two at y = 10 left of the step, two at y = 20 right of it.
    assert_eq!(
        hatch_boxes(&g),
        sorted(vec![
            k((0.0, -20.0), (11.0, 0.0)),
            k((19.0, -20.0), (30.0, 0.0)),
            k((30.0, -20.0), (41.0, 0.0)),
            k((49.0, -20.0), (60.0, 0.0)),
        ])
    );
    assert!((hatch_area(&g) - 880.0).abs() < 1e-6);
    // The step is seen edge on, and its edges (the shoulder) are not drawn.
    let shoulder = g.lines.iter().any(|l| match l.path {
        Path2::Line { a, b } => (a.x - 30.0).abs() < EPS && (b.x - 30.0).abs() < EPS,
        _ => false,
    });
    assert!(!shoulder, "{:?}", lines(&g, LineKind::Visible));
}

#[test]
fn section_and_detail_views_project_lay_out_and_export() {
    let part = minus(
        &cuboid(DVec3::ZERO, DVec3::new(40.0, 30.0, 20.0)),
        &cylinder(DVec3::new(20.0, 15.0, 0.0), DVec3::Z, 4.0, -1.0, 21.0),
    );
    let mut d = Drawing::standard("p.anvprt".into(), Sheet::default());
    let front = d.views[0].id;
    let a =
        d.add_section(SectionDef::new(front, &Orientation::Front.axes(), true, &[DVec2::new(20.0, 15.0)]));
    let b = d.add_detail(DetailDef {
        parent: a,
        centre: [20.0, 0.0, 0.0],
        radius: 6.0,
        scale: detail::default_scale(d.sheet.scale),
    });
    let p = project_views(&d, &[part], HatchPattern::Steel, &Progress::new()).expect("views");
    assert!(p.failed.is_empty(), "{:?}", p.failed);
    assert_eq!(p.geometry.len(), 6);
    // The detail is the section's lines inside the circle (centre (20, 0) on the top view).
    let det = &p.geometry[&b];
    assert_eq!(det.clip, Some((DVec2::new(20.0, 0.0), 6.0)));
    assert!(
        det.lines.iter().all(|l| l
            .path
            .points(0.01)
            .iter()
            .all(|q| q.distance(DVec2::new(20.0, 0.0)) <= 6.0 + 1e-9))
    );
    let pic = layout::layout(&d, &p.geometry);
    // Labels under the views, letters at the cutting line and the circle.
    let labels: Vec<&str> = pic.labels.iter().map(|n| n.text.as_str()).collect();
    for want in ["SECTION A-A", "DETAIL B", "SCALE 2:1", "A", "B"] {
        assert!(labels.contains(&want), "{want} in {labels:?}");
    }
    assert!(pic.strokes.iter().any(|s| s.pen == Pen::Chain && s.view == Some(front)));
    assert_eq!(pic.fills.len(), 2, "an arrow at each end");
    assert!(pic.strokes.iter().any(|s| s.pen == Pen::Hatch && s.view == Some(a)));
    assert!(pic.strokes.iter().any(|s| s.pen == Pen::Hatch && s.view == Some(b)));
    // Every view fits on the sheet, apart from the others.
    let placed: Vec<_> = pic.views.clone();
    for (i, v) in placed.iter().enumerate() {
        assert!(
            v.lo.x >= 10.0 && v.hi.x <= pic.width - 10.0 && v.lo.y >= 10.0 && v.hi.y <= pic.height - 10.0,
            "{v:?}"
        );
        for w in &placed[i + 1..] {
            let apart = v.lo.x >= w.hi.x || v.hi.x <= w.lo.x || v.lo.y >= w.hi.y || v.hi.y <= w.lo.y;
            assert!(apart, "{v:?} overlaps {w:?}");
        }
    }
    let dxf = dxf::write_dxf(&pic.for_output());
    for layer in ["HATCH", "CUTTING_LINE", "CUTTING_LINE_ENDS", "LABELS"] {
        assert!(dxf.contains(&format!("\r\n{layer}\r\n")), "{layer}");
    }
    let read = dxf::read_dxf(&dxf).expect("reads back");
    assert!(read.entities.iter().any(|e| e.layer() == "LABELS" && e.get(1) == Some("SECTION A-A")));
    let pdf = pdf::write_pdf(&pic.for_output(), "t");
    assert!(String::from_utf8_lossy(&pdf).contains("(SECTION A-A) Tj"));
}

/// A 40 x 30 x 20 block standing on the Front plane with one Hole feature at (20, 15),
/// drilled into it.
fn block_with_hole(kind: usize, end: anvil_model::HoleEnd) -> anvil_model::Document {
    use anvil_kernel::v2;
    use anvil_model::hole::{Fit, Standard};
    use anvil_model::{BodyOp, Dim, EndCondition, ExtrudeFeature, HoleFeature, PlaneRef, SketchFeature};
    let mut doc = anvil_model::Document::new();
    let mut sf = SketchFeature::new(PlaneRef::Front);
    sf.sketch.add_rectangle(v2(0.0, 0.0), v2(40.0, 30.0));
    let sk = doc.add_feature(FeatureKind::Sketch(sf));
    doc.add_feature(FeatureKind::Extrude(ExtrudeFeature {
        sketch: sk,
        end: EndCondition::Blind,
        depth: Dim::Value(20.0),
        op: BodyOp::Boss,
        ..Default::default()
    }));
    let mut pts = SketchFeature::new(PlaneRef::Front);
    pts.sketch.add_point(v2(20.0, 15.0));
    let ps = doc.add_feature(FeatureKind::Sketch(pts));
    let mut h = HoleFeature::standard(ps, kind, Standard::IsoMetric, "M6", Fit::Normal).expect("M6");
    h.end = end;
    h.depth = Dim::Value(12.0);
    if let anvil_model::HoleKind::Tapped { thread_depth, .. } = &mut h.kind {
        *thread_depth = Dim::Value(10.0);
    }
    h.reverse = true;
    doc.add_feature(FeatureKind::Hole(h));
    doc
}

#[test]
fn hole_callouts_come_from_the_hole_feature() {
    use anvil_model::HoleEnd;
    let doc = block_with_hole(1, HoleEnd::ThroughAll);
    let part = part_data(&doc, None, "block").expect("part");
    assert_eq!(part.holes.len(), 1);
    let h = &part.holes[0];
    assert!(h.axis.distance(DVec3::Z) < 1e-9, "drilled into the block: {:?}", h.axis);
    assert!(h.centres[0].distance(DVec3::new(20.0, 15.0, 0.0)) < 1e-9);
    let lines = h.callout_lines();
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(lines[0].ends_with(" THRU") && lines[1].starts_with("CBORE \u{d8}"), "{lines:?}");
    let mut d = Drawing::standard("block.anvprt".into(), Sheet::default());
    let new = callout::hole_callouts(&d, &part);
    assert_eq!(new.len(), 1);
    // Only the front view looks along Z.
    assert_eq!(d.view(new[0].view).map(|v| v.role), Some(ViewRole::Front));
    for c in new {
        d.add_callout(c);
    }
    assert!(callout::hole_callouts(&d, &part).is_empty(), "called out once");
    let g = project_views(&d, &part.bodies, part.hatch, &Progress::new()).expect("views").geometry;
    let pic = layout::layout_full(&d, &g, Some(&part), "block");
    assert_eq!(pic.callouts.len(), 1);
    let texts: Vec<&str> = pic.callouts[0].text.iter().map(|n| n.text.as_str()).collect();
    assert_eq!(texts, lines.iter().map(String::as_str).collect::<Vec<_>>());
    // The leader ends on the counterbore's circle in the front view.
    let fv = pic.views.iter().find(|v| v.id == d.views[0].id).expect("front");
    let centre = fv.origin + DVec2::new(20.0, 15.0) * fv.scale;
    assert!((pic.callouts[0].arrow[0].distance(centre) - h.outer_radius * fv.scale).abs() < 1e-9);
    let dxf = dxf::write_dxf(&pic.for_output());
    assert!(dxf.contains(&dxf::dxf_text(&lines[1])));
    // A suppressed hole leaves its callout counted as missing.
    let mut off = doc.clone();
    if let Some(f) = off.features.last_mut() {
        f.suppressed = true;
    }
    let part2 = part_data(&off, None, "block").expect("part");
    let pic = layout::layout_full(&d, &g, Some(&part2), "block");
    assert!(pic.callouts.is_empty());
    assert_eq!(pic.callouts_missing.len(), 1);
}

#[test]
fn every_hole_type_reads_from_the_part() {
    use anvil_model::HoleEnd;
    let text = |kind: usize, end: HoleEnd| {
        let doc = block_with_hole(kind, end);
        part_data(&doc, None, "b").expect("part").holes.first().expect("a hole").callout_lines()
    };
    let simple = text(0, HoleEnd::Blind);
    assert_eq!(simple.len(), 1);
    assert!(simple[0].starts_with('\u{d8}') && simple[0].ends_with(" X 12 DEEP"), "{simple:?}");
    let csk = text(2, HoleEnd::ThroughAll);
    assert!(csk[1].starts_with("CSK \u{d8}") && csk[1].ends_with(" X 90\u{b0}"), "{csk:?}");
    let tap = text(3, HoleEnd::Blind);
    assert_eq!(tap[1], "M6x1 X 10 DEEP", "{tap:?}");
    assert_eq!(tap[0], "\u{d8}5 X 12 DEEP", "{tap:?}");
}
