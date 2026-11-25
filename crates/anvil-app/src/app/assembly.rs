//! Assembly mode: an assembly open in the main window instead of a part. The left pane
//! lists the assembly's planes, its components (with what is inside its subassemblies)
//! and its mates; the command bar inserts parts and subassemblies, adds mates, makes a
//! subassembly flexible or rigid and shows the bill of materials; the view shows every
//! part, at every depth, where the mates put it. Behaviour follows
//! `docs/re/assemblies-and-mates.md`, including "Subassemblies".

use super::interference::{InterferenceOptions, InterferenceTool};
use super::AnvilApp;
use crate::file_error::{FileFailure, FileOp, Kind};
use crate::theme::{self, Icon, Palette, RowLook};
use anvil_kernel::{EdgeRef, FaceId, Frame, Mesh, ModelEdge, Solid, Vec3};
use anvil_model::assembly::{
    file_solid, is_assembly_path, Assembly, AssemblyStatus, BomView, LoadError, MateEntity, MateRef, Parts,
    ASSEMBLY_EXT,
};
use anvil_model::mates::{Alignment, Definition, MateKind, MateState};
use anvil_model::{BuildState, PlaneRef, PART_EXT};
use eframe::egui::{self, RichText, Sense};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Face ids at or above this stand for a face of a component in the combined body.
const COMPONENT_FACE: u32 = 1 << 30;

/// The mate types offered, in menu order.
pub(super) const MATE_TYPES: &[&str] =
    &["Coincident", "Concentric", "Parallel", "Perpendicular", "Tangent", "Distance", "Angle", "Lock"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum AsmSelection {
    None,
    Component(u32),
    Mate(u32),
}

/// The Mate panel: a new mate being set up, or an existing one being edited.
pub(super) struct MateTool {
    pub kind: usize,
    pub picks: Vec<MateRef>,
    pub alignment: Alignment,
    pub flip: bool,
    /// The distance (mm) or angle (degrees) as typed.
    pub value: String,
    /// The mate being edited, when not a new one.
    pub editing: Option<u32>,
}

impl MateTool {
    fn new() -> MateTool {
        MateTool {
            kind: 0,
            picks: Vec::new(),
            alignment: Alignment::Closest,
            flip: false,
            value: String::new(),
            editing: None,
        }
    }

    /// The mate kind as set up, or an error saying what is missing.
    fn kind(&self, asm: &Assembly) -> Result<MateKind, String> {
        let value = || -> Result<f64, String> {
            let v: f64 = self.value.trim().parse().map_err(|_| "type a value".to_string())?;
            if v.is_finite() && v >= 0.0 {
                Ok(v)
            } else {
                Err("the value must be zero or more".into())
            }
        };
        Ok(match MATE_TYPES.get(self.kind).copied().unwrap_or("Coincident") {
            "Concentric" => MateKind::Concentric,
            "Parallel" => MateKind::Parallel,
            "Perpendicular" => MateKind::Perpendicular,
            "Tangent" => MateKind::Tangent,
            "Distance" => MateKind::Distance { value: value()?, flip: self.flip },
            "Angle" => MateKind::Angle { degrees: value()? },
            "Lock" => {
                let [a, b] = self.picks.as_slice() else { return Err("pick two components".into()) };
                let b = b.component.ok_or("pick a component second")?;
                asm.lock(a.component, b).ok_or("pick two components")?
            }
            _ => MateKind::Coincident,
        })
    }

    fn uses_value(&self) -> bool {
        matches!(MATE_TYPES.get(self.kind), Some(&"Distance") | Some(&"Angle"))
    }
}

/// A component inside a subassembly, as the tree lists it.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct NestedRow {
    /// The path of component ids from the top level.
    pub path: Vec<u32>,
    /// Its own instance name, as `link<2>`.
    pub name: String,
    pub assembly: bool,
    pub flexible: bool,
}

/// The name of the component at `path`, from a session's names, or its ids.
pub(super) fn label_of(names: &HashMap<Vec<u32>, String>, path: &[u32]) -> String {
    names
        .get(path)
        .cloned()
        .unwrap_or_else(|| path.iter().map(|id| format!("#{id}")).collect::<Vec<_>>().join("/"))
}

/// Lists the components inside `asm` (a subassembly at `prefix`), depth first.
fn nested_rows(
    asm: &Assembly,
    parts: &mut Parts,
    prefix: &[u32],
    label: &str,
    names: &mut HashMap<Vec<u32>, String>,
    out: &mut Vec<NestedRow>,
) {
    for c in &asm.components {
        let mut path = prefix.to_vec();
        path.push(c.id);
        let full = format!("{label}/{}", c.name);
        names.insert(path.clone(), full.clone());
        out.push(NestedRow {
            path: path.clone(),
            name: c.name.clone(),
            assembly: c.is_assembly(),
            flexible: c.flexible,
        });
        if c.is_assembly() && path.len() <= anvil_model::assembly::MAX_DEPTH {
            if let Ok(sub) = parts.assembly(&c.path) {
                nested_rows(&sub.asm, parts, &path, &full, names, out);
            }
        }
    }
}

/// An assembly open in the window.
pub(super) struct AsmSession {
    pub asm: Assembly,
    saved: Assembly,
    committed: Assembly,
    pub(super) undo: Vec<Assembly>,
    pub(super) redo: Vec<Assembly>,
    pub path: Option<PathBuf>,
    pub parts: Parts,
    pub status: AssemblyStatus,
    /// The combined body of the visible components, in assembly coordinates, with each
    /// face renamed to a `COMPONENT_FACE` id.
    pub body: Solid,
    /// `body`, tessellated for the view.
    mesh: Mesh,
    /// What each renamed face is: its part's component path and its face in the part.
    faces: Vec<(Vec<u32>, FaceId)>,
    /// Model edges in assembly coordinates, named by the part's own faces.
    pub(super) edges: Vec<(Vec<u32>, ModelEdge)>,
    /// Components, at any depth, whose file could not be loaded, with why.
    pub load_errors: Vec<LoadError>,
    /// Every component's name by its path, `arm<1>/link<2>` inside a subassembly.
    pub path_names: HashMap<Vec<u32>, String>,
    /// The components inside each subassembly, for the tree.
    pub nested: Vec<NestedRow>,
    pub selection: AsmSelection,
    pub mate: Option<MateTool>,
    pub bom: bool,
    pub bom_view: BomView,
    /// The Interference panel, while it is open.
    pub interference: Option<InterferenceTool>,
    /// Its options, kept while it is closed.
    pub interference_opts: InterferenceOptions,
}

impl AsmSession {
    fn new(asm: Assembly, path: Option<PathBuf>) -> AsmSession {
        let parts = match &path {
            Some(p) => Parts::for_file(p),
            None => Parts::new(""),
        };
        AsmSession {
            saved: if path.is_some() { asm.clone() } else { Assembly::new() },
            committed: asm.clone(),
            asm,
            undo: Vec::new(),
            redo: Vec::new(),
            path,
            parts,
            status: AssemblyStatus { mates: Vec::new(), components: Vec::new() },
            body: Solid::default(),
            mesh: Mesh::default(),
            faces: Vec::new(),
            edges: Vec::new(),
            load_errors: Vec::new(),
            path_names: HashMap::new(),
            nested: Vec::new(),
            selection: AsmSelection::None,
            mate: None,
            bom: false,
            bom_view: BomView::TopLevel,
            interference: None,
            interference_opts: InterferenceOptions::default(),
        }
    }

    pub fn is_dirty(&self) -> bool {
        self.asm != self.saved
    }

