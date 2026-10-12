//! Migration reports: per feature, per file, and for a whole batch.

use serde::Serialize;

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Rebuilt as an editable AnvilCAD feature.
    Parametric,
    /// Carried over as exact SOLIDWORKS geometry inside an imported body.
    Imported,
    /// Not carried over (no geometry of its own, or suppressed).
    Skipped,
}

#[derive(Serialize, Clone, Debug)]
pub struct FeatureReport {
    pub name: String,
    pub sw_type: String,
    pub outcome: Outcome,
    pub detail: String,
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileStatus {
    /// Every geometric feature was rebuilt parametrically.
    Parametric,
    /// Some features came across as imported geometry.
    Hybrid,
    Failed,
    Skipped,
}

#[derive(Serialize, Clone, Debug)]
pub struct FileReport {
    pub source: String,
    pub output: Option<String>,
    pub status: FileStatus,
    pub features: Vec<FeatureReport>,
    pub warnings: Vec<String>,
    pub sw_volume_mm3: Option<f64>,
    pub anvil_volume_mm3: Option<f64>,
    pub volume_error_pct: Option<f64>,
    /// True when AnvilCAD's rebuilt solid matches SOLIDWORKS' volume within tolerance.
    pub verified: bool,
    pub seconds: f64,
}

impl FileReport {
    pub fn new(source: &str) -> FileReport {
        FileReport {
            source: source.to_string(),
            output: None,
            status: FileStatus::Failed,
            features: Vec::new(),
            warnings: Vec::new(),
            sw_volume_mm3: None,
            anvil_volume_mm3: None,
            volume_error_pct: None,
            verified: false,
            seconds: 0.0,
        }
    }

    pub fn count(&self, o: Outcome) -> usize {
        self.features.iter().filter(|f| f.outcome == o).count()
    }

    pub fn add(&mut self, name: &str, sw_type: &str, outcome: Outcome, detail: impl Into<String>) {
        self.features.push(FeatureReport {
            name: name.to_string(),
            sw_type: sw_type.to_string(),
            outcome,
            detail: detail.into(),
        });
    }
}

#[derive(Serialize)]
pub struct BatchReport {
    pub tool: String,
    pub solidworks: String,
    pub files: Vec<FileReport>,
}

fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\n']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

impl BatchReport {
    pub fn to_csv(&self) -> String {
        let mut out = String::from(
            "source,status,verified,volume_error_pct,parametric_features,imported_features,skipped_features,seconds,output,notes\n",
        );
        for f in &self.files {
            let notes: Vec<String> = f
                .features
                .iter()
                .filter(|x| x.outcome == Outcome::Imported)
                .map(|x| format!("{}: {}", x.name, x.detail))
                .chain(f.warnings.iter().cloned())
                .collect();
            out.push_str(&format!(
                "{},{:?},{},{},{},{},{},{:.1},{},{}\n",
                csv_field(&f.source),
                f.status,
                f.verified,
                f.volume_error_pct.map(|e| format!("{e:.4}")).unwrap_or_default(),
                f.count(Outcome::Parametric),
                f.count(Outcome::Imported),
                f.count(Outcome::Skipped),
                f.seconds,
                csv_field(f.output.as_deref().unwrap_or("")),
                csv_field(&notes.join("; ")),
            ));
        }
        out
    }

    pub fn summary(&self) -> String {
        let n = self.files.len();
        let by = |s: FileStatus| self.files.iter().filter(|f| f.status == s).count();
        let verified = self.files.iter().filter(|f| f.verified).count();
        let feats: usize = self.files.iter().map(|f| f.features.len()).sum();
        let param: usize = self.files.iter().map(|f| f.count(Outcome::Parametric)).sum();
        let imported: usize = self.files.iter().map(|f| f.count(Outcome::Imported)).sum();
        format!(
            "{n} file(s): {} fully parametric, {} hybrid, {} failed, {} skipped. \
             {verified} verified against SOLIDWORKS volume.\n\
             Features: {param} parametric, {imported} imported as geometry, {} skipped (of {feats}).",
            by(FileStatus::Parametric),
            by(FileStatus::Hybrid),
            by(FileStatus::Failed),
            by(FileStatus::Skipped),
            feats - param - imported,
        )
    }
}
