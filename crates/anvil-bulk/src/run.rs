//! Converting jobs, by any number of workers on any number of machines.
//!
//! A worker claims a job by creating its claim file with create-new semantics, which only
//! one creator can win, also on SMB and NFS shares. When the job is done the worker writes
//! the result and removes the claim. A claim older than the timeout belongs to a worker
//! that stopped; another worker takes it over by renaming it away (only one rename can
//! succeed) and claiming afresh. A job with a result is never redone, unless its STEP
//! export has changed or failures are retried.

use crate::migrate;
use crate::project::{Job, JobResult, Kind, Project, Result, Status};
use anvil_io::props::{sidecar::Sheet, ReadOptions};
use anvil_model::document::CustomProperty;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant, SystemTime};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Converter {
    /// Geometry from STEP exports; no SOLIDWORKS needed.
    Step,
    /// The anvil-migrate plug-in, driving the organisation's own SOLIDWORKS (Windows).
    Migrate,
}

#[derive(Clone, Debug)]
pub struct RunOptions {
    pub worker: String,
    pub threads: usize,
    pub converter: Converter,
    pub read: ReadOptions,
    /// Where `anvil-props` keeps property snapshots, if not beside the files.
    pub store: Option<PathBuf>,
    pub claim_timeout: Duration,
    pub retry_failed: bool,
    /// Stop after this many jobs (this worker).
    pub limit: Option<usize>,
    /// The anvil-migrate program and how many parts to give it at a time.
    pub migrate_exe: PathBuf,
    pub migrate_batch: usize,
    pub migrate_args: Vec<String>,
}