    pub fn name(&self) -> String {
        self.path
            .as_ref()
            .and_then(|p| p.file_stem())
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "Assembly1".into())
    }

    /// The part's component path and part face behind a face of the combined body.
    pub fn face(&self, f: FaceId) -> Option<(Vec<u32>, FaceId)> {
        f.feature.checked_sub(COMPONENT_FACE).and_then(|k| self.faces.get(k as usize)).cloned()
    }

    /// Solves the mates and rebuilds the combined body from every part at every depth.
    fn refresh(&mut self) {
        self.status = self.asm.solve(&mut self.parts);
        let (placed, errors) = self.asm.placed_parts(&mut self.parts);
        self.load_errors = errors;
        self.path_names.clear();
        self.nested.clear();
        for c in &self.asm.components {
            self.path_names.insert(vec![c.id], c.name.clone());
            if c.is_assembly() {
                if let Ok(sub) = self.parts.assembly(&c.path) {
                    nested_rows(
                        &sub.asm,
                        &mut self.parts,
                        &[c.id],
                        &c.name,
                        &mut self.path_names,
                        &mut self.nested,
                    );
                }
            }
        }
        let mut polygons = Vec::new();
        let mut index: HashMap<(Vec<u32>, FaceId), u32> = HashMap::new();
        self.faces.clear();
        self.edges.clear();
        for p in placed.iter().filter(|p| !p.hidden) {
            let Ok(part) = self.parts.get(&p.file, p.configuration.as_deref()) else { continue };
            let f = p.placement;
            for e in part.state.body.model_edges() {
                let seg = [f.to_world3(e.seg[0]), f.to_world3(e.seg[1])];
                self.edges.push((p.path.clone(), ModelEdge { seg, faces: e.faces }));
            }
            for mut poly in part.state.body.transformed(&f).polygons {
                let key = (p.path.clone(), poly.tag.id);
                let n = self.faces.len() as u32;
                let k = *index.entry(key.clone()).or_insert_with(|| {
                    self.faces.push(key);
                    n
                });
                poly.tag.id = FaceId { feature: COMPONENT_FACE + k, index: 0 };
                polygons.push(poly);
            }
        }
        self.body = Solid::new(polygons);
        self.mesh = self.body.tessellate();
    }

    /// The load errors of the top-level component `id`: its own and those inside it.
    pub fn errors_of(&self, id: u32) -> Vec<&LoadError> {
        self.load_errors.iter().filter(|e| e.path.first() == Some(&id)).collect()
    }

    fn commit(&mut self) {
        if self.asm != self.committed {
            self.undo.push(std::mem::replace(&mut self.committed, self.asm.clone()));
            self.redo.clear();
        }
    }

    /// The entity a click on a face or edge of the combined body picks: on the part that
    /// owns it, however deep inside subassemblies.
    fn pick_ref(&self, face: Option<FaceId>, edge: Option<(Vec<u32>, EdgeRef)>) -> Option<MateRef> {
        if let Some((c, e)) = edge {
            return Some(MateRef::at(&c, MateEntity::Edge(e)));
        }
        let (c, f) = self.face(face?)?;
        Some(MateRef::at(&c, MateEntity::Face(f)))
    }

    /// Faces of the combined body to highlight (or, while the Interference panel shows
    /// findings, of the overlap volumes).
    pub fn highlighted(&self) -> Vec<FaceId> {
        if self.interference.as_ref().is_some_and(InterferenceTool::shows_findings) {
            return Vec::new();
        }
        let synthetic = |pred: &dyn Fn(&[u32], FaceId) -> bool| -> Vec<FaceId> {
            self.faces
                .iter()
                .enumerate()
                .filter(|(_, (c, f))| pred(c, *f))
                .map(|(k, _)| FaceId { feature: COMPONENT_FACE + k as u32, index: 0 })
                .collect()
        };
        if let Some(t) = &self.mate {
            return synthetic(&|c, f| {
                t.picks.iter().any(|r| {
                    r.path() == c
                        && match r.entity {
                            MateEntity::Face(g) | MateEntity::Plane(PlaneRef::Face(g)) => g == f,
                            _ => false,
                        }
                })
            });
        }
        match self.selection {
            AsmSelection::Component(id) => synthetic(&|c, _| c.first() == Some(&id)),
            AsmSelection::Mate(id) => match self.asm.mate(id) {
                Some(m) => {
                    let on = |r: &MateRef, c: &[u32], f: FaceId| {
                        r.path() == c
                            && match r.entity {
                                MateEntity::Face(g) => g == f,
                                _ => false,
                            }
                    };
                    synthetic(&|c, f| on(&m.a, c, f) || on(&m.b, c, f))
                }
                None => vec![],
            },
            AsmSelection::None => vec![],
        }
    }

    /// Segments (in assembly coordinates) of the edges picked for the Mate panel, or of
    /// the selected mate.
    pub fn highlighted_edges(&self) -> Vec<[Vec3; 2]> {
        let refs: Vec<MateRef> = match (&self.mate, self.selection) {
            (Some(t), _) => t.picks.clone(),
            (None, AsmSelection::Mate(id)) => {
                self.asm.mate(id).map(|m| vec![m.a.clone(), m.b.clone()]).unwrap_or_default()
            }
            _ => vec![],
        };
        self.edges
            .iter()
            .filter(|(c, e)| refs.iter().any(|r| r.path() == *c && r.entity == MateEntity::Edge(e.faces)))
            .map(|(_, e)| e.seg)
            .collect()
    }

    /// True while the Interference panel shows findings: components are then drawn as
    /// wireframe.
    pub fn wireframe(&self) -> bool {
        self.interference.as_ref().is_some_and(InterferenceTool::shows_findings)
    }

    /// A component's instance name, as `bracket<1>`.
    pub fn component_label(&self, id: u32) -> String {
        self.component_name(Some(id))
    }

    /// The name of the component at `path`, as `arm<1>/link<2>` inside a subassembly.
    pub fn path_label(&self, path: &[u32]) -> String {
        label_of(&self.path_names, path)
    }

    /// Faces drawn red: the selected finding's overlap volume.
    pub fn failed_faces(&self) -> Vec<FaceId> {
        self.interference
            .as_ref()
            .filter(|t| t.shows_findings())
            .map(|t| t.selected_faces())
            .unwrap_or_default()
    }

    fn component_name(&self, id: Option<u32>) -> String {
        match id {
            None => self.name(),
            Some(id) => self.asm.component(id).map(|c| c.name.clone()).unwrap_or_else(|| format!("#{id}")),
        }
    }

    /// A short description of a mate entity, such as `bracket<1>: Face <3:0>`.
    pub fn ref_label(&self, r: &MateRef) -> String {
        let what = match r.entity {
            MateEntity::Origin => "Origin".to_string(),
            MateEntity::Plane(p) => p.label(),
            MateEntity::Axis(id) => format!("Axis #{id}"),
            MateEntity::Point(id) => format!("Point #{id}"),
            MateEntity::Face(f) => format!("Face <{}:{}>", f.feature, f.index),
            MateEntity::Edge(e) => {
                format!("Edge <{}:{}|{}:{}>", e.a.feature, e.a.index, e.b.feature, e.b.index)
            }
            MateEntity::Vertex(_) => "Vertex".to_string(),
        };
        let who = if r.component.is_none() { self.name() } else { self.path_label(&r.path()) };
        format!("{who}: {what}")
    }
}

/// Why an assembly was not saved.
#[derive(Debug)]
pub(super) enum SaveRefused {
    /// Saving there would be wrong: not an assembly file, or one of its own subassemblies.
    Refused(String),
    /// Writing the file failed.
    File(Box<anvil_model::FileError>),
}

impl std::fmt::Display for SaveRefused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SaveRefused::Refused(e) => f.write_str(e),
            SaveRefused::File(e) => write!(f, "{e}"),
        }
    }
}

/// A bill of materials as scripts and MCP see it.
pub(super) fn bom_json(view: BomView, rows: &[anvil_model::assembly::BomRow]) -> serde_json::Value {
    use serde_json::json;
    let rows: Vec<_> = rows
        .iter()
        .map(|r| {
            json!({
                "number": r.number,
                "level": r.level,
                "assembly": r.assembly,
                "part_number": r.part_number,
                "description": r.description,
                "path": r.path,
                "configuration": r.configuration,
                "quantity": r.quantity,
            })
        })
        .collect();
    json!({ "view": view.key(), "rows": rows })
}

/// The tree prefix for how well a component is defined: `(f)` fixed, `(-)` under defined,
/// `(+)` over defined, nothing when fully defined.
fn prefix(d: Option<Definition>) -> &'static str {
    match d {
        Some(Definition::Fixed) => "(f) ",
        Some(Definition::UnderDefined(_)) => "(-) ",
        Some(Definition::OverDefined) => "(+) ",
        Some(Definition::FullyDefined) => "",
        None => "(?) ",
    }
}

/// `path` relative to `dir` when it lies inside it, with `/` separators.
fn relative_to(path: &Path, dir: &Path) -> String {
    path.strip_prefix(dir).unwrap_or(path).to_string_lossy().replace('\\', "/")
}

impl AnvilApp {
    pub(super) fn in_assembly(&self) -> bool {
        self.asm.is_some()
    }

    /// Starts a new, empty assembly.
    pub(super) fn new_assembly(&mut self) {
        self.enter_assembly(AsmSession::new(Assembly::new(), None));
        self.status = "New assembly: insert a component to begin".into();
    }

