//! Dimensions on drawing views (see `docs/re/drawings.md`, "Dimensions").
//!
//! Two kinds:
//!
//! - **Model dimensions** come from the part: a sketch's driving dimensions and an
//!   extrude's depth. The drawing stores only which feature and which dimension; the value
//!   and the geometry are taken from the rebuilt part every time, so they always match it.
//! - **Reference dimensions** are added on a view with the sketch Dimension tool's rules
//!   ([`anvil_sketch::dimension::infer`]). They store what was picked, as points in model
//!   space on the view's plane, and are measured again whenever the views update. If what
//!   they measured is no longer in the view they no longer attach, and are shown as such.
//!
//! Either way a dimension is resolved into a small sketch in the view's plane (model
//! millimetres, x to the view's right, y up) holding the measured geometry and one
//! dimension constraint, so measuring and drawing reuse the sketcher's code
//! ([`anvil_sketch::Sketch::measure`] and [`anvil_sketch::dimension::graphic`]).

use crate::hlr::{Orientation, Path2, ViewAxes, ViewGeometry};
use crate::layout::{Align, Note, text_width};
use crate::{ExtrudeDepth, PartData, PlacedSketch};
use anvil_kernel::{Vec2, Vec3, v2};
use anvil_sketch::dimension::{self, Pick};
use anvil_sketch::{Constraint, Geom, Sketch};
use glam::{DVec2, DVec3};
use serde::{Deserialize, Serialize};
use std::f64::consts::TAU;

/// Which of a feature's dimensions a model dimension shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ModelItem {
    /// Dimension `constraint` of a sketch feature. `kind` is the constraint's kind when it
    /// was inserted; if the constraint at that index is now of another kind, the dimension
    /// is gone from the model.
    Sketch { constraint: usize, kind: String },
    /// The depth of an extrude.
    Depth,
}

