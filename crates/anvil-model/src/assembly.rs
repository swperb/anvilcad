//! Assemblies: parts and other assemblies placed relative to each other and held by mates.
//!
//! This follows the specification in `docs/re/assemblies-and-mates.md` ("Assembly
//! structure", "Subassemblies" and their "AnvilCAD decisions"). An [`Assembly`] lists
//! components, each an instance of a part file in one of its configurations, or of another
//! assembly file (a subassembly), with a rigid placement, and the mates between them. The
//! first component inserted is fixed. Mates refer to entities by the same stable names
//! parts use elsewhere (faces and edges by the features that made them, reference geometry
//! by feature id), so they survive the part being rebuilt; an entity inside a subassembly
//! is reached through the ids of the components on the way down to its part.
//!
//! A subassembly is rigid by default: its own mates are solved when it is loaded and it
//! then moves as one body. A flexible one has its components solved in the parent, with
//! its own mates, and the parent keeps where they ended up.
//!
//! Component files are referenced by a path relative to the file of the assembly that
//! holds them and loaded through [`Parts`], which rebuilds each part (and configuration)
//! and loads each subassembly once. An assembly that contains itself is refused, and a
//! missing file leaves its component unloaded but the rest of the assembly whole.

use crate::document::{CustomProperty, Document, FileError, PlaneRef, PropertyKind};
use crate::mates::{self, Alignment, Definition, MateKind, MateState, Relation, Side, SolveReport};
use crate::measure::{self, Pick, Shape};
use crate::rebuild::BuildState;
use anvil_kernel::{Aabb, EdgeRef, FaceId, Frame, Solid, Vec3};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub const ASSEMBLY_FORMAT: &str = "anvil-assembly";
/// The newest assembly format version this build writes and reads. Version 2 added
/// subassemblies: assembly components, flexible solving and mates inside subassemblies.
pub const ASSEMBLY_FORMAT_VERSION: u32 = 2;
/// The version written for an assembly that uses nothing newer, so older builds still
/// open it.
pub const ASSEMBLY_FORMAT_VERSION_BASE: u32 = 1;
pub const ASSEMBLY_EXT: &str = "anvasm";

fn is_false(b: &bool) -> bool {
    !*b
}

/// Where one component of a flexible subassembly sits, relative to the subassembly's
/// frame, as last solved in the parent.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct FlexPlacement {
    /// The component's id in the subassembly.
    pub component: u32,
    pub placement: Frame,
}

/// One placed instance of a part or of a subassembly.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Component {
    pub id: u32,
    /// The instance name: the file's name and an instance number, as `bracket<2>`.
    pub name: String,
    /// The part or assembly file, relative to the assembly file, with `/` separators.
    pub path: String,
    /// The part configuration to use; the part's active one when `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub configuration: Option<String>,
    /// Where the file's own coordinates sit in the assembly.
    pub placement: Frame,
    /// Fixed components never move when mates are solved.
    #[serde(default)]
    pub fixed: bool,
    #[serde(default)]
    pub hidden: bool,
    /// For a subassembly: solve its own mates in this assembly, so its components can
    /// move within their freedom, instead of moving it as one body.
    #[serde(default, skip_serializing_if = "is_false")]
    pub flexible: bool,
    /// For a flexible subassembly: where its components ended up, relative to its frame.
    /// Components not listed sit where the subassembly's own file puts them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub flex_placements: Vec<FlexPlacement>,
}

impl Component {
    /// True when the component is an assembly file.
    pub fn is_assembly(&self) -> bool {
        is_assembly_path(&self.path)
    }

    /// Where the subassembly component `child` (placed at `own` in the subassembly's
    /// file) sits in this flexible subassembly's frame.
    fn child_placement(&self, child: u32, own: Frame) -> Frame {
        if !self.flexible {
            return own;
        }
        self.flex_placements.iter().find(|f| f.component == child).map(|f| f.placement).unwrap_or(own)
    }
}

/// True when `path` names an assembly file.
pub fn is_assembly_path(path: &str) -> bool {
    path.rsplit_once('.').is_some_and(|(_, ext)| ext.eq_ignore_ascii_case(ASSEMBLY_EXT))
}

/// An entity of a part (or of the assembly itself) that a mate refers to.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum MateEntity {
    /// The origin point.
    Origin,
    /// A standard plane, a reference plane or a planar face.
    Plane(PlaneRef),
    /// A reference axis feature.
    Axis(u32),
    /// A reference point feature.
    Point(u32),
    Face(FaceId),
    Edge(EdgeRef),
    /// A vertex, in the part's own coordinates.
    Vertex(Vec3),
}

/// One side of a mate: an entity of a component, or of the assembly when `component` is
/// `None`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MateRef {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub component: Option<u32>,
    /// When `component` is a subassembly and the entity is inside it: the ids of the
    /// components on the way down, each in the assembly above, ending at the part (or
    /// subassembly, for its own planes and origin) that owns the entity.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inner: Vec<u32>,
    pub entity: MateEntity,
}

impl MateRef {
    /// An entity of the component at `path` (top-level id first), or of the assembly
    /// itself when `path` is empty.
    pub fn at(path: &[u32], entity: MateEntity) -> MateRef {
        MateRef {
            component: path.first().copied(),
            inner: path.get(1..).unwrap_or_default().to_vec(),
            entity,
        }
    }

