//! Why opening, saving, importing or exporting a file failed, in words a user can act on
//! (#199). A [`FileFailure`] has the shape of [`anvil_model::FeatureError`]: what failed
//! (the file and what was being done with it), why, and what to try. The error the
//! system or the reader gave, and the full path, go in `details`, which the app folds
//! away under Details; they never replace the plain message.

use anvil_drawing::DrawingFileError;
use anvil_model::step::{StepError, StepParseError};
use anvil_model::FileError;
use serde::Serialize;
use std::io::ErrorKind;
use std::path::Path;

/// What was being done with the file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum FileOp {
    Open,
    Save,
    Import,
    Export,
}

impl FileOp {
    fn verb(self) -> &'static str {
        match self {
            FileOp::Open => "open",
            FileOp::Save => "save",
            FileOp::Import => "import",
            FileOp::Export => "export",
        }
    }

    /// Whether it writes the file, rather than reads it.
    fn writes(self) -> bool {
        matches!(self, FileOp::Save | FileOp::Export)
    }
}

/// The kind of AnvilCAD file, for messages about one that does not read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Part,
    Assembly,
    Drawing,
}

impl Kind {
    fn noun(self) -> &'static str {
        match self {
            Kind::Part => "part",
            Kind::Assembly => "assembly",
            Kind::Drawing => "drawing",
        }
    }
}

/// A file that could not be opened, saved, imported or exported, explained.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct FileFailure {
    pub op: FileOp,
    /// The file's name, without its folder: "bracket.anvprt".
    pub file: String,
    /// Why, as a sentence fragment: "the disk is full".
    pub cause: String,
    /// What to try, as a full sentence.
    pub suggestion: String,
    /// The full path and the system's or reader's own words, for Details.
    pub details: Option<String>,
}

impl FileFailure {
    fn new(
        op: FileOp,
        path: &Path,
        cause: impl Into<String>,
        suggestion: impl Into<String>,
        error: impl std::fmt::Display,
    ) -> FileFailure {
        let file =
            path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
        FileFailure {
            op,
            file,
            cause: cause.into(),
            suggestion: suggestion.into(),
            details: Some(format!("{}\n{error}", path.display())),
        }
    }

    /// The window's title: "Could not open bracket.anvprt".
    pub fn title(&self) -> String {
        format!("Could not {} {}", self.op.verb(), self.file)
    }

    /// The cause as a sentence on its own: "The disk is full.".
    pub fn reason(&self) -> String {
        let mut c = self.cause.chars();
        c.next().map_or_else(String::new, |first| format!("{}{}.", first.to_uppercase(), c.as_str()))
    }

    /// The whole message: "Could not open bracket.anvprt: … Try …".
    pub fn message(&self) -> String {
        format!("{}: {}. {}", self.title(), self.cause, self.suggestion)
    }

    /// The system could not read or write the file.
    pub fn io(op: FileOp, path: &Path, e: &std::io::Error) -> FileFailure {
        let (cause, suggestion) = io_words(op, e);
        FileFailure::new(op, path, cause, suggestion, e)
    }

    /// An AnvilCAD part or assembly file could not be read or written.
    pub fn file(op: FileOp, kind: Kind, path: &Path, e: &FileError) -> FileFailure {
        match e {
            FileError::Io(io) => FileFailure::io(op, path, io),
            FileError::Json(_) => damaged(op, kind, path, e),
            FileError::WrongFormat(_) => wrong_kind(op, kind, path, e),
            FileError::TooNew(_) => too_new(op, kind, path, e),
        }
    }

    /// An AnvilCAD drawing file could not be read or written.
    pub fn drawing(op: FileOp, path: &Path, e: &DrawingFileError) -> FileFailure {
        let kind = Kind::Drawing;
        match e {
            DrawingFileError::Io(io) => FileFailure::io(op, path, io),
            DrawingFileError::Json(_) => damaged(op, kind, path, e),
            DrawingFileError::WrongFormat(_) => wrong_kind(op, kind, path, e),
            DrawingFileError::TooNew(_) => too_new(op, kind, path, e),
        }
    }

    /// A STEP file could not be imported.
    pub fn step(path: &Path, e: &StepError) -> FileFailure {
        let op = FileOp::Import;
        match e {
            StepError::Parse(StepParseError::NotStep) => FileFailure::new(
                op,
                path,
                "it is not a STEP file",
                "Check it is the file you meant, or export it again as STEP (AP203, AP214 or AP242) from the program that made it.",
                e,
            ),
            StepError::Parse(StepParseError::Syntax { line, .. }) => FileFailure::new(
                op,
                path,
                format!("the STEP file is damaged at line {line}"),
                "Export it again from the program that made it; the file may have been cut short or edited by hand.",
                e,
            ),
            StepError::NoBodies(_) => FileFailure::new(
                op,
                path,
                "it holds no solid or surface bodies, only things AnvilCAD does not import, such as curves or an empty assembly",
                "Export it again with solids or surfaces included.",
                e,
            ),
        }
    }
}

