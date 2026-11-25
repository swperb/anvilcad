//! The drawing document: one sheet, the part it shows, and its views, saved as versioned
//! JSON in a `.anvdrw` file (see `docs/re/drawings.md`, "The drawing document").
//!
//! A drawing holds no geometry of its own. It refers to a part file by a path relative to
//! the drawing, and its views are projected from that part's exact bodies whenever the part
//! is loaded or changes. Dimensions refer to views by id and to the part's features by
//! id; their values come from the part every time the views update.

use crate::callout::HoleCallout;
use crate::detail::DetailDef;
use crate::dims::Dimension;
use crate::hlr::Orientation;
use crate::section::SectionDef;
use crate::sheet::{Projection, Scale, Sheet};
use crate::title::TitleBlock;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const DRAWING_FORMAT: &str = "anvil-drawing";
/// Version 2 added dimensions, the title block and the configuration shown; version 3
/// added section and detail views and hole callouts.
pub const DRAWING_FORMAT_VERSION: u32 = 3;
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
    /// A section view cut along a line on another view ([`View::section`]).
    Section,
    /// A detail view of a circle on another view ([`View::detail`]).
    Detail,
}

impl ViewRole {
    /// The direction a standard view looks from. Section and detail views take theirs from
    /// their cutting line and parent: use [`Drawing::view_orientation`] for any view.
    pub fn orientation(self, projection: Projection) -> Orientation {
        match (self, projection) {
            (ViewRole::Front, _) => Orientation::Front,
            (ViewRole::Top, _) => Orientation::Top,
            (ViewRole::Side, Projection::ThirdAngle) => Orientation::Right,
            (ViewRole::Side, Projection::FirstAngle) => Orientation::Left,
            (ViewRole::Isometric, _) => Orientation::Isometric,
            (ViewRole::Section | ViewRole::Detail, _) => Orientation::Front,
        }
    }

    pub fn is_derived(self) -> bool {
        matches!(self, ViewRole::Section | ViewRole::Detail)
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
    /// A section view's cutting line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section: Option<SectionDef>,
    /// A detail view's circle and scale.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<DetailDef>,
    /// A section or detail view's letter, as `A`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub label: String,
}

impl View {
    /// The view this one is made from, for section and detail views.
    pub fn parent(&self) -> Option<u32> {
        self.section.as_ref().map(|s| s.parent).or(self.detail.as_ref().map(|d| d.parent))
    }

    /// What the view is called on the sheet and in the left pane.
    pub fn title(&self, projection: Projection) -> String {
        match self.role {
            ViewRole::Section => format!("Section {0}-{0}", self.label),
            ViewRole::Detail => format!("Detail {}", self.label),
            r => r.orientation(projection).label().to_string(),
        }
    }
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
    /// The next id for a view or a dimension; ids are never reused.
    pub next_id: u32,
    /// The part's configuration the drawing shows, by name; `None` for the configuration
    /// the part was saved in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub configuration: Option<String>,
    #[serde(default)]
    pub dimensions: Vec<Dimension>,
    #[serde(default)]
    pub title_block: TitleBlock,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub callouts: Vec<HoleCallout>,
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
            configuration: None,
            dimensions: Vec::new(),
            title_block: TitleBlock::standard(),
            callouts: Vec::new(),
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
        self.views.push(View {
            id,
            role,
            display,
            tangent,
            section: None,
            detail: None,
            label: String::new(),
        });
        id
    }

    /// The next free letter for a section or detail view: A, B, C... without I, O and Q,
    /// then AA, AB...
    pub fn next_label(&self) -> String {
        const LETTERS: &str = "ABCDEFGHJKLMNPRSTUVWXYZ";
        let used = |l: &str| self.views.iter().any(|v| v.label.eq_ignore_ascii_case(l));
        let one: Vec<String> = LETTERS.chars().map(String::from).collect();
        let two = LETTERS.chars().flat_map(|a| LETTERS.chars().map(move |b| format!("{a}{b}")));
        one.into_iter().chain(two).find(|l| !used(l)).unwrap_or_else(|| format!("V{}", self.next_id))
    }

