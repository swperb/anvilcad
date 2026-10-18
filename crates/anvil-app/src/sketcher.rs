//! Interactive sketch editing: tools, snapping, selection, dragging, dimensions and the
//! on-screen sketch overlay.
//!
//! The entity tools (rectangles, slots, polygons, arcs, circles) follow
//! `docs/re/sketch-entities.md` and the editing tools (fillet, chamfer, trim, extend,
//! split, offset, mirror, patterns, move, rotate, scale) follow `docs/re/sketch-tools.md`.
//! The geometry and relations they add come from `anvil_sketch`; this module turns clicks
//! into calls, applies the pointer inference of sketch-entities.md ("Inferencing and
//! automatic relations") and draws previews.

use crate::camera::Camera;
use anvil_kernel::{v2, Frame, Vec2};
use anvil_model::SketchFeature;
use anvil_sketch::curve::{nearest_on, Curve2};
use anvil_sketch::dimension::{self, Pick};
use anvil_sketch::entities::{
    three_point_arc, PolygonCircle, RectConstruction, RectOptions, SlotLength, SlotOptions, TangentZone,
    POLYGON_SIDES,
};
use anvil_sketch::tools::{ChamferKind, FilletOptions, TrimOptions};
use anvil_sketch::transform::{
    CapEnds, CircularPattern, LinearPattern, OffsetOptions, Transform2, TransformOptions,
};
use anvil_sketch::{
    Constraint, EntityId, Geom, PointId, Renumbering, Sketch, SketchError, SolveResult, ORIGIN,
};
use eframe::egui::{self, Align2, Color32, FontId, Painter, Pos2, Rect, Response, Shape, Stroke};
use std::f64::consts::{PI, TAU};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tool {
    Select,
    Line,
    Centerline,
    Rectangle,
    CenterRectangle,
    Rectangle3Corner,
    Rectangle3Center,
    Parallelogram,
    StraightSlot,
    CenterSlot,
    ArcSlot3,
    CenterArcSlot,
    Polygon,
    Circle,
    PerimeterCircle,
    Arc,
    TangentArc,
    Arc3,
    Point,
    Dimension,
    Fillet,
    Chamfer,
    Trim,
    Extend,
    Split,
    Offset,
    Mirror,
    LinearPattern,
    CircularPattern,
    Move,
    Copy,
    Rotate,
    Scale,
}

impl Tool {
    pub fn label(&self) -> &'static str {
        match self {
            Tool::Select => "Select",
            Tool::Line => "Line",
            Tool::Centerline => "Centerline",
            Tool::Rectangle => "Rectangle",
            Tool::CenterRectangle => "Center Rectangle",
            Tool::Rectangle3Corner => "3 Point Corner Rectangle",
            Tool::Rectangle3Center => "3 Point Center Rectangle",
            Tool::Parallelogram => "Parallelogram",
            Tool::StraightSlot => "Straight Slot",
            Tool::CenterSlot => "Centerpoint Straight Slot",
            Tool::ArcSlot3 => "3 Point Arc Slot",
            Tool::CenterArcSlot => "Centerpoint Arc Slot",
            Tool::Polygon => "Polygon",
            Tool::Circle => "Circle",
            Tool::PerimeterCircle => "Perimeter Circle",
            Tool::Arc => "Centerpoint Arc",
            Tool::TangentArc => "Tangent Arc",
            Tool::Arc3 => "3 Point Arc",
            Tool::Point => "Point",
            Tool::Dimension => "Dimension",
            Tool::Fillet => "Sketch Fillet",
            Tool::Chamfer => "Sketch Chamfer",
            Tool::Trim => "Trim",
            Tool::Extend => "Extend",
            Tool::Split => "Split",
            Tool::Offset => "Offset",
            Tool::Mirror => "Mirror",
            Tool::LinearPattern => "Linear Sketch Pattern",
            Tool::CircularPattern => "Circular Sketch Pattern",
            Tool::Move => "Move",
            Tool::Copy => "Copy",
            Tool::Rotate => "Rotate",
            Tool::Scale => "Scale",
        }
    }

    /// The shorter name the command bar shows, to keep the bar narrow. The full name is
    /// on the tool's button in the sketch panel.
    pub fn bar_label(&self) -> &'static str {
        match self {
            Tool::CenterRectangle => "Center Rect",
            Tool::Rectangle3Corner => "3 Pt Rectangle",
            Tool::Rectangle3Center => "3 Pt Center Rect",
            Tool::CenterSlot => "Center Slot",
            Tool::ArcSlot3 => "Arc Slot",
            Tool::CenterArcSlot => "Center Arc Slot",
            Tool::PerimeterCircle => "Perim. Circle",
            Tool::Arc => "Arc",
            Tool::Arc3 => "3 Pt Arc",
            Tool::Fillet => "Fillet",
            Tool::Chamfer => "Chamfer",
            Tool::LinearPattern => "Pattern",
            Tool::CircularPattern => "Circ. Pattern",
            t => t.label(),
        }
    }

    /// Clicks that make a shape with the tools that build one in a single step.
    fn clicks_needed(self) -> Option<usize> {
        Some(match self {
            Tool::CenterRectangle | Tool::Polygon | Tool::TangentArc => 2,
            Tool::Rectangle3Corner
            | Tool::Rectangle3Center
            | Tool::Parallelogram
            | Tool::StraightSlot
            | Tool::CenterSlot
            | Tool::PerimeterCircle
            | Tool::Arc3 => 3,
            Tool::ArcSlot3 | Tool::CenterArcSlot => 4,
            _ => return None,
        })
    }

    /// Tools that act on the selection from the panel; clicks pick entities for them.
    pub fn uses_selection(self) -> bool {
        matches!(
            self,
            Tool::Offset
                | Tool::Mirror
                | Tool::LinearPattern
                | Tool::CircularPattern
                | Tool::Move
                | Tool::Copy
                | Tool::Rotate
                | Tool::Scale
        )
    }

    /// What the tool asks for next, given how many picks it has.
    pub fn prompt(self, picks: usize) -> &'static str {
        match (self, picks) {
            (Tool::CenterRectangle, 0) | (Tool::Polygon, 0) => "Click the centre.",
            (Tool::CenterRectangle, _) => "Click a corner.",
            (Tool::Polygon, _) => "Click a corner of the polygon.",
            (Tool::Rectangle3Corner | Tool::Parallelogram, 0) => "Click the first corner.",
            (Tool::Rectangle3Corner | Tool::Parallelogram, 1) => "Click the end of the first edge.",
            (Tool::Rectangle3Corner, _) => "Click to set the width.",
            (Tool::Parallelogram, _) => "Click the third corner.",
            (Tool::Rectangle3Center, 0) => "Click the centre.",
            (Tool::Rectangle3Center, 1) => "Click the middle of a side.",
            (Tool::Rectangle3Center, _) => "Click to set the other side.",
            (Tool::StraightSlot, 0) => "Click the centre of the first end.",
            (Tool::StraightSlot, 1) => "Click the centre of the other end.",
            (Tool::CenterSlot, 0) => "Click the slot's centre.",
            (Tool::CenterSlot, 1) => "Click the centre of one end.",
            (Tool::StraightSlot | Tool::CenterSlot, _) => "Click to set the width.",
            (Tool::ArcSlot3, 0) => "Click the start of the slot's arc.",
            (Tool::ArcSlot3, 1) => "Click the end of the arc.",
            (Tool::ArcSlot3, 2) => "Click a point on the arc.",
            (Tool::CenterArcSlot, 0) => "Click the arc's centre.",
            (Tool::CenterArcSlot, 1) => "Click the start of the arc.",
            (Tool::CenterArcSlot, 2) => "Move round and click the end of the arc.",
            (Tool::ArcSlot3 | Tool::CenterArcSlot, _) => "Click to set the width.",
            (Tool::PerimeterCircle, _) => {
                "Click three points on the circle (right-click after two for a diameter)."
            }
            (Tool::Arc3, 0) => "Click the start.",
            (Tool::Arc3, 1) => "Click the end.",
            (Tool::Arc3, _) => "Click a point on the arc.",
            (Tool::TangentArc, 0) => "Click the end of a line or arc.",
            (Tool::TangentArc, _) => {
                "Leave along the curve for a tangent arc, across it for a normal arc; click the end."
            }
            (Tool::Fillet, _) => "Click a corner, or two curves.",
            (Tool::Chamfer, _) => "Click a corner where two lines meet, or two lines.",
            (Tool::Trim, _) => "Click the part of a curve to trim away.",
            (Tool::Extend, _) => "Click a line or arc near the end to extend.",
            (Tool::Split, _) => "Click a curve where it should split (two clicks on a circle).",
            (Tool::Mirror, _) => "Select the entities to mirror, then the mirror line last.",
            (t, _) if t.uses_selection() => "Select the entities, set the options, then Apply.",
            _ => "",
        }
    }
}

/// The toolbar families: one button per family shows the last tool used in it
/// (sketch-entities.md, "Tool families and the property panel", Decision).
pub const DRAW_FAMILIES: [&[Tool]; 7] = [
    &[Tool::Line, Tool::Centerline],
    &[
        Tool::Rectangle,
        Tool::CenterRectangle,
        Tool::Rectangle3Corner,
        Tool::Rectangle3Center,
        Tool::Parallelogram,
    ],
    &[Tool::StraightSlot, Tool::CenterSlot, Tool::ArcSlot3, Tool::CenterArcSlot],
    &[Tool::Polygon],
    &[Tool::Circle, Tool::PerimeterCircle],
    &[Tool::Arc, Tool::TangentArc, Tool::Arc3],
    &[Tool::Point],
];

/// The editing tool families.
pub const MODIFY_FAMILIES: [&[Tool]; 6] = [
    &[Tool::Fillet, Tool::Chamfer],
    &[Tool::Trim, Tool::Extend, Tool::Split],
    &[Tool::Offset],
    &[Tool::Mirror],
    &[Tool::LinearPattern, Tool::CircularPattern],
    &[Tool::Move, Tool::Copy, Tool::Rotate, Tool::Scale],
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkSel {
    Point(PointId),
    Entity(EntityId),
    Constraint(usize),
}

/// A click location, possibly snapped to an existing sketch point, a line's midpoint or a
/// curve.
#[derive(Clone, Copy, Debug)]
struct Click {
    p: Vec2,
    snap: Option<PointId>,
    /// The click is on this curve (and not on a point): the new point is held on it.
    on: Option<EntityId>,
    /// The click is on this line's midpoint marker.
    mid: Option<EntityId>,
}

impl Click {
    fn at(p: Vec2) -> Click {
        Click { p, snap: None, on: None, mid: None }
    }
}

/// How the Trim tool cuts (sketch-tools.md, "Trim Entities").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrimMode {
    Power,
    Corner,
    Inside,
    Outside,
}

