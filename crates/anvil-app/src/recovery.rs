//! Autosave and crash recovery (issue #86).
//!
//! While a document has unsaved changes, the app writes a copy of it to a recovery folder
//! in the user's data directory, never to the user's own file. Each running instance owns
//! one recovery file, `session-<pid>-<time>.json`, and holds an exclusive lock on a
//! matching `.lock` file for as long as it runs. A clean save, or undoing back to the
//! saved state, removes the recovery file; a clean exit removes both files.
//!
//! So after a crash the recovery file is still there and nobody holds its lock, and the
//! next start offers it back. A file whose lock is held belongs to another instance that
//! is still running, and is left alone.
//!
//! Writes happen on a background thread, one at a time and in order, and each is atomic
//! (a temporary file renamed over the old one), so a crash during an autosave leaves the
//! previous copy intact.

use anvil_drawing::Drawing;
use anvil_model::assembly::Assembly;
use anvil_model::document::Document;
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// The `format` field of a recovery file.
pub const RECOVERY_FORMAT: &str = "anvil-recovery";
/// The newest recovery file version this build reads and writes.
pub const RECOVERY_VERSION: u32 = 1;
/// How often unsaved work is autosaved unless `anvil.opt.autosave` says otherwise.
pub const DEFAULT_INTERVAL: Duration = Duration::from_secs(120);

/// The recovery folder: `$ANVILCAD_RECOVERY_DIR` if set; otherwise
/// `%LOCALAPPDATA%\AnvilCAD\Recovery` on Windows,
/// `~/Library/Application Support/AnvilCAD/Recovery` on macOS, and
/// `$XDG_DATA_HOME/anvilcad/recovery` (default `~/.local/share/anvilcad/recovery`) elsewhere.
/// `env` reads an environment variable.
pub fn recovery_dir(env: impl Fn(&str) -> Option<String>) -> Option<PathBuf> {
    let set = |k: &str| env(k).filter(|v| !v.is_empty());
    if let Some(p) = set("ANVILCAD_RECOVERY_DIR") {
        return Some(PathBuf::from(p));
    }
    if cfg!(windows) {
        return set("LOCALAPPDATA").map(|d| Path::new(&d).join("AnvilCAD").join("Recovery"));
    }
    if cfg!(target_os = "macos") {
        return set("HOME").map(|h| {
            Path::new(&h).join("Library").join("Application Support").join("AnvilCAD").join("Recovery")
        });
    }
    let base = set("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| set("HOME").map(|h| Path::new(&h).join(".local").join("share")))?;
    Some(base.join("anvilcad").join("recovery"))
}

/// The recovery folder for this process.
pub fn default_recovery_dir() -> Option<PathBuf> {
    recovery_dir(|k| std::env::var(k).ok())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Part,
    Assembly,
    Drawing,
}

/// What is autosaved: the document, and the file it came from (if it has been saved).
#[derive(Clone, Debug, PartialEq)]
pub enum Content {
    Part(Document),
    Assembly(Assembly),
    Drawing(Drawing),
}

/// A recovery file on disk. The document is kept as the text its own format writes, so
/// reading it back goes through the same checks as opening a file.
#[derive(Serialize, Deserialize)]
struct Envelope {
    format: String,
    version: u32,
    kind: Kind,
    /// The user's file, when the document had been saved before.
    original: Option<PathBuf>,
    /// Seconds since 1970 when this copy was written.
    saved_at: u64,
    document: String,
}

/// A recovery file left by an instance that did not exit cleanly.
#[derive(Debug)]
pub struct Found {
    pub path: PathBuf,
    /// The user's file the work belongs to, if it had been saved.
    pub original: Option<PathBuf>,
    pub saved_at: Option<SystemTime>,
    /// The document, or why it cannot be read, in plain words.
    pub content: Result<Content, String>,
}

impl Found {
    /// The document's name, as the title bar shows it.
    pub fn name(&self) -> String {
        let untitled = match &self.content {
            Ok(Content::Part(_)) => "Part1",
            Ok(Content::Assembly(_)) => "Assembly1",
            Ok(Content::Drawing(_)) => "Drawing1",
            // Nothing is known about it but its file.
            Err(_) if self.original.is_none() => {
                let file =
                    self.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                return format!("Recovery file {file}");
            }
            Err(_) => "",
        };
        self.original
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| format!("{untitled} (never saved)"))
    }
}

