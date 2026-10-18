//! Main application window.

use crate::camera::{Camera, StdView};
use crate::render::{Renderer, SceneData};
use crate::sketcher::{self, OverDefine, SkSel, SketchSession, Tool};
use crate::theme::{self, Icon, Palette, RowLook};
use anvil_kernel::{EdgeRef, FaceId, Mesh, ModelEdge, Vec2, Vec3};
use anvil_model::hole;
use anvil_model::*;
use eframe::egui::{self, Color32, Key, Modifiers, RichText, Sense, Stroke};
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

mod configs;
use configs::{ConfigUi, Manager};

#[derive(Clone, Copy, Debug, PartialEq)]
enum Selection {
    None,
    Plane(PlaneRef),
    Face(FaceId),
    Feature(u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tab {
    Features,
    Sketch,
    Evaluate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PlanePurpose {
    Sketch,
    RefPlane,
}

enum Editor {
    /// Waiting for the user to pick a plane or planar face.
    PickPlane(PlanePurpose),
    /// Editing a feature live. Cancel restores `before`.
    Feature { id: u32, before: Box<Document>, is_new: bool, bufs: HashMap<&'static str, String> },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PendingAction {
    New,
    Open,
    OpenSample,
    Exit,
}

#[derive(Default)]
struct Windows {
    properties: bool,
    equations: bool,
    mass: bool,
    dfm: bool,
    about: bool,
}

/// Properties of another program's file (a SOLIDWORKS part, say): the file's own, shown
/// read-only, and AnvilCAD's own for it, which are edited here and kept in its sidecar.
struct FilePropsWindow {
    sheet: anvil_io::props::sidecar::Sheet,
    /// What the file is and how to convert its geometry.
    about: String,
    new_name: String,
    unsaved: bool,
    status: Option<String>,
}

const CLOSE_UNSAVED: &str = "Save the AnvilCAD properties, or discard the changes.";

/// The design-for-manufacturing check window's state.
struct DfmState {
    process: dfm::Process,
    report: Option<dfm::DfmReport>,
    /// The part changed since the report was made.
    stale: bool,
    /// The finding whose faces are highlighted.
    focus: Option<usize>,
    /// The material the report was made for.
    material: Option<String>,
}

pub struct AnvilApp {
    doc: Document,
    committed: Document,
    saved: Document,
    undo: Vec<Document>,
    redo: Vec<Document>,
    state: BuildState,
    mesh: Mesh,
    edges: Vec<[Vec3; 2]>,
    scene_dirty: bool,
    renderer: Arc<Mutex<Renderer>>,
    camera: Camera,
    fit_pending: bool,
    path: Option<PathBuf>,
    selection: Selection,
    /// Where the selected face was clicked, so a hole can go there.
    face_click: Option<(FaceId, Vec3)>,
    hover_face: Option<FaceId>,
    dfm: DfmState,
    sketch: Option<SketchSession>,
    editor: Option<Editor>,
    tab: Tab,
    windows: Windows,
    message: Option<(String, String)>,
    file_props: Option<FilePropsWindow>,
    confirm: Option<PendingAction>,
    show_edges: bool,
    /// The dark theme is on.
    dark: bool,
    rename: Option<(u32, String)>,
    status: String,
    last_title: String,
    allow_close: bool,
    dragging: bool,
    new_var: String,
    new_prop: String,
    screenshot: Option<(PathBuf, u32)>,
    view_rect: egui::Rect,
    /// While editing a fillet, chamfer or shell: the body before that feature, which is shown
    /// and picked from.
    pick_body: Option<PickBody>,
    hover_edge: Option<EdgeRef>,
    /// `Some` when file dialogs are answered by a script instead of the native dialog
    /// (the headless harness): each answer is a path, or `None` for Cancel.
    scripted_dialogs: Option<VecDeque<Option<PathBuf>>>,
    /// File dialogs asked for since the harness last looked, with the answer each got.
    dialog_log: Vec<DialogRequest>,
    /// The configuration manager and its windows.
    cfg: ConfigUi,
}

/// A file dialog the app asked for while dialogs were scripted.
#[derive(Clone, Debug)]
pub struct DialogRequest {
    pub title: &'static str,
    pub answer: Option<PathBuf>,
}

struct PickBody {
    solid: anvil_kernel::Solid,
    edges: Vec<ModelEdge>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PickKind {
    Edges,
    Faces,
}

impl AnvilApp {
    pub fn new(
        ctx: &egui::Context,
        open: Option<PathBuf>,
        screenshot: Option<PathBuf>,
        edit_sketch: Option<u32>,
    ) -> AnvilApp {
        theme::apply(ctx, false);

        let doc = Document::new();
        let mut app = AnvilApp {
            committed: doc.clone(),
            saved: doc.clone(),
            doc,
            undo: Vec::new(),
            redo: Vec::new(),
            state: BuildState::default(),
            mesh: Mesh::default(),
            edges: Vec::new(),
            scene_dirty: true,
            renderer: Arc::new(Mutex::new(Renderer::new())),
            camera: Camera::default(),
            fit_pending: true,
            path: None,
            selection: Selection::None,
            face_click: None,
            hover_face: None,
            dfm: DfmState {
                process: dfm::Process::CncMachining,
                report: None,
                stale: false,
                focus: None,
                material: None,
            },
            sketch: None,
            editor: None,
            tab: Tab::Features,
            windows: Windows::default(),
            message: None,
            file_props: None,
            confirm: None,
            show_edges: true,
            dark: false,
            rename: None,
            status: "Ready".into(),
            last_title: String::new(),
            allow_close: false,
            dragging: false,
            new_var: String::new(),
            new_prop: String::new(),
            screenshot: screenshot.map(|p| (p, 0)),
            view_rect: egui::Rect::NOTHING,
            pick_body: None,
            hover_edge: None,
            scripted_dialogs: None,
            dialog_log: Vec::new(),
            cfg: ConfigUi::default(),
        };
        if let Some(p) = open {
            if p.as_os_str() == "--sample" {
                app.load_document(crate::sample::sample_part(), None);
            } else {
                app.open_path(&p);
            }
        }
        app.rebuild();
        if let Some(id) = edit_sketch {
            app.edit_sketch(id);
            app.fit_pending = true;
        }
        app
    }

    // ----- document state -------------------------------------------------------------

    /// Opens a window by name (for the command line): properties, mass or dfm.
    pub fn show_window(&mut self, name: &str) {
        match name {
            "properties" => self.windows.properties = true,
            "mass" => self.windows.mass = true,
            "dfm" => self.open_dfm(),
            _ => {}
        }
    }

    /// Assigns a library material by name (for the command line).
    pub fn set_material(&mut self, name: &str) {
        if let Some(m) = Material::library().into_iter().find(|m| m.name == name) {
            self.doc.material = Some(m);
        }
    }

    fn open_dfm(&mut self) {
        self.windows.dfm = true;
        if self.dfm.report.is_none() || self.dfm.stale {
            self.run_dfm();
        }
    }

    fn run_dfm(&mut self) {
        let material = self.doc.material.as_ref().map(|m| m.resolved());
        self.dfm.report = Some(dfm::check(&self.state.body, material.as_ref(), self.dfm.process));
        self.dfm.material = material.map(|m| m.name);
        self.dfm.stale = false;
        self.dfm.focus = None;
        self.scene_dirty = true;
    }

    fn rebuild(&mut self) {
        // Edits to configured parameters belong to the active configuration.
        self.doc.capture_configuration();
        self.state = self.doc.rebuild();
        if self.dfm.report.is_some() {
            self.dfm.stale = true;
        }
        if self.pick_target().is_none() {
            self.pick_body = None;
        }
        let shown = self.pick_body.as_ref().map(|p| &p.solid).unwrap_or(&self.state.body);
        self.mesh = shown.tessellate();
        self.edges = shown.feature_edges();
        self.scene_dirty = true;
        if let Some(s) = &mut self.sketch {
            if let Some(st) = self.state.sketch_status.get(&s.feature) {
                s.status = st.clone();
            }
        }
        let errs = self.state.errors.len();
        self.status =
            if errs == 0 { "Rebuild OK".into() } else { format!("Rebuild finished with {errs} error(s)") };
    }

    fn load_document(&mut self, doc: Document, path: Option<PathBuf>) {
        self.doc = doc;
        self.committed = self.doc.clone();
        self.saved = if path.is_some() { self.doc.clone() } else { Document::new() };
        self.undo.clear();
        self.redo.clear();
        self.path = path;
        self.sketch = None;
        self.editor = None;
        self.selection = Selection::None;
        self.fit_pending = true;
        self.cfg.reset();
        self.rebuild();
    }

    fn is_dirty(&self) -> bool {
        self.doc != self.saved
    }

    fn commit_if_changed(&mut self) {
        let editing_feature = matches!(self.editor, Some(Editor::Feature { .. }));
        if !self.dragging && !editing_feature && self.doc != self.committed {
            self.undo.push(std::mem::replace(&mut self.committed, self.doc.clone()));
            if self.undo.len() > 200 {
                self.undo.remove(0);
            }
            self.redo.clear();
        }
    }

    fn undo(&mut self) {
        if let Some(prev) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.doc, prev));
            self.after_history_jump();
        }
    }

    fn redo(&mut self) {
        if let Some(next) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.doc, next));
            self.after_history_jump();
        }
    }

    fn after_history_jump(&mut self) {
        self.committed = self.doc.clone();
        self.editor = None;
        self.cfg.props = None;
        if let Some(s) = &mut self.sketch {
            s.selection.clear();
            s.dim_edit = None;
            s.cancel_pending();
            if self.doc.sketch(s.feature).is_none() {
                self.sketch = None;
            }
        }
        self.rebuild();
    }

    fn title(&self) -> String {
        let name = self
            .path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "Part1".into());
        format!("{}{} - AnvilCAD", name, if self.is_dirty() { " *" } else { "" })
    }

    fn part_name(&self) -> String {
        self.path
            .as_ref()
            .and_then(|p| p.file_stem())
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "Part1".into())
    }

    // ----- files -------------------------------------------------------------------------

    /// Asks the user for a file. While dialogs are scripted the next queued answer is used
    /// (Cancel when the queue is empty), so a headless run never opens a native window.
    fn file_dialog(
        &mut self,
        title: &'static str,
        native: impl FnOnce() -> Option<PathBuf>,
    ) -> Option<PathBuf> {
        let Some(queue) = &mut self.scripted_dialogs else { return native() };
        let answer = queue.pop_front().flatten();
        self.dialog_log.push(DialogRequest { title, answer: answer.clone() });
        answer
    }

    fn request(&mut self, a: PendingAction) {
        if self.is_dirty() {
            self.confirm = Some(a);
        } else {
            self.perform(a);
        }
    }

    fn perform(&mut self, a: PendingAction) {
        match a {
            PendingAction::New => self.load_document(Document::new(), None),
            PendingAction::OpenSample => self.load_document(crate::sample::sample_part(), None),
            PendingAction::Open => {
                let picked = self.file_dialog("Open", || {
                    rfd::FileDialog::new()
                        .add_filter(
                            "AnvilCAD, STEP and SOLIDWORKS files",
                            &[
                                "anvprt",
                                "anvprtdot",
                                "fprt",
                                "step",
                                "stp",
                                "sldprt",
                                "sldasm",
                                "slddrw",
                                "prtdot",
                            ],
                        )
                        .add_filter("AnvilCAD part", &["anvprt"])
                        .add_filter("STEP", &["step", "stp"])
                        .add_filter("AnvilCAD part template", &["anvprtdot"])
                        .add_filter(
                            "SOLIDWORKS",
                            &["sldprt", "sldasm", "slddrw", "prtdot", "asmdot", "drwdot"],
                        )
                        .pick_file()
                });
                if let Some(p) = picked {
                    self.open_path(&p);
                }
            }
            PendingAction::Exit => {
                self.allow_close = true;
            }
        }
    }

    fn open_path(&mut self, p: &Path) {
        match anvil_io::solidworks::identify(p) {
            Ok(Some(info)) => {
                use anvil_io::props::{sidecar, ReadOptions};
                let sheet =
                    sidecar::Sheet::open(p, &ReadOptions::from_env(), sidecar::store_from_env().as_deref());
                self.file_props = Some(FilePropsWindow {
                    sheet,
                    about: info.explanation(),
                    new_name: String::new(),
                    unsaved: false,
                    status: None,
                });
                return;
            }
            Err(e) => {
                self.message = Some(("Open failed".into(), e.to_string()));
                return;
            }
            Ok(None) => {}
        }
        let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
        if ext == "step" || ext == "stp" {
            self.import_step(p);
            return;
        }
        let is_template = p.extension().map(|e| e.eq_ignore_ascii_case(PART_TEMPLATE_EXT)).unwrap_or(false);
        match Document::load(p) {
            Ok(d) if is_template => self.load_document(Document::from_template(&d), None),
            Ok(d) => self.load_document(d, Some(p.to_path_buf())),
            Err(e) => self.message = Some(("Open failed".into(), format!("{}: {e}", p.display()))),
        }
    }

    /// Opens a STEP file as a new, unsaved part made of imported bodies.
    fn import_step(&mut self, p: &Path) {
        let text = match std::fs::read(p) {
            Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
            Err(e) => {
                self.message = Some(("Import failed".into(), format!("{}: {e}", p.display())));
                return;
            }
        };
        let source = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        match anvil_model::step::document_from_step(&text, &source) {
            Ok((doc, report)) => {
                self.load_document(doc, None);
                self.message = Some(("STEP import".into(), report.summary()));
            }
            Err(e) => self.message = Some(("Import failed".into(), format!("{}: {e}", p.display()))),
        }
    }

    fn save(&mut self, save_as: bool) {
        let path = match (&self.path, save_as) {
            (Some(p), false) => Some(p.clone()),
            _ => {
                let name = format!("{}.{PART_EXT}", self.part_name());
                self.file_dialog("Save As", || {
                    rfd::FileDialog::new()
                        .add_filter("AnvilCAD part", &[PART_EXT])
                        .set_file_name(name)
                        .save_file()
                })
            }
        };
        let Some(path) = path else { return };
        match self.doc.save(&path) {
            Ok(()) => {
                self.path = Some(path.clone());
                self.saved = self.doc.clone();
                self.status = format!("Saved {}", path.display());
            }
            Err(e) => self.message = Some(("Save failed".into(), e.to_string())),
        }
    }

    fn save_template(&mut self) {
        if let Some(path) = self.file_dialog("Save Template", || {
            rfd::FileDialog::new()
                .add_filter("AnvilCAD part template", &[PART_TEMPLATE_EXT])
                .set_file_name(format!("Template.{PART_TEMPLATE_EXT}"))
                .save_file()
        }) {
            match self.doc.save(&path) {
                Ok(()) => self.status = format!("Template saved to {}", path.display()),
                Err(e) => self.message = Some(("Save failed".into(), e.to_string())),
            }
        }
    }

    fn export(&mut self) {
        if self.state.body.is_empty() {
            self.message = Some(("Export".into(), "There is no solid body to export yet.".into()));
            return;
        }
        let name = format!("{}.step", self.part_name());
        if let Some(path) = self.file_dialog("Export", || {
            rfd::FileDialog::new()
                .add_filter("STEP AP214", &["step", "stp"])
                .add_filter("STL", &["stl"])
                .add_filter("Wavefront OBJ", &["obj"])
                .set_file_name(name)
                .save_file()
        }) {
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
            // Exact geometry when the whole part was built exactly, facets otherwise.
            let exact = (ext == "step" || ext == "stp")
                .then(|| anvil_model::step::exact_step(&self.state, &self.part_name()))
                .flatten();
            let result = match exact {
                Some(text) => std::fs::write(&path, text).map(|()| " (exact geometry)"),
                None => {
                    anvil_io::export_by_extension(&self.state.body, &path, &self.part_name()).map(|()| "")
                }
            };
            match result {
                Ok(how) => self.status = format!("Exported {}{how}", path.display()),
                Err(e) => self.message = Some(("Export failed".into(), e.to_string())),
            }
        }
    }

    // ----- commands -------------------------------------------------------------------

    fn start_sketch(&mut self, plane: PlaneRef) {
        let frame = match plane.standard_frame() {
            Some(f) => Some(f),
            None => match plane {
                PlaneRef::Face(f) => self.state.body.planar_face_frame(f),
                PlaneRef::Feature(id) => self.state.frames.get(&id).copied(),
                _ => None,
            },
        };
        let Some(frame) = frame else {
            self.message = Some(("Sketch".into(), "Select a plane or a planar face.".into()));
            return;
        };
        let id = self.doc.add_feature(FeatureKind::Sketch(SketchFeature::new(plane)));
        self.editor = None;
        self.enter_sketch(id, frame);
    }

    fn enter_sketch(&mut self, id: u32, frame: anvil_kernel::Frame) {
        let mut s = SketchSession::new(id, frame);
        if let Some(st) = self.state.sketch_status.get(&id) {
            s.status = st.clone();
        }
        self.camera.normal_to(&frame);
        self.camera.target = frame.origin;
        self.sketch = Some(s);
        self.tab = Tab::Sketch;
        self.selection = Selection::Feature(id);
    }

    fn edit_sketch(&mut self, id: u32) {
        // Make sure the sketch's plane is current before editing.
        self.rebuild();
        let frame = self
            .state
            .frames
            .get(&id)
            .copied()
            .or_else(|| self.doc.sketch(id).and_then(|s| s.plane.standard_frame()));
        match frame {
            Some(f) => self.enter_sketch(id, f),
            None => {
                self.message = Some((
                    "Edit Sketch".into(),
                    "This sketch's plane could not be resolved. Fix the features it depends on first.".into(),
                ))
            }
        }
    }

    fn exit_sketch(&mut self) {
        if let Some(s) = self.sketch.take() {
            // An empty new sketch is just removed.
            if let Some(sf) = self.doc.sketch(s.feature) {
                if sf.sketch.entities.is_empty() && self.doc.dependents(s.feature).is_empty() {
                    self.doc.delete_feature(s.feature);
                } else {
                    self.selection = Selection::Feature(s.feature);
                }
            }
        }
        self.tab = Tab::Features;
        self.rebuild();
    }

    /// The sketch a solid feature should use: the open sketch, or the selected one.
    fn target_sketch(&self) -> Option<u32> {
        if let Some(s) = &self.sketch {
            return Some(s.feature);
        }
        match self.selection {
            Selection::Feature(id) if self.doc.sketch(id).is_some() => Some(id),
            _ => None,
        }
    }

    fn begin_feature(&mut self, make: impl FnOnce(&Document, u32) -> Option<FeatureKind>) {
        let Some(sk) = self.target_sketch() else {
            self.message = Some((
                "Select a sketch".into(),
                "Select a sketch in the feature tree (or open one), then run the command.".into(),
            ));
            return;
        };
        if self.sketch.is_some() {
            self.exit_sketch();
        }
        let before = self.doc.clone();
        let Some(kind) = make(&self.doc, sk) else { return };
        let id = self.doc.add_feature(kind);
        self.open_editor(id, before, true);
    }

    fn open_editor(&mut self, id: u32, before: Document, is_new: bool) {
        let mut bufs = HashMap::new();
        if let Some(f) = self.doc.feature(id) {
            match &f.kind {
                FeatureKind::Extrude(e) => {
                    bufs.insert("depth", e.depth.text());
                }
                FeatureKind::Revolve(r) => {
                    bufs.insert("angle", r.angle.text());
                }
                FeatureKind::Plane(p) => {
                    bufs.insert("offset", p.offset.text());
                }
                FeatureKind::LinearPattern(p) => {
                    bufs.insert("spacing", p.spacing.text());
                    bufs.insert("count", p.count.text());
                }
                FeatureKind::CircularPattern(p) => {
                    bufs.insert("angle", p.total_angle.text());
                    bufs.insert("count", p.count.text());
                }
                FeatureKind::Sketch(_) => {
                    self.edit_sketch(id);
                    return;
                }
                FeatureKind::ImportedBody(_) => {}
                FeatureKind::Fillet(f) => {
                    bufs.insert("radius", f.radius.text());
                    match &f.profile {
                        FilletProfile::ConicRho { rho } => {
                            bufs.insert("rho", rho.text());
                        }
                        FilletProfile::ConicRadius { radius } => {
                            bufs.insert("conic", radius.text());
                        }
                        _ => {}
                    }
                }
                FeatureKind::Chamfer(c) => {
                    bufs.insert("distance", c.distance.text());
                    if let ChamferKind::AngleDistance { angle, .. } = &c.kind {
                        bufs.insert("angle", angle.text());
                    }
                }
                FeatureKind::Shell(s) => {
                    bufs.insert("thickness", s.thickness.text());
                }
                FeatureKind::Hole(h) => bufs = hole_bufs(h),
                _ => {}
            }
        }
        self.editor = Some(Editor::Feature { id, before: Box::new(before), is_new, bufs });
        self.selection = Selection::Feature(id);
        if self.pick_target().is_some() {
            self.compute_pick_body(id);
        }
        self.rebuild();
    }

    fn cmd_extrude(&mut self, op: BodyOp) {
        self.begin_feature(|_, sk| {
            Some(FeatureKind::Extrude(ExtrudeFeature {
                sketch: sk,
                end: if op == BodyOp::Cut { EndCondition::ThroughAll } else { EndCondition::Blind },
                depth: Dim::Value(10.0),
                reverse: false,
                op,
                ..Default::default()
            }))
        });
    }

    fn cmd_revolve(&mut self, op: BodyOp) {
        let mut err = None;
        self.begin_feature(|doc, sk| {
            let sketch = &doc.sketch(sk)?.sketch;
            let lines: Vec<usize> =
                (0..sketch.entities.len()).filter(|&e| sketch.entities[e].is_line()).collect();
            let axis =
                lines.iter().copied().find(|&e| sketch.entities[e].construction).or(lines.first().copied());
            match axis {
                Some(axis) => Some(FeatureKind::Revolve(RevolveFeature {
                    sketch: sk,
                    axis,
                    angle: Dim::Value(360.0),
                    op,
                    ..Default::default()
                })),
                None => {
                    err = Some("A revolve needs a line in the sketch to use as its axis. Add a centerline.");
                    None
                }
            }
        });
        if let Some(e) = err {
            self.message = Some(("Revolve".into(), e.into()));
        }
    }

    fn cmd_pattern(&mut self, circular: bool) {
        let seed = match self.selection {
            Selection::Feature(id) => match self.doc.feature(id).map(|f| &f.kind) {
                Some(FeatureKind::Extrude(_) | FeatureKind::Revolve(_) | FeatureKind::Hole(_)) => Some(id),
                _ => None,
            },
            _ => None,
        };
        let Some(seed) = seed else {
            self.message = Some((
                "Pattern".into(),
                "Select an extrude, revolve or hole feature in the tree to pattern.".into(),
            ));
            return;
        };
        let before = self.doc.clone();
        let kind = if circular {
            FeatureKind::CircularPattern(CircularPatternFeature {
                features: vec![seed],
                axis: Axis::Y,
                total_angle: Dim::Value(360.0),
                count: Dim::Value(4.0),
                ..Default::default()
            })
        } else {
            FeatureKind::LinearPattern(LinearPatternFeature {
                features: vec![seed],
                direction: Axis::X,
                spacing: Dim::Value(20.0),
                count: Dim::Value(3.0),
                ..Default::default()
            })
        };
        let id = self.doc.add_feature(kind);
        self.open_editor(id, before, true);
    }

    fn selected_plane(&self) -> Option<PlaneRef> {
        match self.selection {
            Selection::Plane(p) => Some(p),
            Selection::Face(f) if self.state.body.planar_face_frame(f).is_some() => Some(PlaneRef::Face(f)),
            Selection::Feature(id) => match self.doc.feature(id)?.kind {
                FeatureKind::Plane(_) => Some(PlaneRef::Feature(id)),
                _ => None,
            },
            _ => None,
        }
    }

    fn cmd_sketch(&mut self) {
        if let Some(p) = self.selected_plane() {
            self.start_sketch(p);
        } else {
            self.editor = Some(Editor::PickPlane(PlanePurpose::Sketch));
        }
    }

    /// The feature being edited when it picks edges or faces from the model.
    fn pick_target(&self) -> Option<(u32, PickKind)> {
        let Some(Editor::Feature { id, .. }) = &self.editor else { return None };
        match self.doc.feature(*id)?.kind {
            FeatureKind::Fillet(_) | FeatureKind::Chamfer(_) => Some((*id, PickKind::Edges)),
            FeatureKind::Shell(_) => Some((*id, PickKind::Faces)),
            _ => None,
        }
    }

    /// Rebuilds the model as it stands before `id`, for picking.
    fn compute_pick_body(&mut self, id: u32) {
        let mut d = self.doc.clone();
        d.rollback = d.index_of(id);
        let solid = d.rebuild().body;
        self.pick_body = Some(PickBody { edges: solid.model_edges(), solid });
    }

    fn cmd_edge_treatment(&mut self, chamfer: bool) {
        if self.state.body.is_empty() {
            self.message = Some(("Fillet".into(), "There is no body to fillet yet.".into()));
            return;
        }
        let before = self.doc.clone();
        let kind = if chamfer {
            FeatureKind::Chamfer(ChamferFeature {
                edges: Vec::new(),
                distance: Dim::Value(1.0),
                kind: ChamferKind::EqualDistance,
                tangent_propagation: true,
            })
        } else {
            FeatureKind::Fillet(FilletFeature {
                edges: Vec::new(),
                radius: Dim::Value(2.0),
                profile: FilletProfile::Circular,
                tangent_propagation: true,
            })
        };
        let id = self.doc.add_feature(kind);
        self.open_editor(id, before, true);
    }

    fn cmd_shell(&mut self) {
        if self.state.body.is_empty() {
            self.message = Some(("Shell".into(), "There is no body to shell yet.".into()));
            return;
        }
        let remove = match self.selection {
            Selection::Face(f) => vec![f],
            _ => Vec::new(),
        };
        let before = self.doc.clone();
        let id =
            self.doc.add_feature(FeatureKind::Shell(ShellFeature { remove, thickness: Dim::Value(1.0) }));
        self.open_editor(id, before, true);
    }

    /// Holes go at the points of a sketch: the selected (or open) sketch, or a new sketch
    /// on the selected planar face with one point at its middle.
    fn cmd_hole(&mut self) {
        let title = "Hole";
        if let Some(sk) = self.target_sketch() {
            let has_points = self.doc.sketch(sk).is_some_and(|s| !hole::positions(&s.sketch).is_empty());
            if !has_points {
                self.message = Some((
                    title.into(),
                    "Holes go at the sketch's points. Add points with the Point tool, then click Hole."
                        .into(),
                ));
                return;
            }
            self.begin_feature(|_, sk| Some(FeatureKind::Hole(default_hole(sk))));
            return;
        }
        let face = match self.selection {
            Selection::Face(f) => self.state.body.planar_face_frame(f).map(|fr| (f, fr)),
            _ => None,
        };
        let Some((face, frame)) = face else {
            self.message = Some((
                title.into(),
                "Select a planar face, or a sketch with points, then click Hole.".into(),
            ));
            return;
        };
        let before = self.doc.clone();
        let mut sf = SketchFeature::new(PlaneRef::Face(face));
        let at = match self.face_click {
            Some((f, p)) if f == face => p,
            _ => face_centroid(&self.state.body, face),
        };
        sf.sketch.add_point(frame.to_local(at));
        let sk = self.doc.add_feature(FeatureKind::Sketch(sf));
        let id = self.doc.add_feature(FeatureKind::Hole(default_hole(sk)));
        self.open_editor(id, before, true);
    }

    fn cmd_plane(&mut self) {
        if let Some(p) = self.selected_plane() {
            self.create_ref_plane(p);
        } else {
            self.editor = Some(Editor::PickPlane(PlanePurpose::RefPlane));
        }
    }

    fn create_ref_plane(&mut self, base: PlaneRef) {
        let before = self.doc.clone();
        let id = self.doc.add_feature(FeatureKind::Plane(PlaneFeature {
            base,
            offset: Dim::Value(10.0),
            flip: false,
        }));
        self.open_editor(id, before, true);
    }

    fn plane_picked(&mut self, p: PlaneRef) {
        match self.editor {
            Some(Editor::PickPlane(PlanePurpose::Sketch)) => self.start_sketch(p),
            Some(Editor::PickPlane(PlanePurpose::RefPlane)) => self.create_ref_plane(p),
            _ => {}
        }
    }

    fn delete_selected_feature(&mut self) {
        if let Selection::Feature(id) = self.selection {
            let gone = self.doc.delete_feature(id);
            self.status = format!("Deleted {} feature(s)", gone.len());
            self.selection = Selection::None;
            self.rebuild();
        }
    }

    // ----- UI: top bars ---------------------------------------------------------------

    /// The header: AnvilCAD's mark, the menus, the part's name, and undo, redo and rebuild.
    fn menu_bar(&mut self, ui: &mut egui::Ui) {
        let pal = Palette::of(ui);
        egui::menu::bar(ui, |ui| {
            let (mark, _) = ui.allocate_exact_size(egui::vec2(30.0, 26.0), Sense::hover());
            theme::paint_mark(ui.painter(), mark.shrink2(egui::vec2(3.0, 5.0)), pal.text, pal.accent);
            ui.add_space(4.0);
            ui.menu_button("File", |ui| {
                if ui.button("New            Ctrl+N").clicked() {
                    ui.close_menu();
                    self.request(PendingAction::New);
                }
                if ui.button("Open...        Ctrl+O").clicked() {
                    ui.close_menu();
                    self.request(PendingAction::Open);
                }
                ui.separator();
                if ui.button("Save           Ctrl+S").clicked() {
                    ui.close_menu();
                    self.save(false);
                }
                if ui.button("Save As...").clicked() {
                    ui.close_menu();
                    self.save(true);
                }
                if ui.button("Save as Part Template...").clicked() {
                    ui.close_menu();
                    self.save_template();
                }
                if ui.button("Export (STEP / STL / OBJ)...").clicked() {
                    ui.close_menu();
                    self.export();
                }
                ui.separator();
                if ui.button("Properties...").clicked() {
                    ui.close_menu();
                    self.windows.properties = true;
                }
                ui.separator();
                if ui.button("Exit").clicked() {
                    ui.close_menu();
                    self.request(PendingAction::Exit);
                }
            });
            ui.menu_button("Edit", |ui| {
                if ui.add_enabled(!self.undo.is_empty(), egui::Button::new("Undo   Ctrl+Z")).clicked() {
                    ui.close_menu();
                    self.undo();
                }
                if ui.add_enabled(!self.redo.is_empty(), egui::Button::new("Redo   Ctrl+Y")).clicked() {
                    ui.close_menu();
                    self.redo();
                }
                ui.separator();
                if ui.button("Rebuild   Ctrl+B").clicked() {
                    ui.close_menu();
                    self.rebuild();
                }
            });
            ui.menu_button("View", |ui| {
                for (i, v) in StdView::ALL.iter().enumerate() {
                    if ui.button(format!("{:<12} Ctrl+{}", v.label(), i + 1)).clicked() {
                        ui.close_menu();
                        self.camera.set_view(*v);
                        self.fit_pending = true;
                    }
                }
                if ui.button("Zoom to Fit    F").clicked() {
                    ui.close_menu();
                    self.fit_pending = true;
                }
                ui.checkbox(&mut self.show_edges, "Shaded with edges");
                ui.separator();
                if ui.checkbox(&mut self.dark, "Dark theme").changed() {
                    theme::apply(ui.ctx(), self.dark);
                }
            });
            ui.menu_button("Tools", |ui| {
                if ui.button("Equations...").clicked() {
                    ui.close_menu();
                    self.windows.equations = true;
                }
                if ui.button("Mass Properties...").clicked() {
                    ui.close_menu();
                    self.windows.mass = true;
                }
                if ui.button("Check Design (DFM)...").clicked() {
                    ui.close_menu();
                    self.open_dfm();
                }
            });
            ui.menu_button("Help", |ui| {
                if ui.button("Open Sample Part").clicked() {
                    ui.close_menu();
                    self.request(PendingAction::OpenSample);
                }
                if ui.button("About AnvilCAD").clicked() {
                    ui.close_menu();
                    self.windows.about = true;
                }
            });

            // The part's name, centred in the header and painted (not a widget) so it does
            // not clash with the tree's top row in the harness.
            let bar = ui.max_rect();
            let dirty = self.is_dirty();
            let name = self.part_name();
            let galley = ui.painter().layout_no_wrap(name, egui::FontId::proportional(13.0), pal.text);
            let pos =
                egui::pos2(bar.center().x - galley.size().x / 2.0, bar.center().y - galley.size().y / 2.0);
            let end = pos.x + galley.size().x;
            if pos.x > ui.cursor().left() + 20.0 {
                ui.painter().galley(pos, galley, pal.text);
                if dirty {
                    ui.painter().circle_filled(egui::pos2(end + 8.0, bar.center().y), 3.0, pal.accent);
                }
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                if theme::icon_button(ui, Icon::Rebuild, "Rebuild", "Rebuild (Ctrl+B)", None).clicked() {
                    self.rebuild();
                }
                let r = ui.add_enabled_ui(!self.redo.is_empty(), |ui| {
                    theme::icon_button(ui, Icon::Redo, "Redo", "Redo (Ctrl+Y)", None)
                });
                if r.inner.clicked() {
                    self.redo();
                }
                let r = ui.add_enabled_ui(!self.undo.is_empty(), |ui| {
                    theme::icon_button(ui, Icon::Undo, "Undo", "Undo (Ctrl+Z)", None)
                });
                if r.inner.clicked() {
                    self.undo();
                }
            });
        });
    }

    /// The command bar: tabs, then the open tab's commands in captioned groups.
    fn command_manager(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            for (t, label) in
                [(Tab::Features, "Features"), (Tab::Sketch, "Sketch"), (Tab::Evaluate, "Evaluate")]
            {
                if theme::tab(ui, label, self.tab == t).on_hover_text("Ctrl+Page Up / Page Down").clicked() {
                    self.tab = t;
                }
            }
        });
        let pal = Palette::of(ui);
        let y = ui.cursor().top();
        ui.painter().hline(ui.max_rect().x_range(), y, Stroke::new(1.0, pal.border));
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.set_min_height(62.0);
            match self.tab {
                Tab::Features => {
                    theme::command_group(ui, "Body", |ui| {
                        if theme::command_button(ui, Icon::Extrude, "Extruded Boss/Base", None).clicked() {
                            self.cmd_extrude(BodyOp::Boss);
                        }
                        if theme::command_button(ui, Icon::Revolve, "Revolved Boss/Base", None).clicked() {
                            self.cmd_revolve(BodyOp::Boss);
                        }
                        if theme::command_button(ui, Icon::ExtrudeCut, "Extruded Cut", None).clicked() {
                            self.cmd_extrude(BodyOp::Cut);
                        }
                        if theme::command_button(ui, Icon::RevolveCut, "Revolved Cut", None).clicked() {
                            self.cmd_revolve(BodyOp::Cut);
                        }
                    });
                    theme::group_rule(ui);
                    theme::command_group(ui, "Pattern", |ui| {
                        if theme::command_button(ui, Icon::LinearPattern, "Linear Pattern", None).clicked() {
                            self.cmd_pattern(false);
                        }
                        if theme::command_button(ui, Icon::CircularPattern, "Circular Pattern", None)
                            .clicked()
                        {
                            self.cmd_pattern(true);
                        }
                    });
                    theme::group_rule(ui);
                    theme::command_group(ui, "Modify", |ui| {
                        if theme::command_button(ui, Icon::Fillet, "Fillet", None).clicked() {
                            self.cmd_edge_treatment(false);
                        }
                        if theme::command_button(ui, Icon::Chamfer, "Chamfer", None).clicked() {
                            self.cmd_edge_treatment(true);
                        }
                        if theme::command_button(ui, Icon::Shell, "Shell", None).clicked() {
                            self.cmd_shell();
                        }
                        if theme::command_button(ui, Icon::Hole, "Hole", None).clicked() {
                            self.cmd_hole();
                        }
                    });
                    theme::group_rule(ui);
                    theme::command_group(ui, "Reference", |ui| {
                        if theme::command_button(ui, Icon::Plane, "Reference Plane", None).clicked() {
                            self.cmd_plane();
                        }
                    });
                }
                Tab::Sketch => {
                    if self.sketch.is_none() {
                        theme::command_group(ui, "Start", |ui| {
                            if theme::command_button(ui, Icon::Sketch, "Sketch", None).clicked() {
                                self.cmd_sketch();
                            }
                        });
                        ui.add_space(8.0);
                        ui.weak("Select a plane or planar face, then click Sketch.");
                    } else {
                        let s = self.sketch.as_mut().unwrap();
                        theme::command_group(ui, "Draw", |ui| {
                            for t in SKETCH_TOOLS {
                                if theme::command_button(ui, tool_icon(t), t.label(), Some(s.tool == t))
                                    .clicked()
                                {
                                    s.set_tool(t);
                                }
                            }
                        });
                        theme::group_rule(ui);
                        let mut exit = false;
                        theme::command_group(ui, "Finish", |ui| {
                            exit = theme::command_button(ui, Icon::ExitSketch, "Exit Sketch", None).clicked();
                        });
                        if exit {
                            self.exit_sketch();
                        }
                    }
                }
                Tab::Evaluate => {
                    theme::command_group(ui, "Analyze", |ui| {
                        if theme::command_button(ui, Icon::Mass, "Mass Properties", None).clicked() {
                            self.windows.mass = true;
                        }
                        if theme::command_button(ui, Icon::Check, "Check Design (DFM)", None).clicked() {
                            self.open_dfm();
                        }
                    });
                    theme::group_rule(ui);
                    theme::command_group(ui, "Data", |ui| {
                        if theme::command_button(ui, Icon::Equations, "Equations", None).clicked() {
                            self.windows.equations = true;
                        }
                        if theme::command_button(ui, Icon::Properties, "Custom Properties", None).clicked() {
                            self.windows.properties = true;
                        }
                        if theme::command_button(ui, Icon::Configuration, "Configuration Table", None)
                            .clicked()
                        {
                            self.cfg.table = true;
                        }
                    });
                }
            }
        });
    }

    // ----- UI: left panel ---------------------------------------------------------------

    fn feature_tree(&mut self, ui: &mut egui::Ui) {
        ui.spacing_mut().item_spacing.y = 1.0;
        ui.horizontal(|ui| {
            let pal = Palette::of(ui);
            let (r, _) = ui.allocate_exact_size(egui::vec2(20.0, 20.0), Sense::hover());
            theme::paint_icon(ui.painter(), r, Icon::Part, pal.text, pal.accent);
            ui.label(RichText::new(self.part_name()).size(15.0).strong());
        });
        if self.doc.configurations.len() > 1 {
            let pal = Palette::of(ui);
            ui.label(
                RichText::new(format!("Configuration: {}", self.doc.active_configuration_name()))
                    .size(12.0)
                    .color(pal.text_weak),
            );
        }
        ui.add_space(6.0);
        let mat = self
            .doc
            .material
            .as_ref()
            .map(|m| m.name.clone())
            .unwrap_or_else(|| "Material <not specified>".into());
        if theme::tree_row(ui, Icon::Material, &mat, RowLook::default())
            .on_hover_text("Material: click to change")
            .clicked()
        {
            self.windows.properties = true;
        }
        for p in [PlaneRef::Front, PlaneRef::Top, PlaneRef::Right] {
            let sel = self.selection == Selection::Plane(p);
            let r =
                theme::tree_row(ui, Icon::Plane, &p.label(), RowLook { selected: sel, ..Default::default() });
            if r.clicked() {
                self.selection = Selection::Plane(p);
                if matches!(self.editor, Some(Editor::PickPlane(_))) {
                    self.plane_picked(p);
                }
            }
            r.context_menu(|ui| {
                if ui.button("Sketch").clicked() {
                    ui.close_menu();
                    self.start_sketch(p);
                }
                if ui.button("Normal To").clicked() {
                    ui.close_menu();
                    if let Some(f) = p.standard_frame() {
                        self.camera.normal_to(&f);
                    }
                }
            });
        }
        theme::tree_row(ui, Icon::Origin, "Origin", RowLook::default());
        ui.add_space(4.0);
        theme::section_label(ui, "History");

        let rollback = self.doc.rollback.unwrap_or(usize::MAX);
        let mut action: Option<(u32, &'static str)> = None;
        let features: Vec<(usize, u32, String, bool, Icon)> = self
            .doc
            .features
            .iter()
            .enumerate()
            .map(|(i, f)| (i, f.id, f.name.clone(), f.suppressed, icon(&f.kind)))
            .collect();
        for (i, id, name, suppressed, ic) in features {
            if i == rollback {
                rollback_bar(ui);
            }
            let rolled = i >= rollback;
            let err = self.state.errors.get(&id).cloned();
            if let Some((rid, buf)) = &mut self.rename {
                if *rid == id {
                    let r = ui.text_edit_singleline(buf);
                    if r.lost_focus() {
                        let (rid, buf) = self.rename.take().unwrap();
                        if !buf.trim().is_empty() {
                            if let Some(f) = self.doc.feature_mut(rid) {
                                f.name = buf.trim().to_string();
                            }
                        }
                    } else {
                        r.request_focus();
                    }
                    continue;
                }
            }
            let look = RowLook {
                selected: self.selection == Selection::Feature(id),
                dimmed: suppressed || rolled,
                error: err.is_some(),
                indent: 0,
            };
            let r = theme::tree_row(ui, ic, &name, look);
            let r = match &err {
                Some(e) => r.on_hover_text(e),
                None => r,
            };
            // Migration and provenance notes.
            let r = match self.doc.feature(id).and_then(|f| f.note.clone()) {
                Some(n) => r.on_hover_text(n),
                None => r,
            };
            if r.clicked() {
                self.selection = Selection::Feature(id);
                if matches!(self.editor, Some(Editor::PickPlane(_))) {
                    if let Some(FeatureKind::Plane(_)) = self.doc.feature(id).map(|f| &f.kind) {
                        self.plane_picked(PlaneRef::Feature(id));
                    }
                }
            }
            if r.double_clicked() {
                action = Some((id, "edit"));
            }
            r.context_menu(|ui| {
                let is_sketch = self.doc.sketch(id).is_some();
                if ui.button(if is_sketch { "Edit Sketch" } else { "Edit Feature" }).clicked() {
                    action = Some((id, "edit"));
                    ui.close_menu();
                }
                if ui.button(if suppressed { "Unsuppress" } else { "Suppress" }).clicked() {
                    action = Some((id, "suppress"));
                    ui.close_menu();
                }
                if ui.button("Rename").clicked() {
                    action = Some((id, "rename"));
                    ui.close_menu();
                }
                if ui
                    .button("Configure Feature")
                    .on_hover_text("Vary its suppression and dimensions by configuration")
                    .clicked()
                {
                    action = Some((id, "configure"));
                    ui.close_menu();
                }
                if ui.button("Roll Back to Here").clicked() {
                    action = Some((id, "rollback"));
                    ui.close_menu();
                }
                ui.separator();
                if ui.button("Delete").clicked() {
                    action = Some((id, "delete"));
                    ui.close_menu();
                }
            });
        }
        if rollback <= self.doc.features.len() && self.doc.rollback.is_some() {
            if rollback == self.doc.features.len() {
                rollback_bar(ui);
            }
            ui.add_space(4.0);
            if ui.button("Roll to End").clicked() {
                self.doc.rollback = None;
                self.rebuild();
            }
        }

        if let Some((id, what)) = action {
            match what {
                "edit" => {
                    let before = self.doc.clone();
                    self.open_editor(id, before, false);
                }
                "suppress" => {
                    if let Some(f) = self.doc.feature_mut(id) {
                        f.suppressed = !f.suppressed;
                    }
                    self.rebuild();
                }
                "rename" => {
                    let name = self.doc.feature(id).map(|f| f.name.clone()).unwrap_or_default();
                    self.rename = Some((id, name));
                }
                "configure" => self.configure_feature(id),
                "rollback" => {
                    self.doc.rollback = self.doc.index_of(id);
                    self.rebuild();
                }
                "delete" => {
                    self.selection = Selection::Feature(id);
                    self.delete_selected_feature();
                }
                _ => {}
            }
        }

        if !self.state.variable_errors.is_empty() {
            ui.separator();
            for (n, e) in &self.state.variable_errors {
                ui.colored_label(Color32::from_rgb(200, 30, 30), format!("Equation '{n}': {e}"));
            }
        }
    }

    fn dim_field(
        ui: &mut egui::Ui,
        label: &str,
        dim: &mut Dim,
        buf: &mut String,
        vars: &std::collections::BTreeMap<String, f64>,
    ) -> bool {
        let mut changed = false;
        ui.horizontal(|ui| {
            ui.label(label);
            let r = ui.add(egui::TextEdit::singleline(buf).desired_width(110.0));
            if r.changed() {
                let d = Dim::parse(buf);
                if d.eval(vars).is_ok() {
                    *dim = d;
                    changed = true;
                }
            }
            match Dim::parse(buf).eval(vars) {
                Ok(v) if matches!(Dim::parse(buf), Dim::Expr(_)) => {
                    ui.weak(format!("= {}", sketcher::format_num(v)));
                }
                Err(e) => {
                    ui.colored_label(Color32::from_rgb(200, 30, 30), "!").on_hover_text(e.to_string());
                }
                _ => {}
            }
        });
        changed
    }

    fn property_manager(&mut self, ui: &mut egui::Ui) {
        let Some(editor) = &mut self.editor else { return };
        match editor {
            Editor::PickPlane(purpose) => {
                let sketch = *purpose == PlanePurpose::Sketch;
                panel_title(
                    ui,
                    if sketch { Icon::Sketch } else { Icon::Plane },
                    if sketch { "Sketch" } else { "Plane" },
                );
                ui.label("Select a plane in the tree or a planar face in the graphics area.");
                ui.add_space(4.0);
                let mut picked = None;
                for p in [PlaneRef::Front, PlaneRef::Top, PlaneRef::Right] {
                    if ui.button(p.label()).clicked() {
                        picked = Some(p);
                    }
                }
                ui.add_space(4.0);
                if theme::secondary_button(ui, "Cancel").clicked() {
                    self.editor = None;
                }
                if let Some(p) = picked {
                    self.plane_picked(p);
                }
            }
            Editor::Feature { id, bufs, .. } => {
                let id = *id;
                let vars = self.state.variables.clone();
                let sketch_names: HashMap<u32, String> =
                    self.doc.features.iter().map(|f| (f.id, f.name.clone())).collect();
                // Line entities of a revolve's sketch, offered as axis choices.
                let axis_options: Vec<(usize, String)> = match self.doc.feature(id).map(|f| &f.kind) {
                    Some(FeatureKind::Revolve(r)) => self
                        .doc
                        .sketch(r.sketch)
                        .map(|sf| {
                            sf.sketch
                                .entities
                                .iter()
                                .enumerate()
                                .filter(|(_, e)| e.is_line())
                                .map(|(i, e)| {
                                    (
                                        i,
                                        format!(
                                            "Line {}{}",
                                            i + 1,
                                            if e.construction { " (centerline)" } else { "" }
                                        ),
                                    )
                                })
                                .collect()
                        })
                        .unwrap_or_default(),
                    _ => Vec::new(),
                };
                // How many holes the hole feature's sketch places.
                let hole_points = match self.doc.feature(id).map(|f| &f.kind) {
                    Some(FeatureKind::Hole(h)) => {
                        self.doc.sketch(h.sketch).map_or(0, |s| hole::positions(&s.sketch).len())
                    }
                    _ => 0,
                };
                let mut edit_positions = false;
                let Some(f) = self.doc.feature_mut(id) else {
                    self.editor = None;
                    return;
                };
                panel_title(ui, icon(&f.kind), &f.name);
                let mut changed = false;
                match &mut f.kind {
                    FeatureKind::Extrude(e) => {
                        ui.label(format!(
                            "Sketch: {}",
                            sketch_names.get(&e.sketch).cloned().unwrap_or_default()
                        ));
                        egui::ComboBox::from_label("Operation")
                            .selected_text(if e.op == BodyOp::Boss { "Boss" } else { "Cut" })
                            .show_ui(ui, |ui| {
                                changed |= ui.selectable_value(&mut e.op, BodyOp::Boss, "Boss").changed();
                                changed |= ui.selectable_value(&mut e.op, BodyOp::Cut, "Cut").changed();
                            });
                        egui::ComboBox::from_label("End condition").selected_text(e.end.label()).show_ui(
                            ui,
                            |ui| {
                                for c in EndCondition::ALL {
                                    changed |= ui.selectable_value(&mut e.end, c, c.label()).changed();
                                }
                            },
                        );
                        if e.end.uses_depth() {
                            changed |= Self::dim_field(
                                ui,
                                "Depth (mm)",
                                &mut e.depth,
                                bufs.entry("depth").or_default(),
                                &vars,
                            );
                        }
                        changed |= ui.checkbox(&mut e.reverse, "Reverse direction").changed();
                    }
                    FeatureKind::Revolve(r) => {
                        ui.label(format!(
                            "Sketch: {}",
                            sketch_names.get(&r.sketch).cloned().unwrap_or_default()
                        ));
                        changed |= Self::dim_field(
                            ui,
                            "Angle (deg)",
                            &mut r.angle,
                            bufs.entry("angle").or_default(),
                            &vars,
                        );
                        egui::ComboBox::from_label("Operation")
                            .selected_text(if r.op == BodyOp::Boss { "Boss" } else { "Cut" })
                            .show_ui(ui, |ui| {
                                changed |= ui.selectable_value(&mut r.op, BodyOp::Boss, "Boss").changed();
                                changed |= ui.selectable_value(&mut r.op, BodyOp::Cut, "Cut").changed();
                            });
                        let label = |e: &usize| {
                            axis_options
                                .iter()
                                .find(|(i, _)| i == e)
                                .map(|(_, l)| l.clone())
                                .unwrap_or_else(|| "<missing>".into())
                        };
                        egui::ComboBox::from_label("Axis").selected_text(label(&r.axis)).show_ui(ui, |ui| {
                            for (e, l) in &axis_options {
                                changed |= ui.selectable_value(&mut r.axis, *e, l).changed();
                            }
                        });
                    }
                    FeatureKind::Plane(p) => {
                        ui.label(format!("Base: {}", p.base.label()));
                        changed |= Self::dim_field(
                            ui,
                            "Offset (mm)",
                            &mut p.offset,
                            bufs.entry("offset").or_default(),
                            &vars,
                        );
                        changed |= ui.checkbox(&mut p.flip, "Flip offset").changed();
                    }
                    FeatureKind::LinearPattern(p) => {
                        let names: Vec<String> =
                            p.features.iter().filter_map(|i| sketch_names.get(i).cloned()).collect();
                        ui.label(format!("Features: {}", names.join(", ")));
                        egui::ComboBox::from_label("Direction").selected_text(p.direction.label()).show_ui(
                            ui,
                            |ui| {
                                for a in [Axis::X, Axis::Y, Axis::Z] {
                                    changed |=
                                        ui.selectable_value(&mut p.direction, a, format!("{a:?}")).changed();
                                }
                            },
                        );
                        changed |= Self::dim_field(
                            ui,
                            "Spacing (mm)",
                            &mut p.spacing,
                            bufs.entry("spacing").or_default(),
                            &vars,
                        );
                        changed |= Self::dim_field(
                            ui,
                            "Instances",
                            &mut p.count,
                            bufs.entry("count").or_default(),
                            &vars,
                        );
                    }
                    FeatureKind::CircularPattern(p) => {
                        let names: Vec<String> =
                            p.features.iter().filter_map(|i| sketch_names.get(i).cloned()).collect();
                        ui.label(format!("Features: {}", names.join(", ")));
                        egui::ComboBox::from_label("Axis").selected_text(p.axis.label()).show_ui(ui, |ui| {
                            for a in [Axis::X, Axis::Y, Axis::Z] {
                                changed |= ui.selectable_value(&mut p.axis, a, format!("{a:?}")).changed();
                            }
                        });
                        changed |= Self::dim_field(
                            ui,
                            "Total angle (deg)",
                            &mut p.total_angle,
                            bufs.entry("angle").or_default(),
                            &vars,
                        );
                        changed |= Self::dim_field(
                            ui,
                            "Instances",
                            &mut p.count,
                            bufs.entry("count").or_default(),
                            &vars,
                        );
                    }
                    FeatureKind::ImportedBody(ib) => {
                        ui.label(format!("Geometry imported from {}.", ib.source));
                        if !ib.replaces.is_empty() {
                            ui.label(format!("Stands in for: {}", ib.replaces.join(", ")));
                        }
                        ui.label(format!("{} faces", ib.faces.len()));
                        ui.weak("Features above it are parametric; this body is fixed geometry.");
                    }
                    FeatureKind::Fillet(fl) => {
                        let current = fl.profile.label();
                        egui::ComboBox::from_label("Profile").selected_text(current).show_ui(ui, |ui| {
                            for (i, label) in FilletProfile::LABELS.iter().enumerate() {
                                if ui.selectable_label(current == *label, *label).clicked()
                                    && current != *label
                                {
                                    fl.profile = match i {
                                        0 => FilletProfile::Circular,
                                        1 => FilletProfile::ConicRho { rho: Dim::Value(0.5) },
                                        2 => FilletProfile::ConicRadius { radius: Dim::Value(1.0) },
                                        _ => FilletProfile::CurvatureContinuous,
                                    };
                                    changed = true;
                                }
                            }
                        });
                        changed |= Self::dim_field(
                            ui,
                            "Radius (mm)",
                            &mut fl.radius,
                            bufs.entry("radius").or_default(),
                            &vars,
                        );
                        match &mut fl.profile {
                            FilletProfile::ConicRho { rho } => {
                                changed |= Self::dim_field(
                                    ui,
                                    "Rho (0.5 to 0.95)",
                                    rho,
                                    bufs.entry("rho").or_default(),
                                    &vars,
                                );
                            }
                            FilletProfile::ConicRadius { radius } => {
                                changed |= Self::dim_field(
                                    ui,
                                    "Shoulder radius (mm)",
                                    radius,
                                    bufs.entry("conic").or_default(),
                                    &vars,
                                );
                            }
                            _ => {}
                        }
                        changed |= ui
                            .checkbox(&mut fl.tangent_propagation, "Tangent propagation")
                            .on_hover_text("Also blend the edges that run smoothly on from the selected ones (on the exact kernel)")
                            .changed();
                        ui.label(format!("{} edge(s) selected.", fl.edges.len()));
                        ui.weak("Click edges in the graphics area to add or remove them.");
                        if !fl.edges.is_empty() && ui.button("Clear selection").clicked() {
                            fl.edges.clear();
                            changed = true;
                        }
                    }
                    FeatureKind::Chamfer(ch) => {
                        let current = ch.kind.label();
                        egui::ComboBox::from_label("Type").selected_text(current).show_ui(ui, |ui| {
                            for (i, label) in ChamferKind::LABELS.iter().enumerate() {
                                if ui.selectable_label(current == *label, *label).clicked()
                                    && current != *label
                                {
                                    ch.kind = match i {
                                        0 => ChamferKind::EqualDistance,
                                        1 => ChamferKind::AngleDistance {
                                            angle: Dim::Value(45.0),
                                            flip: false,
                                        },
                                        _ => ChamferKind::OffsetFace,
                                    };
                                    changed = true;
                                }
                            }
                        });
                        changed |= Self::dim_field(
                            ui,
                            "Distance (mm)",
                            &mut ch.distance,
                            bufs.entry("distance").or_default(),
                            &vars,
                        );
                        if let ChamferKind::AngleDistance { angle, flip } = &mut ch.kind {
                            changed |= Self::dim_field(
                                ui,
                                "Angle (degrees)",
                                angle,
                                bufs.entry("angle").or_default(),
                                &vars,
                            );
                            changed |= ui.checkbox(flip, "Measure along the other face").changed();
                        }
                        changed |= ui
                            .checkbox(&mut ch.tangent_propagation, "Tangent propagation")
                            .on_hover_text("Also blend the edges that run smoothly on from the selected ones (on the exact kernel)")
                            .changed();
                        ui.label(format!("{} edge(s) selected.", ch.edges.len()));
                        ui.weak("Click edges in the graphics area to add or remove them.");
                        if !ch.edges.is_empty() && ui.button("Clear selection").clicked() {
                            ch.edges.clear();
                            changed = true;
                        }
                    }
                    FeatureKind::Shell(sh) => {
                        changed |= Self::dim_field(
                            ui,
                            "Thickness (mm)",
                            &mut sh.thickness,
                            bufs.entry("thickness").or_default(),
                            &vars,
                        );
                        ui.label(format!("{} face(s) to remove.", sh.remove.len()));
                        ui.weak("Click faces to open them; leave none for a closed hollow part.");
                        if !sh.remove.is_empty() && ui.button("Clear selection").clicked() {
                            sh.remove.clear();
                            changed = true;
                        }
                    }
                    FeatureKind::Hole(h) => {
                        changed |= hole_editor(ui, h, bufs, &vars, hole_points, &mut edit_positions);
                    }
                    FeatureKind::Sketch(_) => {}
                    _ => {}
                }
                if let Some(e) = self.state.errors.get(&id) {
                    ui.colored_label(Palette::of(ui).error, e);
                }
                ui.add_space(6.0);
                ui.separator();
                ui.add_space(2.0);
                let mut close: Option<bool> = None;
                ui.horizontal(|ui| {
                    if theme::primary_button(ui, "OK").clicked() {
                        close = Some(true);
                    }
                    if theme::secondary_button(ui, "Cancel").clicked() {
                        close = Some(false);
                    }
                });
                if changed {
                    self.rebuild();
                }
                if let Some(ok) = close {
                    self.finish_editor(ok);
                } else if edit_positions {
                    // Keep the hole and open its sketch to move or add points.
                    let sketch = match self.doc.feature(id).map(|f| &f.kind) {
                        Some(FeatureKind::Hole(h)) => Some(h.sketch),
                        _ => None,
                    };
                    self.finish_editor(true);
                    if let Some(sk) = sketch {
                        self.edit_sketch(sk);
                    }
                }
            }
        }
    }

    /// Closes the feature's property panel: OK keeps the edit, Cancel restores the model.
    fn finish_editor(&mut self, ok: bool) {
        if let Some(Editor::Feature { before, is_new, .. }) = self.editor.take() {
            if !ok {
                self.doc = *before;
                if is_new {
                    self.selection = Selection::None;
                }
            }
        }
        self.rebuild();
    }

    fn sketch_manager(&mut self, ui: &mut egui::Ui) {
        let Some(s) = &mut self.sketch else { return };
        let name = self.doc.feature(s.feature).map(|f| f.name.clone()).unwrap_or_default();
        let Some(sf) = self.doc.sketch_mut(s.feature) else {
            self.sketch = None;
            return;
        };
        let pal = Palette::of(ui);
        panel_title(ui, Icon::Sketch, &name);
        let color = if !s.status.converged {
            pal.error
        } else if s.status.dof == 0 {
            pal.text
        } else {
            pal.select
        };
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(egui::vec2(10.0, 16.0), Sense::hover());
            ui.painter().circle_filled(r.center(), 4.0, color);
            ui.colored_label(color, s.status.status_text());
        });
        ui.add_space(4.0);
        theme::section_label(ui, &format!("Tool: {}", s.tool.label()));
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(2.0, 2.0);
            for t in SKETCH_TOOLS {
                if theme::icon_button(ui, tool_icon(t), t.label(), t.label(), Some(s.tool == t)).clicked() {
                    s.set_tool(t);
                }
            }
        });
        ui.separator();

        let mut solve = false;

        // A new dimension that over-defines the sketch: ask what to do with it.
        if let Some(ci) = s.over_define {
            let what = sf.sketch.constraints.get(ci).map_or("dimension", |c| c.label());
            ui.label(RichText::new("Over-defining dimension").strong().color(Color32::from_rgb(200, 30, 30)));
            ui.label(format!(
                "This {} is already set by the sketch's other dimensions and relations.",
                what.to_lowercase()
            ));
            let mut choice = None;
            ui.horizontal_wrapped(|ui| {
                if ui.button("Make Reference").clicked() {
                    choice = Some(OverDefine::Reference);
                }
                if ui.button("Keep Driving").clicked() {
                    choice = Some(OverDefine::Driving);
                }
                if ui.button("Cancel Dimension").clicked() {
                    choice = Some(OverDefine::Cancel);
                }
            });
            ui.weak("A reference dimension shows the size in parentheses and follows the geometry.");
            if let Some(c) = choice {
                s.resolve_over_define(sf, c);
            }
            ui.separator();
        }

        // Dimension editor.
        if let Some((ci, text)) = &mut s.dim_edit {
            let ci = *ci;
            if let Some(c) = sf.sketch.constraints.get(ci).copied() {
                ui.label(RichText::new(format!("Modify {}", c.label())).strong());
                if sf.sketch.is_reference(ci) {
                    let v = c.value().map(|v| sketcher::dimension_text(&c, v)).unwrap_or_default();
                    ui.label(format!("Reference dimension: ({v})"));
                    ui.weak("A reference dimension follows the geometry and cannot be edited.");
                    if ui.button("Make Driving").clicked() {
                        if anvil_sketch::dimension::over_defines(&sf.sketch, ci) {
                            self.status = "That dimension would over-define the sketch.".into();
                        } else {
                            sf.sketch.set_reference(ci, false);
                            solve = true;
                        }
                    }
                } else {
                    let r = ui.add(
                        egui::TextEdit::singleline(text)
                            .desired_width(140.0)
                            .hint_text("value, or =expression"),
                    );
                    ui.weak("Type a value, arithmetic or units (1in + 2mm, 30deg). Start with = to link an equation.");
                    if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                        let angle = matches!(
                            c,
                            anvil_sketch::Constraint::Angle { .. }
                                | anvil_sketch::Constraint::RayAngle { .. }
                        );
                        let vars = &self.state.variables;
                        match equations::parse_dimension_input(text, angle, &|n| vars.get(n).copied()) {
                            Ok(input) => {
                                sf.sketch.constraints[ci].set_value(input.value);
                                sf.set_driven(ci, input.equation);
                                solve = true;
                            }
                            Err(e) => {
                                self.status = format!("Invalid dimension: {e}");
                            }
                        }
                    }
                    if ui.button("Make Reference").clicked() {
                        sf.sketch.set_reference(ci, true);
                        sf.set_driven(ci, None);
                        solve = true;
                    }
                }
                ui.separator();
            } else {
                s.dim_edit = None;
            }
        }

        // Relations for the selection.
        let rels = s.available_relations(&sf.sketch);
        if !rels.is_empty() {
            ui.label(RichText::new("Add Relations").strong());
            ui.horizontal_wrapped(|ui| {
                for (name, c) in rels {
                    if ui.button(name).clicked() {
                        sf.sketch.add_constraint(c);
                        solve = true;
                    }
                }
            });
        }
        let ents = s.selected_entities();
        if !ents.is_empty() {
            let all_cons = ents.iter().all(|&e| sf.sketch.entities[e].construction);
            let mut cons = all_cons;
            if ui.checkbox(&mut cons, "For construction").changed() {
                for &e in &ents {
                    sf.sketch.entities[e].construction = cons;
                }
            }
        }
        if !s.selection.is_empty() && ui.button("Delete Selected  (Del)").clicked() {
            s.delete_selection(sf);
        }
        ui.separator();

        // Existing relations.
        egui::CollapsingHeader::new(format!("Relations ({})", sf.sketch.constraints.len()))
            .default_open(false)
            .show(ui, |ui| {
                let mut del = None;
                for (ci, c) in sf.sketch.constraints.iter().enumerate() {
                    let bad = s.status.unsatisfied.contains(&ci);
                    ui.horizontal(|ui| {
                        let mut t = RichText::new(match c.value() {
                            Some(v) if sf.sketch.is_reference(ci) => {
                                format!("{} = ({})", c.label(), sketcher::dimension_text(c, v))
                            }
                            Some(v) => format!("{} = {}", c.label(), sketcher::dimension_text(c, v)),
                            None => c.label().to_string(),
                        });
                        if bad {
                            t = t.color(pal.error);
                        }
                        if ui.selectable_label(s.selection.contains(&SkSel::Constraint(ci)), t).clicked() {
                            s.selection = vec![SkSel::Constraint(ci)];
                        }
                        let is_origin =
                            matches!(c, anvil_sketch::Constraint::Fix { p: anvil_sketch::ORIGIN, .. });
                        if !is_origin && ui.small_button("x").clicked() {
                            del = Some(ci);
                        }
                    });
                }
                if let Some(ci) = del {
                    sf.delete_constraint(ci);
                    s.selection.clear();
                    s.dim_edit = None;
                    solve = true;
                }
            });
        if solve {
            s.status = sf.sketch.solve();
        }
        ui.separator();
        let mut exit = false;
        ui.horizontal(|ui| {
            if theme::primary_button(ui, "Exit Sketch").clicked() {
                exit = true;
            }
        });
        ui.weak("Middle-drag: rotate. Ctrl+middle or right-drag: pan. Wheel: zoom. Esc: cancel tool.");
        if exit {
            self.exit_sketch();
        }
    }

    // ----- UI: viewport ---------------------------------------------------------------

    fn viewport(&mut self, ui: &mut egui::Ui) {
        let rect = ui.available_rect_before_wrap();
        self.view_rect = rect;
        let resp = ui.allocate_rect(rect, Sense::click_and_drag());
        let painter = ui.painter_at(rect);

        // Background gradient.
        let mut mesh = egui::Mesh::default();
        let pal = Palette::of(ui);
        let (top, bottom) = (pal.view_top, pal.view_bottom);
        mesh.colored_vertex(rect.left_top(), top);
        mesh.colored_vertex(rect.right_top(), top);
        mesh.colored_vertex(rect.right_bottom(), bottom);
        mesh.colored_vertex(rect.left_bottom(), bottom);
        mesh.add_triangle(0, 1, 2);
        mesh.add_triangle(0, 2, 3);
        painter.add(egui::Shape::mesh(mesh));

        if self.fit_pending && rect.width() > 10.0 {
            let mut b = self.state.body.bounds();
            if b.is_empty() {
                for f in self.state.frames.values() {
                    b.add(f.origin);
                }
            }
            self.camera.fit(&b, rect);
            self.fit_pending = false;
        }

        // Camera navigation.
        let (scroll, mods, middle, secondary_down, delta, pointer_in) = ui.input(|i| {
            (
                i.smooth_scroll_delta.y,
                i.modifiers,
                i.pointer.button_down(egui::PointerButton::Middle),
                i.pointer.button_down(egui::PointerButton::Secondary),
                i.pointer.delta(),
                i.pointer.hover_pos().is_some_and(|p| rect.contains(p)),
            )
        });
        if pointer_in && scroll != 0.0 {
            if let Some(p) = resp.hover_pos() {
                self.camera.zoom_at((-scroll as f64 * 0.0025).exp(), p, rect);
            }
        }
        if resp.dragged_by(egui::PointerButton::Middle)
            || (middle && resp.hovered() && delta != egui::Vec2::ZERO)
        {
            if mods.ctrl {
                self.camera.pan(delta.x as f64, delta.y as f64, rect);
            } else if mods.shift {
                self.camera.zoom_at((delta.y as f64 * 0.01).exp(), rect.center(), rect);
            } else {
                self.camera.orbit(delta.x as f64, delta.y as f64);
            }
        }
        if resp.dragged_by(egui::PointerButton::Secondary) && secondary_down {
            self.camera.pan(delta.x as f64, delta.y as f64, rect);
        }

        // Picking edges or faces for a fillet, chamfer or shell being edited.
        if let (Some((fid, kind)), true) = (self.pick_target(), self.sketch.is_none()) {
            let clicked = resp.clicked_by(egui::PointerButton::Primary);
            match kind {
                PickKind::Edges => {
                    let hit = resp.hover_pos().and_then(|m| self.nearest_edge(m, rect));
                    self.hover_edge = hit;
                    if self.hover_face.is_some() {
                        self.hover_face = None;
                        self.scene_dirty = true;
                    }
                    if let (true, Some(e)) = (clicked, hit) {
                        self.toggle_pick(fid, Some(e), None);
                    }
                }
                PickKind::Faces => {
                    let hit = resp
                        .hover_pos()
                        .and_then(|p| {
                            let (o, d) = self.camera.ray(p, rect);
                            self.pick_body.as_ref()?.solid.raycast(o, d)
                        })
                        .map(|h| h.tag.id);
                    if hit != self.hover_face {
                        self.hover_face = hit;
                        self.scene_dirty = true;
                    }
                    if let (true, Some(f)) = (clicked, hit) {
                        self.toggle_pick(fid, None, Some(f));
                    }
                }
            }
        } else if self.sketch.is_none() {
            self.hover_edge = None;
            let hit = resp.hover_pos().and_then(|p| {
                let (o, d) = self.camera.ray(p, rect);
                self.state.body.raycast(o, d)
            });
            let hover = hit.as_ref().map(|h| h.tag.id);
            if hover != self.hover_face {
                self.hover_face = hover;
                self.scene_dirty = true;
            }
            if resp.clicked_by(egui::PointerButton::Primary) {
                match hover {
                    Some(f) => {
                        self.selection = Selection::Face(f);
                        self.face_click = hit.map(|h| (f, h.point));
                        if matches!(self.editor, Some(Editor::PickPlane(_))) {
                            if self.state.body.planar_face_frame(f).is_some() {
                                self.plane_picked(PlaneRef::Face(f));
                            } else {
                                self.status = "That face is not planar.".into();
                            }
                        }
                    }
                    None => self.selection = Selection::None,
                }
                self.scene_dirty = true;
            }
            if resp.double_clicked() {
                if let Some(f) = hover {
                    // Double-clicking a face edits the feature that made it.
                    if self.doc.feature(f.feature).is_some() {
                        let before = self.doc.clone();
                        self.open_editor(f.feature, before, false);
                    }
                }
            }
        } else if self.hover_face.is_some() {
            self.hover_face = None;
            self.scene_dirty = true;
        }

        // Upload and draw the model.
        if self.scene_dirty {
            let shell_faces =
                match self.pick_target().and_then(|(id, _)| self.doc.feature(id)).map(|f| &f.kind) {
                    Some(FeatureKind::Shell(s)) => Some(s.remove.clone()),
                    _ => None,
                };
            let dfm_faces = match (self.windows.dfm, &self.dfm.report, self.dfm.focus) {
                (true, Some(r), Some(i)) => r.findings.get(i).map(|f| f.faces.clone()),
                _ => None,
            };
            let selected: Vec<FaceId> = if let Some(faces) = shell_faces {
                faces
            } else if let Some(faces) = dfm_faces {
                faces
            } else {
                match self.selection {
                    Selection::Face(f) => vec![f],
                    Selection::Feature(id) if self.pick_body.is_none() => {
                        self.state.body.face_ids().into_iter().filter(|f| f.feature == id).collect()
                    }
                    _ => vec![],
                }
            };
            let hover = if self.sketch.is_some() { None } else { self.hover_face };
            let data = SceneData::build(&self.mesh, &self.edges, hover, &selected);
            self.renderer.lock().unwrap().set_scene(data);
            self.scene_dirty = false;
        }
        let mvp = self.camera.mvp(rect);
        let view = self.camera.back;
        let light = self.camera.back * 1.0 + self.camera.up * 0.45 + self.camera.right * 0.25;
        let edges = self.show_edges;
        let cb = self.renderer.lock().unwrap().callback(rect, mvp, view, light, edges);
        painter.add(cb);

        // Overlays: planes, sketches, edges picked for a fillet or chamfer.
        self.draw_planes(&painter, rect);
        self.draw_model_sketches(&painter, rect);
        self.draw_picked_edges(&painter, rect);

        // Sketch interaction.
        if let Some(s) = &mut self.sketch {
            if let Some(sf) = self.doc.sketch_mut(s.feature) {
                let out = s.handle(sf, &resp, &self.camera, rect, ui.ctx());
                self.dragging = out.dragging;
                s.draw(sf, &painter, &self.camera, rect);
            }
        } else {
            self.dragging = false;
        }

        // Axis triad, drawn back to front so the axis nearest the viewer is on top.
        let o = rect.left_bottom() + egui::vec2(46.0, -46.0);
        painter.circle(o, 34.0, pal.float_fill.gamma_multiply(0.7), Stroke::new(1.0, pal.border));
        let (r, g, b) = if pal.dark {
            (
                Color32::from_rgb(240, 100, 100),
                Color32::from_rgb(110, 200, 120),
                Color32::from_rgb(110, 150, 240),
            )
        } else {
            (Color32::from_rgb(200, 50, 50), Color32::from_rgb(40, 140, 60), Color32::from_rgb(50, 90, 210))
        };
        let mut axes = [(Vec3::X, r, "X"), (Vec3::Y, g, "Y"), (Vec3::Z, b, "Z")];
        axes.sort_by(|a, b| a.0.dot(self.camera.back).total_cmp(&b.0.dot(self.camera.back)));
        for (axis, color, name) in axes {
            let d = egui::vec2(axis.dot(self.camera.right) as f32, -axis.dot(self.camera.up) as f32) * 22.0;
            painter.line_segment([o, o + d], Stroke::new(2.2_f32, color));
            painter.circle_filled(o + d, 6.5, color);
            painter.text(
                o + d,
                egui::Align2::CENTER_CENTER,
                name,
                egui::FontId::proportional(9.5),
                Color32::WHITE,
            );
        }
        painter.circle_filled(o, 2.5, pal.text_weak);

        self.view_toolbar(ui, rect);
        if let Some(e) = &self.renderer.lock().unwrap().error {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                format!("OpenGL error: {e}"),
                egui::FontId::proportional(14.0),
                Color32::RED,
            );
        }
    }

    /// The floating view toolbar at the top of the viewport: standard views, zoom to fit and
    /// the edges toggle.
    fn view_toolbar(&mut self, ui: &mut egui::Ui, rect: egui::Rect) {
        let pal = Palette::of(ui);
        let size = egui::vec2(6.0 * 30.0 + 9.0 + 16.0, 36.0);
        let bar =
            egui::Rect::from_center_size(egui::pos2(rect.center().x, rect.top() + 12.0 + size.y / 2.0), size);
        if !rect.contains_rect(bar) {
            return;
        }
        let painter = ui.painter_at(rect);
        painter.rect(
            bar.translate(egui::vec2(0.0, 2.0)),
            10.0,
            Color32::from_black_alpha(if pal.dark { 60 } else { 18 }),
            Stroke::NONE,
            egui::StrokeKind::Outside,
        );
        painter.rect(bar, 10.0, pal.float_fill, Stroke::new(1.0, pal.border), egui::StrokeKind::Inside);
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(bar.shrink2(egui::vec2(8.0, 4.0)))
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        let ui = &mut child;
        ui.spacing_mut().item_spacing.x = 2.0;
        for (view, ic, key) in [
            (StdView::Front, Icon::ViewFront, 1),
            (StdView::Top, Icon::ViewTop, 5),
            (StdView::Right, Icon::ViewRight, 4),
            (StdView::Isometric, Icon::ViewIso, 7),
        ] {
            let label = format!("{} view", view.label());
            let tip = format!("{} (Ctrl+{key})", view.label());
            if theme::icon_button(ui, ic, &label, &tip, None).clicked() {
                self.camera.set_view(view);
                self.fit_pending = true;
            }
        }
        if theme::icon_button(ui, Icon::Fit, "Zoom to Fit", "Zoom to Fit (F)", None).clicked() {
            self.fit_pending = true;
        }
        let (r, _) = ui.allocate_exact_size(egui::vec2(7.0, 20.0), Sense::hover());
        ui.painter().vline(r.center().x, r.y_range(), Stroke::new(1.0, pal.border));
        if theme::icon_button(
            ui,
            Icon::Edges,
            "Shaded with edges",
            "Shaded with edges",
            Some(self.show_edges),
        )
        .clicked()
        {
            self.show_edges = !self.show_edges;
        }
    }

    /// The model edge nearest the cursor (within a few pixels) on the pick body.
    fn nearest_edge(&self, m: egui::Pos2, rect: egui::Rect) -> Option<EdgeRef> {
        let pb = self.pick_body.as_ref()?;
        let mut best: Option<(f32, EdgeRef)> = None;
        for e in &pb.edges {
            let (a, b) = (self.camera.project(e.seg[0], rect), self.camera.project(e.seg[1], rect));
            let ab = b - a;
            let t =
                if ab.length_sq() < 1e-6 { 0.0 } else { ((m - a).dot(ab) / ab.length_sq()).clamp(0.0, 1.0) };
            let d = (a + ab * t - m).length();
            if d < 8.0 && best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, e.faces));
            }
        }
        best.map(|b| b.1)
    }

    /// Adds or removes an edge or face from the fillet, chamfer or shell being edited.
    fn toggle_pick(&mut self, id: u32, edge: Option<EdgeRef>, face: Option<FaceId>) {
        let toggle = |list: &mut Vec<EdgeRef>, e: EdgeRef| match list.iter().position(|x| *x == e) {
            Some(i) => {
                list.remove(i);
            }
            None => list.push(e),
        };
        if let Some(f) = self.doc.feature_mut(id) {
            match (&mut f.kind, edge, face) {
                (FeatureKind::Fillet(fl), Some(e), _) => toggle(&mut fl.edges, e),
                (FeatureKind::Chamfer(ch), Some(e), _) => toggle(&mut ch.edges, e),
                (FeatureKind::Shell(sh), _, Some(face)) => match sh.remove.iter().position(|x| *x == face) {
                    Some(i) => {
                        sh.remove.remove(i);
                    }
                    None => sh.remove.push(face),
                },
                _ => {}
            }
        }
        self.rebuild();
    }

    fn draw_picked_edges(&self, painter: &egui::Painter, rect: egui::Rect) {
        let (Some((id, PickKind::Edges)), Some(pb)) = (self.pick_target(), &self.pick_body) else { return };
        let chosen: &[EdgeRef] = match self.doc.feature(id).map(|f| &f.kind) {
            Some(FeatureKind::Fillet(f)) => &f.edges,
            Some(FeatureKind::Chamfer(c)) => &c.edges,
            _ => &[],
        };
        for e in &pb.edges {
            let color = if chosen.contains(&e.faces) {
                Color32::from_rgb(255, 120, 0)
            } else if self.hover_edge == Some(e.faces) {
                Color32::from_rgb(255, 200, 40)
            } else {
                continue;
            };
            let (a, b) = (self.camera.project(e.seg[0], rect), self.camera.project(e.seg[1], rect));
            painter.line_segment([a, b], Stroke::new(3.0_f32, color));
        }
    }

    fn draw_planes(&self, painter: &egui::Painter, rect: egui::Rect) {
        let sel_frame = match self.selection {
            Selection::Plane(p) => p.standard_frame(),
            Selection::Feature(id) => match self.doc.feature(id).map(|f| &f.kind) {
                Some(FeatureKind::Plane(_)) => self.state.frames.get(&id).copied(),
                _ => None,
            },
            _ => None,
        };
        let Some(f) = sel_frame else { return };
        let b = self.state.body.bounds();
        let s = if b.is_empty() { 50.0 } else { (b.diagonal() * 0.6).max(20.0) };
        let corners = [(-s, -s), (s, -s), (s, s), (-s, s)];
        let pts: Vec<egui::Pos2> =
            corners.iter().map(|&(x, y)| self.camera.project(f.to_world(Vec2 { x, y }), rect)).collect();
        painter.add(egui::Shape::convex_polygon(
            pts.clone(),
            Color32::from_rgba_unmultiplied(80, 140, 255, 40),
            Stroke::new(1.5_f32, Color32::from_rgb(60, 110, 230)),
        ));
    }

    fn draw_model_sketches(&self, painter: &egui::Painter, rect: egui::Rect) {
        let consumed: Vec<u32> = self
            .doc
            .features
            .iter()
            .filter(|f| !f.suppressed)
            .flat_map(|f| match &f.kind {
                FeatureKind::Extrude(e) => vec![e.sketch],
                FeatureKind::Revolve(r) => vec![r.sketch],
                _ => vec![],
            })
            .collect();
        for f in &self.doc.features {
            let FeatureKind::Sketch(sf) = &f.kind else { continue };
            if self.sketch.as_ref().is_some_and(|s| s.feature == f.id) || f.suppressed {
                continue;
            }
            let selected = self.selection == Selection::Feature(f.id);
            if consumed.contains(&f.id) && !selected {
                continue;
            }
            let Some(frame) = self.state.frames.get(&f.id) else { continue };
            let color =
                if selected { Color32::from_rgb(40, 110, 240) } else { Color32::from_rgb(90, 90, 100) };
            sketcher::draw_sketch_geometry(&sf.sketch, frame, painter, &self.camera, rect, &|_| color, 1.4);
        }
    }

    // ----- UI: windows -------------------------------------------------------------------

    fn file_props_window(&mut self, ctx: &egui::Context) {
        use anvil_io::props::sidecar::Origin;
        let Some(w) = &mut self.file_props else { return };
        let name = w.sheet.target.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let mut open = true;
        let mut discard = false;
        egui::Window::new(format!("File Properties - {name}"))
            .id(egui::Id::new("file_props"))
            .open(&mut open)
            .default_width(560.0)
            .show(ctx, |ui| {
                let warn = Color32::from_rgb(170, 90, 0);
                match (&w.sheet.file, &w.sheet.live_error) {
                    (Some((Origin::FileLive(src), _)), _) => {
                        ui.weak(format!(
                            "Read from the file through its {}. AnvilCAD never modifies it.",
                            src.label()
                        ));
                    }
                    (Some((origin, _)), Some(e)) => {
                        ui.weak(format!(
                            "{}. This computer cannot read the file itself: {e}",
                            origin.label()
                        ));
                        if w.sheet.stale {
                            ui.colored_label(
                                warn,
                                "The file has changed since its properties were converted. Convert it again \
                                 with anvil-props to refresh them.",
                            );
                        }
                    }
                    (_, Some(e)) => {
                        ui.colored_label(warn, e);
                    }
                    _ => {}
                }
                if let Some(e) = &w.sheet.sidecar_error {
                    ui.colored_label(warn, format!("AnvilCAD property file: {e}"));
                }

                let rows = w.sheet.rows();
                let file_rows: Vec<_> = rows.iter().filter(|r| r.origin.is_file()).collect();
                ui.add_space(4.0);
                ui.label(RichText::new("Properties in the file").strong());
                if file_rows.is_empty() {
                    ui.weak("None found.");
                } else {
                    egui::Grid::new("file_props_grid").striped(true).num_columns(3).show(ui, |ui| {
                        ui.strong("Property Name");
                        ui.strong("Value");
                        ui.strong("Applies to");
                        ui.end_row();
                        for r in &file_rows {
                            ui.label(&r.name);
                            let v = ui.label(&r.value);
                            if let Some(e) = &r.expression {
                                v.on_hover_text(format!("Text expression: {e}"));
                            }
                            ui.weak(
                                r.configuration
                                    .as_deref()
                                    .map(|c| format!("Configuration {c}"))
                                    .unwrap_or("File".into()),
                            );
                            ui.end_row();
                        }
                    });
                }

                ui.add_space(6.0);
                ui.label(RichText::new("AnvilCAD properties").strong());
                ui.weak(format!(
                    "Kept beside the file in {}, so the file stays untouched.",
                    w.sheet.sidecar_path.file_name().map(|n| n.to_string_lossy()).unwrap_or_default()
                ));
                let mut del = None;
                let names: Vec<String> = file_rows
                    .iter()
                    .filter(|r| r.configuration.is_none())
                    .map(|r| r.name.to_lowercase())
                    .collect();
                egui::Grid::new("anvil_props_grid").striped(true).num_columns(4).show(ui, |ui| {
                    for (i, p) in w.sheet.sidecar.properties.iter_mut().enumerate() {
                        w.unsaved |=
                            ui.add_sized([130.0, 20.0], egui::TextEdit::singleline(&mut p.name)).changed();
                        egui::ComboBox::from_id_salt(("fptype", i))
                            .selected_text(p.kind.label())
                            .width(90.0)
                            .show_ui(ui, |ui| {
                                for k in PropertyKind::ALL {
                                    w.unsaved |= ui.selectable_value(&mut p.kind, k, k.label()).changed();
                                }
                            });
                        w.unsaved |=
                            ui.add_sized([220.0, 20.0], egui::TextEdit::singleline(&mut p.value)).changed();
                        ui.horizontal(|ui| {
                            if ui.small_button("x").clicked() {
                                del = Some(i);
                            }
                            if names.contains(&p.name.to_lowercase()) {
                                ui.colored_label(warn, "(!)").on_hover_text(
                                    "The file has a property with this name too. Both are kept; the file's \
                                     own value is what other programs show.",
                                );
                            }
                        });
                        ui.end_row();
                    }
                });
                if let Some(i) = del {
                    w.sheet.sidecar.properties.remove(i);
                    w.unsaved = true;
                }
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut w.new_name)
                            .hint_text("new property name")
                            .desired_width(160.0),
                    );
                    if ui.button("Add").clicked() && !w.new_name.trim().is_empty() {
                        w.sheet.sidecar.properties.push(CustomProperty {
                            name: w.new_name.trim().into(),
                            kind: PropertyKind::Text,
                            value: String::new(),
                        });
                        w.new_name.clear();
                        w.unsaved = true;
                    }
                });
                ui.horizontal(|ui| {
                    if ui.add_enabled(w.unsaved, egui::Button::new("Save AnvilCAD properties")).clicked() {
                        match w.sheet.save() {
                            Ok(()) => {
                                w.unsaved = false;
                                w.status = Some(format!("Saved to {}", w.sheet.sidecar_path.display()));
                            }
                            Err(e) => w.status = Some(format!("Save failed: {e}")),
                        }
                    }
                    if w.unsaved {
                        ui.colored_label(warn, "Unsaved changes");
                        if w.status.as_deref() == Some(CLOSE_UNSAVED) && ui.button("Discard").clicked() {
                            discard = true;
                        }
                    }
                });
                if let Some(s) = &w.status {
                    ui.weak(s);
                }

                if let Some((_, fp)) = &w.sheet.file {
                    if !fp.summary.is_empty() {
                        ui.collapsing("Summary", |ui| {
                            egui::Grid::new("file_summary").num_columns(2).show(ui, |ui| {
                                for s in &fp.summary {
                                    ui.label(&s.name);
                                    ui.label(&s.value);
                                    ui.end_row();
                                }
                            });
                        });
                    }
                    if !fp.notes.is_empty() {
                        ui.collapsing(format!("Notes ({})", fp.notes.len()), |ui| {
                            for n in &fp.notes {
                                ui.label(n);
                            }
                        });
                    }
                }
                ui.separator();
                ui.weak(&w.about);
            });
        if discard {
            self.file_props = None;
        } else if !open {
            if w.unsaved {
                w.status = Some(CLOSE_UNSAVED.into());
            } else {
                self.file_props = None;
            }
        }
    }

    fn windows(&mut self, ctx: &egui::Context) {
        let mut open = self.windows.properties;
        egui::Window::new("Properties").open(&mut open).default_width(460.0).show(ctx, |ui| {
            ui.label(RichText::new("Material").strong());
            let current = self
                .doc
                .material
                .as_ref()
                .map(|m| m.name.clone())
                .unwrap_or_else(|| "<not specified>".into());
            egui::ComboBox::from_id_salt("material").selected_text(current).show_ui(ui, |ui| {
                if ui.selectable_label(self.doc.material.is_none(), "<not specified>").clicked() {
                    self.doc.material = None;
                }
                for class in [MaterialClass::Metal, MaterialClass::Plastic] {
                    ui.separator();
                    ui.weak(class.label());
                    for m in Material::library().into_iter().filter(|m| m.class == class) {
                        let sel = self.doc.material.as_ref().is_some_and(|c| c.name == m.name);
                        if ui.selectable_label(sel, format!("{}  ({} g/cm³)", m.name, m.density)).clicked() {
                            self.doc.material = Some(m);
                        }
                    }
                }
            });
            if let Some(m) = &mut self.doc.material {
                if m.min_wall == 0.0 {
                    *m = m.resolved();
                }
                egui::Grid::new("material").num_columns(2).striped(true).show(ui, |ui| {
                    ui.label("Density (g/cm³)");
                    ui.add(egui::DragValue::new(&mut m.density).speed(0.01).range(0.0..=100.0));
                    ui.end_row();
                    let known = |v: f64, unit: &str| if v > 0.0 { format!("{v} {unit}") } else { "-".into() };
                    for (label, value) in [
                        ("Class", m.class.label().to_string()),
                        ("Yield strength", known(m.yield_strength, "MPa")),
                        ("Tensile strength", known(m.tensile_strength, "MPa")),
                        ("Elastic modulus", known(m.elastic_modulus, "GPa")),
                        ("Thermal conductivity", known(m.thermal_conductivity, "W/(m·K)")),
                        ("Thinnest machined wall", known(m.min_wall, "mm")),
                        (
                            "Smallest bend radius",
                            m.min_bend_ratio
                                .map(|r| format!("{r} x thickness"))
                                .unwrap_or_else(|| "not bent as sheet".into()),
                        ),
                    ] {
                        ui.label(label);
                        ui.label(value);
                        ui.end_row();
                    }
                });
                ui.weak("Nominal datasheet values; a supplier's certificate governs a real part.");
            }
            ui.separator();
            ui.label(RichText::new("Custom Properties").strong());
            let mut del = None;
            egui::Grid::new("props").striped(true).num_columns(4).show(ui, |ui| {
                ui.strong("Property Name");
                ui.strong("Type");
                ui.strong("Value / Text Expression");
                ui.end_row();
                for (i, p) in self.doc.custom_properties.iter_mut().enumerate() {
                    ui.add(egui::TextEdit::singleline(&mut p.name).desired_width(120.0));
                    egui::ComboBox::from_id_salt(("ptype", i))
                        .selected_text(p.kind.label())
                        .width(90.0)
                        .show_ui(ui, |ui| {
                            for k in PropertyKind::ALL {
                                ui.selectable_value(&mut p.kind, k, k.label());
                            }
                        });
                    match p.kind {
                        PropertyKind::YesNo => {
                            let mut b = p.value.eq_ignore_ascii_case("yes");
                            if ui.checkbox(&mut b, "").changed() {
                                p.value = if b { "Yes".into() } else { "No".into() };
                            }
                        }
                        _ => {
                            ui.add(egui::TextEdit::singleline(&mut p.value).desired_width(180.0));
                        }
                    }
                    if ui.small_button("x").clicked() {
                        del = Some(i);
                    }
                    ui.end_row();
                }
            });
            if let Some(i) = del {
                self.doc.custom_properties.remove(i);
            }
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.new_prop)
                        .hint_text("new property name")
                        .desired_width(160.0),
                );
                if ui.button("Add").clicked() && !self.new_prop.trim().is_empty() {
                    self.doc.custom_properties.push(CustomProperty {
                        name: self.new_prop.trim().into(),
                        kind: PropertyKind::Text,
                        value: String::new(),
                    });
                    self.new_prop.clear();
                }
            });
            ui.separator();
            let m = self.state.mass_report(&self.doc);
            ui.weak(format!(
                "Evaluated: Volume = {:.2} mm³{}",
                m.props.volume,
                m.mass.map(|g| format!(", Mass = {g:.2} g")).unwrap_or_default()
            ));
        });
        self.windows.properties = open;

        let mut open = self.windows.equations;
        let mut changed = false;
        egui::Window::new("Equations - Global Variables").open(&mut open).default_width(420.0).show(
            ctx,
            |ui| {
                let mut del = None;
                let mut renamed = Vec::new();
                egui::Grid::new("vars").striped(true).num_columns(4).show(ui, |ui| {
                    ui.strong("Name");
                    ui.strong("Value / Equation");
                    ui.strong("Evaluates to");
                    ui.end_row();
                    for (i, v) in self.doc.variables.iter_mut().enumerate() {
                        let old = v.name.clone();
                        if ui.add(egui::TextEdit::singleline(&mut v.name).desired_width(100.0)).changed() {
                            renamed.push((old, v.name.clone()));
                            changed = true;
                        }
                        changed |=
                            ui.add(egui::TextEdit::singleline(&mut v.expr).desired_width(140.0)).changed();
                        match (self.state.variables.get(&v.name), self.state.variable_errors.get(&v.name)) {
                            (Some(x), _) => {
                                ui.label(sketcher::format_num(*x));
                            }
                            (_, Some(e)) => {
                                ui.colored_label(Color32::from_rgb(200, 30, 30), "error").on_hover_text(e);
                            }
                            _ => {
                                ui.label("");
                            }
                        }
                        if ui.small_button("x").clicked() {
                            del = Some(i);
                        }
                        ui.end_row();
                    }
                });
                for (old, new) in renamed {
                    self.doc.rename_configured_variable(&old, &new);
                }
                if let Some(i) = del {
                    self.doc.variables.remove(i);
                    changed = true;
                }
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.new_var)
                            .hint_text("new variable")
                            .desired_width(120.0),
                    );
                    if ui.button("Add").clicked() && !self.new_var.trim().is_empty() {
                        self.doc.variables.push(GlobalVariable {
                            name: self.new_var.trim().into(),
                            expr: "0".into(),
                            comment: String::new(),
                        });
                        self.new_var.clear();
                        changed = true;
                    }
                });
                ui.weak("Link a dimension to a variable by typing =name in its value box.");
            },
        );
        self.windows.equations = open;
        if changed {
            self.rebuild();
        }

        let mut open = self.windows.mass;
        egui::Window::new("Mass Properties").open(&mut open).show(ctx, |ui| {
            let m = self.state.mass_report(&self.doc);
            let b = self.state.body.bounds();
            egui::Grid::new("mass").num_columns(2).show(ui, |ui| {
                ui.label("Material");
                ui.label(
                    self.doc
                        .material
                        .as_ref()
                        .map(|m| m.name.clone())
                        .unwrap_or_else(|| "<not specified>".into()),
                );
                ui.end_row();
                ui.label("Mass");
                ui.label(
                    m.mass
                        .map(|g| format!("{g:.3} g  ({:.4} kg, {:.4} lb)", g / 1000.0, g / 453.592_37))
                        .unwrap_or_else(|| "assign a material".into()),
                );
                ui.end_row();
                ui.label("Volume");
                ui.label(format!("{:.3} mm³", m.props.volume));
                ui.end_row();
                ui.label("Surface area");
                ui.label(format!("{:.3} mm²", m.props.surface_area));
                ui.end_row();
                let c = m.props.centroid;
                ui.label("Center of mass");
                ui.label(format!("X = {:.3}  Y = {:.3}  Z = {:.3} mm", c.x, c.y, c.z));
                ui.end_row();
                if let Some(p) = m.principal {
                    ui.label("Principal moments");
                    ui.label(format!(
                        "{:.4e}  {:.4e}  {:.4e} g·mm² (about the center of mass)",
                        p[0], p[1], p[2]
                    ));
                    ui.end_row();
                }
                if let Some(t) = m.inertia {
                    ui.label("Inertia tensor");
                    ui.label(format!(
                        "Ixx {:.4e}  Iyy {:.4e}  Izz {:.4e}\nIxy {:.4e}  Iyz {:.4e}  Izx {:.4e} g·mm²",
                        t[0][0], t[1][1], t[2][2], t[0][1], t[1][2], t[2][0]
                    ));
                    ui.end_row();
                }
                if !b.is_empty() {
                    let s = b.max - b.min;
                    ui.label("Bounding box");
                    ui.label(format!("{:.3} x {:.3} x {:.3} mm", s.x, s.y, s.z));
                    ui.end_row();
                }
            });
            ui.weak("Curved faces are faceted to 0.01 mm chord tolerance.");
        });
        self.windows.mass = open;

        let mut open = self.windows.dfm;
        let mut rerun = false;
        let mut focus = self.dfm.focus;
        egui::Window::new("Check Design").open(&mut open).default_width(520.0).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Process");
                for p in dfm::Process::ALL {
                    if ui.selectable_label(self.dfm.process == p, p.label()).clicked()
                        && self.dfm.process != p
                    {
                        self.dfm.process = p;
                        rerun = true;
                    }
                }
                if ui.button("Run").clicked() {
                    rerun = true;
                }
            });
            let material = self.doc.material.as_ref().map(|m| m.name.clone());
            ui.label(match &material {
                Some(name) => format!("Material: {name}"),
                None => "No material assigned: general limits are used.".into(),
            });
            if self.dfm.stale || (self.dfm.report.is_some() && material != self.dfm.material) {
                ui.colored_label(
                    Color32::from_rgb(200, 120, 0),
                    "The part changed since this check; run it again.",
                );
            }
            let Some(report) = &self.dfm.report else { return };
            ui.separator();
            let (e, w, n) = (
                report.count(dfm::Severity::Error),
                report.count(dfm::Severity::Warning),
                report.count(dfm::Severity::Note),
            );
            let mut summary = format!("{e} errors, {w} warnings, {n} notes");
            if let Some(t) = report.thickness {
                summary += &format!(" · sheet {t:.2} mm thick, {} bends", report.bends);
            }
            ui.strong(summary);
            if report.findings.is_empty() {
                ui.label("Nothing to flag for this process.");
            }
            egui::ScrollArea::vertical().max_height(380.0).show(ui, |ui| {
                for (i, f) in report.findings.iter().enumerate() {
                    let color = match f.severity {
                        dfm::Severity::Error => Color32::from_rgb(200, 40, 40),
                        dfm::Severity::Warning => Color32::from_rgb(200, 120, 0),
                        dfm::Severity::Note => Color32::from_rgb(90, 90, 100),
                    };
                    ui.horizontal_wrapped(|ui| {
                        ui.colored_label(color, f.severity.label());
                        let text = RichText::new(f.rule).strong();
                        if ui
                            .selectable_label(focus == Some(i), text)
                            .on_hover_text("Show its faces")
                            .clicked()
                        {
                            focus = if focus == Some(i) { None } else { Some(i) };
                        }
                        if f.count > 1 {
                            ui.strong(format!("x{}", f.count));
                        }
                        ui.label(&f.message);
                    });
                }
            });
            ui.weak("Limits are common shop rules of thumb; check them with your supplier.");
        });
        if open && rerun {
            self.run_dfm();
        } else if focus != self.dfm.focus {
            self.dfm.focus = focus;
            self.scene_dirty = true;
        }
        if open != self.windows.dfm {
            self.scene_dirty = true;
        }
        self.windows.dfm = open;

        let mut open = self.windows.about;
        egui::Window::new("About AnvilCAD").open(&mut open).show(ctx, |ui| {
            ui.heading(format!("AnvilCAD {}", env!("CARGO_PKG_VERSION")));
            ui.label("Free and open source parametric 3D CAD, written in Rust.");
            ui.label("Licensed under MIT OR Apache-2.0.");
        });
        self.windows.about = open;

        self.file_props_window(ctx);

        if let Some((title, body)) = self.message.clone() {
            egui::Window::new(title)
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.set_max_width(420.0);
                    ui.label(body);
                    if ui.button("OK").clicked() {
                        self.message = None;
                    }
                });
        }

        if let Some(action) = self.confirm {
            egui::Window::new("Unsaved changes")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label(format!("Save changes to {}?", self.part_name()));
                    ui.horizontal(|ui| {
                        if ui.button("Save").clicked() {
                            self.confirm = None;
                            self.save(false);
                            if !self.is_dirty() {
                                self.perform(action);
                            }
                        }
                        if ui.button("Don't Save").clicked() {
                            self.confirm = None;
                            self.perform(action);
                        }
                        if ui.button("Cancel").clicked() {
                            self.confirm = None;
                        }
                    });
                });
        }
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.wants_keyboard_input() {
            return;
        }
        let ctrl = |k: Key| ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, k));
        if ctrl(Key::Z) {
            self.undo();
        }
        if ctrl(Key::Y) {
            self.redo();
        }
        if ctrl(Key::S) {
            self.save(false);
        }
        if ctrl(Key::O) {
            self.request(PendingAction::Open);
        }
        if ctrl(Key::N) {
            self.request(PendingAction::New);
        }
        if ctrl(Key::B) {
            self.rebuild();
        }
        let tabs = [Tab::Features, Tab::Sketch, Tab::Evaluate];
        let at = tabs.iter().position(|t| *t == self.tab).unwrap_or(0);
        if ctrl(Key::PageDown) {
            self.tab = tabs[(at + 1) % tabs.len()];
        }
        if ctrl(Key::PageUp) {
            self.tab = tabs[(at + tabs.len() - 1) % tabs.len()];
        }
        let keys = [Key::Num1, Key::Num2, Key::Num3, Key::Num4, Key::Num5, Key::Num6, Key::Num7];
        for (k, v) in keys.iter().zip(StdView::ALL) {
            if ctrl(*k) {
                self.camera.set_view(v);
                self.fit_pending = true;
            }
        }
        if ctrl(Key::Num8) {
            if let Some(s) = &self.sketch {
                let f = s.frame;
                self.camera.normal_to(&f);
            }
        }
        let (f, del, esc, arrows) = ctx.input(|i| {
            (
                i.key_pressed(Key::F),
                i.key_pressed(Key::Delete),
                i.key_pressed(Key::Escape),
                (
                    i.key_pressed(Key::ArrowLeft) as i32 - i.key_pressed(Key::ArrowRight) as i32,
                    i.key_pressed(Key::ArrowUp) as i32 - i.key_pressed(Key::ArrowDown) as i32,
                ),
            )
        });
        if f {
            self.fit_pending = true;
        }
        if arrows != (0, 0) {
            self.camera.orbit(arrows.0 as f64 * -20.0, arrows.1 as f64 * -20.0);
        }
        if self.sketch.is_none() {
            if del && matches!(self.selection, Selection::Feature(_)) && self.editor.is_none() {
                self.delete_selected_feature();
            }
            if esc {
                if matches!(self.editor, Some(Editor::PickPlane(_))) {
                    self.editor = None;
                } else if self.editor.is_none() {
                    self.selection = Selection::None;
                    self.scene_dirty = true;
                }
            }
        }
    }

    fn handle_screenshot(&mut self, ctx: &egui::Context) {
        let Some((path, frames)) = &mut self.screenshot else { return };
        *frames += 1;
        if *frames == 8 {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
        }
        let path = path.clone();
        let shot = ctx.input(|i| {
            i.raw.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(img) = shot {
            let file = std::fs::File::create(&path).expect("create screenshot");
            let mut enc =
                png::Encoder::new(std::io::BufWriter::new(file), img.size[0] as u32, img.size[1] as u32);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            let mut w = enc.write_header().expect("png header");
            w.write_image_data(img.as_raw()).expect("png data");
            drop(w);
            self.allow_close = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        ctx.request_repaint();
    }
}

// ----- headless harness API --------------------------------------------------------------

impl AnvilApp {
    /// Answers file dialogs from [`AnvilApp::queue_file_dialog`] instead of opening native
    /// dialogs. Used by the headless harness so a run never opens a window.
    pub fn script_file_dialogs(&mut self) {
        self.scripted_dialogs.get_or_insert_with(VecDeque::new);
    }

    /// Queues the answer to the next file dialog: a path, or `None` to cancel it.
    pub fn queue_file_dialog(&mut self, answer: Option<PathBuf>) {
        self.scripted_dialogs.get_or_insert_with(VecDeque::new).push_back(answer);
    }

    /// The file dialogs asked for since the last call.
    pub fn take_dialog_requests(&mut self) -> Vec<DialogRequest> {
        std::mem::take(&mut self.dialog_log)
    }

    /// Opens a part, template or STEP file, as File > Open would (asking nothing).
    pub fn open_file(&mut self, p: &Path) {
        self.open_path(p);
    }

    /// Replaces the document with the sample part.
    pub fn open_sample(&mut self) {
        self.load_document(crate::sample::sample_part(), None);
    }

    /// Replaces the document with an empty part.
    pub fn new_document(&mut self) {
        self.load_document(Document::new(), None);
    }

    /// The part being edited.
    pub fn document(&self) -> &Document {
        &self.doc
    }

    /// The viewport's rectangle in points, once a frame has been drawn.
    pub fn viewport_rect(&self) -> egui::Rect {
        self.view_rect
    }

    /// Screen position (in points) of a model point.
    pub fn project(&self, p: Vec3) -> egui::Pos2 {
        self.camera.project(p, self.view_rect)
    }

    /// Screen position (in points) of a point in the sketch being edited.
    pub fn project_sketch(&self, x: f64, y: f64) -> Option<egui::Pos2> {
        let s = self.sketch.as_ref()?;
        Some(self.camera.project(s.frame.to_world(Vec2 { x, y }), self.view_rect))
    }

    /// A summary of what the user would see, as JSON: the feature tree, rebuild errors,
    /// the body's mass properties, the open sketch, editor, windows and dialogs.
    pub fn snapshot(&self) -> serde_json::Value {
        use serde_json::json;
        let features: Vec<_> = self
            .doc
            .features
            .iter()
            .enumerate()
            .map(|(i, f)| {
                json!({
                    "id": f.id,
                    "name": f.name,
                    "kind": kind_name(&f.kind),
                    "suppressed": f.suppressed,
                    "rolled_back": self.doc.rollback.is_some_and(|r| i >= r),
                    "error": self.state.errors.get(&f.id),
                    "sketch_status": self.state.sketch_status.get(&f.id).map(|s| s.status_text()),
                })
            })
            .collect();
        let body = &self.state.body;
        let body = if body.is_empty() {
            json!({ "empty": true })
        } else {
            let m = body.mass_properties();
            let b = body.bounds();
            json!({
                "empty": false,
                "volume": m.volume,
                "surface_area": m.surface_area,
                "centroid": [m.centroid.x, m.centroid.y, m.centroid.z],
                "bounds": { "min": [b.min.x, b.min.y, b.min.z], "max": [b.max.x, b.max.y, b.max.z] },
                "open_edges": body.open_edges(),
                "exact": self.state.exact.is_some(),
                "triangles": self.mesh.indices.len(),
            })
        };
        let sketch = self.sketch.as_ref().map(|s| {
            let sk = self.doc.sketch(s.feature).map(|f| &f.sketch);
            json!({
                "feature": s.feature,
                "tool": s.tool.label(),
                "status": s.status.status_text(),
                "dof": s.status.dof,
                "points": sk.map(|k| k.points.len()),
                "entities": sk.map(|k| k.entities.len()),
                "constraints": sk.map(|k| k.constraints.len()),
                "selected": s.selection.len(),
                "editing_dimension": s.dim_edit.as_ref().map(|(i, text)| json!({ "constraint": i, "text": text })),
                "dimension_picks": s.picks.len(),
                "over_defining_dimension": s.over_define,
                "reference_dimensions": sk.map(|k| k.annotations.iter().filter(|a| a.reference).count()),
            })
        });
        let editor = match &self.editor {
            None => serde_json::Value::Null,
            Some(Editor::PickPlane(p)) => json!({ "picking_plane_for": format!("{p:?}") }),
            Some(Editor::Feature { id, is_new, .. }) => json!({
                "feature": id,
                "kind": self.doc.feature(*id).map(|f| kind_name(&f.kind)),
                "new": is_new,
            }),
        };
        let configurations: Vec<_> = self
            .doc
            .configuration_tree()
            .into_iter()
            .filter_map(|(id, _)| self.doc.configuration(id))
            .map(|c| {
                json!({
                    "id": c.id,
                    "name": c.name,
                    "active": c.id == self.doc.active_configuration,
                    "parent": c.parent.and_then(|p| self.doc.configuration(p)).map(|p| p.name.clone()),
                    "description": c.description,
                    "suppress_new_features": c.suppress_new_features,
                })
            })
            .collect();
        let configured: Vec<_> = self
            .doc
            .configured
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let values: serde_json::Map<String, serde_json::Value> = self
                    .doc
                    .configurations
                    .iter()
                    .map(|c| (c.name.clone(), json!(self.doc.resolved_value(i, c.id).map(|v| v.text()))))
                    .collect();
                json!({ "parameter": self.doc.param_label(&p.param), "values": values })
            })
            .collect();
        let w = &self.windows;
        let r = self.view_rect;
        json!({
            "title": self.title(),
            "path": self.path.as_ref().map(|p| p.display().to_string()),
            "dirty": self.is_dirty(),
            "status": self.status,
            "tab": format!("{:?}", self.tab),
            "selection": format!("{:?}", self.selection),
            "features": features,
            "errors": self.state.errors.len(),
            "body": body,
            "material": self.doc.material.as_ref().map(|m| m.name.clone()),
            "variables": self.state.variables,
            "manager": format!("{:?}", self.cfg.manager),
            "configurations": configurations,
            "active_configuration": self.doc.active_configuration_name(),
            "configured": configured,
            "sketch": sketch,
            "editor": editor,
            "windows": {
                "properties": w.properties, "equations": w.equations, "mass": w.mass,
                "dfm": w.dfm, "about": w.about,
                "configuration_table": self.cfg.table,
                "configuration_properties": self.cfg.props.is_some(),
            },
            "message": self.message.as_ref().map(|(t, m)| json!({ "title": t, "text": m })),
            "confirm": self.confirm.map(|a| format!("{a:?}")),
            "undo": self.undo.len(),
            "redo": self.redo.len(),
            "viewport": [r.min.x, r.min.y, r.max.x, r.max.y],
        })
    }
}