/// Which sketch chamfer the Chamfer tool makes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChamferType {
    AngleDistance,
    TwoDistances,
    EqualDistance,
}

/// The options shown in the sketch panel for the active tool.
#[derive(Clone, Debug, PartialEq)]
pub struct ToolParams {
    /// Automatic relations from the pointer (sketch-entities.md, "Inferencing").
    pub inference: bool,
    pub rect_construction: RectConstruction,
    pub slot: SlotOptions,
    pub polygon_sides: u32,
    pub polygon_circle: PolygonCircle,
    pub fillet: FilletOptions,
    pub chamfer: ChamferType,
    pub chamfer_d1: f64,
    pub chamfer_d2: f64,
    pub chamfer_angle: f64,
    pub chamfer_keep: bool,
    pub trim_mode: TrimMode,
    pub trim: TrimOptions,
    pub offset: OffsetOptions,
    pub mirror_copy: bool,
    pub linear: LinearPattern,
    pub circular: CircularPattern,
    pub delta: Vec2,
    pub center: Vec2,
    pub degrees: f64,
    pub factor: f64,
    pub copies: u32,
    pub copy: bool,
    pub keep_relations: bool,
}

impl Default for ToolParams {
    fn default() -> Self {
        ToolParams {
            inference: true,
            rect_construction: RectConstruction::Corners,
            slot: SlotOptions::default(),
            polygon_sides: 6,
            polygon_circle: PolygonCircle::Inscribed,
            fillet: FilletOptions::default(),
            chamfer: ChamferType::TwoDistances,
            chamfer_d1: 5.0,
            chamfer_d2: 5.0,
            chamfer_angle: 45.0,
            chamfer_keep: true,
            trim_mode: TrimMode::Power,
            trim: TrimOptions::default(),
            offset: OffsetOptions::default(),
            mirror_copy: true,
            linear: LinearPattern::default(),
            circular: CircularPattern::default(),
            delta: v2(10.0, 0.0),
            center: Vec2::ZERO,
            degrees: 90.0,
            factor: 2.0,
            copies: 1,
            copy: false,
            keep_relations: false,
        }
    }
}

impl ToolParams {
    fn chamfer_kind(&self) -> ChamferKind {
        match self.chamfer {
            ChamferType::AngleDistance => {
                ChamferKind::AngleDistance { distance: self.chamfer_d1, degrees: self.chamfer_angle }
            }
            ChamferType::TwoDistances => ChamferKind::Distances(self.chamfer_d1, self.chamfer_d2),
            ChamferType::EqualDistance => ChamferKind::Equal(self.chamfer_d1),
        }
    }
}

pub struct SketchSession {
    pub feature: u32,
    pub frame: Frame,
    pub tool: Tool,
    pending: Vec<Click>,
    /// What the Dimension tool has picked so far.
    pub picks: Vec<Pick>,
    /// A dimension just added that over-defines the sketch. It is held as a reference
    /// dimension until the user chooses what to do with it.
    pub over_define: Option<usize>,
    pub selection: Vec<SkSel>,
    pub hover: Option<SkSel>,
    dragging: Option<SkSel>,
    pub status: SolveResult,
    /// Dimension currently being edited and its text.
    pub dim_edit: Option<(usize, String)>,
    mouse: Option<Vec2>,
    pub params: ToolParams,
    /// Tools used, most recent first, so each toolbar family shows its last one.
    pub recent: Vec<Tool>,
    /// Why the last pick did nothing, shown in the panel.
    pub notice: Option<String>,
    /// Entities the editing tools have picked so far, with where they were picked.
    entity_picks: Vec<(EntityId, Vec2)>,
    /// The first fillet of a run, which later fillets are made `Equal` to.
    fillet_first: Option<EntityId>,
    /// The curve and end point a tangent arc starts from.
    tangent_from: Option<(EntityId, PointId)>,
    tangent_zone: Option<TangentZone>,
    /// The signed angle the pointer has swept round an arc's centre since its start.
    swept: f64,
    swept_last: Option<f64>,
    shift_held: bool,
    ctrl_held: bool,
}

/// What a frame of interaction did.
#[derive(Default)]
pub struct Outcome {
    pub modified: bool,
    pub dragging: bool,
}

const PICK_PX: f64 = 8.0;

/// Moves expression bindings to where their dimensions went in an edit.
pub fn remap_driven(sf: &mut SketchFeature, map: &[Option<usize>]) {
    sf.driven.retain_mut(|d| match map.get(d.constraint).copied().flatten() {
        Some(n) => {
            d.constraint = n;
            true
        }
        None => false,
    });
}

/// Twice the distance from `p` to the line through `a` and `b`: a slot's width.
fn width_from(a: Vec2, b: Vec2, p: Vec2) -> f64 {
    let d = (b - a).normalized();
    2.0 * d.cross(p - a).abs()
}

impl SketchSession {
    pub fn new(feature: u32, frame: Frame) -> SketchSession {
        SketchSession {
            feature,
            frame,
            tool: Tool::Line,
            pending: Vec::new(),
            picks: Vec::new(),
            over_define: None,
            selection: Vec::new(),
            hover: None,
            dragging: None,
            status: SolveResult::default(),
            dim_edit: None,
            mouse: None,
            params: ToolParams::default(),
            recent: Vec::new(),
            notice: None,
            entity_picks: Vec::new(),
            fillet_first: None,
            tangent_from: None,
            tangent_zone: None,
            swept: 0.0,
            swept_last: None,
            shift_held: false,
            ctrl_held: false,
        }
    }

    pub fn set_tool(&mut self, t: Tool) {
        self.tool = t;
        self.recent.retain(|&r| r != t);
        self.recent.insert(0, t);
        self.fillet_first = None;
        self.notice = None;
        self.cancel_pending();
    }

    /// The tool a toolbar family shows: the last one used from it, or its first.
    pub fn family_tool(&self, family: &[Tool]) -> Tool {
        self.recent.iter().copied().find(|t| family.contains(t)).unwrap_or(family[0])
    }

    pub fn cancel_pending(&mut self) {
        self.pending.clear();
        self.picks.clear();
        self.entity_picks.clear();
        self.tangent_from = None;
        self.tangent_zone = None;
        self.swept = 0.0;
        self.swept_last = None;
    }

    pub fn has_pending(&self) -> bool {
        !self.pending.is_empty() || !self.picks.is_empty() || !self.entity_picks.is_empty()
    }

    /// Picks made so far by the active tool.
    pub fn pick_count(&self) -> usize {
        self.pending.len() + self.entity_picks.len()
    }

    fn to_sketch(&self, cam: &Camera, rect: Rect, p: Pos2) -> Option<Vec2> {
        let (o, d) = cam.ray(p, rect);
        self.frame.intersect_ray(o, d)
    }

    fn to_screen(&self, cam: &Camera, rect: Rect, p: Vec2) -> Pos2 {
        cam.project(self.frame.to_world(p), rect)
    }

    fn inferring(&self) -> bool {
        self.params.inference && !self.ctrl_held
    }

    /// Where a click lands: on an existing point, else (with inference) on a line's
    /// midpoint or on a curve, else where it is.
    fn snap(&self, sk: &Sketch, p: Vec2, radius: f64) -> Click {
        if let Some(id) = sk.pick_point(p, radius) {
            return Click { p: sk.pos(id), snap: Some(id), on: None, mid: None };
        }
        if self.inferring() {
            for (e, ent) in sk.entities.iter().enumerate() {
                if let Geom::Line { a, b } = ent.geom {
                    let m = (sk.pos(a) + sk.pos(b)) * 0.5;
                    if m.distance(p) <= radius {
                        return Click { p: m, snap: None, on: None, mid: Some(e) };
                    }
                }
            }
            if let Some(e) = sk.pick_entity(p, radius) {
                if let Some(c) = Curve2::of(sk, e) {
                    return Click { p: nearest_on(&c, p), snap: None, on: Some(e), mid: None };
                }
            }
        }
        Click::at(p)
    }

    fn point_for(sk: &mut Sketch, c: Click) -> PointId {
        c.snap.unwrap_or_else(|| sk.add_point(c.p))
    }

    /// The relations a click infers for a new point there.
    fn infer_point(sk: &mut Sketch, c: &Click, p: PointId) {
        if let Some(l) = c.mid {
            sk.add_constraint(Constraint::Midpoint { p, line: l });
        } else if let Some(e) = c.on {
            let rel = if sk.entities[e].is_line() {
                Constraint::PointOnLine { p, line: e }
            } else {
                Constraint::PointOnCircle { p, entity: e }
            };
            sk.add_constraint(rel);
        }
    }

    /// After a builder made a shape from `clicks`, joins the shape to what the clicks
    /// snapped to: a click on an existing point merges the new point there into it, and a
    /// click on a curve or a midpoint holds the new point there.
    fn infer(&self, sk: &mut Sketch, clicks: &[Click], first_new: PointId) -> Renumbering {
        let mut ren = Renumbering::identity(sk);
        for c in clicks {
            let tol = 1e-9 * c.p.length().max(1.0);
            let Some(q) = (first_new..sk.points.len()).find(|&i| sk.pos(i).distance(c.p) <= tol) else {
                continue;
            };
            match c.snap {
                Some(p) if p < first_new => {
                    if let Ok(r) = sk.merge_points(p, q) {
                        ren = ren.then(r);
                    }
                }
                _ if self.inferring() => Self::infer_point(sk, c, q),
                _ => {}
            }
        }
        ren
    }

    /// Picks whatever is under the cursor: points first, then dimension labels, then curves.
    fn pick(
        &self,
        sk: &Sketch,
        cam: &Camera,
        rect: Rect,
        screen: Pos2,
        p: Vec2,
        radius: f64,
    ) -> Option<SkSel> {
        if let Some(id) = sk.pick_point(p, radius) {
            return Some(SkSel::Point(id));
        }
        for ci in 0..sk.constraints.len() {
            if let Some(anchor) = dimension::label_position(sk, ci) {
                let s = self.to_screen(cam, rect, anchor);
                if (s.x - screen.x).abs() < 28.0 && (s.y - screen.y).abs() < 9.0 {
                    return Some(SkSel::Constraint(ci));
                }
            }
        }
        sk.pick_entity(p, radius).map(SkSel::Entity)
    }