/// Reads a recovery file. Never fails: what cannot be read is reported in `content`.
pub fn read(path: &Path) -> Found {
    let mut found =
        Found { path: path.to_path_buf(), original: None, saved_at: None, content: Err(String::new()) };
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) => {
            found.content = Err(format!("The recovery file could not be read ({e})."));
            return found;
        }
    };
    let env: Envelope = match serde_json::from_str(&text) {
        Ok(e) => e,
        Err(_) => {
            found.content = Err("The recovery file is damaged and cannot be opened.".into());
            return found;
        }
    };
    found.original = env.original;
    found.saved_at = UNIX_EPOCH.checked_add(Duration::from_secs(env.saved_at));
    if env.format != RECOVERY_FORMAT {
        found.content = Err("This is not an AnvilCAD recovery file.".into());
        return found;
    }
    if env.version > RECOVERY_VERSION {
        found.content = Err(format!(
            "The recovery file was written by a newer AnvilCAD (recovery version {}). Open it with that version.",
            env.version
        ));
        return found;
    }
    found.content = match env.kind {
        Kind::Part => Document::from_json(&env.document).map(Content::Part).map_err(|e| e.to_string()),
        Kind::Assembly => {
            Assembly::from_json(&env.document).map(Content::Assembly).map_err(|e| e.to_string())
        }
        Kind::Drawing => Drawing::from_json(&env.document).map(Content::Drawing).map_err(|e| e.to_string()),
    }
    .map_err(|e| format!("The recovered document cannot be opened: {e}."));
    found
}

/// The lock file that marks `file` as owned by a running instance.
fn lock_path(file: &Path) -> PathBuf {
    file.with_extension("lock")
}

/// Recovery files in `dir` whose instance is no longer running, oldest first. A missing
/// folder has none.
pub fn scan(dir: &Path, skip: Option<&Path>) -> Vec<Found> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut found: Vec<Found> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "json") && Some(p.as_path()) != skip)
        .filter(|p| !owner_running(p))
        .map(|p| read(&p))
        .collect();
    found.sort_by_key(|f| f.saved_at);
    found
}

/// True when another instance holds the lock for `file`.
fn owner_running(file: &Path) -> bool {
    let Ok(lock) = OpenOptions::new().write(true).open(lock_path(file)) else { return false };
    matches!(lock.try_lock(), Err(TryLockError::WouldBlock))
}

