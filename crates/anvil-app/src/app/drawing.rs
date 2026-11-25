//! Drawing mode: a drawing open in the main window instead of a part. The left pane holds
//! the sheet's settings (size, scale, projection), the part the drawing shows and its
//! views; the command bar updates the views and exports PDF; the view area shows the
//! sheet. Behaviour follows `docs/re/drawings.md`.
//!
//! Views are projected on a background thread from the part file on disk, so the window
//! stays responsive; the sheet shows progress and the work can be cancelled. The part file
//! is watched, so saving the part updates the drawing.

use super::AnvilApp;
use crate::theme::{self, Icon, Palette, RowLook};
use anvil_drawing::glam::DVec2;
use anvil_drawing::layout::{Pen, SheetPicture};
use anvil_drawing::{
    DisplayStyle, Drawing, Progress, Projection, Scale, Sheet, SheetSize, TangentEdges, ViewGeometry,
    ViewRole, DRAWING_EXT,
};
use eframe::egui::{self, Color32, Pos2, RichText, Sense, Stroke};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::sync::Arc;
use std::time::SystemTime;

/// How often the part file is checked for changes, in seconds.
const WATCH_EVERY: f64 = 1.0;

/// What a background view update came back with.
enum JobResult {
    Done(HashMap<u32, ViewGeometry>),
    Failed(String),
    Cancelled,
}

/// A view update running on a background thread.
struct Job {
    progress: Arc<Progress>,
    rx: Receiver<JobResult>,
    thread: Option<std::thread::JoinHandle<()>>,
}

/// How the sheet sits in the view area: `zoom` screen points per sheet millimetre and the
/// sheet point at the area's centre. `None` until the first frame fits the sheet.
#[derive(Clone, Copy)]
struct SheetView {
    zoom: f32,
    centre: DVec2,
}

pub(super) struct DrawingSession {
    pub drawing: Drawing,
    saved: Drawing,
    committed: Drawing,
    pub path: Option<PathBuf>,
    undo: Vec<Drawing>,
    redo: Vec<Drawing>,
    pub selected: Option<u32>,
    /// The projected views, by view id.
    geometry: HashMap<u32, ViewGeometry>,
    /// Why the views cannot be shown, in words for the user.
    pub problem: Option<String>,
    /// True when the views are older than the part or the settings (a cancelled update).
    stale: bool,
    job: Option<Job>,
    /// The part file's modification time when the views were last started.
    part_stamp: Option<SystemTime>,
    /// When the part file was last checked (egui time, seconds).
    checked: f64,
    /// Set for a new drawing: choose a scale that fits once the views are in.
    fit_pending: bool,
    scale_text: String,
    view: Option<SheetView>,
    /// The projection the geometry was made for; a change needs new views.
    projected_for: Option<Projection>,
}

impl DrawingSession {
    fn new(drawing: Drawing, path: Option<PathBuf>) -> DrawingSession {
        let scale_text = drawing.sheet.scale.to_string();
        DrawingSession {
            saved: drawing.clone(),
            committed: drawing.clone(),
            drawing,
            path,
            undo: Vec::new(),
            redo: Vec::new(),
            selected: None,
            geometry: HashMap::new(),
            problem: None,
            stale: false,
            job: None,
            part_stamp: None,
            checked: 0.0,
            fit_pending: false,
            scale_text,
            view: None,
            projected_for: None,
        }
    }

    pub fn is_dirty(&self) -> bool {
        self.drawing != self.saved
    }

    pub fn name(&self) -> String {
        self.path
            .as_ref()
            .and_then(|p| p.file_stem())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Drawing1".into())
    }

    fn model_path(&self) -> Option<PathBuf> {
        self.drawing.model_path(self.path.as_deref())
    }

    fn part_name(&self) -> String {
        self.model_path()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_else(|| "No part".into())
    }

    /// The progress of the running update, if one is running: the fraction done, or `None`
    /// while the part is still loading.
    fn progress(&self) -> Option<Option<f32>> {
        self.job.as_ref().map(|j| j.progress.fraction())
    }

