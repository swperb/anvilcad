//! The migration project folder and the records kept in it.
//!
//! ```text
//! project.json         the folders being migrated
//! inventory.jsonl      one line per SOLIDWORKS file found
//! jobs.jsonl           one line per distinct file content
//! work/claims/ab/<hash>.claim     a worker is converting this job
//! work/results/ab/<hash>.json     the job is finished
//! work/parts/ab/<hash>.anvprt     the converted part
//! reports/             summaries written by `report` and `export`
//! ```
//!
//! Every file is written to a temporary name and renamed into place, so readers on other
//! machines never see a half-written record.

use anvil_io::props::FileProperties;
use serde::{Deserialize, Serialize};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

pub const FORMAT: &str = "anvilcad-bulk-migration";
pub const FORMAT_VERSION: u32 = 1;

#[derive(Debug)]
pub enum Error {
    Io(PathBuf, std::io::Error),
    Data(PathBuf, String),
    NotAProject(PathBuf),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Io(p, e) => write!(f, "{}: {e}", p.display()),
            Error::Data(p, e) => write!(f, "{}: {e}", p.display()),
            Error::NotAProject(p) => {
                write!(f, "{} is not a migration project (run `anvil-bulk scan` first)", p.display())
            }
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

pub(crate) fn io<T>(path: &Path, r: std::io::Result<T>) -> Result<T> {
    r.map_err(|e| Error::Io(path.to_path_buf(), e))
}

/// A temporary name next to `path`, unique to this process and thread.
pub(crate) fn tmp_path(path: &Path) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    PathBuf::from(format!("{}.{}-{n}.tmp", path.display(), std::process::id()))
}

/// Writes a file atomically.
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(dir) = path.parent() {
        io(dir, std::fs::create_dir_all(dir))?;
    }
    let tmp = tmp_path(path);
    io(&tmp, std::fs::write(&tmp, bytes))?;
    io(path, std::fs::rename(&tmp, path))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Kind {
    Part,
    Assembly,
    Drawing,
    PartTemplate,
    AssemblyTemplate,
    DrawingTemplate,
}

impl Kind {
    pub fn of(path: &Path) -> Option<Kind> {
        use anvil_io::solidworks::SwDocType as T;
        Some(match anvil_io::solidworks::doc_type_from_extension(path)? {
            T::Part => Kind::Part,
            T::Assembly => Kind::Assembly,
            T::Drawing => Kind::Drawing,
            T::PartTemplate => Kind::PartTemplate,
            T::AssemblyTemplate => Kind::AssemblyTemplate,
            T::DrawingTemplate => Kind::DrawingTemplate,
        })
    }
    /// Parts and part templates carry geometry AnvilCAD converts.
    pub fn has_geometry(self) -> bool {
        matches!(self, Kind::Part | Kind::PartTemplate)
    }
    pub fn label(self) -> &'static str {
        match self {
            Kind::Part => "part",
            Kind::Assembly => "assembly",
            Kind::Drawing => "drawing",
            Kind::PartTemplate => "part template",
            Kind::AssemblyTemplate => "assembly template",
            Kind::DrawingTemplate => "drawing template",
        }
    }
}

/// One SOLIDWORKS file found by a scan.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub path: String,
    /// Index into the project's roots.
    pub root: usize,
    /// Path below the root, with `/` separators.
    pub rel: String,
    pub kind: Kind,
    pub size: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modified_ns: Option<i64>,
    /// SHA-256 of the file's contents, in hex.
    pub hash: String,
    /// A STEP export of this file, if one was found.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step: Option<String>,
}

impl Entry {
    pub fn modified_secs(&self) -> Option<i64> {
        self.modified_ns.map(|n| n.div_euclid(1_000_000_000))
    }
}

/// One distinct file content to convert.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Job {
    pub hash: String,
    pub kind: Kind,
    /// The first file (in path order) with this content; it is the one read.
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step: Option<String>,
    /// How many files share this content.
    pub copies: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Status {
    /// Geometry from STEP, every body exact.
    Exact,
    /// Geometry from STEP, some bodies as meshes.
    Faceted,
    /// Every feature rebuilt as an editable AnvilCAD feature (anvil-migrate plug-in).
    Parametric,
    /// Some features as imported geometry (anvil-migrate plug-in).
    Hybrid,
    /// An assembly or drawing: its properties were recorded; its contents are not
    /// migrated yet.
    PropertiesOnly,
    /// A part with no STEP export beside it and no plug-in to read its geometry.
    NoGeometrySource,
    Failed,
}

impl Status {
    pub const ALL: [Status; 7] = [
        Status::Exact,
        Status::Faceted,
        Status::Parametric,
        Status::Hybrid,
        Status::PropertiesOnly,
        Status::NoGeometrySource,
        Status::Failed,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Status::Exact => "exact",
            Status::Faceted => "faceted",
            Status::Parametric => "parametric",
            Status::Hybrid => "hybrid",
            Status::PropertiesOnly => "properties only",
            Status::NoGeometrySource => "no geometry source",
            Status::Failed => "failed",
        }
    }
    /// Whether a part file was produced.
    pub fn has_part(self) -> bool {
        matches!(self, Status::Exact | Status::Faceted | Status::Parametric | Status::Hybrid)
    }
}

