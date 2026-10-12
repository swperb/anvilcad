//! STEP (ISO 10303-21) import and exact export for AnvilCAD.
//!
//! - [`p21`]: the exchange-structure reader (never panics on malformed input)
//! - [`reader`]: B-rep bodies, units and geometry from the parsed entities
//! - [`shapes`]: curve and surface evaluation and point inversion
//! - [`exact`]: exact `anvil-brep` solids for plane and cylinder bodies
//! - [`mesh`]: tessellation of every other supported surface
//! - [`export`]: exact AP214 export from `anvil-brep` solids

pub mod exact;
pub mod export;
pub mod mesh;
pub mod p21;
pub mod reader;
pub mod shapes;

pub use mesh::{Mesh, MeshFace};

#[derive(Debug, Clone, thiserror::Error)]
pub enum StepError {
    #[error(transparent)]
    Parse(#[from] p21::ParseError),
    #[error("the file contains no solid or surface bodies{}", if .0.is_empty() { String::new() } else { format!(" ({})", .0.join("; ")) })]
    NoBodies(Vec<String>),
}

/// One imported body. `mesh` faces are in the source's face order; when the body
/// could be built exactly, `exact` holds it with faces tagged in the same order.
#[derive(Debug, Clone)]
pub struct Body {
    pub name: String,
    pub exact: Option<anvil_brep::Solid>,
    /// Why the body is not exact, when it is not.
    pub not_exact: Option<String>,
    pub mesh: Mesh,
    /// False for surface models (open shells).
    pub solid: bool,
}

#[derive(Debug, Clone)]
pub struct Import {
    pub bodies: Vec<Body>,
    pub warnings: Vec<String>,
    pub length_unit: String,
    pub schemas: Vec<String>,
}

/// Reads a STEP file's text.
pub fn import(text: &str) -> Result<Import, StepError> {
    let ex = p21::parse(text)?;
    let model = reader::read(&ex);
    let mut warnings = model.warnings.clone();
    if model.assembly {
        warnings.push(
            "this file is an assembly; its parts are imported in their own coordinates (assembly placement is not supported yet)"
                .into(),
        );
    }
    let mut bodies = Vec::new();
    for b in &model.bodies {
        let (exact, not_exact) = match exact::convert(&model, b) {
            Ok(s) => (Some(s), None),
            Err(why) => (None, Some(why)),
        };
        let mesh = mesh::tessellate(&model, b, &mut warnings);
        let name = if b.name.is_empty() { format!("Body #{}", b.id) } else { b.name.clone() };
        bodies.push(Body { name, exact, not_exact, mesh, solid: b.solid });
    }
    if bodies.is_empty() {
        return Err(StepError::NoBodies(warnings));
    }
    Ok(Import { bodies, warnings, length_unit: model.length_unit, schemas: ex.schemas() })
}

#[cfg(test)]
mod tests;
