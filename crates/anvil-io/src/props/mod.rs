//! File properties of other programs' files, read without modifying them.
//!
//! A company that opens `wheel.sldprt` in AnvilCAD should see the custom properties its
//! engineers already set (`Description: large wheel`, `Process: Lathe`), without SOLIDWORKS
//! running. AnvilCAD reads them through public interfaces only (see INTEROPERABILITY.md):
//!
//! - [`oleps`]: the standard OLE property sets Microsoft documents in [MS-OLEPS], inside
//!   compound files ([MS-CFB], [`cfb`]). These are what Windows Explorer shows on a file's
//!   Custom tab. Files saved by SOLIDWORKS 2014 or earlier are compound files.
//! - [`docmgr`] (Windows): the SOLIDWORKS Document Manager API that Dassault Systèmes
//!   publishes for reading properties without SOLIDWORKS, using the customer's own licence
//!   key. Needed for files saved by SOLIDWORKS 2015 or later.
//! - [`sidecar`]: a snapshot of those properties taken by `anvil-props convert`, kept next
//!   to the file. Once a company has converted its files, AnvilCAD shows their properties on
//!   any computer with neither SOLIDWORKS nor a Document Manager key.
//!
//! The other program's properties are never changed and never claimed as AnvilCAD's.
//! AnvilCAD's own properties for the file live in the sidecar, beside them.

pub mod cfb;
#[cfg(windows)]
pub mod docmgr;
pub mod oleps;
pub mod sidecar;

use anvil_model::document::PropertyKind;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Environment variable holding a SOLIDWORKS Document Manager licence key.
pub const KEY_ENV: &str = "ANVIL_SWDM_KEY";

/// Where a set of properties was read from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Source {
    /// Standard OLE property set streams ([MS-OLEPS]).
    OlePropertySet,
    /// The SOLIDWORKS Document Manager API.
    DocumentManager,
}

impl Source {
    pub fn label(&self) -> &'static str {
        match self {
            Source::OlePropertySet => "standard property set",
            Source::DocumentManager => "SOLIDWORKS Document Manager",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FileProperty {
    pub name: String,
    pub kind: PropertyKind,
    /// The evaluated value, as the other program shows it.
    pub value: String,
    /// The text expression, when it differs from the value (for example a link to the
    /// part's mass or material).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expression: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Configuration {
    pub name: String,
    pub properties: Vec<FileProperty>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FileProperties {
    pub source: Source,
    /// File-level custom properties, in the file's order.
    pub properties: Vec<FileProperty>,
    /// Configuration-specific custom properties.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub configurations: Vec<Configuration>,
    /// Standard summary fields (title, author, keywords...).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub summary: Vec<FileProperty>,
    /// Anything the reader could not read or wants the user to know.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

impl FileProperties {
    pub fn new(source: Source) -> FileProperties {
        FileProperties {
            source,
            properties: Vec::new(),
            configurations: Vec::new(),
            summary: Vec::new(),
            notes: Vec::new(),
        }
    }

    pub fn get(&self, name: &str) -> Option<&FileProperty> {
        self.properties.iter().find(|p| p.name.eq_ignore_ascii_case(name))
    }

    pub fn count(&self) -> usize {
        self.properties.len() + self.configurations.iter().map(|c| c.properties.len()).sum::<usize>()
    }
}

#[derive(Clone, Debug, Default)]
pub struct ReadOptions {
    /// A SOLIDWORKS Document Manager licence key. Never written anywhere by AnvilCAD.
    pub document_manager_key: Option<String>,
}

impl ReadOptions {
    /// Takes the Document Manager key from [`KEY_ENV`], if set.
    pub fn from_env() -> ReadOptions {
        let key = std::env::var(KEY_ENV).ok().map(|k| k.trim().to_string()).filter(|k| !k.is_empty());
        ReadOptions { document_manager_key: key }
    }
}

#[derive(Debug)]
pub enum ReadError {
    Io(std::io::Error),
    /// The file is a compound file with no standard property sets.
    NoPropertySets,
    /// The file can only be read through the Document Manager, which is unavailable here.
    NeedsDocumentManager(String),
    /// The Document Manager reported an error.
    DocumentManager(String),
    Damaged(String),
}

impl std::fmt::Display for ReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReadError::Io(e) => write!(f, "{e}"),
            ReadError::NoPropertySets => f.write_str("the file has no standard property sets"),
            ReadError::NeedsDocumentManager(why) => f.write_str(why),
            ReadError::DocumentManager(e) => write!(f, "SOLIDWORKS Document Manager: {e}"),
            ReadError::Damaged(e) => write!(f, "damaged file: {e}"),
        }
    }
}

impl std::error::Error for ReadError {}

impl From<std::io::Error> for ReadError {
    fn from(e: std::io::Error) -> Self {
        ReadError::Io(e)
    }
}

/// Reads a file's properties live, from the file itself. Never writes to the file.
///
/// With a Document Manager key on Windows, SOLIDWORKS files are read through it (it covers
/// every version and configuration properties). Otherwise compound files are read through
/// their standard property sets.
pub fn read(path: &Path, opts: &ReadOptions) -> Result<FileProperties, ReadError> {
    let is_sw = crate::solidworks::doc_type_from_extension(path).is_some();
    #[cfg(windows)]
    let dm_error = match (&opts.document_manager_key, is_sw) {
        (Some(key), true) => match docmgr::read(path, key) {
            Ok(p) => return Ok(p),
            Err(e) => Some(e),
        },
        _ => None,
    };
    #[cfg(not(windows))]
    let dm_error: Option<ReadError> = {
        let _ = opts;
        None
    };

    let bytes = std::fs::read(path)?;
    if cfb::is_cfb(&bytes) {
        let c = cfb::Cfb::parse(&bytes).map_err(|e| ReadError::Damaged(e.to_string()))?;
        return match oleps::read(&c) {
            Some(mut p) => {
                if let Some(e) = dm_error {
                    p.notes.push(format!(
                        "The Document Manager could not read this file ({e}); its standard property \
                         sets were read instead."
                    ));
                }
                Ok(p)
            }
            None => Err(dm_error.unwrap_or(ReadError::NoPropertySets)),
        };
    }
    if let Some(e) = dm_error {
        return Err(e);
    }
    if is_sw {
        return Err(ReadError::NeedsDocumentManager(needs_document_manager().into()));
    }
    Err(ReadError::NoPropertySets)
}

/// Why a modern SOLIDWORKS file could not be read here, and what to do.
pub fn needs_document_manager() -> &'static str {
    if cfg!(windows) {
        "Files saved by SOLIDWORKS 2015 or later are read through the SOLIDWORKS Document \
         Manager. Install it and set ANVIL_SWDM_KEY to your Document Manager licence key \
         (SOLIDWORKS subscription customers can request one from Dassault Systèmes), or open \
         the properties converted by anvil-props."
    } else {
        "Files saved by SOLIDWORKS 2015 or later are read through the SOLIDWORKS Document \
         Manager, which runs on Windows. Convert the files once with anvil-props on a Windows \
         computer; AnvilCAD then shows their properties here."
    }
}