    pub(super) fn open_assembly(&mut self, p: &Path) {
        match Assembly::load(p) {
            Ok(a) => {
                self.enter_assembly(AsmSession::new(a, Some(p.to_path_buf())));
                // Solving on open can move components by rounding error; that is not an
                // unsaved change.
                if let Some(s) = &mut self.asm {
                    s.saved = s.asm.clone();
                    s.committed = s.asm.clone();
                    // Everything that loaded is open; say what did not.
                    if !s.load_errors.is_empty() {
                        let lines: Vec<String> =
                            s.load_errors.iter().map(|e| format!("{}: {}", e.name, e.error)).collect();
                        self.message = Some((
                            "Some components could not be loaded".into(),
                            format!(
                                "The rest of the assembly is open. These components are marked in the tree and \
                                 mates to them are unresolved until their files are back:\n\n{}",
                                lines.join("\n")
                            ),
                        ));
                    }
                }
            }
            Err(e) => self.file_failure = Some(FileFailure::file(FileOp::Open, Kind::Assembly, p, &e)),
        }
    }

    /// Opens recovered assembly work, compared with the user's file so it shows as unsaved.
    pub(super) fn restore_assembly(&mut self, asm: Assembly, path: Option<PathBuf>) {
        let on_disk = path.as_ref().and_then(|p| Assembly::load(p).ok());
        self.enter_assembly(AsmSession::new(asm, path));
        if let Some(s) = &mut self.asm {
            s.saved = on_disk.unwrap_or_default();
        }
    }

    fn enter_assembly(&mut self, s: AsmSession) {
        self.leave_drawing();
        self.sketch = None;
        self.editor = None;
        self.measure = None;
        self.pick_body = None;
        self.selection = super::Selection::None;
        self.asm = Some(s);
        self.fit_pending = true;
        self.refresh_assembly();
    }

    /// Leaves assembly mode (when a part is opened or started).
    pub(super) fn leave_assembly(&mut self) {
        if self.asm.take().is_some() {
            self.fit_pending = true;
        }
    }

    /// Solves the open assembly and shows the result.
    pub(super) fn refresh_assembly(&mut self) {
        let Some(s) = &mut self.asm else { return };
        s.refresh();
        self.state = BuildState::default();
        self.state.body = s.body.clone();
        self.interference_after_change();
        self.show_assembly_scene();
        let Some(s) = &self.asm else { return };
        let bad = s.status.mates.iter().filter(|m| !matches!(m.1, MateState::Satisfied)).count();
        let under = s.status.components.iter().filter(|c| matches!(c.1, Definition::UnderDefined(_))).count();
        self.status = if !s.load_errors.is_empty() {
            format!("{} component(s) could not be loaded", s.load_errors.len())
        } else if bad > 0 {
            format!("{bad} mate(s) have errors or warnings")
        } else if s.asm.components.is_empty() {
            "Insert a component to begin".into()
        } else if under > 0 {
            format!("Mates solved; {under} component(s) under defined")
        } else {
            "Mates solved; fully defined".into()
        };
    }

    /// Shows the assembly in the view: its components shaded, or while the Interference
    /// panel shows findings, its components as wireframe and the overlap volumes shaded.
    pub(super) fn show_assembly_scene(&mut self) {
        let Some(s) = &self.asm else { return };
        self.mesh = match s.interference.as_ref().filter(|t| t.shows_findings()) {
            Some(t) => t.mesh().clone(),
            None => s.mesh.clone(),
        };
        self.edges = s.edges.iter().map(|(_, e)| e.seg).collect();
        self.hover_face = None;
        self.scene_dirty = true;
    }

    pub(super) fn assembly_undo(&mut self, redo: bool) {
        let Some(s) = &mut self.asm else { return };
        let (from, to) = if redo { (&mut s.redo, &mut s.undo) } else { (&mut s.undo, &mut s.redo) };
        if let Some(a) = from.pop() {
            to.push(std::mem::replace(&mut s.asm, a));
            s.committed = s.asm.clone();
            s.mate = None;
            s.selection = AsmSelection::None;
            self.refresh_assembly();
        }
    }

    pub(super) fn assembly_commit(&mut self) {
        if let Some(s) = &mut self.asm {
            s.commit();
        }
    }

    pub(super) fn save_assembly(&mut self, save_as: bool) {
        let Some(s) = &self.asm else { return };
        let path = match (&s.path, save_as) {
            (Some(p), false) => Some(p.clone()),
            _ => {
                let name = format!("{}.{ASSEMBLY_EXT}", s.name());
                self.file_dialog("Save As", || {
                    rfd::FileDialog::new()
                        .add_filter("AnvilCAD assembly", &[ASSEMBLY_EXT])
                        .set_file_name(name)
                        .save_file()
                })
            }
        };
        let Some(path) = path else { return };
        match self.save_assembly_to(&path) {
            Ok(()) => self.status = format!("Saved {}", path.display()),
            Err(SaveRefused::File(e)) => {
                self.file_failure = Some(FileFailure::file(FileOp::Save, Kind::Assembly, &path, &e))
            }
            Err(SaveRefused::Refused(e)) => self.message = Some(("Save failed".into(), e)),
        }
    }

    /// Saves the open assembly as `path`, with its component paths made relative to it.
    /// Refuses a path that is not an `.anvasm` file, or one of its own subassemblies.
    pub(super) fn save_assembly_to(&mut self, path: &Path) -> Result<(), SaveRefused> {
        let Some(s) = self.asm.as_mut() else {
            return Err(SaveRefused::Refused("no assembly is open".into()));
        };
        if !is_assembly_path(&path.to_string_lossy()) {
            return Err(SaveRefused::Refused(format!(
                "{} is not an .{ASSEMBLY_EXT} file; an assembly saves as .{ASSEMBLY_EXT}",
                path.display()
            )));
        }
        // An assembly saved over one of its own subassemblies would contain itself.
        let target = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        let subs: Vec<String> =
            s.asm.components.iter().filter(|c| c.is_assembly()).map(|c| c.path.clone()).collect();
        for sub in subs {
            if s.parts.assembly_files(&sub).contains(&target) {
                return Err(SaveRefused::Refused(format!(
                    "{} is a subassembly of this assembly, so saving over it would make the assembly \
                     contain itself. Save under another name.",
                    path.display()
                )));
            }
        }
        // Component paths are kept relative to the assembly file where they can be.
        let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
        let old = s.parts.base.clone();
        let mut moved = s.asm.clone();
        for c in &mut moved.components {
            let full = old.join(&c.path);
            c.path = relative_to(&full, &dir);
        }
        moved.save(path).map_err(|e| SaveRefused::File(Box::new(e)))?;
        if moved != s.asm || s.path.as_deref() != Some(path) {
            s.asm = moved;
            s.parts = Parts::for_file(path);
        }
        s.path = Some(path.to_path_buf());
        s.saved = s.asm.clone();
        s.committed = s.asm.clone();
        self.refresh_assembly();
        Ok(())
    }

    /// Inserts a part or a subassembly as a new component, beside what is already there.
    pub(super) fn insert_component(&mut self) {
        if self.asm.is_none() {
            self.new_assembly();
        }
        let Some(picked) = self.file_dialog("Insert Component", || {
            rfd::FileDialog::new()
                .add_filter("AnvilCAD part or assembly", &[PART_EXT, ASSEMBLY_EXT])
                .add_filter("AnvilCAD part", &[PART_EXT])
                .add_filter("AnvilCAD assembly", &[ASSEMBLY_EXT])
                .pick_file()
        }) else {
            return;
        };
        self.insert_part(&picked);
    }

    pub(super) fn insert_part(&mut self, file: &Path) {
        if let Err(e) = self.insert_part_at(file, None) {
            self.message = Some(("Insert Component".into(), e));
        }
    }

    /// Inserts `file` (a part or an assembly) as a component, at `at` or beside the
    /// others; returns its id. An assembly that contains itself, or this assembly, is
    /// refused.
    pub(super) fn insert_part_at(&mut self, file: &Path, at: Option<Vec3>) -> Result<u32, String> {
        let Some(s) = &mut self.asm else { return Err("no assembly is open".into()) };
        let path = match &s.path {
            Some(p) => relative_to(file, p.parent().unwrap_or(Path::new(""))),
            None => file.to_string_lossy().replace('\\', "/"),
        };
        s.asm.check_insert(&mut s.parts, &path)?;
        let size = file_solid(&mut s.parts, &path, None)?.bounds();
        // Beside the assembly so far, with a gap, so the new part does not overlap.
        let here = s.body.bounds();
        let at = at.unwrap_or(if here.is_empty() || size.is_empty() {
            Vec3::ZERO
        } else {
            anvil_kernel::v3(here.max.x + 0.25 * here.diagonal().max(10.0) - size.min.x, 0.0, 0.0)
        });
        let id = s.asm.add_component(&path, None, Frame { origin: at, ..Frame::WORLD });
        if is_assembly_path(&path) {
            self.status = format!("Inserted {} as a rigid subassembly", file.display());
        }
        let Some(s) = &mut self.asm else { return Err("no assembly is open".into()) };
        s.selection = AsmSelection::Component(id);
        s.commit();
        self.fit_pending = true;
        self.refresh_assembly();
        Ok(id)
    }