    /// Starts projecting the views again from the part file, replacing any update that is
    /// still running.
    fn start_update(&mut self) {
        self.cancel_update();
        let Some(model) = self.model_path() else {
            self.problem = Some("The drawing has no part. Use Change Part to choose one.".into());
            return;
        };
        self.part_stamp = std::fs::metadata(&model).and_then(|m| m.modified()).ok();
        let progress = Arc::new(Progress::new());
        let (tx, rx) = channel();
        let drawing = self.drawing.clone();
        let p = progress.clone();
        let thread = std::thread::Builder::new().name("drawing views".into()).spawn(move || {
            let result = if model.exists() {
                match anvil_drawing::load_part(&model) {
                    Ok(bodies) => match anvil_drawing::project_views(&drawing, &bodies, &p) {
                        Ok(g) => JobResult::Done(g),
                        Err(_) => JobResult::Cancelled,
                    },
                    Err(e) => JobResult::Failed(format!("The views cannot be drawn: {e}.")),
                }
            } else {
                JobResult::Failed(format!(
                    "The part {} cannot be found. Put it back, or use Change Part to point the drawing at it.",
                    model.display()
                ))
            };
            let _ = tx.send(result);
        });
        match thread {
            Ok(t) => {
                self.projected_for = Some(self.drawing.sheet.projection);
                self.job = Some(Job { progress, rx, thread: Some(t) });
            }
            Err(e) => self.problem = Some(format!("The views cannot be updated: {e}.")),
        }
    }

    fn cancel_update(&mut self) {
        if let Some(j) = self.job.take() {
            j.progress.cancel();
            self.stale = true;
        }
    }

    /// Waits for a running update to finish (the headless harness, so runs repeat).
    pub fn finish_update(&mut self) {
        if let Some(t) = self.job.as_mut().and_then(|j| j.thread.take()) {
            let _ = t.join();
        }
    }

    /// Takes in a finished update. Returns a status line to show, if there is news.
    fn poll(&mut self) -> Option<String> {
        let j = self.job.as_ref()?;
        let result = match j.rx.try_recv() {
            Ok(r) => r,
            Err(TryRecvError::Empty) => return None,
            Err(TryRecvError::Disconnected) => {
                JobResult::Failed("The view update stopped unexpectedly.".into())
            }
        };
        self.job = None;
        Some(match result {
            JobResult::Done(g) => {
                self.geometry = g;
                self.problem = None;
                self.stale = false;
                if std::mem::take(&mut self.fit_pending) {
                    self.fit_scale();
                    // Part of making the drawing, not a change to undo.
                    self.committed = self.drawing.clone();
                    if self.path.is_none() {
                        self.saved = self.drawing.clone();
                    }
                }
                "Views updated".into()
            }
            JobResult::Failed(e) => {
                self.geometry.clear();
                self.problem = Some(e.clone());
                e
            }
            JobResult::Cancelled => "View update cancelled; the views may be out of date".into(),
        })
    }

    /// Sets the largest standard scale at which the views fit the sheet.
    fn fit_scale(&mut self) {
        let g = &self.geometry;
        let scale =
            anvil_drawing::layout::fit_scale(&self.drawing, |v| g.get(&v.id).and_then(|g| g.bounds()));
        self.drawing.sheet.scale = scale;
        self.scale_text = scale.to_string();
    }

    fn picture(&self) -> SheetPicture {
        anvil_drawing::layout(&self.drawing, &self.geometry)
    }

    fn commit(&mut self) {
        if self.drawing != self.committed {
            self.undo.push(std::mem::replace(&mut self.committed, self.drawing.clone()));
            if self.undo.len() > 200 {
                self.undo.remove(0);
            }
            self.redo.clear();
        }
    }

    /// Fits the whole sheet in the view area on the next frame.
    pub fn fit_view(&mut self) {
        self.view = None;
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
}

impl Drop for DrawingSession {
    fn drop(&mut self) {
        self.cancel_update();
    }
}

fn view_label(role: ViewRole, projection: Projection) -> &'static str {
    role.orientation(projection).label()
}

fn view_icon(role: ViewRole, projection: Projection) -> Icon {
    match role.orientation(projection) {
        anvil_drawing::Orientation::Front => Icon::ViewFront,
        anvil_drawing::Orientation::Top => Icon::ViewTop,
        anvil_drawing::Orientation::Isometric => Icon::ViewIso,
        _ => Icon::ViewRight,
    }
}

/// The colour lines are drawn on paper with, in either theme: paper stays white.
const INK: Color32 = Color32::from_rgb(0x1a, 0x1a, 0x1a);

impl AnvilApp {
    /// Waits for background work (a drawing's view update) to finish, so a headless run
    /// gives the same result every time.
    pub fn finish_background_work(&mut self) {
        if let Some(s) = &mut self.drw {
            s.finish_update();
        }
    }

