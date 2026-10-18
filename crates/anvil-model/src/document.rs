//! The part document: feature tree, global variables, material and custom properties.

use crate::equations::{eval, eval_all, EvalError};
use crate::material::Material;
use anvil_kernel::{v3, EdgeRef, FaceId, Frame, Vec3};
use anvil_sketch::Sketch;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const PART_FORMAT: &str = "anvil-part";
/// Format ids written by earlier builds that are still read.
pub const LEGACY_PART_FORMATS: &[&str] = &["ferrocad-part"];
pub const PART_FORMAT_VERSION: u32 = 1;
/// Native part file extension (".anvprt"); templates use ".anvprtdot".
pub const PART_EXT: &str = "anvprt";
pub const PART_TEMPLATE_EXT: &str = "anvprtdot";

/// A dimension that is either a literal value or an expression over global variables.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Dim {
    Value(f64),
    Expr(String),
}

impl Dim {
    pub fn eval(&self, vars: &BTreeMap<String, f64>) -> Result<f64, EvalError> {
        match self {
            Dim::Value(v) => Ok(*v),
            Dim::Expr(e) => eval(e, &|n| vars.get(n).copied()),
        }
    }

    /// Parses user input: a plain number becomes a value, anything else an expression.
    pub fn parse(s: &str) -> Dim {
        match s.trim().parse::<f64>() {
            Ok(v) => Dim::Value(v),
            Err(_) => Dim::Expr(s.trim().trim_start_matches('=').trim().to_string()),
        }
    }

    pub fn text(&self) -> String {
        match self {
            Dim::Value(v) => format!("{v}"),
            Dim::Expr(e) => format!("={e}"),
        }
    }
}

impl From<f64> for Dim {
    fn from(v: f64) -> Dim {
        Dim::Value(v)
    }
}

/// What a sketch or reference plane is attached to.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum PlaneRef {
    Front,
    Top,
    Right,
    /// A planar face of the body, named by the feature that created it.
    Face(FaceId),
    /// Another feature's plane (a reference plane or a sketch).
    Feature(u32),
    /// A fixed placement in model space, used for imported sketches whose original
    /// reference could not be mapped.
    Fixed(Frame),
}

impl PlaneRef {
    pub fn label(&self) -> String {
        match self {
            PlaneRef::Front => "Front Plane".into(),
            PlaneRef::Top => "Top Plane".into(),
            PlaneRef::Right => "Right Plane".into(),
            PlaneRef::Face(f) => format!("Face <{}:{}>", f.feature, f.index),
            PlaneRef::Feature(id) => format!("Feature #{id}"),
            PlaneRef::Fixed(_) => "Fixed placement".into(),
        }
    }

    pub fn standard_frame(&self) -> Option<Frame> {
        match self {
            PlaneRef::Front => Some(Frame::WORLD),
            PlaneRef::Top => {
                Some(Frame { origin: Vec3::ZERO, x: Vec3::X, y: v3(0.0, 0.0, -1.0), z: Vec3::Y })
            }
            PlaneRef::Right => {
                Some(Frame { origin: Vec3::ZERO, x: v3(0.0, 0.0, -1.0), y: Vec3::Y, z: Vec3::X })
            }
            PlaneRef::Fixed(f) => Some(*f),
            _ => None,
        }
    }

