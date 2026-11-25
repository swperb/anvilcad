//! Shells: a solid hollowed to a wall of even thickness, opened through chosen faces.
//!
//! The cavity is the solid with every face moved inwards by the wall thickness, except
//! the faces to remove, which stay where they are (see [`crate::offset`]); the shell is
//! the solid less the cavity. The cavity's copies of the removed faces lie on the solid's
//! own, so the subtraction takes both away and the cavity opens through them, with the
//! walls ending at the removed faces' plane. Moving them outwards instead would carry the
//! cavity past that plane, into any wall that rises beyond it (the step of an L-shaped
//! block opened at its short leg). The behaviour is
//! specified in `docs/re/shell-and-fillet.md`.

use crate::boolean::{self, BoolOp};
use crate::topo::Solid;
use crate::{KernelError, Result, offset, voids};

/// The cavity for a shell of `solid` with wall `thickness`, open through the faces at
/// indices `remove`, whose copies in the cavity stay on the solid's faces. Its faces keep
/// the solid's face order. With nothing removed every face moves inwards and the cavity
/// lies wholly inside the solid, so the shell is a sealed hollow.
pub fn cavity(solid: &Solid, remove: &[usize], thickness: f64) -> Result<Solid> {
    if !(thickness.is_finite() && thickness > 0.0) {
        return Err(KernelError::InvalidInput("the wall thickness must be a positive number".into()));
    }
    if let Some(&f) = remove.iter().find(|&&f| f >= solid.faces.len()) {
        return Err(KernelError::InvalidInput(format!("face {f} does not exist")));
    }
    let distance: Vec<f64> =
        (0..solid.faces.len()).map(|f| if remove.contains(&f) { 0.0 } else { -thickness }).collect();
    let cavity = offset::offset_faces(solid, &distance)?;
    // A sealed cavity has every face moved, so a wall as thick as the part turns it
    // inside out as a whole (each face passes its opposite) without any one face turning
    // over in its own plane. Its volume shows it: it must be positive and less than the
    // part's.
    if remove.is_empty() {
        let size = solid.bounds().map_or(1.0, |(lo, hi)| (hi - lo).max_element().max(1e-9));
        let (inside, whole) = (voids::signed_volume(&cavity)?, voids::signed_volume(solid)?);
        if !(inside > 1e-9 * size.powi(3) && inside < whole) {
            return Err(KernelError::Unsupported(
                "the wall is too thick: it leaves no room for the hollow".into(),
            ));
        }
    }
    Ok(cavity)
}

/// `solid` hollowed to a wall of `thickness`, open through the faces at `remove`. With no
/// face removed the cavity is sealed: the result is one solid whose second shell bounds
/// the void.
pub fn shell(solid: &Solid, remove: &[usize], thickness: f64) -> Result<Vec<Solid>> {
    let hollow = cavity(solid, remove, thickness)?;
    boolean::boolean(solid, &hollow, BoolOp::Subtract)
}
