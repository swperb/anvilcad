//! Migration reports and progress.
//!
//! - `files.csv`: one row per original file: status, duplicates, geometry, properties, and
//!   where its migrated part is.
//! - `properties.csv`: every custom property of every file.
//! - `summary.csv`: counts by status for the whole migration, per year last modified, and
//!   per top-level folder.
//! - `summary.json`: the overall counts, for scripts.

use crate::export::{destination, root_labels};
use crate::project::{io, Job, JobResult, Project, Result, Status};
use std::collections::{BTreeMap, HashMap};
use std::path::Path;

/// Counts by status, plus pending, for one group of files.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub struct Counts {
    pub files: usize,
    pub unique: usize,
    pub by_status: BTreeMap<String, usize>,
    pub pending: usize,
}

impl Counts {
    fn add(&mut self, status: Option<Status>, first_copy: bool) {
        self.files += 1;
        if first_copy {
            self.unique += 1;
        }
        match status {
            Some(s) => *self.by_status.entry(s.label().to_string()).or_default() += 1,
            None => self.pending += 1,
        }
    }
    pub fn status(&self, s: Status) -> usize {
        self.by_status.get(s.label()).copied().unwrap_or(0)
    }
}

#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct ReportSummary {
    pub all: Counts,
    pub by_year: BTreeMap<String, Counts>,
    pub by_folder: BTreeMap<String, Counts>,
}

fn results(project: &Project, jobs: &[Job]) -> HashMap<String, JobResult> {
    jobs.iter().filter_map(|j| project.result(&j.hash).map(|r| (j.hash.clone(), r))).collect()
}

/// Writes the reports into the project's `reports` folder. With `export`, `files.csv` also
/// says where each migrated part was written.
pub fn report(project: &Project, export: Option<&Path>) -> Result<ReportSummary> {
    let pf = project.load()?;
    let labels = root_labels(&pf.roots);
    let inventory = project.inventory()?;
    let jobs = project.jobs()?;
    let results = results(project, &jobs);
    let first: HashMap<&str, &str> = jobs.iter().map(|j| (j.hash.as_str(), j.path.as_str())).collect();

    let mut sum = ReportSummary::default();
    let mut files = Vec::with_capacity(inventory.len());
    let mut props = Vec::new();
    for e in &inventory {
        let r = results.get(&e.hash);
        let status = r.map(|r| r.status);
        let original = first.get(e.hash.as_str()).copied().unwrap_or("");
        let is_first = original == e.path;
        let year = e
            .modified_secs()
            .and_then(|s| crate::date(s).get(..4).map(str::to_string))
            .unwrap_or_else(|| "unknown".into());
        let top = e.rel.split('/').next().filter(|_| e.rel.contains('/')).unwrap_or("");
        let folder = format!("{}/{top}", labels.get(e.root).map(String::as_str).unwrap_or("root"));
        sum.all.add(status, is_first);
        sum.by_year.entry(year).or_default().add(status, is_first);
        sum.by_folder.entry(folder).or_default().add(status, is_first);

        let output = match (export, r) {
            (Some(out), Some(r)) if r.status.has_part() => destination(out, &labels, e).display().to_string(),
            _ => String::new(),
        };
        let fp = r.and_then(|r| r.properties.as_ref());
        let message = r
            .map(|r| {
                let mut m = r.messages.clone();
                if let Some(pe) = &r.properties_error {
                    m.push(format!("properties: {pe}"));
                }
                m.join(" | ")
            })
            .unwrap_or_default();
        files.push(vec![
            e.path.clone(),
            e.kind.label().into(),
            e.size.to_string(),
            e.modified_secs().map(crate::date).unwrap_or_default(),
            e.hash.get(..16).unwrap_or("").into(),
            if is_first { String::new() } else { original.to_string() },
            status.map(|s| s.label()).unwrap_or("pending").into(),
            r.map(|r| r.converter.clone()).unwrap_or_default(),
            r.map(|r| r.bodies.to_string()).unwrap_or_default(),
            r.map(|r| r.exact_bodies.to_string()).unwrap_or_default(),
            r.and_then(|r| r.volume_mm3).map(|v| format!("{v:.3}")).unwrap_or_default(),
            fp.map(|p| p.count().to_string()).unwrap_or_default(),
            e.step.clone().unwrap_or_default(),
            output,
            message,
        ]);
        if let Some(fp) = fp {
            for p in &fp.properties {
                props.push(vec![
                    e.path.clone(),
                    String::new(),
                    p.name.clone(),
                    p.kind.label().into(),
                    p.value.clone(),
                    p.expression.clone().unwrap_or_default(),
                ]);
            }
            for c in &fp.configurations {
                for p in &c.properties {
                    props.push(vec![
                        e.path.clone(),
                        c.name.clone(),
                        p.name.clone(),
                        p.kind.label().into(),
                        p.value.clone(),
                        p.expression.clone().unwrap_or_default(),
                    ]);
                }
            }
        }
    }

    let dir = project.reports_dir();
    io(&dir, std::fs::create_dir_all(&dir))?;
    let p = dir.join("files.csv");
    io(
        &p,
        crate::write_csv(
            &p,
            &[
                "path",
                "type",
                "bytes",
                "modified",
                "sha256",
                "duplicate_of",
                "status",
                "converter",
                "bodies",
                "exact_bodies",
                "volume_mm3",
                "properties",
                "step_file",
                "migrated_part",
                "messages",
            ],
            &files,
        ),
    )?;
    let p = dir.join("properties.csv");
    io(
        &p,
        crate::write_csv(&p, &["path", "configuration", "property", "type", "value", "expression"], &props),
    )?;

    let mut header = vec!["group", "key", "files", "unique"];
    header.extend(Status::ALL.iter().map(|s| s.label()));
    header.push("pending");
    let row = |group: &str, key: &str, c: &Counts| {
        let mut r = vec![group.to_string(), key.to_string(), c.files.to_string(), c.unique.to_string()];
        r.extend(Status::ALL.iter().map(|s| c.status(*s).to_string()));
        r.push(c.pending.to_string());
        r
    };
    let mut rows = vec![row("all", "", &sum.all)];
    rows.extend(sum.by_year.iter().map(|(k, c)| row("year modified", k, c)));
    rows.extend(sum.by_folder.iter().map(|(k, c)| row("folder", k, c)));
    let p = dir.join("summary.csv");
    io(&p, crate::write_csv(&p, &header, &rows))?;
    let json = serde_json::to_vec_pretty(&sum).unwrap_or_default();
    crate::project::write_atomic(&dir.join("summary.json"), &json)?;
    Ok(sum)
}