    pub(super) fn in_drawing(&self) -> bool {
        self.drw.is_some()
    }

    /// Starts a drawing of the open part, with the standard views and an isometric. The
    /// part must have been saved, because the drawing refers to its file.
    pub(super) fn new_drawing(&mut self) {
        if self.in_drawing() {
            self.message =
                Some(("New Drawing".into(), "Open the part to draw first, then choose New Drawing.".into()));
            return;
        }
        if self.in_assembly() {
            self.message = Some((
                "New Drawing".into(),
                "Drawings of assemblies are not supported yet. Open a part, then choose New Drawing.".into(),
            ));
            return;
        }
        let Some(part) = self.path.clone() else {
            self.message = Some((
                "New Drawing".into(),
                "Save the part first: a drawing shows a part file, and updates when that file changes."
                    .into(),
            ));
            return;
        };
        let part = std::path::absolute(&part).unwrap_or(part);
        let mut d = Drawing::standard(String::new(), Sheet::default());
        d.set_model(&part, None);
        let mut s = DrawingSession::new(d, None);
        s.fit_pending = true;
        self.enter_drawing(s);
        self.status = "New drawing: projecting the views".into();
    }

    pub(super) fn open_drawing(&mut self, p: &Path) {
        match Drawing::load(p) {
            Ok(d) => {
                self.enter_drawing(DrawingSession::new(d, Some(p.to_path_buf())));
                self.status = format!("Opened {}", p.display());
            }
            Err(e) => self.message = Some(("Open failed".into(), format!("{}: {e}", p.display()))),
        }
    }

    /// Opens recovered drawing work, compared with the user's file so it shows as unsaved.
    pub(super) fn restore_drawing(&mut self, d: Drawing, path: Option<PathBuf>) {
        let on_disk = path.as_ref().and_then(|p| Drawing::load(p).ok());
        self.enter_drawing(DrawingSession::new(d, path));
        if let Some(s) = &mut self.drw {
            s.saved = on_disk.unwrap_or_default();
        }
    }

    fn enter_drawing(&mut self, mut s: DrawingSession) {
        self.leave_assembly();
        self.sketch = None;
        self.editor = None;
        self.measure = None;
        self.pick_body = None;
        self.selection = super::Selection::None;
        s.start_update();
        self.drw = Some(s);
    }

    /// Leaves drawing mode (when a part or an assembly is opened or started).
    pub(super) fn leave_drawing(&mut self) {
        if self.drw.take().is_some() {
            self.fit_pending = true;
        }
    }

    /// Projects the views again (Rebuild and Update Views).
    pub(super) fn update_views(&mut self) {
        if let Some(s) = &mut self.drw {
            s.start_update();
            self.status = "Updating views".into();
        }
    }

    pub(super) fn drawing_undo(&mut self, redo: bool) {
        let Some(s) = &mut self.drw else { return };
        let (from, to) = if redo { (&mut s.redo, &mut s.undo) } else { (&mut s.undo, &mut s.redo) };
        if let Some(d) = from.pop() {
            let old = std::mem::replace(&mut s.drawing, d);
            let part_changed = old.model != s.drawing.model;
            to.push(old);
            s.committed = s.drawing.clone();
            s.scale_text = s.drawing.sheet.scale.to_string();
            if s.selected.is_some_and(|id| s.drawing.view(id).is_none()) {
                s.selected = None;
            }
            if part_changed || s.projected_for != Some(s.drawing.sheet.projection) {
                s.start_update();
            }
        }
    }

    pub(super) fn save_drawing(&mut self, save_as: bool) {
        let Some(s) = &self.drw else { return };
        let path = match (&s.path, save_as) {
            (Some(p), false) => Some(p.clone()),
            _ => {
                let name = format!("{}.{DRAWING_EXT}", s.name());
                self.file_dialog("Save As", || {
                    rfd::FileDialog::new()
                        .add_filter("AnvilCAD drawing", &[DRAWING_EXT])
                        .set_file_name(name)
                        .save_file()
                })
            }
        };
        let Some(path) = path else { return };
        let Some(s) = &mut self.drw else { return };
        // The part is kept relative to the drawing's folder where it can be.
        let mut out = s.drawing.clone();
        if let Some(model) = s.model_path() {
            let model = std::path::absolute(&model).unwrap_or(model);
            out.set_model(&model, Some(&path));
        }
        match out.save(&path) {
            Ok(()) => {
                s.drawing = out;
                s.path = Some(path.clone());
                s.saved = s.drawing.clone();
                s.committed = s.drawing.clone();
                self.status = format!("Saved {}", path.display());
            }
            Err(e) => self.message = Some(("Save failed".into(), e.to_string())),
        }
    }

