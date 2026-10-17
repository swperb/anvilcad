//! Bulk migration of an organisation's SOLIDWORKS files to AnvilCAD.
//!
//! A migration lives in a project folder, which can sit on a network share so that many
//! machines work on it at once:
//!
//! 1. [`scan`] walks the organisation's folders, shares and synced cloud folders, records
//!    every SOLIDWORKS file with a content hash, pairs each with a STEP export of it if one
//!    exists, and makes one job per distinct file content. Copies of the same file are
//!    converted once.
//! 2. [`run`] converts jobs. Any number of workers on any number of machines can run at
//!    once: each job is claimed with an atomic file creation, and finished jobs are never
//!    redone, so a run can be stopped and resumed at any time.
//! 3. [`export`] writes the migrated folder tree, one AnvilCAD part per original file, and
//!    [`report`] summarises the whole migration.
//!
//! Every step costs time proportional to the number of files (plus their size, for
//! hashing), so the work grows linearly with the organisation's archive.
//!
//! Nothing here needs SOLIDWORKS. Geometry comes from STEP files (exact where the STEP file
//! is) and custom properties from the files' standard property sets or `anvil-props`
//! snapshots. When an organisation still has SOLIDWORKS licences, two optional plug-ins
//! recover more: `anvil-migrate` for feature trees ([`migrate`]) and the Document Manager
//! for properties of newer files. The original files are only ever read.

pub mod export;
pub mod migrate;
pub mod project;
pub mod report;
pub mod run;
pub mod scan;

pub use export::export;
pub use project::Project;
pub use report::report;
pub use run::run;
pub use scan::scan;

/// A CSV cell. Cells a spreadsheet would run as a formula get a leading apostrophe.
pub(crate) fn csv_cell(s: &str) -> String {
    let formula =
        s.starts_with(['=', '+', '@', '\t', '\r']) || (s.starts_with('-') && s.parse::<f64>().is_err());
    let s = if formula { format!("'{s}") } else { s.to_string() };
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s
    }
}

pub(crate) fn write_csv(
    path: &std::path::Path,
    header: &[&str],
    rows: &[Vec<String>],
) -> std::io::Result<()> {
    use std::io::Write;
    let tmp = project::tmp_path(path);
    {
        let mut w = std::io::BufWriter::new(std::fs::File::create(&tmp)?);
        // A byte order mark so spreadsheet programs read the file as UTF-8.
        w.write_all(b"\xEF\xBB\xBF")?;
        writeln!(w, "{}", header.join(","))?;
        for r in rows {
            writeln!(w, "{}", r.iter().map(|c| csv_cell(c)).collect::<Vec<_>>().join(","))?;
        }
        w.flush()?;
    }
    std::fs::rename(&tmp, path)
}

pub(crate) fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// YYYY-MM-DD for seconds since 1970-01-01 UTC.
pub(crate) fn date(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!("{y:04}-{m:02}-{d:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_and_dates() {
        assert_eq!(csv_cell("a, b"), "\"a, b\"");
        assert_eq!(csv_cell("=1+1"), "'=1+1");
        assert_eq!(csv_cell("-2.5"), "-2.5");
        assert_eq!(date(0), "1970-01-01");
        assert_eq!(date(1_710_460_800), "2024-03-15");
        assert_eq!(date(-1), "1969-12-31");
    }
}