/// Something a reference dimension measured, in model space on the view's plane.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum RefPick {
    Point {
        p: [f64; 3],
    },
    Line {
        a: [f64; 3],
        b: [f64; 3],
    },
    Circle {
        c: [f64; 3],
        r: f64,
    },
    /// Counter-clockwise from `a` to `b` as seen in the dimension's orientation.
    Arc {
        c: [f64; 3],
        a: [f64; 3],
        b: [f64; 3],
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DimSource {
    Model {
        feature: u32,
        item: ModelItem,
    },
    Reference {
        picks: Vec<RefPick>,
        /// The inferred dimension, on the sketch [`reference_sketch`] builds from `picks`.
        measure: Constraint,
        /// The direction the view looked from when the picks were made.
        orientation: Orientation,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Dimension {
    pub id: u32,
    /// The view it is shown in.
    pub view: u32,
    pub source: DimSource,
    /// How far the value has been moved from its default place, in model space on the
    /// view's plane, so it follows the geometry it measures.
    #[serde(default)]
    pub offset: [f64; 3],
}

impl Dimension {
    pub fn is_reference(&self) -> bool {
        matches!(self.source, DimSource::Reference { .. })
    }

    /// True when `self` and `other` show the same model dimension.
    pub fn same_model_item(&self, other: &Dimension) -> bool {
        match (&self.source, &other.source) {
            (DimSource::Model { feature: f, item: a }, DimSource::Model { feature: g, item: b }) => {
                f == g
                    && match (a, b) {
                        (ModelItem::Depth, ModelItem::Depth) => true,
                        (
                            ModelItem::Sketch { constraint: c, .. },
                            ModelItem::Sketch { constraint: d, .. },
                        ) => c == d,
                        _ => false,
                    }
            }
            _ => false,
        }
    }
}

/// Something on a view the Dimension tool can pick, in the view's plane.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Pick2 {
    Point(DVec2),
    Line(DVec2, DVec2),
    Circle(DVec2, f64),
    /// Centre, radius, and the ends counter-clockwise.
    Arc(DVec2, f64, DVec2, DVec2),
}

/// A dimension worked out against the current part and view: the measured geometry as a
/// small sketch in the view's plane, the dimension on it, and where its value goes.
#[derive(Debug, Clone, PartialEq)]
pub struct Resolved {
    pub sketch: Sketch,
    pub constraint: Constraint,
    /// Where the value is drawn, in the view's plane.
    pub label: DVec2,
    pub value: f64,
    pub text: String,
    /// A reference dimension whose picks are no longer in the view.
    pub dangling: bool,
}

/// What resolving a dimension needs to know about its view.
#[derive(Debug, Clone, Copy)]
pub struct ViewContext<'a> {
    pub part: Option<&'a PartData>,
    pub geometry: Option<&'a ViewGeometry>,
    pub orientation: Orientation,
    /// Sheet millimetres per model millimetre.
    pub scale: f64,
}

/// How far a model dimension's value is moved out from the view by default, in sheet
/// millimetres.
pub const DEFAULT_STANDOFF: f64 = 8.0;
/// Dimension text height, in sheet millimetres (ISO 129 practice).
pub const TEXT_HEIGHT: f64 = 3.5;
/// Arrowhead length and width, in sheet millimetres.
pub const ARROW: (f64, f64) = (3.0, 1.0);
/// The space kept clear round a dimension's value, in sheet millimetres.
const TEXT_GAP: f64 = 0.8;

fn dv(v: Vec2) -> DVec2 {
    DVec2::new(v.x, v.y)
}

fn kv(v: DVec2) -> Vec2 {
    v2(v.x, v.y)
}

fn d3(v: Vec3) -> DVec3 {
    DVec3::new(v.x, v.y, v.z)
}

fn arr(v: DVec3) -> [f64; 3] {
    [v.x, v.y, v.z]
}

fn lift(axes: &ViewAxes, p: DVec2) -> DVec3 {
    axes.right * p.x + axes.up * p.y
}

fn flat(axes: &ViewAxes, p: [f64; 3]) -> DVec2 {
    axes.project(DVec3::from_array(p))
}

/// A length in millimetres to two decimals, trailing zeros dropped.
pub fn format_number(v: f64) -> String {
    let mut s = format!("{:.2}", v);
    if s.contains('.') {
        while s.ends_with('0') {
            s.pop();
        }
        if s.ends_with('.') {
            s.pop();
        }
    }
    if s == "-0" { "0".into() } else { s }
}

/// The text a dimension shows: a length, Ø for a diameter, R for a radius, ° for an angle,
/// in parentheses for a reference dimension.
pub fn format_value(c: &Constraint, value: f64, reference: bool) -> String {
    let n = format_number(value.abs());
    let t = match c {
        Constraint::Diameter { .. } | Constraint::AxisDiameter { .. } => format!("\u{d8}{n}"),
        Constraint::Radius { .. } => format!("R{n}"),
        Constraint::RayAngle { .. } | Constraint::Angle { .. } => format!("{n}\u{b0}"),
        _ => n,
    };
    if reference { format!("({t})") } else { t }
}

/// True for the dimension kinds a drawing can show: those the sketcher can draw.
fn drawable(c: &Constraint) -> bool {
    matches!(
        c,
        Constraint::Distance { .. }
            | Constraint::HDistance { .. }
            | Constraint::VDistance { .. }
            | Constraint::Length { .. }
            | Constraint::Radius { .. }
            | Constraint::Diameter { .. }
            | Constraint::PointLineDistance { .. }
            | Constraint::AxisDiameter { .. }
            | Constraint::ArcLength { .. }
            | Constraint::RayAngle { .. }
            | Constraint::Offset { .. }
    )
}

/// Swaps the two lines of an angle, for a sketch seen mirrored.
fn mirror_constraint(c: Constraint) -> Constraint {
    match c {
        Constraint::RayAngle { a, b, flip_a, flip_b, degrees } => {
            Constraint::RayAngle { a: b, b: a, flip_a: flip_b, flip_b: flip_a, degrees }
        }
        c => c,
    }
}

/// Swaps each arc's ends, so arcs stay counter-clockwise in a sketch seen mirrored.
fn mirror_arcs(sk: &mut Sketch) {
    for e in &mut sk.entities {
        if let Geom::Arc { c, a, b } = e.geom {
            e.geom = Geom::Arc { c, a: b, b: a };
        }
    }
}

/// `placed` seen in a view, as a sketch in the view's plane, and whether it is seen
/// mirrored. `None` unless the view looks square onto the sketch.
fn sketch_in_view(placed: &PlacedSketch, axes: &ViewAxes) -> Option<(Sketch, bool, impl Fn(Vec2) -> DVec2)> {
    let f = placed.frame;
    let normal = d3(f.z).normalize_or_zero();
    if normal.dot(axes.eye).abs() < 1.0 - 1e-9 {
        return None;
    }
    let ax = *axes;
    let map = move |p: Vec2| ax.project(d3(f.to_world(p)));
    let (ex, ey) = (axes.project(d3(f.x)), axes.project(d3(f.y)));
    let mirrored = ex.perp_dot(ey) < 0.0;
    let mut sk = placed.sketch.clone();
    for p in &mut sk.points {
        let q = map(v2(p.x, p.y));
        p.x = q.x;
        p.y = q.y;
    }
    if mirrored {
        mirror_arcs(&mut sk);
    }
    Some((sk, mirrored, map))
}

/// A sketch's horizontal or vertical distance, as seen in a view: still along the view's
/// x or y when the sketch's axis lies along one of them, otherwise not drawable.
fn axis_distance_in_view(c: Constraint, placed: &PlacedSketch, axes: &ViewAxes) -> Option<Constraint> {
    let along = |d: Vec3| -> Option<bool> {
        let e = axes.project(d3(d));
        if e.y.abs() < 1e-9 {
            Some(true)
        } else if e.x.abs() < 1e-9 {
            Some(false)
        } else {
            None
        }
    };
    let horizontal = |p, q, value| Constraint::HDistance { p, q, value };
    let vertical = |p, q, value| Constraint::VDistance { p, q, value };
    Some(match c {
        Constraint::HDistance { p, q, value } => {
            if along(placed.frame.x)? {
                horizontal(p, q, value)
            } else {
                vertical(p, q, value)
            }
        }
        Constraint::VDistance { p, q, value } => {
            if along(placed.frame.y)? {
                horizontal(p, q, value)
            } else {
                vertical(p, q, value)
            }
        }
        c => c,
    })
}

/// The middle of a view's lines, or the origin for an empty view.
fn view_centre(g: Option<&ViewGeometry>) -> DVec2 {
    g.and_then(|g| g.bounds()).map_or(DVec2::ZERO, |(lo, hi)| (lo + hi) * 0.5)
}

/// Moves a value placed on the geometry out from the view's middle.
fn stand_off(at: DVec2, centre: DVec2, scale: f64) -> DVec2 {
    let d = at - centre;
    let dir = if d.length() > 1e-9 { d / d.length() } else { DVec2::Y };
    at + dir * (DEFAULT_STANDOFF / scale.max(1e-9))
}

fn resolve_sketch_dim(
    feature: u32,
    ci: usize,
    kind: &str,
    offset: DVec2,
    cx: &ViewContext,
) -> Option<Resolved> {
    let part = cx.part?;
    let placed = part.sketch(feature)?;
    let c = *placed.sketch.constraints.get(ci)?;
    if c.label() != kind || !c.is_dimension() || !drawable(&c) {
        return None;
    }
    let axes = cx.orientation.axes();
    let (sk, mirrored, map) = sketch_in_view(placed, &axes)?;
    let mut c = axis_distance_in_view(c, placed, &axes)?;
    if mirrored {
        c = mirror_constraint(c);
    }
    let value = sk.measure(&c)?;
    let reference = placed.sketch.is_reference(ci);
    let label = match placed.sketch.annotation(ci).filter(|a| a.offset.length() > 1e-9) {
        // Where the user placed it in the sketch.
        Some(_) => map(dimension::label_position(&placed.sketch, ci)?),
        None => stand_off(dv(dimension::anchor(&sk, &c)?), view_centre(cx.geometry), cx.scale),
    };
    Some(Resolved {
        text: format_value(&c, value, reference),
        sketch: sk,
        constraint: c,
        label: label + offset,
        value,
        dangling: false,
    })
}

fn resolve_depth(depth: &ExtrudeDepth, offset: DVec2, cx: &ViewContext) -> Option<Resolved> {
    let part = cx.part?;
    let placed = part.sketch(depth.sketch)?;
    let axes = cx.orientation.axes();
    let f = placed.frame;
    let dir = d3(f.z).normalize_or_zero() * depth.sign;
    if dir.dot(axes.eye).abs() > 1e-9 || depth.depth <= 0.0 {
        return None;
    }
    let d2 = axes.project(dir);
    let n = d2.perp();
    // The profile point furthest out to one side, so the dimension runs beside the part.
    let start = d3(f.z).normalize_or_zero() * depth.start;
    let mut best: Option<(f64, DVec3)> = None;
    for e in placed.sketch.entities.iter().filter(|e| !e.construction) {
        for p in e.points() {
            let Some(q) = placed.sketch.points.get(p) else { continue };
            let w = d3(f.to_world(v2(q.x, q.y))) + start;
            let k = axes.project(w).dot(n);
            if best.is_none_or(|(b, _)| k > b + 1e-12) {
                best = Some((k, w));
            }
        }
    }
    let (_, base) = best?;
    let (p0, p1) = if depth.mid_plane {
        (base - dir * depth.depth * 0.5, base + dir * depth.depth * 0.5)
    } else {
        (base, base + dir * depth.depth)
    };
    let mut sk = Sketch::new();
    let a = sk.add_point(kv(axes.project(p0)));
    let b = sk.add_point(kv(axes.project(p1)));
    let c = Constraint::Distance { p: a, q: b, value: depth.depth };
    let mid = (axes.project(p0) + axes.project(p1)) * 0.5;
    let label = mid + n * (DEFAULT_STANDOFF / cx.scale.max(1e-9));
    Some(Resolved {
        text: format_value(&c, depth.depth, false),
        sketch: sk,
        constraint: c,
        label: label + offset,
        value: depth.depth,
        dangling: false,
    })
}

/// The sketch a reference dimension measures on: its picks, in order, as points, lines,
/// circles and arcs in the view's plane. Returns the sketch and the picks on it.
pub fn reference_sketch(picks: &[Pick2]) -> (Sketch, Vec<Pick>) {
    let mut sk = Sketch::new();
    let mut out = Vec::new();
    for p in picks {
        out.push(match *p {
            Pick2::Point(q) => Pick::Point(sk.add_point(kv(q))),
            Pick2::Line(a, b) => {
                let (a, b) = (sk.add_point(kv(a)), sk.add_point(kv(b)));
                Pick::Entity(sk.add_line(a, b))
            }
            Pick2::Circle(c, r) => {
                let c = sk.add_point(kv(c));
                Pick::Entity(sk.add_circle(c, r))
            }
            Pick2::Arc(c, _, a, b) => {
                let (c, a, b) = (sk.add_point(kv(c)), sk.add_point(kv(a)), sk.add_point(kv(b)));
                Pick::Entity(sk.add_arc(c, a, b))
            }
        });
    }
    (sk, out)
}

/// A stored pick seen in the view `axes` looks from; `mirrored` when that view looks the
/// other way along the same axis as the one the pick was made in.
fn pick_in_view(p: &RefPick, axes: &ViewAxes, mirrored: bool) -> Pick2 {
    match p {
        RefPick::Point { p } => Pick2::Point(flat(axes, *p)),
        RefPick::Line { a, b } => Pick2::Line(flat(axes, *a), flat(axes, *b)),
        RefPick::Circle { c, r } => Pick2::Circle(flat(axes, *c), *r),
        RefPick::Arc { c, a, b } => {
            let (c, a, b) = (flat(axes, *c), flat(axes, *a), flat(axes, *b));
            let r = a.distance(c);
            if mirrored { Pick2::Arc(c, r, b, a) } else { Pick2::Arc(c, r, a, b) }
        }
    }
}

fn pick_to_model(p: &Pick2, axes: &ViewAxes) -> RefPick {
    match *p {
        Pick2::Point(q) => RefPick::Point { p: arr(lift(axes, q)) },
        Pick2::Line(a, b) => RefPick::Line { a: arr(lift(axes, a)), b: arr(lift(axes, b)) },
        Pick2::Circle(c, r) => RefPick::Circle { c: arr(lift(axes, c)), r },
        Pick2::Arc(c, _, a, b) => {
            RefPick::Arc { c: arr(lift(axes, c)), a: arr(lift(axes, a)), b: arr(lift(axes, b)) }
        }
    }
}

/// How close two places must be to be the same place, in model millimetres.
fn tolerance(g: &ViewGeometry) -> f64 {
    let size = g.bounds().map_or(1.0, |(lo, hi)| (hi - lo).max_element().max(1.0));
    1e-6 * size + 1e-6
}

/// True when `p` is still among the view's pickable things.
fn still_there(p: &Pick2, picks: &Pickables, tol: f64) -> bool {
    let same = |a: DVec2, b: DVec2| a.distance(b) <= tol;
    match *p {
        Pick2::Point(q) => picks.points.iter().any(|&v| same(v, q)),
        Pick2::Line(a, b) => {
            picks.lines.iter().any(|&(c, d)| (same(a, c) && same(b, d)) || (same(a, d) && same(b, c)))
        }
        Pick2::Circle(c, r) => picks.circles.iter().any(|&(d, s)| same(c, d) && (r - s).abs() <= tol),
        Pick2::Arc(c, r, a, b) => picks
            .arcs
            .iter()
            .any(|&(d, s, e, f)| same(c, d) && (r - s).abs() <= tol && same(a, e) && same(b, f)),
    }
}

fn resolve_reference(
    picks: &[RefPick],
    measure: &Constraint,
    made_in: Orientation,
    offset: DVec2,
    cx: &ViewContext,
    found: Option<&Pickables>,
) -> Option<Resolved> {
    let axes = cx.orientation.axes();
    let eyes = made_in.axes().eye.dot(axes.eye);
    if eyes.abs() < 1.0 - 1e-9 {
        return None;
    }
    let mirrored = eyes < 0.0;
    let in_view: Vec<Pick2> = picks.iter().map(|p| pick_in_view(p, &axes, mirrored)).collect();
    let dangling = match (cx.geometry, found) {
        (Some(g), Some(found)) => {
            let tol = tolerance(g);
            !in_view.iter().all(|p| still_there(p, found, tol))
        }
        (Some(g), None) => {
            let found = pickables(g);
            let tol = tolerance(g);
            !in_view.iter().all(|p| still_there(p, &found, tol))
        }
        (None, _) => true,
    };
    let (sk, _) = reference_sketch(&in_view);
    let c = if mirrored { mirror_constraint(*measure) } else { *measure };
    let value = sk.measure(&c)?;
    let label = dv(dimension::anchor(&sk, &c)?) + offset;
    Some(Resolved { text: format_value(&c, value, true), sketch: sk, constraint: c, label, value, dangling })
}

/// Works `d` out against the current part and view. `None` when it is no longer in the
/// model (a model dimension whose feature or dimension is gone) or does not suit the view.
/// `found` may hold the view's [`pickables`], to save finding them again.
pub fn resolve(d: &Dimension, cx: &ViewContext, found: Option<&Pickables>) -> Option<Resolved> {
    let axes = cx.orientation.axes();
    let offset = axes.project(DVec3::from_array(d.offset));
    match &d.source {
        DimSource::Model { feature, item: ModelItem::Sketch { constraint, kind } } => {
            resolve_sketch_dim(*feature, *constraint, kind, offset, cx)
        }
        DimSource::Model { feature, item: ModelItem::Depth } => {
            let depth = cx.part?.depths.iter().find(|x| x.feature == *feature)?;
            resolve_depth(depth, offset, cx)
        }
        DimSource::Reference { picks, measure, orientation } => {
            resolve_reference(picks, measure, *orientation, offset, cx, found)
        }
    }
}

/// A reference dimension on view `view` from `picks` made in a view looking from
/// `orientation`, with its value placed at `at` (all in the view's plane). `None` when the
/// picks do not make a dimension.
pub fn reference_dimension(
    view: u32,
    orientation: Orientation,
    picks: &[Pick2],
    at: DVec2,
) -> Option<Dimension> {
    let (sk, on) = reference_sketch(picks);
    let measure = dimension::infer(&sk, &on, kv(at))?;
    if !drawable(&measure) {
        return None;
    }
    let anchor = dv(dimension::anchor(&sk, &measure)?);
    let axes = orientation.axes();
    Some(Dimension {
        id: 0,
        view,
        source: DimSource::Reference {
            picks: picks.iter().map(|p| pick_to_model(p, &axes)).collect(),
            measure,
            orientation,
        },
        offset: arr(lift(&axes, at - anchor)),
    })
}

/// Moves a dimension's value by `delta` in the view's plane.
pub fn move_label(d: &mut Dimension, orientation: Orientation, delta: DVec2) {
    let o = DVec3::from_array(d.offset) + lift(&orientation.axes(), delta);
    d.offset = arr(o);
}

/// The model dimensions of `part` that `drawing` does not show yet, each in the first
/// front, top or side view that suits it (see `docs/re/drawings.md`, "Model dimensions").
pub fn model_dimensions(
    drawing: &crate::Drawing,
    part: &PartData,
    geometry: &std::collections::HashMap<u32, ViewGeometry>,
) -> Vec<Dimension> {
    use crate::ViewRole;
    let projection = drawing.sheet.projection;
    let scale = drawing.sheet.scale.factor();
    let mut views: Vec<&crate::View> = drawing
        .views
        .iter()
        .filter(|v| matches!(v.role, ViewRole::Front | ViewRole::Top | ViewRole::Side))
        .collect();
    let rank = |r: ViewRole| match r {
        ViewRole::Front => 0,
        ViewRole::Top => 1,
        ViewRole::Side => 2,
        _ => 3,
    };
    views.sort_by_key(|v| rank(v.role));
    let mut out: Vec<Dimension> = Vec::new();
    let place = |source: DimSource, out: &mut Vec<Dimension>| {
        let probe = Dimension { id: 0, view: 0, source, offset: [0.0; 3] };
        if drawing.dimensions.iter().chain(out.iter()).any(|d| d.same_model_item(&probe)) {
            return;
        }
        for v in &views {
            let cx = ViewContext {
                part: Some(part),
                geometry: geometry.get(&v.id),
                orientation: v.role.orientation(projection),
                scale,
            };
            // A zero distance (two points held level, say) says nothing on a drawing.
            if resolve(&probe, &cx, None).is_some_and(|r| r.value.abs() > 1e-9) {
                out.push(Dimension { view: v.id, ..probe });
                return;
            }
        }
    };
    for placed in &part.sketches {
        for (ci, c) in placed.sketch.constraints.iter().enumerate() {
            if !c.is_dimension() || !drawable(c) || placed.sketch.is_reference(ci) {
                continue;
            }
            let item = ModelItem::Sketch { constraint: ci, kind: c.label().to_string() };
            place(DimSource::Model { feature: placed.feature, item }, &mut out);
        }
    }
    for depth in &part.depths {
        place(DimSource::Model { feature: depth.feature, item: ModelItem::Depth }, &mut out);
    }
    out
}

/// Things on a view the Dimension tool can pick, in the view's plane: line and arc ends
/// and circle and arc centres, straight lines (pieces of one edge joined), whole circles
/// and arcs. Ellipses and traced curves are left out.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Pickables {
    pub points: Vec<DVec2>,
    pub lines: Vec<(DVec2, DVec2)>,
    pub circles: Vec<(DVec2, f64)>,
    /// Centre, radius, and the ends counter-clockwise.
    pub arcs: Vec<(DVec2, f64, DVec2, DVec2)>,
}