/// Deletes a recovery file the user discarded or restored, and its lock file.
pub fn remove(file: &Path) -> std::io::Result<()> {
    let _ = std::fs::remove_file(lock_path(file));
    match std::fs::remove_file(file) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

enum Job {
    Write {
        content: Box<Content>,
        original: Option<PathBuf>,
    },
    Remove,
    /// Replies once every job before it is done.
    Sync(Sender<()>),
    Finish,
}

/// This instance's recovery file and the thread that writes it.
pub struct Recovery {
    dir: PathBuf,
    file: PathBuf,
    jobs: Sender<Job>,
    worker: Option<JoinHandle<()>>,
    /// The last write's failure, for the status bar.
    error: Arc<Mutex<Option<String>>>,
    /// Held for the life of the instance; see the module comment.
    lock: Option<File>,
}

impl Recovery {
    /// Claims a new recovery file in `dir`, creating the folder if needed.
    pub fn start(dir: &Path) -> std::io::Result<Recovery> {
        std::fs::create_dir_all(dir)?;
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        let file = dir.join(format!("session-{}-{nanos}.json", std::process::id()));
        let lock = OpenOptions::new().create(true).truncate(false).write(true).open(lock_path(&file))?;
        lock.try_lock().map_err(std::io::Error::other)?;
        let (jobs, rx) = channel::<Job>();
        let error = Arc::new(Mutex::new(None));
        let (target, err) = (file.clone(), error.clone());
        let worker = std::thread::Builder::new().name("anvilcad-autosave".into()).spawn(move || {
            for job in rx {
                let result = match job {
                    Job::Write { content, original } => write(&target, &content, original),
                    Job::Remove => match std::fs::remove_file(&target) {
                        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
                        _ => Ok(()),
                    },
                    Job::Sync(reply) => {
                        let _ = reply.send(());
                        continue;
                    }
                    Job::Finish => {
                        let _ = std::fs::remove_file(&target);
                        break;
                    }
                };
                if let Ok(mut slot) = err.lock() {
                    *slot = result.err();
                }
            }
        })?;
        Ok(Recovery { dir: dir.to_path_buf(), file, jobs, worker: Some(worker), error, lock: Some(lock) })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn file(&self) -> &Path {
        &self.file
    }

    /// Queues a copy of `content` to be written.
    pub fn save(&self, content: Content, original: Option<PathBuf>) {
        let _ = self.jobs.send(Job::Write { content: Box::new(content), original });
    }

    /// Queues removal of the recovery file: the work is saved or discarded.
    pub fn clear(&self) {
        let _ = self.jobs.send(Job::Remove);
    }

    /// Why the last autosave failed, if it did.
    pub fn last_error(&self) -> Option<String> {
        self.error.lock().ok().and_then(|e| e.clone())
    }

    /// Waits until every job queued so far is done.
    pub fn flush(&self) {
        let (tx, rx) = channel::<()>();
        if self.jobs.send(Job::Sync(tx)).is_ok() {
            let _ = rx.recv();
        }
    }

    /// A clean exit: removes the recovery file and lock, after any queued writes.
    pub fn finish(mut self) {
        let _ = self.jobs.send(Job::Finish);
        if let Some(w) = self.worker.take() {
            let _ = w.join();
        }
        // Let go of the lock before deleting its file: Windows will not delete a file
        // another handle has locked.
        self.lock = None;
        let _ = std::fs::remove_file(lock_path(&self.file));
    }
}

fn write(target: &Path, content: &Content, original: Option<PathBuf>) -> Result<(), String> {
    let (kind, document) = match content {
        Content::Part(d) => (Kind::Part, d.to_json().map_err(|e| e.to_string())),
        Content::Assembly(a) => (Kind::Assembly, a.to_json().map_err(|e| e.to_string())),
        Content::Drawing(d) => (Kind::Drawing, d.to_json().map_err(|e| e.to_string())),
    };
    let document = document?;
    let saved_at = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let env = Envelope {
        format: RECOVERY_FORMAT.into(),
        version: RECOVERY_VERSION,
        kind,
        original,
        saved_at,
        document,
    };
    let text = serde_json::to_string(&env).map_err(|e| e.to_string())?;
    let tmp = target.with_extension("json.tmp");
    std::fs::write(&tmp, text).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, target).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use anvil_kernel::v2;
    use anvil_model::{FeatureKind, PlaneRef, SketchFeature};

    fn temp_dir(tag: &str) -> PathBuf {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        let d = std::env::temp_dir().join(format!("anvil-recovery-{tag}-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&d).expect("temp dir");
        d
    }

    fn part() -> Document {
        let mut doc = Document::new();
        let mut sf = SketchFeature::new(PlaneRef::Front);
        sf.sketch.add_rectangle(v2(0.0, 0.0), v2(40.0, 20.0));
        doc.add_feature(FeatureKind::Sketch(sf));
        doc
    }

    #[test]
    fn the_folder_follows_each_platform() {
        let env = |pairs: &'static [(&'static str, &'static str)]| {
            move |k: &str| pairs.iter().find(|(n, _)| *n == k).map(|(_, v)| v.to_string())
        };
        assert_eq!(
            recovery_dir(env(&[("ANVILCAD_RECOVERY_DIR", "/r"), ("HOME", "/h")])),
            Some(PathBuf::from("/r"))
        );
        let home = recovery_dir(env(&[("HOME", "/h"), ("LOCALAPPDATA", r"C:\L")]));
        let expected = if cfg!(windows) {
            Path::new(r"C:\L").join("AnvilCAD").join("Recovery")
        } else if cfg!(target_os = "macos") {
            PathBuf::from("/h/Library/Application Support/AnvilCAD/Recovery")
        } else {
            PathBuf::from("/h/.local/share/anvilcad/recovery")
        };
        assert_eq!(home, Some(expected));
    }

    #[test]
    fn a_running_instance_is_skipped_and_a_crashed_one_is_found() {
        let dir = temp_dir("scan");
        let original = dir.join("bracket.anvprt");
        let live = Recovery::start(&dir).expect("start");
        live.save(Content::Part(part()), Some(original.clone()));
        live.flush();
        assert_eq!(live.last_error(), None);
        // This instance holds the lock, so its own copy is not offered.
        assert!(scan(&dir, None).is_empty());

        // A crash: the process ends without `finish`, which here means the lock is let go
        // while the file stays.
        let file = live.file().to_path_buf();
        drop(live);
        let found = scan(&dir, None);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].path, file);
        assert_eq!(found[0].original, Some(original));
        assert_eq!(found[0].name(), "bracket.anvprt");
        assert_eq!(found[0].content, Ok(Content::Part(part())));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn clear_and_a_clean_exit_remove_the_copy() {
        let dir = temp_dir("clear");
        let r = Recovery::start(&dir).expect("start");
        r.save(Content::Part(part()), None);
        r.flush();
        assert!(r.file().exists());
        r.clear();
        r.flush();
        assert!(!r.file().exists());
        r.save(Content::Part(part()), None);
        let (file, lock) = (r.file().to_path_buf(), lock_path(r.file()));
        r.finish();
        assert!(!file.exists() && !lock.exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn damaged_and_newer_files_are_reported_in_plain_words() {
        let dir = temp_dir("bad");
        let damaged = dir.join("session-1-1.json");
        std::fs::write(&damaged, "{ not json").expect("write");
        let newer = dir.join("session-2-2.json");
        let env = Envelope {
            format: RECOVERY_FORMAT.into(),
            version: RECOVERY_VERSION + 1,
            kind: Kind::Part,
            original: None,
            saved_at: 0,
            document: String::new(),
        };
        std::fs::write(&newer, serde_json::to_string(&env).expect("json")).expect("write");
        let found = scan(&dir, None);
        let msg =
            |p: &Path| found.iter().find(|f| f.path == p).and_then(|f| f.content.as_ref().err().cloned());
        assert_eq!(msg(&damaged).as_deref(), Some("The recovery file is damaged and cannot be opened."));
        assert!(
            msg(&newer).is_some_and(|m| m.starts_with("The recovery file was written by a newer AnvilCAD"))
        );
        // Reading never deletes anything.
        assert!(damaged.exists() && newer.exists());
        let _ = std::fs::remove_dir_all(dir);
    }
}
