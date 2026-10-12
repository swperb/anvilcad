//! Interactive sketch editing: tools, snapping, selection, dragging, dimensions and the
//! on-screen sketch overlay.

use crate::camera::Camera;
use anvil_kernel::{v2, Frame, Vec2};
use anvil_model::SketchFeature;
use anvil_sketch::{Constraint, EntityId, Geom, PointId, Sketch, SolveResult, ORIGIN};
use eframe::egui::{self, Align2, Color32, FontId, Painter, Pos2, Rect, Response, Shape, Stroke};
use std::f64::consts::TAU;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Select,
    Line,
    Centerline,
    Rectangle,
    Circle,
    Arc,
    Dimension,
}

impl Tool {
    pub fn label(&self) -> &'static str {
        match self {
            Tool::Select => "Select",
            Tool::Line => "Line",
            Tool::Centerline => "Centerline",
            Tool::Rectangle => "Rectangle",
            Tool::Circle => "Circle",
            Tool::Arc => "Centerpoint Arc",
            Tool::Dimension => "Smart Dimension",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkSel {
    Point(PointId),
    Entity(EntityId),
    Constraint(usize),
}

/// A click location, possibly snapped to an existing sketch point.
#[derive(Clone, Copy, Debug)]
struct Click {
    p: Vec2,
    snap: Option<PointId>,
}

pub struct SketchSession {
    pub feature: u32,
    pub frame: Frame,
    pub tool: Tool,
    pending: Vec<Click>,
    pending_pick: Option<SkSel>,
    pub selection: Vec<SkSel>,
    pub hover: Option<SkSel>,
    dragging: Option<SkSel>,
    pub status: SolveResult,
    /// Dimension currently being edited and its text.
    pub dim_edit: Option<(usize, String)>,
    mouse: Option<Vec2>,
}

/// What a frame of interaction did.
#[derive(Default)]
pub struct Outcome {
    pub modified: bool,
    pub dragging: bool,
}

const PICK_PX: f64 = 8.0;

impl SketchSession {
    pub fn new(feature: u32, frame: Frame) -> SketchSession {
        SketchSession {
            feature,
            frame,
            tool: Tool::Line,
            pending: Vec::new(),
            pending_pick: None,
            selection: Vec::new(),
            hover: None,
            dragging: None,
            status: SolveResult::default(),
            dim_edit: None,
            mouse: None,
        }
    }

    pub fn set_tool(&mut self, t: Tool) {
        self.tool = t;
        self.cancel_pending();
    }

    pub fn cancel_pending(&mut self) {
        self.pending.clear();
        self.pending_pick = None;
    }

    pub fn has_pending(&self) -> bool {
        !self.pending.is_empty() || self.pending_pick.is_some()
    }

    fn to_sketch(&self, cam: &Camera, rect: Rect, p: Pos2) -> Option<Vec2> {
        let (o, d) = cam.ray(p, rect);
        self.frame.intersect_ray(o, d)
    }

    fn to_screen(&self, cam: &Camera, rect: Rect, p: Vec2) -> Pos2 {
        cam.project(self.frame.to_world(p), rect)
    }

    fn snap(&self, sk: &Sketch, p: Vec2, radius: f64) -> Click {
        match sk.pick_point(p, radius) {
            Some(id) => Click { p: sk.pos(id), snap: Some(id) },
            None => Click { p, snap: None },
        }
    }

    fn point_for(sk: &mut Sketch, c: Click) -> PointId {
        c.snap.unwrap_or_else(|| sk.add_point(c.p))
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
        for (ci, c) in sk.constraints.iter().enumerate() {
            if let Some(anchor) = dimension_anchor(sk, c) {
                let s = self.to_screen(cam, rect, anchor);
                if (s.x - screen.x).abs() < 28.0 && (s.y - screen.y).abs() < 9.0 {
                    return Some(SkSel::Constraint(ci));
                }
            }
        }
        sk.pick_entity(p, radius).map(SkSel::Entity)
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

        let (esc, delete, shift) = ctx.input(|i| {
            (
                i.key_pressed(egui::Key::Escape),
                i.key_pressed(egui::Key::Delete),
                i.modifiers.shift || i.modifiers.ctrl,
            )
        });
        if esc {
            if self.has_pending() {
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
            self.cancel_pending();
        }

        let Some(mouse) = self.mouse else {
            return out;
        };
        let screen = hover_screen.or(resp.interact_pointer_pos()).unwrap_or_default();

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
                            if let Geom::Circle { c, .. } = sf.sketch.entities[e].geom {
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
                        SkSel::Constraint(_) => {}
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
                Tool::Line | Tool::Centerline => {
                    let c = self.snap(sk, mouse, radius);
                    if let Some(start) = self.pending.pop() {
                        let a = Self::point_for(sk, start);
                        let b = Self::point_for(sk, c);
                        if a != b {
                            let line = sk.add_line(a, b);
                            sk.entities[line].construction = self.tool == Tool::Centerline;
                            auto_hv(sk, line);
                            out.modified = true;
                        }
                        // Clicking onto an existing point closes the chain.
                        if c.snap.is_none() || a == b {
                            self.pending.push(Click { p: sk.pos(b), snap: Some(b) });
                        }
                    } else {
                        self.pending.push(c);
                    }
                }
                Tool::Rectangle => {
                    let c = self.snap(sk, mouse, radius);
                    if let Some(first) = self.pending.pop() {
                        if (c.p.x - first.p.x).abs() > 1e-9 && (c.p.y - first.p.y).abs() > 1e-9 {
                            let lines = sk.add_rectangle(first.p, c.p);
                            for click in [first, c] {
                                if let Some(id) = click.snap {
                                    let corner = rect_corner_near(sk, &lines, click.p);
                                    sk.add_constraint(Constraint::Coincident { p: corner, q: id });
                                }
                            }
                            self.status = sk.solve();
                            out.modified = true;
                        }
                    } else {
                        self.pending.push(c);
                    }
                }
                Tool::Circle => {
                    let c = self.snap(sk, mouse, radius);
                    if let Some(center) = self.pending.pop() {
                        let r = center.p.distance(mouse);
                        if r > 1e-6 {
                            let cid = Self::point_for(sk, center);
                            sk.add_circle(cid, r);
                            out.modified = true;
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
                        let end_p = center.p + (mouse - center.p).normalized() * r;
                        let end = match c.snap {
                            Some(id) if (sk.pos(id).distance(center.p) - r).abs() < radius => c,
                            _ => Click { p: end_p, snap: None },
                        };
                        if r > 1e-6 && end.p.distance(start.p) > 1e-6 {
                            let cid = Self::point_for(sk, center);
                            let a = Self::point_for(sk, start);
                            let b = Self::point_for(sk, end);
                            sk.add_arc(cid, a, b);
                            self.status = sk.solve();
                            out.modified = true;
                        }
                        self.pending.clear();
                    } else {
                        self.pending.push(c);
                    }
                }
                Tool::Dimension => {
                    let hit = self.pick(sk, cam, rect, screen, mouse, radius);
                    if let Some(ci) = self.dimension_click(sk, hit) {
                        self.selection = vec![SkSel::Constraint(ci)];
                        self.begin_dim_edit(sf, ci);
                        out.modified = true;
                    }
                }
            }
        }

        if out.modified && !out.dragging {
            self.status = sf.sketch.solve();
        }
        out
    }

    /// Smart Dimension: one click on a circle or arc dimensions it; a line needs a second
    /// (placement) click, or a second line for an angle; two points give a distance.
    fn dimension_click(&mut self, sk: &mut Sketch, hit: Option<SkSel>) -> Option<usize> {
        let first = self.pending_pick.take();
        let c = match (first, hit) {
            (None, Some(SkSel::Entity(e))) => match sk.entities[e].geom {
                Geom::Circle { r, .. } => Constraint::Diameter { entity: e, value: 2.0 * r },
                Geom::Arc { .. } => Constraint::Radius { entity: e, value: sk.radius(e) },
                Geom::Line { .. } => {
                    self.pending_pick = hit;
                    return None;
                }
            },
            (None, Some(SkSel::Point(p))) => {
                self.pending_pick = Some(SkSel::Point(p));
                return None;
            }
            (Some(SkSel::Entity(l)), Some(SkSel::Entity(m))) if l != m && sk.entities[m].is_line() => {
                let (u, w) = (line_dir(sk, l), line_dir(sk, m));
                Constraint::Angle { a: l, b: m, degrees: u.cross(w).atan2(u.dot(w)).to_degrees() }
            }
            (Some(SkSel::Entity(l)), _) => Constraint::Length { line: l, value: sk.line_length(l) },
            (Some(SkSel::Point(p)), Some(SkSel::Point(q))) if p != q => {
                Constraint::Distance { p, q, value: sk.pos(p).distance(sk.pos(q)) }
            }
            _ => return None,
        };
        Some(sk.add_constraint(c))
    }

    pub fn begin_dim_edit(&mut self, sf: &SketchFeature, ci: usize) {
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
            if !matches!(sf.sketch.constraints[c], Constraint::Fix { p: ORIGIN, .. }) {
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
        // Deleting a point deletes the curves that use it.
        for s in &self.selection {
            if let SkSel::Point(p) = s {
                if *p != ORIGIN {
                    for (ei, e) in sf.sketch.entities.iter().enumerate() {
                        if e.points().contains(p) {
                            ents.push(ei);
                        }
                    }
                }
            }
        }
        if !ents.is_empty() {
            sf.delete_entities(&ents);
        }
        self.selection.clear();
        self.dim_edit = None;
        self.status = sf.sketch.solve();
    }

    /// Relations that can be added for the current selection.
    pub fn available_relations(&self, sk: &Sketch) -> Vec<(&'static str, Constraint)> {
        let mut pts = Vec::new();
        let mut lines = Vec::new();
        let mut rounds = Vec::new();
        for s in &self.selection {
            match *s {
                SkSel::Point(p) => pts.push(p),
                SkSel::Entity(e) if sk.entities[e].is_line() => lines.push(e),
                SkSel::Entity(e) => rounds.push(e),
                SkSel::Constraint(_) => {}
            }
        }
        let mut out: Vec<(&'static str, Constraint)> = Vec::new();
        match (pts.len(), lines.len(), rounds.len()) {
            (0, 1, 0) => {
                out.push(("Horizontal", Constraint::Horizontal { line: lines[0] }));
                out.push(("Vertical", Constraint::Vertical { line: lines[0] }));
            }
            (0, 2, 0) => {
                let (a, b) = (lines[0], lines[1]);
                out.push(("Parallel", Constraint::Parallel { a, b }));
                out.push(("Perpendicular", Constraint::Perpendicular { a, b }));
                out.push(("Equal", Constraint::Equal { a, b }));
            }
            (0, 0, 2) => {
                let (a, b) = (rounds[0], rounds[1]);
                out.push(("Equal", Constraint::Equal { a, b }));
                out.push(("Concentric", Constraint::Concentric { a, b }));
            }
            (0, 1, 1) => out.push(("Tangent", Constraint::Tangent { line: lines[0], entity: rounds[0] })),
            (1, 0, 0) => {
                let p = pts[0];
                let q = sk.pos(p);
                out.push(("Fix", Constraint::Fix { p, x: q.x, y: q.y }));
            }
            (2, 0, 0) => {
                let (p, q) = (pts[0], pts[1]);
                out.push(("Coincident", Constraint::Coincident { p, q }));
                out.push(("Horizontal", Constraint::HorizontalPoints { p, q }));
                out.push(("Vertical", Constraint::VerticalPoints { p, q }));
            }
            (1, 1, 0) => {
                let (p, line) = (pts[0], lines[0]);
                out.push(("Coincident", Constraint::PointOnLine { p, line }));
                out.push(("Midpoint", Constraint::Midpoint { p, line }));
            }
            (1, 0, 1) => out.push(("Coincident", Constraint::PointOnCircle { p: pts[0], entity: rounds[0] })),
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

    /// Draws the sketch being edited, with tool previews.
    pub fn draw(&self, sf: &SketchFeature, painter: &Painter, cam: &Camera, rect: Rect) {
        let sel = |s: SkSel| self.selection.contains(&s);
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

        // Points.
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
            painter.circle_filled(s, if i == ORIGIN { 3.5 } else { 2.5 }, c);
        }
        draw_origin(&self.frame, painter, cam, rect);

        // Dimensions.
        for (ci, c) in sf.sketch.constraints.iter().enumerate() {
            let (Some(anchor), Some(v)) = (dimension_anchor(&sf.sketch, c), c.value()) else {
                continue;
            };
            let s = cam.project(self.frame.to_world(anchor), rect);
            let mut text = format_num(display_value(c, v));
            text = match c {
                Constraint::Diameter { .. } => format!("Ø{text}"),
                Constraint::Radius { .. } => format!("R{text}"),
                Constraint::Angle { .. } => format!("{text}°"),
                _ => text,
            };
            if sf.driven_expr(ci).is_some() {
                text = format!("Σ {text}");
            }
            let color = if sel(SkSel::Constraint(ci)) {
                Color32::from_rgb(255, 140, 0)
            } else if self.status.unsatisfied.contains(&ci) {
                Color32::from_rgb(220, 30, 30)
            } else if self.hover == Some(SkSel::Constraint(ci)) {
                Color32::from_rgb(240, 170, 60)
            } else {
                Color32::from_rgb(10, 30, 140)
            };
            let r = painter.text(s, Align2::CENTER_CENTER, &text, FontId::proportional(13.0), color);
            painter.rect_filled(r.expand(2.0), 2.0, Color32::from_rgba_unmultiplied(255, 255, 255, 170));
            painter.text(s, Align2::CENTER_CENTER, text, FontId::proportional(13.0), color);
        }

        // Tool preview.
        let Some(m) = self.mouse else { return };
        let to = |p: Vec2| cam.project(self.frame.to_world(p), rect);
        let preview = Stroke::new(1.2_f32, Color32::from_rgb(80, 120, 255));
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
            (Tool::Arc, [c]) => {
                painter.line_segment([to(c.p), to(m)], preview);
            }
            (Tool::Arc, [c, s]) => {
                let r = c.p.distance(s.p);
                let a0 = (s.p - c.p).angle();
                let sweep = ((m - c.p).angle() - a0).rem_euclid(TAU);
                let n = 48;
                let pts: Vec<Pos2> = (0..=n)
                    .map(|i| to(c.p + Vec2::from_angle(a0 + sweep * i as f64 / n as f64) * r))
                    .collect();
                painter.add(Shape::line(pts, preview));
            }
            _ => {}
        }
        // Snap indicator.
        if let Some(SkSel::Point(p)) = self.hover {
            if self.tool != Tool::Select {
                painter.circle_stroke(
                    to(sf.sketch.pos(p)),
                    6.0,
                    Stroke::new(1.5_f32, Color32::from_rgb(255, 140, 0)),
                );
            }
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
        let pts: Vec<Pos2> = match e.geom {
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
        };
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
/// automatic relations.
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

fn rect_corner_near(sk: &Sketch, lines: &[EntityId; 4], p: Vec2) -> PointId {
    lines
        .iter()
        .filter_map(|&l| match sk.entities[l].geom {
            Geom::Line { a, .. } => Some(a),
            _ => None,
        })
        .min_by(|&a, &b| sk.pos(a).distance(p).partial_cmp(&sk.pos(b).distance(p)).unwrap())
        .unwrap()
}

/// Where a dimension's value label is drawn, in sketch coordinates.
pub fn dimension_anchor(sk: &Sketch, c: &Constraint) -> Option<Vec2> {
    use Constraint::*;
    let mid = |e: EntityId| match sk.entities.get(e)?.geom {
        Geom::Line { a, b } => {
            let (pa, pb) = (sk.pos(a), sk.pos(b));
            Some((pa + pb) * 0.5)
        }
        _ => None,
    };
    let rim = |e: EntityId| {
        let ent = sk.entities.get(e)?;
        let center = match ent.geom {
            Geom::Circle { c, .. } | Geom::Arc { c, .. } => sk.pos(c),
            _ => return None,
        };
        let r = sk.radius(e);
        Some(center + Vec2::from_angle(std::f64::consts::FRAC_PI_4) * r)
    };
    match *c {
        Length { line, .. } => mid(line),
        Radius { entity, .. } | Diameter { entity, .. } => rim(entity),
        Distance { p, q, .. } => Some((sk.pos(p) + sk.pos(q)) * 0.5),
        // Horizontal distances sit level with the second point, vertical ones in line
        // with it, so a pair of them on the same points never overlap.
        HDistance { p, q, .. } => Some(v2((sk.pos(p).x + sk.pos(q).x) * 0.5, sk.pos(q).y)),
        VDistance { p, q, .. } => Some(v2(sk.pos(q).x, (sk.pos(p).y + sk.pos(q).y) * 0.5)),
        Angle { a, b, .. } => Some((mid(a)? + mid(b)?) * 0.5),
        _ => None,
    }
}

/// HDistance and VDistance store a signed value; show its magnitude.
pub fn display_value(c: &Constraint, v: f64) -> f64 {
    match c {
        Constraint::HDistance { .. } | Constraint::VDistance { .. } => v.abs(),
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