    /// The component path: the top-level id, then the ids inside it.
    pub fn path(&self) -> Vec<u32> {
        self.component.into_iter().chain(self.inner.iter().copied()).collect()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Mate {
    pub id: u32,
    pub name: String,
    pub kind: MateKind,
    #[serde(default)]
    pub alignment: Alignment,
    pub a: MateRef,
    pub b: MateRef,
    #[serde(default)]
    pub suppressed: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Assembly {
    pub format: String,
    pub format_version: u32,
    #[serde(default)]
    pub custom_properties: Vec<CustomProperty>,
    #[serde(default)]
    pub components: Vec<Component>,
    #[serde(default)]
    pub mates: Vec<Mate>,
    pub next_id: u32,
}

impl Default for Assembly {
    fn default() -> Self {
        Assembly::new()
    }
}

/// A part loaded and rebuilt for an assembly.
#[derive(Clone)]
pub struct LoadedPart {
    pub doc: Document,
    pub state: BuildState,
}

/// A subassembly loaded for an assembly and solved with its own mates. Its components'
/// paths are rewritten to be relative to the top assembly's folder, like every path a
/// [`Parts`] is asked for.
#[derive(Clone)]
pub struct LoadedAssembly {
    pub asm: Assembly,
    pub status: AssemblyStatus,
}

/// Loads and rebuilds the parts an assembly uses, each file and configuration once, and
/// loads its subassemblies.
#[derive(Default)]
pub struct Parts {
    /// The directory component paths are relative to.
    pub base: PathBuf,
    /// The top assembly's own file, which no subassembly may contain.
    root: Option<PathBuf>,
    parts: HashMap<(String, Option<String>), Result<LoadedPart, String>>,
    assemblies: HashMap<String, Result<Arc<LoadedAssembly>, String>>,
    /// Subassemblies being loaded, outermost first, to catch one that contains itself.
    loading: Vec<(PathBuf, String)>,
    /// Subassembly files refused because they contain themselves, with why.
    cycles: HashMap<PathBuf, String>,
}

/// Subassemblies nest at most this deep; deeper is reported as an error rather than
/// followed, so a loop that slips past the cycle check cannot run away.
pub const MAX_DEPTH: usize = 64;

/// `path` made absolute and with links resolved where it exists, for comparing files.
fn same_file_key(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// A file's name, for messages.
fn file_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

/// True for `/a`, `\a`, `C:/a` and `C:\a`.
fn is_absolute(path: &str) -> bool {
    let b = path.as_bytes();
    path.starts_with(['/', '\\']) || (b.len() > 2 && b[1] == b':' && (b[2] == b'/' || b[2] == b'\\'))
}

/// `child` (relative to the folder of the file `parent`, or absolute) as a path relative to
/// the folder `parent` is relative to, with `/` separators and `.` and `..` folded away.
pub fn resolve_relative(parent: &str, child: &str) -> String {
    let child = child.replace('\\', "/");
    if is_absolute(&child) {
        return child;
    }
    let parent = parent.replace('\\', "/");
    let dir = parent.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
    let absolute = parent.starts_with('/');
    let mut out: Vec<&str> = Vec::new();
    for seg in dir.split('/').chain(child.split('/')) {
        match seg {
            "" | "." => {}
            ".." if out.last().is_some_and(|s| *s != ".." && !s.ends_with(':')) => {
                out.pop();
            }
            ".." if absolute || out.last().is_some_and(|s| s.ends_with(':')) => {}
            s => out.push(s),
        }
    }
    let joined = out.join("/");
    if absolute {
        format!("/{joined}")
    } else {
        joined
    }
}

impl Parts {
    pub fn new(base: impl Into<PathBuf>) -> Parts {
        Parts { base: base.into(), ..Parts::default() }
    }

    /// For an assembly saved at `file`.
    pub fn for_file(file: &Path) -> Parts {
        let mut p = Parts::new(file.parent().unwrap_or(Path::new(".")));
        p.root = Some(same_file_key(file));
        p
    }

    /// The full path of a component's file.
    pub fn resolve(&self, path: &str) -> PathBuf {
        self.base.join(path)
    }

    /// Adds an already-loaded part under `path`, for parts that are not on disk yet.
    pub fn insert(&mut self, path: &str, configuration: Option<&str>, mut doc: Document) {
        let state = doc.rebuild();
        self.parts
            .insert((path.to_string(), configuration.map(str::to_string)), Ok(LoadedPart { doc, state }));
    }

    /// Adds an assembly under `path`, for assemblies that are not on disk yet. Its
    /// component paths are relative to `path`'s folder, as in a file.
    pub fn insert_assembly(&mut self, path: &str, asm: Assembly) {
        self.assemblies.remove(path);
        let loaded = self.prepare(path, asm);
        self.assemblies.insert(path.to_string(), loaded.map(Arc::new));
    }

    /// Forgets a file so that it is read again from disk. Every subassembly is read
    /// again too, since any of them may hold the file.
    pub fn reload(&mut self, path: &str) {
        self.parts.retain(|(p, _), _| p != path);
        self.reload_assemblies();
    }

    /// Forgets every subassembly, so that they are read again (their parts stay loaded).
    pub fn reload_assemblies(&mut self) {
        self.assemblies.clear();
        self.cycles.clear();
    }

    pub fn get(&mut self, path: &str, configuration: Option<&str>) -> Result<&LoadedPart, String> {
        if is_assembly_path(path) {
            return Err(format!("{} is an assembly, not a part", file_name(path)));
        }
        let key = (path.to_string(), configuration.map(str::to_string));
        if !self.parts.contains_key(&key) {
            let loaded = self.load(path, configuration);
            self.parts.insert(key.clone(), loaded);
        }
        match self.parts.get(&key) {
            Some(Ok(p)) => Ok(p),
            Some(Err(e)) => Err(e.clone()),
            None => Err(format!("{path} is not loaded")),
        }
    }

    fn missing(&self, path: &str, file: &Path) -> Option<String> {
        (!file.exists())
            .then(|| format!("{} is missing: there is no file at {}", file_name(path), file.display()))
    }

    fn load(&self, path: &str, configuration: Option<&str>) -> Result<LoadedPart, String> {
        let file = self.resolve(path);
        if let Some(e) = self.missing(path, &file) {
            return Err(e);
        }
        let mut doc = Document::load(&file).map_err(|e| format!("{}: {e}", file.display()))?;
        if let Some(name) = configuration {
            let id = doc
                .configuration_by_name(name)
                .map(|c| c.id)
                .ok_or_else(|| format!("{path} has no configuration named \"{name}\""))?;
            doc.activate_configuration(id).map_err(|e| format!("{path}: {e}"))?;
        }
        let state = doc.rebuild();
        Ok(LoadedPart { doc, state })
    }

    /// The subassembly at `path` (relative to `base`), loaded and solved.
    pub fn assembly(&mut self, path: &str) -> Result<Arc<LoadedAssembly>, String> {
        if let Some(r) = self.assemblies.get(path) {
            return r.clone();
        }
        let loaded = self.load_assembly(path).map(Arc::new);
        self.assemblies.insert(path.to_string(), loaded.clone());
        loaded
    }

    fn load_assembly(&mut self, path: &str) -> Result<LoadedAssembly, String> {
        let file = self.resolve(path);
        if let Some(e) = self.missing(path, &file) {
            return Err(e);
        }
        let key = same_file_key(&file);
        if let Some(e) = self.cycles.get(&key) {
            return Err(e.clone());
        }
        let start = self.loading.iter().position(|(k, _)| *k == key);
        if start.is_some() || self.root.as_ref() == Some(&key) {
            // The files between this one and its second appearance.
            let between: Vec<&str> =
                self.loading[start.map_or(0, |i| i + 1)..].iter().map(|(_, p)| file_name(p)).collect();
            let name = file_name(path);
            let e = if between.is_empty() {
                format!("{name} contains itself, so it cannot be a component of itself")
            } else {
                format!(
                    "{name} contains itself through {}, so it cannot be a component of itself",
                    between.join(", ")
                )
            };
            self.cycles.insert(key, e.clone());
            return Err(e);
        }
        let asm = Assembly::load(&file).map_err(|e| format!("{}: {e}", file.display()))?;
        if self.loading.len() >= MAX_DEPTH {
            return Err(format!("{} is nested more than {MAX_DEPTH} subassemblies deep", file_name(path)));
        }
        self.loading.push((key.clone(), path.to_string()));
        let loaded = self.prepare(path, asm);
        self.loading.pop();
        // Found to contain itself while it loaded: refused, so nothing can loop through it.
        match self.cycles.get(&key) {
            Some(e) => Err(e.clone()),
            None => loaded,
        }
    }

    /// Rewrites a subassembly's paths to be relative to `base` and solves it.
    fn prepare(&mut self, path: &str, mut asm: Assembly) -> Result<LoadedAssembly, String> {
        for c in &mut asm.components {
            c.path = resolve_relative(path, &c.path);
        }
        // Its own subassemblies load now, while this one is on the loading stack, so a
        // loop back to it is caught.
        for c in asm.components.iter().filter(|c| c.is_assembly()) {
            let _ = self.assembly(&c.path);
        }
        let status = asm.solve(self);
        Ok(LoadedAssembly { asm, status })
    }

    /// True when `path` is the top assembly's own file.
    pub fn is_root(&self, path: &str) -> bool {
        self.root.is_some() && self.root == Some(same_file_key(&self.resolve(path)))
    }

    /// Why the subassembly at `path` cannot be used, when it contains itself at any depth.
    pub fn cycle_in(&mut self, path: &str) -> Option<String> {
        let mut seen = HashSet::new();
        self.find_cycle(path, &mut seen)
    }

    fn find_cycle(&mut self, path: &str, seen: &mut HashSet<String>) -> Option<String> {
        if !seen.insert(path.to_string()) {
            return None;
        }
        let sub = match self.assembly(path) {
            Ok(s) => s,
            Err(e) if self.cycles.contains_key(&same_file_key(&self.resolve(path))) => return Some(e),
            Err(_) => return None,
        };
        for c in sub.asm.components.iter().filter(|c| c.is_assembly()) {
            if let Some(e) = self.find_cycle(&c.path, seen) {
                return Some(e);
            }
        }
        None
    }

    /// The files of `path` and of every subassembly inside it, made absolute.
    pub fn assembly_files(&mut self, path: &str) -> Vec<PathBuf> {
        let mut out = Vec::new();
        self.collect_files(path, &mut out);
        out
    }

    fn collect_files(&mut self, path: &str, out: &mut Vec<PathBuf>) {
        let key = same_file_key(&self.resolve(path));
        if out.contains(&key) {
            return;
        }
        out.push(key);
        if let Ok(sub) = self.assembly(path) {
            for c in sub.asm.components.iter().filter(|c| c.is_assembly()) {
                self.collect_files(&c.path, out);
            }
        }
    }
}

/// The three views of a bill of materials (see "Bills of materials" in the spec).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum BomView {
    /// The assembly's own components: parts and subassemblies, not what is inside them.
    #[default]
    TopLevel,
    /// Every part at every depth, with its total quantity; no subassemblies.
    PartsOnly,
    /// The top level, with each subassembly's rows indented below it.
    Indented,
}

impl BomView {
    pub const ALL: [BomView; 3] = [BomView::TopLevel, BomView::PartsOnly, BomView::Indented];

    pub fn label(self) -> &'static str {
        match self {
            BomView::TopLevel => "Top level",
            BomView::PartsOnly => "Parts only",
            BomView::Indented => "Indented",
        }
    }

    /// `top`, `parts` or `indented`, as scripts name the views.
    pub fn key(self) -> &'static str {
        match self {
            BomView::TopLevel => "top",
            BomView::PartsOnly => "parts",
            BomView::Indented => "indented",
        }
    }

    pub fn from_key(s: &str) -> Option<BomView> {
        match s.trim().to_ascii_lowercase().replace(['-', '_', ' '], "").as_str() {
            "top" | "toplevel" => Some(BomView::TopLevel),
            "parts" | "partsonly" => Some(BomView::PartsOnly),
            "indented" => Some(BomView::Indented),
            _ => None,
        }
    }
}

/// One line of a bill of materials.
#[derive(Clone, Debug, PartialEq)]
pub struct BomRow {
    /// The row's number among the rows of its level (and parent), from 1.
    pub item: usize,
    /// The item number as shown: `2`, or `2.1` for the first row inside item 2.
    pub number: String,
    /// How deep the row is: 0 for the assembly's own components.
    pub level: usize,
    /// True for a subassembly.
    pub assembly: bool,
    /// The file's `PartNo` property, or its file name when that is empty.
    pub part_number: String,
    pub description: String,
    /// The file, relative to the top assembly's folder.
    pub path: String,
    pub configuration: Option<String>,
    /// Instances: in the whole assembly for parts-only rows, in the parent otherwise.
    pub quantity: usize,
}

/// The outcome of solving an assembly's mates.
#[derive(Clone, Debug, PartialEq)]
pub struct AssemblyStatus {
    /// Per mate, in the assembly's order. Suppressed mates are `Satisfied`.
    pub mates: Vec<(u32, MateState)>,
    /// Per component, in the assembly's order.
    pub components: Vec<(u32, Definition)>,
}

impl AssemblyStatus {
    pub fn mate(&self, id: u32) -> Option<&MateState> {
        self.mates.iter().find(|m| m.0 == id).map(|m| &m.1)
    }

    pub fn component(&self, id: u32) -> Option<Definition> {
        self.components.iter().find(|c| c.0 == id).map(|c| c.1)
    }

