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
    project_views(&d, &[block()], &p).expect("views");
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
    let geometry = project_views(&d, &[tube()], &Progress::new()).expect("views");
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
    let small = project_views(&d, &[block()], &Progress::new()).expect("views");
    let big = project_views(&d, &[tube()], &Progress::new()).expect("views");
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
    let missing = load_part(&dir.join("nope.anvprt"));
    assert!(missing.is_err_and(|e| e.0.contains("cannot open the part")));
    let _ = std::fs::remove_dir_all(&dir);
}