    /// The face through `p` (assembly coordinates), for a scripted mate: the path of the
    /// part's component, however deep, and the face on that part.
    pub(super) fn assembly_face_at(&self, p: Vec3) -> Result<(Vec<u32>, FaceId), String> {
        let s = self.asm.as_ref().ok_or("no assembly is open")?;
        let f = super::scripting::face_through(&s.body, p)?;
        s.face(f).ok_or_else(|| "that face is not on a component".to_string())
    }

    /// Adds a mate and solves; a mate that cannot hold is taken out again, and why is
    /// returned. For scripts.
    pub(super) fn script_mate(
        &mut self,
        kind: MateKind,
        alignment: Alignment,
        a: MateRef,
        b: MateRef,
    ) -> Result<u32, String> {
        let Some(s) = &mut self.asm else { return Err("no assembly is open".into()) };
        if a.component.is_some() && a.component == b.component {
            return Err("the two sides are on the same component".into());
        }
        let before = s.asm.clone();
        let alignment = match alignment {
            Alignment::Closest => s.asm.closest_alignment(&mut s.parts, &a, &b),
            other => other,
        };
        let id = s.asm.add_mate(kind, alignment, a, b);
        self.refresh_assembly();
        let Some(s) = &mut self.asm else { return Err("no assembly is open".into()) };
        let trouble = match s.status.mate(id) {
            Some(MateState::Unsatisfied(r)) => {
                Some(format!("it cannot hold with the other mates (it stays {r:.3} mm off); check them, or change this one"))
            }
            Some(MateState::Invalid(e)) => Some(e.to_string()),
            _ => None,
        };
        if let Some(why) = trouble {
            s.asm = before;
            self.refresh_assembly();
            return Err(why);
        }
        s.selection = AsmSelection::Mate(id);
        s.commit();
        Ok(id)
    }

    /// Opens the Mate panel for a new mate, taking a selected component's nothing.
    pub(super) fn start_mate(&mut self) {
        if self.asm.is_some() {
            self.close_interference();
        }
        if let Some(s) = &mut self.asm {
            s.mate = Some(MateTool::new());
            self.scene_dirty = true;
        }
    }

    fn edit_mate(&mut self, id: u32) {
        self.close_interference();
        let Some(s) = &mut self.asm else { return };
        let Some(m) = s.asm.mate(id) else { return };
        let kind = MATE_TYPES.iter().position(|t| *t == m.kind.label()).unwrap_or(0);
        let (value, flip) = match m.kind {
            MateKind::Distance { value, flip } => (format!("{value}"), flip),
            MateKind::Angle { degrees } => (format!("{degrees}"), false),
            _ => (String::new(), false),
        };
        s.mate = Some(MateTool {
            kind,
            picks: vec![m.a.clone(), m.b.clone()],
            alignment: m.alignment,
            flip,
            value,
            editing: Some(id),
        });
        self.scene_dirty = true;
    }

    /// Adds or drops an entity from the Mate panel's picks. A third pick starts over from it.
    pub(super) fn mate_pick(&mut self, r: MateRef) {
        let Some(s) = &mut self.asm else { return };
        let Some(t) = &mut s.mate else { return };
        if let Some(i) = t.picks.iter().position(|p| *p == r) {
            t.picks.remove(i);
        } else {
            if t.picks.len() >= 2 {
                t.picks.clear();
            }
            t.picks.push(r);
        }
        // A distance or angle starts at what the components measure now.
        if let ([a, b], true) = (t.picks.as_slice(), t.uses_value() && t.editing.is_none()) {
            let probe = match MATE_TYPES.get(t.kind) {
                Some(&"Angle") => MateKind::Angle { degrees: 0.0 },
                _ => MateKind::Distance { value: 0.0, flip: false },
            };
            let alignment = match t.alignment {
                Alignment::Closest => s.asm.closest_alignment(&mut s.parts, a, b),
                other => other,
            };
            if let Some(v) = s.asm.measure_now(&mut s.parts, &probe, alignment, a, b) {
                t.value = format!("{:.3}", v.abs()).trim_end_matches('0').trim_end_matches('.').to_string();
                t.flip = matches!(probe, MateKind::Distance { .. }) && v < 0.0;
            }
        }
        self.scene_dirty = true;
    }

    /// Adds the mate set up in the Mate panel (or applies the edit) and solves.
    pub(super) fn apply_mate(&mut self) -> Result<u32, String> {
        let Some(s) = &mut self.asm else { return Err("no assembly is open".into()) };
        let Some(t) = &s.mate else { return Err("the Mate panel is not open".into()) };
        let [a, b] = t.picks.as_slice() else { return Err("pick two entities".into()) };
        let (a, b) = (a.clone(), b.clone());
        if a.component.is_some() && a.component == b.component {
            return Err("pick entities on two different components".into());
        }
        let kind = t.kind(&s.asm)?;
        let alignment = match t.alignment {
            Alignment::Closest => s.asm.closest_alignment(&mut s.parts, &a, &b),
            other => other,
        };
        let id = match t.editing {
            Some(id) => {
                if let Some(m) = s.asm.mate_mut(id) {
                    m.kind = kind;
                    m.alignment = alignment;
                    m.a = a;
                    m.b = b;
                }
                id
            }
            None => s.asm.add_mate(kind, alignment, a, b),
        };
        s.mate = None;
        s.selection = AsmSelection::Mate(id);
        s.commit();
        self.refresh_assembly();
        Ok(id)
    }

    pub(super) fn delete_assembly_selection(&mut self) {
        let Some(s) = &mut self.asm else { return };
        match s.selection {
            AsmSelection::Component(id) => {
                s.asm.delete_component(id);
            }
            AsmSelection::Mate(id) => s.asm.delete_mate(id),
            AsmSelection::None => return,
        }
        s.selection = AsmSelection::None;
        s.commit();
        self.refresh_assembly();
    }

    /// Hover and clicks in the view while an assembly is open.
    pub(super) fn assembly_pointer(&mut self, resp: &egui::Response, rect: egui::Rect) {
        // While findings are shown, the overlap volumes are what can be picked.
        if let Some(t) =
            self.asm.as_ref().and_then(|s| s.interference.as_ref()).filter(|t| t.shows_findings())
        {
            let hit = resp.hover_pos().and_then(|p| {
                let (o, d) = self.camera.ray(p, rect);
                t.regions.raycast(o, d)
            });
            let hover = hit.map(|h| h.tag.id);
            let picked = hover.and_then(|f| t.finding_of_face(f));
            if hover != self.hover_face {
                self.hover_face = hover;
                self.scene_dirty = true;
            }
            if resp.clicked_by(egui::PointerButton::Primary) {
                if let Some(k) = picked {
                    self.select_finding(Some(k));
                }
            }
            return;
        }
        let picking = self.asm.as_ref().is_some_and(|s| s.mate.is_some());
        let hit = resp.hover_pos().and_then(|p| {
            let (o, d) = self.camera.ray(p, rect);
            self.state.body.raycast(o, d)
        });
        let hover = hit.map(|h| h.tag.id);
        if hover != self.hover_face {
            self.hover_face = hover;
            self.scene_dirty = true;
        }
        if !resp.clicked_by(egui::PointerButton::Primary) {
            return;
        }
        let Some(s) = &mut self.asm else { return };
        if picking {
            let edge = resp.interact_pointer_pos().and_then(|m| {
                let (o, dir) = self.camera.ray(m, rect);
                // Edges behind the face under the pointer cannot be picked.
                let tol = 1e-3 * s.body.bounds().diagonal().max(1.0);
                let front = hit.map(|h| h.t + tol).unwrap_or(f64::INFINITY);
                let mut best: Option<(f32, (Vec<u32>, EdgeRef))> = None;
                for (c, e) in &s.edges {
                    let (a, b) = (self.camera.project(e.seg[0], rect), self.camera.project(e.seg[1], rect));
                    let ab = b - a;
                    let t = if ab.length_sq() < 1e-6 {
                        0.0
                    } else {
                        ((m - a).dot(ab) / ab.length_sq()).clamp(0.0, 1.0)
                    };
                    let d = (a + ab * t - m).length();
                    let depth = (e.seg[0].lerp(e.seg[1], t as f64) - o).dot(dir);
                    if d < 4.0 && depth <= front && best.as_ref().is_none_or(|(bd, _)| d < *bd) {
                        best = Some((d, (c.clone(), e.faces)));
                    }
                }
                best.map(|b| b.1)
            });
            if let Some(r) = s.pick_ref(hover, edge) {
                self.mate_pick(r);
            }
        } else {
            // A part inside a subassembly selects the subassembly.
            s.selection = match hover.and_then(|f| s.face(f)).and_then(|(c, _)| c.first().copied()) {
                Some(c) => AsmSelection::Component(c),
                None => AsmSelection::None,
            };
            self.scene_dirty = true;
        }
    }

