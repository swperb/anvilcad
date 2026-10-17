//! Shells: a solid hollowed to a wall of even thickness, opened through chosen faces.
//!
//! The cavity is the solid with every face moved inwards by the wall thickness, except
//! the faces to remove, which move outwards by it so the cavity breaks through them
//! (see [`crate::offset`]); the shell is the solid less the cavity. The behaviour is
//! specified in `docs/re/shell-and-fillet.md`.

use crate::boolean::{self, BoolOp};
use crate::topo::Solid;
use crate::{KernelError, Result, offset};

/// The cavity for a shell of `solid` with wall `thickness`, open through the faces at
/// indices `remove`. Its faces keep the solid's face order.
pub fn cavity(solid: &Solid, remove: &[usize], thickness: f64) -> Result<Solid> {
    if !(thickness.is_finite() && thickness > 0.0) {
        return Err(KernelError::InvalidInput("the wall thickness must be a positive number".into()));
    }
    if remove.is_empty() {
        return Err(KernelError::Unsupported(
            "a shell with no face removed (a sealed hollow) needs a second shell inside the solid".into(),
        ));
    }
    if let Some(&f) = remove.iter().find(|&&f| f >= solid.faces.len()) {
        return Err(KernelError::InvalidInput(format!("face {f} does not exist")));
    }
    let distance: Vec<f64> =
        (0..solid.faces.len()).map(|f| if remove.contains(&f) { thickness } else { -thickness }).collect();
    offset::offset_faces(solid, &distance)
}

/// `solid` hollowed to a wall of `thickness`, open through the faces at `remove`.
pub fn shell(solid: &Solid, remove: &[usize], thickness: f64) -> Result<Vec<Solid>> {
    let hollow = cavity(solid, remove, thickness)?;
    boolean::boolean(solid, &hollow, BoolOp::Subtract)
}
