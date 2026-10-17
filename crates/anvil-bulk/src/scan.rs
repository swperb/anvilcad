//! Inventory: find every SOLIDWORKS file, hash it, pair it with its STEP export, and make
//! one job per distinct content.

use crate::project::{self, io, Entry, Job, Kind, Project, ProjectFile, Result};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

#[derive(Clone, Debug, Default)]
pub struct ScanOptions {
    /// Folders (or single files) to migrate. Empty: the project's saved roots.
    pub roots: Vec<PathBuf>,
    /// Folders holding STEP exports that mirror the roots' folder structure.
    pub step_roots: Vec<PathBuf>,
    /// Files hashed at once. Hashing is limited by disk and network speed.
    pub threads: usize,
}

#[derive(Clone, Debug, Default)]
pub struct ScanSummary {
    pub files: usize,
    pub jobs: usize,
    /// Files whose hash was computed this time (the rest were unchanged since last scan).
    pub hashed: usize,
    pub bytes_hashed: u64,
    pub with_step: usize,
    /// Folders or files that could not be read, and why.
    pub problems: Vec<(String, String)>,
}

fn ext_lower(p: &Path) -> String {
    p.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase()
}

fn is_step(p: &Path) -> bool {
    matches!(ext_lower(p).as_str(), "step" | "stp")
}

/// The path without its extension, lower-cased, as a key for pairing files.
fn stem_key(p: &str) -> String {
    let p = p.replace('\\', "/");
    let cut = match (p.rfind('.'), p.rfind('/')) {
        (Some(dot), Some(slash)) if dot > slash => dot,
        (Some(dot), None) => dot,
        _ => p.len(),
    };
    p[..cut].to_lowercase()
}

fn rel_string(root: &Path, p: &Path) -> String {
    let rel = p.strip_prefix(root).unwrap_or(p);
    let parts: Vec<String> = rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
    if parts.is_empty() {
        p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
    } else {
        parts.join("/")
    }
}

struct Found {
    path: PathBuf,
    root: usize,
    rel: String,
    kind: Kind,
}

/// Walks a root. Symbolic links to folders are not followed, so a link cycle on a share
/// cannot loop; unreadable folders are recorded and skipped.
fn walk(dir: &Path, f: &mut dyn FnMut(&Path), problems: &mut Vec<(String, String)>) {
    let rd = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(e) => {
            problems.push((dir.display().to_string(), e.to_string()));
            return;
        }
    };
    let mut entries: Vec<_> = rd.filter_map(|e| e.ok()).collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let p = e.path();
        match e.file_type() {
            Ok(t) if t.is_dir() => walk(&p, f, problems),
            Ok(t) if t.is_file() || (t.is_symlink() && p.is_file()) => f(&p),
            _ => {}
        }
    }
}

/// A file's SHA-256 in hex and its length, or why it could not be read.
type Hashed = std::io::Result<(String, u64)>;

fn is_lock_file(p: &Path) -> bool {
    p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("~$"))
}

fn hash_file(p: &Path) -> std::io::Result<(String, u64)> {
    let mut f = std::fs::File::open(p)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    let mut total = 0u64;
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        total += n as u64;
        h.update(&buf[..n]);
    }
    let hex: String = h.finalize().iter().map(|b| format!("{b:02x}")).collect();
    Ok((hex, total))
}

fn modified_ns(m: &std::fs::Metadata) -> Option<i64> {
    m.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok().map(|d| d.as_nanos() as i64)
}