fn kind_name(k: &FeatureKind) -> &'static str {
    match k {
        FeatureKind::Sketch(_) => "Sketch",
        FeatureKind::Extrude(e) if e.op == BodyOp::Cut => "ExtrudedCut",
        FeatureKind::Extrude(_) => "Extrude",
        FeatureKind::Revolve(r) if r.op == BodyOp::Cut => "RevolvedCut",
        FeatureKind::Revolve(_) => "Revolve",
        FeatureKind::Plane(_) => "Plane",
        FeatureKind::LinearPattern(_) => "LinearPattern",
        FeatureKind::CircularPattern(_) => "CircularPattern",
        FeatureKind::ImportedBody(_) => "ImportedBody",
        FeatureKind::Fillet(_) => "Fillet",
        FeatureKind::Chamfer(_) => "Chamfer",
        FeatureKind::Shell(_) => "Shell",
        FeatureKind::Hole(_) => "Hole",
        FeatureKind::Mirror(_) => "Mirror",
        FeatureKind::SketchPattern(_) => "SketchPattern",
    }
}

fn icon(k: &FeatureKind) -> Icon {
    match k {
        FeatureKind::Sketch(_) => Icon::Sketch,
        FeatureKind::Extrude(e) if e.op == BodyOp::Cut => Icon::ExtrudeCut,
        FeatureKind::Extrude(_) => Icon::Extrude,
        FeatureKind::Revolve(r) if r.op == BodyOp::Cut => Icon::RevolveCut,
        FeatureKind::Revolve(_) => Icon::Revolve,
        FeatureKind::Plane(_) => Icon::Plane,
        FeatureKind::LinearPattern(_) => Icon::LinearPattern,
        FeatureKind::CircularPattern(_) => Icon::CircularPattern,
        FeatureKind::ImportedBody(_) => Icon::Imported,
        FeatureKind::Fillet(_) => Icon::Fillet,
        FeatureKind::Chamfer(_) => Icon::Chamfer,
        FeatureKind::Shell(_) => Icon::Shell,
        FeatureKind::Hole(_) => Icon::Hole,
        FeatureKind::Mirror(_) => Icon::LinearPattern,
        FeatureKind::SketchPattern(_) => Icon::LinearPattern,
    }
}

