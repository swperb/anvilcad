//! anvil-migrate: batch-migrates SOLIDWORKS parts to AnvilCAD, keeping the feature tree.
//!
//! Runs on a Windows machine with SOLIDWORKS installed and licensed. SOLIDWORKS itself
//! reads each file; AnvilCAD rebuilds every feature and checks the result against
//! SOLIDWORKS' geometry.

#[cfg(windows)]
mod com;
#[cfg(windows)]
mod export;
mod report;
#[cfg(windows)]
mod sw;

use std::path::{Path, PathBuf};

const USAGE: &str = "\
Usage: anvil-migrate [OPTIONS] <INPUT>...

Migrates SOLIDWORKS parts (.sldprt) to AnvilCAD parts (.anvprt), keeping the feature tree.
INPUT may be files or folders (searched recursively).

Options:
  -o, --out <DIR>      Output folder (default: ./migrated). Folder structure is preserved.
      --no-verify      Skip per-feature verification against SOLIDWORKS (faster)
      --precise-geometry  Tessellate imported geometry at 0.01 mm (slower, more accurate)
      --limit <N>      Process at most N parts
      --progid <ID>    SOLIDWORKS automation ProgID (default: SldWorks.Application)
  -h, --help           Show this help

Writes migration-report.json and migration-report.csv to the output folder.";

struct Args {
    inputs: Vec<PathBuf>,
    out: PathBuf,
    verify: bool,
    precise: bool,
    limit: Option<usize>,
    progid: String,
}

fn parse_args() -> Result<Args, String> {
    let mut a = Args {
        inputs: Vec::new(),
        out: PathBuf::from("migrated"),
        verify: true,
        precise: false,
        limit: None,
        progid: "SldWorks.Application".into(),
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "-h" | "--help" => return Err(String::new()),
            "-o" | "--out" => a.out = it.next().ok_or("--out needs a folder")?.into(),
            "--no-verify" => a.verify = false,
            "--precise-geometry" => a.precise = true,
            "--limit" => {
                a.limit = Some(it.next().and_then(|n| n.parse().ok()).ok_or("--limit needs a number")?)
            }
            "--progid" => a.progid = it.next().ok_or("--progid needs a value")?,
            s if s.starts_with('-') => return Err(format!("unknown option {s}")),
            _ => a.inputs.push(arg.into()),
        }
    }
    if a.inputs.is_empty() {
        return Err("no input files or folders given".into());
    }
    Ok(a)
}

fn ext_lower(p: &Path) -> String {
    p.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase()
}

/// Collects SOLIDWORKS files, remembering the input root for relative output paths.
fn collect(inputs: &[PathBuf]) -> Vec<(PathBuf, PathBuf)> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<(PathBuf, PathBuf)>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        let mut entries: Vec<_> = rd.flatten().map(|e| e.path()).collect();
        entries.sort();
        for p in entries {
            if p.is_dir() {
                walk(root, &p, out);
            } else if matches!(ext_lower(&p).as_str(), "sldprt" | "sldasm" | "slddrw")
                && !p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("~$"))
            {
                out.push((root.to_path_buf(), p));
            }
        }
    }
    let mut out = Vec::new();
    for i in inputs {
        if i.is_dir() {
            walk(i, i, &mut out);
        } else {
            out.push((i.parent().map(Path::to_path_buf).unwrap_or_default(), i.clone()));
        }
    }
    out
}

#[cfg(windows)]
fn run(args: Args) -> Result<(), String> {
    use report::{BatchReport, FileReport, FileStatus};

    let files = collect(&args.inputs);
    let files: Vec<_> = match args.limit {
        Some(n) => files.into_iter().take(n).collect(),
        None => files,
    };
    if files.is_empty() {
        return Err("no SOLIDWORKS files found".into());
    }
    std::fs::create_dir_all(&args.out).map_err(|e| format!("cannot create {}: {e}", args.out.display()))?;

    eprintln!("Connecting to SOLIDWORKS...");
    let session =
        sw::Session::connect(&args.progid).map_err(|e| format!("could not start SOLIDWORKS: {e}"))?;
    eprintln!("Using {} ({}).", session.release_name(), session.revision);

    let opts = export::Options { verify_each: args.verify, precise_geometry: args.precise };
    let mut batch = BatchReport {
        tool: format!("anvil-migrate {}", env!("CARGO_PKG_VERSION")),
        solidworks: format!("{} ({})", session.release_name(), session.revision),
        files: Vec::new(),
    };
    let total = files.len();
    for (n, (root, path)) in files.iter().enumerate() {
        let src = path.to_string_lossy().to_string();
        eprint!("[{}/{total}] {} ... ", n + 1, path.display());
        if ext_lower(path) != "sldprt" {
            let mut r = FileReport::new(&src);
            r.status = FileStatus::Skipped;
            r.warnings.push("assemblies and drawings are not migrated yet".into());
            eprintln!("skipped (assemblies and drawings come later)");
            batch.files.push(r);
            continue;
        }
        let (doc, mut rep) = export::migrate_part(&session, &src, &opts);
        if let Some(doc) = doc {
            let rel = path.strip_prefix(root).unwrap_or(path);
            let out = args.out.join(rel).with_extension(anvil_model::PART_EXT);
            if let Some(dir) = out.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            match doc.save(&out) {
                Ok(()) => rep.output = Some(out.to_string_lossy().to_string()),
                Err(e) => {
                    rep.status = FileStatus::Failed;
                    rep.warnings.push(format!("could not save: {e}"));
                }
            }
        }
        eprintln!(
            "{:?}{} ({} parametric, {} imported) {:.1}s",
            rep.status,
            if rep.verified { ", verified" } else { ", NOT verified" },
            rep.count(report::Outcome::Parametric),
            rep.count(report::Outcome::Imported),
            rep.seconds
        );
        batch.files.push(rep);
        // Keep the report current so an interrupted run still leaves results.
        write_reports(&args.out, &batch);
    }
    session.shutdown();
    write_reports(&args.out, &batch);
    println!("{}", batch.summary());
    println!("Report: {}", args.out.join("migration-report.csv").display());
    Ok(())
}