    pub fn ok(&self) -> bool {
        self.mates.iter().all(|m| m.1 == MateState::Satisfied)
    }
}

/// A part placed in an assembly, at any depth.
#[derive(Clone, Debug, PartialEq)]
pub struct PlacedPart {
    /// The ids of the components on the way down: the top-level component first.
    pub path: Vec<u32>,
    /// The instance names on the way down, joined by `/`, as `arm<1>/link<2>`.
    pub name: String,
    /// The part file, relative to the top assembly's folder.
    pub file: String,
    pub configuration: Option<String>,
    /// Where the part's own coordinates sit in the top assembly.
    pub placement: Frame,
    /// True when it or a subassembly above it is hidden.
    pub hidden: bool,
}

/// A component at any depth that could not be loaded.
#[derive(Clone, Debug, PartialEq)]
pub struct LoadError {
    pub path: Vec<u32>,
    /// The instance names on the way down, as for [`PlacedPart::name`].
    pub name: String,
    pub error: String,
}

/// The shape of a mate entity on a rebuilt part, in the part's coordinates.
pub fn entity_shape(st: Option<&BuildState>, e: &MateEntity) -> Result<Shape, String> {
    let plane = |f: Frame| Shape::Plane { origin: f.origin, normal: f.z };
    match (*e, st) {
        (MateEntity::Origin, _) => Ok(Shape::Point(Vec3::ZERO)),
        (MateEntity::Vertex(p), _) => Ok(Shape::Point(p)),
        (MateEntity::Plane(p), _) if p.standard_frame().is_some() => {
            Ok(plane(p.standard_frame().unwrap_or(Frame::WORLD)))
        }
        (_, None) => Err("an assembly has only its origin and standard planes to mate to".into()),
        (MateEntity::Plane(PlaneRef::Face(f)), Some(st)) | (MateEntity::Face(f), Some(st)) => {
            measure::shape_of(st, &Pick::Face(f))
        }
        (MateEntity::Plane(PlaneRef::Feature(id)), Some(st)) => {
            st.frames.get(&id).map(|f| plane(*f)).ok_or_else(|| format!("plane #{id} is not in the part"))
        }
        (MateEntity::Plane(_), Some(_)) => Err("unsupported plane".into()),
        (MateEntity::Axis(id), Some(st)) => st
            .axes
            .get(&id)
            .map(|&(origin, dir)| Shape::Line { origin, dir: dir.normalized() })
            .ok_or_else(|| format!("axis #{id} is not in the part")),
        (MateEntity::Point(id), Some(st)) => st
            .points
            .get(&id)
            .map(|&p| Shape::Point(p))
            .ok_or_else(|| format!("point #{id} is not in the part")),
        (MateEntity::Edge(edge), Some(st)) => measure::shape_of(st, &Pick::Edge(edge)),
    }
}

/// `s`, given in `f`'s coordinates, in the coordinates `f` is placed in.
pub fn place_shape(f: &Frame, s: &Shape) -> Shape {
    let p = |q: Vec3| f.to_world3(q);
    let d = |q: Vec3| f.dir_to_world(q);
    match *s {
        Shape::Point(q) => Shape::Point(p(q)),
        Shape::Line { origin, dir } => Shape::Line { origin: p(origin), dir: d(dir) },
        Shape::Plane { origin, normal } => Shape::Plane { origin: p(origin), normal: d(normal) },
        Shape::Circle { center, normal, radius } => {
            Shape::Circle { center: p(center), normal: d(normal), radius }
        }
        Shape::Cylinder { origin, axis, radius } => {
            Shape::Cylinder { origin: p(origin), axis: d(axis), radius }
        }
        Shape::Sphere { center, radius } => Shape::Sphere { center: p(center), radius },
    }
}

/// `file`'s name without its extension, or the whole path.
fn stem(path: &str) -> &str {
    let file = file_name(path);
    file.rsplit_once('.').map(|(s, _)| s).filter(|s| !s.is_empty()).unwrap_or(file)
}

/// Joins solids into one, polygons and all.
fn merged(solids: Vec<Solid>) -> Solid {
    Solid::new(solids.into_iter().flat_map(|s| s.polygons).collect())
}

/// The body of a part or assembly file as a component would show it, in the file's own
/// coordinates: for an assembly, its visible parts at every depth, placed.
pub fn file_solid(parts: &mut Parts, path: &str, configuration: Option<&str>) -> Result<Solid, String> {
    if !is_assembly_path(path) {
        return Ok(parts.get(path, configuration)?.state.body.clone());
    }
    if let Some(e) = parts.cycle_in(path) {
        return Err(e);
    }
    let sub = parts.assembly(path)?;
    let (placed, _) = sub.asm.placed_parts(parts);
    let mut solids = Vec::new();
    for p in placed.iter().filter(|p| !p.hidden) {
        if let Ok(part) = parts.get(&p.file, p.configuration.as_deref()) {
            solids.push(part.state.body.transformed(&p.placement));
        }
    }
    Ok(merged(solids))
}

impl Assembly {
    pub fn new() -> Assembly {
        let prop =
            |n: &str| CustomProperty { name: n.into(), kind: PropertyKind::Text, value: String::new() };
        Assembly {
            format: ASSEMBLY_FORMAT.into(),
            format_version: ASSEMBLY_FORMAT_VERSION_BASE,
            custom_properties: vec![prop("Description"), prop("PartNo"), prop("Revision"), prop("DrawnBy")],
            components: Vec::new(),
            mates: Vec::new(),
            next_id: 1,
        }
    }

    /// True when the assembly uses anything format version 1 cannot hold.
    pub fn uses_subassemblies(&self) -> bool {
        self.components.iter().any(|c| c.is_assembly() || c.flexible || !c.flex_placements.is_empty())
            || self.mates.iter().any(|m| !m.a.inner.is_empty() || !m.b.inner.is_empty())
    }

    /// The assembly as saved, in the oldest format version that holds everything it uses.
    pub fn to_json(&self) -> Result<String, FileError> {
        let mut a = self.clone();
        a.format_version =
            if a.uses_subassemblies() { ASSEMBLY_FORMAT_VERSION } else { ASSEMBLY_FORMAT_VERSION_BASE };
        Ok(serde_json::to_string_pretty(&a)?)
    }

    /// Reads an assembly file of any version this build knows. Version 1 files read
    /// as they are: everything version 2 added defaults to off or empty.
    pub fn from_json(s: &str) -> Result<Assembly, FileError> {
        let v: serde_json::Value = serde_json::from_str(s)?;
        let format = v.get("format").and_then(|f| f.as_str()).unwrap_or("");
        if format != ASSEMBLY_FORMAT {
            return Err(FileError::WrongFormat(format.into()));
        }
        let ver = v.get("format_version").and_then(|f| f.as_u64()).unwrap_or(0) as u32;
        if ver > ASSEMBLY_FORMAT_VERSION {
            return Err(FileError::TooNew(ver));
        }
        let mut a: Assembly = serde_json::from_value(v)?;
        // The version is worked out again on saving, from what the assembly uses.
        a.format_version = ASSEMBLY_FORMAT_VERSION_BASE;
        for c in &mut a.components {
            c.path = c.path.replace('\\', "/");
        }
        let top = a.components.iter().map(|c| c.id).chain(a.mates.iter().map(|m| m.id)).max().unwrap_or(0);
        a.next_id = a.next_id.max(top + 1);
        Ok(a)
    }