/// The hole a new Hole command makes: a through clearance hole for an M6 screw.
fn default_hole(sketch: u32) -> HoleFeature {
    HoleFeature::standard(sketch, 0, hole::Standard::IsoMetric, "M6", hole::Fit::Normal).unwrap_or(
        HoleFeature {
            sketch,
            kind: HoleKind::Simple,
            diameter: Dim::Value(6.6),
            end: HoleEnd::ThroughAll,
            depth: Dim::Value(18.0),
            drill_angle: Some(Dim::Value(hole::DRILL_ANGLE)),
            near_sink: None,
            reverse: false,
            size: None,
        },
    )
}

/// The area-weighted middle of a face of the faceted body.
fn face_centroid(body: &anvil_kernel::Solid, face: FaceId) -> Vec3 {
    let (mut sum, mut area) = (Vec3::ZERO, 0.0);
    for p in body.face_polygons(face) {
        for i in 1..p.verts.len().saturating_sub(1) {
            let (a, b, c) = (p.verts[0], p.verts[i], p.verts[i + 1]);
            let w = (b - a).cross(c - a).length();
            sum += (a + b + c) * (w / 3.0);
            area += w;
        }
    }
    if area > 0.0 {
        sum * (1.0 / area)
    } else {
        Vec3::ZERO
    }
}

