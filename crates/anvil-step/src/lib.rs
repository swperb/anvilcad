//! STEP (ISO 10303-21) import and exact export for AnvilCAD.
//!
//! - [`assembly`]: product structure and occurrence placement of assemblies
//! - [`p21`]: the exchange-structure reader (never panics on malformed input)
//! - [`reader`]: B-rep bodies, units and geometry from the parsed entities
//! - [`shapes`]: curve and surface evaluation and point inversion
//! - [`exact`]: exact `anvil-brep` solids for plane and cylinder bodies
//! - [`mesh`]: tessellation of every other supported surface
//! - [`export`]: exact AP214 export from `anvil-brep` solids

pub mod assembly;
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

/// One use of a product in an assembly: a part or a sub-assembly placed in its parent.
/// A part used three times is three occurrences, each with its own bodies.
#[derive(Debug, Clone)]
pub struct Occurrence {
    /// The instance name: the usage's own name, or the product's name with an
    /// instance number.
    pub name: String,
    /// The name of the product this is a use of.
    pub product: String,
    /// The parent occurrence's index in [`Import::occurrences`]; `None` for a root.
    pub parent: Option<usize>,
    /// Where the occurrence sits in the root's coordinates (mm): its origin and x, y
    /// and z axes, the composition of every placement down from the root.
    pub placement: anvil_brep::Frame,
    /// Indices into [`Import::bodies`] of the bodies this occurrence itself places
    /// (not those of its children), already moved by `placement`.
    pub bodies: Vec<usize>,
}

#[derive(Debug, Clone)]
pub struct Import {
    pub bodies: Vec<Body>,
    /// The product structure of an assembly, parents before children; empty when the
    /// file is not an assembly or its structure could not be read.
    pub occurrences: Vec<Occurrence>,
    pub warnings: Vec<String>,
    pub length_unit: String,
    pub schemas: Vec<String>,
}

impl Body {
    /// The body moved by a rigid motion.
    fn placed(&self, m: &anvil_brep::Frame) -> Body {
        let mut b = self.clone();
        b.exact = self.exact.as_ref().map(|s| s.transformed(m));
        for v in &mut b.mesh.vertices {
            *v = m.apply_point(glam::DVec3::from_array(*v)).to_array();
        }
        b
    }
}

/// Reads a STEP file's text.
pub fn import(text: &str) -> Result<Import, StepError> {
    let ex = p21::parse(text)?;
    let model = reader::read(&ex);
    let mut warnings = model.warnings.clone();
    // Each body the file defines, keyed by its entity id, in its own coordinates.
    let mut defined: Vec<(u64, Body)> = Vec::new();
    for b in &model.bodies {
        // An exact body's mesh is the kernel's own tessellation of it (which also proves
        // every face lays out); otherwise the importer meshes the file's faces itself.
        let (exact, not_exact, mesh) = match exact::convert(&model, b) {
            Ok(s) => match mesh::from_exact(&s, b.faces.len()) {
                Some(m) => (Some(s), None, m),
                None => (
                    None,
                    Some("a face could not be laid out exactly, so it is imported as a mesh".into()),
                    mesh::tessellate(&model, b, &mut warnings),
                ),
            },
            Err(why) => (None, Some(why), mesh::tessellate(&model, b, &mut warnings)),
        };
        let name = if b.name.is_empty() { format!("Body #{}", b.id) } else { b.name.clone() };
        defined.push((b.id, Body { name, exact, not_exact, mesh, solid: b.solid }));
    }
    for t in &model.tessellated {
        let mesh = Mesh {
            vertices: t.vertices.iter().map(|v| [v.x, v.y, v.z]).collect(),
            faces: t
                .faces
                .iter()
                .map(|(planar, tris)| MeshFace { planar: *planar, triangles: tris.clone() })
                .collect(),
        };
        let name = if t.name.is_empty() { "Tessellated body".to_string() } else { t.name.clone() };
        let body = Body {
            name,
            exact: None,
            not_exact: Some("the file stores this body as triangles, so it is imported as a mesh".into()),
            mesh,
            solid: t.solid,
        };
        defined.push((t.id, body));
    }
    let ids: std::collections::HashSet<u64> = defined.iter().map(|(id, _)| *id).collect();
    let structure = assembly::read(&ex, model.length_scale, &ids).filter(|s| !s.instances.is_empty());
    let mut bodies = Vec::new();
    let mut occurrences = Vec::new();
    match structure {
        Some(s) => {
            warnings.extend(s.warnings);
            occurrences = s
                .nodes
                .into_iter()
                .map(|n| Occurrence {
                    name: n.name,
                    product: n.product,
                    parent: n.parent,
                    placement: n.place,
                    bodies: Vec::new(),
                })
                .collect::<Vec<_>>();
            let by_id: std::collections::HashMap<u64, &Body> =
                defined.iter().map(|(id, b)| (*id, b)).collect();
            let mut per_node = vec![0usize; occurrences.len()];
            for i in &s.instances {
                if let Some(c) = per_node.get_mut(i.node) {
                    *c += 1;
                }
            }
            let mut placed = std::collections::HashSet::new();
            for i in &s.instances {
                let (Some(body), Some(occ)) = (by_id.get(&i.item), occurrences.get_mut(i.node)) else {
                    continue;
                };
                let mut b = if i.place == anvil_brep::Frame::world() {
                    (*body).clone()
                } else {
                    body.placed(&i.place)
                };
                b.name = if per_node[i.node] == 1 {
                    occ.name.clone()
                } else {
                    format!("{} / {}", occ.name, b.name)
                };
                occ.bodies.push(bodies.len());
                bodies.push(b);
                placed.insert(i.item);
            }
            // Bodies no occurrence uses keep their own coordinates.
            bodies.extend(defined.into_iter().filter(|(id, _)| !placed.contains(id)).map(|(_, b)| b));
        }
        None => {
            if model.assembly {
                warnings.push(
                    "this file is an assembly, but its product structure could not be read; its parts are imported in their own coordinates"
                        .into(),
                );
            }
            bodies.extend(defined.into_iter().map(|(_, b)| b));
        }
    }
    if bodies.is_empty() {
        return Err(StepError::NoBodies(warnings));
    }
    Ok(Import { bodies, occurrences, warnings, length_unit: model.length_unit, schemas: ex.schemas() })
}

#[cfg(test)]
mod tests;