    /// Builds the active tool's shape from click positions `c` (the last one being the
    /// pick that completes it). Returns the first edge of a 3 point rectangle or
    /// parallelogram, which gets automatic horizontal or vertical.
    fn build(&self, sk: &mut Sketch, c: &[Vec2]) -> Result<Option<EntityId>, SketchError> {
        let p = &self.params;
        let center_cons = match p.rect_construction {
            RectConstruction::None => RectConstruction::Corners,
            other => other,
        };
        let opts = |construction| RectOptions { construction, equal_sides: self.shift_held };
        match (self.tool, c) {
            (Tool::CenterRectangle, [a, b]) => {
                sk.add_center_rectangle(*a, *b, opts(center_cons)).map(|_| None)
            }
            (Tool::Rectangle3Corner, [a, b, d]) => sk
                .add_3pt_corner_rectangle(*a, *b, *d, opts(RectConstruction::None))
                .map(|r| Some(r.sides[0])),
            (Tool::Rectangle3Center, [a, b, d]) => {
                sk.add_3pt_center_rectangle(*a, *b, *d, opts(center_cons)).map(|r| Some(r.sides[0]))
            }
            (Tool::Parallelogram, [a, b, d]) => {
                sk.add_parallelogram(*a, *b, *d, opts(RectConstruction::None)).map(|r| Some(r.sides[0]))
            }
            (Tool::StraightSlot, [a, b, w]) => {
                sk.add_straight_slot(*a, *b, width_from(*a, *b, *w), p.slot).map(|_| None)
            }
            (Tool::CenterSlot, [m, e, w]) => {
                sk.add_center_straight_slot(*m, *e, width_from(*m, *e, *w), p.slot).map(|_| None)
            }
            (Tool::ArcSlot3, [s, e, t, w]) => {
                let (o, a, _) = three_point_arc(*s, *e, *t).ok_or(SketchError::Degenerate)?;
                let width = 2.0 * (w.distance(o) - a.distance(o)).abs();
                sk.add_3pt_arc_slot(*s, *e, *t, width, p.slot).map(|_| None)
            }
            (Tool::CenterArcSlot, [o, s, e, w]) => {
                let width = 2.0 * (w.distance(*o) - s.distance(*o)).abs();
                sk.add_centerpoint_arc_slot(*o, *s, *e, self.swept, width, p.slot).map(|_| None)
            }
            (Tool::Polygon, [c, v]) => {
                sk.add_polygon(*c, *v, p.polygon_sides, p.polygon_circle).map(|_| None)
            }
            (Tool::PerimeterCircle, [a, b, d]) => sk.add_circle_3pt(*a, *b, *d).map(|_| None),
            (Tool::PerimeterCircle, [a, b]) => sk.add_circle_2pt(*a, *b).map(|_| None),
            (Tool::Arc3, [a, b, t]) => sk.add_3pt_arc(*a, *b, *t).map(|_| None),
            (Tool::Arc, [o, s, e]) => sk.add_centerpoint_arc(*o, *s, *e, self.swept).map(|_| None),
            (Tool::TangentArc, [_, e]) => {
                let (from, at) = self.tangent_from.ok_or(SketchError::Degenerate)?;
                sk.add_tangent_arc(from, at, *e, self.tangent_zone.unwrap_or(TangentZone::Forward))
                    .map(|_| None)
            }
            _ => Err(SketchError::Degenerate),
        }
    }

    /// Completes a shape from the pending clicks and `last` on the real sketch.
    fn commit_shape(&mut self, sf: &mut SketchFeature, last: Click) -> bool {
        let mut clicks = self.pending.clone();
        clicks.push(last);
        let pts: Vec<Vec2> = clicks.iter().map(|c| c.p).collect();
        let first_new = sf.sketch.points.len();
        match self.build(&mut sf.sketch, &pts) {
            Ok(edge) => {
                if let (Some(l), true) = (edge, self.inferring()) {
                    auto_hv(&mut sf.sketch, l);
                }
                // A tangent arc already shares its start point.
                let skip = usize::from(self.tool == Tool::TangentArc);
                let ren = self.infer(&mut sf.sketch, &clicks[skip..], first_new);
                remap_driven(sf, &ren.constraints);
                self.pending.clear();
                self.tangent_from = None;
                self.tangent_zone = None;
                self.notice = None;
                true
            }
            Err(e) => {
                // The tool waits for another pick.
                self.notice = Some(format!("{}: {e}.", self.tool.label()));
                false
            }
        }
    }

    pub fn handle(
        &mut self,
        sf: &mut SketchFeature,
        resp: &Response,
        cam: &Camera,
        rect: Rect,
        ctx: &egui::Context,
    ) -> Outcome {
        let mut out = Outcome::default();
        let radius = PICK_PX / cam.pixels_per_unit(rect);
        let hover_screen = resp.hover_pos();
        self.mouse = hover_screen.and_then(|s| self.to_sketch(cam, rect, s));
        self.hover = match (hover_screen, self.mouse) {
            (Some(s), Some(p)) => self.pick(&sf.sketch, cam, rect, s, p, radius),
            _ => None,
        };

        let (esc, delete, shift, shift_only, ctrl) = ctx.input(|i| {
            (
                i.key_pressed(egui::Key::Escape),
                i.key_pressed(egui::Key::Delete),
                i.modifiers.shift || i.modifiers.ctrl,
                i.modifiers.shift,
                i.modifiers.ctrl || i.modifiers.command,
            )
        });
        self.shift_held = shift_only;
        self.ctrl_held = ctrl;
        if esc {
            if !self.picks.is_empty() {
                // Esc takes back the last pick.
                self.picks.pop();
            } else if self.has_pending() {
                self.cancel_pending();
            } else if self.tool != Tool::Select {
                self.tool = Tool::Select;
            } else {
                self.selection.clear();
            }
        }
        if delete && !self.selection.is_empty() && !ctx.wants_keyboard_input() {
            self.delete_selection(sf);
            out.modified = true;
        }
        if resp.secondary_clicked() {
            // A right-click after two picks makes a perimeter circle on them as a diameter.
            if self.tool == Tool::PerimeterCircle && self.pending.len() == 2 {
                let last = self.pending.pop();
                if let Some(last) = last {
                    out.modified |= self.commit_shape(sf, last);
                }
            }
            self.cancel_pending();
        }

        let Some(mouse) = self.mouse else {
            return out;
        };
        let screen = hover_screen.or(resp.interact_pointer_pos()).unwrap_or_default();
        self.track_pointer(&sf.sketch, mouse, radius);

        // Dragging in the select tool.
        if self.tool == Tool::Select {
            if resp.drag_started_by(egui::PointerButton::Primary) {
                let start = ctx
                    .input(|i| i.pointer.press_origin())
                    .and_then(|s| self.to_sketch(cam, rect, s).map(|p| (s, p)));
                if let Some((s, p)) = start {
                    self.dragging = match self.pick(&sf.sketch, cam, rect, s, p, radius) {
                        Some(SkSel::Point(ORIGIN)) => None,
                        other => other,
                    };
                }
            }
            if let Some(d) = self.dragging {
                if resp.dragged_by(egui::PointerButton::Primary) {
                    match d {
                        SkSel::Point(p) => {
                            self.status = sf.sketch.drag_point(p, mouse);
                        }
                        SkSel::Entity(e) => {
                            if let Some(Geom::Circle { c, .. }) = sf.sketch.entities.get(e).map(|x| x.geom) {
                                let r = sf.sketch.pos(c).distance(mouse);
                                let backup = sf.sketch.clone();
                                if let Geom::Circle { r: rr, .. } = &mut sf.sketch.entities[e].geom {
                                    *rr = r;
                                }
                                let st = sf.sketch.solve();
                                if st.converged {
                                    self.status = st;
                                } else {
                                    sf.sketch = backup;
                                }
                            }
                        }
                        SkSel::Constraint(ci) => {
                            // Dragging a dimension moves its value.
                            let sk = &mut sf.sketch;
                            if let Some(a) = sk.constraints.get(ci).and_then(|c| dimension::anchor(sk, c)) {
                                sk.annotation_mut(ci).offset = mouse - a;
                            }
                        }
                    }
                    out.modified = true;
                    out.dragging = true;
                }
                if resp.drag_stopped() {
                    self.dragging = None;
                    out.modified = true;
                }
            }
        }

        if resp.clicked_by(egui::PointerButton::Primary) {
            out.modified |= self.click(sf, cam, rect, screen, mouse, radius, shift);
        }

        if out.modified && !out.dragging {
            self.status = sf.sketch.solve();
        }
        out
    }

    /// Follows the pointer between clicks: the angle swept round an arc's centre, and the
    /// tangent arc's intent zone.
    fn track_pointer(&mut self, sk: &Sketch, mouse: Vec2, radius: f64) {
        if matches!(self.tool, Tool::Arc | Tool::CenterArcSlot) && self.pending.len() == 2 {
            let c = self.pending[0].p;
            let a = (mouse - c).angle();
            if mouse.distance(c) > 1e-12 {
                if let Some(last) = self.swept_last {
                    self.swept += (a - last + PI).rem_euclid(TAU) - PI;
                }
                self.swept_last = Some(a);
            }
        }
        if let (Tool::TangentArc, [start], Some((from, at))) =
            (self.tool, self.pending.as_slice(), self.tangent_from)
        {
            let d = mouse - start.p;
            if d.length() <= radius {
                // Back at the end point: choose afresh.
                self.tangent_zone = None;
            } else if self.tangent_zone.is_none() {
                if let Some(t) = sk.leaving_direction(from, at) {
                    self.tangent_zone = Some(TangentZone::from_directions(t, d));
                }
            }
        }
    }