    /// Writes the sheet as a PDF file the user picks.
    pub(super) fn export_drawing_pdf(&mut self) {
        let Some(s) = &self.drw else {
            self.message = Some(("Export PDF".into(), "Open or start a drawing to export it as PDF.".into()));
            return;
        };
        if s.job.is_some() {
            self.message = Some((
                "Export PDF".into(),
                "The views are still updating. Export again when they are done.".into(),
            ));
            return;
        }
        if let Some(p) = &s.problem {
            self.message = Some(("Export PDF".into(), p.clone()));
            return;
        }
        let name = format!("{}.pdf", s.name());
        let Some(path) = self.file_dialog("Export PDF", || {
            rfd::FileDialog::new().add_filter("PDF", &["pdf"]).set_file_name(name).save_file()
        }) else {
            return;
        };
        let Some(s) = &self.drw else { return };
        match anvil_drawing::export_pdf(&s.drawing, &s.geometry, &path, &s.name()) {
            Ok(()) => self.status = format!("Exported {}", path.display()),
            Err(e) => self.message = Some(("Export failed".into(), format!("{}: {e}", path.display()))),
        }
    }

    /// Points the drawing at another part file.
    pub(super) fn change_drawing_part(&mut self) {
        if self.drw.is_none() {
            return;
        }
        let Some(part) = self.file_dialog("Change Part", || {
            rfd::FileDialog::new().add_filter("AnvilCAD part", &[anvil_model::PART_EXT]).pick_file()
        }) else {
            return;
        };
        let Some(s) = &mut self.drw else { return };
        let part = std::path::absolute(&part).unwrap_or(part);
        let path = s.path.clone();
        s.drawing.set_model(&part, path.as_deref());
        s.fit_pending = s.geometry.is_empty();
        s.start_update();
        self.status = format!("The drawing now shows {}", s.part_name());
    }