    /// Adds a section view cut along `def`, with the next letter. Returns its id.
    pub fn add_section(&mut self, def: SectionDef) -> u32 {
        let label = self.next_label();
        let id = self.add_view(ViewRole::Section, DisplayStyle::HiddenRemoved, TangentEdges::Removed);
        if let Some(v) = self.view_mut(id) {
            v.section = Some(def);
            v.label = label;
        }
        id
    }

    /// Adds a detail view of `def`'s circle, with the next letter and its parent's display.
    pub fn add_detail(&mut self, def: DetailDef) -> u32 {
        let label = self.next_label();
        let (display, tangent) = self
            .view(def.parent)
            .map_or((DisplayStyle::HiddenRemoved, TangentEdges::Removed), |p| (p.display, p.tangent));
        let id = self.add_view(ViewRole::Detail, display, tangent);
        if let Some(v) = self.view_mut(id) {
            v.detail = Some(def);
            v.label = label;
        }
        id
    }

    /// Removes a section or detail view, the views made from it, and their dimensions and
    /// callouts. Standard views are kept. Returns how many views went.
    pub fn remove_view(&mut self, id: u32) -> usize {
        if !self.view(id).is_some_and(|v| v.role.is_derived()) {
            return 0;
        }
        let mut gone = vec![id];
        loop {
            let more: Vec<u32> = self
                .views
                .iter()
                .filter(|v| !gone.contains(&v.id) && v.parent().is_some_and(|p| gone.contains(&p)))
                .map(|v| v.id)
                .collect();
            if more.is_empty() {
                break;
            }
            gone.extend(more);
        }
        self.views.retain(|v| !gone.contains(&v.id));
        self.dimensions.retain(|d| !gone.contains(&d.view));
        self.callouts.retain(|c| !gone.contains(&c.view));
        gone.len()
    }

    /// The direction any view looks from: a standard view's own, a section view's from its
    /// cutting line, a detail view's from its parent. `None` for a view that is missing or
    /// made from one that is.
    pub fn view_orientation(&self, id: u32) -> Option<Orientation> {
        let mut v = self.view(id)?;
        for _ in 0..=self.views.len() {
            match v.role {
                ViewRole::Section => return v.section.as_ref()?.orientation(),
                ViewRole::Detail => v = self.view(v.detail.as_ref()?.parent)?,
                r => return Some(r.orientation(self.sheet.projection)),
            }
        }
        None
    }

    /// The scale a view is drawn at: a detail view's own, otherwise the sheet's.
    pub fn view_scale(&self, id: u32) -> Scale {
        match self.view(id).and_then(|v| v.detail.as_ref()) {
            Some(d) => d.scale,
            None => self.sheet.scale,
        }
    }

    /// Adds a hole callout and returns its id.
    pub fn add_callout(&mut self, mut c: HoleCallout) -> u32 {
        c.id = self.next_id;
        self.next_id += 1;
        let id = c.id;
        self.callouts.push(c);
        id
    }

    pub fn callout_mut(&mut self, id: u32) -> Option<&mut HoleCallout> {
        self.callouts.iter_mut().find(|c| c.id == id)
    }

    pub fn view(&self, id: u32) -> Option<&View> {
        self.views.iter().find(|v| v.id == id)
    }

    pub fn view_mut(&mut self, id: u32) -> Option<&mut View> {
        self.views.iter_mut().find(|v| v.id == id)
    }

    /// Adds a dimension and returns its id.
    pub fn add_dimension(&mut self, mut d: Dimension) -> u32 {
        d.id = self.next_id;
        self.next_id += 1;
        let id = d.id;
        self.dimensions.push(d);
        id
    }

