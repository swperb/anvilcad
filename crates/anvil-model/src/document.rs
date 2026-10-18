//! The part document: feature tree, global variables, material and custom properties.

use crate::config::{Configuration, ConfiguredParam, DEFAULT_CONFIGURATION};
use crate::equations::{eval, eval_all, EvalError};
use crate::material::Material;
use anvil_kernel::{v3, EdgeRef, FaceId, Frame, Vec3};
use anvil_sketch::Sketch;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const PART_FORMAT: &str = "anvil-part";
/// Format ids written by earlier builds that are still read.
pub const LEGACY_PART_FORMATS: &[&str] = &["ferrocad-part"];
/// The newest part format version this build reads and writes.
pub const PART_FORMAT_VERSION: u32 = 2;
/// The version written for parts that use nothing newer: one configuration and nothing
/// configured. Builds from before configurations read it.
pub const PART_FORMAT_VERSION_BASE: u32 = 1;
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
        self.dependency() == Some(id)
    }

    /// The feature this plane comes from, if any.
    pub fn dependency(&self) -> Option<u32> {
        match *self {
            PlaneRef::Face(f) => Some(f.feature),
            PlaneRef::Feature(f) => Some(f),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum BodyOp {
    #[default]
    Boss,
    Cut,
}

/// Where an extrusion ends, for one direction. See `docs/re/extrude-revolve.md`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum EndCondition {
    #[default]
    Blind,
    MidPlane,
    ThroughAll,
    ThroughAllBoth,
    /// To the next face of the part that covers the whole profile.
    UpToNext,
    /// To the plane parallel to the sketch through a point (the feature's `target`).
    UpToVertex,
    /// To a plane or planar face (the feature's `target`), which may lean.
    UpToSurface,
    /// To a set distance (the depth) short of, or past, a plane or planar face.
    OffsetFromSurface,
}

impl EndCondition {
    pub const ALL: [EndCondition; 8] = [
        EndCondition::Blind,
        EndCondition::ThroughAll,
        EndCondition::ThroughAllBoth,
        EndCondition::UpToNext,
        EndCondition::UpToVertex,
        EndCondition::UpToSurface,
        EndCondition::OffsetFromSurface,
        EndCondition::MidPlane,
    ];
    /// The conditions Direction 2 offers (the two-sided ones are not).
    pub const SECOND: [EndCondition; 6] = [
        EndCondition::Blind,
        EndCondition::ThroughAll,
        EndCondition::UpToNext,
        EndCondition::UpToVertex,
        EndCondition::UpToSurface,
        EndCondition::OffsetFromSurface,
    ];
    pub fn label(&self) -> &'static str {
        match self {
            EndCondition::Blind => "Blind",
            EndCondition::MidPlane => "Mid Plane",
            EndCondition::ThroughAll => "Through All",
            EndCondition::ThroughAllBoth => "Through All - Both",
            EndCondition::UpToNext => "Up To Next",
            EndCondition::UpToVertex => "Up To Vertex",
            EndCondition::UpToSurface => "Up To Surface",
            EndCondition::OffsetFromSurface => "Offset From Surface",
        }
    }
    /// True when the condition reads the depth field (a blind depth or an offset).
    pub fn uses_depth(&self) -> bool {
        matches!(self, EndCondition::Blind | EndCondition::MidPlane | EndCondition::OffsetFromSurface)
    }
    /// True when the condition needs a target (a point or a plane).
    pub fn needs_target(&self) -> bool {
        matches!(self, EndCondition::UpToVertex | EndCondition::UpToSurface | EndCondition::OffsetFromSurface)
    }
    /// True when the condition already reaches both ways from the sketch.
    pub fn two_sided(&self) -> bool {
        matches!(self, EndCondition::MidPlane | EndCondition::ThroughAllBoth)
    }
}

/// A point a feature refers to.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum PointRef {
    /// A point of a sketch, by feature id and point index.
    SketchPoint { sketch: u32, point: usize },
    /// A fixed position in model space, such as a model vertex picked in the graphics
    /// area. It does not follow later edits of the geometry it was picked on.
    Model(Vec3),
    /// A reference point feature.
    Feature(u32),
}

impl PointRef {
    /// The feature this point comes from, if any.
    pub fn dependency(&self) -> Option<u32> {
        match *self {
            PointRef::SketchPoint { sketch, .. } => Some(sketch),
            PointRef::Feature(id) => Some(id),
            PointRef::Model(_) => None,
        }
    }
}

