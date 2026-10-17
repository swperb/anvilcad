//! anvil-props: shows and bulk-converts the custom properties of SOLIDWORKS files, without
//! SOLIDWORKS.
//!
//! `convert` walks folders (local disks, network shares, synced cloud folders) and records
//! each file's properties in an AnvilCAD property file beside it, plus a company-wide index.
//! Run it once while the company still has a SOLIDWORKS Document Manager key; afterwards
//! AnvilCAD shows the properties on any computer, with no SOLIDWORKS licence.
//! The SOLIDWORKS files themselves are only ever read.

use anvil_io::props::sidecar::{self, Outcome, Sheet};
use anvil_io::props::{self, FileProperties, ReadOptions, KEY_ENV};
use anvil_io::solidworks::doc_type_from_extension;
use std::io::Write;
use std::path::{Path, PathBuf};

const USAGE: &str = "\
Usage:
  anvil-props show [--json] [OPTIONS] <FILE>...
  anvil-props convert [OPTIONS] <INPUT>...

show     Prints a file's properties: the file's own (read live, or from a converted
         snapshot) and AnvilCAD's.
convert  Finds every SOLIDWORKS file under INPUT (files or folders, searched recursively)
         and records its custom properties in <file>.anvilprops.json, keeping any
         AnvilCAD properties already there. The SOLIDWORKS files are never modified.
         Works on any folder this computer can open: local disks, network shares
         (\\\\server\\share, /mnt/eng), and folders synced by cloud storage clients.

Options:
      --key-file <FILE>  File holding a SOLIDWORKS Document Manager licence key
                         (default: the ANVIL_SWDM_KEY environment variable). Needed, on
                         Windows, for files saved by SOLIDWORKS 2015 or later.
      --store <DIR>      Keep property files under DIR, mirroring each file's full path,
                         instead of beside the files (for read-only shares). Default: the
                         ANVIL_PROPS_STORE environment variable, if set.
  -o, --report <DIR>     convert: where to write the reports (default: ./anvil-props-report)
      --force            convert: read every file again, even if its snapshot is current
      --dry-run          convert: read and report, but write no property files
      --limit <N>        convert: process at most N files
  -h, --help             Show this help

convert writes to the report folder:
  conversion-report.csv  one row per file: status, property count, property file, message
  properties.csv         one row per property of every file: the company-wide index
  conversion-report.json the same, for scripts";

struct Args {
    command: String,
    inputs: Vec<PathBuf>,
    json: bool,
    key_file: Option<PathBuf>,
    store: Option<PathBuf>,
    report: PathBuf,
    force: bool,
    dry_run: bool,
    limit: Option<usize>,
}

fn parse_args() -> Result<Args, String> {
    let mut it = std::env::args().skip(1);
    let command = match it.next() {
        Some(c) if c == "show" || c == "convert" => c,
        Some(c) if c == "-h" || c == "--help" => return Err(String::new()),
        Some(c) => return Err(format!("unknown command '{c}'")),
        None => return Err(String::new()),
    };
    let mut a = Args {
        command,
        inputs: Vec::new(),
        json: false,
        key_file: None,
        store: sidecar::store_from_env(),
        report: PathBuf::from("anvil-props-report"),
        force: false,
        dry_run: false,
        limit: None,
    };
    while let Some(s) = it.next() {
        let mut val = |name: &str| it.next().ok_or_else(|| format!("{name} needs a value"));
        match s.as_str() {
            "--json" => a.json = true,
            "--key-file" => a.key_file = Some(val("--key-file")?.into()),
            "--store" => a.store = Some(val("--store")?.into()),
            "-o" | "--report" => a.report = val("--report")?.into(),
            "--force" => a.force = true,
            "--dry-run" => a.dry_run = true,
            "--limit" => a.limit = Some(val("--limit")?.parse().map_err(|_| "--limit needs a number")?),
            "-h" | "--help" => return Err(String::new()),
            o if o.starts_with('-') && o.len() > 1 => return Err(format!("unknown option '{o}'")),
            _ => a.inputs.push(s.into()),
        }
    }
    if a.inputs.is_empty() {
        return Err("no input given".into());
    }
    Ok(a)
}

