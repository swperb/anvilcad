//! AnvilCAD's property file for another program's file: `wheel.sldprt.anvilprops.json`.
//!
//! It holds two separate things:
//!
//! - `imported`: a snapshot of the file's own properties, as read by `anvil-props convert`
//!   (or by AnvilCAD). It is refreshed from the file and never edited by hand in AnvilCAD,
//!   so the other program's properties stay its own.
//! - `properties`: AnvilCAD's own properties for the file, which AnvilCAD users edit.
//!
//! The file it describes is never opened for writing. Sidecars live next to the file by
//! default, or under a separate store folder (for read-only shares), mirroring the file's
//! absolute path.

use super::{FileProperties, FileProperty, ReadOptions, Source};
use anvil_model::document::{CustomProperty, PropertyKind};
use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};

pub const FORMAT: &str = "anvilcad-file-properties";
pub const FORMAT_VERSION: u32 = 1;
pub const SUFFIX: &str = ".anvilprops.json";
/// Environment variable naming a sidecar store folder.
pub const STORE_ENV: &str = "ANVIL_PROPS_STORE";

/// Identifies the version of the file a snapshot was taken from.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stamp {
    pub name: String,
    pub size: u64,
    /// Modification time, nanoseconds since 1970-01-01 UTC (as precise as the file system).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modified_ns: Option<i64>,
}

impl Stamp {
    pub fn of(path: &Path) -> std::io::Result<Stamp> {
        let m = std::fs::metadata(path)?;
        let modified_ns = m
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_nanos() as i64);
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        Ok(Stamp { name, size: m.len(), modified_ns })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Imported {
    /// When the snapshot was taken, seconds since 1970-01-01 UTC.
    pub read_at: i64,
    pub file: Stamp,
    #[serde(flatten)]
    pub properties: FileProperties,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Sidecar {
    pub format: String,
    pub format_version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub imported: Option<Imported>,
    /// AnvilCAD's own properties for this file.
    #[serde(default)]
    pub properties: Vec<CustomProperty>,
}

impl Default for Sidecar {
    fn default() -> Self {
        Sidecar {
            format: FORMAT.into(),
            format_version: FORMAT_VERSION,
            imported: None,
            properties: Vec::new(),
        }
    }
}

#[derive(Debug)]
pub enum SidecarError {
    Io(std::io::Error),
    Json(serde_json::Error),
    WrongFormat(String),
    TooNew(u32),
}

impl std::fmt::Display for SidecarError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SidecarError::Io(e) => write!(f, "{e}"),
            SidecarError::Json(e) => write!(f, "property file is damaged: {e}"),
            SidecarError::WrongFormat(s) => write!(f, "not an AnvilCAD property file (format '{s}')"),
            SidecarError::TooNew(v) => {
                write!(f, "property file version {v} is newer than this build supports")
            }
        }
    }
}

impl std::error::Error for SidecarError {}

impl From<std::io::Error> for SidecarError {
    fn from(e: std::io::Error) -> Self {
        SidecarError::Io(e)
    }
}

impl Sidecar {
    /// Loads a sidecar; `Ok(None)` if there is none.
    pub fn load(path: &Path) -> Result<Option<Sidecar>, SidecarError> {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        let v: serde_json::Value = serde_json::from_str(&text).map_err(SidecarError::Json)?;
        let format = v.get("format").and_then(|f| f.as_str()).unwrap_or("");
        if format != FORMAT {
            return Err(SidecarError::WrongFormat(format.into()));
        }
        let ver = v.get("format_version").and_then(|f| f.as_u64()).unwrap_or(0) as u32;
        if ver > FORMAT_VERSION {
            return Err(SidecarError::TooNew(ver));
        }
        serde_json::from_value(v).map(Some).map_err(SidecarError::Json)
    }

