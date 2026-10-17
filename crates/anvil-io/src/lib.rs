//! AnvilCAD file exchange.
//!
//! - [`stl`]: binary and ASCII STL export
//! - [`obj`]: Wavefront OBJ export
//! - [`step`]: STEP AP214 export (faceted boundary representation)
//! - [`solidworks`]: identifies SOLIDWORKS files and their container generation
//! - [`props`]: reads custom properties from other programs' files without modifying them,
//!   and keeps AnvilCAD's own properties for those files in a sidecar

#[cfg(windows)]
pub mod com;
pub mod obj;
pub mod props;
pub mod solidworks;
pub mod step;
pub mod stl;

use std::path::Path;

/// Exports a solid, choosing the format from the file extension.
pub fn export_by_extension(solid: &anvil_kernel::Solid, path: &Path, name: &str) -> std::io::Result<()> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    let bytes = match ext.as_str() {
        "stl" => stl::write_binary(solid, name),
        "obj" => obj::write(solid, name).into_bytes(),
        "step" | "stp" => step::write(solid, name).into_bytes(),
        other => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("unsupported export format '.{other}'"),
            ))
        }
    };
    std::fs::write(path, bytes)
}
