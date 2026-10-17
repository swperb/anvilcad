//! anvil-bulk: migrates an organisation's SOLIDWORKS files to AnvilCAD, without SOLIDWORKS.

use anvil_bulk::run::{Converter, RunOptions};
use anvil_bulk::scan::ScanOptions;
use anvil_bulk::Project;
use anvil_io::props::ReadOptions;
use std::path::PathBuf;
use std::time::Duration;

const USAGE: &str = "\
Usage:
  anvil-bulk scan   --project <DIR> [--step-root <DIR>]... [--threads <N>] <INPUT>...
  anvil-bulk run    --project <DIR> [OPTIONS]
  anvil-bulk status --project <DIR> [--workers <N>]
  anvil-bulk export --project <DIR> --out <DIR> [--overwrite]
  anvil-bulk report --project <DIR>

Migrates an organisation's SOLIDWORKS files to AnvilCAD. The original files are only read.

scan    Finds every SOLIDWORKS file under INPUT (folders, network shares such as
        \\\\server\\eng or /mnt/eng, folders synced by cloud storage clients), hashes it, and
        pairs each part with a STEP export of it: <name>.step or .stp beside it, or at the
        same relative path under a --step-root. Copies of the same file become one job.
        Run it again after adding files or STEP exports; unchanged files are not re-read.
run     Converts jobs. Start it on as many machines as you like, all pointing at the same
        project folder on a share; each job is done once. Stop it at any time and run it
        again to carry on.
status  Shows progress and an estimate of the time left.
export  Writes the migrated tree under --out: one .anvprt per original part, at the same
        relative path, with the file's custom properties (and any AnvilCAD properties
        added with anvil-props). Then writes the reports.
report  Writes the reports to <project>/reports: files.csv, properties.csv, summary.csv,
        summary.json.

run options:
      --worker <NAME>       Name recorded with results (default: computer name and process)
      --threads <N>         Jobs converted at once on this machine (default: CPU count)
      --key-file <FILE>     SOLIDWORKS Document Manager key, optional (default: ANVIL_SWDM_KEY)
                            for properties of files saved by SOLIDWORKS 2015 or later (Windows)
      --store <DIR>         Folder of anvil-props snapshots (default: ANVIL_PROPS_STORE)
      --retry-failed        Convert failed jobs again
      --limit <N>           Stop after N jobs
      --claim-timeout <MIN> Take over jobs claimed longer ago than this (default: 240)
      --with-anvil-migrate  Optional plug-in: convert parts with anvil-migrate instead, which
                            keeps feature trees but needs SOLIDWORKS on this machine
      --migrate-exe <PATH>  The anvil-migrate program (default: anvil-migrate on PATH)
      --migrate-batch <N>   Parts per anvil-migrate run (default: 20)";

struct Args {
    command: String,
    project: Option<PathBuf>,
    inputs: Vec<PathBuf>,
    step_roots: Vec<PathBuf>,
    out: Option<PathBuf>,
    overwrite: bool,
    workers: usize,
    run: RunOptions,
    key_file: Option<PathBuf>,
}

fn default_worker() -> String {
    let host = std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "worker".into());
    format!("{host}-{}", std::process::id())
}

fn threads() -> usize {
    std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4)
}

fn parse() -> Result<Args, String> {
    let mut it = std::env::args().skip(1);
    let command = match it.next() {
        Some(c) if ["scan", "run", "status", "export", "report"].contains(&c.as_str()) => c,
        Some(c) if c == "-h" || c == "--help" => return Err(String::new()),
        Some(c) => return Err(format!("unknown command '{c}'")),
        None => return Err(String::new()),
    };
    let mut run = RunOptions::new(default_worker());
    run.threads = threads();
    run.store = anvil_io::props::sidecar::store_from_env();
    let mut a = Args {
        command,
        project: None,
        inputs: Vec::new(),
        step_roots: Vec::new(),
        out: None,
        overwrite: false,
        workers: 1,
        run,
        key_file: None,
    };
    let mut scan_threads = None;
    while let Some(s) = it.next() {
        let mut val = |n: &str| it.next().ok_or_else(|| format!("{n} needs a value"));
        let num = |v: String, n: &str| v.parse::<usize>().map_err(|_| format!("{n} needs a number"));
        match s.as_str() {
            "--project" => a.project = Some(val("--project")?.into()),
            "--step-root" => a.step_roots.push(val("--step-root")?.into()),
            "--out" | "-o" => a.out = Some(val("--out")?.into()),
            "--overwrite" => a.overwrite = true,
            "--workers" => a.workers = num(val("--workers")?, "--workers")?,
            "--threads" => scan_threads = Some(num(val("--threads")?, "--threads")?),
            "--worker" => a.run.worker = val("--worker")?,
            "--key-file" => a.key_file = Some(val("--key-file")?.into()),
            "--store" => a.run.store = Some(val("--store")?.into()),
            "--retry-failed" => a.run.retry_failed = true,
            "--limit" => a.run.limit = Some(num(val("--limit")?, "--limit")?),
            "--claim-timeout" => {
                a.run.claim_timeout =
                    Duration::from_secs(60 * num(val("--claim-timeout")?, "--claim-timeout")? as u64)
            }
            "--with-anvil-migrate" => a.run.converter = Converter::Migrate,
            "--migrate-exe" => a.run.migrate_exe = val("--migrate-exe")?.into(),
            "--migrate-batch" => a.run.migrate_batch = num(val("--migrate-batch")?, "--migrate-batch")?,
            "-h" | "--help" => return Err(String::new()),
            o if o.starts_with('-') && o.len() > 1 => return Err(format!("unknown option '{o}'")),
            _ => a.inputs.push(s.into()),
        }
    }
    if let Some(t) = scan_threads {
        a.run.threads = t.max(1);
    }
    if a.project.is_none() {
        return Err("--project is required".into());
    }
    if a.command == "export" && a.out.is_none() {
        return Err("export needs --out".into());
    }
    Ok(a)
}

