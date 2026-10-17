//! Writing the migrated folder tree: one AnvilCAD part per original part file, at the same
//! relative path, including every copy of a file that was converted once.

use crate::project::{io, Entry, Project, Result};
use anvil_io::props::sidecar::{self, Sidecar};
use anvil_model::Document;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default)]
pub struct ExportSummary {
    pub written: usize,
    /// Parts that already existed in the output and were left alone.
    pub kept: usize,
    /// Parts that received AnvilCAD properties from an `.anvilprops.json` file.
    pub merged: usize,
    /// Files with no converted part (yet).
    pub without_part: usize,
    pub problems: Vec<(String, String)>,
}

/// A folder name for each root: its last component, made unique.
pub fn root_labels(roots: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    roots
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let base: String = Path::new(r)
                .components()
                .filter_map(|c| match c {
                    std::path::Component::Normal(n) => Some(n.to_string_lossy().into_owned()),
                    _ => None,
                })
                .next_back()
                .unwrap_or_else(|| "root".into());
            let mut label = base.clone();
            if !seen.insert(label.clone()) {
                label = format!("{base}-{}", i + 1);
                seen.insert(label.clone());
            }
            label
        })
        .collect()
}

/// Where an entry's migrated part goes under `out`.
pub fn destination(out: &Path, labels: &[String], e: &Entry) -> PathBuf {
    let mut p = out.join(labels.get(e.root).map(String::as_str).unwrap_or("root"));
    for part in e.rel.split('/') {
        if !part.is_empty() && part != "." && part != ".." {
            p.push(part);
        }
    }
    p.with_extension(anvil_model::PART_EXT)
}

fn merge_anvil_properties(dest: &Path, side: &Sidecar) -> std::result::Result<bool, String> {
    if side.properties.is_empty() {
        return Ok(false);
    }
    let mut doc = Document::load(dest).map_err(|e| e.to_string())?;
    let mut added = false;
    for p in &side.properties {
        if !doc.custom_properties.iter().any(|q| q.name.eq_ignore_ascii_case(&p.name)) {
            doc.custom_properties.push(p.clone());
            added = true;
        }
    }
    if added {
        doc.save(dest).map_err(|e| e.to_string())?;
    }
    Ok(added)
}

/// Writes the migrated tree under `out`, then the reports. Existing parts in `out` are kept
/// unless `overwrite`, so edits made to exported parts are never lost.
pub fn export(project: &Project, out: &Path, store: Option<&Path>, overwrite: bool) -> Result<ExportSummary> {
    let pf = project.load()?;
    let labels = root_labels(&pf.roots);
    let mut s = ExportSummary::default();
    for e in project.inventory()? {
        let part = project.part_path(&e.hash);
        let has_part = project.result(&e.hash).is_some_and(|r| r.status.has_part()) && part.exists();
        if !has_part {
            s.without_part += 1;
            continue;
        }
        let dest = destination(out, &labels, &e);
        if dest.exists() && !overwrite {
            s.kept += 1;
            continue;
        }
        if let Some(dir) = dest.parent() {
            io(dir, std::fs::create_dir_all(dir))?;
        }
        if let Err(err) = std::fs::copy(&part, &dest) {
            s.problems.push((dest.display().to_string(), err.to_string()));
            continue;
        }
        s.written += 1;
        // AnvilCAD properties added to the original file through anvil-props come along.
        let side_path = sidecar::locate(Path::new(&e.path), store);
        match Sidecar::load(&side_path) {
            Ok(Some(side)) => match merge_anvil_properties(&dest, &side) {
                Ok(true) => s.merged += 1,
                Ok(false) => {}
                Err(err) => s.problems.push((dest.display().to_string(), err)),
            },
            Ok(None) => {}
            Err(err) => s.problems.push((side_path.display().to_string(), err.to_string())),
        }
    }
    crate::report::report(project, Some(out))?;
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_are_unique() {
        let l = root_labels(&["/srv/eng".into(), "/mnt/eng".into(), "C:\\".into()]);
        assert_eq!(l[0], "eng");
        assert_eq!(l[1], "eng-2");
        assert!(!l[2].is_empty());
    }

    #[test]
    fn destinations_stay_inside_the_output() {
        let e = Entry {
            path: "/srv/eng/a/../wheel.sldprt".into(),
            root: 0,
            rel: "a/../../wheel.SLDPRT".into(),
            kind: crate::project::Kind::Part,
            size: 1,
            modified_ns: None,
            hash: "ab".into(),
            step: None,
        };
        let d = destination(Path::new("/out"), &["eng".into()], &e);
        assert_eq!(d, Path::new("/out/eng/a/wheel.anvprt"));
    }
}