    /// A primary click in the viewport. Returns true if the sketch changed.
    #[allow(clippy::too_many_arguments)]
    fn click(
        &mut self,
        sf: &mut SketchFeature,
        cam: &Camera,
        rect: Rect,
        screen: Pos2,
        mouse: Vec2,
        radius: f64,
        shift: bool,
    ) -> bool {
        let mut modified = false;
        if let Some(n) = self.tool.clicks_needed() {
            let c = self.snap(&sf.sketch, mouse, radius);
            if self.tool == Tool::TangentArc && self.pending.is_empty() {
                // A tangent arc starts at the end of a line or arc.
                let from = c.snap.and_then(|p| {
                    let ends = sf.sketch.entities_ending_at(p);
                    let solid = ends.iter().copied().find(|&e| !sf.sketch.entities[e].construction);
                    solid.or(ends.first().copied()).map(|e| (e, p))
                });
                match from {
                    Some(f) => {
                        self.tangent_from = Some(f);
                        self.tangent_zone = None;
                        self.pending.push(c);
                    }
                    None => self.notice = Some("Start a tangent arc at the end of a line or arc.".into()),
                }
            } else if self.pending.len() + 1 >= n {
                modified = self.commit_shape(sf, c);
            } else {
                if matches!(self.tool, Tool::CenterArcSlot) && self.pending.len() == 1 {
                    self.swept = 0.0;
                    self.swept_last = Some((c.p - self.pending[0].p).angle());
                }
                self.pending.push(c);
            }
            return modified;
        }
        let sk = &mut sf.sketch;
        match self.tool {
            Tool::Select => {
                let hit = self.pick(sk, cam, rect, screen, mouse, radius);
                match hit {
                    Some(h) => {
                        if shift {
                            if let Some(i) = self.selection.iter().position(|s| *s == h) {
                                self.selection.remove(i);
                            } else {
                                self.selection.push(h);
                            }
                        } else {
                            self.selection = vec![h];
                        }
                        if let SkSel::Constraint(ci) = h {
                            self.begin_dim_edit(sf, ci);
                        }
                    }
                    None => {
                        if !shift {
                            self.selection.clear();
                        }
                    }
                }
            }
            t if t.uses_selection() => {
                // Each click adds an entity to the selection or takes it out.
                if let Some(e) = sk.pick_entity(mouse, radius) {
                    let h = SkSel::Entity(e);
                    if let Some(i) = self.selection.iter().position(|s| *s == h) {
                        self.selection.remove(i);
                    } else {
                        self.selection.push(h);
                    }
                }
            }
            Tool::Line | Tool::Centerline => {
                let c = self.snap(sk, mouse, radius);
                if let Some(start) = self.pending.pop() {
                    let a = Self::point_for(sk, start);
                    let b = Self::point_for(sk, c);
                    if a != b {
                        if self.inferring() {
                            if start.snap.is_none() {
                                Self::infer_point(sk, &start, a);
                            }
                            if c.snap.is_none() {
                                Self::infer_point(sk, &c, b);
                            }
                        }
                        let line = sk.add_line(a, b);
                        sk.entities[line].construction = self.tool == Tool::Centerline;
                        if self.inferring() {
                            auto_hv(sk, line);
                            auto_tangent(sk, line, a);
                        }
                        modified = true;
                    }
                    // Clicking onto an existing point closes the chain.
                    if c.snap.is_none() || a == b {
                        self.pending.push(Click { p: sk.pos(b), snap: Some(b), on: None, mid: None });
                    }
                } else {
                    self.pending.push(c);
                }
            }
            Tool::Rectangle => {
                let c = self.snap(sk, mouse, radius);
                if let Some(first) = self.pending.pop() {
                    let first_new = sk.points.len();
                    let opts =
                        RectOptions { construction: RectConstruction::None, equal_sides: self.shift_held };
                    match sk.add_corner_rectangle(first.p, c.p, opts) {
                        Ok(_) => {
                            let ren = self.infer(sk, &[first, c], first_new);
                            remap_driven(sf, &ren.constraints);
                            modified = true;
                        }
                        Err(_) => self.pending.push(first),
                    }
                } else {
                    self.pending.push(c);
                }
            }
            Tool::Point => {
                // A lone point, e.g. where a hole goes. Clicking an existing point adds
                // nothing; a click on a curve or a midpoint holds the point there.
                let c = self.snap(sk, mouse, radius);
                if c.snap.is_none() {
                    let p = sk.add_point(c.p);
                    Self::infer_point(sk, &c, p);
                    modified = true;
                }
            }
            Tool::Circle => {
                let c = self.snap(sk, mouse, radius);
                if let Some(center) = self.pending.pop() {
                    let r = center.p.distance(mouse);
                    if r > 1e-6 {
                        let cid = Self::point_for(sk, center);
                        if center.snap.is_none() && self.inferring() {
                            Self::infer_point(sk, &center, cid);
                        }
                        sk.add_circle(cid, r);
                        modified = true;
                    }
                } else {
                    self.pending.push(c);
                }
            }
            Tool::Arc => {
                let c = self.snap(sk, mouse, radius);
                if self.pending.len() == 2 {
                    let (center, start) = (self.pending[0], self.pending[1]);
                    let r = center.p.distance(start.p);
                    let end = match c.snap {
                        Some(id) if (sk.pos(id).distance(center.p) - r).abs() < radius => c,
                        _ => Click::at(center.p + (mouse - center.p).normalized() * r),
                    };
                    let first_new = sk.points.len();
                    if sk.add_centerpoint_arc(center.p, start.p, end.p, self.swept).is_ok() {
                        let ren = self.infer(sk, &[center, start, end], first_new);
                        remap_driven(sf, &ren.constraints);
                        modified = true;
                    }
                    self.pending.clear();
                } else {
                    if self.pending.len() == 1 {
                        self.swept = 0.0;
                        self.swept_last = Some((c.p - self.pending[0].p).angle());
                    }
                    self.pending.push(c);
                }
            }
            Tool::Dimension => {
                let hit = self.pick(sk, cam, rect, screen, mouse, radius);
                if let Some(ci) = self.dimension_click(sk, hit, mouse) {
                    self.selection = vec![SkSel::Constraint(ci)];
                    if dimension::over_defines(sk, ci) {
                        // Hold it as a reference dimension until the user decides.
                        sk.set_reference(ci, true);
                        self.over_define = Some(ci);
                        self.dim_edit = None;
                    } else {
                        self.begin_dim_edit(sf, ci);
                    }
                    modified = true;
                } else if let (Some(SkSel::Constraint(ci)), true) = (hit, self.picks.is_empty()) {
                    // Clicking an existing dimension's value edits it.
                    self.selection = vec![SkSel::Constraint(ci)];
                    self.begin_dim_edit(sf, ci);
                }
            }
            _ => modified = self.edit_click(sf, mouse, radius),
        }
        modified
    }

    /// A click with one of the editing tools that work by picking: fillet, chamfer, trim,
    /// extend and split.
    fn edit_click(&mut self, sf: &mut SketchFeature, mouse: Vec2, radius: f64) -> bool {
        let sk = &mut sf.sketch;
        let point = sk.pick_point(mouse, radius).filter(|&p| p != ORIGIN);
        let entity = sk.pick_entity(mouse, radius);
        let p = self.params.clone();
        let result: Result<Renumbering, SketchError> = match self.tool {
            Tool::Fillet => match (point, entity, self.entity_picks.first().copied()) {
                (Some(x), _, _) => sk.fillet_corners(&[x], p.fillet, self.first_fillet()).map(|r| {
                    self.fillet_first = self.fillet_first.or(r.entities.first().copied());
                    r.renumbering
                }),
                (None, Some(e), Some((e1, p1))) if e != e1 => {
                    self.entity_picks.clear();
                    sk.fillet_entities(e1, p1, e, mouse, p.fillet, self.first_fillet()).map(|r| {
                        self.fillet_first = self.fillet_first.or(r.entities.first().copied());
                        r.renumbering
                    })
                }
                (None, Some(e), _) => {
                    self.entity_picks = vec![(e, mouse)];
                    return false;
                }
                _ => return false,
            },
            Tool::Chamfer => match (point, entity, self.entity_picks.first().copied()) {
                (Some(x), _, _) => {
                    sk.chamfer_corner(x, None, p.chamfer_kind(), p.chamfer_keep).map(|r| r.renumbering)
                }
                (None, Some(e), Some((e1, p1))) if e != e1 => {
                    self.entity_picks.clear();
                    let shared = shared_end(sk, e1, e);
                    match shared {
                        Some(x) => sk
                            .chamfer_corner(x, Some(e1), p.chamfer_kind(), p.chamfer_keep)
                            .map(|r| r.renumbering),
                        None => {
                            let backup = sk.clone();
                            let r = sk.trim_corner(e1, p1, e, mouse).and_then(|(x, ren)| {
                                let first = ren.entity(e1);
                                let c = sk.chamfer_corner(x, first, p.chamfer_kind(), p.chamfer_keep)?;
                                Ok(ren.then(c.renumbering))
                            });
                            if r.is_err() {
                                *sk = backup;
                            }
                            r
                        }
                    }
                }
                (None, Some(e), _) => {
                    self.entity_picks = vec![(e, mouse)];
                    return false;
                }
                _ => return false,
            },
            Tool::Trim => {
                let Some(e) = entity else { return false };
                match p.trim_mode {
                    TrimMode::Power => sk.trim_at(e, mouse, p.trim).map(|r| r.renumbering),
                    TrimMode::Corner => match self.entity_picks.first().copied() {
                        Some((e1, p1)) if e1 != e => {
                            self.entity_picks.clear();
                            sk.trim_corner(e1, p1, e, mouse).map(|(_, r)| r)
                        }
                        _ => {
                            self.entity_picks = vec![(e, mouse)];
                            return false;
                        }
                    },
                    TrimMode::Inside | TrimMode::Outside => {
                        if self.entity_picks.len() < 2 {
                            if !self.entity_picks.iter().any(|(x, _)| *x == e) {
                                self.entity_picks.push((e, mouse));
                            }
                            return false;
                        }
                        let bounds = [self.entity_picks[0].0, self.entity_picks[1].0];
                        let r = sk
                            .trim_away(bounds, &[e], p.trim_mode == TrimMode::Inside)
                            .map(|r| r.renumbering);
                        if let Ok(ren) = &r {
                            // The boundaries stay picked for the next curve.
                            self.entity_picks.retain_mut(|(x, _)| match ren.entity(*x) {
                                Some(n) => {
                                    *x = n;
                                    true
                                }
                                None => false,
                            });
                        }
                        r
                    }
                }
            }
            Tool::Extend => {
                let Some(e) = entity else { return false };
                sk.extend(e, mouse).map(|(_, r)| r)
            }
            Tool::Split => {
                let Some(e) = entity else { return false };
                if matches!(sk.entities[e].geom, Geom::Circle { .. }) {
                    match self.entity_picks.first().copied() {
                        Some((e1, p1)) if e1 == e => {
                            self.entity_picks.clear();
                            sk.split_circle(e, p1, mouse).map(|_| Renumbering::identity(sk))
                        }
                        _ => {
                            self.entity_picks = vec![(e, mouse)];
                            return false;
                        }
                    }
                } else {
                    sk.split_at(e, mouse).map(|_| Renumbering::identity(sk))
                }
            }
            _ => return false,
        };
        match result {
            Ok(ren) => {
                if !ren.is_identity() {
                    self.selection.clear();
                    self.dim_edit = None;
                    self.fillet_first = self.fillet_first.and_then(|f| ren.entity(f));
                }
                remap_driven(sf, &ren.constraints);
                self.notice = None;
                true
            }
            Err(e) => {
                self.entity_picks.clear();
                self.notice = Some(format!("{}: {e}.", self.tool.label()));
                false
            }
        }
    }