impl RunOptions {
    pub fn new(worker: impl Into<String>) -> RunOptions {
        RunOptions {
            worker: worker.into(),
            threads: 1,
            converter: Converter::Step,
            read: ReadOptions::default(),
            store: None,
            claim_timeout: Duration::from_secs(4 * 3600),
            retry_failed: false,
            limit: None,
            migrate_exe: PathBuf::from("anvil-migrate"),
            migrate_batch: 20,
            migrate_args: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct RunSummary {
    /// Jobs this worker finished, by status.
    pub done: Vec<(Status, usize)>,
    /// Jobs that were pending when the run started.
    pub pending_at_start: usize,
    /// Jobs skipped because another worker held them.
    pub busy: usize,
}

impl RunSummary {
    pub fn total(&self) -> usize {
        self.done.iter().map(|(_, n)| n).sum()
    }
    fn add(&mut self, s: Status) {
        match self.done.iter_mut().find(|(k, _)| *k == s) {
            Some((_, n)) => *n += 1,
            None => self.done.push((s, 1)),
        }
    }
}

/// Whether a job still needs converting.
pub fn is_pending(job: &Job, result: Option<&JobResult>, retry_failed: bool) -> bool {
    match result {
        None => true,
        Some(r) if r.step != job.step && job.kind.has_geometry() => true,
        Some(r) => retry_failed && r.status == Status::Failed,
    }
}

/// A held claim; the claim file is removed when it is dropped.
pub struct Claim {
    path: PathBuf,
}

impl Drop for Claim {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Tries to claim a job. `Ok(None)` when another worker holds a fresh claim.
pub fn try_claim(
    project: &Project,
    hash: &str,
    worker: &str,
    timeout: Duration,
) -> std::io::Result<Option<Claim>> {
    let path = project.claim_path(hash);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    for attempt in 0..2 {
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut f) => {
                use std::io::Write;
                let _ = writeln!(f, "{{\"worker\": {:?}, \"started\": {}}}", worker, crate::now_secs());
                return Ok(Some(Claim { path }));
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && attempt == 0 => {
                let age = std::fs::metadata(&path)
                    .and_then(|m| m.modified())
                    .ok()
                    .and_then(|t| SystemTime::now().duration_since(t).ok());
                if age.is_none_or(|a| a < timeout) {
                    return Ok(None);
                }
                // Stale: take it over. Only one worker's rename of the old claim succeeds.
                let aside = crate::project::tmp_path(&path);
                if std::fs::rename(&path, &aside).is_err() {
                    return Ok(None);
                }
                let _ = std::fs::remove_file(&aside);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => return Ok(None),
            Err(e) => return Err(e),
        }
    }
    Ok(None)
}

/// Reads a file's custom properties: live from the file, or from an anvil-props snapshot.
fn read_properties(path: &Path, opts: &RunOptions, r: &mut JobResult) {
    let sheet = Sheet::open(path, &opts.read, opts.store.as_deref());
    match sheet.file {
        Some((_, fp)) => {
            if sheet.stale {
                r.messages.push("Properties come from a snapshot older than the file.".into());
            }
            r.properties = Some(fp);
        }
        None => r.properties_error = sheet.live_error,
    }
}

fn new_result(job: &Job, converter: &str, worker: &str) -> JobResult {
    JobResult {
        hash: job.hash.clone(),
        status: Status::Failed,
        converter: converter.into(),
        step: job.step.clone(),
        bodies: 0,
        exact_bodies: 0,
        volume_mm3: None,
        properties: None,
        properties_error: None,
        messages: Vec::new(),
        worker: worker.into(),
        seconds: 0.0,
        finished: 0,
    }
}

/// The file's own custom properties as the migrated part's custom properties.
pub(crate) fn custom_properties(r: &JobResult) -> Option<Vec<CustomProperty>> {
    let fp = r.properties.as_ref()?;
    Some(
        fp.properties
            .iter()
            .map(|p| CustomProperty { name: p.name.clone(), kind: p.kind, value: p.value.clone() })
            .collect(),
    )
}

/// Converts one job natively: properties, and geometry from its STEP export.
pub fn convert_native(project: &Project, job: &Job, opts: &RunOptions) -> JobResult {
    let started = Instant::now();
    let mut r = new_result(job, "step", &opts.worker);
    read_properties(Path::new(&job.path), opts, &mut r);
    if !job.kind.has_geometry() {
        r.status = Status::PropertiesOnly;
        r.messages.push(format!(
            "The {} was recorded with its properties; {}s are not migrated yet.",
            job.kind.label(),
            job.kind.label()
        ));
    } else if let Some(step) = &job.step {
        // A STEP file that trips an internal error must not stop a run of thousands.
        let out =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| step_part(project, job, step, &mut r)));
        if let Err(e) = out.unwrap_or_else(|_| Err("internal error while importing the STEP file".into())) {
            r.status = Status::Failed;
            r.messages.push(e);
        }
    } else {
        r.status = Status::NoGeometrySource;
        r.messages.push(
            "No STEP export was found beside this part or in the STEP folders. Export STEP from \
             SOLIDWORKS, or use the anvil-migrate plug-in, and run again."
                .into(),
        );
    }
    r.seconds = started.elapsed().as_secs_f64();
    r.finished = crate::now_secs();
    r
}

fn step_part(project: &Project, job: &Job, step: &str, r: &mut JobResult) -> std::result::Result<(), String> {
    let bytes = std::fs::read(step).map_err(|e| format!("{step}: {e}"))?;
    let text = String::from_utf8_lossy(&bytes);
    let name = Path::new(step).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let (mut doc, report) = anvil_model::step::document_from_step(&text, &name)?;
    if report.bodies == 0 {
        return Err("the STEP file holds no solid bodies".into());
    }
    if let Some(props) = custom_properties(r) {
        doc.custom_properties = props;
    }
    let st = doc.rebuild();
    if !st.errors.is_empty() {
        r.messages.extend(st.errors.values().map(|e| format!("rebuild: {e}")));
    }
    r.bodies = report.bodies;
    r.exact_bodies = report.exact;
    r.volume_mm3 = Some(st.body.mass_properties().volume);
    r.messages.extend(report.meshed.iter().cloned());
    r.messages.extend(report.warnings.iter().take(20).cloned());
    r.status = if report.exact == report.bodies { Status::Exact } else { Status::Faceted };
    let out = project.part_path(&job.hash);
    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    doc.save(&out).map_err(|e| format!("{}: {e}", out.display()))
}

/// Runs this worker until no claimable job is left (or the limit is reached).
pub fn run(project: &Project, opts: &RunOptions, progress: &(dyn Fn(&str) + Sync)) -> Result<RunSummary> {
    project.load()?;
    let jobs = project.jobs()?;
    let pending: Vec<&Job> =
        jobs.iter().filter(|j| is_pending(j, project.result(&j.hash).as_ref(), opts.retry_failed)).collect();
    let mut summary = RunSummary { pending_at_start: pending.len(), ..RunSummary::default() };
    let limit = opts.limit.unwrap_or(usize::MAX);
    match opts.converter {
        Converter::Step => run_native(project, &pending, opts, limit, &mut summary, progress),
        Converter::Migrate => migrate::run_plugin(project, &pending, opts, limit, &mut summary, progress),
    }
    Ok(summary)
}

fn run_native(
    project: &Project,
    pending: &[&Job],
    opts: &RunOptions,
    limit: usize,
    summary: &mut RunSummary,
    progress: &(dyn Fn(&str) + Sync),
) {
    let next = AtomicUsize::new(0);
    let taken = AtomicUsize::new(0);
    let outcomes = std::sync::Mutex::new(Vec::new());
    let busy = AtomicUsize::new(0);
    std::thread::scope(|s| {
        for _ in 0..opts.threads.max(1) {
            s.spawn(|| loop {
                let k = next.fetch_add(1, Ordering::Relaxed);
                let Some(job) = pending.get(k) else { break };
                // Another worker may have finished it since the run started.
                if !is_pending(job, project.result(&job.hash).as_ref(), opts.retry_failed) {
                    continue;
                }
                let claim = match try_claim(project, &job.hash, &opts.worker, opts.claim_timeout) {
                    Ok(Some(c)) => c,
                    Ok(None) => {
                        busy.fetch_add(1, Ordering::Relaxed);
                        continue;
                    }
                    Err(e) => {
                        progress(&format!("cannot claim {}: {e}", job.path));
                        continue;
                    }
                };
                if taken.fetch_add(1, Ordering::Relaxed) >= limit {
                    break;
                }
                let r = convert_native(project, job, opts);
                let status = r.status;
                match project.save_result(&r) {
                    Ok(()) => {
                        progress(&format!("{}: {}", status.label(), job.path));
                        if let Ok(mut o) = outcomes.lock() {
                            o.push(status);
                        }
                    }
                    Err(e) => progress(&format!("cannot record the result for {}: {e}", job.path)),
                }
                drop(claim);
            });
        }
    });
    for s in outcomes.into_inner().unwrap_or_else(|e| e.into_inner()) {
        summary.add(s);
    }
    summary.busy = busy.into_inner();
}

/// Converts one job natively and records it; used by the plug-in for non-part files.
pub(crate) fn record_native(
    project: &Project,
    job: &Job,
    opts: &RunOptions,
    summary: &mut RunSummary,
) -> Status {
    let r = convert_native(project, job, opts);
    let s = r.status;
    if project.save_result(&r).is_ok() {
        summary.add(s);
    }
    s
}

pub(crate) fn record(project: &Project, r: &JobResult, summary: &mut RunSummary) -> Result<()> {
    project.save_result(r)?;
    summary.add(r.status);
    Ok(())
}

pub(crate) fn result_for(job: &Job, converter: &str, worker: &str) -> JobResult {
    new_result(job, converter, worker)
}

pub(crate) fn properties_into(path: &Path, opts: &RunOptions, r: &mut JobResult) {
    read_properties(path, opts, r)
}

/// Parts the plug-in should take.
pub(crate) fn plugin_takes(job: &Job) -> bool {
    job.kind == Kind::Part
}