/// Text buffers for the hole editor's dimension fields.
fn hole_bufs(h: &HoleFeature) -> HashMap<&'static str, String> {
    let mut b = HashMap::new();
    b.insert("diameter", h.diameter.text());
    b.insert("depth", h.depth.text());
    if let Some(a) = &h.drill_angle {
        b.insert("drill_angle", a.text());
    }
    if let Some(s) = &h.near_sink {
        b.insert("near_diameter", s.diameter.text());
        b.insert("near_angle", s.angle.text());
    }
    match &h.kind {
        HoleKind::Simple => {}
        HoleKind::Counterbore { diameter, depth } => {
            b.insert("cb_diameter", diameter.text());
            b.insert("cb_depth", depth.text());
        }
        HoleKind::Countersink(s) => {
            b.insert("cs_diameter", s.diameter.text());
            b.insert("cs_angle", s.angle.text());
        }
        HoleKind::Tapped { major, pitch, thread_depth } => {
            b.insert("major", major.text());
            b.insert("pitch", pitch.text());
            b.insert("thread_depth", thread_depth.text());
        }
    }
    b
}

/// The hole's property panel: type, standard size, dimensions, end and options.
fn hole_editor(
    ui: &mut egui::Ui,
    h: &mut HoleFeature,
    bufs: &mut HashMap<&'static str, String>,
    vars: &std::collections::BTreeMap<String, f64>,
    points: usize,
    edit_positions: &mut bool,
) -> bool {
    use hole::{Fit, Standard};
    let mut changed = false;
    let mut resized = false;
    let current = h.kind.index();
    egui::ComboBox::from_label("Hole type").selected_text(h.kind.label()).show_ui(ui, |ui| {
        for (i, label) in HoleKind::LABELS.iter().enumerate() {
            if ui.selectable_label(current == i, *label).clicked() && current != i {
                h.set_kind(i);
                resized = true;
            }
        }
    });

    // Standard, size and fit fill the dimensions below; any of them can then be changed.
    let standard = h.size.as_ref().map(|z| z.standard).unwrap_or_default();
    let mut pick: Option<(Standard, String, Fit)> = None;
    egui::ComboBox::from_label("Standard").selected_text(standard.label()).show_ui(ui, |ui| {
        for st in Standard::ALL {
            if ui.selectable_label(st == standard, st.label()).clicked() && st != standard {
                if let Some(first) = st.sizes().first() {
                    pick = Some((st, first.name.to_string(), Fit::Normal));
                }
            }
        }
    });
    let size_name = h.size.as_ref().map(|z| z.size.clone()).unwrap_or_else(|| "Custom".into());
    let fit = h.size.as_ref().map(|z| z.fit).unwrap_or_default();
    egui::ComboBox::from_label("Size").selected_text(&size_name).show_ui(ui, |ui| {
        for sz in standard.sizes() {
            if ui.selectable_label(sz.name == size_name, sz.name).clicked() && sz.name != size_name {
                pick = Some((standard, sz.name.to_string(), fit));
            }
        }
    });
    if !matches!(h.kind, HoleKind::Tapped { .. }) && h.size.is_some() {
        egui::ComboBox::from_label("Fit").selected_text(fit.label()).show_ui(ui, |ui| {
            for f in Fit::ALL {
                if ui.selectable_label(f == fit, f.label()).clicked() && f != fit {
                    pick = Some((standard, size_name.clone(), f));
                }
            }
        });
    }
    if let Some((st, size, fit)) = pick {
        if let Some(mut n) = HoleFeature::standard(h.sketch, h.kind.index(), st, &size, fit) {
            // Keep the placement choices; take the standard's sizes.
            n.reverse = h.reverse;
            if !matches!(h.kind, HoleKind::Tapped { .. }) {
                n.end = h.end;
                n.depth = h.depth.clone();
            }
            n.near_sink = h.near_sink.as_ref().and(n.near_sink.clone().or_else(|| default_near_sink(&n)));
            *h = n;
            resized = true;
        }
    }

    ui.add_space(4.0);
    let label = if matches!(h.kind, HoleKind::Tapped { .. }) { "Tap drill (mm)" } else { "Diameter (mm)" };
    let field = |ui: &mut egui::Ui,
                 label: &str,
                 dim: &mut Dim,
                 key: &'static str,
                 bufs: &mut HashMap<&'static str, String>| {
        AnvilApp::dim_field(ui, label, dim, bufs.entry(key).or_insert_with(|| dim.text()), vars)
    };
    let mut typed = field(ui, label, &mut h.diameter, "diameter", bufs);
    match &mut h.kind {
        HoleKind::Simple => {}
        HoleKind::Counterbore { diameter, depth } => {
            typed |= field(ui, "Counterbore diameter (mm)", diameter, "cb_diameter", bufs);
            typed |= field(ui, "Counterbore depth (mm)", depth, "cb_depth", bufs);
        }
        HoleKind::Countersink(s) => {
            typed |= field(ui, "Countersink diameter (mm)", &mut s.diameter, "cs_diameter", bufs);
            typed |= field(ui, "Countersink angle (deg)", &mut s.angle, "cs_angle", bufs);
        }
        HoleKind::Tapped { major, pitch, thread_depth } => {
            typed |= field(ui, "Thread diameter (mm)", major, "major", bufs);
            typed |= field(ui, "Pitch (mm)", pitch, "pitch", bufs);
            typed |= field(ui, "Thread depth (mm)", thread_depth, "thread_depth", bufs);
            ui.weak("The thread is recorded on the hole; the solid shows the tap drill.");
        }
    }

    ui.add_space(4.0);
    egui::ComboBox::from_label("End condition").selected_text(h.end.label()).show_ui(ui, |ui| {
        for e in HoleEnd::ALL {
            changed |= ui.selectable_value(&mut h.end, e, e.label()).changed();
        }
    });
    if h.end == HoleEnd::Blind {
        typed |= field(ui, "Depth to shoulder (mm)", &mut h.depth, "depth", bufs);
        let mut point = h.drill_angle.is_some();
        if ui.checkbox(&mut point, "Drill point").changed() {
            h.drill_angle = point.then_some(Dim::Value(hole::DRILL_ANGLE));
            bufs.insert("drill_angle", hole::DRILL_ANGLE.to_string());
            changed = true;
        }
        if let Some(a) = &mut h.drill_angle {
            typed |= field(ui, "Drill point angle (deg)", a, "drill_angle", bufs);
        }
    }
    if !matches!(h.kind, HoleKind::Countersink(_)) {
        let mut near = h.near_sink.is_some();
        if ui.checkbox(&mut near, "Near-side countersink").changed() {
            h.near_sink = if near { default_near_sink(h) } else { None };
            resized = true;
        }
        if let Some(s) = &mut h.near_sink {
            typed |= field(ui, "Countersink diameter (mm)", &mut s.diameter, "near_diameter", bufs);
            typed |= field(ui, "Countersink angle (deg)", &mut s.angle, "near_angle", bufs);
        }
    }
    changed |= ui.checkbox(&mut h.reverse, "Reverse direction").changed();
    // A size typed over by hand is no longer the standard's.
    if typed {
        if let Some(z) = &mut h.size {
            z.size = format!("{} (custom)", z.size.trim_end_matches(" (custom)"));
        }
    }

    ui.add_space(4.0);
    ui.label(match points {
        1 => "1 hole, at the sketch's point.".to_string(),
        n => format!("{n} holes, one at each of the sketch's points."),
    });
    if ui.button("Edit positions").clicked() {
        *edit_positions = true;
    }
    if resized {
        *bufs = hole_bufs(h);
    }
    changed || typed || resized
}