fn hms(secs: f64) -> String {
    let s = secs.max(0.0).round() as u64;
    let (d, h, m) = (s / 86_400, s % 86_400 / 3600, s % 3600 / 60);
    if d > 0 {
        format!("{d} d {h} h")
    } else if h > 0 {
        format!("{h} h {m} min")
    } else {
        format!("{m} min {} s", s % 60)
    }
}

fn main() {
    let mut a = match parse() {
        Ok(a) => a,
        Err(e) => {
            if !e.is_empty() {
                eprintln!("anvil-bulk: {e}\n");
            }
            eprintln!("{USAGE}");
            std::process::exit(if e.is_empty() { 0 } else { 2 });
        }
    };
    let Some(dir) = a.project.clone() else { return };
    let project = Project::at(dir);
    let say = |s: &str| eprintln!("{s}");
    let result: Result<bool, String> = (|| match a.command.as_str() {
        "scan" => {
            let opts = ScanOptions {
                roots: a.inputs.clone(),
                step_roots: a.step_roots.clone(),
                threads: a.run.threads,
            };
            let s = anvil_bulk::scan(&project, &opts, &say).map_err(|e| e.to_string())?;
            println!(
                "{} SOLIDWORKS files, {} distinct ({} duplicates), {} with a STEP export. Hashed {} files ({:.1} GB) this time.",
                s.files,
                s.jobs,
                s.files - s.jobs,
                s.with_step,
                s.hashed,
                s.bytes_hashed as f64 / 1e9
            );
            if !s.problems.is_empty() {
                println!("{} paths could not be read; see reports/scan-problems.csv", s.problems.len());
            }
            Ok(true)
        }
        "run" => {
            a.run.read = ReadOptions::from_env();
            if let Some(f) = &a.key_file {
                let key = std::fs::read_to_string(f).map_err(|e| format!("{}: {e}", f.display()))?;
                a.run.read.document_manager_key = Some(key.trim().to_string()).filter(|k| !k.is_empty());
            }
            let s = anvil_bulk::run(&project, &a.run, &say).map_err(|e| e.to_string())?;
            let parts: Vec<String> = s.done.iter().map(|(k, n)| format!("{n} {}", k.label())).collect();
            println!(
                "{} of {} pending jobs done by {} ({}). {} held by other workers.",
                s.total(),
                s.pending_at_start,
                a.run.worker,
                if parts.is_empty() { "none".into() } else { parts.join(", ") },
                s.busy
            );
            let _ = anvil_bulk::report(&project, None);
            Ok(true)
        }
        "status" => {
            let p = anvil_bulk::report::progress(&project, false).map_err(|e| e.to_string())?;
            println!("{} jobs: {} pending, {} being converted", p.jobs, p.pending, p.claimed);
            for (k, n) in &p.done {
                println!("  {k}: {n}");
            }
            if let (Some(m), Some(eta)) = (p.mean_seconds, p.eta(a.workers)) {
                println!("{m:.1} s per job on average; about {} left with {} worker(s)", hms(eta), a.workers);
            }
            Ok(true)
        }
        "export" => {
            let out = a.out.clone().unwrap_or_default();
            let s = anvil_bulk::export(&project, &out, a.run.store.as_deref(), a.overwrite)
                .map_err(|e| e.to_string())?;
            println!(
                "Wrote {} parts to {} ({} kept as they were, {} with AnvilCAD properties added); files without a migrated part: {}.",
                s.written,
                out.display(),
                s.kept,
                s.merged,
                s.without_part
            );
            for (p, m) in &s.problems {
                println!("  {p}: {m}");
            }
            Ok(s.problems.is_empty())
        }
        _ => {
            let s = anvil_bulk::report(&project, None).map_err(|e| e.to_string())?;
            println!("{} files; reports in {}", s.all.files, project.reports_dir().display());
            Ok(true)
        }
    })();
    match result {
        Ok(ok) => std::process::exit(if ok { 0 } else { 1 }),
        Err(e) => {
            eprintln!("anvil-bulk: {e}");
            std::process::exit(1);
        }
    }
}