fn read_options(a: &Args) -> Result<ReadOptions, String> {
    let mut opts = ReadOptions::from_env();
    if let Some(f) = &a.key_file {
        let key = std::fs::read_to_string(f).map_err(|e| format!("{}: {e}", f.display()))?;
        opts.document_manager_key = Some(key.trim().to_string()).filter(|k| !k.is_empty());
    }
    Ok(opts)
}

fn main() {
    let a = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            if !e.is_empty() {
                eprintln!("anvil-props: {e}\n");
            }
            eprintln!("{USAGE}");
            std::process::exit(if e.is_empty() { 0 } else { 2 });
        }
    };
    let opts = match read_options(&a) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("anvil-props: {e}");
            std::process::exit(2);
        }
    };
    let ok = match a.command.as_str() {
        "show" => show(&a, &opts),
        _ => convert(&a, &opts),
    };
    std::process::exit(if ok { 0 } else { 1 });
}

// ----- show ---------------------------------------------------------------------------

fn show(a: &Args, opts: &ReadOptions) -> bool {
    let mut ok = true;
    let mut json = Vec::new();
    for f in &a.inputs {
        let sheet = Sheet::open(f, opts, a.store.as_deref());
        ok &= sheet.file.is_some() || !sheet.sidecar.properties.is_empty();
        if a.json {
            json.push(sheet_json(&sheet));
            continue;
        }
        println!("{}", f.display());
        match (&sheet.file, &sheet.live_error) {
            (Some((origin, _)), Some(e)) if origin.is_file() => {
                println!("  (not readable here: {e})");
                println!(
                    "  showing the converted snapshot{}",
                    if sheet.stale { "; the file has changed since" } else { "" }
                );
            }
            (None, Some(e)) => println!("  file properties: {e}"),
            _ => {}
        }
        if let Some(e) = &sheet.sidecar_error {
            println!("  AnvilCAD property file: {e}");
        }
        let rows = sheet.rows();
        let width = rows.iter().map(|r| r.name.chars().count()).max().unwrap_or(0);
        let mut last_cfg: Option<&str> = None;
        for r in &rows {
            if r.configuration.as_deref() != last_cfg {
                if let Some(c) = &r.configuration {
                    println!("  [configuration: {c}]");
                }
                last_cfg = r.configuration.as_deref();
            }
            let expr = r.expression.as_ref().map(|e| format!("   = {e}")).unwrap_or_default();
            let flag = if r.conflict { "   (also set in the file; the file's value is kept)" } else { "" };
            println!("  {:width$}: {}{expr}   [{}]{flag}", r.name, r.value, r.origin.label());
        }
        if let Some((_, fp)) = &sheet.file {
            for s in &fp.summary {
                println!("  {:width$}: {}   [summary]", s.name, s.value);
            }
            for n in &fp.notes {
                println!("  note: {n}");
            }
        }
        if rows.is_empty() {
            println!("  (no properties)");
        }
    }
    if a.json {
        println!("{}", serde_json::to_string_pretty(&json).unwrap());
    }
    ok
}

fn sheet_json(s: &Sheet) -> serde_json::Value {
    let rows: Vec<_> = s
        .rows()
        .iter()
        .map(|r| {
            serde_json::json!({
                "name": r.name,
                "type": r.kind,
                "value": r.value,
                "expression": r.expression,
                "configuration": r.configuration,
                "origin": r.origin.label(),
                "from_file": r.origin.is_file(),
                "conflict": r.conflict,
            })
        })
        .collect();
    serde_json::json!({
        "file": s.target.display().to_string(),
        "property_file": s.sidecar_path.display().to_string(),
        "properties": rows,
        "summary": s.file.as_ref().map(|(_, fp)| fp.summary.clone()).unwrap_or_default(),
        "notes": s.file.as_ref().map(|(_, fp)| fp.notes.clone()).unwrap_or_default(),
        "live_error": s.live_error,
        "stale": s.stale,
    })
}

