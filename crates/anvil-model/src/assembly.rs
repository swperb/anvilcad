//! Assemblies: parts placed relative to each other and held by mates.
//!
//! This follows the specification in `docs/re/assemblies-and-mates.md` ("Assembly
//! structure" and "AnvilCAD decisions"). An [`Assembly`] lists components, each an
//! instance of a part file in one of its configurations with a rigid placement, and the
//! mates between them. The first component inserted is fixed. Mates refer to entities by
//! the same stable names parts use elsewhere (faces and edges by the features that made
//! them, reference geometry by feature id), so they survive the part being rebuilt.
//!
//! Part files are referenced by a path relative to the assembly file and loaded through
//! [`Parts`], which rebuilds each part (and configuration) once.

use crate::document::{CustomProperty, Document, FileError, PlaneRef, PropertyKind};
use crate::mates::{self, Alignment, Definition, MateKind, MateState, Relation, Side, SolveReport};
use crate::measure::{self, Pick, Shape};
use crate::rebuild::BuildState;
use anvil_kernel::{EdgeRef, FaceId, Frame, Solid, Vec3};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub const ASSEMBLY_FORMAT: &str = "anvil-assembly";
pub const ASSEMBLY_FORMAT_VERSION: u32 = 1;
pub const ASSEMBLY_EXT: &str = "anvasm";

