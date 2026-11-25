//! The Interference panel of assembly mode: which components overlap, by how much, which
//! touch, and which are closer than a clearance. Behaviour follows "Interference and
//! clearance" in `docs/re/assemblies-and-mates.md`; the check itself is
//! [`anvil_model::interference`].
//!
//! The parts' bodies are gathered on the UI thread (they are already loaded) and checked on
//! a background thread, so the window stays responsive; the panel shows progress and the
//! check can be cancelled. While the panel shows findings, the components are drawn as
//! wireframe and the overlap volumes shaded, the selected one red; touching and clearance
//! findings show a line between their closest points.

use super::AnvilApp;
use crate::theme::{self, Icon, Palette, RowLook};
use anvil_kernel::{FaceId, Mesh, Solid, Vec3};
use anvil_model::assembly::Assembly;
use anvil_model::interference::{self, Cancelled, Finding, FindingKind, Options, Progress, Report};
use eframe::egui::{self, RichText, Stroke};
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::sync::Arc;

/// Face ids at or above this name the overlap volume of a finding (`OVERLAP_FACE + k` for
/// finding `k`) in the view.
const OVERLAP_FACE: u32 = 1 << 29;

/// A check running on a background thread.
struct Job {
    progress: Arc<Progress>,
    rx: Receiver<Result<Report, Cancelled>>,
    thread: Option<std::thread::JoinHandle<()>>,
    /// The assembly being checked.
    checked: Assembly,
    skipped: Vec<(Vec<u32>, String)>,
}

impl Drop for Job {
    fn drop(&mut self) {
        self.progress.cancel();
    }
}

/// The Interference panel's options, kept while the panel is closed.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct InterferenceOptions {
    pub touching: bool,
    pub clearance_on: bool,
    /// The clearance as typed, in mm.
    pub clearance: String,
    pub include_hidden: bool,
    /// Check each top-level subassembly as one component.
    pub whole_subassemblies: bool,
}

impl Default for InterferenceOptions {
    fn default() -> Self {
        InterferenceOptions {
            touching: false,
            clearance_on: false,
            clearance: "0.5".into(),
            include_hidden: false,
            whole_subassemblies: false,
        }
    }
}

impl InterferenceOptions {
    /// The options for the model, or what is wrong with them.
    pub fn to_options(&self) -> Result<Options, String> {
        let clearance = if self.clearance_on {
            let c: f64 = self.clearance.trim().parse().map_err(|_| "type a clearance in mm".to_string())?;
            if !(c.is_finite() && c > 0.0) {
                return Err("the clearance must be more than 0 mm".into());
            }
            Some(c)
        } else {
            None
        };
        Ok(Options {
            clearance,
            touching: self.touching,
            include_hidden: self.include_hidden,
            components: Vec::new(),
            whole_subassemblies: self.whole_subassemblies,
        })
    }
}

/// The open Interference panel.
#[derive(Default)]
pub(super) struct InterferenceTool {
    job: Option<Job>,
    pub report: Option<Report>,
    /// The finding selected in the list or the view.
    pub selected: Option<usize>,
    /// Why there are no results to show (a bad option, a cancelled check, a change).
    pub note: Option<String>,
    /// The assembly the report describes.
    checked: Option<Assembly>,
    /// The overlap volumes, each finding's faces named `OVERLAP_FACE + k`.
    pub regions: Solid,
    regions_mesh: Mesh,
}

impl InterferenceTool {
    pub fn running(&self) -> bool {
        self.job.is_some()
    }

    /// Pairs checked and pairs in all, while a check runs.
    pub fn counts(&self) -> Option<(usize, usize)> {
        self.job.as_ref().map(|j| j.progress.counts())
    }

    /// True while findings are shown: the view then draws components as wireframe.
    pub fn shows_findings(&self) -> bool {
        self.report.as_ref().is_some_and(|r| !r.findings.is_empty())
    }

    pub fn finding(&self, k: usize) -> Option<&Finding> {
        self.report.as_ref()?.findings.get(k)
    }