    fn first_fillet(&self) -> Option<EntityId> {
        if self.params.fillet.dimension_each {
            None
        } else {
            self.fillet_first
        }
    }

    /// Applies a selection tool (offset, mirror, patterns, move, copy, rotate, scale) to
    /// the selected entities with the panel's options. Returns a message for the status
    /// line, or why it could not.
    pub fn apply(&mut self, sf: &mut SketchFeature) -> Result<String, String> {
        let ents = self.selected_entities();
        if ents.is_empty() {
            return Err("Select the entities first.".into());
        }
        let p = self.params.clone();
        let sk = &mut sf.sketch;
        let xopts =
            |copy: bool| TransformOptions { copy, copies: p.copies.max(1), keep_relations: p.keep_relations };
        let r: Result<(usize, Renumbering), SketchError> = match self.tool {
            Tool::Offset => {
                sk.offset_entities(&ents, p.offset).map(|r| (r.entities.len(), Renumbering::identity(sk)))
            }
            Tool::Mirror => {
                let axis = ents.iter().rev().copied().find(|&e| sk.entities[e].is_line());
                match axis {
                    Some(a) => {
                        let rest: Vec<EntityId> = ents.iter().copied().filter(|&e| e != a).collect();
                        sk.mirror_entities(&rest, a, p.mirror_copy).map(|(m, r)| (m.len(), r))
                    }
                    None => Err(SketchError::Unsupported("select a line to mirror about last")),
                }
            }
            Tool::LinearPattern => {
                sk.linear_pattern(&ents, &p.linear).map(|r| (r.instances.len(), Renumbering::identity(sk)))
            }
            Tool::CircularPattern => sk
                .circular_pattern(&ents, &p.circular)
                .map(|r| (r.instances.len(), Renumbering::identity(sk))),
            Tool::Move => sk
                .transform_entities(&ents, Transform2::Translate(p.delta), xopts(false))
                .map(|(e, r)| (e.len(), r)),
            Tool::Copy => sk
                .transform_entities(&ents, Transform2::Translate(p.delta), xopts(true))
                .map(|(e, r)| (e.len(), r)),
            Tool::Rotate => sk
                .transform_entities(
                    &ents,
                    Transform2::Rotate { center: p.center, degrees: p.degrees },
                    xopts(p.copy),
                )
                .map(|(e, r)| (e.len(), r)),
            Tool::Scale => sk
                .transform_entities(
                    &ents,
                    Transform2::Scale { about: p.center, factor: p.factor },
                    xopts(p.copy),
                )
                .map(|(e, r)| (e.len(), r)),
            _ => return Err("Pick a tool that works on the selection.".into()),
        };
        match r {
            Ok((n, ren)) => {
                remap_driven(sf, &ren.constraints);
                self.selection.clear();
                self.dim_edit = None;
                self.status = sf.sketch.solve();
                Ok(format!("{}: {n} entities.", self.tool.label()))
            }
            Err(e) => Err(format!("{}: {e}.", self.tool.label())),
        }
    }

    /// The Construction Geometry command on the selected entities.
    pub fn toggle_construction(&mut self, sf: &mut SketchFeature) -> bool {
        let ents = self.selected_entities();
        !ents.is_empty() && sf.sketch.toggle_construction(&ents).is_ok()
    }

    /// The Dimension tool (see `docs/re/sketch-dimension.md`). Clicking a point or curve
    /// adds it to the picks; clicking empty space, or a curve already picked, places the
    /// dimension the picks make there. Returns the new dimension's index.
    fn dimension_click(&mut self, sk: &mut Sketch, hit: Option<SkSel>, at: Vec2) -> Option<usize> {
        let pick = match hit {
            Some(SkSel::Point(p)) => Some(Pick::Point(p)),
            Some(SkSel::Entity(e)) => Some(Pick::Entity(e)),
            _ => None,
        };
        match pick {
            Some(pk) if !self.picks.contains(&pk) => {
                self.picks.push(pk);
                // Start again from this pick when it cannot join the others.
                let lone_point = matches!(self.picks.as_slice(), [Pick::Point(_)]);
                if self.picks.len() > dimension::MAX_PICKS
                    || (!lone_point && dimension::infer(sk, &self.picks, at).is_none())
                {
                    self.picks = vec![pk];
                }
                None
            }
            _ => {
                let c = dimension::infer(sk, &self.picks, at)?;
                self.picks.clear();
                let ci = sk.add_constraint(c);
                if let Some(a) = dimension::anchor(sk, &c) {
                    sk.annotation_mut(ci).offset = at - a;
                }
                Some(ci)
            }
        }
    }

    /// The user's answer for a dimension that over-defines the sketch: keep it as a
    /// reference dimension, keep it driving anyway, or take it back out.
    pub fn resolve_over_define(&mut self, sf: &mut SketchFeature, choice: OverDefine) {
        let Some(ci) = self.over_define.take() else { return };
        match choice {
            OverDefine::Reference => {}
            OverDefine::Driving => sf.sketch.set_reference(ci, false),
            OverDefine::Cancel => {
                sf.delete_constraint(ci);
                self.selection.clear();
            }
        }
        self.status = sf.sketch.solve();
    }

    pub fn begin_dim_edit(&mut self, sf: &SketchFeature, ci: usize) {
        if self.over_define.is_some() {
            return;
        }
        if let Some(v) = sf.sketch.constraints.get(ci).and_then(|c| c.value()) {
            let text = match sf.driven_expr(ci) {
                Some(e) => format!("={e}"),
                None => format_num(display_value(&sf.sketch.constraints[ci], v)),
            };
            self.dim_edit = Some((ci, text));
        }
    }

    pub fn delete_selection(&mut self, sf: &mut SketchFeature) {
        let mut cons: Vec<usize> = self
            .selection
            .iter()
            .filter_map(|s| match s {
                SkSel::Constraint(c) => Some(*c),
                _ => None,
            })
            .collect();
        cons.sort_unstable();
        // Never delete the origin's own fix.
        for c in cons.into_iter().rev() {
            if !matches!(sf.sketch.constraints.get(c), Some(Constraint::Fix { p: ORIGIN, .. }) | None) {
                sf.delete_constraint(c);
            }
        }
        let mut ents: Vec<EntityId> = self
            .selection
            .iter()
            .filter_map(|s| match s {
                SkSel::Entity(e) => Some(*e),
                _ => None,
            })
            .collect();
        let mut points: Vec<PointId> = self
            .selection
            .iter()
            .filter_map(|s| match s {
                SkSel::Point(p) if *p != ORIGIN => Some(*p),
                _ => None,
            })
            .collect();
        // Deleting a split point joins its two pieces again (sketch-tools.md, "Split
        // Entities"). One at a time, since each join renumbers the sketch.
        if ents.is_empty() && points.len() == 1 && sf.sketch.can_join_at(points[0]) {
            if let Ok((_, ren)) = sf.sketch.join_at(points[0]) {
                remap_driven(sf, &ren.constraints);
            }
            points.clear();
        }
        // Deleting a point deletes the curves that use it; a lone point goes on its own.
        let mut lone = Vec::new();
        for &p in &points {
            let users = sf.sketch.entities_using(p);
            if users.is_empty() {
                lone.push(p);
            }
            ents.extend(users);
        }
        if !ents.is_empty() {
            sf.delete_entities(&ents);
        }
        if !lone.is_empty() && ents.is_empty() {
            let ren = sf.sketch.remove_points(&lone);
            remap_driven(sf, &ren.constraints);
        }
        self.selection.clear();
        self.dim_edit = None;
        self.over_define = None;
        self.status = sf.sketch.solve();
    }