/// What an Up To Vertex, Up To Surface or Offset From Surface end refers to.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum EndTarget {
    Point(PointRef),
    /// A plane or planar face. `reverse_offset` puts an offset past the surface (away
    /// from the sketch) instead of short of it; `translate` moves the surface along the
    /// extrusion by the offset instead of offsetting it along its own normal.
    Plane {
        plane: PlaneRef,
        #[serde(default)]
        reverse_offset: bool,
        #[serde(default)]
        translate: bool,
    },
}

impl EndTarget {
    pub fn label(&self) -> String {
        match self {
            EndTarget::Point(PointRef::SketchPoint { sketch, point }) => {
                format!("Point {} of feature #{sketch}", point + 1)
            }
            EndTarget::Point(PointRef::Model(p)) => format!("Point ({:.3}, {:.3}, {:.3})", p.x, p.y, p.z),
            EndTarget::Point(PointRef::Feature(id)) => format!("Point #{id}"),
            EndTarget::Plane { plane, .. } => plane.label(),
        }
    }

    fn dependencies(&self) -> Vec<u32> {
        match *self {
            EndTarget::Point(p) => p.dependency().into_iter().collect(),
            EndTarget::Plane { plane, .. } => plane.dependency().into_iter().collect(),
        }
    }
}

/// Draft while extruding: the side faces lean by `angle` (degrees) from the direction
/// of extrusion, closing the section in with distance from the sketch, or opening it
/// out when `outward` is set.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Draft {
    pub angle: Dim,
    #[serde(default)]
    pub outward: bool,
}

/// The second direction of a two-ended extrusion: it runs the opposite way from the
/// sketch with its own end, depth and draft.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EndSpec {
    pub end: EndCondition,
    pub depth: Dim,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<EndTarget>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub draft: Option<Draft>,
}

/// How a thin feature's wall sits on the sketch curves.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThinKind {
    /// All the thickness on one side: outside a closed contour, to the left of an open
    /// one (walking it from its first point), or the other side when reversed.
    #[default]
    OneDirection,
    /// Half the thickness on each side.
    MidPlane,
    /// `thickness` on the first side and `thickness2` on the other.
    TwoDirection,
}

impl ThinKind {
    pub const ALL: [ThinKind; 3] = [ThinKind::OneDirection, ThinKind::MidPlane, ThinKind::TwoDirection];
    pub fn label(&self) -> &'static str {
        match self {
            ThinKind::OneDirection => "One-Direction",
            ThinKind::MidPlane => "Mid-Plane",
            ThinKind::TwoDirection => "Two-Direction",
        }
    }
}

/// A thin feature: the sketch curves become a wall of a set thickness instead of the
/// regions they bound.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ThinWall {
    pub kind: ThinKind,
    pub thickness: Dim,
    #[serde(default = "default_thickness2")]
    pub thickness2: Dim,
    #[serde(default)]
    pub reverse: bool,
    /// Rounds the corners of open contours with this inside radius.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fillet: Option<Dim>,
    /// Closes both ends of a closed contour's wall with caps this thick, inside its length.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cap: Option<Dim>,
}

fn default_thickness2() -> Dim {
    Dim::Value(1.0)
}