    /// Runs every frame in drawing mode: takes in finished updates and watches the part.
    fn drawing_tick(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        let Some(s) = &mut self.drw else { return };
        if let Some(msg) = s.poll() {
            self.status = msg;
        }
        if s.job.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
            return;
        }
        if now - s.checked >= WATCH_EVERY {
            s.checked = now;
            if let Some(model) = s.model_path() {
                let stamp = std::fs::metadata(&model).and_then(|m| m.modified()).ok();
                if stamp != s.part_stamp {
                    s.start_update();
                    self.status = format!("{} changed: updating views", s.part_name());
                }
            }
        }
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(WATCH_EVERY));
    }

    /// Ends the frame's edits: records an undo step, and starts new views when the
    /// projection changed.
    pub(super) fn drawing_commit(&mut self) {
        let Some(s) = &mut self.drw else { return };
        if s.job.is_none() && s.projected_for.is_some() && s.projected_for != Some(s.drawing.sheet.projection)
        {
            s.start_update();
        }
        s.commit();
    }

    pub(super) fn drawing_commands(&mut self, ui: &mut egui::Ui) {
        let busy = self.drw.as_ref().is_some_and(|s| s.job.is_some());
        theme::command_group(ui, "Views", |ui| {
            if theme::command_button(ui, Icon::Rebuild, "Update Views", None).clicked() {
                self.update_views();
            }
        });
        theme::group_rule(ui);
        theme::command_group(ui, "Part", |ui| {
            if theme::command_button(ui, Icon::Part, "Change Part", None).clicked() {
                self.change_drawing_part();
            }
        });
        theme::group_rule(ui);
        theme::command_group(ui, "Output", |ui| {
            let r =
                ui.add_enabled_ui(!busy, |ui| theme::command_button(ui, Icon::ExportPdf, "Export PDF", None));
            if r.inner.clicked() {
                self.export_drawing_pdf();
            }
        });
    }

    /// The left pane in drawing mode.
    pub(super) fn drawing_tree(&mut self, ui: &mut egui::Ui) {
        let pal = Palette::of(ui);
        let Some(s) = &mut self.drw else { return };
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(egui::vec2(20.0, 20.0), Sense::hover());
            theme::paint_icon(ui.painter(), r, Icon::Drawing, pal.text, pal.accent);
            ui.label(RichText::new(s.name()).size(15.0).strong());
        });
        ui.add_space(6.0);

        theme::section_label(ui, "Sheet");
        let mut message = None;
        egui::Grid::new("drawing-sheet").num_columns(2).min_col_width(92.0).spacing([8.0, 6.0]).show(
            ui,
            |ui| {
                let name = ui.label("Size").id;
                let sheet = &mut s.drawing.sheet;
                egui::ComboBox::from_id_salt("sheet-size")
                    .width(150.0)
                    .selected_text(sheet.size.label())
                    .show_ui(ui, |ui| {
                        for z in SheetSize::ALL {
                            ui.selectable_value(&mut sheet.size, z, z.label());
                        }
                    })
                    .response
                    .labelled_by(name);
                ui.end_row();
                let name = ui.label("Scale").id;
                ui.horizontal(|ui| {
                    let r = ui
                        .add(egui::TextEdit::singleline(&mut s.scale_text).desired_width(70.0))
                        .labelled_by(name);
                    if r.lost_focus() {
                        match Scale::parse(&s.scale_text) {
                            Ok(v) => s.drawing.sheet.scale = v,
                            Err(e) => {
                                message = Some(format!("Scale: {e}"));
                                s.scale_text = s.drawing.sheet.scale.to_string();
                            }
                        }
                    }
                    let can_fit = !s.geometry.is_empty();
                    if ui
                        .add_enabled(can_fit, egui::Button::new("Fit"))
                        .on_hover_text("The largest standard scale at which the views fit the sheet")
                        .clicked()
                    {
                        s.fit_scale();
                    }
                });
                ui.end_row();
                let name = ui.label("Projection").id;
                let sheet = &mut s.drawing.sheet;
                egui::ComboBox::from_id_salt("sheet-projection")
                    .width(150.0)
                    .selected_text(sheet.projection.label())
                    .show_ui(ui, |ui| {
                        for p in Projection::ALL {
                            ui.selectable_value(&mut sheet.projection, p, p.label());
                        }
                    })
                    .response
                    .labelled_by(name);
                ui.end_row();
            },
        );
        if let Some(m) = message {
            self.status = m;
        }
        let Some(s) = &mut self.drw else { return };

        ui.add_space(6.0);
        theme::section_label(ui, "Part");
        let look = RowLook { error: s.problem.is_some(), ..Default::default() };
        let mut row = theme::tree_row(ui, Icon::Part, &s.part_name(), look);
        if let Some(p) = s.model_path() {
            row = row.on_hover_text(p.display().to_string());
        }
        let _ = row;

        ui.add_space(6.0);
        theme::section_label(ui, "Views");
        let projection = s.drawing.sheet.projection;
        let mut select = None;
        for v in &s.drawing.views {
            let look = RowLook { selected: s.selected == Some(v.id), ..Default::default() };
            if theme::tree_row(ui, view_icon(v.role, projection), view_label(v.role, projection), look)
                .clicked()
            {
                select = Some(v.id);
            }
        }
        if let Some(id) = select {
            s.selected = if s.selected == Some(id) { None } else { Some(id) };
        }

        let Some(id) = s.selected else { return };
        let Some(v) = s.drawing.view_mut(id) else { return };
        ui.add_space(6.0);
        theme::section_label(ui, &format!("{} view", view_label(v.role, projection)));
        egui::Grid::new("drawing-view").num_columns(2).min_col_width(92.0).spacing([8.0, 6.0]).show(
            ui,
            |ui| {
                let name = ui.label("Display").id;
                egui::ComboBox::from_id_salt("view-display")
                    .width(150.0)
                    .selected_text(v.display.label())
                    .show_ui(ui, |ui| {
                        for d in DisplayStyle::ALL {
                            ui.selectable_value(&mut v.display, d, d.label());
                        }
                    })
                    .response
                    .labelled_by(name);
                ui.end_row();
                let name = ui.label("Tangent edges").id;
                egui::ComboBox::from_id_salt("view-tangent")
                    .width(150.0)
                    .selected_text(v.tangent.label())
                    .show_ui(ui, |ui| {
                        for t in TangentEdges::ALL {
                            ui.selectable_value(&mut v.tangent, t, t.label());
                        }
                    })
                    .response
                    .labelled_by(name);
                ui.end_row();
            },
        );
    }

    /// The view area in drawing mode: the sheet, with progress and problems over it.
    pub(super) fn drawing_viewport(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        self.drawing_tick(&ctx);
        let pal = Palette::of(ui);
        let rect = ui.max_rect();
        let resp = ui.interact(rect, ui.id().with("drawing-sheet"), Sense::click_and_drag());
        let Some(s) = &mut self.drw else { return };
        let (w, h) = s.drawing.sheet.size.size_mm();
        let fit = |rect: egui::Rect| SheetView {
            zoom: ((rect.width() - 48.0) / w as f32).min((rect.height() - 48.0) / h as f32).max(0.05),
            centre: DVec2::new(w * 0.5, h * 0.5),
        };
        let focused_text = ctx.memory(|m| m.focused().is_some());
        if s.view.is_none()
            || (!focused_text && ctx.input(|i| i.key_pressed(egui::Key::F)))
            || resp.double_clicked()
        {
            s.view = Some(fit(rect));
        }
        let mut sv = s.view.unwrap_or_else(|| fit(rect));
        // Zoom about the pointer, pan by dragging.
        if resp.hovered() {
            let scroll = ctx.input(|i| i.smooth_scroll_delta.y);
            if let Some(m) = resp.hover_pos().filter(|_| scroll != 0.0) {
                let before = to_sheet(&sv, rect, m);
                sv.zoom = (sv.zoom * (scroll * 0.005).exp()).clamp(0.05, 200.0);
                let after = to_sheet(&sv, rect, m);
                sv.centre += before - after;
            }
        }
        if resp.dragged() {
            let d = resp.drag_delta();
            sv.centre -= DVec2::new(d.x as f64, -d.y as f64) / sv.zoom as f64;
        }
        s.view = Some(sv);

        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, pal.view_bottom);
        let to_screen = |p: DVec2| -> Pos2 {
            rect.center()
                + egui::vec2(
                    ((p.x - sv.centre.x) * sv.zoom as f64) as f32,
                    ((sv.centre.y - p.y) * sv.zoom as f64) as f32,
                )
        };
        let paper = egui::Rect::from_two_pos(to_screen(DVec2::new(0.0, h)), to_screen(DVec2::new(w, 0.0)));
        painter.rect_filled(paper.translate(egui::vec2(2.0, 3.0)), 0.0, Color32::from_black_alpha(40));
        painter.rect_filled(paper, 0.0, Color32::WHITE);

        let pic = s.picture();
        let zoom = sv.zoom;
        for st in &pic.strokes {
            let pts: Vec<Pos2> = st.path.points(0.3 / zoom as f64).into_iter().map(&to_screen).collect();
            let width = (st.pen.width() as f32 * zoom).max(if st.pen == Pen::Visible { 1.2 } else { 0.8 });
            let stroke = Stroke::new(width, INK);
            match st.pen.dash() {
                Some((d, g)) => {
                    painter.extend(egui::Shape::dashed_line(
                        &pts,
                        stroke,
                        (d as f32 * zoom).max(2.0),
                        (g as f32 * zoom).max(1.5),
                    ));
                }
                None => {
                    painter.add(egui::Shape::line(pts, stroke));
                }
            }
        }
        for n in &pic.notes {
            let size = (n.height as f32 * zoom * 1.4).max(1.0);
            if size >= 4.0 {
                painter.text(
                    to_screen(n.pos),
                    egui::Align2::RIGHT_BOTTOM,
                    &n.text,
                    egui::FontId::proportional(size),
                    INK,
                );
            }
        }

        // Pick views by clicking inside them.
        let hover_view = resp.hover_pos().and_then(|m| {
            let p = to_sheet(&sv, rect, m);
            pic.views
                .iter()
                .find(|v| {
                    p.x >= v.lo.x - 3.0 && p.x <= v.hi.x + 3.0 && p.y >= v.lo.y - 3.0 && p.y <= v.hi.y + 3.0
                })
                .map(|v| v.id)
        });
        if resp.clicked() {
            s.selected = hover_view;
        }
        for v in &pic.views {
            let r = egui::Rect::from_two_pos(
                to_screen(v.lo - DVec2::splat(3.0)),
                to_screen(v.hi + DVec2::splat(3.0)),
            );
            if s.selected == Some(v.id) {
                painter.rect_stroke(r, 2.0, Stroke::new(1.5, pal.select), egui::StrokeKind::Outside);
            } else if hover_view == Some(v.id) {
                painter.rect_stroke(
                    r,
                    2.0,
                    Stroke::new(1.0, pal.select.gamma_multiply(0.5)),
                    egui::StrokeKind::Outside,
                );
            }
        }

        // Progress, problems and stale views, over the top of the sheet.
        let progress = s.progress();
        let banner = if let Some(f) = progress {
            Some((
                match f {
                    Some(f) => format!("Updating views... {:.0}%", f * 100.0),
                    None => "Updating views: loading the part...".to_string(),
                },
                false,
            ))
        } else if let Some(p) = &s.problem {
            Some((p.clone(), true))
        } else if s.stale {
            Some(("The views may be out of date. Update Views to bring them up to date.".to_string(), false))
        } else {
            None
        };
        let mut cancel = false;
        let mut change = false;
        if let Some((text, error)) = banner {
            let width = (rect.width() - 40.0).min(520.0);
            let area = egui::Rect::from_min_size(
                egui::pos2(rect.center().x - width / 2.0, rect.top() + 16.0),
                egui::vec2(width, 0.0),
            );
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(area));
            egui::Frame::new()
                .fill(pal.float_fill)
                .stroke(Stroke::new(1.0, if error { pal.error } else { pal.border }))
                .corner_radius(8.0)
                .inner_margin(10.0)
                .show(&mut child, |ui| {
                    ui.set_width(width - 20.0);
                    ui.label(RichText::new(text).color(if error { pal.error } else { pal.text }));
                    if let Some(f) = progress {
                        ui.horizontal(|ui| {
                            ui.add(egui::ProgressBar::new(f.unwrap_or(0.0)).desired_width(width - 110.0));
                            cancel = theme::secondary_button(ui, "Cancel").clicked();
                        });
                    } else if error {
                        change = theme::secondary_button(ui, "Change Part...").clicked();
                    }
                });
        }
        if cancel {
            if let Some(s) = &mut self.drw {
                s.cancel_update();
                self.status = "View update cancelled; the views may be out of date".into();
            }
        }
        if change {
            self.change_drawing_part();
        }
    }
}