    fn references(&self, id: u32) -> bool {
        match *self {
            PlaneRef::Face(f) => f.feature == id,
            PlaneRef::Feature(f) => f == id,
            _ => false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BodyOp {
    Boss,
    Cut,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EndCondition {
    Blind,
    MidPlane,
    ThroughAll,
    ThroughAllBoth,
}

impl EndCondition {
    pub const ALL: [EndCondition; 4] =
        [EndCondition::Blind, EndCondition::MidPlane, EndCondition::ThroughAll, EndCondition::ThroughAllBoth];
    pub fn label(&self) -> &'static str {
        match self {
            EndCondition::Blind => "Blind",
            EndCondition::MidPlane => "Mid Plane",
            EndCondition::ThroughAll => "Through All",
            EndCondition::ThroughAllBoth => "Through All - Both",
        }
    }
    pub fn uses_depth(&self) -> bool {
        matches!(self, EndCondition::Blind | EndCondition::MidPlane)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Axis {
    X,
    Y,
    Z,
    /// Any line in space (direction need not be unit length).
    Line {
        origin: Vec3,
        dir: Vec3,
    },
}

impl Axis {
    pub fn dir(&self) -> Vec3 {
        match self {
            Axis::X => Vec3::X,
            Axis::Y => Vec3::Y,
            Axis::Z => Vec3::Z,
            Axis::Line { dir, .. } => dir.normalized(),
        }
    }

    /// A point on the axis.
    pub fn origin(&self) -> Vec3 {
        match self {
            Axis::Line { origin, .. } => *origin,
            _ => Vec3::ZERO,
        }
    }

    pub fn label(&self) -> String {
        match self {
            Axis::Line { .. } => "Custom axis".into(),
            other => format!("{other:?}"),
        }
    }
}

/// A sketch dimension driven by an expression (SOLIDWORKS "equation" on a dimension).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DrivenDim {
    pub constraint: usize,
    pub expr: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SketchFeature {
    pub plane: PlaneRef,
    pub sketch: Sketch,
    #[serde(default)]
    pub driven: Vec<DrivenDim>,
}

impl SketchFeature {
    pub fn new(plane: PlaneRef) -> SketchFeature {
        SketchFeature { plane, sketch: Sketch::new(), driven: Vec::new() }
    }

    /// Removes a constraint and keeps expression bindings pointing at the right ones.
    pub fn delete_constraint(&mut self, ci: usize) {
        if ci >= self.sketch.constraints.len() {
            return;
        }
        self.sketch.remove_constraint(ci);
        self.driven.retain(|d| d.constraint != ci);
        for d in &mut self.driven {
            if d.constraint > ci {
                d.constraint -= 1;
            }
        }
    }

    pub fn set_driven(&mut self, ci: usize, expr: Option<String>) {
        self.driven.retain(|d| d.constraint != ci);
        if let Some(e) = expr {
            self.driven.push(DrivenDim { constraint: ci, expr: e });
        }
    }

    pub fn driven_expr(&self, ci: usize) -> Option<&str> {
        self.driven.iter().find(|d| d.constraint == ci).map(|d| d.expr.as_str())
    }

    /// Deletes entities and the constraints on them, keeping expression bindings on the
    /// dimensions that remain.
    pub fn delete_entities(&mut self, ids: &[usize]) {
        let map = self.sketch.delete_entities(ids);
        self.driven.retain_mut(|d| match map.get(d.constraint).copied().flatten() {
            Some(ni) => {
                d.constraint = ni;
                true
            }
            None => false,
        });
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExtrudeFeature {
    pub sketch: u32,
    pub end: EndCondition,
    pub depth: Dim,
    #[serde(default)]
    pub reverse: bool,
    pub op: BodyOp,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RevolveFeature {
    pub sketch: u32,
    /// A line entity in the sketch used as the axis of revolution.
    pub axis: usize,
    /// Degrees.
    pub angle: Dim,
    pub op: BodyOp,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlaneFeature {
    pub base: PlaneRef,
    pub offset: Dim,
    #[serde(default)]
    pub flip: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LinearPatternFeature {
    pub features: Vec<u32>,
    pub direction: Axis,
    pub spacing: Dim,
    pub count: Dim,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CircularPatternFeature {
    pub features: Vec<u32>,
    pub axis: Axis,
    /// Degrees. 360 spaces instances evenly around the full circle.
    pub total_angle: Dim,
    pub count: Dim,
}

/// Geometry captured from another CAD system for features that cannot be rebuilt
/// parametrically. It replaces the body at its position in the tree, and later features
/// build on it. Coordinates are millimetres.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ImportedBodyFeature {
    /// Where the geometry came from, e.g. "SOLIDWORKS 2024".
    pub source: String,
    /// Names of the original features this body stands in for.
    pub replaces: Vec<String>,
    pub vertices: Vec<[f64; 3]>,
    pub faces: Vec<ImportedFace>,
    /// The body as an exact solid, when the source geometry allowed it (faces tagged in
    /// the same order as `faces`). Later features then build on the exact kernel.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exact: Option<anvil_brep::Solid>,
    /// False (the default): the body replaces everything built so far, as when it
    /// stands in for a source system's geometry at this point in the tree. True: the
    /// body is added alongside existing bodies, as for each body of an imported file.
    #[serde(default)]
    pub adds: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ImportedFace {
    pub planar: bool,
    pub triangles: Vec<[u32; 3]>,
}

/// Constant-radius fillet on edges, each named by the two faces it separates.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FilletFeature {
    pub edges: Vec<EdgeRef>,
    pub radius: Dim,
}

/// Chamfer on edges.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChamferFeature {
    pub edges: Vec<EdgeRef>,
    pub distance: Dim,
    /// How the bevel is set out (equal distance in parts saved before there was a choice).
    #[serde(default)]
    pub kind: ChamferKind,
}

/// How a chamfer's bevel is set out from its edges.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum ChamferKind {
    /// Set back the distance along both faces.
    #[default]
    EqualDistance,
    /// Set back the distance along one face (each edge's first face, or its second when
    /// flipped), leaving it at the angle (degrees).
    AngleDistance { angle: Dim, flip: bool },
    /// Where the faces, offset by the distance, meet, dropped back square to each face:
    /// the bevel joins the points a fillet of that radius would touch.
    OffsetFace,
}

impl ChamferKind {
    pub const LABELS: [&'static str; 3] = ["Equal distance", "Angle and distance", "Offset face"];

    pub fn label(&self) -> &'static str {
        match self {
            ChamferKind::EqualDistance => Self::LABELS[0],
            ChamferKind::AngleDistance { .. } => Self::LABELS[1],
            ChamferKind::OffsetFace => Self::LABELS[2],
        }
    }
}

/// Hollows the body to a wall thickness, opening the listed faces.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ShellFeature {
    pub remove: Vec<FaceId>,
    pub thickness: Dim,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum FeatureKind {
    Sketch(SketchFeature),
    Extrude(ExtrudeFeature),
    Revolve(RevolveFeature),
    Plane(PlaneFeature),
    LinearPattern(LinearPatternFeature),
    CircularPattern(CircularPatternFeature),
    ImportedBody(ImportedBodyFeature),
    Fillet(FilletFeature),
    Chamfer(ChamferFeature),
    Shell(ShellFeature),
}

impl FeatureKind {
    /// Default name prefix, following SOLIDWORKS conventions.
    pub fn name_prefix(&self) -> &'static str {
        match self {
            FeatureKind::Sketch(_) => "Sketch",
            FeatureKind::Extrude(e) if e.op == BodyOp::Cut => "Cut-Extrude",
            FeatureKind::Extrude(_) => "Boss-Extrude",
            FeatureKind::Revolve(r) if r.op == BodyOp::Cut => "Cut-Revolve",
            FeatureKind::Revolve(_) => "Revolve",
            FeatureKind::Plane(_) => "Plane",
            FeatureKind::LinearPattern(_) => "LPattern",
            FeatureKind::CircularPattern(_) => "CirPattern",
            FeatureKind::ImportedBody(_) => "Imported",
            FeatureKind::Fillet(_) => "Fillet",
            FeatureKind::Chamfer(_) => "Chamfer",
            FeatureKind::Shell(_) => "Shell",
        }
    }

    /// Feature ids this feature depends on directly.
    pub fn dependencies(&self) -> Vec<u32> {
        let plane_dep = |p: &PlaneRef| match *p {
            PlaneRef::Face(f) => vec![f.feature],
            PlaneRef::Feature(f) => vec![f],
            _ => vec![],
        };
        match self {
            FeatureKind::Sketch(s) => plane_dep(&s.plane),
            FeatureKind::Extrude(e) => vec![e.sketch],
            FeatureKind::Revolve(r) => vec![r.sketch],
            FeatureKind::Plane(p) => plane_dep(&p.base),
            FeatureKind::LinearPattern(p) => p.features.clone(),
            FeatureKind::CircularPattern(p) => p.features.clone(),
            FeatureKind::ImportedBody(_) => vec![],
            FeatureKind::Fillet(f) => edge_features(&f.edges),
            FeatureKind::Chamfer(c) => edge_features(&c.edges),
            FeatureKind::Shell(s) => {
                let mut v: Vec<u32> = s.remove.iter().map(|f| f.feature).collect();
                v.sort_unstable();
                v.dedup();
                v
            }
        }
    }

    fn references(&self, id: u32) -> bool {
        match self {
            FeatureKind::Sketch(s) => s.plane.references(id),
            FeatureKind::Plane(p) => p.base.references(id),
            _ => self.dependencies().contains(&id),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Feature {
    pub id: u32,
    pub name: String,
    #[serde(default)]
    pub suppressed: bool,
    pub kind: FeatureKind,
    /// Provenance or migration notes, shown alongside the feature.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PropertyKind {
    Text,
    Number,
    Date,
    YesNo,
}

impl PropertyKind {
    pub const ALL: [PropertyKind; 4] =
        [PropertyKind::Text, PropertyKind::Number, PropertyKind::Date, PropertyKind::YesNo];
    pub fn label(&self) -> &'static str {
        match self {
            PropertyKind::Text => "Text",
            PropertyKind::Number => "Number",
            PropertyKind::Date => "Date",
            PropertyKind::YesNo => "Yes or no",
        }
    }
}

/// File custom property, modelled on the SOLIDWORKS custom property tab.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CustomProperty {
    pub name: String,
    pub kind: PropertyKind,
    pub value: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GlobalVariable {
    pub name: String,
    pub expr: String,
    #[serde(default)]
    pub comment: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Document {
    pub format: String,
    pub format_version: u32,
    #[serde(default)]
    pub material: Option<Material>,
    #[serde(default)]
    pub custom_properties: Vec<CustomProperty>,
    #[serde(default)]
    pub variables: Vec<GlobalVariable>,
    pub features: Vec<Feature>,
    pub next_id: u32,
    /// Features at or after this index are rolled back (not rebuilt).
    #[serde(default)]
    pub rollback: Option<usize>,
}

impl Default for Document {
    fn default() -> Self {
        Document::new()
    }
}

#[derive(Debug)]
pub enum FileError {
    Io(std::io::Error),
    Json(serde_json::Error),
    WrongFormat(String),
    TooNew(u32),
}

impl std::fmt::Display for FileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FileError::Io(e) => write!(f, "{e}"),
            FileError::Json(e) => write!(f, "file is damaged: {e}"),
            FileError::WrongFormat(s) => write!(f, "not an AnvilCAD part file (format '{s}')"),
            FileError::TooNew(v) => {
                write!(f, "file format version {v} is newer than this build supports ({PART_FORMAT_VERSION})")
            }
        }
    }
}

impl std::error::Error for FileError {}

impl From<std::io::Error> for FileError {
    fn from(e: std::io::Error) -> Self {
        FileError::Io(e)
    }
}
impl From<serde_json::Error> for FileError {
    fn from(e: serde_json::Error) -> Self {
        FileError::Json(e)
    }
}

impl Document {
    /// A new part with the default template's custom properties.
    pub fn new() -> Document {
        let prop =
            |n: &str| CustomProperty { name: n.into(), kind: PropertyKind::Text, value: String::new() };
        Document {
            format: PART_FORMAT.into(),
            format_version: PART_FORMAT_VERSION,
            material: None,
            custom_properties: vec![
                prop("Description"),
                prop("PartNo"),
                prop("Revision"),
                prop("DrawnBy"),
                prop("Finish"),
            ],
            variables: Vec::new(),
            features: Vec::new(),
            next_id: 1,
            rollback: None,
        }
    }

    /// Starts a new document from a template document: everything is copied except that
    /// it becomes a fresh, unsaved part.
    pub fn from_template(template: &Document) -> Document {
        let mut d = template.clone();
        d.rollback = None;
        d
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("document serializes")
    }

    pub fn from_json(s: &str) -> Result<Document, FileError> {
        let v: serde_json::Value = serde_json::from_str(s)?;
        let format = v.get("format").and_then(|f| f.as_str()).unwrap_or("");
        if format != PART_FORMAT && !LEGACY_PART_FORMATS.contains(&format) {
            return Err(FileError::WrongFormat(format.into()));
        }
        let ver = v.get("format_version").and_then(|f| f.as_u64()).unwrap_or(0) as u32;
        if ver > PART_FORMAT_VERSION {
            return Err(FileError::TooNew(ver));
        }
        Ok(serde_json::from_value(v)?)
    }

    pub fn save(&self, path: &std::path::Path) -> Result<(), FileError> {
        // Write to a temporary file first so a crash never leaves a half-written part.
        let tmp = path.with_extension("anvprt.tmp");
        std::fs::write(&tmp, self.to_json())?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    pub fn load(path: &std::path::Path) -> Result<Document, FileError> {
        Document::from_json(&std::fs::read_to_string(path)?)
    }

    pub fn index_of(&self, id: u32) -> Option<usize> {
        self.features.iter().position(|f| f.id == id)
    }

    pub fn feature(&self, id: u32) -> Option<&Feature> {
        self.features.iter().find(|f| f.id == id)
    }

    pub fn feature_mut(&mut self, id: u32) -> Option<&mut Feature> {
        self.features.iter_mut().find(|f| f.id == id)
    }

    pub fn sketch(&self, id: u32) -> Option<&SketchFeature> {
        match &self.feature(id)?.kind {
            FeatureKind::Sketch(s) => Some(s),
            _ => None,
        }
    }

    pub fn sketch_mut(&mut self, id: u32) -> Option<&mut SketchFeature> {
        match &mut self.feature_mut(id)?.kind {
            FeatureKind::Sketch(s) => Some(s),
            _ => None,
        }
    }

    fn unique_name(&self, prefix: &str) -> String {
        (1..).map(|i| format!("{prefix}{i}")).find(|n| !self.features.iter().any(|f| &f.name == n)).unwrap()
    }

    /// Appends a feature (or inserts it at the rollback bar) and returns its id.
    pub fn add_feature(&mut self, kind: FeatureKind) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        let name = self.unique_name(kind.name_prefix());
        let f = Feature { id, name, suppressed: false, kind, note: None };
        match self.rollback {
            Some(i) if i <= self.features.len() => {
                self.features.insert(i, f);
                self.rollback = Some(i + 1);
            }
            _ => self.features.push(f),
        }
        id
    }

    /// All features that depend on `id`, directly or indirectly.
    pub fn dependents(&self, id: u32) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        let mut frontier = vec![id];
        while let Some(cur) = frontier.pop() {
            for f in &self.features {
                if f.kind.references(cur) && !out.contains(&f.id) && f.id != id {
                    out.push(f.id);
                    frontier.push(f.id);
                }
            }
        }
        out
    }

    /// Deletes a feature and everything that depends on it. Returns the deleted ids.
    pub fn delete_feature(&mut self, id: u32) -> Vec<u32> {
        let mut gone = self.dependents(id);
        gone.push(id);
        if let Some(rb) = self.rollback {
            let removed_before = self.features.iter().take(rb).filter(|f| gone.contains(&f.id)).count();
            self.rollback = Some(rb - removed_before);
        }
        self.features.retain(|f| !gone.contains(&f.id));
        gone
    }

    pub fn evaluate_variables(&self) -> (BTreeMap<String, f64>, BTreeMap<String, EvalError>) {
        let defs: Vec<(String, String)> =
            self.variables.iter().map(|v| (v.name.clone(), v.expr.clone())).collect();
        eval_all(&defs)
    }
}

fn edge_features(edges: &[EdgeRef]) -> Vec<u32> {
    let mut v: Vec<u32> = edges.iter().flat_map(|e| [e.a.feature, e.b.feature]).collect();
    v.sort_unstable();
    v.dedup();
    v
}
