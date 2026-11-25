//! The drawing document: one sheet, the part it shows, and its views, saved as versioned
//! JSON in a `.anvdrw` file (see `docs/re/drawings.md`, "The drawing document").
//!
//! A drawing holds no geometry of its own. It refers to a part file by a path relative to
//! the drawing, and its views are projected from that part's exact bodies whenever the part
//! is loaded or changes.

use crate::hlr::Orientation;
use crate::sheet::{Projection, Sheet};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const DRAWING_FORMAT: &str = "anvil-drawing";
pub const DRAWING_FORMAT_VERSION: u32 = 1;
pub const DRAWING_EXT: &str = "anvdrw";

/// What a view is for, which decides its direction and where it sits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ViewRole {
    /// The front view, which the projected views align to.
    Front,
    /// The projected view of the top, above (third angle) or below (first angle) the front.
    Top,
    /// The projected side view: the right view to the right of the front in third-angle
    /// projection, the left view to the right of the front in first-angle projection.
    Side,
    /// An isometric view, in the corner the other views leave free.
    Isometric,
}

impl ViewRole {
    pub fn orientation(self, projection: Projection) -> Orientation {
        match (self, projection) {
            (ViewRole::Front, _) => Orientation::Front,
            (ViewRole::Top, _) => Orientation::Top,
            (ViewRole::Side, Projection::ThirdAngle) => Orientation::Right,
            (ViewRole::Side, Projection::FirstAngle) => Orientation::Left,
            (ViewRole::Isometric, _) => Orientation::Isometric,
        }
    }
}

/// Which lines a view draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DisplayStyle {
    /// Only the lines seen from the view's direction.
    HiddenRemoved,
    /// Seen lines solid, hidden lines dashed.
    HiddenDashed,
    /// Every line solid, seen or not.
    Wireframe,
}

impl DisplayStyle {
    pub const ALL: [DisplayStyle; 3] =
        [DisplayStyle::HiddenRemoved, DisplayStyle::HiddenDashed, DisplayStyle::Wireframe];

    pub fn label(self) -> &'static str {
        match self {
            DisplayStyle::HiddenRemoved => "Hidden lines removed",
            DisplayStyle::HiddenDashed => "Hidden lines dashed",
            DisplayStyle::Wireframe => "Wireframe",
        }
    }
}

/// How a view draws tangent edges: edges where faces meet smoothly, as a fillet meets the
/// faces beside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TangentEdges {
    /// Drawn like any other edge.
    Visible,
    /// Drawn as thin lines.
    Thin,
    /// Not drawn.
    Removed,
}

impl TangentEdges {
    pub const ALL: [TangentEdges; 3] = [TangentEdges::Visible, TangentEdges::Thin, TangentEdges::Removed];

    pub fn label(self) -> &'static str {
        match self {
            TangentEdges::Visible => "Visible",
            TangentEdges::Thin => "Thin lines",
            TangentEdges::Removed => "Removed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct View {
    pub id: u32,
    pub role: ViewRole,
    pub display: DisplayStyle,
    pub tangent: TangentEdges,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Drawing {
    pub format: String,
    pub format_version: u32,
    /// The part the views show: relative to the drawing's folder, with `/` separators, or
    /// absolute when the two are on different drives. Empty when no part is set yet.
    pub model: String,
    pub sheet: Sheet,
    pub views: Vec<View>,
    pub next_id: u32,
}

#[derive(Debug)]
pub enum DrawingFileError {
    Io(std::io::Error),
    Json(serde_json::Error),
    WrongFormat(String),
    TooNew(u32),
}

impl std::fmt::Display for DrawingFileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DrawingFileError::Io(e) => write!(f, "{e}"),
            DrawingFileError::Json(e) => write!(f, "file is damaged: {e}"),
            DrawingFileError::WrongFormat(s) => write!(f, "not an AnvilCAD drawing file (format '{s}')"),
            DrawingFileError::TooNew(v) => write!(
                f,
                "drawing format version {v} is newer than this build supports ({DRAWING_FORMAT_VERSION}); \
                 update AnvilCAD to open it"
            ),
        }
    }
}

impl std::error::Error for DrawingFileError {}

impl From<std::io::Error> for DrawingFileError {
    fn from(e: std::io::Error) -> Self {
        DrawingFileError::Io(e)
    }
}

impl From<serde_json::Error> for DrawingFileError {
    fn from(e: serde_json::Error) -> Self {
        DrawingFileError::Json(e)
    }
}

impl Default for Drawing {
    fn default() -> Self {
        Drawing::new(Sheet::default())
    }
}

impl Drawing {
    /// A drawing with no part and no views.
    pub fn new(sheet: Sheet) -> Drawing {
        Drawing {
            format: DRAWING_FORMAT.into(),
            format_version: DRAWING_FORMAT_VERSION,
            model: String::new(),
            sheet,
            views: Vec::new(),
            next_id: 1,
        }
    }

    /// A drawing of `model` with the three standard views and an isometric. Orthographic
    /// views have hidden lines and tangent edges removed; the isometric shows tangent
    /// edges, as drafting practice draws pictorial views.
    pub fn standard(model: String, sheet: Sheet) -> Drawing {
        let mut d = Drawing::new(sheet);
        d.model = model;
        for role in [ViewRole::Front, ViewRole::Top, ViewRole::Side, ViewRole::Isometric] {
            let tangent =
                if role == ViewRole::Isometric { TangentEdges::Visible } else { TangentEdges::Removed };
            d.add_view(role, DisplayStyle::HiddenRemoved, tangent);
        }
        d
    }