/// A near-side countersink a little wider than the bore it opens.
fn default_near_sink(h: &HoleFeature) -> Option<hole::Sink> {
    let bore = match &h.kind {
        HoleKind::Counterbore { diameter: Dim::Value(d), .. } => *d,
        HoleKind::Tapped { major: Dim::Value(m), .. } => *m,
        _ => match h.diameter {
            Dim::Value(d) => d,
            Dim::Expr(_) => 6.0,
        },
    };
    let angle = h.size.as_ref().map_or(hole::CSINK_ANGLE, |z| z.standard.csink_angle());
    Some(hole::Sink { diameter: Dim::Value(bore + 1.0), angle: Dim::Value(angle) })
}

/// The sketch tools, in command-bar order.
const SKETCH_TOOLS: [Tool; 8] = [
    Tool::Select,
    Tool::Line,
    Tool::Centerline,
    Tool::Rectangle,
    Tool::Circle,
    Tool::Arc,
    Tool::Point,
    Tool::Dimension,
];

fn tool_icon(t: Tool) -> Icon {
    match t {
        Tool::Select => Icon::Select,
        Tool::Line => Icon::Line,
        Tool::Centerline => Icon::Centerline,
        Tool::Rectangle => Icon::Rectangle,
        Tool::Circle => Icon::Circle,
        Tool::Arc => Icon::Arc,
        Tool::Point => Icon::Point,
        Tool::Dimension => Icon::Dimension,
    }
}

