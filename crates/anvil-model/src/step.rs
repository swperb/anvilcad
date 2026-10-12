//! STEP import into a new part document.

use crate::document::{Document, FeatureKind, ImportedBodyFeature, ImportedFace};

/// What a STEP import produced, for telling the user.
#[derive(Debug, Clone, Default)]
pub struct StepReport {
    pub bodies: usize,
    /// Bodies imported as exact solids; later features on them build exactly.
    pub exact: usize,
    /// Why each non-exact body is a mesh.
    pub meshed: Vec<String>,
    pub warnings: Vec<String>,
    pub length_unit: String,
}

impl StepReport {
    pub fn summary(&self) -> String {
        let mut s = format!(
            "Imported {} bod{} ({} exact, {} as meshes). File units: {}.",
            self.bodies,
            if self.bodies == 1 { "y" } else { "ies" },
            self.exact,
            self.bodies - self.exact,
            self.length_unit
        );
        for m in &self.meshed {
            s.push_str(&format!("\n- {m}"));
        }
        let shown = self.warnings.len().min(8);
        for w in &self.warnings[..shown] {
            s.push_str(&format!("\n- {w}"));
        }
        if self.warnings.len() > shown {
            s.push_str(&format!("\n- and {} more warnings", self.warnings.len() - shown));
        }
        s
    }
}

/// A new part holding every body of a STEP file as an imported body.
pub fn document_from_step(text: &str, source: &str) -> Result<(Document, StepReport), String> {
    let imp = anvil_step::import(text).map_err(|e| e.to_string())?;
    let mut doc = Document::new();
    let mut report = StepReport {
        bodies: imp.bodies.len(),
        warnings: imp.warnings.clone(),
        length_unit: imp.length_unit.clone(),
        ..StepReport::default()
    };
    for b in imp.bodies {
        if b.exact.is_some() {
            report.exact += 1;
        } else if let Some(why) = &b.not_exact {
            report.meshed.push(format!("{}: {why}", b.name));
        }
        let faces = b
            .mesh
            .faces
            .into_iter()
            .map(|f| ImportedFace { planar: f.planar, triangles: f.triangles })
            .collect();
        doc.add_feature(FeatureKind::ImportedBody(ImportedBodyFeature {
            source: format!("STEP: {source}"),
            replaces: vec![b.name],
            vertices: b.mesh.vertices,
            faces,
            exact: b.exact,
            adds: true,
        }));
    }
    Ok((doc, report))
}

/// Exact STEP text for the part's bodies, when the whole part was built exactly.
pub fn exact_step(state: &crate::BuildState, name: &str) -> Option<String> {
    let bodies = state.exact.as_ref()?;
    (!bodies.is_empty()).then(|| anvil_step::export::write(bodies, name))
}
