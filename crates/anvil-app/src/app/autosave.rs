//! Autosave and crash recovery in the app (issue #86). The files themselves are handled by
//! [`crate::recovery`]; this is when to write them and how the user gets work back.

use super::AnvilApp;
use crate::recovery::{self, Content, Found, Recovery};
use anvil_model::document::Document;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

/// Autosave's state in the app. Off (no `recovery`) in the headless harness and tests
/// unless they turn it on, so they never write to the user's data folder.
pub(super) struct Autosave {
    recovery: Option<Recovery>,
    /// How often to autosave; `None` when `anvil.opt.autosave = 0` turned it off.
    pub every: Option<Duration>,
    last: Instant,
    /// What the recovery file holds now, so an unchanged document is not written again.
    written: Option<Content>,
    /// The last write failure shown in the status bar, so it is shown once.
    reported: Option<String>,
    /// Work left by an instance that did not exit cleanly, waiting for Restore or Discard.
    pub found: Vec<Found>,
}

impl Default for Autosave {
    fn default() -> Self {
        Autosave {
            recovery: None,
            every: Some(recovery::DEFAULT_INTERVAL),
            last: Instant::now(),
            written: None,
            reported: None,
            found: Vec::new(),
        }
    }
}

/// "3 minutes ago" and the like.
fn ago(t: Option<SystemTime>) -> String {
    let Some(secs) = t.and_then(|t| SystemTime::now().duration_since(t).ok()).map(|d| d.as_secs()) else {
        return "at an unknown time".into();
    };
    let unit = |n: u64, one: &str| format!("{n} {one}{} ago", if n == 1 { "" } else { "s" });
    match secs {
        0..60 => "less than a minute ago".into(),
        60..3600 => unit(secs / 60, "minute"),
        3600..86_400 => unit(secs / 3600, "hour"),
        _ => unit(secs / 86_400, "day"),
    }
}

impl AnvilApp {
    /// Turns on autosave into `dir` and lists work an earlier instance left behind.
    pub fn enable_recovery(&mut self, dir: &Path) -> Result<(), String> {
        let r = Recovery::start(dir).map_err(|e| {
            format!("Autosave is off: the recovery folder {} cannot be used ({e}).", dir.display())
        })?;
        self.autosave.found = recovery::scan(dir, Some(r.file()));
        self.autosave.recovery = Some(r);
        Ok(())
    }

    /// Turns on autosave in the user's recovery folder; says in the status bar if it cannot.
    pub fn enable_default_recovery(&mut self) {
        let result = match recovery::default_recovery_dir() {
            Some(dir) => self.enable_recovery(&dir),
            None => Err("Autosave is off: there is no home folder to keep recovery files in.".into()),
        };
        if let Err(e) = result {
            self.status = e;
        }
    }

    /// The open document as autosave writes it, and the file it belongs to.
    fn autosave_content(&self) -> (Content, Option<PathBuf>) {
        if let Some(d) = &self.drw {
            return (Content::Drawing(d.drawing.clone()), d.path.clone());
        }
        match &self.asm {
            Some(s) => (Content::Assembly(s.asm.clone()), s.path.clone()),
            None => (Content::Part(self.doc.clone()), self.path.clone()),
        }
    }

    /// Queues a copy of the document if it changed since the last one.
    fn autosave_write(&mut self) {
        let Some(r) = &self.autosave.recovery else { return };
        let (content, original) = self.autosave_content();
        if self.autosave.written.as_ref() != Some(&content) {
            r.save(content.clone(), original);
            self.autosave.written = Some(content);
        }
    }

    /// Runs every frame: removes the copy once there is nothing unsaved, and writes one
    /// when the interval has passed.
    pub(super) fn autosave_tick(&mut self, ctx: &egui::Context) {
        let Some(r) = &self.autosave.recovery else { return };
        if let Some(e) = r.last_error() {
            if self.autosave.reported.as_ref() != Some(&e) {
                self.status = format!("Autosave failed ({e}). Save your work to keep it safe.");
                self.autosave.reported = Some(e);
            }
        }
        if !self.is_dirty() {
            // Saved, undone back to the saved state, or a new document: nothing to recover.
            if self.autosave.written.take().is_some() {
                r.clear();
            }
            return;
        }
        let Some(every) = self.autosave.every else { return };
        let waited = self.autosave.last.elapsed();
        if waited < every {
            // Wake up for it even if the user leaves the window alone.
            ctx.request_repaint_after(every - waited);
            return;
        }
        self.autosave.last = Instant::now();
        self.autosave_write();
    }