/// What converting one job produced.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct JobResult {
    pub hash: String,
    pub status: Status,
    /// `step` or `anvil-migrate`.
    pub converter: String,
    /// The STEP file used, if any. A job whose STEP export appears later is redone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step: Option<String>,
    #[serde(default)]
    pub bodies: usize,
    #[serde(default)]
    pub exact_bodies: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub volume_mm3: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub properties: Option<FileProperties>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub properties_error: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub messages: Vec<String>,
    pub worker: String,
    pub seconds: f64,
    pub finished: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProjectFile {
    pub format: String,
    pub format_version: u32,
    pub roots: Vec<String>,
    #[serde(default)]
    pub step_roots: Vec<String>,
    pub created: i64,
}

/// A migration project folder.
#[derive(Clone, Debug)]
pub struct Project {
    pub dir: PathBuf,
}

fn shard(hash: &str) -> &str {
    hash.get(..2).unwrap_or("00")
}

impl Project {
    pub fn at(dir: impl Into<PathBuf>) -> Project {
        Project { dir: dir.into() }
    }

    pub fn file_path(&self) -> PathBuf {
        self.dir.join("project.json")
    }
    pub fn inventory_path(&self) -> PathBuf {
        self.dir.join("inventory.jsonl")
    }
    pub fn jobs_path(&self) -> PathBuf {
        self.dir.join("jobs.jsonl")
    }
    pub fn reports_dir(&self) -> PathBuf {
        self.dir.join("reports")
    }
    pub fn claim_path(&self, hash: &str) -> PathBuf {
        self.dir.join("work/claims").join(shard(hash)).join(format!("{hash}.claim"))
    }
    pub fn claims_dir(&self) -> PathBuf {
        self.dir.join("work/claims")
    }
    pub fn result_path(&self, hash: &str) -> PathBuf {
        self.dir.join("work/results").join(shard(hash)).join(format!("{hash}.json"))
    }
    pub fn part_path(&self, hash: &str) -> PathBuf {
        self.dir.join("work/parts").join(shard(hash)).join(format!("{hash}.{}", anvil_model::PART_EXT))
    }

    pub fn load(&self) -> Result<ProjectFile> {
        let p = self.file_path();
        if !p.exists() {
            return Err(Error::NotAProject(self.dir.clone()));
        }
        let text = io(&p, std::fs::read_to_string(&p))?;
        let pf: ProjectFile =
            serde_json::from_str(&text).map_err(|e| Error::Data(p.clone(), e.to_string()))?;
        if pf.format != FORMAT {
            return Err(Error::Data(p, format!("not a migration project (format '{}')", pf.format)));
        }
        if pf.format_version > FORMAT_VERSION {
            return Err(Error::Data(
                p,
                format!("format version {} is newer than this build", pf.format_version),
            ));
        }
        Ok(pf)
    }

    pub fn save(&self, pf: &ProjectFile) -> Result<()> {
        let json = serde_json::to_vec_pretty(pf).map_err(|e| Error::Data(self.file_path(), e.to_string()))?;
        write_atomic(&self.file_path(), &json)
    }

    pub fn inventory(&self) -> Result<Vec<Entry>> {
        read_lines(&self.inventory_path())
    }
    pub fn jobs(&self) -> Result<Vec<Job>> {
        read_lines(&self.jobs_path())
    }

    /// A finished job's result, if there is one. A damaged result counts as none, so the
    /// job is simply done again.
    pub fn result(&self, hash: &str) -> Option<JobResult> {
        let text = std::fs::read_to_string(self.result_path(hash)).ok()?;
        serde_json::from_str(&text).ok()
    }

    pub fn save_result(&self, r: &JobResult) -> Result<()> {
        let p = self.result_path(&r.hash);
        let json = serde_json::to_vec_pretty(r).map_err(|e| Error::Data(p.clone(), e.to_string()))?;
        write_atomic(&p, &json)
    }
}

/// Reads a JSON Lines file; a missing file is empty.
pub(crate) fn read_lines<T: serde::de::DeserializeOwned>(path: &Path) -> Result<Vec<T>> {
    let f = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(Error::Io(path.to_path_buf(), e)),
    };
    let mut out = Vec::new();
    for (n, line) in std::io::BufReader::new(f).lines().enumerate() {
        let line = io(path, line)?;
        if line.trim().is_empty() {
            continue;
        }
        let v = serde_json::from_str(&line)
            .map_err(|e| Error::Data(path.to_path_buf(), format!("line {}: {e}", n + 1)))?;
        out.push(v);
    }
    Ok(out)
}

pub(crate) fn write_lines<T: Serialize>(path: &Path, items: &[T]) -> Result<()> {
    if let Some(dir) = path.parent() {
        io(dir, std::fs::create_dir_all(dir))?;
    }
    let tmp = tmp_path(path);
    {
        let f = io(&tmp, std::fs::File::create(&tmp))?;
        let mut w = std::io::BufWriter::new(f);
        for it in items {
            let line =
                serde_json::to_string(it).map_err(|e| Error::Data(path.to_path_buf(), e.to_string()))?;
            io(&tmp, writeln!(w, "{line}"))?;
        }
        io(&tmp, w.flush())?;
    }
    io(path, std::fs::rename(&tmp, path))
}
