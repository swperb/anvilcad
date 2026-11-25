//! AnvilCAD drawings: a sheet showing standard views of a part, projected from the part's
//! exact bodies with hidden lines removed or dashed, and exported as vector PDF.
//!
//! The behaviour follows the specification in `docs/re/drawings.md`. The pieces:
//!
//! - [`sheet`]: paper sizes, scales and the projection method;
//! - [`document`]: the `.anvdrw` drawing document and its views;
//! - [`hlr`]: hidden-line projection of exact solids;
//! - [`layout`]: placing the views on the sheet and choosing pens;
//! - [`pdf`]: writing the laid-out sheet as PDF.

pub mod document;
pub mod hlr;
pub mod layout;
pub mod pdf;
pub mod sheet;

pub use glam;

pub use document::{DRAWING_EXT, DisplayStyle, Drawing, DrawingFileError, TangentEdges, View, ViewRole};
pub use hlr::{LineKind, Orientation, Path2, Progress, ProjectError, ViewGeometry};
pub use layout::{Pen, SheetPicture, layout};
pub use sheet::{Projection, Scale, Sheet, SheetSize};

use anvil_brep::Solid;
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

/// Loads and rebuilds the part file at `path` and returns its exact bodies.
pub fn load_part(path: &Path) -> Result<Vec<Solid>, PartError> {
    let mut doc = anvil_model::Document::load(path)
        .map_err(|e| PartError(format!("cannot open the part {}: {e}", path.display())))?;
    part_bodies(&doc.rebuild())
}

/// Projects every view of `drawing` from `bodies`, once per direction.
pub fn project_views(
    drawing: &Drawing,
    bodies: &[Solid],
    progress: &Progress,
) -> Result<HashMap<u32, ViewGeometry>, ProjectError> {
    let mut by_dir: HashMap<Orientation, ViewGeometry> = HashMap::new();
    let mut out = HashMap::new();
    let mut dirs: Vec<Orientation> = Vec::new();
    for v in &drawing.views {
        let o = v.role.orientation(drawing.sheet.projection);
        if !dirs.contains(&o) {
            dirs.push(o);
        }
    }
    progress.set_stages(dirs.len());
    for v in &drawing.views {
        let o = v.role.orientation(drawing.sheet.projection);
        if !by_dir.contains_key(&o) {
            progress.begin_stage(by_dir.len());
            let g = hlr::project(bodies, &o.axes(), progress)?;
            by_dir.insert(o, g);
        }
        if let Some(g) = by_dir.get(&o) {
            out.insert(v.id, g.clone());
        }
    }
    Ok(out)
}

/// Writes the drawing's sheet to a PDF file.
pub fn export_pdf(
    drawing: &Drawing,
    geometry: &HashMap<u32, ViewGeometry>,
    path: &Path,
    title: &str,
) -> std::io::Result<()> {
    let bytes = pdf::write_pdf(&layout(drawing, geometry), title);
    let tmp = path.with_extension("pdf.tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests;