#[cfg(windows)]
fn write_reports(out: &Path, batch: &report::BatchReport) {
    let _ = std::fs::write(out.join("migration-report.json"), serde_json::to_string_pretty(batch).unwrap());
    let _ = std::fs::write(out.join("migration-report.csv"), batch.to_csv());
}

#[cfg(not(windows))]
fn run(_: Args) -> Result<(), String> {
    Err("anvil-migrate needs Windows with SOLIDWORKS installed".into())
}

fn main() {
    match parse_args().and_then(run) {
        Ok(()) => {}
        Err(e) if e.is_empty() => println!("{USAGE}"),
        Err(e) => {
            eprintln!("error: {e}\n\n{USAGE}");
            std::process::exit(1);
        }
    }
}

#[cfg(all(windows, test))]
mod bench {
    #[test]
    #[ignore]
    fn com_call_cost() {
        let s = crate::sw::Session::connect("SldWorks.Application").unwrap();
        let t = std::time::Instant::now();
        for _ in 0..200 {
            s.app.string("RevisionNumber", &[]).unwrap();
        }
        let per = t.elapsed().as_secs_f64() / 200.0 * 1000.0;
        let t = std::time::Instant::now();
        for _ in 0..200 {
            s.app.bool("Visible", &[]).unwrap();
        }
        let per2 = t.elapsed().as_secs_f64() / 200.0 * 1000.0;
        println!("RevisionNumber: {per:.3} ms/call, Visible: {per2:.3} ms/call");
    }
}

#[cfg(all(windows, test))]
mod maintenance {
    /// Lists open documents; closes the one named in ANVIL_CLOSE (exact path) if present.
    #[test]
    #[ignore]
    fn list_and_close_leftover() {
        let s = crate::sw::Session::connect("SldWorks.Application").unwrap();
        let target = std::env::var("ANVIL_CLOSE").unwrap_or_default();
        let mut doc = s.app.opt_obj("GetFirstDocument", &[]).unwrap();
        let mut to_close = None;
        while let Some(d) = doc {
            let path = d.string("GetPathName", &[]).unwrap_or_default();
            let title = d.string("GetTitle", &[]).unwrap_or_default();
            println!("open: {title} | {path}");
            if !target.is_empty() && path.eq_ignore_ascii_case(&target) {
                to_close = Some(title);
            }
            doc = d.opt_obj("GetNext", &[]).unwrap();
        }
        if let Some(t) = to_close {
            s.app.call("CloseDoc", &[t.as_str().into()]).unwrap();
            println!("closed: {t}");
        }
    }
}

#[cfg(all(windows, test))]
mod probe_fillet {
    use crate::com::Arg;

    /// Prints what the API returns for a fillet's selections (ANVIL_PART, ANVIL_FEATURE).
    #[test]
    #[ignore]
    fn fillet_selections() {
        let s = crate::sw::Session::connect("SldWorks.Application").unwrap();
        let path = std::env::var("ANVIL_PART").unwrap();
        let want = std::env::var("ANVIL_FEATURE").unwrap();
        let doc = s.open_part(&path).unwrap();
        let mut f = doc.model.opt_obj("FirstFeature", &[]).unwrap();
        while let Some(feat) = f {
            if feat.string("Name", &[]).unwrap() == want {
                let data = feat.obj("GetDefinition", &[]).unwrap();
                println!("before access: Edges={:?}", data.call("Edges", &[]).map(|v| v.list().len()));
                let ok = data.call("AccessSelections", &[(&doc.model).into(), Arg::Disp(None)]);
                println!("AccessSelections -> {ok:?}");
                println!(
                    "FilletItemsCount={:?} EdgeCount={:?} Type={:?}",
                    data.call("FilletItemsCount", &[]),
                    data.call("GetEdgeCount", &[]),
                    data.call("Type", &[])
                );
                println!("Edges={:?}", data.call("Edges", &[]).map(|v| v.list().len()));
                println!(
                    "item0={:?}",
                    data.call("GetFilletItemAtIndex", &[0.into()]).map(|v| format!("{v:?}"))
                );
                println!(
                    "item1={:?}",
                    data.call("GetFilletItemAtIndex", &[1.into()]).map(|v| format!("{v:?}"))
                );
                for e in data.call("Edges", &[]).map(|v| v.objects()).unwrap_or_default().iter().take(2) {
                    println!("  edge mid {:?}", crate::sw::edge_midpoint(e));
                }
                let _ = data.call("ReleaseSelectionAccess", &[]);
                break;
            }
            f = feat.opt_obj("GetNextFeature", &[]).unwrap();
        }
        s.close(&doc);
    }
}
