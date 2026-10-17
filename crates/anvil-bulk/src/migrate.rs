//! The optional anvil-migrate plug-in.
//!
//! When an organisation still has SOLIDWORKS licences, `anvil-migrate` recovers each part's
//! feature tree by asking the organisation's own copy of SOLIDWORKS to open it through the
//! published API. This module runs it as a separate program, a batch of parts at a time so
//! SOLIDWORKS starts once per batch, and reads its `migration-report.json`. Nothing else
//! depends on it: without it, parts are converted from their STEP exports.

use crate::project::{Job, JobResult, Project, Status};
use crate::run::{self, RunOptions, RunSummary};
use std::collections::HashSet;
use std::path::Path;

/// anvil-migrate's per-file status, as written in its report.
fn status(s: &str) -> Status {
    match s {
        "Parametric" => Status::Parametric,
        "Hybrid" => Status::Hybrid,
        "Skipped" => Status::PropertiesOnly,
        _ => Status::Failed,
    }
}

fn tail(s: &str, n: usize) -> String {
    let t: Vec<char> = s.trim().chars().collect();
    t[t.len().saturating_sub(n)..].iter().collect()
}

/// Runs anvil-migrate on a batch and records a result for every job in it.
fn run_batch(project: &Project, batch: &[&Job], opts: &RunOptions, n: usize, summary: &mut RunSummary) {
    let safe: String = opts.worker.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect();
    let tmp = project.dir.join("work/tmp").join(format!("{safe}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let started = std::time::Instant::now();
    let output = std::process::Command::new(&opts.migrate_exe)
        .arg("-o")
        .arg(&tmp)
        .args(&opts.migrate_args)
        .args(batch.iter().map(|j| &j.path))
        .output();
    let seconds = started.elapsed().as_secs_f64() / batch.len().max(1) as f64;
    let (report, problem) = match &output {
        Ok(o) => {
            let text = std::fs::read_to_string(tmp.join("migration-report.json")).unwrap_or_default();
            let v: serde_json::Value = serde_json::from_str(&text).unwrap_or(serde_json::Value::Null);
            let why = format!(
                "anvil-migrate did not report on this file. {}",
                tail(&String::from_utf8_lossy(&o.stderr), 400)
            );
            (v, why)
        }
        Err(e) => (serde_json::Value::Null, format!("could not run {}: {e}", opts.migrate_exe.display())),
    };
    let files = report.get("files").and_then(|f| f.as_array()).cloned().unwrap_or_default();
    for job in batch {
        let mut r: JobResult = run::result_for(job, "anvil-migrate", &opts.worker);
        r.seconds = seconds;
        run::properties_into(Path::new(&job.path), opts, &mut r);
        match files.iter().find(|f| f.get("source").and_then(|s| s.as_str()) == Some(job.path.as_str())) {
            Some(f) => {
                r.status = status(f.get("status").and_then(|s| s.as_str()).unwrap_or(""));
                r.volume_mm3 = f.get("anvil_volume_mm3").and_then(|v| v.as_f64());
                if let Some(w) = f.get("warnings").and_then(|w| w.as_array()) {
                    r.messages.extend(w.iter().filter_map(|m| m.as_str().map(str::to_string)));
                }
                if r.status.has_part() {
                    let verified = f.get("verified").and_then(|v| v.as_bool()).unwrap_or(false);
                    if !verified {
                        r.messages.push("The volume was not verified against SOLIDWORKS.".into());
                    }
                    let from = f.get("output").and_then(|o| o.as_str()).map(std::path::PathBuf::from);
                    let to = project.part_path(&job.hash);
                    let moved = from.as_ref().is_some_and(|from| {
                        to.parent().is_some_and(|d| std::fs::create_dir_all(d).is_ok())
                            && (std::fs::rename(from, &to).is_ok() || std::fs::copy(from, &to).is_ok())
                    });
                    if !moved {
                        r.status = Status::Failed;
                        r.messages
                            .push("anvil-migrate reported a part but it could not be collected.".into());
                    }
                }
            }
            None => r.messages.push(problem.clone()),
        }
        r.finished = crate::now_secs();
        let _ = run::record(project, &r, summary);
    }
    let _ = std::fs::remove_dir_all(&tmp);
}

pub(crate) fn run_plugin(
    project: &Project,
    pending: &[&Job],
    opts: &RunOptions,
    limit: usize,
    summary: &mut RunSummary,
    progress: &(dyn Fn(&str) + Sync),
) {
    let mut taken = 0usize;
    let mut batches = 0usize;
    let mut batch: Vec<&Job> = Vec::new();
    let mut claims = Vec::new();
    let mut names: HashSet<String> = HashSet::new();
    let flush = |batch: &mut Vec<&Job>,
                 claims: &mut Vec<run::Claim>,
                 names: &mut HashSet<String>,
                 n: usize,
                 summary: &mut RunSummary| {
        if batch.is_empty() {
            return;
        }
        progress(&format!("anvil-migrate: {} part(s), batch {n}", batch.len()));
        run_batch(project, batch, opts, n, summary);
        batch.clear();
        claims.clear();
        names.clear();
    };
    for job in pending {
        if taken >= limit {
            break;
        }
        if !run::is_pending(job, project.result(&job.hash).as_ref(), opts.retry_failed) {
            continue;
        }
        // anvil-migrate names its output after the input file, so a batch holds each
        // file name once.
        let name =
            Path::new(&job.path).file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
        if run::plugin_takes(job) && names.contains(&name) {
            batches += 1;
            flush(&mut batch, &mut claims, &mut names, batches, summary);
        }
        let claim = match run::try_claim(project, &job.hash, &opts.worker, opts.claim_timeout) {
            Ok(Some(c)) => c,
            Ok(None) => {
                summary.busy += 1;
                continue;
            }
            Err(e) => {
                progress(&format!("cannot claim {}: {e}", job.path));
                continue;
            }
        };
        // Re-check with the claim held: another worker may have finished it meanwhile.
        if !run::is_pending(job, project.result(&job.hash).as_ref(), opts.retry_failed) {
            continue;
        }
        taken += 1;
        if !run::plugin_takes(job) {
            let s = run::record_native(project, job, opts, summary);
            progress(&format!("{}: {}", s.label(), job.path));
            drop(claim);
            continue;
        }
        names.insert(name);
        batch.push(job);
        claims.push(claim);
        if batch.len() >= opts.migrate_batch.max(1) {
            batches += 1;
            flush(&mut batch, &mut claims, &mut names, batches, summary);
        }
    }
    batches += 1;
    flush(&mut batch, &mut claims, &mut names, batches, summary);
}