    /// Relations that can be added for the current selection (sketch-tools.md, "Every 2D
    /// relation"). Selections of more than two lines, arcs or points add one pairwise
    /// relation between the first and each other one, as a group.
    pub fn available_relations(&self, sk: &Sketch) -> Vec<(&'static str, Vec<Constraint>)> {
        let mut pts = Vec::new();
        let mut lines = Vec::new();
        let mut rounds = Vec::new();
        for s in &self.selection {
            match *s {
                SkSel::Point(p) if p < sk.points.len() => pts.push(p),
                SkSel::Entity(e) if e < sk.entities.len() && sk.entities[e].is_line() => lines.push(e),
                SkSel::Entity(e) if e < sk.entities.len() => rounds.push(e),
                _ => {}
            }
        }
        let pairwise = |ids: &[usize], f: &dyn Fn(usize, usize) -> Constraint| -> Vec<Constraint> {
            ids.iter().skip(1).map(|&b| f(ids[0], b)).collect()
        };
        let center = |e: EntityId| match sk.entities[e].geom {
            Geom::Circle { c, .. } | Geom::Arc { c, .. } => c,
            Geom::Line { a, .. } => a,
        };
        let mut out: Vec<(&'static str, Vec<Constraint>)> = Vec::new();
        match (pts.len(), lines.len(), rounds.len()) {
            (0, 1, 0) => {
                let l = lines[0];
                out.push(("Horizontal", vec![Constraint::Horizontal { line: l }]));
                out.push(("Vertical", vec![Constraint::Vertical { line: l }]));
                if let Geom::Line { a, b } = sk.entities[l].geom {
                    let (pa, pb) = (sk.pos(a), sk.pos(b));
                    let degrees = (pb - pa).angle().to_degrees();
                    out.push(("Fix", vec![Constraint::FixLine { line: l, x: pa.x, y: pa.y, degrees }]));
                }
            }
            (0, n, 0) if n >= 2 => {
                out.push(("Horizontal", lines.iter().map(|&l| Constraint::Horizontal { line: l }).collect()));
                out.push(("Vertical", lines.iter().map(|&l| Constraint::Vertical { line: l }).collect()));
                out.push(("Collinear", pairwise(&lines, &|a, b| Constraint::Collinear { a, b })));
                out.push(("Parallel", pairwise(&lines, &|a, b| Constraint::Parallel { a, b })));
                if n == 2 {
                    out.push(("Perpendicular", vec![Constraint::Perpendicular { a: lines[0], b: lines[1] }]));
                }
                out.push(("Equal", pairwise(&lines, &|a, b| Constraint::Equal { a, b })));
                if n == 3 {
                    // Two lines symmetric about the third (the last picked).
                    if let Some(sym) = symmetric_lines(sk, lines[0], lines[1], lines[2]) {
                        out.push(("Symmetric", sym));
                    }
                }
            }
            (0, 0, 1) => {
                let e = rounds[0];
                let c = sk.pos(center(e));
                out.push(("Fix", vec![Constraint::FixRound { entity: e, x: c.x, y: c.y, r: sk.radius(e) }]));
            }
            (0, 0, n) if n >= 2 => {
                if n == 2 {
                    let (a, b) = (rounds[0], rounds[1]);
                    out.push((
                        "Tangent",
                        vec![Constraint::TangentCircles { a, b, inside: sk.circles_inside(a, b) }],
                    ));
                }
                out.push(("Equal", pairwise(&rounds, &|a, b| Constraint::Equal { a, b })));
                out.push(("Concentric", pairwise(&rounds, &|a, b| Constraint::Concentric { a, b })));
                out.push(("Coradial", pairwise(&rounds, &|a, b| Constraint::Coradial { a, b })));
            }
            (0, 1, 1) => {
                out.push(("Tangent", vec![Constraint::Tangent { line: lines[0], entity: rounds[0] }]))
            }
            (0, 1, 2) => {
                // Two circles or arcs symmetric about the line.
                let (a, b, l) = (rounds[0], rounds[1], lines[0]);
                out.push((
                    "Symmetric",
                    vec![
                        Constraint::Symmetric { p: center(a), q: center(b), line: l },
                        Constraint::Equal { a, b },
                    ],
                ));
            }
            (1, 0, 0) => {
                let p = pts[0];
                let q = sk.pos(p);
                out.push(("Fix", vec![Constraint::Fix { p, x: q.x, y: q.y }]));
            }
            (n, 0, 0) if n >= 2 => {
                out.push(("Coincident", pairwise(&pts, &|p, q| Constraint::Coincident { p, q })));
                out.push(("Horizontal", pairwise(&pts, &|p, q| Constraint::HorizontalPoints { p, q })));
                out.push(("Vertical", pairwise(&pts, &|p, q| Constraint::VerticalPoints { p, q })));
            }
            (1, 1, 0) => {
                let (p, line) = (pts[0], lines[0]);
                out.push(("Coincident", vec![Constraint::PointOnLine { p, line }]));
                out.push(("Midpoint", vec![Constraint::Midpoint { p, line }]));
            }
            (1, 2, 0) => {
                out.push((
                    "Intersection",
                    vec![Constraint::Intersection { p: pts[0], a: lines[0], b: lines[1] }],
                ));
            }
            (2, 1, 0) => {
                out.push(("Symmetric", vec![Constraint::Symmetric { p: pts[0], q: pts[1], line: lines[0] }]));
            }
            (1, 0, 1) => {
                let (p, e) = (pts[0], rounds[0]);
                out.push(("Coincident", vec![Constraint::PointOnCircle { p, entity: e }]));
                // A point and an arc: Concentric puts the point on the centre.
                out.push(("Concentric", vec![Constraint::Coincident { p, q: center(e) }]));
            }
            _ => {}
        }
        out
    }

    pub fn selected_entities(&self) -> Vec<EntityId> {
        self.selection
            .iter()
            .filter_map(|s| match s {
                SkSel::Entity(e) => Some(*e),
                _ => None,
            })
            .collect()
    }

    /// The options of the active tool, for the sketch panel. Returns true if the sketch
    /// changed (an Apply), and puts a message for the status line in `status`.
    pub fn options_ui(&mut self, ui: &mut egui::Ui, sf: &mut SketchFeature, status: &mut String) -> bool {
        let mut changed = false;
        let prompt = self.tool.prompt(self.pick_count());
        if !prompt.is_empty() {
            ui.weak(prompt);
        }
        if let Some(n) = &self.notice {
            ui.colored_label(Color32::from_rgb(200, 90, 20), n);
        }
        let num = |ui: &mut egui::Ui, label: &str, v: &mut f64, speed: f64| {
            ui.horizontal(|ui| {
                ui.label(label);
                ui.add(egui::DragValue::new(v).speed(speed).max_decimals(3));
            });
        };
        let p = &mut self.params;
        match self.tool {
            Tool::Line | Tool::Centerline | Tool::Rectangle | Tool::Point | Tool::Circle | Tool::Arc => {
                ui.checkbox(&mut p.inference, "Automatic relations (hold Ctrl to skip)");
            }
            Tool::CenterRectangle | Tool::Rectangle3Center | Tool::Rectangle3Corner | Tool::Parallelogram => {
                if matches!(self.tool, Tool::CenterRectangle | Tool::Rectangle3Center) {
                    ui.horizontal(|ui| {
                        ui.label("Construction lines");
                        ui.radio_value(&mut p.rect_construction, RectConstruction::Corners, "From corners");
                        ui.radio_value(
                            &mut p.rect_construction,
                            RectConstruction::Midpoints,
                            "From midpoints",
                        );
                    });
                }
                ui.weak("Hold Shift for equal sides.");
                ui.checkbox(&mut p.inference, "Automatic relations (hold Ctrl to skip)");
            }
            Tool::StraightSlot | Tool::CenterSlot | Tool::ArcSlot3 | Tool::CenterArcSlot => {
                if matches!(self.tool, Tool::StraightSlot | Tool::CenterSlot) {
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut p.slot.length, SlotLength::CenterToCenter, "Center to center");
                        ui.radio_value(&mut p.slot.length, SlotLength::Overall, "Overall length");
                    });
                }
                ui.checkbox(&mut p.slot.dimensions, "Add dimensions");
            }
            Tool::Polygon => {
                ui.horizontal(|ui| {
                    ui.label("Sides");
                    ui.add(egui::DragValue::new(&mut p.polygon_sides).range(POLYGON_SIDES));
                });
                ui.horizontal(|ui| {
                    ui.radio_value(&mut p.polygon_circle, PolygonCircle::Inscribed, "Inscribed circle");
                    ui.radio_value(
                        &mut p.polygon_circle,
                        PolygonCircle::Circumscribed,
                        "Circumscribed circle",
                    );
                });
            }
            Tool::Fillet => {
                num(ui, "Radius", &mut p.fillet.radius, 0.1);
                ui.checkbox(&mut p.fillet.keep_constrained_corners, "Keep constrained corners");
                ui.checkbox(&mut p.fillet.dimension_each, "Dimension each fillet");
            }
            Tool::Chamfer => {
                ui.horizontal(|ui| {
                    ui.radio_value(&mut p.chamfer, ChamferType::AngleDistance, "Angle-distance");
                    ui.radio_value(&mut p.chamfer, ChamferType::TwoDistances, "Distance-distance");
                    ui.radio_value(&mut p.chamfer, ChamferType::EqualDistance, "Equal distance");
                });
                num(ui, "Distance 1", &mut p.chamfer_d1, 0.1);
                match p.chamfer {
                    ChamferType::TwoDistances => num(ui, "Distance 2", &mut p.chamfer_d2, 0.1),
                    ChamferType::AngleDistance => num(ui, "Angle", &mut p.chamfer_angle, 0.5),
                    ChamferType::EqualDistance => {}
                }
                ui.checkbox(&mut p.chamfer_keep, "Keep constrained corners");
            }
            Tool::Trim => {
                ui.horizontal_wrapped(|ui| {
                    ui.radio_value(&mut p.trim_mode, TrimMode::Power, "Power trim");
                    ui.radio_value(&mut p.trim_mode, TrimMode::Corner, "Corner");
                    ui.radio_value(&mut p.trim_mode, TrimMode::Inside, "Trim away inside");
                    ui.radio_value(&mut p.trim_mode, TrimMode::Outside, "Trim away outside");
                });
                if matches!(p.trim_mode, TrimMode::Inside | TrimMode::Outside) {
                    ui.weak("Click two boundaries, then the curves to trim.");
                }
                ui.checkbox(
                    &mut p.trim.keep_as_construction,
                    "Keep trimmed entities as construction geometry",
                );
                ui.checkbox(&mut p.trim.ignore_construction, "Ignore trimming of construction geometry");
            }
            Tool::Offset => {
                num(ui, "Distance", &mut p.offset.distance, 0.1);
                ui.checkbox(&mut p.offset.dimensions, "Add dimensions");
                ui.checkbox(&mut p.offset.reverse, "Reverse");
                ui.checkbox(&mut p.offset.chain, "Select chain");
                ui.checkbox(&mut p.offset.bidirectional, "Bi-directional");
                ui.horizontal(|ui| {
                    ui.label("Cap ends");
                    ui.radio_value(&mut p.offset.caps, CapEnds::None, "None");
                    ui.radio_value(&mut p.offset.caps, CapEnds::Arcs, "Arcs");
                    ui.radio_value(&mut p.offset.caps, CapEnds::Lines, "Lines");
                });
                ui.checkbox(&mut p.offset.base_construction, "Base geometry as construction");
                ui.checkbox(&mut p.offset.offset_construction, "Offset geometry as construction");
            }
            Tool::Mirror => {
                ui.checkbox(&mut p.mirror_copy, "Copy");
            }
            Tool::LinearPattern => {
                let l = &mut p.linear;
                for (k, name) in ["Direction 1", "Direction 2"].iter().enumerate() {
                    ui.label(egui::RichText::new(*name).strong());
                    ui.horizontal(|ui| {
                        ui.label("Instances");
                        ui.add(egui::DragValue::new(&mut l.count[k]).range(1..=1000));
                        ui.label("Spacing");
                        ui.add(egui::DragValue::new(&mut l.spacing[k]).speed(0.1).max_decimals(3));
                    });
                    // Direction 2's angle is shown from the y axis, as users type it
                    // (sketch-tools.md, "Linear", Decision); it is stored from x.
                    let mut shown = if k == 0 { l.degrees[0] } else { l.degrees[1] - 90.0 };
                    ui.horizontal(|ui| {
                        ui.label(if k == 0 { "Angle" } else { "Angle from Y" });
                        if ui.add(egui::DragValue::new(&mut shown).speed(0.5).max_decimals(3)).changed() {
                            l.degrees[k] = if k == 0 { shown } else { shown + 90.0 };
                        }
                    });
                }
                ui.checkbox(&mut l.dimension_spacing, "Dimension spacing");
                ui.checkbox(&mut l.fix_direction, "Fix X-axis direction");
                ui.checkbox(&mut l.dimension_angle, "Dimension angle between axes");
            }
            Tool::CircularPattern => {
                let c = &mut p.circular;
                num(ui, "Center X", &mut c.center.x, 0.1);
                num(ui, "Center Y", &mut c.center.y, 0.1);
                ui.horizontal(|ui| {
                    ui.label("Instances");
                    ui.add(egui::DragValue::new(&mut c.count).range(2..=1000));
                });
                num(ui, "Total angle", &mut c.total_degrees, 1.0);
                ui.checkbox(&mut c.reverse, "Reverse direction");
                ui.checkbox(&mut c.dimension_radius, "Dimension radius");
                ui.checkbox(&mut c.dimension_angle, "Dimension angular spacing");
            }
            Tool::Move | Tool::Copy => {
                num(ui, "Delta X", &mut p.delta.x, 0.1);
                num(ui, "Delta Y", &mut p.delta.y, 0.1);
                if self.tool == Tool::Copy {
                    ui.horizontal(|ui| {
                        ui.label("Copies");
                        ui.add(egui::DragValue::new(&mut p.copies).range(1..=100));
                    });
                }
                ui.checkbox(&mut p.keep_relations, "Keep relations");
            }
            Tool::Rotate | Tool::Scale => {
                num(ui, "Center X", &mut p.center.x, 0.1);
                num(ui, "Center Y", &mut p.center.y, 0.1);
                if self.tool == Tool::Rotate {
                    num(ui, "Angle", &mut p.degrees, 0.5);
                } else {
                    num(ui, "Scale factor", &mut p.factor, 0.05);
                }
                ui.checkbox(&mut p.copy, "Copy");
                if p.copy {
                    ui.horizontal(|ui| {
                        ui.label("Copies");
                        ui.add(egui::DragValue::new(&mut p.copies).range(1..=100));
                    });
                }
                ui.checkbox(&mut p.keep_relations, "Keep relations");
            }
            _ => {}
        }
        if self.tool.uses_selection() {
            let n = self.selected_entities().len();
            ui.horizontal(|ui| {
                ui.label(format!("{n} selected"));
                if ui.add_enabled(n > 0, egui::Button::new("Apply")).clicked() {
                    match self.apply(sf) {
                        Ok(m) => {
                            *status = m;
                            changed = true;
                        }
                        Err(e) => self.notice = Some(e),
                    }
                }
            });
        }
        changed
    }

    /// Draws the sketch being edited, with tool previews.
    pub fn draw(&self, sf: &SketchFeature, painter: &Painter, cam: &Camera, rect: Rect) {
        let sel = |s: SkSel| {
            self.selection.contains(&s)
                || match s {
                    SkSel::Point(p) => self.picks.contains(&Pick::Point(p)),
                    SkSel::Entity(e) => {
                        self.picks.contains(&Pick::Entity(e))
                            || self.entity_picks.iter().any(|(x, _)| *x == e)
                    }
                    SkSel::Constraint(_) => false,
                }
        };
        let conflicted: Vec<usize> = self
            .status
            .unsatisfied
            .iter()
            .flat_map(|&c| sf.sketch.constraints.get(c).map(|c| c.entities()).unwrap_or_default())
            .collect();
        let style = |e: EntityId| -> Color32 {
            if sel(SkSel::Entity(e)) {
                Color32::from_rgb(255, 140, 0)
            } else if self.hover == Some(SkSel::Entity(e)) {
                Color32::from_rgb(240, 170, 60)
            } else if conflicted.contains(&e) {
                Color32::from_rgb(220, 30, 30)
            } else if self.status.converged && self.status.dof == 0 {
                Color32::from_rgb(20, 20, 20)
            } else {
                Color32::from_rgb(20, 60, 230)
            }
        };
        draw_sketch_geometry(&sf.sketch, &self.frame, painter, cam, rect, &style, 1.8);

        // Points. A point no curve ends at but which sits on two curves is a virtual
        // sharp, drawn as a small cross.
        for (i, p) in sf.sketch.points.iter().enumerate() {
            let s = cam.project(self.frame.to_world(p.pos()), rect);
            let c = if sel(SkSel::Point(i)) {
                Color32::from_rgb(255, 140, 0)
            } else if self.hover == Some(SkSel::Point(i)) {
                Color32::from_rgb(240, 170, 60)
            } else if i == ORIGIN {
                Color32::from_rgb(200, 30, 30)
            } else {
                Color32::from_rgb(30, 30, 30)
            };
            if i != ORIGIN && is_virtual_sharp(&sf.sketch, i) {
                let st = Stroke::new(1.3_f32, c);
                painter.line_segment([s + egui::vec2(-4.0, -4.0), s + egui::vec2(4.0, 4.0)], st);
                painter.line_segment([s + egui::vec2(-4.0, 4.0), s + egui::vec2(4.0, -4.0)], st);
            } else {
                painter.circle_filled(s, if i == ORIGIN { 3.5 } else { 2.5 }, c);
            }
        }
        draw_origin(&self.frame, painter, cam, rect);

        // Dimensions.
        let sk = &sf.sketch;
        for (ci, c) in sk.constraints.iter().enumerate() {
            let (Some(at), Some(v)) = (dimension::label_position(sk, ci), c.value()) else {
                continue;
            };
            let reference = sk.is_reference(ci);
            let mut text = dimension_text(c, v);
            if reference {
                text = format!("({text})");
            }
            if sf.driven_expr(ci).is_some() {
                text = format!("Σ {text}");
            }
            let color = if sel(SkSel::Constraint(ci)) {
                Color32::from_rgb(255, 140, 0)
            } else if self.status.unsatisfied.contains(&ci) {
                Color32::from_rgb(220, 30, 30)
            } else if self.hover == Some(SkSel::Constraint(ci)) {
                Color32::from_rgb(240, 170, 60)
            } else if reference {
                Color32::from_rgb(120, 120, 120)
            } else {
                Color32::from_rgb(10, 30, 140)
            };
            draw_dimension(sk, &self.frame, painter, cam, rect, c, at, &text, color);
        }

        // The dimension the picks make, following the cursor.
        if let (Tool::Dimension, Some(m)) = (self.tool, self.mouse) {
            if let Some(c) = dimension::infer(sk, &self.picks, m) {
                let text = c.value().map(|v| dimension_text(&c, v)).unwrap_or_default();
                draw_dimension(
                    sk,
                    &self.frame,
                    painter,
                    cam,
                    rect,
                    &c,
                    m,
                    &text,
                    Color32::from_rgb(80, 120, 255),
                );
            }
        }

        // Tool preview.
        let Some(m) = self.mouse else { return };
        let to = |p: Vec2| cam.project(self.frame.to_world(p), rect);
        let preview_color = Color32::from_rgb(80, 120, 255);
        let preview = Stroke::new(1.2_f32, preview_color);
        let radius = PICK_PX / cam.pixels_per_unit(rect);
        let new_entities = |tmp: &Sketch, first: usize| {
            for e in first..tmp.entities.len() {
                let pts = entity_polyline(tmp, e, &to);
                if tmp.entities[e].construction {
                    painter.extend(Shape::dashed_line(&pts, preview, 6.0, 4.0));
                } else {
                    painter.add(Shape::line(pts, preview));
                }
            }
        };
        if let Some(n) = self.tool.clicks_needed() {
            let snapped = self.snap(sk, m, radius);
            let mut pts: Vec<Vec2> = self.pending.iter().map(|c| c.p).collect();
            pts.push(snapped.p);
            let mut tmp = sk.clone();
            let first = tmp.entities.len();
            let partial = match (self.tool, pts.len()) {
                (_, k) if k == n => self.build(&mut tmp, &pts).map(|_| ()),
                (Tool::PerimeterCircle, 2) => tmp.add_circle_2pt(pts[0], pts[1]).map(|_| ()),
                (Tool::ArcSlot3, 3) => tmp.add_3pt_arc(pts[0], pts[1], pts[2]).map(|_| ()),
                (Tool::CenterArcSlot, 3) => {
                    tmp.add_centerpoint_arc(pts[0], pts[1], pts[2], self.swept).map(|_| ())
                }
                _ => Err(SketchError::Degenerate),
            };
            if partial.is_ok() {
                new_entities(&tmp, first);
            } else if pts.len() >= 2 {
                let line: Vec<Pos2> = pts.iter().map(|&p| to(p)).collect();
                painter.add(Shape::line(line, preview));
            }
        }
        // The fillet or chamfer a click on the corner under the pointer would make.
        if let (Tool::Fillet | Tool::Chamfer, Some(SkSel::Point(p))) = (self.tool, self.hover) {
            let mut tmp = sk.clone();
            let first = tmp.entities.len();
            let made = if self.tool == Tool::Fillet {
                tmp.fillet_corners(&[p], self.params.fillet, None).is_ok()
            } else {
                tmp.chamfer_corner(p, None, self.params.chamfer_kind(), false).is_ok()
            };
            if made {
                new_entities(&tmp, first);
            }
        }
        match (self.tool, self.pending.as_slice()) {
            (Tool::Line | Tool::Centerline, [a]) => {
                painter.line_segment([to(a.p), to(m)], preview);
                let len = a.p.distance(m);
                painter.text(
                    to(m) + egui::vec2(12.0, -12.0),
                    Align2::LEFT_BOTTOM,
                    format!("L {}", format_num(len)),
                    FontId::proportional(12.0),
                    Color32::DARK_GRAY,
                );
            }
            (Tool::Rectangle, [a]) => {
                let pts = [a.p, v2(m.x, a.p.y), m, v2(a.p.x, m.y)];
                for i in 0..4 {
                    painter.line_segment([to(pts[i]), to(pts[(i + 1) % 4])], preview);
                }
                painter.text(
                    to(m) + egui::vec2(12.0, -12.0),
                    Align2::LEFT_BOTTOM,
                    format!("{} x {}", format_num((m.x - a.p.x).abs()), format_num((m.y - a.p.y).abs())),
                    FontId::proportional(12.0),
                    Color32::DARK_GRAY,
                );
            }
            (Tool::Circle, [c]) => {
                let r = c.p.distance(m);
                let pts: Vec<Pos2> =
                    (0..=64).map(|i| to(c.p + Vec2::from_angle(i as f64 / 64.0 * TAU) * r)).collect();
                painter.add(Shape::line(pts, preview));
                painter.text(
                    to(m) + egui::vec2(12.0, -12.0),
                    Align2::LEFT_BOTTOM,
                    format!("R {}", format_num(r)),
                    FontId::proportional(12.0),
                    Color32::DARK_GRAY,
                );
            }
            (Tool::Arc | Tool::CenterArcSlot, [c]) => {
                painter.line_segment([to(c.p), to(m)], preview);
            }
            (Tool::Arc, [c, s]) => {
                // The arc runs the way the pointer went round the centre.
                let r = c.p.distance(s.p);
                let a0 = (s.p - c.p).angle();
                let ccw = ((m - c.p).angle() - a0).rem_euclid(TAU);
                let sweep = if self.swept >= 0.0 { ccw } else { ccw - TAU };
                let n = 48;
                let pts: Vec<Pos2> = (0..=n)
                    .map(|i| to(c.p + Vec2::from_angle(a0 + sweep * i as f64 / n as f64) * r))
                    .collect();
                painter.add(Shape::line(pts, preview));
            }
            _ => {}
        }
        // Snap indicator: a ring on a point, a square on a midpoint, a tick on a curve.
        if !matches!(self.tool, Tool::Select | Tool::Dimension) && !self.tool.uses_selection() {
            let orange = Stroke::new(1.5_f32, Color32::from_rgb(255, 140, 0));
            let c = self.snap(sk, m, radius);
            if let Some(p) = c.snap {
                painter.circle_stroke(to(sk.pos(p)), 6.0, orange);
            } else if c.mid.is_some() {
                painter.rect_stroke(
                    Rect::from_center_size(to(c.p), egui::vec2(9.0, 9.0)),
                    0.0,
                    orange,
                    egui::StrokeKind::Middle,
                );
            } else if c.on.is_some() {
                painter.circle_stroke(to(c.p), 3.5, orange);
            }
        }
    }
}

