//! AnvilCAD drawings: a sheet showing standard views of a part, projected from the part's
//! exact bodies with hidden lines removed or dashed, with model and reference dimensions
//! and a title block filled from the part's properties, exported as vector PDF or DXF.
//!
//! The behaviour follows the specification in `docs/re/drawings.md`. The pieces:
//!
//! - [`sheet`]: paper sizes, scales and the projection method;
//! - [`document`]: the `.anvdrw` drawing document and its views;
//! - [`hlr`]: hidden-line projection of exact solids;
//! - [`section`]: section views, cut from the part along a line, and hatching;
//! - [`detail`]: detail views, a circle of another view at a larger scale;
//! - [`callout`]: hole callouts from the Hole feature's data;
//! - [`dims`]: model and reference dimensions, and how they are drawn;
//! - [`title`]: the title block and its `{Name}` placeholders;
//! - [`layout`]: placing the views on the sheet and choosing pens;
//! - [`pdf`] and [`dxf`]: writing the laid-out sheet.

pub mod callout;
pub mod detail;
pub mod dims;
pub mod document;
pub mod dxf;
pub mod hlr;
pub mod layout;
pub mod pdf;
pub mod section;
pub mod sheet;
pub mod title;

pub use glam;

pub use callout::{HoleCallout, HoleData, HoleType};
pub use detail::DetailDef;
pub use dims::{DimSource, Dimension, ModelItem, RefPick};
pub use document::{DRAWING_EXT, DisplayStyle, Drawing, DrawingFileError, TangentEdges, View, ViewRole};
pub use hlr::{LineKind, Orientation, Path2, Progress, ProjectError, ViewGeometry};
pub use layout::{Pen, SheetPicture, layout};
pub use section::{HatchPattern, SectionDef};
pub use sheet::{Projection, Scale, Sheet, SheetSize};
pub use title::{TitleBlock, TitleField};

use anvil_brep::Solid;
use anvil_kernel::Frame;
use anvil_model::{BodyOp, EndCondition, FeatureKind};
use anvil_sketch::Sketch;
use std::collections::HashMap;
use std::path::Path;

/// Why a part's views cannot be drawn, in words for the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartError(pub String);

impl std::fmt::Display for PartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for PartError {}

/// A solved sketch of the part and where it lies in model space.
#[derive(Debug, Clone, PartialEq)]
pub struct PlacedSketch {
    pub feature: u32,
    pub sketch: Sketch,
    pub frame: Frame,
}

/// The depth of an extrude, from its start plane along its direction.
#[derive(Debug, Clone, PartialEq)]
pub struct ExtrudeDepth {
    pub feature: u32,
    /// The extrude's sketch feature.
    pub sketch: u32,
    /// The start plane: the sketch plane moved along its normal by this much.
    pub start: f64,
    /// +1 when the extrude goes along the sketch normal, -1 against it.
    pub sign: f64,
    pub depth: f64,
    /// The depth is split evenly on both sides of the start plane.
    pub mid_plane: bool,
}

/// Everything a drawing takes from its part: the exact bodies the views are projected
/// from, the sketches and depths model dimensions come from, and the values the title block
/// is filled with.
#[derive(Debug, Clone, Default)]
pub struct PartData {
    pub bodies: Vec<Solid>,
    /// Every built sketch, in feature order.
    pub sketches: Vec<PlacedSketch>,
    /// Every built Blind or Mid Plane extrude, in feature order.
    pub depths: Vec<ExtrudeDepth>,
    /// Custom properties as they apply in the configuration shown, in the part's order.
    pub properties: Vec<(String, String)>,
    /// The configuration the data comes from.
    pub configuration: String,
    /// Every configuration of the part, in the part's order.
    pub configurations: Vec<String>,
    pub material: Option<String>,
    /// How section views hatch the part's material.
    pub hatch: HatchPattern,
    /// Every built Hole feature, in feature order.
    pub holes: Vec<HoleData>,
    /// The part's file name without its extension.
    pub file_name: String,
}

impl PartData {
    pub fn sketch(&self, feature: u32) -> Option<&PlacedSketch> {
        self.sketches.iter().find(|s| s.feature == feature)
    }
}

/// The exact bodies a rebuilt part ends with, ready to be projected.
pub fn part_bodies(state: &anvil_model::BuildState) -> Result<Vec<Solid>, PartError> {
    match &state.exact {
        Some(bodies) if !bodies.is_empty() => Ok(bodies.clone()),
        Some(_) => Err(PartError("the part has no solid body yet; add a feature that makes one".into())),
        None => Err(PartError(match &state.faceted_from {
            Some((_, why)) => format!(
                "the part is not exact from one of its features on ({why}), and drawing views need exact geometry"
            ),
            None => "the part is not exact, and drawing views need exact geometry".into(),
        })),
    }
}