    pub(super) fn assembly_commands(&mut self, ui: &mut egui::Ui) {
        let selected = self.asm.as_ref().map(|s| s.selection).unwrap_or(AsmSelection::None);
        theme::command_group(ui, "Assembly", |ui| {
            if theme::command_button(ui, Icon::InsertComponent, "Insert Component", None).clicked() {
                self.insert_component();
            }
            let open = self.asm.as_ref().is_some_and(|s| s.mate.is_some());
            if theme::command_button(ui, Icon::Mate, "Mate", Some(open)).clicked() {
                if open {
                    if let Some(s) = &mut self.asm {
                        s.mate = None;
                    }
                } else {
                    self.start_mate();
                }
            }
        });
        theme::group_rule(ui);
        theme::command_group(ui, "Component", |ui| {
            if let AsmSelection::Component(id) = selected {
                let fixed = self.asm.as_ref().and_then(|s| s.asm.component(id)).is_some_and(|c| c.fixed);
                let label = if fixed { "Float" } else { "Fix" };
                if theme::command_button(ui, Icon::Origin, label, None).clicked() {
                    self.set_fixed(id, !fixed);
                }
                let hidden = self.asm.as_ref().and_then(|s| s.asm.component(id)).is_some_and(|c| c.hidden);
                if theme::command_button(ui, Icon::Part, if hidden { "Show" } else { "Hide" }, None).clicked()
                {
                    if let Some(c) = self.asm.as_mut().and_then(|s| s.asm.component_mut(id)) {
                        c.hidden = !hidden;
                    }
                    self.assembly_commit();
                    self.refresh_assembly();
                }
                let sub = self.asm.as_ref().and_then(|s| s.asm.component(id)).filter(|c| c.is_assembly());
                if let Some(flexible) = sub.map(|c| c.flexible) {
                    let label = if flexible { "Make Rigid" } else { "Make Flexible" };
                    let tip = if flexible {
                        "Move the subassembly as one body again"
                    } else {
                        "Solve the subassembly's own mates here, so its parts can move within them"
                    };
                    if theme::command_button(ui, Icon::Assembly, label, Some(flexible))
                        .on_hover_text(tip)
                        .clicked()
                    {
                        if let Err(e) = self.set_flexible(id, !flexible) {
                            self.status = e;
                        }
                    }
                }
            } else {
                ui.label(RichText::new("Select a component").size(11.5).weak());
            }
        });
        theme::group_rule(ui);
        theme::command_group(ui, "Evaluate", |ui| {
            let open = self.asm.as_ref().is_some_and(|s| s.interference.is_some());
            if theme::command_button(ui, Icon::Interference, "Interference", Some(open)).clicked() {
                if open {
                    self.close_interference();
                } else {
                    self.open_interference();
                }
            }
            if theme::command_button(ui, Icon::Properties, "Bill of Materials", None).clicked() {
                if let Some(s) = &mut self.asm {
                    s.bom = !s.bom;
                }
            }
            if theme::command_button(ui, Icon::Rebuild, "Rebuild", None).clicked() {
                // Subassembly files are read again, so changes saved to them show.
                if let Some(s) = &mut self.asm {
                    s.parts.reload_assemblies();
                }
                self.refresh_assembly();
            }
        });
    }

    /// Makes a subassembly flexible or rigid and solves.
    pub(super) fn set_flexible(&mut self, id: u32, flexible: bool) -> Result<(), String> {
        let s = self.asm.as_mut().ok_or("no assembly is open")?;
        s.asm.set_flexible(id, flexible)?;
        let name = s.component_label(id);
        self.assembly_commit();
        self.refresh_assembly();
        self.status = format!("{name} is now {}", if flexible { "flexible" } else { "rigid" });
        Ok(())
    }

    pub(super) fn set_fixed(&mut self, id: u32, fixed: bool) {
        if let Some(c) = self.asm.as_mut().and_then(|s| s.asm.component_mut(id)) {
            c.fixed = fixed;
        }
        self.assembly_commit();
        self.refresh_assembly();
    }