fn to_sheet(sv: &SheetView, rect: egui::Rect, p: Pos2) -> DVec2 {
    let d = p - rect.center();
    DVec2::new(sv.centre.x + d.x as f64 / sv.zoom as f64, sv.centre.y - d.y as f64 / sv.zoom as f64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use anvil_model::Document;

    fn temp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("anvil-drawing-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).expect("temp dir");
        d
    }

    fn step(ctx: &egui::Context, app: &mut AnvilApp) {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(1400.0, 900.0))),
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| app.frame(ctx));
    }

    fn settle(ctx: &egui::Context, app: &mut AnvilApp) {
        if let Some(s) = &mut app.drw {
            s.finish_update();
        }
        for _ in 0..3 {
            step(ctx, app);
        }
    }

    #[test]
    fn new_drawing_needs_a_saved_part_and_shows_four_views() {
        let ctx = egui::Context::default();
        let mut app = AnvilApp::new(&ctx, None, None, None);
        app.load_document(crate::sample::sample_part(), None);
        app.new_drawing();
        assert!(!app.in_drawing());
        assert!(app.message.as_ref().is_some_and(|m| m.1.contains("Save the part first")));
        app.message = None;

        let dir = temp_dir("new");
        let part = dir.join("block.anvprt");
        app.doc.save(&part).expect("save part");
        app.path = Some(part.clone());
        app.saved = app.doc.clone();
        app.new_drawing();
        assert!(app.in_drawing());
        settle(&ctx, &mut app);
        let s = app.drw.as_ref().expect("drawing");
        assert_eq!(s.problem, None);
        assert_eq!(s.geometry.len(), 4);
        // The scale was fitted to the sheet as part of making it, not as a step to undo.
        assert!(s.undo.is_empty());
        assert_ne!(s.drawing.sheet.scale, Scale::FULL, "the sample part is fitted below or above 1:1");

        // Save, export and reopen.
        let file = dir.join("block.anvdrw");
        app.queue_file_dialog(Some(file.clone()));
        app.save_drawing(false);
        let saved = Drawing::load(&file).expect("saved drawing");
        assert_eq!(saved.model, "block.anvprt");
        let pdf = dir.join("block.pdf");
        app.queue_file_dialog(Some(pdf.clone()));
        app.export_drawing_pdf();
        let bytes = std::fs::read(&pdf).expect("pdf written");
        assert!(bytes.starts_with(b"%PDF-1.4"));
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.matches("\nS\n").count() > 20 && !text.contains("/Image"));

        app.leave_drawing();
        app.open_path(&file);
        assert!(app.in_drawing());
        settle(&ctx, &mut app);
        assert_eq!(app.drw.as_ref().map(|s| s.geometry.len()), Some(4));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn views_update_when_the_part_file_changes_and_report_a_missing_part() {
        let ctx = egui::Context::default();
        let mut app = AnvilApp::new(&ctx, None, None, None);
        let dir = temp_dir("watch");
        let part = dir.join("p.anvprt");
        crate::sample::sample_part().save(&part).expect("save");
        let mut d = Drawing::standard("p.anvprt".into(), Sheet::default());
        let file = dir.join("p.anvdrw");
        d.set_model(&part, Some(&file));
        d.save(&file).expect("save drawing");
        app.open_path(&file);
        settle(&ctx, &mut app);
        let width = |app: &AnvilApp| {
            let s = app.drw.as_ref().expect("drawing");
            let front = s.drawing.views[0].id;
            s.geometry.get(&front).and_then(|g| g.bounds()).map(|(lo, hi)| hi.x - lo.x)
        };
        let before = width(&app).expect("front view");

        // Replace the part with a plain 10 mm wide block; the watcher picks it up.
        let block = block_part(10.0);
        std::thread::sleep(std::time::Duration::from_millis(20));
        block.save(&part).expect("save");
        if let Some(s) = &mut app.drw {
            s.part_stamp = None; // as if the clock moved on
            s.checked = -10.0;
        }
        step(&ctx, &mut app);
        settle(&ctx, &mut app);
        let after = width(&app).expect("front view");
        assert!((after - 10.0).abs() < 1e-6, "front view is {after} wide, was {before}");

        std::fs::remove_file(&part).expect("remove part");
        app.update_views();
        settle(&ctx, &mut app);
        let s = app.drw.as_ref().expect("drawing");
        assert!(s.problem.as_ref().is_some_and(|p| p.contains("cannot be found")), "{:?}", s.problem);
        assert!(s.geometry.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn changing_the_projection_moves_the_side_view_and_undo_restores_it() {
        let ctx = egui::Context::default();
        let mut app = AnvilApp::new(&ctx, None, None, None);
        let dir = temp_dir("proj");
        let part = dir.join("b.anvprt");
        block_part(20.0).save(&part).expect("save");
        let mut d = Drawing::standard(String::new(), Sheet::default());
        d.set_model(&part, None);
        app.enter_drawing(DrawingSession::new(d, None));
        settle(&ctx, &mut app);
        let side_x = |app: &AnvilApp| {
            let s = app.drw.as_ref().expect("drawing");
            let pic = s.picture();
            let id = |r: ViewRole| s.drawing.views.iter().find(|v| v.role == r).map(|v| v.id);
            let at = |id: Option<u32>| pic.views.iter().find(|v| Some(v.id) == id).map(|v| v.lo);
            (at(id(ViewRole::Front)), at(id(ViewRole::Side)), at(id(ViewRole::Top)))
        };
        let (front, side, top) = side_x(&app);
        let (front, side, top) = (front.expect("front"), side.expect("side"), top.expect("top"));
        assert!(side.x > front.x && top.y < front.y, "first angle: side right, top below");
        if let Some(s) = &mut app.drw {
            s.drawing.sheet.projection = Projection::ThirdAngle;
        }
        step(&ctx, &mut app);
        settle(&ctx, &mut app);
        let (front, _, top) = side_x(&app);
        assert!(top.expect("top").y > front.expect("front").y, "third angle: top above");
        app.undo();
        settle(&ctx, &mut app);
        assert_eq!(app.drw.as_ref().map(|s| s.drawing.sheet.projection), Some(Projection::FirstAngle));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A part made of one block `w` wide, 20 tall and 10 deep.
    pub(super) fn block_part(w: f64) -> Document {
        use anvil_kernel::v2;
        use anvil_model::{BodyOp, Dim, EndCondition, ExtrudeFeature, FeatureKind, PlaneRef, SketchFeature};
        let mut doc = Document::new();
        let mut sf = SketchFeature::new(PlaneRef::Front);
        sf.sketch.add_rectangle(v2(0.0, 0.0), v2(w, 20.0));
        let sk = doc.add_feature(FeatureKind::Sketch(sf));
        doc.add_feature(FeatureKind::Extrude(ExtrudeFeature {
            sketch: sk,
            end: EndCondition::Blind,
            depth: Dim::Value(10.0),
            reverse: false,
            op: BodyOp::Boss,
            ..Default::default()
        }));
        doc
    }
}