    /// Writes the sidecar atomically, creating its folder if needed.
    pub fn save(&self, path: &Path) -> Result<(), SidecarError> {
        if !path.to_string_lossy().ends_with(SUFFIX) {
            // Guards against ever writing over the file being described.
            return Err(SidecarError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("refusing to write a property file without the {SUFFIX} suffix"),
            )));
        }
        if let Some(dir) = path.parent() {
            if !dir.as_os_str().is_empty() {
                std::fs::create_dir_all(dir)?;
            }
        }
        let tmp = PathBuf::from(format!("{}.tmp", path.display()));
        let json = serde_json::to_string_pretty(self).map_err(SidecarError::Json)?;
        std::fs::write(&tmp, json)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    /// Whether the snapshot was taken from the file as it is now.
    pub fn is_current(&self, target: &Path) -> bool {
        match (&self.imported, Stamp::of(target)) {
            (Some(i), Ok(now)) => i.file == now,
            _ => false,
        }
    }
}

/// The sidecar path for `target`: next to it, or mirrored under `store`.
pub fn path_for(target: &Path, store: Option<&Path>) -> PathBuf {
    let Some(store) = store else {
        return PathBuf::from(format!("{}{SUFFIX}", target.display()));
    };
    let abs = std::path::absolute(target).unwrap_or_else(|_| target.to_path_buf());
    let mut out = store.to_path_buf();
    for c in abs.components() {
        match c {
            Component::Prefix(p) => {
                // C:\ -> C, \\server\share -> server/share
                for part in p.as_os_str().to_string_lossy().split(['\\', '/', ':', '?']) {
                    if !part.is_empty() && part != "." && part != "UNC" {
                        out.push(part);
                    }
                }
            }
            Component::Normal(n) => out.push(n),
            Component::RootDir | Component::CurDir | Component::ParentDir => {}
        }
    }
    PathBuf::from(format!("{}{SUFFIX}", out.display()))
}

/// The store folder from [`STORE_ENV`], if set.
pub fn store_from_env() -> Option<PathBuf> {
    std::env::var_os(STORE_ENV).filter(|s| !s.is_empty()).map(PathBuf::from)
}

