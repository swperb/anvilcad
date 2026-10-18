//! Interactive sketch editing: tools, snapping, selection, dragging, dimensions and the
//! on-screen sketch overlay.

use crate::camera::Camera;
use anvil_kernel::{v2, Frame, Vec2};
use anvil_model::SketchFeature;
use anvil_sketch::dimension::{self, Pick};
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
    Point,
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
            Tool::Point => "Point",
            Tool::Dimension => "Dimension",
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
            picks: Vec::new(),
            over_define: None,
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
        self.picks.clear();
    }

    pub fn has_pending(&self) -> bool {
        !self.pending.is_empty() || !self.picks.is_empty()
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
                Tool::Point => {
                    // A lone point, e.g. where a hole goes. Clicking an existing point
                    // adds nothing.
                    if sk.pick_point(mouse, radius).is_none() {
                        sk.add_point(mouse);
                        out.modified = true;
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
                        out.modified = true;
                    } else if let (Some(SkSel::Constraint(ci)), true) = (hit, self.picks.is_empty()) {
                        // Clicking an existing dimension's value edits it.
                        self.selection = vec![SkSel::Constraint(ci)];
                        self.begin_dim_edit(sf, ci);
                    }
                }
            }
        }

        if out.modified && !out.dragging {
            self.status = sf.sketch.solve();
        }
        out
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
        self.over_define = None;
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
        let sel = |s: SkSel| {
            self.selection.contains(&s)
                || match s {
                    SkSel::Point(p) => self.picks.contains(&Pick::Point(p)),
                    SkSel::Entity(e) => self.picks.contains(&Pick::Entity(e)),
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