    pub fn save(&self, path: &Path) -> Result<(), FileError> {
        let tmp = path.with_extension("anvasm.tmp");
        std::fs::write(&tmp, self.to_json()?)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    pub fn load(path: &Path) -> Result<Assembly, FileError> {
        Assembly::from_json(&std::fs::read_to_string(path)?)
    }

    fn take_id(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    pub fn component(&self, id: u32) -> Option<&Component> {
        self.components.iter().find(|c| c.id == id)
    }

    pub fn component_mut(&mut self, id: u32) -> Option<&mut Component> {
        self.components.iter_mut().find(|c| c.id == id)
    }

    pub fn mate(&self, id: u32) -> Option<&Mate> {
        self.mates.iter().find(|m| m.id == id)
    }

    pub fn mate_mut(&mut self, id: u32) -> Option<&mut Mate> {
        self.mates.iter_mut().find(|m| m.id == id)
    }

    /// Places an instance of the part or assembly at `path` (relative to the assembly
    /// file). The first component is fixed at the assembly origin, whatever `placement`
    /// says. Use [`Assembly::check_insert`] first for an assembly file.
    pub fn add_component(&mut self, path: &str, configuration: Option<&str>, placement: Frame) -> u32 {
        let id = self.take_id();
        let base = stem(path).to_string();
        let n = (1..)
            .find(|n| {
                let name = format!("{base}<{n}>");
                !self.components.iter().any(|c| c.name == name)
            })
            .unwrap_or(1);
        let first = self.components.is_empty();
        self.components.push(Component {
            id,
            name: format!("{base}<{n}>"),
            path: path.replace('\\', "/"),
            configuration: configuration.map(str::to_string),
            placement: if first { Frame::WORLD } else { placement },
            fixed: first,
            hidden: false,
            flexible: false,
            flex_placements: Vec::new(),
        });
        id
    }

    /// Why the file at `path` cannot be inserted: a missing or unreadable file, or an
    /// assembly that contains itself or this assembly.
    pub fn check_insert(&self, parts: &mut Parts, path: &str) -> Result<(), String> {
        if !is_assembly_path(path) {
            return parts.get(path, None).map(|_| ());
        }
        if parts.is_root(path) {
            return Err(format!(
                "{} contains itself, so it cannot be a component of itself",
                file_name(path)
            ));
        }
        parts.assembly(path)?;
        match parts.cycle_in(path) {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    /// Removes a component and the mates that refer to it; returns the mates removed.
    pub fn delete_component(&mut self, id: u32) -> Vec<u32> {
        self.components.retain(|c| c.id != id);
        let gone: Vec<u32> = self
            .mates
            .iter()
            .filter(|m| m.a.component == Some(id) || m.b.component == Some(id))
            .map(|m| m.id)
            .collect();
        self.mates.retain(|m| !gone.contains(&m.id));
        gone
    }

    /// Makes a subassembly flexible or rigid. Turning it rigid forgets where its
    /// components were moved to. Fails for a part.
    pub fn set_flexible(&mut self, id: u32, flexible: bool) -> Result<(), String> {
        let c = self.component_mut(id).ok_or_else(|| format!("component #{id} is missing"))?;
        if !c.is_assembly() {
            return Err(format!("{} is a part; only a subassembly can be flexible", c.name));
        }
        c.flexible = flexible;
        if !flexible {
            c.flex_placements.clear();
        }
        Ok(())
    }

    /// Adds a mate named after its kind (`Coincident1`, `Distance2`, ...).
    pub fn add_mate(&mut self, kind: MateKind, alignment: Alignment, a: MateRef, b: MateRef) -> u32 {
        let id = self.take_id();
        let label = kind.label();
        let n = (1..)
            .find(|n| {
                let name = format!("{label}{n}");
                !self.mates.iter().any(|m| m.name == name)
            })
            .unwrap_or(1);
        self.mates.push(Mate { id, name: format!("{label}{n}"), kind, alignment, a, b, suppressed: false });
        id
    }

    pub fn delete_mate(&mut self, id: u32) {
        self.mates.retain(|m| m.id != id);
    }

    /// The shape of an entity of component `c` (or of a component inside it, through
    /// `inner`), in `c`'s own coordinates.
    fn component_shape(
        parts: &mut Parts,
        c: &Component,
        inner: &[u32],
        e: &MateEntity,
    ) -> Result<Shape, String> {
        if !c.is_assembly() {
            if !inner.is_empty() {
                return Err(format!("{} is a part; it has no components inside", c.name));
            }
            let part =
                parts.get(&c.path, c.configuration.as_deref()).map_err(|e| format!("{}: {e}", c.name))?;
            return entity_shape(Some(&part.state), e).map_err(|e| format!("{}: {e}", c.name));
        }
        let Some((first, rest)) = inner.split_first() else {
            return entity_shape(None, e).map_err(|e| format!("{}: {e}", c.name));
        };
        let sub = parts.assembly(&c.path).map_err(|e| format!("{}: {e}", c.name))?;
        let d =
            sub.asm.component(*first).ok_or_else(|| format!("{}: component #{first} is missing", c.name))?;
        let shape = Assembly::component_shape(parts, d, rest, e).map_err(|e| format!("{}/{e}", c.name))?;
        Ok(place_shape(&c.child_placement(d.id, d.placement), &shape))
    }

    /// The shape of one side of a mate, in that (top-level) component's coordinates.
    pub fn ref_shape(&self, parts: &mut Parts, r: &MateRef) -> Result<Shape, String> {
        match r.component {
            None => entity_shape(None, &r.entity),
            Some(cid) => {
                let c = self.component(cid).ok_or_else(|| format!("component #{cid} is missing"))?;
                Assembly::component_shape(parts, c, &r.inner, &r.entity)
            }
        }
    }

    /// The alignment a new mate between `a` and `b` should have to leave the components
    /// as close as they are: aligned when their directions already point the same way.
    pub fn closest_alignment(&self, parts: &mut Parts, a: &MateRef, b: &MateRef) -> Alignment {
        let dir = |s: Shape| match s {
            Shape::Plane { normal, .. } | Shape::Circle { normal, .. } => Some(normal),
            Shape::Line { dir, .. } => Some(dir),
            Shape::Cylinder { axis, .. } => Some(axis),
            _ => None,
        };
        let world_dir = |r: &MateRef, parts: &mut Parts| {
            let s = self.ref_shape(parts, r).ok()?;
            let d = dir(s)?;
            let f = r.component.and_then(|c| self.component(c)).map(|c| c.placement).unwrap_or(Frame::WORLD);
            Some(f.dir_to_world(d))
        };
        match (world_dir(a, parts), world_dir(b, parts)) {
            (Some(da), Some(db)) if da.dot(db) < 0.0 => Alignment::AntiAligned,
            (Some(_), Some(_)) => Alignment::Aligned,
            _ => Alignment::Closest,
        }
    }

    /// What a distance or angle mate between `a` and `b` would measure where the
    /// components are now (see [`mates::measure_now`]).
    pub fn measure_now(
        &self,
        parts: &mut Parts,
        kind: &MateKind,
        alignment: Alignment,
        a: &MateRef,
        b: &MateRef,
    ) -> Option<f64> {
        let place = |r: &MateRef| match r.component {
            Some(c) => self.component(c).map(|c| c.placement),
            None => Some(Frame::WORLD),
        };
        let (sa, sb) = (self.ref_shape(parts, a).ok()?, self.ref_shape(parts, b).ok()?);
        mates::measure_now(kind, alignment, (&place(a)?, &sa), (&place(b)?, &sb))
    }

    /// Solves the mates and moves the components that are not fixed. The components of
    /// flexible subassemblies are solved here too, with their own mates, and where they
    /// end up is kept in [`Component::flex_placements`].
    pub fn solve(&mut self, parts: &mut Parts) -> AssemblyStatus {
        let index: HashMap<u32, usize> = self.components.iter().enumerate().map(|(i, c)| (c.id, i)).collect();
        let mut placements: Vec<Frame> = self.components.iter().map(|c| c.placement).collect();
        let mut fixed: Vec<bool> = self.components.iter().map(|c| c.fixed).collect();
        let mut relations = Vec::new();
        // The bodies of flexible subassemblies' components: (subassembly index, the
        // subassembly, its component's id) for each placement after the top-level ones.
        let mut flex: Vec<(usize, Arc<LoadedAssembly>, u32)> = Vec::new();
        let mut flex_index: HashMap<(usize, u32), usize> = HashMap::new();
        for (i, c) in self.components.iter().enumerate().filter(|(_, c)| c.flexible && c.is_assembly()) {
            let Ok(sub) = parts.assembly(&c.path) else { continue };
            for d in &sub.asm.components {
                flex_index.insert((i, d.id), placements.len());
                placements.push(mates::compose(&c.placement, &c.child_placement(d.id, d.placement)));
                fixed.push(false);
                flex.push((i, sub.clone(), d.id));
            }
            // Components fixed in the subassembly hold to its frame.
            for d in sub.asm.components.iter().filter(|d| d.fixed) {
                let origin = Shape::Point(Vec3::ZERO);
                relations.push(Relation {
                    kind: MateKind::Lock { relative: d.placement },
                    alignment: Alignment::Closest,
                    a: Side { component: Some(i), shape: origin },
                    b: Side { component: flex_index.get(&(i, d.id)).copied(), shape: origin },
                });
            }
            // Its own mates; a side on the subassembly's own planes uses its frame.
            for m in sub.asm.mates.iter().filter(|m| !m.suppressed) {
                let side = |r: &MateRef, parts: &mut Parts| -> Option<Side> {
                    let shape = sub.asm.ref_shape(parts, r).ok()?;
                    let component = match r.component {
                        None => Some(i),
                        Some(d) => Some(*flex_index.get(&(i, d))?),
                    };
                    Some(Side { component, shape })
                };
                if let (Some(a), Some(b)) = (side(&m.a, parts), side(&m.b, parts)) {
                    relations.push(Relation { kind: m.kind, alignment: m.alignment, a, b });
                }
            }
        }
        let inner_relations = relations.len();
        let mut owners = Vec::new();
        let mut states: Vec<(u32, MateState)> = Vec::new();
        for m in &self.mates {
            if m.suppressed {
                states.push((m.id, MateState::Satisfied));
                continue;
            }
            let side = |r: &MateRef, parts: &mut Parts| -> Result<Side, String> {
                let Some(c) = r.component else {
                    return Ok(Side { component: None, shape: self.ref_shape(parts, r)? });
                };
                let i = *index.get(&c).ok_or_else(|| format!("component #{c} is missing"))?;
                let comp = &self.components[i];
                if let (Some((first, rest)), true) = (r.inner.split_first(), comp.flexible) {
                    // Inside a flexible subassembly: on that component's own body.
                    if let Some(&k) = flex_index.get(&(i, *first)) {
                        let sub = &flex[k - self.components.len()].1;
                        let d = sub
                            .asm
                            .component(*first)
                            .ok_or_else(|| format!("{}: component #{first} is missing", comp.name))?;
                        let shape = Assembly::component_shape(parts, d, rest, &r.entity)
                            .map_err(|e| format!("{}/{e}", comp.name))?;
                        return Ok(Side { component: Some(k), shape });
                    }
                }
                Ok(Side { component: Some(i), shape: self.ref_shape(parts, r)? })
            };
            match (side(&m.a, parts), side(&m.b, parts)) {
                (Ok(a), Ok(b)) => {
                    relations.push(Relation { kind: m.kind, alignment: m.alignment, a, b });
                    owners.push(states.len());
                    states.push((m.id, MateState::Satisfied));
                }
                (Err(e), _) | (_, Err(e)) => states.push((m.id, MateState::Invalid(e))),
            }
        }
        let SolveReport { mates: solved, components } = mates::solve(&mut placements, &fixed, &relations);
        for (k, st) in solved.into_iter().skip(inner_relations).enumerate() {
            if let Some(&o) = owners.get(k) {
                states[o].1 = st;
            }
        }
        let n = self.components.len();
        for (c, p) in self.components.iter_mut().zip(&placements) {
            c.placement = *p;
        }
        // Only subassemblies that loaded are solved; the rest keep what they had.
        let solved_flex: HashSet<usize> = flex.iter().map(|f| f.0).collect();
        for i in &solved_flex {
            self.components[*i].flex_placements.clear();
        }
        for ((i, _, d), p) in flex.iter().zip(placements.iter().skip(n)) {
            let c = &mut self.components[*i];
            let placement = mates::relative(&c.placement, p);
            c.flex_placements.push(FlexPlacement { component: *d, placement });
        }
        let mut components: Vec<(u32, Definition)> =
            self.components.iter().map(|c| c.id).zip(components).collect();
        // A component on a mate that could not be resolved is not known to be defined.
        for (m, st) in self.mates.iter().zip(&states) {
            if matches!(st.1, MateState::Invalid(_)) {
                for cid in [m.a.component, m.b.component].into_iter().flatten() {
                    if let Some(c) = components.iter_mut().find(|c| c.0 == cid) {
                        if c.1 == Definition::FullyDefined {
                            c.1 = Definition::UnderDefined(1);
                        }
                    }
                }
            }
        }
        AssemblyStatus { mates: states, components }
    }

    /// A lock mate that holds `b` where it is now relative to `a`.
    pub fn lock(&self, a: Option<u32>, b: u32) -> Option<MateKind> {
        let fa = match a {
            Some(a) => self.component(a)?.placement,
            None => Frame::WORLD,
        };
        let fb = self.component(b)?.placement;
        Some(MateKind::Lock { relative: mates::relative(&fa, &fb) })
    }

    /// Every part in the assembly, at every depth, placed in this assembly's coordinates,
    /// and the components (at any depth) whose file could not be loaded.
    pub fn placed_parts(&self, parts: &mut Parts) -> (Vec<PlacedPart>, Vec<LoadError>) {
        let mut placed = Vec::new();
        let mut errors = Vec::new();
        self.place_into(parts, &Frame::WORLD, &[], "", false, None, &mut placed, &mut errors);
        (placed, errors)
    }

    #[allow(clippy::too_many_arguments)]
    fn place_into(
        &self,
        parts: &mut Parts,
        outer: &Frame,
        prefix: &[u32],
        names: &str,
        hidden: bool,
        flex: Option<&Component>,
        placed: &mut Vec<PlacedPart>,
        errors: &mut Vec<LoadError>,
    ) {
        for c in &self.components {
            let own = flex.map(|f| f.child_placement(c.id, c.placement)).unwrap_or(c.placement);
            let at = mates::compose(outer, &own);
            let mut path = prefix.to_vec();
            path.push(c.id);
            let name = if names.is_empty() { c.name.clone() } else { format!("{names}/{}", c.name) };
            let hidden = hidden || c.hidden;
            if !c.is_assembly() {
                if let Err(error) = parts.get(&c.path, c.configuration.as_deref()) {
                    errors.push(LoadError { path, name, error });
                    continue;
                }
                placed.push(PlacedPart {
                    path,
                    name,
                    file: c.path.clone(),
                    configuration: c.configuration.clone(),
                    placement: at,
                    hidden,
                });
                continue;
            }
            if path.len() > MAX_DEPTH {
                let error = format!("{} is nested more than {MAX_DEPTH} subassemblies deep", c.name);
                errors.push(LoadError { path, name, error });
                continue;
            }
            match parts.assembly(&c.path) {
                Ok(sub) => sub.asm.place_into(parts, &at, &path, &name, hidden, Some(c), placed, errors),
                Err(error) => errors.push(LoadError { path, name, error }),
            }
        }
    }

    /// The component at `path` (top-level id first) and the names on the way, joined by
    /// `/`; `None` when a step is missing.
    pub fn path_name(&self, parts: &mut Parts, path: &[u32]) -> Option<String> {
        let (first, rest) = path.split_first()?;
        let c = self.component(*first)?;
        if rest.is_empty() {
            return Some(c.name.clone());
        }
        let sub = parts.assembly(&c.path).ok()?;
        Some(format!("{}/{}", c.name, sub.asm.path_name(parts, rest)?))
    }

    /// The component path named by instance names joined by `/` (`arm<1>/link<2>`), or by
    /// a top-level id.
    pub fn path_named(&self, parts: &mut Parts, name: &str) -> Option<Vec<u32>> {
        let (head, tail) = match name.split_once('/') {
            Some((h, t)) => (h, Some(t)),
            None => (name, None),
        };
        let c = self.components.iter().find(|c| c.name == head || c.id.to_string() == head)?;
        let mut path = vec![c.id];
        if let Some(tail) = tail {
            let sub = parts.assembly(&c.path).ok()?;
            path.extend(sub.asm.path_named(parts, tail)?);
        }
        Some(path)
    }

    /// The bodies of the visible components, placed in the assembly: for a subassembly,
    /// its visible parts at every depth joined into one solid.
    pub fn bodies(&self, parts: &mut Parts) -> Vec<(u32, Result<Solid, String>)> {
        let (placed, errors) = self.placed_parts(parts);
        self.components
            .iter()
            .filter(|c| !c.hidden)
            .map(|c| {
                let mine = |p: &[u32]| p.first() == Some(&c.id);
                if let Some(e) = errors.iter().find(|e| e.path.len() == 1 && mine(&e.path)) {
                    return (c.id, Err(e.error.clone()));
                }
                let mut solids = Vec::new();
                for p in placed.iter().filter(|p| mine(&p.path) && !p.hidden) {
                    if let Ok(part) = parts.get(&p.file, p.configuration.as_deref()) {
                        solids.push(part.state.body.transformed(&p.placement));
                    }
                }
                let body = if c.is_assembly() { merged(solids) } else { solids.pop().unwrap_or_default() };
                (c.id, Ok(body))
            })
            .collect()
    }

    /// The bounds of everything visible, in assembly coordinates.
    pub fn bounds(&self, parts: &mut Parts) -> Aabb {
        let mut b = Aabb::empty();
        for (_, s) in self.bodies(parts) {
            if let Ok(s) = s {
                let sb = s.bounds();
                if !sb.is_empty() {
                    b.add(sb.min);
                    b.add(sb.max);
                }
            }
        }
        b
    }

    /// The parts list: one row per part file and configuration, in the order they were
    /// first inserted, with how many instances there are. Subassemblies are rows of their
    /// own. The same as `bom(parts, BomView::TopLevel)`.
    pub fn bill_of_materials(&self, parts: &mut Parts) -> Vec<BomRow> {
        self.bom(parts, BomView::TopLevel)
    }

    /// The bill of materials in one of its views (see "Bills of materials" in the spec).
    pub fn bom(&self, parts: &mut Parts, view: BomView) -> Vec<BomRow> {
        match view {
            BomView::TopLevel => self.bom_level(parts, 0, "", false),
            BomView::Indented => self.bom_level(parts, 0, "", true),
            BomView::PartsOnly => {
                let (placed, _) = self.placed_parts(parts);
                let mut rows: Vec<BomRow> = Vec::new();
                for p in &placed {
                    if let Some(row) =
                        rows.iter_mut().find(|r| r.path == p.file && r.configuration == p.configuration)
                    {
                        row.quantity += 1;
                        continue;
                    }
                    let (part_number, description) =
                        file_properties(parts, &p.file, p.configuration.as_deref());
                    let item = rows.len() + 1;
                    rows.push(BomRow {
                        item,
                        number: item.to_string(),
                        level: 0,
                        assembly: false,
                        part_number,
                        description,
                        path: p.file.clone(),
                        configuration: p.configuration.clone(),
                        quantity: 1,
                    });
                }
                rows
            }
        }
    }

    /// One level of the BOM: a row per file and configuration among this assembly's own
    /// components, and with `indent`, each subassembly's rows below it.
    fn bom_level(&self, parts: &mut Parts, level: usize, prefix: &str, indent: bool) -> Vec<BomRow> {
        let mut rows: Vec<BomRow> = Vec::new();
        for c in &self.components {
            if let Some(row) =
                rows.iter_mut().find(|r| r.path == c.path && r.configuration == c.configuration)
            {
                row.quantity += 1;
                continue;
            }
            let (part_number, description) = file_properties(parts, &c.path, c.configuration.as_deref());
            let item = rows.len() + 1;
            rows.push(BomRow {
                item,
                number: if prefix.is_empty() { item.to_string() } else { format!("{prefix}.{item}") },
                level,
                assembly: c.is_assembly(),
                part_number,
                description,
                path: c.path.clone(),
                configuration: c.configuration.clone(),
                quantity: 1,
            });
        }
        if !indent {
            return rows;
        }
        let mut out = Vec::new();
        for row in rows {
            let children = match row.assembly && level < MAX_DEPTH {
                true => match parts.assembly(&row.path) {
                    Ok(sub) => sub.asm.bom_level(parts, level + 1, &row.number, true),
                    Err(_) => Vec::new(),
                },
                false => Vec::new(),
            };
            out.push(row);
            out.extend(children);
        }
        out
    }
}

/// A file's part number (its `PartNo`, or its name) and description, for the BOM.
fn file_properties(parts: &mut Parts, path: &str, configuration: Option<&str>) -> (String, String) {
    let props: Vec<CustomProperty> = if is_assembly_path(path) {
        parts.assembly(path).map(|a| a.asm.custom_properties.clone()).unwrap_or_default()
    } else {
        parts.get(path, configuration).map(|p| p.doc.custom_properties.clone()).unwrap_or_default()
    };
    let prop =
        |name: &str| props.iter().find(|q| q.name == name).map(|q| q.value.clone()).unwrap_or_default();
    let number =
        Some(prop("PartNo")).filter(|s| !s.trim().is_empty()).unwrap_or_else(|| stem(path).to_string());
    (number, prop("Description"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{BodyOp, Dim, EndCondition, ExtrudeFeature, FeatureKind, SketchFeature};
    use anvil_kernel::{v2, v3};

    /// A `w` x `d` x `h` block with its corner at the origin, extruded up from the front
    /// plane (xy) along +z.
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

    fn faces(parts: &mut Parts, path: &str) -> Vec<(FaceId, Shape)> {
        let st = &parts.get(path, None).expect("part loads").state;
        st.body
            .face_ids()
            .into_iter()
            .filter_map(|f| measure::shape_of(st, &Pick::Face(f)).ok().map(|s| (f, s)))
            .collect()
    }

    /// The planar face whose outward normal is `n`.
    fn face(parts: &mut Parts, path: &str, n: Vec3) -> FaceId {
        faces(parts, path)
            .into_iter()
            .find(|(_, s)| matches!(s, Shape::Plane { normal, .. } if (*normal - n).length() < 1e-9))
            .map(|(f, _)| f)
            .expect("face with that normal")
    }

    fn sample() -> (Assembly, Parts, u32, u32) {
        let mut parts = Parts::new(".");
        parts.insert("base.anvprt", None, block(40.0, 30.0, 10.0));
        parts.insert("cube.anvprt", None, block(10.0, 10.0, 10.0));
        let mut asm = Assembly::new();
        let base = asm.add_component("base.anvprt", None, Frame::WORLD);
        let cube =
            asm.add_component("cube.anvprt", None, Frame { origin: v3(13.0, 7.0, 35.0), ..Frame::WORLD });
        (asm, parts, base, cube)
    }

    #[test]
    fn components_get_instance_names_and_the_first_is_fixed() {
        let (mut asm, _, base, cube) = sample();
        let again = asm.add_component("sub/cube.anvprt", None, Frame::WORLD);
        assert_eq!(asm.component(base).map(|c| (c.name.as_str(), c.fixed)), Some(("base<1>", true)));
        assert_eq!(asm.component(cube).map(|c| (c.name.as_str(), c.fixed)), Some(("cube<1>", false)));
        assert_eq!(asm.component(again).map(|c| c.name.as_str()), Some("cube<2>"));
    }

    #[test]
    fn mating_a_cube_into_the_corner_of_a_base() {
        let (mut asm, mut parts, base, cube) = sample();
        let on = |c: u32, f: FaceId| MateRef {
            inner: Vec::new(),
            component: Some(c),
            entity: MateEntity::Face(f),
        };
        let (b_top, c_bottom) =
            (face(&mut parts, "base.anvprt", Vec3::Z), face(&mut parts, "cube.anvprt", -Vec3::Z));
        let (b_left, c_left) =
            (face(&mut parts, "base.anvprt", -Vec3::X), face(&mut parts, "cube.anvprt", -Vec3::X));
        let (b_front, c_front) =
            (face(&mut parts, "base.anvprt", -Vec3::Y), face(&mut parts, "cube.anvprt", -Vec3::Y));
        let align = asm.closest_alignment(&mut parts, &on(base, b_top), &on(cube, c_bottom));
        assert_eq!(align, Alignment::AntiAligned);
        asm.add_mate(MateKind::Coincident, align, on(base, b_top), on(cube, c_bottom));
        let status = asm.solve(&mut parts);
        assert!(status.ok(), "{status:?}");
        assert_eq!(status.component(cube), Some(Definition::UnderDefined(3)));
        assert!(
            (asm.component(cube).map(|c| c.placement.origin).unwrap_or_default() - v3(13.0, 7.0, 10.0))
                .length()
                < 1e-6
        );

        asm.add_mate(MateKind::Coincident, Alignment::Aligned, on(base, b_left), on(cube, c_left));
        asm.add_mate(
            MateKind::Distance { value: 5.0, flip: false },
            Alignment::Aligned,
            on(base, b_front),
            on(cube, c_front),
        );
        let status = asm.solve(&mut parts);
        assert!(status.ok(), "{status:?}");
        assert_eq!(status.component(base), Some(Definition::Fixed));
        assert_eq!(status.component(cube), Some(Definition::FullyDefined));
        let p = asm.component(cube).map(|c| c.placement.origin).unwrap_or_default();
        // The front faces have outward normal -y: 5 mm along it is y = -5.
        assert!((p - v3(0.0, -5.0, 10.0)).length() < 1e-6, "{p:?}");
        let names: Vec<&str> = asm.mates.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["Coincident1", "Coincident2", "Distance1"]);

        // Placed bodies: the cube's volume is 1000 mm^3 and its top is at z = 20.
        let bodies = asm.bodies(&mut parts);
        let cube_body =
            bodies.iter().find(|b| b.0 == cube).and_then(|b| b.1.as_ref().ok()).expect("cube body");
        assert!((cube_body.mass_properties().volume - 1000.0).abs() < 1e-6);
        assert!((cube_body.bounds().max.z - 20.0).abs() < 1e-6);
    }

    #[test]
    fn mates_to_missing_entities_are_invalid_and_deleting_a_component_removes_its_mates() {
        let (mut asm, mut parts, base, cube) = sample();
        let bogus = FaceId { feature: 99, index: 0 };
        let m = asm.add_mate(
            MateKind::Coincident,
            Alignment::Closest,
            MateRef { inner: Vec::new(), component: Some(base), entity: MateEntity::Face(bogus) },
            MateRef { inner: Vec::new(), component: Some(cube), entity: MateEntity::Origin },
        );
        let status = asm.solve(&mut parts);
        assert!(matches!(status.mate(m), Some(MateState::Invalid(_))));
        assert_eq!(asm.delete_component(cube), vec![m]);
        assert!(asm.mates.is_empty());
    }

    #[test]
    fn standard_planes_and_lock() {
        let (mut asm, mut parts, base, cube) = sample();
        // Unfix the base and put its right plane on the assembly's right plane, then lock
        // the cube to the base where it is.
        if let Some(c) = asm.component_mut(base) {
            c.fixed = false;
            c.placement.origin = v3(9.0, 0.0, 0.0);
        }
        let right = MateEntity::Plane(PlaneRef::Right);
        asm.add_mate(
            MateKind::Coincident,
            Alignment::Aligned,
            MateRef { inner: Vec::new(), component: None, entity: right },
            MateRef { inner: Vec::new(), component: Some(base), entity: right },
        );
        let lock = asm.lock(Some(base), cube).expect("lock");
        asm.add_mate(
            lock,
            Alignment::Closest,
            MateRef { inner: Vec::new(), component: Some(base), entity: MateEntity::Origin },
            MateRef { inner: Vec::new(), component: Some(cube), entity: MateEntity::Origin },
        );
        let status = asm.solve(&mut parts);
        assert!(status.ok(), "{status:?}");
        let b = asm.component(base).map(|c| c.placement.origin).unwrap_or_default();
        let c = asm.component(cube).map(|c| c.placement.origin).unwrap_or_default();
        assert!(b.x.abs() < 1e-6, "{b:?}");
        assert!((c - v3(4.0, 7.0, 35.0)).length() < 1e-6, "{c:?}");
        // The base keeps the three motions in the plane, and the cube goes with it.
        assert_eq!(status.component(base), Some(Definition::UnderDefined(3)));
        assert_eq!(status.component(cube), Some(Definition::UnderDefined(3)));
    }

    #[test]
    fn bill_of_materials_counts_instances() {
        let (mut asm, mut parts, _, _) = sample();
        asm.add_component("cube.anvprt", None, Frame::WORLD);
        let mut numbered = block(10.0, 10.0, 10.0);
        if let Some(p) = numbered.custom_properties.iter_mut().find(|p| p.name == "PartNo") {
            p.value = "AC-1001".into();
        }
        parts.insert("pin.anvprt", None, numbered);
        asm.add_component("pin.anvprt", None, Frame::WORLD);
        let bom = asm.bill_of_materials(&mut parts);
        let rows: Vec<(usize, &str, usize)> =
            bom.iter().map(|r| (r.item, r.part_number.as_str(), r.quantity)).collect();
        assert_eq!(rows, [(1, "base", 1), (2, "cube", 2), (3, "AC-1001", 1)]);
    }

    #[test]
    fn files_round_trip() {
        let dir = std::env::temp_dir().join(format!("anvil-asm-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        block(40.0, 30.0, 10.0).save(&dir.join("base.anvprt")).expect("save part");
        block(10.0, 10.0, 10.0).save(&dir.join("cube.anvprt")).expect("save part");
        let (mut asm, _, base, cube) = sample();
        asm.add_mate(
            MateKind::Distance { value: 2.5, flip: true },
            Alignment::AntiAligned,
            MateRef { inner: Vec::new(), component: Some(base), entity: MateEntity::Plane(PlaneRef::Top) },
            MateRef {
                inner: Vec::new(),
                component: Some(cube),
                entity: MateEntity::Vertex(v3(1.0, 2.0, 3.0)),
            },
        );
        let file = dir.join("a.anvasm");
        asm.save(&file).expect("save assembly");
        let back = Assembly::load(&file).expect("load assembly");
        assert_eq!(back, asm);
        let mut parts = Parts::for_file(&file);
        let status = back.clone().solve(&mut parts);
        assert!(status.ok(), "{status:?}");
        assert!(matches!(
            Assembly::from_json(&Document::new().to_json().unwrap()),
            Err(FileError::WrongFormat(_))
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A frame at `o`, turned 90 degrees about z (x goes to y).
    fn turned(o: Vec3) -> Frame {
        Frame { origin: o, x: v3(0.0, 1.0, 0.0), y: v3(-1.0, 0.0, 0.0), z: Vec3::Z }
    }

    fn close(a: Vec3, b: Vec3) -> bool {
        (a - b).length() < 1e-6
    }

    /// `sub/arm.anvasm`: a fixed 20 x 10 x 5 plate and a 2 x 2 x 8 peg at (3, 4, 5).
    fn arm(parts: &mut Parts) -> (u32, u32) {
        parts.insert("sub/plate.anvprt", None, block(20.0, 10.0, 5.0));
        parts.insert("sub/peg.anvprt", None, block(2.0, 2.0, 8.0));
        let mut arm = Assembly::new();
        let plate = arm.add_component("plate.anvprt", None, Frame::WORLD);
        let peg = arm.add_component("peg.anvprt", None, Frame { origin: v3(3.0, 4.0, 5.0), ..Frame::WORLD });
        parts.insert_assembly("sub/arm.anvasm", arm);
        (plate, peg)
    }

    #[test]
    fn relative_paths_resolve_through_subassembly_folders() {
        assert_eq!(resolve_relative("sub/arm.anvasm", "peg.anvprt"), "sub/peg.anvprt");
        assert_eq!(resolve_relative("sub/arm.anvasm", "../base.anvprt"), "base.anvprt");
        assert_eq!(resolve_relative("a/b/c.anvasm", "./d/../e.anvprt"), "a/b/e.anvprt");
        assert_eq!(resolve_relative("c.anvasm", "../up.anvprt"), "../up.anvprt");
        assert_eq!(resolve_relative("/abs/c.anvasm", "x\\y.anvprt"), "/abs/x/y.anvprt");
        assert_eq!(resolve_relative("sub/c.anvasm", "C:\\parts\\p.anvprt"), "C:/parts/p.anvprt");
        assert_eq!(resolve_relative("C:/work/c.anvasm", "../p.anvprt"), "C:/p.anvprt");
    }

    #[test]
    fn nested_parts_are_placed_through_their_subassembly() {
        let mut parts = Parts::new(".");
        parts.insert("base.anvprt", None, block(40.0, 30.0, 10.0));
        let (plate, peg) = arm(&mut parts);
        let mut asm = Assembly::new();
        let base = asm.add_component("base.anvprt", None, Frame::WORLD);
        let arm = asm.add_component("sub/arm.anvasm", None, turned(v3(10.0, 0.0, 20.0)));
        assert_eq!(asm.check_insert(&mut parts, "sub/arm.anvasm"), Ok(()));
        let (placed, errors) = asm.placed_parts(&mut parts);
        assert!(errors.is_empty(), "{errors:?}");
        let names: Vec<&str> = placed.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["base<1>", "arm<1>/plate<1>", "arm<1>/peg<1>"]);
        assert_eq!(placed[2].path, vec![arm, peg]);
        assert_eq!(placed[2].file, "sub/peg.anvprt");
        // (3, 4, 5) in the arm, turned 90 degrees about z and moved to (10, 0, 20).
        assert!(close(placed[2].placement.origin, v3(6.0, 3.0, 25.0)), "{:?}", placed[2].placement);
        assert!(close(placed[2].placement.x, v3(0.0, 1.0, 0.0)));

        // A parent mate on the plate inside the arm sets the arm down on the base.
        let plate_bottom = face(&mut parts, "sub/plate.anvprt", -Vec3::Z);
        let base_top = face(&mut parts, "base.anvprt", Vec3::Z);
        let inside = MateRef::at(&[arm, plate], MateEntity::Face(plate_bottom));
        assert_eq!(inside.path(), vec![arm, plate]);
        let on_base =
            MateRef { inner: Vec::new(), component: Some(base), entity: MateEntity::Face(base_top) };
        assert_eq!(asm.closest_alignment(&mut parts, &on_base, &inside), Alignment::AntiAligned);
        let m = asm.add_mate(MateKind::Coincident, Alignment::AntiAligned, on_base, inside);
        let status = asm.solve(&mut parts);
        assert_eq!(status.mate(m), Some(&MateState::Satisfied), "{status:?}");
        let at = asm.component(arm).map(|c| c.placement.origin).unwrap_or_default();
        assert!(close(at, v3(10.0, 0.0, 10.0)), "{at:?}");
        let (placed, _) = asm.placed_parts(&mut parts);
        assert!(close(placed[2].placement.origin, v3(6.0, 3.0, 15.0)), "{:?}", placed[2].placement);
        assert_eq!(asm.path_name(&mut parts, &[arm, peg]).as_deref(), Some("arm<1>/peg<1>"));
        assert_eq!(asm.path_named(&mut parts, "arm<1>/peg<1>"), Some(vec![arm, peg]));
        // The arm's bodies come back as one solid: 1000 + 32 mm^3, its top at z = 23.
        let bodies = asm.bodies(&mut parts);
        let arm_body = bodies.iter().find(|b| b.0 == arm).and_then(|b| b.1.as_ref().ok()).expect("arm body");
        assert!((arm_body.mass_properties().volume - 1032.0).abs() < 1e-6);
        assert!((arm_body.bounds().max.z - 23.0).abs() < 1e-6);
        // An entity path into a part is refused plainly.
        let wrong = MateRef::at(&[base, 1], MateEntity::Origin);
        assert!(asm.ref_shape(&mut parts, &wrong).is_err_and(|e| e.contains("is a part")));
    }

    /// `sub/slide.anvasm`: a fixed 40 x 10 x 5 rail and a 10 x 10 x 5 car on it, held
    /// on its top and front faces so that it slides along x.
    fn slide(parts: &mut Parts) -> (u32, u32) {
        parts.insert("sub/rail.anvprt", None, block(40.0, 10.0, 5.0));
        parts.insert("sub/car.anvprt", None, block(10.0, 10.0, 5.0));
        let mut s = Assembly::new();
        let rail = s.add_component("rail.anvprt", None, Frame::WORLD);
        let car = s.add_component("car.anvprt", None, Frame { origin: v3(0.0, 0.0, 5.0), ..Frame::WORLD });
        let on = |c: u32, f: FaceId| MateRef {
            inner: Vec::new(),
            component: Some(c),
            entity: MateEntity::Face(f),
        };
        let (rt, cb) = (face(parts, "sub/rail.anvprt", Vec3::Z), face(parts, "sub/car.anvprt", -Vec3::Z));
        let (rf, cf) = (face(parts, "sub/rail.anvprt", -Vec3::Y), face(parts, "sub/car.anvprt", -Vec3::Y));
        s.add_mate(MateKind::Coincident, Alignment::AntiAligned, on(rail, rt), on(car, cb));
        s.add_mate(MateKind::Coincident, Alignment::Aligned, on(rail, rf), on(car, cf));
        parts.insert_assembly("sub/slide.anvasm", s);
        (rail, car)
    }

    #[test]
    fn a_flexible_subassembly_solves_its_own_mates_in_the_parent() {
        let mut parts = Parts::new(".");
        parts.insert("stop.anvprt", None, block(5.0, 10.0, 5.0));
        let (rail, car) = slide(&mut parts);
        let mut asm = Assembly::new();
        let s = asm.add_component("sub/slide.anvasm", None, Frame::WORLD);
        let stop =
            asm.add_component("stop.anvprt", None, Frame { origin: v3(30.0, 0.0, 5.0), ..Frame::WORLD });
        if let Some(c) = asm.component_mut(stop) {
            c.fixed = true;
        }
        // The car's right face against the stop's left face.
        let car_right = face(&mut parts, "sub/car.anvprt", Vec3::X);
        let stop_left = face(&mut parts, "stop.anvprt", -Vec3::X);
        let m = asm.add_mate(
            MateKind::Coincident,
            Alignment::AntiAligned,
            MateRef { inner: Vec::new(), component: Some(stop), entity: MateEntity::Face(stop_left) },
            MateRef::at(&[s, car], MateEntity::Face(car_right)),
        );
        // Rigid, the car stays where the subassembly's file puts it.
        asm.solve(&mut parts);
        let (placed, _) = asm.placed_parts(&mut parts);
        let car_at = placed.iter().find(|p| p.path == [s, car]).map(|p| p.placement.origin);
        assert!(car_at.is_some_and(|p| close(p, v3(0.0, 0.0, 5.0))), "{car_at:?}");
        assert_eq!(
            asm.set_flexible(stop, true).err().as_deref(),
            Some("stop<1> is a part; only a subassembly can be flexible")
        );
        asm.set_flexible(s, true).expect("a subassembly");
        let status = asm.solve(&mut parts);
        assert!(status.ok(), "{status:?}");
        assert_eq!(status.mate(m), Some(&MateState::Satisfied));
        let c = asm.component(s).expect("slide").clone();
        assert!(close(c.placement.origin, Vec3::ZERO), "{:?}", c.placement);
        let at = |id: u32| c.flex_placements.iter().find(|f| f.component == id).map(|f| f.placement.origin);
        assert!(at(car).is_some_and(|p| close(p, v3(20.0, 0.0, 5.0))), "{:?}", c.flex_placements);
        assert!(at(rail).is_some_and(|p| close(p, Vec3::ZERO)), "{:?}", c.flex_placements);
        let (placed, _) = asm.placed_parts(&mut parts);
        let car_at = placed.iter().find(|p| p.path == [s, car]).map(|p| p.placement.origin);
        assert!(car_at.is_some_and(|p| close(p, v3(20.0, 0.0, 5.0))), "{car_at:?}");
        // The subassembly's own file still has the car where it was.
        let sub = parts.assembly("sub/slide.anvasm").expect("slide loads");
        assert!(close(
            sub.asm.component(car).map(|c| c.placement.origin).unwrap_or_default(),
            v3(0.0, 0.0, 5.0)
        ));
        // Rigid again: the moved positions are forgotten.
        asm.set_flexible(s, false).expect("a subassembly");
        assert!(asm.component(s).is_some_and(|c| c.flex_placements.is_empty()));
    }

    #[test]
    fn bill_of_materials_at_each_level() {
        let mut parts = Parts::new(".");
        parts.insert("base.anvprt", None, block(40.0, 30.0, 10.0));
        let mut pin = block(2.0, 2.0, 8.0);
        if let Some(p) = pin.custom_properties.iter_mut().find(|p| p.name == "PartNo") {
            p.value = "AC-7".into();
        }
        parts.insert("sub/pin.anvprt", None, pin);
        parts.insert("sub/link.anvprt", None, block(20.0, 10.0, 5.0));
        let mut sub = Assembly::new();
        sub.add_component("link.anvprt", None, Frame::WORLD);
        sub.add_component("link.anvprt", None, Frame::WORLD);
        sub.add_component("pin.anvprt", None, Frame::WORLD);
        if let Some(p) = sub.custom_properties.iter_mut().find(|p| p.name == "Description") {
            p.value = "Linkage".into();
        }
        parts.insert_assembly("sub/linkage.anvasm", sub);
        let mut asm = Assembly::new();
        asm.add_component("base.anvprt", None, Frame::WORLD);
        asm.add_component("sub/linkage.anvasm", None, Frame::WORLD);
        asm.add_component("sub/linkage.anvasm", None, Frame::WORLD);
        type Row<'a> = (&'a str, usize, bool, &'a str, &'a str, usize);
        let rows = |v: BomView, parts: &mut Parts| -> Vec<(String, usize, bool, String, String, usize)> {
            asm.bom(parts, v)
                .into_iter()
                .map(|r| (r.number, r.level, r.assembly, r.part_number, r.description, r.quantity))
                .collect()
        };
        let want = |w: &[Row]| -> Vec<(String, usize, bool, String, String, usize)> {
            w.iter().map(|r| (r.0.into(), r.1, r.2, r.3.into(), r.4.into(), r.5)).collect()
        };
        assert_eq!(
            rows(BomView::TopLevel, &mut parts),
            want(&[("1", 0, false, "base", "", 1), ("2", 0, true, "linkage", "Linkage", 2)])
        );
        assert_eq!(asm.bill_of_materials(&mut parts), asm.bom(&mut parts, BomView::TopLevel));
        assert_eq!(
            rows(BomView::PartsOnly, &mut parts),
            want(&[
                ("1", 0, false, "base", "", 1),
                ("2", 0, false, "link", "", 4),
                ("3", 0, false, "AC-7", "", 2)
            ])
        );
        assert_eq!(
            rows(BomView::Indented, &mut parts),
            want(&[
                ("1", 0, false, "base", "", 1),
                ("2", 0, true, "linkage", "Linkage", 2),
                ("2.1", 1, false, "link", "", 2),
                ("2.2", 1, false, "AC-7", "", 1),
            ])
        );
        let paths: Vec<String> = asm.bom(&mut parts, BomView::Indented).into_iter().map(|r| r.path).collect();
        assert_eq!(paths, ["base.anvprt", "sub/linkage.anvasm", "sub/link.anvprt", "sub/pin.anvprt"]);
        for v in BomView::ALL {
            assert_eq!(BomView::from_key(v.key()), Some(v));
        }
        assert_eq!(BomView::from_key("Parts-only"), Some(BomView::PartsOnly));
    }

    fn temp_dir(tag: &str) -> PathBuf {
        static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("anvil-sub-{tag}-{}-{n}", std::process::id()));
        std::fs::create_dir_all(dir.join("sub")).expect("temp dir");
        dir
    }

    #[test]
    fn an_assembly_that_contains_itself_is_refused() {
        let dir = temp_dir("cycle");
        block(10.0, 10.0, 10.0).save(&dir.join("cube.anvprt")).expect("save part");
        // a.anvasm holds sub/b.anvasm, which holds a.anvasm again.
        let mut a = Assembly::new();
        a.add_component("cube.anvprt", None, Frame::WORLD);
        a.add_component("sub/b.anvasm", None, Frame::WORLD);
        a.save(&dir.join("a.anvasm")).expect("save a");
        let mut b = Assembly::new();
        b.add_component("../cube.anvprt", None, Frame::WORLD);
        b.add_component("../a.anvasm", None, Frame::WORLD);
        b.save(&dir.join("sub/b.anvasm")).expect("save b");

        // Opened from a top assembly beside them, a refuses to go in.
        let top = Assembly::new();
        let mut parts = Parts::for_file(&dir.join("top.anvasm"));
        let e = top.check_insert(&mut parts, "a.anvasm").expect_err("a loop");
        assert_eq!(e, "a.anvasm contains itself through b.anvasm, so it cannot be a component of itself");
        assert!(file_solid(&mut parts, "a.anvasm", None).is_err());

        // Opening a itself still works: everything but the loop loads, and it is reported.
        let mut a = Assembly::load(&dir.join("a.anvasm")).expect("load a");
        let mut parts = Parts::for_file(&dir.join("a.anvasm"));
        a.solve(&mut parts);
        let (placed, errors) = a.placed_parts(&mut parts);
        let names: Vec<&str> = placed.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["cube<1>", "b<1>/cube<1>"]);
        let [err] = errors.as_slice() else { panic!("{errors:?}") };
        assert_eq!(err.name, "b<1>/a<1>");
        assert_eq!(
            err.error,
            "a.anvasm contains itself through b.anvasm, so it cannot be a component of itself"
        );
        // And it cannot be inserted into itself.
        assert_eq!(
            a.check_insert(&mut parts, "a.anvasm").err().as_deref(),
            Some("a.anvasm contains itself, so it cannot be a component of itself")
        );
        let bom = a.bom(&mut parts, BomView::Indented);
        assert_eq!(bom.len(), 4, "{bom:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_child_file_leaves_the_rest_loaded() {
        let dir = temp_dir("missing");
        block(10.0, 10.0, 10.0).save(&dir.join("sub/cube.anvprt")).expect("save part");
        block(40.0, 30.0, 10.0).save(&dir.join("base.anvprt")).expect("save part");
        let mut sub = Assembly::new();
        sub.add_component("cube.anvprt", None, Frame::WORLD);
        sub.add_component("gone.anvprt", None, Frame::WORLD);
        sub.save(&dir.join("sub/kit.anvasm")).expect("save sub");
        let mut top = Assembly::new();
        top.add_component("base.anvprt", None, Frame::WORLD);
        let kit =
            top.add_component("sub/kit.anvasm", None, Frame { origin: v3(0.0, 0.0, 10.0), ..Frame::WORLD });
        let lost = top.add_component("nowhere.anvasm", None, Frame::WORLD);
        let file = dir.join("top.anvasm");
        top.save(&file).expect("save top");

        let mut asm = Assembly::load(&file).expect("opens");
        let mut parts = Parts::for_file(&file);
        let status = asm.solve(&mut parts);
        assert_eq!(status.components.len(), 3);
        let (placed, errors) = asm.placed_parts(&mut parts);
        let names: Vec<&str> = placed.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["base<1>", "kit<1>/cube<1>"]);
        assert!(close(placed[1].placement.origin, v3(0.0, 0.0, 10.0)));
        let errs: Vec<(&str, bool)> =
            errors.iter().map(|e| (e.name.as_str(), e.error.contains("is missing"))).collect();
        assert_eq!(errs, [("kit<1>/gone<1>", true), ("nowhere<1>", true)]);
        assert!(
            errors[0].error.starts_with("gone.anvprt is missing: there is no file at "),
            "{}",
            errors[0].error
        );
        assert_eq!(errors[1].path, vec![lost]);
        // The kit still has a body; the lost component reports why it has none.
        let bodies = asm.bodies(&mut parts);
        assert!(bodies.iter().any(|(id, b)| *id == kit && b.as_ref().is_ok_and(|s| !s.is_empty())));
        assert!(bodies.iter().any(|(id, b)| *id == lost && b.is_err()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn subassemblies_save_and_reopen_as_format_version_2() {
        let dir = temp_dir("round");
        block(40.0, 10.0, 5.0).save(&dir.join("sub/rail.anvprt")).expect("save");
        block(10.0, 10.0, 5.0).save(&dir.join("sub/car.anvprt")).expect("save");
        block(5.0, 10.0, 5.0).save(&dir.join("stop.anvprt")).expect("save");
        let mut parts = Parts::new(&dir);
        let (_, car) = slide(&mut parts);
        let sub = parts.assembly("sub/slide.anvasm").expect("slide").asm.clone();
        // Saved with paths relative to its own folder again.
        let mut on_disk = sub.clone();
        for c in &mut on_disk.components {
            c.path = c.path.trim_start_matches("sub/").to_string();
        }
        on_disk.save(&dir.join("sub/slide.anvasm")).expect("save sub");
        assert!(std::fs::read_to_string(dir.join("sub/slide.anvasm"))
            .expect("read")
            .contains("\"format_version\": 1"));

        let mut asm = Assembly::new();
        let s = asm.add_component("sub/slide.anvasm", None, Frame::WORLD);
        let stop =
            asm.add_component("stop.anvprt", None, Frame { origin: v3(30.0, 0.0, 5.0), ..Frame::WORLD });
        if let Some(c) = asm.component_mut(stop) {
            c.fixed = true;
        }
        asm.set_flexible(s, true).expect("flexible");
        let mut parts = Parts::for_file(&dir.join("top.anvasm"));
        let car_right = face(&mut parts, "sub/car.anvprt", Vec3::X);
        let stop_left = face(&mut parts, "stop.anvprt", -Vec3::X);
        asm.add_mate(
            MateKind::Coincident,
            Alignment::AntiAligned,
            MateRef { inner: Vec::new(), component: Some(stop), entity: MateEntity::Face(stop_left) },
            MateRef::at(&[s, car], MateEntity::Face(car_right)),
        );
        assert!(asm.solve(&mut parts).ok());
        let file = dir.join("top.anvasm");
        asm.save(&file).expect("save top");
        let text = std::fs::read_to_string(&file).expect("read");
        assert!(
            text.contains("\"format_version\": 2")
                && text.contains("\"flexible\": true")
                && text.contains("\"inner\"")
        );
        let mut back = Assembly::load(&file).expect("reopen");
        assert_eq!(back, asm);
        let mut parts = Parts::for_file(&file);
        assert!(back.solve(&mut parts).ok());
        let (placed, errors) = back.placed_parts(&mut parts);
        assert!(errors.is_empty(), "{errors:?}");
        let car_at = placed.iter().find(|p| p.path == [s, car]).map(|p| p.placement.origin);
        assert!(car_at.is_some_and(|p| close(p, v3(20.0, 0.0, 5.0))), "{car_at:?}");
        // Too new for this build is refused plainly; version 1 still reads.
        let newer = text.replace("\"format_version\": 2", "\"format_version\": 3");
        assert!(matches!(Assembly::from_json(&newer), Err(FileError::TooNew(3))));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