    pub fn dimension(&self, id: u32) -> Option<&Dimension> {
        self.dimensions.iter().find(|d| d.id == id)
    }

    pub fn dimension_mut(&mut self, id: u32) -> Option<&mut Dimension> {
        self.dimensions.iter_mut().find(|d| d.id == id)
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
        let used = d
            .views
            .iter()
            .map(|v| v.id)
            .chain(d.dimensions.iter().map(|x| x.id))
            .chain(d.callouts.iter().map(|x| x.id))
            .max();
        d.next_id = d.next_id.max(used.map_or(1, |m| m + 1));
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
        d.configuration = Some("Long".into());
        d.title_block.fields[0].text = "{Description} ({PartNo})".into();
        let text = d.to_json().expect("json");
        assert!(text.contains("\"format\": \"anvil-drawing\""));
        assert!(text.contains("\"format_version\": 3"));
        assert_eq!(Drawing::from_json(&text).expect("read"), d);
    }

    #[test]
    fn refuses_other_and_newer_files() {
        let part = r#"{"format": "anvil-part", "format_version": 1}"#;
        assert!(matches!(Drawing::from_json(part), Err(DrawingFileError::WrongFormat(_))));
        let text = Drawing::default()
            .to_json()
            .expect("json")
            .replace("\"format_version\": 3", "\"format_version\": 99");
        assert!(matches!(Drawing::from_json(&text), Err(DrawingFileError::TooNew(99))));
        assert!(matches!(Drawing::from_json("{"), Err(DrawingFileError::Json(_))));
    }

    #[test]
    fn version_1_files_open_with_the_standard_title_block() {
        let v1 = r#"{"format": "anvil-drawing", "format_version": 1, "model": "p.anvprt",
            "sheet": {"size": "A3", "scale": {"sheet": 1, "model": 1}, "projection": "FirstAngle"},
            "views": [{"id": 1, "role": "Front", "display": "HiddenRemoved", "tangent": "Removed"}],
            "next_id": 2}"#;
        let d = Drawing::from_json(v1).expect("version 1");
        assert_eq!(d.title_block, TitleBlock::standard());
        assert!(d.dimensions.is_empty());
        assert_eq!(d.configuration, None);
        assert!(d.to_json().expect("json").contains("\"format_version\": 3"));
    }

    #[test]
    fn section_and_detail_views_round_trip_and_go_with_their_parent() {
        use glam::DVec2;
        let mut d = Drawing::standard("p.anvprt".into(), Sheet::default());
        let front = d.views[0].id;
        let axes = Orientation::Front.axes();
        let a = d.add_section(SectionDef::new(front, &axes, true, &[DVec2::new(0.0, 5.0)]));
        let b =
            d.add_detail(DetailDef { parent: a, centre: [1.0, 0.0, 2.0], radius: 4.0, scale: Scale::FULL });
        d.add_callout(HoleCallout { id: 0, view: b, feature: 3, offset: [0.0; 3] });
        assert_eq!(d.view(a).map(|v| v.label.as_str()), Some("A"));
        assert_eq!(d.view(b).map(|v| v.title(d.sheet.projection)), Some("Detail B".into()));
        assert_eq!(d.view_orientation(a), Some(Orientation::Top));
        assert_eq!(d.view_orientation(b), Some(Orientation::Top), "a detail looks as its parent");
        assert_eq!(d.view_scale(b), Scale::FULL);
        let text = d.to_json().expect("json");
        assert_eq!(Drawing::from_json(&text).expect("read"), d);
        // Standard views stay; a section takes its detail and the detail's callout with it.
        assert_eq!(d.remove_view(front), 0);
        assert_eq!(d.remove_view(a), 2);
        assert_eq!(d.views.len(), 4);
        assert!(d.callouts.is_empty());
        // Letters skip I, O and Q and are reused once free.
        assert_eq!(d.next_label(), "A");
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