// ----- convert ------------------------------------------------------------------------

/// SOLIDWORKS files under `root`. Symbolic links to folders are not followed, so a link
/// cycle on a share cannot loop. Unreadable folders are reported, not fatal.
fn collect(root: &Path, out: &mut Vec<PathBuf>, problems: &mut Vec<(PathBuf, String)>) {
    let md = match std::fs::symlink_metadata(root) {
        Ok(m) => m,
        Err(e) => {
            problems.push((root.to_path_buf(), e.to_string()));
            return;
        }
    };
    if !md.is_dir() {
        if is_candidate(root) {
            out.push(root.to_path_buf());
        } else if root.is_file() && md.file_type().is_file() {
            problems.push((root.to_path_buf(), "not a SOLIDWORKS file".into()));
        }
        return;
    }
    let mut entries: Vec<_> = match std::fs::read_dir(root) {
        Ok(rd) => rd.filter_map(|e| e.ok()).collect(),
        Err(e) => {
            problems.push((root.to_path_buf(), e.to_string()));
            return;
        }
    };
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let p = e.path();
        match e.file_type() {
            Ok(t) if t.is_dir() => collect(&p, out, problems),
            Ok(t) if t.is_file() && is_candidate(&p) => out.push(p),
            // Symbolic links to files are followed; links to folders are not.
            Ok(t) if t.is_symlink() && p.is_file() && is_candidate(&p) => out.push(p),
            _ => {}
        }
    }
}

fn is_candidate(p: &Path) -> bool {
    let name = p.file_name().map(|n| n.to_string_lossy()).unwrap_or_default();
    // "~$name" files are SOLIDWORKS' and Office's lock files for open documents.
    doc_type_from_extension(p).is_some() && !name.starts_with("~$")
}

#[derive(serde::Serialize)]
struct FileRow {
    file: String,
    status: &'static str,
    properties: usize,
    property_file: String,
    message: String,
}

fn convert(a: &Args, opts: &ReadOptions) -> bool {
    let mut files = Vec::new();
    let mut problems = Vec::new();
    for i in &a.inputs {
        collect(i, &mut files, &mut problems);
    }
    if let Some(n) = a.limit {
        files.truncate(n);
    }
    if opts.document_manager_key.is_none() || cfg!(not(windows)) {
        eprintln!(
            "note: no Document Manager {}; files saved by SOLIDWORKS 2015 or later will be \
             reported as needing it, and files from 2014 or earlier are read from their \
             standard property sets.",
            if cfg!(windows) {
                format!("key ({KEY_ENV} or --key-file)")
            } else {
                "(it runs on Windows)".into()
            }
        );
    }
    eprintln!("{} SOLIDWORKS file(s) found", files.len());

    let mut rows: Vec<FileRow> = problems
        .iter()
        .map(|(p, m)| FileRow {
            file: p.display().to_string(),
            status: "skipped",
            properties: 0,
            property_file: String::new(),
            message: m.clone(),
        })
        .collect();
    let mut index: Vec<[String; 6]> = Vec::new();
    let (mut done, mut current, mut failed) = (0, 0, 0);
    for (n, f) in files.iter().enumerate() {
        let outcome = sidecar::convert(f, opts, a.store.as_deref(), a.force, a.dry_run);
        let (status, count, side, message, fp): (_, _, _, _, Option<FileProperties>) = match outcome {
            Outcome::Imported { count, sidecar, properties } => {
                done += 1;
                ("converted", count, sidecar, String::new(), Some(properties))
            }
            Outcome::DryRun { count, sidecar, properties } => {
                done += 1;
                ("read (dry run)", count, sidecar, String::new(), Some(properties))
            }
            Outcome::UpToDate { sidecar, properties } => {
                current += 1;
                ("up to date", properties.count(), sidecar, String::new(), Some(properties))
            }
            Outcome::Failed(m) => {
                failed += 1;
                ("failed", 0, PathBuf::new(), m, None)
            }
        };
        eprintln!(
            "[{}/{}] {status}: {}{}",
            n + 1,
            files.len(),
            f.display(),
            if message.is_empty() { String::new() } else { format!(" ({message})") }
        );
        let file = f.display().to_string();
        if let Some(fp) = &fp {
            let mut push = |cfg: &str, p: &props::FileProperty| {
                index.push([
                    file.clone(),
                    cfg.to_string(),
                    p.name.clone(),
                    p.kind.label().to_string(),
                    p.value.clone(),
                    p.expression.clone().unwrap_or_default(),
                ])
            };
            fp.properties.iter().for_each(|p| push("", p));
            for c in &fp.configurations {
                c.properties.iter().for_each(|p| push(&c.name, p));
            }
        }
        let message = if message.is_empty() && fp.as_ref().is_some_and(|p| !p.notes.is_empty()) {
            fp.unwrap().notes.join(" ")
        } else {
            message
        };
        rows.push(FileRow {
            file,
            status,
            properties: count,
            property_file: if side.as_os_str().is_empty() {
                String::new()
            } else {
                side.display().to_string()
            },
            message,
        });
    }

    if let Err(e) = write_reports(&a.report, &rows, &index) {
        eprintln!("anvil-props: could not write reports to {}: {e}", a.report.display());
        return false;
    }
    eprintln!(
        "\n{done} {}, {current} already up to date, {failed} failed, {} skipped. Reports in {}",
        if a.dry_run { "read" } else { "converted" },
        problems.len(),
        a.report.display()
    );
    failed == 0
}

