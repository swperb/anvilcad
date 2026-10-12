//! SOLIDWORKS file identification.
//!
//! AnvilCAD reads SOLIDWORKS files through anvil-migrate, which drives an installed copy
//! of SOLIDWORKS through its public API. This module identifies files so the application can
//! explain what it is looking at and how to convert it.
//!
//! Background: through SOLIDWORKS 2014 files were OLE compound documents. From 2015 on they
//! use a different, undocumented container. No public specification exists for either.

use std::path::Path;

/// How many release years back AnvilCAD aims to support.
pub const SUPPORT_WINDOW_YEARS: u32 = 5;

const OLE_MAGIC: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwDocType {
    Part,
    Assembly,
    Drawing,
    PartTemplate,
    AssemblyTemplate,
    DrawingTemplate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwContainer {
    /// OLE compound document: SOLIDWORKS 2014 or earlier.
    Ole,
    /// Post-2015 container.
    Modern,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SwFileInfo {
    pub doc_type: SwDocType,
    pub container: SwContainer,
    pub size: u64,
}

impl SwFileInfo {
    /// A user-facing explanation of what can and cannot be done with this file today.
    pub fn explanation(&self) -> String {
        let kind = match self.doc_type {
            SwDocType::Part => "part",
            SwDocType::Assembly => "assembly",
            SwDocType::Drawing => "drawing",
            SwDocType::PartTemplate => "part template",
            SwDocType::AssemblyTemplate => "assembly template",
            SwDocType::DrawingTemplate => "drawing template",
        };
        let era = match self.container {
            SwContainer::Ole => " (saved by SOLIDWORKS 2014 or earlier)",
            SwContainer::Modern => "",
        };
        format!(
            "This is a SOLIDWORKS {kind}{era}. Convert it with anvil-migrate on a computer \
             that has SOLIDWORKS installed; parts keep their feature tree. Example: \
             anvil-migrate -o migrated \"<folder with your files>\""
        )
    }
}

pub fn doc_type_from_extension(path: &Path) -> Option<SwDocType> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    Some(match ext.as_str() {
        "sldprt" => SwDocType::Part,
        "sldasm" => SwDocType::Assembly,
        "slddrw" => SwDocType::Drawing,
        "prtdot" => SwDocType::PartTemplate,
        "asmdot" => SwDocType::AssemblyTemplate,
        "drwdot" => SwDocType::DrawingTemplate,
        _ => return None,
    })
}

pub fn identify_bytes(path: &Path, header: &[u8], size: u64) -> Option<SwFileInfo> {
    let doc_type = doc_type_from_extension(path)?;
    let container =
        if header.len() >= 8 && header[..8] == OLE_MAGIC { SwContainer::Ole } else { SwContainer::Modern };
    Some(SwFileInfo { doc_type, container, size })
}

/// Identifies a SOLIDWORKS file on disk. Returns `Ok(None)` if the extension is not a
/// SOLIDWORKS one.
pub fn identify(path: &Path) -> std::io::Result<Option<SwFileInfo>> {
    use std::io::Read;
    if doc_type_from_extension(path).is_none() {
        return Ok(None);
    }
    let mut f = std::fs::File::open(path)?;
    let size = f.metadata()?.len();
    let mut header = [0u8; 8];
    let n = f.read(&mut header)?;
    Ok(identify_bytes(path, &header[..n], size))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_container() {
        let p = Path::new("bracket.SLDPRT");
        let old = identify_bytes(p, &OLE_MAGIC, 100).unwrap();
        assert_eq!(old.container, SwContainer::Ole);
        assert_eq!(old.doc_type, SwDocType::Part);
        let new = identify_bytes(p, &[0, 1, 2, 3, 4, 5, 6, 7], 100).unwrap();
        assert_eq!(new.container, SwContainer::Modern);
        assert!(identify_bytes(Path::new("a.step"), &OLE_MAGIC, 1).is_none());
        assert_eq!(doc_type_from_extension(Path::new("x.slddrw")), Some(SwDocType::Drawing));
    }
}