    /// Writes a copy of unsaved work now and waits for it. For tests and scripts.
    pub fn autosave_now(&mut self) {
        if self.is_dirty() {
            self.autosave_write();
        }
        if let Some(r) = &self.autosave.recovery {
            r.flush();
        }
    }

    /// Work a crash left behind, for the harness: each one's name, and why it cannot be
    /// opened if it cannot.
    pub fn recovery_found(&self) -> Vec<serde_json::Value> {
        self.autosave
            .found
            .iter()
            .map(|f| serde_json::json!({ "name": f.name(), "path": f.path, "error": f.content.as_ref().err() }))
            .collect()
    }

    /// This instance's recovery file, while autosave is on.
    pub fn recovery_file(&self) -> Option<&Path> {
        self.autosave.recovery.as_ref().map(|r| r.file())
    }

    /// A clean exit: the recovery file and its lock go.
    pub fn finish_recovery(&mut self) {
        if let Some(r) = self.autosave.recovery.take() {
            r.finish();
        }
    }

    /// Opens recovered work `i` in place of the current document.
    pub fn restore_recovered(&mut self, i: usize) {
        if i >= self.autosave.found.len() {
            return;
        }
        if self.is_dirty() {
            self.message = Some((
                "Restore".into(),
                format!("Save or close {} first; restoring replaces the open document.", self.part_name()),
            ));
            return;
        }
        let f = self.autosave.found.remove(i);
        let name = f.name();
        match f.content {
            Ok(Content::Part(doc)) => {
                // Compared with the user's file, so the restored work shows as unsaved.
                let on_disk = f.original.as_ref().and_then(|p| Document::load(p).ok());
                self.load_document(doc, f.original.clone());
                self.saved = on_disk.unwrap_or_default();
            }
            Ok(Content::Assembly(asm)) => self.restore_assembly(asm, f.original.clone()),
            Ok(Content::Drawing(d)) => self.restore_drawing(d, f.original.clone()),
            Err(_) => {
                // Not offered for files that cannot be read; keep it listed.
                self.autosave.found.insert(i, f);
                return;
            }
        }
        // Into this instance's own recovery file before the old one goes, so a crash right
        // now still leaves a copy.
        self.autosave.written = None;
        self.autosave_now();
        if let Err(e) = recovery::remove(&f.path) {
            self.status = format!("Restored {name}, but its old recovery file could not be removed ({e}).");
            return;
        }
        self.status = format!("Restored {} from autosave. Save it to keep it.", self.part_name());
    }

    /// Deletes recovered work `i` (Discard), or hides one that cannot be read (Dismiss),
    /// which keeps its file.
    pub fn discard_recovered(&mut self, i: usize) {
        if i >= self.autosave.found.len() {
            return;
        }
        let f = self.autosave.found.remove(i);
        if f.content.is_ok() {
            if let Err(e) = recovery::remove(&f.path) {
                self.status = format!("The recovered copy of {} could not be deleted ({e}).", f.name());
            }
        }
    }