/// A CSV cell. Cells a spreadsheet would run as a formula get a leading apostrophe.
fn cell(s: &str) -> String {
    let formula =
        s.starts_with(['=', '+', '@', '\t', '\r']) || (s.starts_with('-') && s.parse::<f64>().is_err());
    let s = if formula { format!("'{s}") } else { s.to_string() };
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s
    }
}

fn write_csv(path: &Path, header: &[&str], rows: impl Iterator<Item = Vec<String>>) -> std::io::Result<()> {
    let mut w = std::io::BufWriter::new(std::fs::File::create(path)?);
    // A byte order mark so spreadsheet programs read the file as UTF-8.
    w.write_all(b"\xEF\xBB\xBF")?;
    writeln!(w, "{}", header.join(","))?;
    for r in rows {
        writeln!(w, "{}", r.iter().map(|c| cell(c)).collect::<Vec<_>>().join(","))?;
    }
    w.flush()
}

fn write_reports(dir: &Path, rows: &[FileRow], index: &[[String; 6]]) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    write_csv(
        &dir.join("conversion-report.csv"),
        &["file", "status", "properties", "property_file", "message"],
        rows.iter().map(|r| {
            vec![
                r.file.clone(),
                r.status.into(),
                r.properties.to_string(),
                r.property_file.clone(),
                r.message.clone(),
            ]
        }),
    )?;
    write_csv(
        &dir.join("properties.csv"),
        &["file", "configuration", "property", "type", "value", "expression"],
        index.iter().map(|r| r.to_vec()),
    )?;
    let json = serde_json::json!({ "files": rows });
    std::fs::write(dir.join("conversion-report.json"), serde_json::to_string_pretty(&json).unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_cells() {
        assert_eq!(cell("large wheel"), "large wheel");
        assert_eq!(cell("a, b"), "\"a, b\"");
        assert_eq!(cell("say \"hi\""), "\"say \"\"hi\"\"\"");
        assert_eq!(cell("=HYPERLINK(1)"), "'=HYPERLINK(1)");
        assert_eq!(cell("-5"), "-5");
        assert_eq!(cell("-cmd"), "'-cmd");
    }

    #[test]
    fn candidates() {
        assert!(is_candidate(Path::new("/x/wheel.SLDPRT")));
        assert!(!is_candidate(Path::new("/x/~$wheel.sldprt")));
        assert!(!is_candidate(Path::new("/x/wheel.sldprt.anvilprops.json")));
        assert!(!is_candidate(Path::new("/x/wheel.step")));
    }
}