    pub fn add_view(&mut self, role: ViewRole, display: DisplayStyle, tangent: TangentEdges) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        self.views.push(View { id, role, display, tangent });
        id
    }

    pub fn view(&self, id: u32) -> Option<&View> {
        self.views.iter().find(|v| v.id == id)
    }

    pub fn view_mut(&mut self, id: u32) -> Option<&mut View> {
        self.views.iter_mut().find(|v| v.id == id)
    }

    /// The part file, found from the drawing's own path.
    pub fn model_path(&self, drawing_path: Option<&Path>) -> Option<PathBuf> {
        if self.model.is_empty() {
            return None;
        }
        let m = Path::new(&self.model);
        if m.is_absolute() {
            return Some(m.to_path_buf());
        }
        Some(match drawing_path.and_then(Path::parent) {
            Some(dir) => dir.join(m),
            None => m.to_path_buf(),
        })
    }

    /// Points the drawing at `part`, stored relative to `drawing_path`'s folder when they
    /// share one.
    pub fn set_model(&mut self, part: &Path, drawing_path: Option<&Path>) {
        self.model = relative_path(part, drawing_path.and_then(Path::parent));
    }

    pub fn to_json(&self) -> Result<String, DrawingFileError> {
        let mut d = self.clone();
        d.format = DRAWING_FORMAT.into();
        d.format_version = DRAWING_FORMAT_VERSION;
        Ok(serde_json::to_string_pretty(&d)?)
    }

    pub fn from_json(s: &str) -> Result<Drawing, DrawingFileError> {
        let v: serde_json::Value = serde_json::from_str(s)?;
        let format = v.get("format").and_then(|f| f.as_str()).unwrap_or("");
        if format != DRAWING_FORMAT {
            return Err(DrawingFileError::WrongFormat(format.into()));
        }
        let ver = v.get("format_version").and_then(|f| f.as_u64()).unwrap_or(0) as u32;
        if ver > DRAWING_FORMAT_VERSION {
            return Err(DrawingFileError::TooNew(ver));
        }
        let mut d: Drawing = serde_json::from_value(v)?;
        d.next_id = d.next_id.max(d.views.iter().map(|v| v.id + 1).max().unwrap_or(1));
        Ok(d)
    }

    /// Saves through a temporary file, so a crash never leaves a half-written drawing.
    pub fn save(&self, path: &Path) -> Result<(), DrawingFileError> {
        let tmp = path.with_extension(format!("{DRAWING_EXT}.tmp"));
        std::fs::write(&tmp, self.to_json()?)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    pub fn load(path: &Path) -> Result<Drawing, DrawingFileError> {
        Drawing::from_json(&std::fs::read_to_string(path)?)
    }
}

/// `target` relative to `base`, with `/` separators, or `target` as given when no relative
/// path reaches it (another drive on Windows).
fn relative_path(target: &Path, base: Option<&Path>) -> String {
    let slash =
        |p: &Path| p.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>();
    let Some(base) = base.filter(|b| !b.as_os_str().is_empty()) else {
        return target.to_string_lossy().replace('\\', "/");
    };
    let abs = |p: &Path| std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf());
    let (t, b) = (abs(target), abs(base));
    let (tc, bc) = (slash(&t), slash(&b));
    if tc.first() != bc.first() {
        return t.to_string_lossy().into_owned();
    }
    let common = tc.iter().zip(&bc).take_while(|(a, b)| a == b).count();
    let mut parts: Vec<String> = std::iter::repeat_n("..".to_string(), bc.len() - common).collect();
    parts.extend(tc[common..].iter().cloned());
    parts.join("/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sheet::{Scale, SheetSize};

    #[test]
    fn round_trips_through_json() {
        let sheet = Sheet {
            size: SheetSize::AnsiB,
            scale: Scale { sheet: 1, model: 2 },
            projection: Projection::ThirdAngle,
        };
        let mut d = Drawing::standard("parts/bracket.anvprt".into(), sheet);
        d.views[1].display = DisplayStyle::HiddenDashed;
        let text = d.to_json().expect("json");
        assert!(text.contains("\"format\": \"anvil-drawing\""));
        assert!(text.contains("\"format_version\": 1"));
        assert_eq!(Drawing::from_json(&text).expect("read"), d);
    }

    #[test]
    fn refuses_other_and_newer_files() {
        let part = r#"{"format": "anvil-part", "format_version": 1}"#;
        assert!(matches!(Drawing::from_json(part), Err(DrawingFileError::WrongFormat(_))));
        let text = Drawing::default()
            .to_json()
            .expect("json")
            .replace("\"format_version\": 1", "\"format_version\": 99");
        assert!(matches!(Drawing::from_json(&text), Err(DrawingFileError::TooNew(99))));
        assert!(matches!(Drawing::from_json("{"), Err(DrawingFileError::Json(_))));
    }

    #[test]
    fn side_view_follows_the_projection() {
        assert_eq!(ViewRole::Side.orientation(Projection::ThirdAngle), Orientation::Right);
        assert_eq!(ViewRole::Side.orientation(Projection::FirstAngle), Orientation::Left);
    }

    #[test]
    fn model_paths_are_relative_to_the_drawing() {
        let dir = std::env::temp_dir().join("anvil-drawing-paths");
        let mut d = Drawing::default();
        d.set_model(&dir.join("parts").join("p.anvprt"), Some(&dir.join("drw").join("d.anvdrw")));
        assert_eq!(d.model, "../parts/p.anvprt");
        assert_eq!(
            d.model_path(Some(&dir.join("drw").join("d.anvdrw"))),
            Some(dir.join("drw").join("../parts/p.anvprt"))
        );
        d.set_model(&dir.join("p.anvprt"), Some(&dir.join("d.anvdrw")));
        assert_eq!(d.model, "p.anvprt");
        assert_eq!(Drawing::default().model_path(None), None);
    }
}