/// True for a point that no curve uses but that is held on two curves: the corner a
/// fillet or chamfer cut away (sketch-tools.md, "The corner point and its dimensions").
fn is_virtual_sharp(sk: &Sketch, p: PointId) -> bool {
    if !sk.entities_using(p).is_empty() {
        return false;
    }
    sk.constraints
        .iter()
        .filter(|c| matches!(c, Constraint::PointOnLine { p: q, .. } | Constraint::PointOnCircle { p: q, .. } if *q == p))
        .count()
        >= 2
}

/// The screen polyline of one entity.
fn entity_polyline(sk: &Sketch, e: EntityId, to: &dyn Fn(Vec2) -> Pos2) -> Vec<Pos2> {
    match sk.entities[e].geom {
        Geom::Line { a, b } => vec![to(sk.pos(a)), to(sk.pos(b))],
        Geom::Circle { c, r } => {
            (0..=72).map(|i| to(sk.pos(c) + Vec2::from_angle(i as f64 / 72.0 * TAU) * r)).collect()
        }
        Geom::Arc { c, a, b } => {
            let (cc, pa, pb) = (sk.pos(c), sk.pos(a), sk.pos(b));
            let r = pa.distance(cc);
            let a0 = (pa - cc).angle();
            let mut sweep = ((pb - cc).angle() - a0).rem_euclid(TAU);
            if sweep < 1e-9 {
                sweep = TAU;
            }
            let n = ((sweep / TAU) * 72.0).ceil().max(4.0) as usize;
            (0..=n).map(|i| to(cc + Vec2::from_angle(a0 + sweep * i as f64 / n as f64) * r)).collect()
        }
    }
}