/// Why the system could not read or write a file, and what to try.
fn io_words(op: FileOp, e: &std::io::Error) -> (String, String) {
    let write = op.writes();
    let (cause, suggestion): (&str, &str) = match e.kind() {
        // Windows reports a file another program holds open as a sharing or lock violation.
        _ if cfg!(windows) && matches!(e.raw_os_error(), Some(32 | 33)) => {
            ("another program has the file open", "Close the file in the other program, then try again.")
        }
        ErrorKind::NotFound if write => {
            ("the folder it goes in does not exist", "Pick a folder that exists, or create the folder first.")
        }
        ErrorKind::NotFound => (
            "the file does not exist; it may have been moved, renamed or deleted",
            "Check the file's name and folder, then try again.",
        ),
        ErrorKind::PermissionDenied if write => (
            "you do not have permission to write there, or the file is read-only",
            "Pick another folder, or clear the file's read-only setting.",
        ),
        ErrorKind::PermissionDenied => (
            "you do not have permission to read it",
            "Ask the file's owner for access, or copy it to a folder you can read.",
        ),
        ErrorKind::ReadOnlyFilesystem => {
            ("the disk or folder it goes on is read-only", "Pick a folder on another disk.")
        }
        ErrorKind::StorageFull | ErrorKind::QuotaExceeded => {
            ("the disk is full", "Free some space on it, or pick a folder on another disk.")
        }
        ErrorKind::IsADirectory => ("it is a folder, not a file", "Pick a file instead."),
        ErrorKind::ResourceBusy => {
            ("another program has the file open", "Close the file in the other program, then try again.")
        }
        ErrorKind::InvalidInput if op == FileOp::Export => (
            "AnvilCAD does not write files of this type",
            "Pick STEP, 3MF, STL or OBJ as the type, or end the name with .step, .3mf, .stl or .obj.",
        ),
        // A text file that is not UTF-8: not a file AnvilCAD wrote.
        ErrorKind::InvalidData if !write => (
            "it is not a file AnvilCAD can read, or it is damaged",
            "Check it is the file you meant, or open a backup copy of it.",
        ),
        _ if write => (
            "the computer could not write it",
            "Try again, or pick another folder; the error under Details says more.",
        ),
        _ => (
            "the computer could not read it",
            "Try again, or copy the file to another folder first; the error under Details says more.",
        ),
    };
    (cause.into(), suggestion.into())
}

fn damaged(op: FileOp, kind: Kind, path: &Path, e: impl std::fmt::Display) -> FileFailure {
    let noun = kind.noun();
    if op.writes() {
        // Turning the file into text failed: a fault in AnvilCAD, not in the disk.
        return FileFailure::new(
            op,
            path,
            format!("the {noun} could not be turned into its file format"),
            "Undo the last change and save again; if it keeps failing, report it on GitHub with the error under Details.",
            e,
        );
    }
    FileFailure::new(
        op,
        path,
        format!("the file is damaged, or is not an AnvilCAD {noun} file"),
        "Open a backup copy of it, or check it is the file you meant.",
        e,
    )
}

fn wrong_kind(op: FileOp, kind: Kind, path: &Path, e: impl std::fmt::Display) -> FileFailure {
    let noun = kind.noun();
    FileFailure::new(
        op,
        path,
        format!("its name says it is an AnvilCAD {noun}, but it holds something else"),
        "Check it is the file you meant; a file renamed from another type cannot be opened this way.",
        e,
    )
}