impl ThinWall {
    pub fn new(thickness: f64) -> ThinWall {
        ThinWall {
            kind: ThinKind::OneDirection,
            thickness: Dim::Value(thickness),
            thickness2: Dim::Value(thickness),
            reverse: false,
            fillet: None,
            cap: None,
        }
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
    /// A reference axis feature.
    Feature(u32),
}

impl Axis {
    pub fn dir(&self) -> Vec3 {
        match self {
            Axis::X => Vec3::X,
            Axis::Y => Vec3::Y,
            Axis::Z => Vec3::Z,
            Axis::Line { dir, .. } => dir.normalized(),
            Axis::Feature(_) => Vec3::Z,
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
            Axis::Feature(id) => format!("Axis #{id}"),
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

/// An extruded boss or cut. Direction 1 is `end`, `depth`, `target` and `draft`; see
/// `docs/re/extrude-revolve.md` for every option.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExtrudeFeature {
    pub sketch: u32,
    pub end: EndCondition,
    /// Blind depth, total depth for Mid Plane, or the offset for Offset From Surface.
    pub depth: Dim,
    #[serde(default)]
    pub reverse: bool,
    pub op: BodyOp,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<EndTarget>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub draft: Option<Draft>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direction2: Option<EndSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thin: Option<ThinWall>,
    /// Start on a plane this far from the sketch plane, along the sketch normal
    /// (against it when `start_flip` is set), instead of on the sketch plane.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_offset: Option<Dim>,
    #[serde(default)]
    pub start_flip: bool,
    /// Cuts only: remove the material outside the profile instead of inside.
    #[serde(default)]
    pub flip_side: bool,
}

impl Default for ExtrudeFeature {
    fn default() -> Self {
        ExtrudeFeature {
            sketch: 0,
            end: EndCondition::Blind,
            depth: Dim::Value(10.0),
            reverse: false,
            op: BodyOp::Boss,
            target: None,
            draft: None,
            direction2: None,
            thin: None,
            start_offset: None,
            start_flip: false,
            flip_side: false,
        }
    }
}

impl ExtrudeFeature {
    /// Every feature this extrusion refers to.
    fn dependencies(&self) -> Vec<u32> {
        let mut v = vec![self.sketch];
        for t in self.target.iter().chain(self.direction2.iter().filter_map(|d| d.target.as_ref())) {
            v.extend(t.dependencies());
        }
        v.sort_unstable();
        v.dedup();
        v
    }
}

/// How far a revolve turns. See `docs/re/extrude-revolve.md`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum RevolveKind {
    /// The angle one way from the sketch.
    #[default]
    OneDirection,
    /// Half the angle each way from the sketch.
    MidPlane,
    /// The angle one way and `angle2` (degrees) the other.
    TwoDirection { angle2: Dim },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RevolveFeature {
    pub sketch: u32,
    /// A line entity in the sketch used as the axis of revolution.
    pub axis: usize,
    /// Degrees.
    pub angle: Dim,
    pub op: BodyOp,
    #[serde(default)]
    pub kind: RevolveKind,
    #[serde(default)]
    pub reverse: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thin: Option<ThinWall>,
    /// Cuts only: remove the material outside the profile within the swept angle.
    #[serde(default)]
    pub flip_side: bool,
}

impl Default for RevolveFeature {
    fn default() -> Self {
        RevolveFeature {
            sketch: 0,
            axis: 0,
            angle: Dim::Value(360.0),
            op: BodyOp::Boss,
            kind: RevolveKind::OneDirection,
            reverse: false,
            thin: None,
            flip_side: false,
        }
    }
}

/// A reference plane. See `docs/re/reference-geometry.md`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlaneFeature {
    /// The plane it is parallel to, turned from or halfway from.
    pub base: PlaneRef,
    /// The distance of an offset plane.
    pub offset: Dim,
    /// Offset planes: put the plane on the other side, turned over (as it always has
    /// been). At angle planes: turn the other way.
    #[serde(default)]
    pub flip: bool,
    #[serde(default)]
    pub kind: crate::refgeom::PlaneKind,
    /// Turns the plane over (its normal and x axis reverse) without moving it.
    #[serde(default)]
    pub flip_normal: bool,
}

impl Default for PlaneFeature {
    fn default() -> Self {
        PlaneFeature {
            base: PlaneRef::Front,
            offset: Dim::Value(10.0),
            flip: false,
            kind: crate::refgeom::PlaneKind::Offset,
            flip_normal: false,
        }
    }
}

/// A linear pattern: copies of its seed features along one or two directions. See
/// `docs/re/mirror-and-patterns.md`. Counts include the seed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LinearPatternFeature {
    pub features: Vec<u32>,
    pub direction: Axis,
    pub spacing: Dim,
    pub count: Dim,
    #[serde(default)]
    pub reverse: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direction2: Option<PatternDirection>,
    /// Copy only the seed along Direction 2, not the whole Direction 1 row.
    #[serde(default)]
    pub seed_only: bool,
    /// Instances left out, as 1-based (Direction 1, Direction 2) places; the seed is (1, 1).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skip: Vec<[u32; 2]>,
}

impl Default for LinearPatternFeature {
    fn default() -> Self {
        LinearPatternFeature {
            features: Vec::new(),
            direction: Axis::X,
            spacing: Dim::Value(20.0),
            count: Dim::Value(3.0),
            reverse: false,
            direction2: None,
            seed_only: false,
            skip: Vec::new(),
        }
    }
}

/// The second direction of a linear pattern.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PatternDirection {
    pub direction: Axis,
    pub spacing: Dim,
    pub count: Dim,
    #[serde(default)]
    pub reverse: bool,
}

fn yes() -> bool {
    true
}