/// Draws sketch curves with per-entity colors. Construction geometry is dashed.
pub fn draw_sketch_geometry(
    sk: &Sketch,
    frame: &Frame,
    painter: &Painter,
    cam: &Camera,
    rect: Rect,
    color: &dyn Fn(EntityId) -> Color32,
    width: f32,
) {
    let to = |p: Vec2| cam.project(frame.to_world(p), rect);
    for (ei, e) in sk.entities.iter().enumerate() {
        let pts = entity_polyline(sk, ei, &to);
        let stroke = Stroke::new(width, color(ei));
        if e.construction {
            painter.extend(Shape::dashed_line(&pts, stroke, 8.0, 5.0));
        } else {
            painter.add(Shape::line(pts, stroke));
        }
    }
}

pub fn draw_origin(frame: &Frame, painter: &Painter, cam: &Camera, rect: Rect) {
    let o = cam.project(frame.origin, rect);
    let len = 22.0 / cam.pixels_per_unit(rect);
    let x = cam.project(frame.to_world(v2(len, 0.0)), rect);
    let y = cam.project(frame.to_world(v2(0.0, len)), rect);
    let red = Stroke::new(1.5_f32, Color32::from_rgb(200, 30, 30));
    painter.line_segment([o, x], red);
    painter.line_segment([o, y], red);
}

fn line_dir(sk: &Sketch, e: EntityId) -> Vec2 {
    match sk.entities[e].geom {
        Geom::Line { a, b } => sk.pos(b) - sk.pos(a),
        _ => Vec2::ZERO,
    }
}

/// Adds horizontal/vertical relations to nearly axis-aligned new lines, like SOLIDWORKS'
/// automatic relations (sketch-entities.md, "Inferencing", Decision 3: within 3°).
fn auto_hv(sk: &mut Sketch, line: EntityId) {
    let d = line_dir(sk, line);
    let ang = d.y.atan2(d.x).abs();
    let tol = 3f64.to_radians();
    if ang < tol || (std::f64::consts::PI - ang) < tol {
        sk.add_constraint(Constraint::Horizontal { line });
    } else if (ang - std::f64::consts::FRAC_PI_2).abs() < tol {
        sk.add_constraint(Constraint::Vertical { line });
    }
}

/// A new line drawn from the end of an arc that leaves within 3° of the arc's direction
/// there gets `Tangent` (sketch-entities.md, "Inferencing", Decision 4).
fn auto_tangent(sk: &mut Sketch, line: EntityId, start: PointId) {
    let Geom::Line { a, b } = sk.entities[line].geom else { return };
    let away = if a == start { sk.pos(b) - sk.pos(a) } else { sk.pos(a) - sk.pos(b) };
    for e in sk.entities_ending_at(start) {
        if e == line || sk.entities[e].is_line() {
            continue;
        }
        if let Some(t) = sk.leaving_direction(e, start) {
            if t.dot(away.normalized()) > 3f64.to_radians().cos() {
                sk.add_constraint(Constraint::Tangent { line, entity: e });
                return;
            }
        }
    }
}

/// An end point two entities share.
fn shared_end(sk: &Sketch, e1: EntityId, e2: EntityId) -> Option<PointId> {
    let ends = |e: EntityId| match sk.entities.get(e)?.geom {
        Geom::Line { a, b } | Geom::Arc { a, b, .. } => Some([a, b]),
        Geom::Circle { .. } => None,
    };
    let (x, y) = (ends(e1)?, ends(e2)?);
    x.into_iter().find(|p| y.contains(p))
}

/// Symmetric relations that mirror line `a` onto line `b` across line `axis`, pairing each
/// end of `a` with the end of `b` nearer its mirror image.
fn symmetric_lines(sk: &Sketch, a: EntityId, b: EntityId, axis: EntityId) -> Option<Vec<Constraint>> {
    let (Geom::Line { a: a0, b: a1 }, Geom::Line { a: b0, b: b1 }, Geom::Line { a: x0, b: x1 }) =
        (sk.entities[a].geom, sk.entities[b].geom, sk.entities[axis].geom)
    else {
        return None;
    };
    let (pa, pb) = (sk.pos(x0), sk.pos(x1));
    let image = anvil_sketch::curve::reflect(sk.pos(a0), pa, pb);
    let (q0, q1) = if image.distance(sk.pos(b0)) <= image.distance(sk.pos(b1)) { (b0, b1) } else { (b1, b0) };
    Some(vec![
        Constraint::Symmetric { p: a0, q: q0, line: axis },
        Constraint::Symmetric { p: a1, q: q1, line: axis },
    ])
}

/// How the Dimension tool's over-define prompt was answered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OverDefine {
    Reference,
    Driving,
    Cancel,
}

/// A dimension's value as shown on screen, with its symbol.
pub fn dimension_text(c: &Constraint, v: f64) -> String {
    let text = format_num(display_value(c, v));
    match c {
        Constraint::Diameter { .. } | Constraint::AxisDiameter { .. } => format!("Ø{text}"),
        Constraint::Radius { .. } => format!("R{text}"),
        Constraint::Angle { .. } | Constraint::RayAngle { .. } => format!("{text}°"),
        _ => text,
    }
}

/// Draws one dimension: extension and dimension lines, arrows, and its value at `at`.
#[allow(clippy::too_many_arguments)]
fn draw_dimension(
    sk: &Sketch,
    frame: &Frame,
    painter: &Painter,
    cam: &Camera,
    rect: Rect,
    c: &Constraint,
    at: Vec2,
    text: &str,
    color: Color32,
) {
    let to = |p: Vec2| cam.project(frame.to_world(p), rect);
    let g = dimension::graphic(sk, c, at);
    let stroke = Stroke::new(1.0_f32, color);
    for (a, b) in &g.lines {
        painter.line_segment([to(*a), to(*b)], stroke);
    }
    for &(center, r, a0, sweep) in &g.arcs {
        let n = ((sweep / TAU) * 72.0).ceil().max(4.0) as usize;
        let pts: Vec<Pos2> =
            (0..=n).map(|i| to(center + Vec2::from_angle(a0 + sweep * i as f64 / n as f64) * r)).collect();
        painter.add(Shape::line(pts, stroke));
    }
    for &(tip, dir) in &g.arrows {
        let t = to(tip);
        let d = to(tip + dir) - t;
        let len = d.length();
        if len < 1e-6 {
            continue;
        }
        let d = d / len;
        let n = egui::vec2(-d.y, d.x);
        let back = t - d * 9.0;
        painter.add(Shape::convex_polygon(vec![t, back + n * 3.0, back - n * 3.0], color, Stroke::NONE));
    }
    let s = to(at);
    let r = painter.text(s, Align2::CENTER_CENTER, text, FontId::proportional(13.0), color);
    painter.rect_filled(r.expand(2.0), 2.0, Color32::from_rgba_unmultiplied(255, 255, 255, 200));
    painter.text(s, Align2::CENTER_CENTER, text, FontId::proportional(13.0), color);
}

/// HDistance and VDistance store a signed value; show its magnitude.
pub fn display_value(c: &Constraint, v: f64) -> f64 {
    match c {
        Constraint::HDistance { .. } | Constraint::VDistance { .. } | Constraint::Offset { .. } => v.abs(),
        _ => v,
    }
}

pub fn format_num(v: f64) -> String {
    let s = format!("{v:.3}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" {
        "0".into()
    } else {
        s.to_string()
    }
}