    pub(super) fn recovery_window(&mut self, ctx: &egui::Context) {
        if self.autosave.found.is_empty() {
            return;
        }
        let dir = self.autosave.recovery.as_ref().map(|r| r.dir().display().to_string()).unwrap_or_default();
        let mut action = None;
        egui::Window::new("Recover unsaved work")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.set_max_width(460.0);
                ui.label("AnvilCAD did not close normally last time. It kept copies of these documents' unsaved changes:");
                ui.add_space(6.0);
                for (i, f) in self.autosave.found.iter().enumerate() {
                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.strong(f.name());
                        if f.saved_at.is_some() {
                            ui.weak(format!("autosaved {}", ago(f.saved_at)));
                        }
                    });
                    if let Some(p) = &f.original {
                        ui.weak(p.display().to_string());
                    }
                    match &f.content {
                        Ok(_) => {
                            ui.horizontal(|ui| {
                                if ui.button("Restore").on_hover_text("Open this work; then save it").clicked() {
                                    action = Some((i, true));
                                }
                                if ui.button("Discard").on_hover_text("Delete this copy for good").clicked() {
                                    action = Some((i, false));
                                }
                            });
                        }
                        Err(why) => {
                            ui.label(why);
                            ui.label(format!("The file is kept in {dir}."));
                            if ui.button("Dismiss").clicked() {
                                action = Some((i, false));
                            }
                        }
                    }
                }
            });
        match action {
            Some((i, true)) => self.restore_recovered(i),
            Some((i, false)) => self.discard_recovered(i),
            None => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anvil_kernel::v2;
    use anvil_model::{FeatureKind, PlaneRef, SketchFeature};

    fn temp_dir(tag: &str) -> PathBuf {
        let nanos =
            SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        let d = std::env::temp_dir().join(format!("anvil-autosave-{tag}-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&d).expect("temp dir");
        d
    }

    /// An app with autosave on in `dir/recovery`, editing a part saved as `dir/block.anvprt`.
    fn app_with_saved_part(dir: &Path) -> (egui::Context, AnvilApp, PathBuf) {
        let ctx = egui::Context::default();
        let mut app = AnvilApp::new(&ctx, None, None, None);
        let file = dir.join("block.anvprt");
        Document::new().save(&file).expect("save");
        app.open_file(&file);
        app.enable_recovery(&dir.join("recovery")).expect("recovery");
        assert!(!app.is_dirty());
        (ctx, app, file)
    }

    fn edit(app: &mut AnvilApp) {
        let mut sf = SketchFeature::new(PlaneRef::Front);
        sf.sketch.add_rectangle(v2(0.0, 0.0), v2(40.0, 20.0));
        app.doc.add_feature(FeatureKind::Sketch(sf));
    }

    #[test]
    fn autosave_writes_a_copy_and_never_the_users_file() {
        let dir = temp_dir("write");
        let (_ctx, mut app, file) = app_with_saved_part(&dir);
        let before = std::fs::read(&file).expect("read");
        edit(&mut app);
        app.autosave_now();
        let copy = app.recovery_file().expect("autosave on").to_path_buf();
        assert!(copy.exists());
        assert_eq!(std::fs::read(&file).expect("read"), before, "the user's file is untouched");
        assert_eq!(recovery::read(&copy).content, Ok(Content::Part(app.doc.clone())));
        app.finish_recovery();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn after_a_crash_restore_brings_back_the_exact_feature_tree() {
        let dir = temp_dir("restore");
        let (_ctx, mut app, file) = app_with_saved_part(&dir);
        edit(&mut app);
        let tree = app.doc.features.clone();
        app.autosave_now();
        let old_copy = app.recovery_file().expect("on").to_path_buf();
        // A crash: no clean exit, so the copy stays and the lock is let go.
        drop(app);

        let ctx = egui::Context::default();
        let mut app = AnvilApp::new(&ctx, None, None, None);
        app.enable_recovery(&dir.join("recovery")).expect("recovery");
        assert_eq!(app.autosave.found.len(), 1);
        assert_eq!(app.autosave.found[0].name(), "block.anvprt");
        app.restore_recovered(0);
        assert_eq!(app.doc.features, tree);
        assert_eq!(app.path.as_deref(), Some(file.as_path()));
        assert!(app.is_dirty(), "restored work is unsaved until the user saves it");
        assert!(app.autosave.found.is_empty());
        // The work now lives in this instance's copy; the old one is gone.
        assert!(!old_copy.exists());
        assert!(app.recovery_file().is_some_and(|p| p.exists()));
        app.finish_recovery();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_clean_save_and_a_clean_exit_remove_the_copy() {
        let dir = temp_dir("save");
        let (ctx, mut app, _file) = app_with_saved_part(&dir);
        edit(&mut app);
        app.autosave_now();
        let copy = app.recovery_file().expect("on").to_path_buf();
        assert!(copy.exists());
        app.save(false);
        app.autosave_tick(&ctx);
        app.autosave_now();
        assert!(!copy.exists(), "saved, so nothing to recover");

        edit(&mut app);
        app.autosave_now();
        assert!(copy.exists());
        app.finish_recovery();
        assert!(!copy.exists() && !copy.with_extension("lock").exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_damaged_recovery_file_is_reported_and_kept() {
        let dir = temp_dir("damaged");
        let rec = dir.join("recovery");
        std::fs::create_dir_all(&rec).expect("dir");
        let bad = rec.join("session-1-1.json");
        std::fs::write(&bad, "{ half a file").expect("write");
        let ctx = egui::Context::default();
        let mut app = AnvilApp::new(&ctx, None, None, None);
        app.enable_recovery(&rec).expect("recovery");
        assert_eq!(app.autosave.found.len(), 1);
        assert_eq!(
            app.autosave.found[0].content.as_ref().err().map(String::as_str),
            Some("The recovery file is damaged and cannot be opened.")
        );
        // Restore is not offered for it; Dismiss hides it and keeps the file.
        app.restore_recovered(0);
        assert_eq!(app.autosave.found.len(), 1);
        app.discard_recovered(0);
        assert!(app.autosave.found.is_empty() && bad.exists());
        app.finish_recovery();
        let _ = std::fs::remove_dir_all(dir);
    }
}