    /// The left pane in assembly mode: the Mate panel when it is open, then the tree.
    pub(super) fn assembly_tree(&mut self, ui: &mut egui::Ui) {
        if self.asm.as_ref().is_some_and(|s| s.mate.is_some()) {
            self.mate_panel(ui);
            ui.add_space(10.0);
            ui.separator();
        } else if self.asm.as_ref().is_some_and(|s| s.interference.is_some()) {
            self.interference_panel(ui);
            ui.add_space(10.0);
            ui.separator();
        }
        let Some(s) = &self.asm else { return };
        let picking = s.mate.is_some();
        let pal = Palette::of(ui);
        ui.spacing_mut().item_spacing.y = 1.0;
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(egui::vec2(20.0, 20.0), Sense::hover());
            theme::paint_icon(ui.painter(), r, Icon::Assembly, pal.text, pal.accent);
            ui.label(RichText::new(s.name()).size(15.0).strong());
        });
        ui.add_space(6.0);
        let mut picked: Option<MateRef> = None;
        let mut select: Option<AsmSelection> = None;
        let mut edit: Option<u32> = None;
        let mut toggle_suppress: Option<u32> = None;
        // The assembly's own planes and origin, which mates can use.
        let own = [
            (Icon::Plane, MateEntity::Plane(PlaneRef::Front)),
            (Icon::Plane, MateEntity::Plane(PlaneRef::Top)),
            (Icon::Plane, MateEntity::Plane(PlaneRef::Right)),
            (Icon::Origin, MateEntity::Origin),
        ];
        let label = |e: &MateEntity| match e {
            MateEntity::Plane(p) => p.label(),
            _ => "Origin".into(),
        };
        let is_picked = |r: &MateRef| s.mate.as_ref().is_some_and(|t| t.picks.contains(r));
        for (icon, e) in own {
            let r = MateRef { inner: Vec::new(), component: None, entity: e };
            let look = RowLook { selected: is_picked(&r), ..Default::default() };
            if theme::tree_row(ui, icon, &label(&e), look).clicked() && picking {
                picked = Some(r);
            }
        }
        for c in &s.asm.components {
            let def = s.status.component(c.id);
            let errors = s.errors_of(c.id);
            let error = !errors.is_empty() || def == Some(Definition::OverDefined);
            let look = RowLook {
                selected: s.selection == AsmSelection::Component(c.id),
                dimmed: c.hidden,
                error,
                indent: 0,
            };
            let flex = if c.flexible { " (flexible)" } else { "" };
            let text = format!("{}{}{flex}", prefix(def), c.name);
            let icon = if c.is_assembly() { Icon::Assembly } else { Icon::Part };
            let mut row = theme::tree_row(ui, icon, &text, look);
            if let Some(Definition::UnderDefined(n)) = def {
                row = row.on_hover_text(format!("Can still move {n} way(s)"));
            }
            if !errors.is_empty() {
                let lines: Vec<String> =
                    errors
                        .iter()
                        .map(|e| {
                            if e.path.len() == 1 {
                                e.error.clone()
                            } else {
                                format!("{}: {}", e.name, e.error)
                            }
                        })
                        .collect();
                row = row.on_hover_text(lines.join("\n"));
            }
            if row.clicked() {
                select = Some(AsmSelection::Component(c.id));
            }
            // A selected component's planes and origin, to mate to.
            if s.selection == AsmSelection::Component(c.id)
                || (picking && s.selection == AsmSelection::Component(c.id))
            {
                for (icon, e) in own {
                    let r = MateRef { inner: Vec::new(), component: Some(c.id), entity: e };
                    let look = RowLook { selected: is_picked(&r), indent: 1, ..Default::default() };
                    if theme::tree_row(ui, icon, &label(&e), look).clicked() && picking {
                        picked = Some(r);
                    }
                }
            }
            // What is inside a subassembly, read only: its own mates live in its file.
            for n in s.nested.iter().filter(|n| n.path.first() == Some(&c.id)) {
                let bad = s.load_errors.iter().find(|e| e.path == n.path);
                let look = RowLook {
                    selected: false,
                    dimmed: c.hidden,
                    error: bad.is_some(),
                    indent: (n.path.len() - 1).min(8) as u8,
                };
                let flex = if n.flexible { " (flexible)" } else { "" };
                let icon = if n.assembly { Icon::Assembly } else { Icon::Part };
                let mut row = theme::tree_row(ui, icon, &format!("{}{flex}", n.name), look);
                row = match bad {
                    Some(e) => row.on_hover_text(&e.error),
                    None => row.on_hover_text(format!("Inside {}; open its file to change it", c.name)),
                };
                if row.clicked() {
                    select = Some(AsmSelection::Component(c.id));
                }
            }
        }
        if !s.asm.mates.is_empty() {
            ui.add_space(4.0);
            let _ = theme::tree_row(ui, Icon::Mate, "Mates", RowLook::default());
            for m in &s.asm.mates {
                let st = s.status.mate(m.id);
                let error = matches!(st, Some(MateState::Unsatisfied(_)) | Some(MateState::Invalid(_)));
                let warn = matches!(st, Some(MateState::Redundant));
                let look = RowLook {
                    selected: s.selection == AsmSelection::Mate(m.id),
                    dimmed: m.suppressed,
                    error: error || warn,
                    indent: 1,
                };
                let text = format!(
                    "{} ({}, {})",
                    m.name,
                    s.component_name(m.a.component),
                    s.component_name(m.b.component)
                );
                let mut row = theme::tree_row(ui, Icon::Mate, &text, look);
                row = match st {
                    Some(MateState::Unsatisfied(r)) => {
                        row.on_hover_text(format!("Not satisfied: off by {r:.4}"))
                    }
                    Some(MateState::Invalid(e)) => row.on_hover_text(e),
                    Some(MateState::Redundant) => row.on_hover_text(
                        "Satisfied, but other mates already fix this value: it over defines the assembly",
                    ),
                    _ => row,
                };
                if row.clicked() {
                    select = Some(AsmSelection::Mate(m.id));
                }
                if row.double_clicked() {
                    edit = Some(m.id);
                }
                row.context_menu(|ui| {
                    if ui.button("Edit").clicked() {
                        edit = Some(m.id);
                        ui.close_menu();
                    }
                    if ui.button(if m.suppressed { "Unsuppress" } else { "Suppress" }).clicked() {
                        toggle_suppress = Some(m.id);
                        ui.close_menu();
                    }
                });
            }
        }
        if let Some(r) = picked {
            self.mate_pick(r);
        }
        if let (Some(sel), Some(s)) = (select, &mut self.asm) {
            s.selection = sel;
            self.scene_dirty = true;
        }
        if let Some(id) = edit {
            self.edit_mate(id);
        }
        if let Some(id) = toggle_suppress {
            if let Some(m) = self.asm.as_mut().and_then(|s| s.asm.mate_mut(id)) {
                m.suppressed = !m.suppressed;
            }
            self.assembly_commit();
            self.refresh_assembly();
        }
    }

    fn mate_panel(&mut self, ui: &mut egui::Ui) {
        let mut apply = false;
        let mut cancel = false;
        let Some(s) = &mut self.asm else { return };
        let labels: Vec<String> =
            s.mate.as_ref().map(|t| t.picks.iter().map(|r| s.ref_label(r)).collect()).unwrap_or_default();
        let Some(t) = &mut s.mate else { return };
        super::panel_title(ui, Icon::Mate, if t.editing.is_some() { "Edit Mate" } else { "Mate" });
        theme::section_label(ui, "Entities to mate");
        if labels.is_empty() {
            ui.label(RichText::new("Click two faces or edges in the view, or planes in the tree.").weak());
        }
        for l in &labels {
            ui.label(l);
        }
        theme::section_label(ui, "Mate type");
        egui::ComboBox::from_id_salt("mate-type")
            .selected_text(MATE_TYPES.get(t.kind).copied().unwrap_or("Coincident"))
            .show_ui(ui, |ui| {
                for (i, n) in MATE_TYPES.iter().enumerate() {
                    ui.selectable_value(&mut t.kind, i, *n);
                }
            });
        if t.uses_value() {
            ui.horizontal(|ui| {
                let unit = if MATE_TYPES.get(t.kind) == Some(&"Angle") { "deg" } else { "mm" };
                ui.label("Value");
                ui.add(egui::TextEdit::singleline(&mut t.value).desired_width(80.0));
                ui.label(unit);
            });
            if MATE_TYPES.get(t.kind) == Some(&"Distance") {
                ui.checkbox(&mut t.flip, "Flip dimension");
            }
        }
        theme::section_label(ui, "Alignment");
        ui.horizontal(|ui| {
            ui.selectable_value(&mut t.alignment, Alignment::Closest, "Closest");
            ui.selectable_value(&mut t.alignment, Alignment::Aligned, "Aligned");
            ui.selectable_value(&mut t.alignment, Alignment::AntiAligned, "Anti-aligned");
        });
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if theme::primary_button(ui, "OK").clicked() {
                apply = true;
            }
            if theme::secondary_button(ui, "Cancel").clicked() {
                cancel = true;
            }
        });
        if cancel {
            s.mate = None;
            self.scene_dirty = true;
        } else if apply {
            if let Err(e) = self.apply_mate() {
                self.status = format!("Mate: {e}");
            }
        }
    }

    pub(super) fn bom_window(&mut self, ctx: &egui::Context) {
        let Some(s) = &mut self.asm else { return };
        if !s.bom {
            return;
        }
        let rows = s.asm.bom(&mut s.parts, s.bom_view);
        let mut open = true;
        let mut view = s.bom_view;
        egui::Window::new("Bill of Materials").open(&mut open).resizable(true).show(ctx, |ui| {
            ui.horizontal(|ui| {
                for v in BomView::ALL {
                    ui.selectable_value(&mut view, v, v.label());
                }
            });
            ui.add_space(4.0);
            egui::Grid::new("bom").num_columns(5).striped(true).show(ui, |ui| {
                for h in ["Item", "Part number", "Description", "Configuration", "Qty"] {
                    ui.strong(h);
                }
                ui.end_row();
                for r in &rows {
                    ui.label(&r.number);
                    ui.horizontal(|ui| {
                        // One level lines up under the subassembly's name, past its icon.
                        ui.add_space((14.0 + ui.spacing().item_spacing.x) * r.level as f32);
                        if r.assembly {
                            let (rect, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), Sense::hover());
                            let pal = Palette::of(ui);
                            theme::paint_icon(ui.painter(), rect, Icon::Assembly, pal.text, pal.accent);
                        }
                        ui.label(&r.part_number);
                    });
                    ui.label(&r.description);
                    // Assemblies have no configurations yet.
                    ui.label(if r.assembly { "" } else { r.configuration.as_deref().unwrap_or("Default") });
                    ui.label(r.quantity.to_string());
                    ui.end_row();
                }
            });
            let note = match view {
                BomView::TopLevel => "Parts and subassemblies of this assembly; not what is inside them.",
                BomView::PartsOnly => "Every part at every depth, counted in the whole assembly.",
                BomView::Indented => "Subassemblies with their parts below them, counted per subassembly.",
            };
            ui.add_space(4.0);
            ui.label(RichText::new(note).size(11.5).weak());
        });
        s.bom_view = view;
        if !open {
            s.bom = false;
        }
    }

    /// The assembly part of the harness snapshot.
    pub(super) fn assembly_snapshot(&self) -> Option<serde_json::Value> {
        use serde_json::json;
        let s = self.asm.as_ref()?;
        let comps: Vec<_> = s
            .asm
            .components
            .iter()
            .map(|c| {
                let def = s.status.component(c.id);
                let inside: Vec<_> = s
                    .nested
                    .iter()
                    .filter(|n| n.path.first() == Some(&c.id))
                    .map(|n| json!({ "path": n.path, "name": s.path_label(&n.path), "assembly": n.assembly }))
                    .collect();
                json!({
                    "id": c.id,
                    "name": c.name,
                    "path": c.path,
                    "kind": if c.is_assembly() { "assembly" } else { "part" },
                    "flexible": c.flexible,
                    "inside": inside,
                    "fixed": c.fixed,
                    "hidden": c.hidden,
                    "origin": [c.placement.origin.x, c.placement.origin.y, c.placement.origin.z],
                    "z_axis": [c.placement.z.x, c.placement.z.y, c.placement.z.z],
                    "definition": match def {
                        Some(Definition::Fixed) => "fixed".to_string(),
                        Some(Definition::FullyDefined) => "fully defined".to_string(),
                        Some(Definition::UnderDefined(n)) => format!("under defined ({n})"),
                        Some(Definition::OverDefined) => "over defined".to_string(),
                        None => "not solved".to_string(),
                    },
                })
            })
            .collect();
        let mates: Vec<_> = s
            .asm
            .mates
            .iter()
            .map(|m| {
                let state = match s.status.mate(m.id) {
                    Some(MateState::Satisfied) | None => "satisfied".to_string(),
                    Some(MateState::Unsatisfied(r)) => format!("not satisfied ({r:.6})"),
                    Some(MateState::Redundant) => "redundant".to_string(),
                    Some(MateState::Invalid(e)) => format!("invalid: {e}"),
                };
                json!({ "id": m.id, "name": m.name, "kind": m.kind.label(), "suppressed": m.suppressed, "state": state })
            })
            .collect();
        let errors: Vec<_> = s
            .load_errors
            .iter()
            .map(|e| json!({ "path": e.path, "name": e.name, "error": e.error }))
            .collect();
        Some(json!({
            "name": s.name(),
            "path": s.path,
            "components": comps,
            "mates": mates,
            "load_errors": errors,
            "bom": s.bom.then(|| s.bom_view.key()),
            "interference": self.interference_snapshot(),
            "mate_panel": s.mate.as_ref().map(|t| json!({
                "type": MATE_TYPES.get(t.kind),
                "picks": t.picks.iter().map(|r| s.ref_label(r)).collect::<Vec<_>>(),
                "value": t.value,
                "flip": t.flip,
            })),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anvil_kernel::{v2, v3};
    use anvil_model::{BodyOp, Dim, Document, EndCondition, ExtrudeFeature, FeatureKind, SketchFeature};

    fn block(w: f64, d: f64, h: f64) -> Document {
        let mut doc = Document::new();
        let mut sf = SketchFeature::new(PlaneRef::Front);
        sf.sketch.add_rectangle(v2(0.0, 0.0), v2(w, d));
        let sk = doc.add_feature(FeatureKind::Sketch(sf));
        doc.add_feature(FeatureKind::Extrude(ExtrudeFeature {
            sketch: sk,
            end: EndCondition::Blind,
            depth: Dim::Value(h),
            reverse: false,
            op: BodyOp::Boss,
            ..Default::default()
        }));
        doc
    }

    fn harness() -> (egui::Context, AnvilApp, PathBuf) {
        let dir = std::env::temp_dir().join(format!("anvil-app-asm-{}-{}", std::process::id(), rand_tag()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        block(40.0, 30.0, 10.0).save(&dir.join("base.anvprt")).expect("save");
        block(10.0, 10.0, 10.0).save(&dir.join("cube.anvprt")).expect("save");
        let ctx = egui::Context::default();
        let mut app = AnvilApp::new(&ctx, None, None, None);
        app.script_file_dialogs();
        (ctx, app, dir)
    }

    /// A tag no other test in this process gets. The clock alone was not enough: on macOS
    /// it moves in whole microseconds, so two tests starting together shared a folder and
    /// one's save renamed the other's temporary file away.
    fn rand_tag() -> String {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let t = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        format!("{t}-{n}")
    }

    fn step(ctx: &egui::Context, app: &mut AnvilApp) {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0))),
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| app.frame(ctx));
    }

    /// The face of a component whose outward normal in the assembly is `n`, as a mate pick.
    fn face_facing(app: &AnvilApp, comp: u32, n: Vec3) -> MateRef {
        face_on(app, &[comp], n)
    }

    /// The face of the part at `path` (inside subassemblies) whose outward normal in the
    /// assembly is `n`, as a mate pick.
    fn face_on(app: &AnvilApp, path: &[u32], n: Vec3) -> MateRef {
        let s = app.asm.as_ref().expect("assembly");
        let f = s
            .body
            .polygons
            .iter()
            .filter(|p| (p.normal - n).length() < 1e-9)
            .filter_map(|p| s.face(p.tag.id))
            .find(|(c, _)| c == path)
            .map(|(_, f)| f)
            .expect("face");
        MateRef::at(path, MateEntity::Face(f))
    }

    /// Issue #93: a subassembly goes in rigid, a mate here can pick a face inside it, it
    /// can be made flexible, the BOM has its three views, a loop is refused and a missing
    /// file is reported on open.
    #[test]
    fn subassemblies_in_the_app() {
        let (ctx, mut app, dir) = harness();
        std::fs::create_dir_all(dir.join("kit")).expect("kit dir");
        let mut kit = Assembly::new();
        let first = kit.add_component("../cube.anvprt", None, Frame::WORLD);
        kit.add_component("../cube.anvprt", None, Frame { origin: v3(10.0, 0.0, 0.0), ..Frame::WORLD });
        kit.save(&dir.join("kit/kit.anvasm")).expect("save kit");
        app.new_assembly();
        app.insert_part(&dir.join("base.anvprt"));
        let k =
            app.insert_part_at(&dir.join("kit/kit.anvasm"), Some(v3(0.0, 0.0, 30.0))).expect("kit goes in");
        step(&ctx, &mut app);
        let s = app.asm.as_ref().expect("assembly");
        assert!(s.load_errors.is_empty(), "{:?}", s.load_errors);
        let inside: Vec<String> = s.nested.iter().map(|n| s.path_label(&n.path)).collect();
        assert_eq!(inside, ["kit<1>/cube<1>", "kit<1>/cube<2>"]);
        let base = s.asm.components[0].id;

        // The kit's first cube sits down on the base: the whole kit moves.
        app.start_mate();
        let (ra, rb) = (face_facing(&app, base, Vec3::Z), face_on(&app, &[k, first], -Vec3::Z));
        assert_eq!(
            app.asm
                .as_ref()
                .map(|s| s.ref_label(&rb))
                .as_deref()
                .map(|l| l.starts_with("kit<1>/cube<1>: Face")),
            Some(true)
        );
        app.mate_pick(ra);
        app.mate_pick(rb);
        app.apply_mate().expect("mate applies");
        let at =
            app.asm.as_ref().and_then(|s| s.asm.component(k)).map(|c| c.placement.origin).unwrap_or_default();
        assert!((at - v3(0.0, 0.0, 10.0)).length() < 1e-6, "{at:?}");

        app.set_flexible(k, true).expect("a subassembly");
        assert!(app.status.contains("kit<1> is now flexible"), "{}", app.status);
        assert!(app.set_flexible(base, true).is_err());
        let snap = app.assembly_snapshot().expect("snapshot");
        assert_eq!(snap["components"][1]["kind"], "assembly");
        assert_eq!(snap["components"][1]["flexible"], true);
        assert_eq!(snap["components"][1]["inside"].as_array().map(Vec::len), Some(2));

        if let Some(s) = &mut app.asm {
            s.bom = true;
            s.bom_view = BomView::Indented;
        }
        step(&ctx, &mut app);
        let s = app.asm.as_mut().expect("assembly");
        let rows: Vec<(String, String, usize)> = s
            .asm
            .bom(&mut s.parts, BomView::Indented)
            .into_iter()
            .map(|r| (r.number, r.part_number, r.quantity))
            .collect();
        let want = [("1", "base", 1), ("2", "kit", 1), ("2.1", "cube", 2)];
        assert_eq!(rows, want.map(|(n, p, q)| (n.to_string(), p.to_string(), q)));

        // An assembly that holds itself is refused, and nothing is added.
        let mut lp = Assembly::new();
        lp.add_component("loop.anvasm", None, Frame::WORLD);
        lp.save(&dir.join("loop.anvasm")).expect("save loop");
        let e = app.insert_part_at(&dir.join("loop.anvasm"), None).expect_err("refused");
        assert_eq!(e, "loop.anvasm contains itself, so it cannot be a component of itself");
        assert_eq!(app.asm.as_ref().map(|s| s.asm.components.len()), Some(2));

        // A file whose subassembly lost a part opens, and says what is missing.
        let mut broken = Assembly::new();
        broken.add_component("base.anvprt", None, Frame::WORLD);
        broken.add_component("kit/kit.anvasm", None, Frame::WORLD);
        broken.save(&dir.join("broken.anvasm")).expect("save");
        std::fs::remove_file(dir.join("cube.anvprt")).expect("remove cube");
        app.open_assembly(&dir.join("broken.anvasm"));
        step(&ctx, &mut app);
        let s = app.asm.as_ref().expect("opened");
        assert_eq!(s.load_errors.len(), 2, "{:?}", s.load_errors);
        assert!(!s.body.is_empty(), "the base still shows");
        let (title, text) = app.message.clone().expect("a report");
        assert_eq!(title, "Some components could not be loaded");
        assert!(text.contains("kit<1>/cube<1>: cube.anvprt is missing"), "{text}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn insert_mate_save_and_reopen() {
        let (ctx, mut app, dir) = harness();
        app.new_assembly();
        app.insert_part(&dir.join("base.anvprt"));
        app.insert_part(&dir.join("cube.anvprt"));
        step(&ctx, &mut app);
        let ids: Vec<u32> =
            app.asm.as_ref().map(|s| s.asm.components.iter().map(|c| c.id).collect()).unwrap_or_default();
        let [base, cube] = ids[..] else { panic!("two components: {ids:?}") };
        // The cube went in beside the base, not on top of it.
        let first = app
            .asm
            .as_ref()
            .and_then(|s| s.asm.component(cube))
            .map(|c| c.placement.origin)
            .unwrap_or_default();
        assert!(first.x > 40.0, "{first:?}");

        for (a, b, kind) in [(Vec3::Z, -Vec3::Z, 0usize), (-Vec3::X, -Vec3::X, 0), (-Vec3::Y, -Vec3::Y, 5)] {
            app.start_mate();
            if let Some(t) = app.asm.as_mut().and_then(|s| s.mate.as_mut()) {
                t.kind = kind;
            }
            let (ra, rb) = (face_facing(&app, base, a), face_facing(&app, cube, b));
            app.mate_pick(ra);
            app.mate_pick(rb);
            if kind == 5 {
                // The distance starts at the current gap: both front faces are at y = 0.
                let v = app
                    .asm
                    .as_ref()
                    .and_then(|s| s.mate.as_ref())
                    .map(|t| t.value.clone())
                    .unwrap_or_default();
                assert_eq!(v, "0");
                if let Some(t) = app.asm.as_mut().and_then(|s| s.mate.as_mut()) {
                    t.value = "4".into();
                }
            }
            app.apply_mate().expect("mate added");
            step(&ctx, &mut app);
        }
        let s = app.asm.as_ref().expect("assembly");
        assert!(s.status.ok(), "{:?}", s.status);
        assert_eq!(s.status.component(cube), Some(Definition::FullyDefined));
        let o = s.asm.component(cube).map(|c| c.placement.origin).unwrap_or_default();
        assert!((o - v3(0.0, -4.0, 10.0)).length() < 1e-6, "{o:?}");
        let snap = app.assembly_snapshot().expect("snapshot");
        assert_eq!(snap["components"][1]["definition"], "fully defined");
        assert_eq!(snap["mates"][2]["name"], "Distance1");

        // Save next to the parts, so the paths become relative, then reopen.
        let file = dir.join("pair.anvasm");
        app.queue_file_dialog(Some(file.clone()));
        app.save_assembly(true);
        let saved = Assembly::load(&file).expect("saved");
        assert_eq!(saved.components[1].path, "cube.anvprt");
        app.leave_assembly();
        app.open_file(&file);
        let s = app.asm.as_ref().expect("reopened");
        assert!(s.status.ok() && !s.is_dirty());
        assert!((s.body.mass_properties().volume - (12000.0 + 1000.0)).abs() < 1e-6);

        // Undo the save-free state: deleting the cube removes its mates; undo brings them back.
        app.asm.as_mut().expect("asm").selection = AsmSelection::Component(cube);
        app.delete_assembly_selection();
        assert!(app.asm.as_ref().is_some_and(|s| s.asm.mates.is_empty()));
        app.assembly_undo(false);
        assert_eq!(app.asm.as_ref().map(|s| s.asm.mates.len()), Some(3));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Issue #66: the Interference panel checks in the background, shows the overlap and
    /// drops results that no longer describe the assembly.
    #[test]
    fn interference_panel_finds_the_overlap_and_follows_changes() {
        let (ctx, mut app, dir) = harness();
        app.new_assembly();
        app.insert_part(&dir.join("base.anvprt"));
        app.insert_part(&dir.join("cube.anvprt"));
        let cube = app.asm.as_ref().and_then(|s| s.asm.components.get(1)).map(|c| c.id).expect("cube");
        let move_cube = |app: &mut AnvilApp, z: f64| {
            if let Some(c) = app.asm.as_mut().and_then(|s| s.asm.component_mut(cube)) {
                c.placement.origin = v3(5.0, 5.0, z);
            }
            app.assembly_commit();
            app.refresh_assembly();
        };
        // Half the cube sinks into the base: 10 x 10 x 5.
        move_cube(&mut app, 5.0);
        let shaded = app.mesh.indices.len();
        app.open_interference();
        assert!(app.asm.as_ref().and_then(|s| s.interference.as_ref()).is_some_and(|t| t.running()));
        app.finish_background_work();
        step(&ctx, &mut app);
        let snap = app.assembly_snapshot().expect("snapshot");
        let r = &snap["interference"]["report"];
        assert_eq!(r["findings"].as_array().map(Vec::len), Some(1), "{snap}");
        assert_eq!(r["findings"][0]["kind"], "overlap");
        assert_eq!(r["findings"][0]["a"], "base<1>");
        assert_eq!(r["findings"][0]["b"], "cube<1>");
        let v = r["findings"][0]["volume"].as_f64().unwrap_or(0.0);
        assert!((v - 500.0).abs() < 1e-6 * 500.0, "{v}");
        assert!(app.status.contains("1 overlap"), "{}", app.status);
        // The view shows the overlap volume (a box: 12 triangles), selected and red.
        assert_eq!(snap["interference"]["selected"], 0);
        assert_eq!(app.mesh.indices.len(), 12);
        let s = app.asm.as_ref().expect("asm");
        assert_eq!(s.failed_faces().len(), 1);
        assert!(s.highlighted().is_empty());

        // Resting the cube on the base changes the assembly: the results go.
        move_cube(&mut app, 10.0);
        let t = app.asm.as_ref().and_then(|s| s.interference.as_ref()).expect("panel");
        assert!(t.report.is_none() && t.note.as_deref().is_some_and(|n| n.contains("changed")));
        assert_eq!(app.mesh.indices.len(), shaded);
        // Now it only touches, which is reported when asked for.
        if let Some(s) = &mut app.asm {
            s.interference_opts.touching = true;
        }
        app.start_interference().expect("starts");
        app.finish_background_work();
        step(&ctx, &mut app);
        let snap = app.assembly_snapshot().expect("snapshot");
        assert_eq!(snap["interference"]["report"]["findings"][0]["kind"], "touching", "{snap}");
        // A bad clearance is refused with a note, not a check.
        if let Some(s) = &mut app.asm {
            s.interference_opts.clearance_on = true;
            s.interference_opts.clearance = "-1".into();
        }
        assert!(app.start_interference().is_err());
        let t = app.asm.as_ref().and_then(|s| s.interference.as_ref()).expect("panel");
        assert!(t.note.as_deref().is_some_and(|n| n.contains("more than 0")), "{:?}", t.note);
        // Starting a mate closes the panel and the view shades the components again.
        app.start_mate();
        let s = app.asm.as_ref().expect("asm");
        assert!(s.interference.is_none() && s.mate.is_some());
        assert_eq!(app.mesh.indices.len(), shaded);
        step(&ctx, &mut app);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn export_3mf_writes_each_component_in_place() {
        let (ctx, mut app, dir) = harness();
        app.new_assembly();
        app.insert_part(&dir.join("base.anvprt"));
        app.insert_part(&dir.join("cube.anvprt"));
        step(&ctx, &mut app);
        let s = app.asm.as_ref().expect("assembly");
        let names: Vec<String> = s.asm.components.iter().map(|c| c.name.clone()).collect();
        let cube_at = s.asm.components[1].placement.origin;

        let file = dir.join("pair.3mf");
        let how = app.write_export(&file).expect("exported");
        assert_eq!(how, " (2 bodies)");
        let parts = anvil_io::threemf::read_parts(&std::fs::read(&file).expect("read")).expect("package");
        let model = parts.iter().find(|p| p.0 == "3D/3dmodel.model").expect("model part");
        let model = String::from_utf8(model.1.clone()).expect("utf-8");
        assert!(model.contains(r#"<metadata name="Title">Assembly1</metadata>"#), "{model}");
        for (k, name) in names.iter().enumerate() {
            let line = format!(
                r#"<object id="{}" type="model" name="{}">"#,
                k + 2,
                name.replace('<', "&lt;").replace('>', "&gt;")
            );
            assert!(model.contains(&line), "{line} in {model}");
        }
        // The cube's far corner is where the assembly placed it.
        let corner = cube_at + v3(10.0, 10.0, 10.0);
        // 3MF coordinates are written to the nearest nanometre.
        let nm = |x: f64| (x * 1e6).round() / 1e6;
        let [x, y, z] = anvil_io::threemf::to_build(corner);
        let vertex = format!(r#"<vertex x="{}" y="{}" z="{}"/>"#, nm(x), nm(y), nm(z));
        assert!(model.contains(&vertex), "{vertex} in {model}");
        assert_eq!(model.matches("<vertex ").count(), 16);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