/// Joins collinear pieces that share an end, so an edge split into seen and hidden parts
/// is one line. Ends are found through a grid, so this stays fast on large views.
fn join_lines(lines: Vec<(DVec2, DVec2)>, tol: f64) -> Vec<(DVec2, DVec2)> {
    use std::collections::HashMap;
    let cell = (tol * 4.0).max(1e-12);
    let key = |p: DVec2| ((p.x / cell).floor() as i64, (p.y / cell).floor() as i64);
    let mut grid: HashMap<(i64, i64), Vec<(usize, DVec2)>> = HashMap::new();
    for (i, &(a, b)) in lines.iter().enumerate() {
        grid.entry(key(a)).or_default().push((i, a));
        grid.entry(key(b)).or_default().push((i, b));
    }
    let on_line = |(a, b): (DVec2, DVec2), p: DVec2| {
        let d = b - a;
        (d.perp_dot(p - a) / d.length()).abs() <= tol
    };
    let mut parent: Vec<usize> = (0..lines.len()).collect();
    fn root(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    for (i, &(a, b)) in lines.iter().enumerate() {
        for p in [a, b] {
            let (kx, ky) = key(p);
            for dx in -1..=1 {
                for dy in -1..=1 {
                    let Some(near) = grid.get(&(kx + dx, ky + dy)) else { continue };
                    for &(j, q) in near {
                        if j == i || q.distance(p) > tol {
                            continue;
                        }
                        let (c, d) = lines[j];
                        if on_line((a, b), c) && on_line((a, b), d) {
                            let (ri, rj) = (root(&mut parent, i), root(&mut parent, j));
                            parent[ri] = rj;
                        }
                    }
                }
            }
        }
    }
    let mut groups: HashMap<usize, Vec<usize>> = HashMap::new();
    for i in 0..lines.len() {
        let r = root(&mut parent, i);
        groups.entry(r).or_default().push(i);
    }
    let mut out: Vec<(usize, (DVec2, DVec2))> = groups
        .into_values()
        .map(|members| {
            let first = members[0];
            let (a, b) = lines[first];
            let dir = (b - a).normalize_or(DVec2::X);
            let (mut lo, mut hi) = ((0.0, a), (0.0, a));
            for &m in &members {
                let (c, d) = lines[m];
                for p in [c, d] {
                    let t = (p - a).dot(dir);
                    if t < lo.0 {
                        lo = (t, p);
                    }
                    if t > hi.0 {
                        hi = (t, p);
                    }
                }
            }
            (first, (lo.1, hi.1))
        })
        .collect();
    // Keep the views' own order, so results repeat.
    out.sort_by_key(|(first, _)| *first);
    out.into_iter().map(|(_, l)| l).collect()
}

/// A conic piece that is part of a circle: its centre, radius, and the angles it covers
/// counter-clockwise (start, sweep).
fn circular(path: &Path2) -> Option<(DVec2, f64, f64, f64)> {
    let Path2::Conic { c, u, v, t0, t1 } = path else { return None };
    let r = u.length();
    if r < 1e-12 || (v.length() - r).abs() > 1e-9 * r || u.dot(*v).abs() > 1e-9 * r * r {
        return None;
    }
    // c + u cos t + v sin t is at angle (angle of u) + sign * t.
    let sign = u.perp_dot(*v).signum();
    let au = u.y.atan2(u.x);
    let (a, b) = (au + sign * t0, au + sign * t1);
    let (start, sweep) = if b >= a { (a, b - a) } else { (b, a - b) };
    Some((*c, r, start.rem_euclid(TAU), sweep.min(TAU)))
}

/// Pieces of one circle: its centre, radius, and the angle ranges they cover.
type Ring = (DVec2, f64, Vec<(f64, f64)>);

/// Finds the pickable things of a view.
pub fn pickables(g: &ViewGeometry) -> Pickables {
    let tol = tolerance(g);
    let mut lines = Vec::new();
    // Circle pieces grouped by centre and radius, with the angle ranges they cover.
    let mut rings: Vec<Ring> = Vec::new();
    for l in &g.lines {
        match &l.path {
            Path2::Line { a, b } if a.distance(*b) > tol => lines.push((*a, *b)),
            p => {
                if let Some((c, r, start, sweep)) = circular(p) {
                    match rings.iter_mut().find(|(d, s, _)| d.distance(c) <= tol && (s - r).abs() <= tol) {
                        Some((_, _, spans)) => spans.push((start, sweep)),
                        None => rings.push((c, r, vec![(start, sweep)])),
                    }
                }
            }
        }
    }
    let lines = join_lines(lines, tol);
    let mut out = Pickables::default();
    for &(a, b) in &lines {
        out.points.push(a);
        out.points.push(b);
    }
    out.lines = lines;
    for (c, r, spans) in rings {
        out.points.push(c);
        let ang_tol = tol / r;
        for (start, sweep) in merge_spans(spans, ang_tol) {
            if sweep >= TAU - ang_tol {
                out.circles.push((c, r));
            } else {
                let a = c + DVec2::from_angle(start) * r;
                let b = c + DVec2::from_angle(start + sweep) * r;
                out.points.push(a);
                out.points.push(b);
                out.arcs.push((c, r, a, b));
            }
        }
    }
    let mut points: Vec<DVec2> = Vec::new();
    for p in out.points {
        if !points.iter().any(|q| q.distance(p) <= tol) {
            points.push(p);
        }
    }
    out.points = points;
    out
}

/// Joins angle ranges (start in [0, 2π), sweep) that touch or overlap, going round.
fn merge_spans(mut spans: Vec<(f64, f64)>, tol: f64) -> Vec<(f64, f64)> {
    spans.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut out: Vec<(f64, f64)> = Vec::new();
    for (s, w) in spans {
        match out.last_mut() {
            Some((ls, lw)) if s <= *ls + *lw + tol => *lw = (*lw).max(s + w - *ls),
            _ => out.push((s, w)),
        }
    }
    // The last range may run on past 2π into the first.
    if out.len() > 1 {
        let (fs, fw) = out[0];
        if let Some(&(ls, lw)) = out.last().filter(|(ls, lw)| ls + lw + tol >= fs + TAU) {
            out.pop();
            out[0] = (ls, (fs + fw + TAU - ls).max(lw));
        }
    }
    out.into_iter().map(|(s, w)| (s, w.min(TAU))).collect()
}

/// The pickable thing nearest to `at`, within `tol` (view-plane millimetres). Points come
/// before lines and curves, so a corner is easy to pick.
pub fn nearest(found: &Pickables, at: DVec2, tol: f64) -> Option<Pick2> {
    let best_point = found
        .points
        .iter()
        .map(|&p| (p.distance(at), p))
        .filter(|(d, _)| *d <= tol)
        .min_by(|a, b| a.0.total_cmp(&b.0));
    if let Some((_, p)) = best_point {
        return Some(Pick2::Point(p));
    }
    let mut best: Option<(f64, Pick2)> = None;
    let mut consider = |d: f64, p: Pick2| {
        if d <= tol && best.is_none_or(|(b, _)| d < b) {
            best = Some((d, p));
        }
    };
    for &(a, b) in &found.lines {
        let d = b - a;
        let t = ((at - a).dot(d) / d.length_squared()).clamp(0.0, 1.0);
        consider(at.distance(a + d * t), Pick2::Line(a, b));
    }
    for &(c, r) in &found.circles {
        consider((at.distance(c) - r).abs(), Pick2::Circle(c, r));
    }
    for &(c, r, a, b) in &found.arcs {
        let (aa, ab, ap) = ((a - c).to_angle(), (b - c).to_angle(), (at - c).to_angle());
        let sweep = (ab - aa).rem_euclid(TAU);
        if (ap - aa).rem_euclid(TAU) <= sweep {
            consider((at.distance(c) - r).abs(), Pick2::Arc(c, r, a, b));
        }
    }
    best.map(|(_, p)| p)
}

/// A dimension drawn on the sheet, in sheet millimetres.
#[derive(Debug, Clone, PartialEq)]
pub struct DimPicture {
    pub id: u32,
    /// Extension and dimension lines, broken round the value.
    pub lines: Vec<Path2>,
    /// Filled arrowheads.
    pub arrows: Vec<[DVec2; 3]>,
    pub text: Note,
    /// The box round the value, for picking and dragging it.
    pub text_box: (DVec2, DVec2),
    pub reference: bool,
    pub dangling: bool,
}

/// The parts of segment `a`-`b` outside the box `lo`-`hi`.
fn outside_box(a: DVec2, b: DVec2, lo: DVec2, hi: DVec2) -> Vec<(DVec2, DVec2)> {
    let d = b - a;
    let (mut t0, mut t1) = (0.0_f64, 1.0_f64);
    for (p, q) in [(-d.x, a.x - lo.x), (d.x, hi.x - a.x), (-d.y, a.y - lo.y), (d.y, hi.y - a.y)] {
        if p.abs() < 1e-15 {
            if q < 0.0 {
                return vec![(a, b)];
            }
        } else {
            let r = q / p;
            if p < 0.0 {
                t0 = t0.max(r);
            } else {
                t1 = t1.min(r);
            }
        }
    }
    if t0 >= t1 {
        return vec![(a, b)];
    }
    let mut out = Vec::new();
    if t0 > 1e-9 {
        out.push((a, a + d * t0));
    }
    if t1 < 1.0 - 1e-9 {
        out.push((a + d * t1, b));
    }
    out
}

/// Draws a resolved dimension on the sheet, for a view whose model origin falls at
/// `origin` on the sheet at scale `s`.
pub fn picture(id: u32, r: &Resolved, reference: bool, origin: DVec2, s: f64) -> DimPicture {
    let m = |p: Vec2| origin + dv(p) * s;
    let g = dimension::graphic(&r.sketch, &r.constraint, kv(r.label));
    let centre = origin + r.label * s;
    let half = DVec2::new(text_width(&r.text) * TEXT_HEIGHT * 0.5 + TEXT_GAP, TEXT_HEIGHT * 0.5 + TEXT_GAP);
    let (lo, hi) = (centre - half, centre + half);
    let mut lines = Vec::new();
    for (a, b) in &g.lines {
        for (p, q) in outside_box(m(*a), m(*b), lo, hi) {
            lines.push(Path2::Line { a: p, b: q });
        }
    }
    for &(c, rad, a0, sweep) in &g.arcs {
        let arc =
            Path2::Conic { c: m(c), u: DVec2::X * rad * s, v: DVec2::Y * rad * s, t0: a0, t1: a0 + sweep };
        let pts = arc.points(0.02);
        let mut run: Vec<DVec2> = Vec::new();
        for w in pts.windows(2) {
            for (p, q) in outside_box(w[0], w[1], lo, hi) {
                if run.last().is_some_and(|l| l.distance(p) < 1e-9) {
                    run.push(q);
                } else {
                    if run.len() > 1 {
                        lines.push(Path2::Poly(std::mem::take(&mut run)));
                    }
                    run = vec![p, q];
                }
            }
        }
        if run.len() > 1 {
            lines.push(Path2::Poly(run));
        }
    }
    let arrows = g
        .arrows
        .iter()
        .map(|&(tip, dir)| {
            let (tip, dir) = (m(tip), dv(dir).normalize_or(DVec2::X));
            let back = tip - dir * ARROW.0;
            let side = dir.perp() * (ARROW.1 * 0.5);
            [tip, back + side, back - side]
        })
        .collect();
    DimPicture {
        id,
        lines,
        arrows,
        text: Note {
            pos: centre - DVec2::Y * (TEXT_HEIGHT * 0.5),
            height: TEXT_HEIGHT,
            text: r.text.clone(),
            align: Align::Center,
        },
        text_box: (lo, hi),
        reference,
        dangling: r.dangling,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_drop_trailing_zeros() {
        assert_eq!(format_number(40.0), "40");
        assert_eq!(format_number(12.5), "12.5");
        assert_eq!(format_number(12.3456), "12.35");
        assert_eq!(format_number(-0.001), "0");
        let d = Constraint::Diameter { entity: 0, value: 8.0 };
        assert_eq!(format_value(&d, 8.0, false), "\u{d8}8");
        assert_eq!(format_value(&Constraint::Radius { entity: 0, value: 5.0 }, 5.0, true), "(R5)");
    }

    #[test]
    fn a_line_split_in_two_is_one_pick_and_a_split_circle_is_one_circle() {
        use crate::hlr::{LineKind, ViewLine};
        let line = |a: DVec2, b: DVec2, kind| ViewLine { kind, path: Path2::Line { a, b } };
        let ring = |t0: f64, t1: f64| ViewLine {
            kind: LineKind::Visible,
            path: Path2::Conic { c: DVec2::new(5.0, 5.0), u: DVec2::X * 2.0, v: DVec2::Y * 2.0, t0, t1 },
        };
        let g = ViewGeometry {
            hatch: Vec::new(),
            clip: None,
            lines: vec![
                line(DVec2::ZERO, DVec2::new(10.0, 0.0), LineKind::Visible),
                line(DVec2::new(10.0, 0.0), DVec2::new(30.0, 0.0), LineKind::Hidden),
                ring(0.0, 2.0),
                ring(2.0, TAU),
                ViewLine {
                    kind: LineKind::Visible,
                    path: Path2::Conic {
                        c: DVec2::new(20.0, 10.0),
                        u: DVec2::X * 3.0,
                        v: DVec2::Y * 3.0,
                        t0: 0.0,
                        t1: std::f64::consts::FRAC_PI_2,
                    },
                },
            ],
        };
        let p = pickables(&g);
        assert_eq!(p.lines, vec![(DVec2::ZERO, DVec2::new(30.0, 0.0))]);
        assert_eq!(p.circles, vec![(DVec2::new(5.0, 5.0), 2.0)]);
        assert_eq!(p.arcs.len(), 1);
        let (c, r, a, b) = p.arcs[0];
        assert_eq!((c, r), (DVec2::new(20.0, 10.0), 3.0));
        assert!(a.distance(DVec2::new(23.0, 10.0)) < 1e-12 && b.distance(DVec2::new(20.0, 13.0)) < 1e-12);
        // Corners come first; near a line, the line.
        assert_eq!(nearest(&p, DVec2::new(0.2, 0.1), 0.5), Some(Pick2::Point(DVec2::ZERO)));
        assert_eq!(
            nearest(&p, DVec2::new(15.0, 0.3), 0.5),
            Some(Pick2::Line(DVec2::ZERO, DVec2::new(30.0, 0.0)))
        );
        assert_eq!(nearest(&p, DVec2::new(5.0, 7.2), 0.5), Some(Pick2::Circle(DVec2::new(5.0, 5.0), 2.0)));
        assert_eq!(nearest(&p, DVec2::new(15.0, 9.0), 0.5), None);
    }

    #[test]
    fn reference_dimensions_follow_the_sketch_rules_and_show_in_parentheses() {
        let o = Orientation::Front;
        let line = Pick2::Line(DVec2::ZERO, DVec2::new(30.0, 40.0));
        let cx = ViewContext { part: None, geometry: None, orientation: o, scale: 1.0 };
        // Beside the line: its length. Below it: its horizontal extent.
        let d = reference_dimension(1, o, &[line], DVec2::new(5.0, 30.0)).expect("length");
        let r = resolve(&d, &cx, None).expect("resolved");
        assert_eq!((r.value, r.text.as_str()), (50.0, "(50)"));
        assert!(r.dangling, "no view geometry to attach to");
        let d = reference_dimension(1, o, &[line], DVec2::new(15.0, -8.0)).expect("horizontal");
        assert_eq!(resolve(&d, &cx, None).map(|r| r.value), Some(30.0));
        // The value goes where it was placed.
        assert!(resolve(&d, &cx, None).is_some_and(|r| r.label.distance(DVec2::new(15.0, -8.0)) < 1e-9));
        let circle = Pick2::Circle(DVec2::new(3.0, 4.0), 6.0);
        let d = reference_dimension(1, o, &[circle], DVec2::new(20.0, 20.0)).expect("diameter");
        assert_eq!(resolve(&d, &cx, None).map(|r| r.text), Some("(\u{d8}12)".into()));
    }

    #[test]
    fn reference_dimensions_turn_with_the_side_view() {
        // Made on the left view, shown on the right view: the same distance, mirrored.
        let left = Orientation::Left;
        let picks = [Pick2::Point(DVec2::new(-10.0, 0.0)), Pick2::Point(DVec2::new(20.0, 5.0))];
        let d = reference_dimension(3, left, &picks, DVec2::new(5.0, -10.0)).expect("distance");
        let cx = ViewContext { part: None, geometry: None, orientation: Orientation::Right, scale: 1.0 };
        let r = resolve(&d, &cx, None).expect("resolved");
        assert_eq!(r.value.abs(), 30.0);
        assert!(r.label.distance(DVec2::new(-5.0, -10.0)) < 1e-9, "{:?}", r.label);
        let cx = ViewContext { orientation: Orientation::Top, ..cx };
        assert_eq!(resolve(&d, &cx, None), None, "a top view does not see a side view's picks");
    }

    #[test]
    fn segments_are_broken_round_the_value() {
        let (lo, hi) = (DVec2::new(4.0, -1.0), DVec2::new(6.0, 1.0));
        let parts = outside_box(DVec2::ZERO, DVec2::new(10.0, 0.0), lo, hi);
        assert_eq!(
            parts,
            vec![(DVec2::ZERO, DVec2::new(4.0, 0.0)), (DVec2::new(6.0, 0.0), DVec2::new(10.0, 0.0))]
        );
        let parts = outside_box(DVec2::new(0.0, 5.0), DVec2::new(10.0, 5.0), lo, hi);
        assert_eq!(parts.len(), 1);
    }
}