/// Finds an existing sidecar: next to the file first, then in `store`. When there is none,
/// returns where a new one should go (next to the file unless a store is given).
pub fn locate(target: &Path, store: Option<&Path>) -> PathBuf {
    let beside = path_for(target, None);
    if beside.exists() {
        return beside;
    }
    match store {
        Some(s) => path_for(target, Some(s)),
        None => beside,
    }
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// What [`convert`] did with one file.
#[derive(Debug)]
pub enum Outcome {
    /// Properties were read and the sidecar written.
    Imported {
        count: usize,
        sidecar: PathBuf,
        properties: FileProperties,
    },
    /// The sidecar already holds a snapshot of the file as it is now.
    UpToDate {
        sidecar: PathBuf,
        properties: FileProperties,
    },
    /// Properties were read but, as asked, nothing was written.
    DryRun {
        count: usize,
        sidecar: PathBuf,
        properties: FileProperties,
    },
    Failed(String),
}

/// Reads `target`'s properties and records them in its sidecar, keeping any AnvilCAD
/// properties already there. `target` itself is only read.
pub fn convert(
    target: &Path,
    opts: &ReadOptions,
    store: Option<&Path>,
    force: bool,
    dry_run: bool,
) -> Outcome {
    let path = locate(target, store);
    let mut side = match Sidecar::load(&path) {
        Ok(s) => s.unwrap_or_default(),
        Err(e) => return Outcome::Failed(format!("{}: {e}", path.display())),
    };
    if !force && side.is_current(target) {
        if let Some(i) = side.imported.take() {
            return Outcome::UpToDate { sidecar: path, properties: i.properties };
        }
    }
    let stamp = match Stamp::of(target) {
        Ok(s) => s,
        Err(e) => return Outcome::Failed(e.to_string()),
    };
    let props = match super::read(target, opts) {
        Ok(p) => p,
        Err(e) => return Outcome::Failed(e.to_string()),
    };
    let count = props.count();
    if dry_run {
        return Outcome::DryRun { count, sidecar: path, properties: props };
    }
    side.imported = Some(Imported { read_at: now(), file: stamp, properties: props.clone() });
    match side.save(&path) {
        Ok(()) => Outcome::Imported { count, sidecar: path, properties: props },
        Err(e) => Outcome::Failed(format!("{}: {e}", path.display())),
    }
}

/// Where a row in a [`Sheet`] comes from.
#[derive(Clone, Debug, PartialEq)]
pub enum Origin {
    /// Read from the file just now.
    FileLive(Source),
    /// From the sidecar snapshot taken at `read_at`.
    FileSnapshot { source: Source, read_at: i64 },
    /// AnvilCAD's own property, kept in the sidecar.
    AnvilCad,
}

impl Origin {
    pub fn label(&self) -> String {
        match self {
            Origin::FileLive(s) => format!("File ({})", s.label()),
            Origin::FileSnapshot { read_at, .. } => {
                format!("File (converted {})", super::oleps::civil_date(read_at.div_euclid(86_400)))
            }
            Origin::AnvilCad => "AnvilCAD".into(),
        }
    }
    pub fn is_file(&self) -> bool {
        !matches!(self, Origin::AnvilCad)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub name: String,
    pub kind: PropertyKind,
    pub value: String,
    pub expression: Option<String>,
    /// Set for configuration-specific properties.
    pub configuration: Option<String>,
    pub origin: Origin,
    /// An AnvilCAD property whose name the file also uses. Both are shown; the file's own
    /// value is not replaced.
    pub conflict: bool,
}

/// Everything known about one file's properties, for display.
#[derive(Clone, Debug)]
pub struct Sheet {
    pub target: PathBuf,
    pub sidecar_path: PathBuf,
    pub sidecar: Sidecar,
    pub file: Option<(Origin, FileProperties)>,
    /// Why the file could not be read live, if it could not.
    pub live_error: Option<String>,
    /// The snapshot is older than the file.
    pub stale: bool,
    pub sidecar_error: Option<String>,
}

impl Sheet {
    pub fn open(target: &Path, opts: &ReadOptions, store: Option<&Path>) -> Sheet {
        let sidecar_path = locate(target, store);
        let (sidecar, sidecar_error) = match Sidecar::load(&sidecar_path) {
            Ok(s) => (s.unwrap_or_default(), None),
            Err(e) => (Sidecar::default(), Some(e.to_string())),
        };
        let stale = sidecar.imported.is_some() && !sidecar.is_current(target);
        let (file, live_error) = match super::read(target, opts) {
            Ok(p) => (Some((Origin::FileLive(p.source), p)), None),
            Err(e) => {
                let snap = sidecar.imported.as_ref().map(|i| {
                    (
                        Origin::FileSnapshot { source: i.properties.source, read_at: i.read_at },
                        i.properties.clone(),
                    )
                });
                (snap, Some(e.to_string()))
            }
        };
        Sheet { target: target.to_path_buf(), sidecar_path, sidecar, file, live_error, stale, sidecar_error }
    }

    /// File properties first (file level, then per configuration), then AnvilCAD's own.
    pub fn rows(&self) -> Vec<Row> {
        let mut rows = Vec::new();
        let row = |p: &FileProperty, cfg: Option<&str>, origin: &Origin| Row {
            name: p.name.clone(),
            kind: p.kind,
            value: p.value.clone(),
            expression: p.expression.clone(),
            configuration: cfg.map(str::to_string),
            origin: origin.clone(),
            conflict: false,
        };
        if let Some((origin, fp)) = &self.file {
            rows.extend(fp.properties.iter().map(|p| row(p, None, origin)));
            for c in &fp.configurations {
                rows.extend(c.properties.iter().map(|p| row(p, Some(&c.name), origin)));
            }
        }
        for p in &self.sidecar.properties {
            let conflict = self.file.as_ref().is_some_and(|(_, fp)| fp.get(&p.name).is_some());
            rows.push(Row {
                name: p.name.clone(),
                kind: p.kind,
                value: p.value.clone(),
                expression: None,
                configuration: None,
                origin: Origin::AnvilCad,
                conflict,
            });
        }
        rows
    }

    /// A file-level property's value: the file's own value wins over AnvilCAD's.
    pub fn value(&self, name: &str) -> Option<&str> {
        if let Some(p) = self.file.as_ref().and_then(|(_, fp)| fp.get(name)) {
            return Some(&p.value);
        }
        self.sidecar.properties.iter().find(|p| p.name.eq_ignore_ascii_case(name)).map(|p| p.value.as_str())
    }

    /// Saves AnvilCAD's own properties. A live read is recorded as the new snapshot, so the
    /// properties stay visible on computers that cannot read the file.
    pub fn save(&mut self) -> Result<(), SidecarError> {
        if let Some((Origin::FileLive(_), fp)) = &self.file {
            if let Ok(stamp) = Stamp::of(&self.target) {
                self.sidecar.imported =
                    Some(Imported { read_at: now(), file: stamp, properties: fp.clone() });
                self.stale = false;
            }
        }
        self.sidecar.save(&self.sidecar_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::props::cfb;
    use crate::props::oleps::{self, writer as ps};

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("anvil-props-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// A compound file with Description, Process and Finish as user-defined properties.
    pub fn wheel_bytes(description: &str) -> Vec<u8> {
        let user = ps::section(
            1252,
            &[(2, ps::V::Str(description)), (3, ps::V::Str("Lathe")), (4, ps::V::Str("galvanized"))],
            &[(2, "Description"), (3, "Process"), (4, "Finish")],
        );
        let doc = ps::stream(&[(oleps::FMTID_USER_DEFINED, user)]);
        cfb::writer::build(&[(oleps::DOC_SUMMARY_STREAM, &doc)])
    }

    #[test]
    fn opens_wheel_and_keeps_file_properties_separate() {
        let d = scratch("sheet");
        let wheel = d.join("wheel.sldprt");
        let bytes = wheel_bytes("large wheel");
        std::fs::write(&wheel, &bytes).unwrap();

        let mut sheet = Sheet::open(&wheel, &ReadOptions::default(), None);
        assert!(sheet.live_error.is_none());
        assert_eq!(sheet.value("Description"), Some("large wheel"));
        assert_eq!(sheet.value("process"), Some("Lathe"));
        assert_eq!(sheet.value("Finish"), Some("galvanized"));

        // AnvilCAD's own properties go in the sidecar; one reuses a file name.
        sheet.sidecar.properties.push(CustomProperty {
            name: "Inspection".into(),
            kind: PropertyKind::Text,
            value: "CMM".into(),
        });
        sheet.sidecar.properties.push(CustomProperty {
            name: "Finish".into(),
            kind: PropertyKind::Text,
            value: "powder coat".into(),
        });
        sheet.save().unwrap();
        assert_eq!(std::fs::read(&wheel).unwrap(), bytes, "the file itself is never written");

        let sheet = Sheet::open(&wheel, &ReadOptions::default(), None);
        let rows = sheet.rows();
        assert_eq!(rows.len(), 5);
        assert!(rows[..3].iter().all(|r| r.origin == Origin::FileLive(Source::OlePropertySet)));
        let ours: Vec<_> = rows.iter().filter(|r| r.origin == Origin::AnvilCad).collect();
        assert_eq!(ours.len(), 2);
        assert!(!ours[0].conflict && ours[1].conflict);
        // The file's own value is not taken over.
        assert_eq!(sheet.value("Finish"), Some("galvanized"));
        assert_eq!(sheet.value("Inspection"), Some("CMM"));
    }

    #[test]
    fn converted_snapshot_is_shown_when_the_file_cannot_be_read() {
        let d = scratch("snapshot");
        let wheel = d.join("wheel.sldprt");
        std::fs::write(&wheel, wheel_bytes("large wheel")).unwrap();
        let opts = ReadOptions::default();
        assert!(matches!(convert(&wheel, &opts, None, false, false), Outcome::Imported { count: 3, .. }));
        assert!(matches!(convert(&wheel, &opts, None, false, false), Outcome::UpToDate { .. }));

        // Stand in for a modern file this computer cannot read: same size, not a compound file.
        let len = std::fs::metadata(&wheel).unwrap().len() as usize;
        let mtime = std::fs::metadata(&wheel).unwrap().modified().unwrap();
        std::fs::write(&wheel, vec![0u8; len]).unwrap();
        let f = std::fs::File::options().write(true).open(&wheel).unwrap();
        f.set_modified(mtime).unwrap();
        drop(f);

        let sheet = Sheet::open(&wheel, &opts, None);
        assert!(sheet.live_error.is_some());
        assert!(!sheet.stale);
        assert!(matches!(sheet.file, Some((Origin::FileSnapshot { .. }, _))));
        assert_eq!(sheet.value("Description"), Some("large wheel"));

        // Once the file changes, the snapshot is flagged as stale but still shown.
        std::fs::write(&wheel, vec![0u8; len + 1]).unwrap();
        let sheet = Sheet::open(&wheel, &opts, None);
        assert!(sheet.stale);
        assert_eq!(sheet.value("Process"), Some("Lathe"));
        // Converting again fails (unreadable here) and leaves the old snapshot alone.
        assert!(matches!(convert(&wheel, &opts, None, false, false), Outcome::Failed(_)));
        assert_eq!(Sheet::open(&wheel, &opts, None).value("Finish"), Some("galvanized"));
    }

    #[test]
    fn conversion_keeps_anvilcad_properties_and_refreshes_the_snapshot() {
        let d = scratch("refresh");
        let wheel = d.join("wheel.sldprt");
        std::fs::write(&wheel, wheel_bytes("large wheel")).unwrap();
        let opts = ReadOptions::default();
        let mut sheet = Sheet::open(&wheel, &opts, None);
        sheet.sidecar.properties.push(CustomProperty {
            name: "Cell".into(),
            kind: PropertyKind::Text,
            value: "Lathe 3".into(),
        });
        sheet.save().unwrap();
        // Both versions pad to the same compound-file size, and two writes this close together
        // can share a modification time (as on Windows runners), so move the time on as an
        // edit made later would.
        let mtime = std::fs::metadata(&wheel).unwrap().modified().unwrap();
        std::fs::write(&wheel, wheel_bytes("larger wheel, rev B")).unwrap();
        let f = std::fs::File::options().write(true).open(&wheel).unwrap();
        f.set_modified(mtime + std::time::Duration::from_secs(2)).unwrap();
        drop(f);
        assert!(matches!(convert(&wheel, &opts, None, false, false), Outcome::Imported { .. }));
        let sheet = Sheet::open(&wheel, &opts, None);
        assert_eq!(sheet.value("Description"), Some("larger wheel, rev B"));
        assert_eq!(sheet.value("Cell"), Some("Lathe 3"));
    }

    #[test]
    fn store_paths_mirror_the_absolute_path() {
        let p = path_for(Path::new("/srv/eng/parts/wheel.sldprt"), Some(Path::new("/props")));
        if cfg!(windows) {
            assert!(p.to_string_lossy().ends_with("wheel.sldprt.anvilprops.json"));
        } else {
            assert_eq!(p, Path::new("/props/srv/eng/parts/wheel.sldprt.anvilprops.json"));
        }
        assert_eq!(path_for(Path::new("a/b.sldprt"), None), Path::new("a/b.sldprt.anvilprops.json"));
    }

    #[test]
    fn sidecar_never_overwrites_the_target() {
        let d = scratch("guard");
        let wheel = d.join("wheel.sldprt");
        std::fs::write(&wheel, b"original").unwrap();
        assert!(Sidecar::default().save(&wheel).is_err());
        assert_eq!(std::fs::read(&wheel).unwrap(), b"original");
    }
}