/// How far a migration has got, for `anvil-bulk status`.
#[derive(Clone, Debug, Default)]
pub struct Progress {
    pub jobs: usize,
    pub done: BTreeMap<String, usize>,
    pub pending: usize,
    /// Claim files present: jobs being converted now (or by workers that stopped).
    pub claimed: usize,
    /// Mean seconds per finished job.
    pub mean_seconds: Option<f64>,
}

impl Progress {
    /// Estimated time left with `workers` running, in seconds.
    pub fn eta(&self, workers: usize) -> Option<f64> {
        self.mean_seconds.map(|m| m * self.pending as f64 / workers.max(1) as f64)
    }
}

fn count_claims(dir: &Path) -> usize {
    let Ok(rd) = std::fs::read_dir(dir) else { return 0 };
    rd.filter_map(|e| e.ok())
        .map(|e| {
            let p = e.path();
            if p.is_dir() {
                count_claims(&p)
            } else {
                usize::from(p.extension().is_some_and(|x| x == "claim"))
            }
        })
        .sum()
}

pub fn progress(project: &Project, retry_failed: bool) -> Result<Progress> {
    project.load()?;
    let jobs = project.jobs()?;
    let mut p = Progress { jobs: jobs.len(), ..Progress::default() };
    let (mut secs, mut n) = (0.0, 0usize);
    for j in &jobs {
        let r = project.result(&j.hash);
        if crate::run::is_pending(j, r.as_ref(), retry_failed) {
            p.pending += 1;
        }
        if let Some(r) = r {
            *p.done.entry(r.status.label().to_string()).or_default() += 1;
            secs += r.seconds;
            n += 1;
        }
    }
    p.mean_seconds = (n > 0).then(|| secs / n as f64);
    p.claimed = count_claims(&project.claims_dir());
    Ok(p)
}