/// Rebuilds `doc` in `configuration` (its active one when `None`) and collects what a
/// drawing needs from it.
pub fn part_data(
    doc: &anvil_model::Document,
    configuration: Option<&str>,
    file_name: &str,
) -> Result<PartData, PartError> {
    let mut doc = doc.clone();
    doc.normalize_configurations();
    if let Some(name) = configuration {
        let Some(id) = doc.configuration_by_name(name).map(|c| c.id) else {
            return Err(PartError(format!(
                "the part has no configuration \"{name}\"; choose another configuration for the drawing"
            )));
        };
        doc.activate_configuration(id).map_err(|e| PartError(format!("cannot show \"{name}\": {e}")))?;
    }
    let state = doc.rebuild();
    let bodies = part_bodies(&state)?;
    let mut sketches = Vec::new();
    let mut depths = Vec::new();
    let mut holes = Vec::new();
    let limit = doc.rollback.unwrap_or(usize::MAX).min(doc.features.len());
    for f in doc.features.iter().take(limit) {
        if f.suppressed || state.errors.contains_key(&f.id) {
            continue;
        }
        match &f.kind {
            FeatureKind::Sketch(sf) => {
                if let Some(frame) = state.frames.get(&f.id) {
                    sketches.push(PlacedSketch { feature: f.id, sketch: sf.sketch.clone(), frame: *frame });
                }
            }
            FeatureKind::Extrude(ef) if matches!(ef.end, EndCondition::Blind | EndCondition::MidPlane) => {
                let Ok(depth) = ef.depth.eval(&state.variables) else { continue };
                let start = match &ef.start_offset {
                    Some(d) => match d.eval(&state.variables) {
                        Ok(v) if ef.start_flip => -v,
                        Ok(v) => v,
                        Err(_) => continue,
                    },
                    None => 0.0,
                };
                let boss = ef.op == BodyOp::Boss;
                let sign = if boss != ef.reverse { 1.0 } else { -1.0 };
                depths.push(ExtrudeDepth {
                    feature: f.id,
                    sketch: ef.sketch,
                    start,
                    sign,
                    depth,
                    mid_plane: ef.end == EndCondition::MidPlane,
                });
            }
            FeatureKind::Hole(h) => {
                if let Some(hole) = hole_data(f.id, h, &doc, &state) {
                    holes.push(hole);
                }
            }
            _ => {}
        }
    }
    let active = doc.active_configuration;
    Ok(PartData {
        bodies,
        sketches,
        depths,
        properties: doc.properties_for(active).into_iter().map(|p| (p.name, p.value)).collect(),
        configuration: doc.active_configuration_name().to_string(),
        configurations: doc.configurations.iter().map(|c| c.name.clone()).collect(),
        material: doc.material.as_ref().map(|m| m.name.clone()),
        hatch: HatchPattern::for_material(
            doc.material.as_ref().map(|m| m.name.as_str()),
            doc.material.as_ref().is_some_and(|m| m.class == anvil_model::material::MaterialClass::Plastic),
        ),
        holes,
        file_name: file_name.to_string(),
    })
}

/// What a drawing needs from a built Hole feature: its resolved sizes, where its holes
/// are, and which way they run.
fn hole_data(
    feature: u32,
    h: &anvil_model::HoleFeature,
    doc: &anvil_model::Document,
    state: &anvil_model::BuildState,
) -> Option<HoleData> {
    use anvil_model::HoleKind;
    let vars = &state.variables;
    let sec = h.resolve(vars).ok()?;
    let frame = state.frames.get(&h.sketch)?;
    let sk = doc.features.iter().find(|f| f.id == h.sketch).and_then(|f| match &f.kind {
        FeatureKind::Sketch(sf) => Some(&sf.sketch),
        _ => None,
    })?;
    let d3 = |v: anvil_kernel::Vec3| glam::DVec3::new(v.x, v.y, v.z);
    let out = if h.reverse { -frame.z } else { frame.z };
    let centres: Vec<glam::DVec3> = anvil_model::hole::positions(sk)
        .into_iter()
        .map(|c| d3(frame.origin + frame.x * c.x + frame.y * c.y))
        .collect();
    if centres.is_empty() {
        return None;
    }
    let kind = match &h.kind {
        HoleKind::Simple => HoleType::Simple,
        HoleKind::Counterbore { .. } => {
            let (diameter, depth) = sec.counterbore?;
            HoleType::Counterbore { diameter, depth }
        }
        HoleKind::Countersink(_) => {
            let (diameter, angle) = sec.sink?;
            HoleType::Countersink { diameter, angle }
        }
        HoleKind::Tapped { major, pitch, thread_depth } => {
            let (m, p, t) = (major.eval(vars).ok()?, pitch.eval(vars).ok()?, thread_depth.eval(vars).ok()?);
            HoleType::Tapped { thread: callout::thread_name(h.size.as_ref(), m, p), depth: t }
        }
    };
    let near_sink = match h.kind {
        HoleKind::Countersink(_) => None,
        _ => sec.sink,
    };
    let outer = [Some(sec.diameter), sec.counterbore.map(|c| c.0), sec.sink.map(|s| s.0)]
        .into_iter()
        .flatten()
        .fold(0.0, f64::max);
    Some(HoleData {
        feature,
        kind,
        diameter: sec.diameter,
        depth: sec.depth,
        near_sink,
        centres,
        axis: -d3(out),
        outer_radius: 0.5 * outer,
    })
}