/// Scans the roots into the project, reusing hashes of files unchanged since the last scan.
/// `progress` receives occasional status lines.
pub fn scan(project: &Project, opts: &ScanOptions, progress: &(dyn Fn(&str) + Sync)) -> Result<ScanSummary> {
    let saved = project.load().ok();
    let roots: Vec<PathBuf> = if opts.roots.is_empty() {
        saved.as_ref().map(|s| s.roots.iter().map(PathBuf::from).collect()).unwrap_or_default()
    } else {
        opts.roots.iter().map(|r| std::path::absolute(r).unwrap_or_else(|_| r.clone())).collect()
    };
    let step_roots: Vec<PathBuf> = if opts.step_roots.is_empty() {
        saved.as_ref().map(|s| s.step_roots.iter().map(PathBuf::from).collect()).unwrap_or_default()
    } else {
        opts.step_roots.iter().map(|r| std::path::absolute(r).unwrap_or_else(|_| r.clone())).collect()
    };
    io(&project.dir, std::fs::create_dir_all(&project.dir))?;
    let mut summary = ScanSummary::default();

    // Walk the roots, collecting SOLIDWORKS files and STEP files that sit beside them.
    let mut found: Vec<Found> = Vec::new();
    let mut beside: HashMap<String, PathBuf> = HashMap::new();
    for (ri, root) in roots.iter().enumerate() {
        let mut visit = |p: &Path| {
            if is_lock_file(p) {
                return;
            }
            if let Some(kind) = Kind::of(p) {
                found.push(Found { path: p.to_path_buf(), root: ri, rel: rel_string(root, p), kind });
            } else if is_step(p) {
                beside.entry(stem_key(&p.to_string_lossy())).or_insert_with(|| p.to_path_buf());
            }
        };
        match std::fs::symlink_metadata(root) {
            Ok(m) if m.is_dir() => walk(root, &mut visit, &mut summary.problems),
            Ok(_) => visit(root),
            Err(e) => summary.problems.push((root.display().to_string(), e.to_string())),
        }
        progress(&format!("{}: {} SOLIDWORKS files so far", root.display(), found.len()));
    }
    // STEP exports kept in a separate tree that mirrors the roots.
    let mut mirrored: HashMap<String, PathBuf> = HashMap::new();
    for sr in &step_roots {
        let mut visit = |p: &Path| {
            if is_step(p) {
                mirrored.entry(stem_key(&rel_string(sr, p))).or_insert_with(|| p.to_path_buf());
            }
        };
        walk(sr, &mut visit, &mut summary.problems);
    }

    // Hash, reusing the previous inventory for files whose size and time are unchanged.
    let previous: HashMap<String, Entry> =
        project.inventory().unwrap_or_default().into_iter().map(|e| (e.path.clone(), e)).collect();
    let mut entries: Vec<Option<Entry>> = Vec::with_capacity(found.len());
    let mut to_hash: Vec<usize> = Vec::new();
    for (i, f) in found.iter().enumerate() {
        let path = f.path.display().to_string();
        let meta = match std::fs::metadata(&f.path) {
            Ok(m) => m,
            Err(e) => {
                summary.problems.push((path, e.to_string()));
                entries.push(None);
                continue;
            }
        };
        let mns = modified_ns(&meta);
        let step = beside
            .get(&stem_key(&path))
            .or_else(|| mirrored.get(&stem_key(&f.rel)))
            .map(|p| p.display().to_string());
        let reuse =
            previous.get(&path).filter(|p| p.size == meta.len() && mns.is_some() && p.modified_ns == mns);
        let hash = reuse.map(|p| p.hash.clone()).unwrap_or_default();
        if hash.is_empty() {
            to_hash.push(i);
        }
        entries.push(Some(Entry {
            path,
            root: f.root,
            rel: f.rel.clone(),
            kind: f.kind,
            size: meta.len(),
            modified_ns: mns,
            hash,
            step,
        }));
    }

    let next = AtomicUsize::new(0);
    let done = AtomicUsize::new(0);
    let out: Mutex<Vec<(usize, Hashed)>> = Mutex::new(Vec::new());
    let threads = opts.threads.max(1).min(to_hash.len().max(1));
    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| loop {
                let k = next.fetch_add(1, Ordering::Relaxed);
                let Some(&i) = to_hash.get(k) else { break };
                let r = hash_file(&found[i].path);
                if let Ok(mut v) = out.lock() {
                    v.push((i, r));
                }
                let d = done.fetch_add(1, Ordering::Relaxed) + 1;
                if d.is_multiple_of(1000) {
                    progress(&format!("hashed {d} of {} files", to_hash.len()));
                }
            });
        }
    });
    let hashed = out.into_inner().unwrap_or_else(|e| e.into_inner());
    for (i, r) in hashed {
        match r {
            Ok((h, n)) => {
                if let Some(e) = &mut entries[i] {
                    e.hash = h;
                    summary.hashed += 1;
                    summary.bytes_hashed += n;
                }
            }
            Err(e) => {
                summary.problems.push((found[i].path.display().to_string(), e.to_string()));
                entries[i] = None;
            }
        }
    }
    let mut entries: Vec<Entry> = entries.into_iter().flatten().filter(|e| !e.hash.is_empty()).collect();
    entries.sort_by(|a, b| a.path.cmp(&b.path));

    // One job per distinct content: the first file in path order is the one read.
    let mut by_hash: BTreeMap<&str, Job> = BTreeMap::new();
    for e in &entries {
        let j = by_hash.entry(&e.hash).or_insert_with(|| Job {
            hash: e.hash.clone(),
            kind: e.kind,
            path: e.path.clone(),
            step: None,
            copies: 0,
        });
        j.copies += 1;
        if j.step.is_none() {
            j.step = e.step.clone();
        }
    }
    let mut jobs: Vec<Job> = by_hash.into_values().collect();
    // Parts first: they carry the geometry, and later work (assemblies) will build on them.
    jobs.sort_by(|a, b| (!a.kind.has_geometry(), &a.path).cmp(&(!b.kind.has_geometry(), &b.path)));

    summary.files = entries.len();
    summary.jobs = jobs.len();
    summary.with_step = entries.iter().filter(|e| e.step.is_some()).count();

    project::write_lines(&project.inventory_path(), &entries)?;
    project::write_lines(&project.jobs_path(), &jobs)?;
    project.save(&ProjectFile {
        format: project::FORMAT.into(),
        format_version: project::FORMAT_VERSION,
        roots: roots.iter().map(|r| r.display().to_string()).collect(),
        step_roots: step_roots.iter().map(|r| r.display().to_string()).collect(),
        created: saved.map(|s| s.created).unwrap_or_else(crate::now_secs),
    })?;
    let rows: Vec<Vec<String>> = summary.problems.iter().map(|(p, m)| vec![p.clone(), m.clone()]).collect();
    let dir = project.reports_dir();
    io(&dir, std::fs::create_dir_all(&dir))?;
    let p = dir.join("scan-problems.csv");
    io(&p, crate::write_csv(&p, &["path", "problem"], &rows))?;
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stem_keys() {
        assert_eq!(stem_key("/a/B/Wheel.SLDPRT"), "/a/b/wheel");
        assert_eq!(stem_key("C:\\x\\wheel.step"), "c:/x/wheel");
        assert_eq!(stem_key("dir.v2/wheel"), "dir.v2/wheel");
    }
}