    /// The finding whose overlap volume a view face belongs to.
    pub fn finding_of_face(&self, f: FaceId) -> Option<usize> {
        let k = f.feature.checked_sub(OVERLAP_FACE)? as usize;
        self.finding(k).map(|_| k)
    }

    /// The faces of the selected finding's overlap volume.
    pub fn selected_faces(&self) -> Vec<FaceId> {
        match self.selected {
            Some(k) => vec![FaceId { feature: OVERLAP_FACE + k as u32, index: 0 }],
            None => Vec::new(),
        }
    }

    pub fn mesh(&self) -> &Mesh {
        &self.regions_mesh
    }

    fn cancel(&mut self) {
        if let Some(j) = self.job.take() {
            j.progress.cancel();
        }
    }

    /// Waits for a running check to finish (the headless harness, so runs repeat).
    pub fn finish(&mut self) {
        if let Some(t) = self.job.as_mut().and_then(|j| j.thread.take()) {
            let _ = t.join();
        }
    }

    fn show(&mut self, report: Report, checked: Assembly) {
        let mut polygons = Vec::new();
        for (k, f) in report.findings.iter().enumerate() {
            for mut p in f.region.polygons.iter().cloned() {
                p.tag.id = FaceId { feature: OVERLAP_FACE + k as u32, index: 0 };
                polygons.push(p);
            }
        }
        self.regions = Solid::new(polygons);
        self.regions_mesh = self.regions.tessellate();
        self.selected = (!report.findings.is_empty()).then_some(0);
        self.report = Some(report);
        self.checked = Some(checked);
        self.note = None;
    }

    fn clear(&mut self) {
        self.report = None;
        self.selected = None;
        self.checked = None;
        self.regions = Solid::default();
        self.regions_mesh = Mesh::default();
    }

    /// Takes in a finished check. Returns a status line when there is news.
    fn poll(&mut self, names: &dyn Fn(&[u32]) -> String) -> Option<String> {
        let j = self.job.as_ref()?;
        let result = match j.rx.try_recv() {
            Ok(r) => r,
            Err(TryRecvError::Empty) => return None,
            Err(TryRecvError::Disconnected) => Err(Cancelled),
        };
        let mut job = self.job.take()?;
        let checked = std::mem::replace(&mut job.checked, Assembly::new());
        let skipped = std::mem::take(&mut job.skipped);
        Some(match result {
            Ok(mut report) => {
                report.skipped = skipped;
                let line = summary(&report, names);
                self.show(report, checked);
                format!("Interference: {line}")
            }
            Err(Cancelled) => {
                self.note = Some("The check was cancelled. Calculate to check again.".into());
                "Interference check cancelled".into()
            }
        })
    }
}

/// One line saying what a report found, such as "2 overlaps, 1 touching".
pub(super) fn summary(r: &Report, names: &dyn Fn(&[u32]) -> String) -> String {
    let count = |want: fn(&FindingKind) -> bool| r.findings.iter().filter(|f| want(&f.kind)).count();
    let overlaps = count(|k| matches!(k, FindingKind::Overlap { .. }));
    let touching = count(|k| matches!(k, FindingKind::Touching));
    let close = count(|k| matches!(k, FindingKind::Clearance { .. }));
    let mut parts = Vec::new();
    let plural = |n: usize, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
    if overlaps > 0 {
        parts.push(plural(overlaps, "overlap", "overlaps"));
    }
    if touching > 0 {
        parts.push(plural(touching, "pair touching", "pairs touching"));
    }
    if close > 0 {
        parts.push(plural(close, "pair within the clearance", "pairs within the clearance"));
    }
    let mut line = if parts.is_empty() {
        format!("no interference ({} checked)", plural(r.pairs, "pair", "pairs"))
    } else {
        parts.join(", ")
    };
    if !r.skipped.is_empty() {
        let who: Vec<String> = r.skipped.iter().map(|(c, _)| names(c)).collect();
        line.push_str(&format!("; not checked: {} (file not loaded)", who.join(", ")));
    }
    line
}