fn too_new(op: FileOp, kind: Kind, path: &Path, e: impl std::fmt::Display) -> FileFailure {
    let noun = kind.noun();
    FileFailure::new(
        op,
        path,
        format!("the {noun} was saved by a newer version of AnvilCAD"),
        "Update AnvilCAD to open it.",
        e,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Error;

    fn check(f: &FileFailure) {
        let m = f.message();
        assert!(!f.cause.ends_with('.') && f.suggestion.ends_with('.'), "{m}");
        // No raw error text or debug output in the message itself.
        for t in ["os error", "{", "}", "::", "Some(", "\""] {
            assert!(!f.cause.contains(t) && !f.suggestion.contains(t), "{m}");
        }
    }

    #[test]
    fn a_missing_file_says_what_failed_and_what_to_do() {
        let p = Path::new("/work/parts/bracket.anvprt");
        let e = Error::from(ErrorKind::NotFound);
        let f = FileFailure::io(FileOp::Open, p, &e);
        check(&f);
        assert_eq!(f.title(), "Could not open bracket.anvprt");
        assert_eq!(
            f.message(),
            "Could not open bracket.anvprt: the file does not exist; it may have been moved, renamed or deleted. \
             Check the file's name and folder, then try again."
        );
        // The full path and the system's words are kept, under Details.
        assert_eq!(f.details.as_deref(), Some(format!("/work/parts/bracket.anvprt\n{e}").as_str()));
        assert_eq!(f.reason(), "The file does not exist; it may have been moved, renamed or deleted.");
        // Saving into a folder that is not there says so instead.
        let f = FileFailure::io(FileOp::Save, p, &e);
        check(&f);
        assert_eq!(f.title(), "Could not save bracket.anvprt");
        assert_eq!(f.cause, "the folder it goes in does not exist");
    }

    #[test]
    fn each_io_error_has_its_own_words() {
        let p = Path::new("plate.step");
        let kinds = [
            (
                FileOp::Save,
                ErrorKind::PermissionDenied,
                "you do not have permission to write there, or the file is read-only",
            ),
            (FileOp::Open, ErrorKind::PermissionDenied, "you do not have permission to read it"),
            (FileOp::Export, ErrorKind::StorageFull, "the disk is full"),
            (FileOp::Save, ErrorKind::ReadOnlyFilesystem, "the disk or folder it goes on is read-only"),
            (FileOp::Open, ErrorKind::IsADirectory, "it is a folder, not a file"),
            (FileOp::Open, ErrorKind::InvalidData, "it is not a file AnvilCAD can read, or it is damaged"),
            (FileOp::Export, ErrorKind::Interrupted, "the computer could not write it"),
            (FileOp::Import, ErrorKind::Interrupted, "the computer could not read it"),
        ];
        for (op, kind, cause) in kinds {
            let f = FileFailure::io(op, p, &Error::from(kind));
            check(&f);
            assert_eq!(f.cause, cause, "{op:?} {kind:?}");
        }
        // The export of an unknown type names the types there are.
        let e = Error::new(ErrorKind::InvalidInput, "unsupported export format '.igs'");
        let f = FileFailure::io(FileOp::Export, Path::new("plate.igs"), &e);
        check(&f);
        assert_eq!(f.cause, "AnvilCAD does not write files of this type");
        assert!(f.details.as_deref().is_some_and(|d| d.contains(".igs")));
    }

    #[test]
    fn part_assembly_and_drawing_files_that_do_not_read_say_why() {
        let p = Path::new("bracket.anvprt");
        let json = serde_json::from_str::<serde_json::Value>("{").expect_err("bad json");
        let f = FileFailure::file(FileOp::Open, Kind::Part, p, &FileError::Json(json));
        check(&f);
        assert_eq!(f.cause, "the file is damaged, or is not an AnvilCAD part file");
        assert!(f.details.as_deref().is_some_and(|d| d.contains("line 1")), "{:?}", f.details);
        let f = FileFailure::file(FileOp::Open, Kind::Assembly, Path::new("a.anvasm"), &FileError::TooNew(9));
        check(&f);
        assert_eq!(f.cause, "the assembly was saved by a newer version of AnvilCAD");
        assert_eq!(f.suggestion, "Update AnvilCAD to open it.");
        assert!(f.details.as_deref().is_some_and(|d| d.contains('9')));
        let f = FileFailure::drawing(
            FileOp::Open,
            Path::new("d.anvdrw"),
            &DrawingFileError::WrongFormat("x".into()),
        );
        check(&f);
        assert_eq!(f.cause, "its name says it is an AnvilCAD drawing, but it holds something else");
        // An I/O error inside a part error is said as one.
        let f = FileFailure::file(
            FileOp::Save,
            Kind::Part,
            p,
            &FileError::Io(Error::from(ErrorKind::StorageFull)),
        );
        assert_eq!(f.cause, "the disk is full");
    }

    #[test]
    fn step_files_that_do_not_import_say_why() {
        let p = Path::new("/in/plate.step");
        let not_step =
            anvil_model::step::document_from_step("hello", "plate.step").map(|_| ()).expect_err("not step");
        let f = FileFailure::step(p, &not_step);
        check(&f);
        assert_eq!(f.title(), "Could not import plate.step");
        assert_eq!(f.cause, "it is not a STEP file");
        let empty = "ISO-10303-21;\nHEADER;\nENDSEC;\nDATA;\nENDSEC;\nEND-ISO-10303-21;\n";
        let none =
            anvil_model::step::document_from_step(empty, "plate.step").map(|_| ()).expect_err("no bodies");
        let f = FileFailure::step(p, &none);
        check(&f);
        assert!(f.cause.starts_with("it holds no solid or surface bodies"), "{}", f.cause);
        let broken = "ISO-10303-21;\nHEADER;\nENDSEC;\nDATA;\n#1=CARTESIAN_POINT('',(0.,0.,0.);\nENDSEC;\nEND-ISO-10303-21;\n";
        if let Err(e @ StepError::Parse(StepParseError::Syntax { line, .. })) =
            anvil_model::step::document_from_step(broken, "plate.step").map(|_| ())
        {
            let f = FileFailure::step(p, &e);
            check(&f);
            assert_eq!(f.cause, format!("the STEP file is damaged at line {line}"));
            assert_eq!(line, 5);
        } else {
            panic!("a syntax error was expected");
        }
    }
}