/// One placed instance of a part.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Component {
    pub id: u32,
    /// The instance name: the part's file name and an instance number, as `bracket<2>`.
    pub name: String,
    /// The part file, relative to the assembly file, with `/` separators.
    pub path: String,
    /// The part configuration to use; the part's active one when `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub configuration: Option<String>,
    /// Where the part's own coordinates sit in the assembly.
    pub placement: Frame,
    /// Fixed components never move when mates are solved.
    #[serde(default)]
    pub fixed: bool,
    #[serde(default)]
    pub hidden: bool,
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
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct MateRef {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub component: Option<u32>,
    pub entity: MateEntity,
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

/// Loads and rebuilds the parts an assembly uses, each file and configuration once.
#[derive(Default)]
pub struct Parts {
    /// The directory component paths are relative to.
    pub base: PathBuf,
    parts: HashMap<(String, Option<String>), Result<LoadedPart, String>>,
}

impl Parts {
    pub fn new(base: impl Into<PathBuf>) -> Parts {
        Parts { base: base.into(), parts: HashMap::new() }
    }

    /// For an assembly saved at `file`.
    pub fn for_file(file: &Path) -> Parts {
        Parts::new(file.parent().unwrap_or(Path::new(".")))
    }

    /// The full path of a component's part file.
    pub fn resolve(&self, path: &str) -> PathBuf {
        self.base.join(path)
    }

    /// Adds an already-loaded part under `path`, for parts that are not on disk yet.
    pub fn insert(&mut self, path: &str, configuration: Option<&str>, mut doc: Document) {
        let state = doc.rebuild();
        self.parts
            .insert((path.to_string(), configuration.map(str::to_string)), Ok(LoadedPart { doc, state }));
    }

    /// Forgets a part so that it is read again from disk.
    pub fn reload(&mut self, path: &str) {
        self.parts.retain(|(p, _), _| p != path);
    }

    pub fn get(&mut self, path: &str, configuration: Option<&str>) -> Result<&LoadedPart, String> {
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

    fn load(&self, path: &str, configuration: Option<&str>) -> Result<LoadedPart, String> {
        let file = self.resolve(path);
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
}

/// One line of a bill of materials.
#[derive(Clone, Debug, PartialEq)]
pub struct BomRow {
    pub item: usize,
    /// The part's `PartNo` property, or its file name when that is empty.
    pub part_number: String,
    pub description: String,
    pub path: String,
    pub configuration: Option<String>,
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

/// The shape of a mate entity on a rebuilt part, in the part's coordinates.
pub fn entity_shape(st: Option<&BuildState>, e: &MateEntity) -> Result<Shape, String> {
    let plane = |f: Frame| Shape::Plane { origin: f.origin, normal: f.z };
    match (*e, st) {
        (MateEntity::Origin, _) => Ok(Shape::Point(Vec3::ZERO)),
        (MateEntity::Vertex(p), _) => Ok(Shape::Point(p)),
        (MateEntity::Plane(p), _) if p.standard_frame().is_some() => {
            Ok(plane(p.standard_frame().unwrap_or(Frame::WORLD)))
        }
        (_, None) => Err("the assembly has only its origin and standard planes to mate to".into()),
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

/// `file`'s name without its extension, or the whole path.
fn stem(path: &str) -> &str {
    let file = path.rsplit(['/', '\\']).next().unwrap_or(path);
    file.rsplit_once('.').map(|(s, _)| s).filter(|s| !s.is_empty()).unwrap_or(file)
}

impl Assembly {
    pub fn new() -> Assembly {
        let prop =
            |n: &str| CustomProperty { name: n.into(), kind: PropertyKind::Text, value: String::new() };
        Assembly {
            format: ASSEMBLY_FORMAT.into(),
            format_version: ASSEMBLY_FORMAT_VERSION,
            custom_properties: vec![prop("Description"), prop("PartNo"), prop("Revision"), prop("DrawnBy")],
            components: Vec::new(),
            mates: Vec::new(),
            next_id: 1,
        }
    }

    pub fn to_json(&self) -> Result<String, FileError> {
        Ok(serde_json::to_string_pretty(self)?)
    }

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

    /// Places an instance of the part at `path` (relative to the assembly file). The first
    /// component is fixed at the assembly origin, whatever `placement` says.
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
        });
        id
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

    /// The shape of one side of a mate, in that component's coordinates.
    pub fn ref_shape(&self, parts: &mut Parts, r: &MateRef) -> Result<Shape, String> {
        match r.component {
            None => entity_shape(None, &r.entity),
            Some(cid) => {
                let c = self.component(cid).ok_or_else(|| format!("component #{cid} is missing"))?;
                let part =
                    parts.get(&c.path, c.configuration.as_deref()).map_err(|e| format!("{}: {e}", c.name))?;
                entity_shape(Some(&part.state), &r.entity).map_err(|e| format!("{}: {e}", c.name))
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

    /// Solves the mates and moves the components that are not fixed.
    pub fn solve(&mut self, parts: &mut Parts) -> AssemblyStatus {
        let index: HashMap<u32, usize> = self.components.iter().enumerate().map(|(i, c)| (c.id, i)).collect();
        let mut relations = Vec::new();
        let mut owners = Vec::new();
        let mut states: Vec<(u32, MateState)> = Vec::new();
        for m in &self.mates {
            if m.suppressed {
                states.push((m.id, MateState::Satisfied));
                continue;
            }
            let side = |r: &MateRef, parts: &mut Parts| -> Result<Side, String> {
                let component = match r.component {
                    Some(c) => Some(*index.get(&c).ok_or_else(|| format!("component #{c} is missing"))?),
                    None => None,
                };
                Ok(Side { component, shape: self.ref_shape(parts, r)? })
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
        let mut placements: Vec<Frame> = self.components.iter().map(|c| c.placement).collect();
        let fixed: Vec<bool> = self.components.iter().map(|c| c.fixed).collect();
        let SolveReport { mates: solved, components } = mates::solve(&mut placements, &fixed, &relations);
        for (k, st) in solved.into_iter().enumerate() {
            if let Some(&o) = owners.get(k) {
                states[o].1 = st;
            }
        }
        for (c, p) in self.components.iter_mut().zip(&placements) {
            c.placement = *p;
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

    /// The bodies of the visible components, placed in the assembly.
    pub fn bodies(&self, parts: &mut Parts) -> Vec<(u32, Result<Solid, String>)> {
        self.components
            .iter()
            .filter(|c| !c.hidden)
            .map(|c| {
                let body = parts
                    .get(&c.path, c.configuration.as_deref())
                    .map(|p| p.state.body.transformed(&c.placement));
                (c.id, body)
            })
            .collect()
    }

    /// The parts list: one row per part file and configuration, in the order they were
    /// first inserted, with how many instances there are.
    pub fn bill_of_materials(&self, parts: &mut Parts) -> Vec<BomRow> {
        let mut rows: Vec<BomRow> = Vec::new();
        for c in &self.components {
            if let Some(row) =
                rows.iter_mut().find(|r| r.path == c.path && r.configuration == c.configuration)
            {
                row.quantity += 1;
                continue;
            }
            let prop = |name: &str, parts: &mut Parts| -> String {
                parts
                    .get(&c.path, c.configuration.as_deref())
                    .ok()
                    .and_then(|p| {
                        p.doc.custom_properties.iter().find(|q| q.name == name).map(|q| q.value.clone())
                    })
                    .unwrap_or_default()
            };
            let part_number = Some(prop("PartNo", parts)).filter(|s| !s.trim().is_empty());
            rows.push(BomRow {
                item: rows.len() + 1,
                part_number: part_number.unwrap_or_else(|| stem(&c.path).to_string()),
                description: prop("Description", parts),
                path: c.path.clone(),
                configuration: c.configuration.clone(),
                quantity: 1,
            });
        }
        rows
    }
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
        let on = |c: u32, f: FaceId| MateRef { component: Some(c), entity: MateEntity::Face(f) };
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
            MateRef { component: Some(base), entity: MateEntity::Face(bogus) },
            MateRef { component: Some(cube), entity: MateEntity::Origin },
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
            MateRef { component: None, entity: right },
            MateRef { component: Some(base), entity: right },
        );
        let lock = asm.lock(Some(base), cube).expect("lock");
        asm.add_mate(
            lock,
            Alignment::Closest,
            MateRef { component: Some(base), entity: MateEntity::Origin },
            MateRef { component: Some(cube), entity: MateEntity::Origin },
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
            MateRef { component: Some(base), entity: MateEntity::Plane(PlaneRef::Top) },
            MateRef { component: Some(cube), entity: MateEntity::Vertex(v3(1.0, 2.0, 3.0)) },
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
}