/// What a finding measured, as the list shows it.
pub(super) fn amount(f: &Finding) -> String {
    let approx = if f.exact { "" } else { "~" };
    match f.kind {
        FindingKind::Overlap { volume } => format!("{approx}{volume:.3} mm³"),
        FindingKind::Touching => "touching".into(),
        FindingKind::Clearance { distance } => format!("{approx}{distance:.3} mm"),
    }
}

impl AnvilApp {
    /// Opens the Interference panel and checks the assembly.
    pub(super) fn open_interference(&mut self) {
        let Some(s) = &mut self.asm else {
            self.status = "Open an assembly to check it for interference".into();
            return;
        };
        s.mate = None;
        if s.interference.is_none() {
            s.interference = Some(InterferenceTool::default());
        }
        if let Err(e) = self.start_interference() {
            self.status = format!("Interference: {e}");
        }
    }

    pub(super) fn close_interference(&mut self) {
        if let Some(s) = &mut self.asm {
            if let Some(mut t) = s.interference.take() {
                t.cancel();
            }
        }
        self.show_assembly_scene();
    }

    /// Starts checking the assembly with the panel's options, replacing a running check.
    pub(super) fn start_interference(&mut self) -> Result<(), String> {
        let Some(s) = &mut self.asm else { return Err("no assembly is open".into()) };
        let Some(t) = &mut s.interference else { return Err("the Interference panel is not open".into()) };
        t.cancel();
        let opts = match s.interference_opts.to_options() {
            Ok(o) => o,
            Err(e) => {
                t.note = Some(format!("Cannot check: {e}."));
                return Err(e);
            }
        };
        let (bodies, skipped) = s.asm.check_bodies(&mut s.parts, &opts);
        let progress = Arc::new(Progress::new());
        let (tx, rx) = channel();
        let p = progress.clone();
        let thread = std::thread::Builder::new().name("interference".into()).spawn(move || {
            let _ = tx.send(interference::check(&bodies, &opts, &p));
        });
        match thread {
            Ok(thread) => {
                t.job = Some(Job { progress, rx, thread: Some(thread), checked: s.asm.clone(), skipped });
                t.note = None;
                self.status = "Checking for interference...".into();
                Ok(())
            }
            Err(e) => {
                let msg = format!("the check could not start: {e}");
                t.note = Some(format!("The check could not start: {e}."));
                Err(msg)
            }
        }
    }

    /// Runs the check to the end on this thread with `opts` and shows the result in the
    /// panel (scripts and MCP, which want the answer back).
    pub(super) fn run_interference(&mut self, opts: &Options) -> Result<Report, String> {
        let Some(s) = &mut self.asm else { return Err("no assembly is open".into()) };
        s.mate = None;
        // The panel shows the options the result came from.
        let o = &mut s.interference_opts;
        o.touching = opts.touching;
        o.include_hidden = opts.include_hidden;
        o.whole_subassemblies = opts.whole_subassemblies;
        o.clearance_on = opts.clearance.is_some();
        if let Some(c) = opts.clearance {
            o.clearance = format!("{c}");
        }
        let t = s.interference.get_or_insert_with(InterferenceTool::default);
        t.cancel();
        let report = s.asm.interference(&mut s.parts, opts, &Progress::new()).map_err(|_| "cancelled")?;
        t.show(report.clone(), s.asm.clone());
        let line = summary(&report, &|c| s.path_label(c));
        self.status = format!("Interference: {line}");
        self.show_assembly_scene();
        Ok(report)
    }

    /// Runs every frame in assembly mode: takes in a finished check.
    pub(super) fn assembly_tick(&mut self, ctx: &egui::Context) {
        let Some(s) = &mut self.asm else { return };
        let names = s.path_names.clone();
        let Some(t) = &mut s.interference else { return };
        let name = |p: &[u32]| super::assembly::label_of(&names, p);
        if let Some(msg) = t.poll(&name) {
            self.status = msg;
            self.show_assembly_scene();
        } else if t.running() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }

    /// Drops results that no longer describe the assembly, after a change to it.
    pub(super) fn interference_after_change(&mut self) {
        let Some(s) = &mut self.asm else { return };
        let Some(t) = &mut s.interference else { return };
        let stale_job = t.job.as_ref().is_some_and(|j| j.checked != s.asm);
        let stale_report = t.checked.as_ref().is_some_and(|c| *c != s.asm);
        if stale_job || stale_report {
            t.cancel();
            t.clear();
            t.note = Some("The assembly changed. Calculate to check it again.".into());
        }
    }

    /// Selects a finding in the list and the view.
    pub(super) fn select_finding(&mut self, k: Option<usize>) {
        if let Some(t) = self.asm.as_mut().and_then(|s| s.interference.as_mut()) {
            t.selected = k.filter(|k| t.finding(*k).is_some());
            self.scene_dirty = true;
        }
    }

    /// The Interference panel, at the top of the left pane.
    pub(super) fn interference_panel(&mut self, ui: &mut egui::Ui) {
        let mut calculate = false;
        let mut cancel = false;
        let mut close = false;
        let mut pick: Option<Option<usize>> = None;
        let Some(s) = &mut self.asm else { return };
        let pal = Palette::of(ui);
        super::panel_title(ui, Icon::Interference, "Interference");
        let running = s.interference.as_ref().is_some_and(InterferenceTool::running);
        theme::section_label(ui, "Options");
        ui.add_enabled_ui(!running, |ui| {
            let o = &mut s.interference_opts;
            ui.checkbox(&mut o.touching, "Report touching components");
            ui.horizontal(|ui| {
                ui.checkbox(&mut o.clearance_on, "Clearance below");
                ui.add_enabled(
                    o.clearance_on,
                    egui::TextEdit::singleline(&mut o.clearance).desired_width(56.0),
                );
                ui.label("mm");
            });
            ui.checkbox(&mut o.include_hidden, "Include hidden components");
            ui.checkbox(&mut o.whole_subassemblies, "Treat subassemblies as one component").on_hover_text(
                "Check each subassembly as a whole, leaving out interference between its own parts",
            );
        });
        ui.add_space(6.0);
        let Some(t) = &s.interference else { return };
        if let Some((done, total)) = t.counts() {
            ui.label(format!("Checking pair {} of {}...", done.min(total), total.max(1)));
            ui.horizontal(|ui| {
                let f = if total > 0 { done as f32 / total as f32 } else { 0.0 };
                ui.add(egui::ProgressBar::new(f).desired_width((ui.available_width() - 92.0).max(40.0)));
                cancel = theme::secondary_button(ui, "Cancel").clicked();
            });
        } else {
            ui.horizontal(|ui| {
                calculate = theme::primary_button(ui, "Calculate").clicked();
                close = theme::secondary_button(ui, "Close").clicked();
            });
        }
        if let Some(n) = &t.note {
            ui.add_space(4.0);
            ui.label(RichText::new(n).color(pal.text_weak));
        }
        if let Some(r) = &t.report {
            theme::section_label(ui, "Results");
            let names = |p: &[u32]| s.path_label(p);
            ui.label(RichText::new(summary(r, &names)).color(pal.text_weak));
            ui.add_space(2.0);
            for (k, f) in r.findings.iter().enumerate() {
                let icon = match f.kind {
                    FindingKind::Overlap { .. } => Icon::Interference,
                    FindingKind::Touching => Icon::Mate,
                    FindingKind::Clearance { .. } => Icon::Measure,
                };
                let look = RowLook {
                    selected: t.selected == Some(k),
                    error: matches!(f.kind, FindingKind::Overlap { .. }),
                    ..Default::default()
                };
                let text = format!("{}, {}: {}", names(&f.a), names(&f.b), amount(f));
                let how = if f.exact { "exact" } else { "approximate: a part is faceted" };
                let tip = match f.kind {
                    FindingKind::Overlap { .. } => format!("They overlap by {} ({how})", amount(f)),
                    FindingKind::Touching => "They touch without overlapping".to_string(),
                    FindingKind::Clearance { .. } => {
                        format!("Their minimum distance is {} ({how})", amount(f))
                    }
                };
                if theme::tree_row(ui, icon, &text, look).on_hover_text(tip).clicked() {
                    pick = Some(Some(k));
                }
            }
        }
        if cancel {
            if let Some(t) = &mut s.interference {
                t.cancel();
                t.note = Some("The check was cancelled. Calculate to check again.".into());
            }
            self.status = "Interference check cancelled".into();
        }
        if let Some(k) = pick {
            self.select_finding(k);
        }
        if calculate {
            if let Err(e) = self.start_interference() {
                self.status = format!("Interference: {e}");
            }
            self.show_assembly_scene();
        }
        if close {
            self.close_interference();
        }
    }