/// The title of a panel in the left pane: an icon and a name, over a rule.
fn panel_title(ui: &mut egui::Ui, ic: Icon, title: &str) {
    let pal = Palette::of(ui);
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(egui::vec2(22.0, 22.0), Sense::hover());
        theme::paint_icon(ui.painter(), r, ic, pal.text, pal.accent);
        ui.label(RichText::new(title).size(16.0).strong());
    });
    ui.add_space(2.0);
    ui.separator();
}

/// The rollback bar: features below it are not built.
fn rollback_bar(ui: &mut egui::Ui) {
    let pal = Palette::of(ui);
    let (r, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 8.0), Sense::hover());
    let bar = r.shrink2(egui::vec2(4.0, 2.5));
    ui.painter().rect_filled(bar, 2.0, pal.accent);
    ui.painter().circle_filled(bar.left_center(), 4.0, pal.accent);
}

impl AnvilApp {
    /// Runs one UI frame. Separate from `eframe::App::update` so tests can drive it.
    pub fn frame(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.viewport().close_requested()) && !self.allow_close && self.is_dirty() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.confirm = Some(PendingAction::Exit);
        }
        if self.allow_close && self.screenshot.is_none() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }

        let title = self.title();
        if title != self.last_title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.last_title = title;
        }

        self.shortcuts(ctx);

        let pal = Palette::for_dark(self.dark);
        let chrome = |m: egui::Margin| egui::Frame::NONE.fill(pal.chrome).inner_margin(m);
        egui::TopBottomPanel::top("menu")
            .frame(chrome(egui::Margin::symmetric(8, 3)))
            .show_separator_line(false)
            .show(ctx, |ui| {
                self.menu_bar(ui);
            });
        egui::TopBottomPanel::top("commands")
            .frame(chrome(egui::Margin { left: 8, right: 8, top: 0, bottom: 2 }))
            .show(ctx, |ui| {
                self.command_manager(ui);
            });
        egui::TopBottomPanel::bottom("status").frame(chrome(egui::Margin::symmetric(10, 4))).show(
            ctx,
            |ui| {
                ui.horizontal(|ui| {
                    let bad = !self.state.errors.is_empty() || !self.state.variable_errors.is_empty();
                    let (r, _) = ui.allocate_exact_size(egui::vec2(10.0, 14.0), Sense::hover());
                    ui.painter().circle_filled(r.center(), 3.5, if bad { pal.error } else { pal.ok });
                    ui.label(RichText::new(&self.status).size(12.5));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        egui::Frame::NONE
                            .stroke(Stroke::new(1.0, pal.border))
                            .corner_radius(4)
                            .inner_margin(egui::Margin::symmetric(6, 1))
                            .show(ui, |ui| {
                                ui.label(RichText::new("MMGS").size(11.5).color(pal.text_weak))
                                    .on_hover_text("Units: millimetre, gram, second");
                            });
                        if self.doc.configurations.len() > 1 {
                            ui.label(RichText::new(self.doc.active_configuration_name()).size(12.5))
                                .on_hover_text("The active configuration");
                        }
                        if let Some(s) = &self.sketch {
                            ui.separator();
                            ui.label(RichText::new(s.status.status_text()).size(12.5));
                            ui.label(RichText::new("Editing sketch").size(12.5).color(pal.accent));
                        }
                    });
                });
            },
        );
        egui::SidePanel::left("tree")
            .resizable(true)
            .default_width(270.0)
            .frame(egui::Frame::NONE.fill(pal.panel).inner_margin(egui::Margin {
                left: 10,
                right: 8,
                top: 12,
                bottom: 8,
            }))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    if self.sketch.is_some() {
                        self.sketch_manager(ui);
                    } else if self.editor.is_some() {
                        self.property_manager(ui);
                    } else {
                        self.manager_tabs(ui);
                        match self.cfg.manager {
                            Manager::Features => self.feature_tree(ui),
                            Manager::Configurations => self.configuration_manager(ui),
                        }
                    }
                });
            });
        egui::CentralPanel::default().frame(egui::Frame::NONE).show(ctx, |ui| {
            self.viewport(ui);
        });
        self.windows(ctx);
        self.config_windows(ctx);
        self.commit_if_changed();
        self.handle_screenshot(ctx);
    }
}