/// A circular pattern: copies of its seed features turned about an axis. With equal
/// spacing the angle is the total (360 spreads the instances evenly round the full
/// circle); otherwise it is the angle between neighbouring instances. Counts include
/// the seed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CircularPatternFeature {
    pub features: Vec<u32>,
    pub axis: Axis,
    /// Degrees: the total with equal spacing, the step otherwise.
    pub total_angle: Dim,
    pub count: Dim,
    #[serde(default = "yes")]
    pub equal_spacing: bool,
    #[serde(default)]
    pub reverse: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direction2: Option<CircularDirection2>,
    /// Instances left out, as (direction, k): k counts from the seed (1) in turning order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skip: Vec<[u32; 2]>,
}

impl Default for CircularPatternFeature {
    fn default() -> Self {
        CircularPatternFeature {
            features: Vec::new(),
            axis: Axis::Y,
            total_angle: Dim::Value(360.0),
            count: Dim::Value(4.0),
            equal_spacing: true,
            reverse: false,
            direction2: None,
            skip: Vec::new(),
        }
    }
}

/// The other way round the axis. Symmetric copies Direction 1's angle and count.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CircularDirection2 {
    #[serde(default)]
    pub symmetric: bool,
    pub angle: Dim,
    pub count: Dim,
    #[serde(default = "yes")]
    pub equal_spacing: bool,
}

/// Reflects seed features (or the whole body) in a plane or planar face, and
/// optionally in a second one. See `docs/re/mirror-and-patterns.md`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MirrorFeature {
    pub plane: PlaneRef,
    #[serde(default)]
    pub features: Vec<u32>,
    /// Mirror the whole body built so far (and join the copy to it) instead of features.
    #[serde(default)]
    pub bodies: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plane2: Option<PlaneRef>,
    /// With two planes, mirror only the seed in the second plane, not the first copy.
    #[serde(default)]
    pub seed_only: bool,
}

/// Copies of seed features at the free points of a sketch: each copy is the seed
/// moved so its reference point lands on a sketch point.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SketchPatternFeature {
    pub features: Vec<u32>,
    pub sketch: u32,
    /// Where on the seed each copy is placed from; the seed's centroid when `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference: Option<PointRef>,
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
    /// The round's cross-section (circular in parts saved before there was a choice).
    #[serde(default)]
    pub profile: FilletProfile,
    /// Also round every edge running smoothly on from the selected ones (off in parts
    /// saved before there was a choice, so they rebuild as they did).
    #[serde(default)]
    pub tangent_propagation: bool,
}

/// The cross-section of a fillet's round, between where a ball of its radius touches the
/// faces (see `docs/re/fillet-curved-edges.md`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum FilletProfile {
    /// The ball's circular arc.
    #[default]
    Circular,
    /// A conic weighted by rho, from 0.5 (a parabola) to 0.95.
    ConicRho { rho: Dim },
    /// A conic with this radius of curvature at its shoulder point.
    ConicRadius { radius: Dim },
    /// A curve that meets each face with the face's own curvature.
    CurvatureContinuous,
}

impl FilletProfile {
    pub const LABELS: [&'static str; 4] = ["Circular", "Conic rho", "Conic radius", "Curvature continuous"];

    pub fn label(&self) -> &'static str {
        match self {
            FilletProfile::Circular => Self::LABELS[0],
            FilletProfile::ConicRho { .. } => Self::LABELS[1],
            FilletProfile::ConicRadius { .. } => Self::LABELS[2],
            FilletProfile::CurvatureContinuous => Self::LABELS[3],
        }
    }
}

/// Chamfer on edges.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChamferFeature {
    pub edges: Vec<EdgeRef>,
    pub distance: Dim,
    /// How the bevel is set out (equal distance in parts saved before there was a choice).
    #[serde(default)]
    pub kind: ChamferKind,
    /// Also bevel every edge running smoothly on from the selected ones (off in parts
    /// saved before there was a choice).
    #[serde(default)]
    pub tangent_propagation: bool,
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
    Hole(crate::hole::HoleFeature),
    Mirror(MirrorFeature),
    SketchPattern(SketchPatternFeature),
    Axis(crate::refgeom::AxisFeature),
    Point(crate::refgeom::PointFeature),
    CoordSys(crate::refgeom::CoordSysFeature),
    Sweep(crate::sweep::SweepFeature),
    Draft(crate::modify::DraftFeature),
    Dome(crate::modify::DomeFeature),
}