    /// Lines between the closest points of the touching and clearance findings, with the
    /// selected one's distance.
    pub(super) fn draw_interference(&self, painter: &egui::Painter, rect: egui::Rect, pal: &Palette) {
        let Some(t) = self.asm.as_ref().and_then(|s| s.interference.as_ref()) else { return };
        let Some(r) = &t.report else { return };
        for (k, f) in r.findings.iter().enumerate() {
            let Some((p, q)) = f.points else { continue };
            let selected = t.selected == Some(k);
            let color = if selected { pal.error } else { pal.accent };
            let (a, b) = (self.camera.project(p, rect), self.camera.project(q, rect));
            // A ring round the spot, since a touch or a small gap is a dot on screen.
            painter.circle_stroke(
                a + (b - a) * 0.5,
                9.0,
                Stroke::new(if selected { 2.0 } else { 1.5 }, color),
            );
            painter.line_segment([a, b], Stroke::new(if selected { 2.5 } else { 1.5 }, color));
            painter.circle_filled(a, if selected { 3.5 } else { 2.5 }, color);
            painter.circle_filled(b, if selected { 3.5 } else { 2.5 }, color);
            if selected {
                let galley = painter.layout_no_wrap(amount(f), egui::FontId::proportional(13.0), pal.text);
                let at = a + (b - a) * 0.5 + egui::vec2(12.0, -12.0 - galley.size().y);
                let back = egui::Rect::from_min_size(at, galley.size()).expand2(egui::vec2(5.0, 2.0));
                painter.rect(
                    back,
                    4.0,
                    pal.float_fill,
                    Stroke::new(1.0, pal.border),
                    egui::StrokeKind::Inside,
                );
                painter.galley(at, galley, pal.text);
            }
        }
    }

    /// The Interference panel in the harness snapshot.
    pub(super) fn interference_snapshot(&self) -> Option<serde_json::Value> {
        use serde_json::json;
        let s = self.asm.as_ref()?;
        let t = s.interference.as_ref()?;
        Some(json!({
            "running": t.running(),
            "note": t.note,
            "selected": t.selected,
            "options": {
                "touching": s.interference_opts.touching,
                "clearance": s.interference_opts.clearance_on.then(|| s.interference_opts.clearance.clone()),
                "include_hidden": s.interference_opts.include_hidden,
                "whole_subassemblies": s.interference_opts.whole_subassemblies,
            },
            "report": t.report.as_ref().map(|r| report_json(r, &|c| s.path_label(c))),
        }))
    }
}

/// A report as scripts, MCP and the harness see it.
pub(super) fn report_json(r: &Report, names: &dyn Fn(&[u32]) -> String) -> serde_json::Value {
    use serde_json::json;
    let xyz = |v: Vec3| json!([v.x, v.y, v.z]);
    let findings: Vec<_> = r
        .findings
        .iter()
        .map(|f| {
            json!({
                "kind": f.label(),
                "a": names(&f.a),
                "b": names(&f.b),
                "a_id": f.a.first(),
                "b_id": f.b.first(),
                "a_path": f.a,
                "b_path": f.b,
                "volume": f.volume(),
                "distance": f.distance(),
                "points": f.points.map(|(p, q)| json!([xyz(p), xyz(q)])),
                "exact": f.exact,
            })
        })
        .collect();
    let skipped: Vec<_> =
        r.skipped.iter().map(|(c, why)| json!({ "component": names(c), "error": why })).collect();
    json!({ "findings": findings, "pairs": r.pairs, "skipped": skipped, "summary": summary(r, names) })
}