/// Loads and rebuilds the part file at `path` in `configuration` (its saved active one
/// when `None`) and collects what a drawing needs from it.
pub fn load_part(path: &Path, configuration: Option<&str>) -> Result<PartData, PartError> {
    let doc = anvil_model::Document::load(path)
        .map_err(|e| PartError(format!("cannot open the part {}: {e}", path.display())))?;
    let name = path.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    part_data(&doc, configuration, &name)
}

/// The views of a drawing, projected: each view's lines by id, and why any view could not
/// be made, in words for the user.
#[derive(Debug, Clone, Default)]
pub struct Projected {
    pub geometry: HashMap<u32, ViewGeometry>,
    pub failed: HashMap<u32, String>,
}

/// Projects every view of `drawing` from `bodies`: standard views once per direction,
/// section views cut from the bodies (hatched with `hatch`), and detail views clipped from
/// their parents.
pub fn project_views(
    drawing: &Drawing,
    bodies: &[Solid],
    hatch: HatchPattern,
    progress: &Progress,
) -> Result<Projected, ProjectError> {
    let projection = drawing.sheet.projection;
    let mut dirs: Vec<Orientation> = Vec::new();
    let mut sections = 0;
    for v in &drawing.views {
        match v.role {
            ViewRole::Section => sections += 1,
            ViewRole::Detail => {}
            r => {
                let o = r.orientation(projection);
                if !dirs.contains(&o) {
                    dirs.push(o);
                }
            }
        }
    }
    progress.set_stages(dirs.len() + sections);
    let mut stage = 0;
    let mut by_dir: HashMap<Orientation, ViewGeometry> = HashMap::new();
    let mut out = Projected::default();
    for v in &drawing.views {
        match v.role {
            ViewRole::Section | ViewRole::Detail => {}
            r => {
                let o = r.orientation(projection);
                if let std::collections::hash_map::Entry::Vacant(e) = by_dir.entry(o) {
                    progress.begin_stage(stage);
                    stage += 1;
                    e.insert(hlr::project(bodies, &o.axes(), progress)?);
                }
                if let Some(g) = by_dir.get(&o) {
                    out.geometry.insert(v.id, g.clone());
                }
            }
        }
    }
    for v in drawing.views.iter().filter(|v| v.role == ViewRole::Section) {
        progress.begin_stage(stage);
        stage += 1;
        let Some(def) = &v.section else {
            out.failed
                .insert(v.id, "the section view has no cutting line; delete it and make it again".into());
            continue;
        };
        match section::project_section(def, bodies, hatch, progress)? {
            Ok(g) => {
                out.geometry.insert(v.id, g);
            }
            Err(e) => {
                out.failed.insert(v.id, e.0);
            }
        }
    }
    // Details of details need their parent first; each pass places those whose parent is in.
    let mut left: Vec<&View> = drawing.views.iter().filter(|v| v.role == ViewRole::Detail).collect();
    while !left.is_empty() {
        let before = left.len();
        left.retain(|v| {
            let Some(def) = &v.detail else {
                out.failed.insert(v.id, "the detail view has no circle; delete it and make it again".into());
                return false;
            };
            if let Some(why) = out.failed.get(&def.parent).cloned() {
                out.failed.insert(v.id, format!("its parent view cannot be shown: {why}"));
                return false;
            }
            let (Some(parent), Some(o)) =
                (out.geometry.get(&def.parent), drawing.view_orientation(def.parent))
            else {
                return true;
            };
            let (c, r) = def.circle(&o.axes());
            let g = detail::detail_geometry(parent, c, r);
            out.geometry.insert(v.id, g);
            false
        });
        if left.len() == before {
            for v in left {
                out.failed.insert(v.id, "its parent view is missing; delete it".into());
            }
            break;
        }
    }
    Ok(out)
}

/// Writes `bytes` to `path` through a temporary file, so a crash never leaves half a file.
fn write_atomic(path: &Path, ext: &str, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension(format!("{ext}.tmp"));
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}

/// Writes the drawing's sheet to a PDF file.
pub fn export_pdf(
    drawing: &Drawing,
    geometry: &HashMap<u32, ViewGeometry>,
    part: Option<&PartData>,
    path: &Path,
    title: &str,
) -> std::io::Result<()> {
    let pic = layout::layout_full(drawing, geometry, part, title);
    write_atomic(path, "pdf", &pdf::write_pdf(&pic.for_output(), title))
}

/// Writes the drawing's sheet to a DXF file.
pub fn export_dxf(
    drawing: &Drawing,
    geometry: &HashMap<u32, ViewGeometry>,
    part: Option<&PartData>,
    path: &Path,
    title: &str,
) -> std::io::Result<()> {
    let pic = layout::layout_full(drawing, geometry, part, title);
    write_atomic(path, "dxf", dxf::write_dxf(&pic.for_output()).as_bytes())
}

#[cfg(test)]
mod tests;