impl FeatureKind {
    /// Default name prefix, following SOLIDWORKS conventions.
    pub fn name_prefix(&self) -> &'static str {
        match self {
            FeatureKind::Sketch(_) => "Sketch",
            FeatureKind::Extrude(e) if e.op == BodyOp::Cut && e.thin.is_some() => "Cut-Extrude-Thin",
            FeatureKind::Extrude(e) if e.op == BodyOp::Cut => "Cut-Extrude",
            FeatureKind::Extrude(e) if e.thin.is_some() => "Extrude-Thin",
            FeatureKind::Extrude(_) => "Boss-Extrude",
            FeatureKind::Revolve(r) if r.op == BodyOp::Cut => "Cut-Revolve",
            FeatureKind::Revolve(r) if r.thin.is_some() => "Revolve-Thin",
            FeatureKind::Revolve(_) => "Revolve",
            FeatureKind::Plane(_) => "Plane",
            FeatureKind::LinearPattern(_) => "LPattern",
            FeatureKind::CircularPattern(_) => "CirPattern",
            FeatureKind::ImportedBody(_) => "Imported",
            FeatureKind::Fillet(_) => "Fillet",
            FeatureKind::Chamfer(_) => "Chamfer",
            FeatureKind::Shell(_) => "Shell",
            FeatureKind::Hole(_) => "Hole",
            FeatureKind::Mirror(_) => "Mirror",
            FeatureKind::Sweep(s) if s.op == BodyOp::Cut => "Cut-Sweep",
            FeatureKind::Sweep(_) => "Sweep",
            FeatureKind::Draft(_) => "Draft",
            FeatureKind::Dome(_) => "Dome",
            FeatureKind::Axis(_) => "Axis",
            FeatureKind::Point(_) => "Point",
            FeatureKind::CoordSys(_) => "Coordinate System",
            FeatureKind::SketchPattern(_) => "Sketch-Pattern",
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
            FeatureKind::Extrude(e) => e.dependencies(),
            FeatureKind::Revolve(r) => vec![r.sketch],
            FeatureKind::Hole(h) => vec![h.sketch],
            FeatureKind::Plane(p) => {
                let mut v = plane_dep(&p.base);
                v.extend(p.kind.dependencies());
                v.sort_unstable();
                v.dedup();
                v
            }
            FeatureKind::Axis(a) => a.kind.dependencies(),
            FeatureKind::Sweep(s) => s.dependencies(),
            FeatureKind::Draft(d) => d.dependencies(),
            FeatureKind::Dome(d) => d.dependencies(),
            FeatureKind::Point(p) => p.dependencies(),
            FeatureKind::CoordSys(c) => c.dependencies(),
            FeatureKind::LinearPattern(p) => p.features.clone(),
            FeatureKind::CircularPattern(p) => p.features.clone(),
            FeatureKind::Mirror(m) => {
                let mut v = m.features.clone();
                v.extend(m.plane.dependency());
                v.extend(m.plane2.and_then(|p| p.dependency()));
                v.sort_unstable();
                v.dedup();
                v
            }
            FeatureKind::SketchPattern(p) => {
                let mut v = p.features.clone();
                v.push(p.sketch);
                v.extend(p.reference.and_then(|r| r.dependency()));
                v.sort_unstable();
                v.dedup();
                v
            }
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
    /// The part's configurations (see `config.rs`). Never empty once normalized.
    #[serde(default)]
    pub configurations: Vec<Configuration>,
    /// The configuration whose values the feature tree holds.
    #[serde(default)]
    pub active_configuration: u32,
    /// Parameters with a value per configuration.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub configured: Vec<ConfiguredParam>,
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
            format_version: PART_FORMAT_VERSION_BASE,
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
            configurations: vec![Configuration::new(1, DEFAULT_CONFIGURATION)],
            active_configuration: 1,
            configured: Vec::new(),
        }
    }

    /// Starts a new document from a template document: everything is copied except that
    /// it becomes a fresh, unsaved part.
    pub fn from_template(template: &Document) -> Document {
        let mut d = template.clone();
        d.rollback = None;
        d.normalize_configurations();
        d
    }

    /// The part as saved: with the tree's values recorded into the active configuration,
    /// and the oldest format version that holds everything it uses.
    pub fn to_json(&self) -> String {
        let mut d = self.clone();
        d.capture_configuration();
        d.format_version =
            if d.uses_configurations() { PART_FORMAT_VERSION } else { PART_FORMAT_VERSION_BASE };
        serde_json::to_string_pretty(&d).expect("document serializes")
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
        let mut d: Document = serde_json::from_value(v)?;
        // The version is worked out again on saving, from what the part uses.
        d.format_version = PART_FORMAT_VERSION_BASE;
        d.normalize_configurations();
        Ok(d)
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
        self.feature_added(id);
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
        self.features_deleted(&gone);
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