impl eframe::App for AnvilApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.frame(ctx);
    }

    fn on_exit(&mut self) {}
}

/// End-to-end UI tests: real egui frames driven by synthetic mouse input (no window or GPU).
#[cfg(test)]
mod ui_tests {
    use super::*;
    use anvil_kernel::v2;
    use anvil_sketch::{Constraint, Geom, ORIGIN};
    use eframe::egui::{vec2, Event, PointerButton, Pos2, RawInput, Rect};

    struct Harness {
        ctx: egui::Context,
        app: AnvilApp,
    }

    impl Harness {
        fn new() -> Harness {
            let ctx = egui::Context::default();
            let app = AnvilApp::new(&ctx, None, None, None);
            let mut h = Harness { ctx, app };
            h.step(vec![]);
            h.step(vec![]);
            h
        }

        fn step(&mut self, events: Vec<Event>) {
            let input = RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1400.0, 900.0))),
                events,
                ..Default::default()
            };
            let app = &mut self.app;
            let _ = self.ctx.run(input, |ctx| app.frame(ctx));
        }

        fn button(p: Pos2, pressed: bool) -> Event {
            Event::PointerButton {
                pos: p,
                button: PointerButton::Primary,
                pressed,
                modifiers: Default::default(),
            }
        }

        fn click(&mut self, p: Pos2) {
            self.step(vec![Event::PointerMoved(p)]);
            self.step(vec![Self::button(p, true)]);
            self.step(vec![Self::button(p, false)]);
            self.step(vec![]);
        }

        fn drag(&mut self, from: Pos2, to: Pos2) {
            self.step(vec![Event::PointerMoved(from)]);
            self.step(vec![Self::button(from, true)]);
            for i in 1..=8 {
                let t = i as f32 / 8.0;
                self.step(vec![Event::PointerMoved(from + (to - from) * t)]);
            }
            self.step(vec![Self::button(to, false)]);
            self.step(vec![]);
        }

        /// Screen position of a point in the open sketch.
        fn screen(&self, x: f64, y: f64) -> Pos2 {
            let s = self.app.sketch.as_ref().expect("sketch open");
            self.app.camera.project(s.frame.to_world(v2(x, y)), self.app.view_rect)
        }

        fn tool(&mut self, t: Tool) {
            self.app.sketch.as_mut().unwrap().set_tool(t);
            self.step(vec![]);
        }

        fn sketch(&self) -> &anvil_sketch::Sketch {
            let id = self.app.sketch.as_ref().unwrap().feature;
            &self.app.doc.sketch(id).unwrap().sketch
        }
    }

    fn line_ends(sk: &anvil_sketch::Sketch, e: usize) -> (anvil_kernel::Vec2, anvil_kernel::Vec2) {
        match sk.entities[e].geom {
            Geom::Line { a, b } => (sk.pos(a), sk.pos(b)),
            _ => panic!("not a line"),
        }
    }

    /// A 40 x 40 x 20 block from a sketch on the Front plane.
    fn block(h: &mut Harness) -> u32 {
        h.app.start_sketch(PlaneRef::Front);
        h.step(vec![]);
        let id = h.app.sketch.as_ref().unwrap().feature;
        h.app.doc.sketch_mut(id).unwrap().sketch.add_rectangle(v2(-20.0, -20.0), v2(20.0, 20.0));
        h.app.exit_sketch();
        h.app.cmd_extrude(BodyOp::Boss);
        h.step(vec![]);
        let ex = h.app.doc.features.last().unwrap().id;
        if let Some(FeatureKind::Extrude(e)) = h.app.doc.feature_mut(ex).map(|f| &mut f.kind) {
            e.depth = Dim::Value(20.0);
        }
        h.app.finish_editor(true);
        h.step(vec![]);
        ex
    }

    #[test]
    fn hole_on_a_selected_face_then_more_points() {
        let mut h = Harness::new();
        let ex = block(&mut h);
        let full = 40.0 * 40.0 * 20.0;

        // Hole on the top face: one M6 clearance hole through, at the face's middle.
        h.app.selection = Selection::Face(FaceId { feature: ex, index: FaceId::END_CAP });
        h.app.cmd_hole();
        h.step(vec![]);
        assert!(matches!(h.app.editor, Some(Editor::Feature { .. })), "the hole editor opens");
        h.app.finish_editor(true);
        h.step(vec![]);
        let st = &h.app.state;
        assert!(st.errors.is_empty(), "{:?}", st.errors);
        let one = std::f64::consts::PI * 3.3 * 3.3 * 20.0;
        let v = st.body.mass_properties().volume;
        assert!((v - (full - one)).abs() / full < 1e-3, "{v}");
        let hole = h.app.doc.features.last().unwrap().id;
        let Some(FeatureKind::Hole(hf)) = h.app.doc.feature(hole).map(|f| f.kind.clone()) else {
            panic!("expected a hole")
        };
        let centre = h.app.state.frames[&hf.sketch]
            .to_world(hole::positions(&h.app.doc.sketch(hf.sketch).unwrap().sketch)[0]);
        assert!((centre - Vec3 { x: 0.0, y: 0.0, z: 20.0 }).length() < 1e-9, "{centre:?}");

        // Add a second point with the Point tool: the hole feature cuts two holes.
        h.app.edit_sketch(hf.sketch);
        h.step(vec![]);
        h.tool(Tool::Point);
        let p = h.screen(10.0, 10.0);
        h.click(p);
        h.app.exit_sketch();
        h.step(vec![]);
        assert_eq!(hole::positions(&h.app.doc.sketch(hf.sketch).unwrap().sketch).len(), 2);
        let v = h.app.state.body.mass_properties().volume;
        assert!((v - (full - 2.0 * one)).abs() / full < 1e-3, "{v}");
    }

    #[test]
    fn hole_needs_a_face_or_points() {
        let mut h = Harness::new();
        block(&mut h);
        h.app.selection = Selection::None;
        let n = h.app.doc.features.len();
        h.app.cmd_hole();
        assert_eq!(h.app.doc.features.len(), n);
        assert!(h.app.message.as_ref().is_some_and(|(_, m)| m.contains("planar face")));
    }

    #[test]
    fn sketch_plate_with_hole_and_extrude() {
        let mut h = Harness::new();
        h.app.start_sketch(PlaneRef::Front);
        h.step(vec![]);
        assert!(h.app.view_rect.width() > 100.0);

        h.tool(Tool::Rectangle);
        let (a, b) = (h.screen(-20.0, -10.0), h.screen(20.0, 10.0));
        h.click(a);
        h.click(b);
        assert_eq!(h.sketch().entities.len(), 4, "rectangle creates four lines");

        // Circle centered by clicking on the origin, which snaps to it.
        h.tool(Tool::Circle);
        let (c, r) = (h.screen(0.0, 0.0), h.screen(5.0, 0.0));
        h.click(c);
        h.click(r);
        let sk = h.sketch();
        assert_eq!(sk.entities.len(), 5);
        match sk.entities[4].geom {
            Geom::Circle { c, r } => {
                assert_eq!(c, ORIGIN, "circle center snapped to the origin");
                assert!((r - 5.0).abs() < 0.3, "radius {r}");
            }
            _ => panic!("expected a circle"),
        }

        // Dimension tool on the bottom edge: pick the line, then click to place.
        h.tool(Tool::Dimension);
        let (mid, place) = (h.screen(0.0, -10.0), h.screen(0.0, -16.0));
        h.click(mid);
        h.click(place);
        assert!(h.sketch().constraints.iter().any(|c| matches!(c, Constraint::Length { line: 0, .. })));

        h.app.exit_sketch();
        h.step(vec![]);
        h.app.cmd_extrude(BodyOp::Boss);
        h.step(vec![]);
        h.app.finish_editor(true);
        h.step(vec![]);

        let st = &h.app.state;
        assert!(st.errors.is_empty(), "{:?}", st.errors);
        let w = 40.0;
        let v = st.body.mass_properties().volume;
        let expected = (w * 20.0 - std::f64::consts::PI * 25.0) * 10.0;
        assert!((v - expected).abs() / expected < 0.05, "volume {v} vs {expected}");
        assert_eq!(st.body.open_edges(), 0);

        // Undo takes the extrude back out.
        h.app.undo();
        h.step(vec![]);
        assert!(h.app.state.body.is_empty());
        h.app.redo();
        h.step(vec![]);
        assert!(!h.app.state.body.is_empty());
    }

    #[test]
    fn dimension_tool_infers_places_and_flags_over_defining() {
        let mut h = Harness::new();
        h.app.start_sketch(PlaneRef::Front);
        h.step(vec![]);
        h.tool(Tool::Rectangle);
        let (a, b) = (h.screen(-20.0, -10.0), h.screen(20.0, 10.0));
        h.click(a);
        h.click(b);
        h.tool(Tool::Circle);
        let (c, r) = (h.screen(0.0, 0.0), h.screen(5.0, 0.0));
        h.click(c);
        h.click(r);
        let radius = h.sketch().radius(4);

        h.tool(Tool::Dimension);
        // A circle, placed up and to the right: its diameter, with the value where it was
        // placed.
        let (rim, place) = (h.screen(radius, 0.0), h.screen(12.0, 8.0));
        h.click(rim);
        assert_eq!(h.app.sketch.as_ref().unwrap().picks.len(), 1);
        h.click(place);
        let ci = h.sketch().constraints.len() - 1;
        assert_eq!(h.sketch().constraints[ci], Constraint::Diameter { entity: 4, value: 2.0 * radius });
        let at = anvil_sketch::dimension::label_position(h.sketch(), ci).unwrap();
        assert!(at.distance(v2(12.0, 8.0)) < 0.2, "label at {at:?}");
        assert!(h.app.sketch.as_ref().unwrap().dim_edit.is_some(), "the value box opens");

        // The bottom edge: its length.
        let (mid, place) = (h.screen(0.0, -10.0), h.screen(0.0, -16.0));
        h.click(mid);
        h.click(place);
        let len = h.sketch().constraints.len() - 1;
        assert!(
            matches!(h.sketch().constraints[len], Constraint::Length { line: 0, value } if (value - 40.0).abs() < 1e-3)
        );

        // Esc takes back the last pick.
        h.click(h.screen(20.0, 0.0));
        assert_eq!(h.app.sketch.as_ref().unwrap().picks.len(), 1);
        h.step(vec![Event::Key {
            key: Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Default::default(),
        }]);
        h.step(vec![]);
        assert!(h.app.sketch.as_ref().unwrap().picks.is_empty());
        assert_eq!(h.app.sketch.as_ref().unwrap().tool, Tool::Dimension, "Esc with picks keeps the tool");

        // Two opposite corners, placed above: the horizontal distance, which the length
        // already sets, so it over-defines the sketch and is held as a reference.
        let n = h.sketch().constraints.len();
        h.click(h.screen(-20.0, -10.0));
        h.click(h.screen(20.0, 10.0));
        h.click(h.screen(0.0, 15.0));
        let sk = h.sketch();
        assert_eq!(sk.constraints.len(), n + 1);
        assert!(
            matches!(sk.constraints[n], Constraint::HDistance { value, .. } if (value.abs() - 40.0).abs() < 1e-3)
        );
        assert!(sk.is_reference(n));
        let s = h.app.sketch.as_ref().unwrap();
        assert_eq!(s.over_define, Some(n));
        assert!(s.status.converged, "a reference dimension does not over-define: {:?}", s.status);

        // Changing the driving length moves the reference dimension with it.
        let id = s.feature;
        let sf = h.app.doc.sketch_mut(id).unwrap();
        sf.sketch.constraints[len].set_value(50.0);
        sf.sketch.solve();
        assert!(
            matches!(sf.sketch.constraints[n], Constraint::HDistance { value, .. } if (value.abs() - 50.0).abs() < 1e-6)
        );

        // Keeping it driving instead makes it redundant, and a different value conflicts.
        let s = h.app.sketch.as_mut().unwrap();
        s.resolve_over_define(h.app.doc.sketch_mut(id).unwrap(), OverDefine::Driving);
        assert!(!h.sketch().is_reference(n));
        assert!(anvil_sketch::dimension::over_defines(h.sketch(), n));
        let sf = h.app.doc.sketch_mut(id).unwrap();
        sf.sketch.constraints[n].set_value(45.0);
        assert!(!sf.sketch.solve().converged);
        sf.sketch.constraints[n].set_value(50.0);
        h.app.sketch.as_mut().unwrap().status = h.app.doc.sketch_mut(id).unwrap().sketch.solve();

        // Dragging a value in the select tool moves its label.
        h.tool(Tool::Select);
        let from = anvil_sketch::dimension::label_position(h.sketch(), ci).unwrap();
        let (f, t) = (h.screen(from.x, from.y), h.screen(-14.0, 12.0));
        h.drag(f, t);
        let at = anvil_sketch::dimension::label_position(h.sketch(), ci).unwrap();
        assert!(at.distance(v2(-14.0, 12.0)) < 0.2, "label dragged to {at:?}");
    }

    #[test]
    fn dragging_a_corner_keeps_the_rectangle() {
        let mut h = Harness::new();
        h.app.start_sketch(PlaneRef::Top);
        h.step(vec![]);
        h.tool(Tool::Rectangle);
        let (a, b) = (h.screen(0.0, 0.0), h.screen(20.0, 10.0));
        h.click(a);
        h.click(b);
        h.tool(Tool::Select);
        let (from, to) = (h.screen(20.0, 10.0), h.screen(30.0, 18.0));
        h.drag(from, to);

        let sk = h.sketch();
        for e in 0..4 {
            let (p, q) = line_ends(sk, e);
            let d = q - p;
            assert!(d.x.abs() < 1e-6 || d.y.abs() < 1e-6, "line {e} is no longer axis aligned: {d:?}");
        }
        let far = sk.points.iter().map(|p| p.x + p.y).fold(f64::MIN, f64::max);
        assert!((far - 48.0).abs() < 1.0, "corner did not follow the drag: {far}");
    }

    #[test]
    fn click_face_then_sketch_on_it() {
        let mut h = Harness::new();
        h.app.load_document(crate::sample::sample_part(), None);
        for _ in 0..3 {
            h.step(vec![]);
        }
        // A point on the plate's top face, clear of the holes and the boss.
        let p = h.app.camera.project(anvil_kernel::v3(35.0, 10.0, 0.0), h.app.view_rect);
        h.click(p);
        let Selection::Face(face) = h.app.selection else {
            panic!("expected a face selection, got {:?}", h.app.selection)
        };
        assert_eq!(face.index, FaceId::END_CAP, "top cap of the base extrude");
        h.app.cmd_sketch();
        h.step(vec![]);
        let s = h.app.sketch.as_ref().expect("sketch started on the face");
        assert!((s.frame.origin.y - 10.0).abs() < 1e-9);
        assert!((s.frame.z - anvil_kernel::Vec3::Y).length() < 1e-9);
    }

    /// A 40 x 20 x 10 block (sketch on Top, extruded 10 up).
    fn block_doc() -> (Document, u32) {
        let mut doc = Document::new();
        let mut sf = SketchFeature::new(PlaneRef::Top);
        sf.sketch.add_rectangle(v2(-20.0, -10.0), v2(20.0, 10.0));
        let sk = doc.add_feature(FeatureKind::Sketch(sf));
        let ex = doc.add_feature(FeatureKind::Extrude(ExtrudeFeature {
            sketch: sk,
            end: EndCondition::Blind,
            depth: Dim::Value(10.0),
            reverse: false,
            op: BodyOp::Boss,
            ..Default::default()
        }));
        (doc, ex)
    }

    #[test]
    fn fillet_by_clicking_edges() {
        let mut h = Harness::new();
        let (doc, ex) = block_doc();
        h.app.load_document(doc, None);
        for _ in 0..3 {
            h.step(vec![]);
        }
        h.app.cmd_edge_treatment(false);
        h.step(vec![]);
        // Pick a top edge: between the end cap and a side face of the extrude. Take the
        // one whose midpoint lies farthest on screen from every other edge, so the
        // click cannot land on an edge behind it (the edge list's order is not fixed).
        let body = h.app.pick_body.as_ref().unwrap();
        let (cam, rect) = (h.app.camera, h.app.view_rect);
        let screen_gap = |p: Pos2, a: Pos2, b: Pos2| {
            let ab = b - a;
            let t = ((p - a).dot(ab) / ab.length_sq().max(1e-12)).clamp(0.0, 1.0);
            (a + ab * t - p).length()
        };
        let clearance = |e: &anvil_kernel::solid::ModelEdge| {
            let mid = cam.project((e.seg[0] + e.seg[1]) * 0.5, rect);
            body.edges
                .iter()
                .filter(|o| o.faces != e.faces)
                .map(|o| screen_gap(mid, cam.project(o.seg[0], rect), cam.project(o.seg[1], rect)))
                .fold(f32::INFINITY, f32::min)
        };
        let target = body
            .edges
            .iter()
            .filter(|e| {
                let f = e.faces;
                f.a.feature == ex
                    && f.b.feature == ex
                    && (f.a.index == FaceId::END_CAP || f.b.index == FaceId::END_CAP)
            })
            .max_by(|a, b| clearance(a).total_cmp(&clearance(b)))
            .copied()
            .expect("a top edge");
        let mid = (target.seg[0] + target.seg[1]) * 0.5;
        let p = h.app.camera.project(mid, h.app.view_rect);
        h.click(p);
        let id = match &h.app.editor {
            Some(Editor::Feature { id, .. }) => *id,
            _ => panic!("fillet editor not open"),
        };
        let edges = |h: &Harness| match &h.app.doc.feature(id).unwrap().kind {
            FeatureKind::Fillet(f) => f.edges.clone(),
            _ => unreachable!(),
        };
        assert_eq!(edges(&h), vec![target.faces], "clicking the edge selects it");
        // Clicking again deselects, a third click selects it again.
        h.click(p);
        assert!(edges(&h).is_empty());
        h.click(p);
        assert_eq!(edges(&h).len(), 1);

        let before = h.app.pick_body.as_ref().unwrap().solid.mass_properties().volume;
        h.app.finish_editor(true);
        h.step(vec![]);
        assert!(h.app.state.errors.is_empty(), "{:?}", h.app.state.errors);
        assert!(h.app.pick_body.is_none());
        let len = (target.seg[0] - target.seg[1]).length();
        let removed = before - h.app.state.body.mass_properties().volume;
        // Corner area (1 - pi/4) r^2 with r = 2, along the whole edge. The rounding arc is
        // faceted in 10-degree chords, which over-cuts this small region by about 2%.
        let expected = (1.0 - std::f64::consts::PI / 4.0) * 4.0 * len;
        assert!((removed - expected).abs() / expected < 0.03, "removed {removed}, expected {expected}");
    }

    #[test]
    fn shell_from_selected_face() {
        let mut h = Harness::new();
        let (doc, ex) = block_doc();
        h.app.load_document(doc, None);
        for _ in 0..3 {
            h.step(vec![]);
        }
        // Select the top face, then Shell: it becomes the open face.
        h.app.selection = Selection::Face(FaceId { feature: ex, index: FaceId::END_CAP });
        h.app.cmd_shell();
        h.step(vec![]);
        h.app.finish_editor(true);
        h.step(vec![]);
        assert!(h.app.state.errors.is_empty(), "{:?}", h.app.state.errors);
        let v = h.app.state.body.mass_properties().volume;
        let expected = 40.0 * 20.0 * 10.0 - 38.0 * 18.0 * 9.0;
        assert!((v - expected).abs() < 1e-4, "{v} vs {expected}");
    }

    #[test]
    fn line_chain_closes_into_profile() {
        let mut h = Harness::new();
        h.app.start_sketch(PlaneRef::Right);
        h.step(vec![]);
        h.tool(Tool::Line);
        let pts = [(0.0, 0.0), (30.0, 0.0), (30.0, 12.0), (10.0, 25.0), (0.0, 0.0)];
        for (x, y) in pts {
            let p = h.screen(x, y);
            h.click(p);
        }
        let sk = h.sketch();
        assert_eq!(sk.entities.len(), 4);
        // Nearly horizontal first segment got an automatic horizontal relation.
        assert!(sk.constraints.iter().any(|c| matches!(c, Constraint::Horizontal { line: 0 })));
        let profiles = sk.profiles().expect("closed profile");
        assert_eq!(profiles.len(), 1);
    }
}
