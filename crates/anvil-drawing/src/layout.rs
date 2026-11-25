//! Laying a drawing out on its sheet: where each view goes, which lines it draws and with
//! which pen (see `docs/re/drawings.md`, "Standard views" and "Line styles"). The result,
//! a [`SheetPicture`], is what both the app and the PDF writer draw.
//!
//! Sheet coordinates are millimetres from the paper's lower-left corner, y up.

use crate::PartData;
use crate::callout::{self, CalloutPicture};
use crate::dims::{self, DimPicture, Pickables, ViewContext};
use crate::document::{DisplayStyle, Drawing, TangentEdges, View, ViewRole};
use crate::hlr::{LineKind, Orientation, Path2, ViewGeometry};
use crate::sheet::{FRAME_MARGIN, Projection, Scale, Sheet, SheetSize};
use crate::title::{self, TitleBlock, Values};
use crate::{detail, section};
use glam::{DVec2, DVec3};
use std::collections::HashMap;

/// The pens a sheet is drawn with, after ISO 128 (thick 0.5 mm, thin 0.25 mm).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Pen {
    /// Seen edges and outlines: thick, continuous.
    Visible,
    /// Tangent edges drawn thin: thin, continuous.
    Thin,
    /// Hidden edges: thin, dashed.
    Hidden,
    /// The drawing frame and the title block's outline.
    Frame,
    /// The title block's inner lines.
    Title,
    /// Dimension and extension lines: thin, continuous.
    Dim,
    /// Hatching of cut faces: thin, continuous (dashed patterns are drawn as pieces).
    Hatch,
    /// A cutting line: thin, long-dash dot.
    Chain,
    /// The thick ends and corners of a cutting line.
    CutEnds,
}

impl Pen {
    /// Line width in millimetres.
    pub fn width(self) -> f64 {
        match self {
            Pen::Visible | Pen::Frame | Pen::CutEnds => 0.5,
            Pen::Thin | Pen::Hidden | Pen::Title | Pen::Dim | Pen::Hatch | Pen::Chain => 0.25,
        }
    }

    /// Dash pattern in millimetres, on and off in turn, for broken pens.
    pub fn dash(self) -> Option<&'static [f64]> {
        match self {
            Pen::Hidden => Some(&[3.0, 1.0]),
            Pen::Chain => Some(&[12.0, 1.25, 0.5, 1.25]),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Stroke {
    pub pen: Pen,
    pub path: Path2,
    /// The view the stroke belongs to, or `None` for the sheet's own lines.
    pub view: Option<u32>,
}

/// How a note's text sits on its baseline point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
}

/// Text on the sheet: its baseline starts, is centred on, or ends at `pos`.
#[derive(Debug, Clone, PartialEq)]
pub struct Note {
    pub pos: DVec2,
    /// Letter height (of capitals) in millimetres.
    pub height: f64,
    pub text: String,
    pub align: Align,
}

/// Width of `text` in Helvetica, in letter heights: near enough to place and fit text.
pub fn text_width(text: &str) -> f64 {
    // Helvetica advance widths (per em) over its capital height (0.718 em).
    text.chars()
        .map(|c| match c {
            ' ' | '.' | ',' | ':' | ';' | 'i' | 'l' | 'j' | '!' | '|' | '\'' => 0.278,
            '(' | ')' | '-' | 'f' | 't' | 'r' | '[' | ']' | '/' => 0.333,
            '\u{b0}' => 0.4,
            'm' | 'M' | 'W' => 0.833,
            'w' => 0.722,
            '\u{d8}' | 'O' | 'Q' | 'G' | 'C' | 'D' | 'H' | 'N' | 'U' | 'R' => 0.75,
            c if c.is_ascii_uppercase() => 0.667,
            _ => 0.556,
        })
        .sum::<f64>()
        / 0.718
}

/// Where a view landed: the box round its lines, in sheet millimetres.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlacedView {
    pub id: u32,
    pub lo: DVec2,
    pub hi: DVec2,
    /// Where the model's origin falls on the sheet.
    pub origin: DVec2,
    /// Sheet millimetres per model millimetre in this view.
    pub scale: f64,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct SheetPicture {
    pub width: f64,
    pub height: f64,
    pub strokes: Vec<Stroke>,
    pub notes: Vec<Note>,
    pub views: Vec<PlacedView>,
    pub dims: Vec<DimPicture>,
    /// Filled triangles (arrowheads of cutting lines), with the pen they belong with.
    pub fills: Vec<(Pen, [DVec2; 3])>,
    /// View labels and the letters of cutting lines and detail circles.
    pub labels: Vec<Note>,
    pub callouts: Vec<CalloutPicture>,
    /// Title-block placeholders the part does not set (only known once the part is in).
    pub title_missing: Vec<String>,
    /// Model dimensions no longer in the model, by id.
    pub dims_missing: Vec<u32>,
    /// Hole callouts whose Hole feature is gone, suppressed or no longer square to the
    /// view, by id.
    pub callouts_missing: Vec<u32>,
}

impl SheetPicture {
    /// The picture as it goes to PDF and DXF: dimensions that no longer attach to the model
    /// are left out, so a stale value is never printed.
    pub fn for_output(&self) -> SheetPicture {
        let mut p = self.clone();
        p.dims.retain(|d| !d.dangling);
        p
    }
}

/// Height of a view label's first line and of its scale line, and the gap above it, in
/// sheet millimetres.
pub const LABEL_HEIGHT: f64 = 5.0;
const SCALE_HEIGHT: f64 = 3.5;
const LABEL_GAP: f64 = 3.0;

/// The room a derived view's label takes below it.
fn label_room(has_scale: bool) -> f64 {
    LABEL_GAP + LABEL_HEIGHT + if has_scale { 2.0 + SCALE_HEIGHT } else { 0.0 }
}

impl Path2 {
    /// The path scaled by `s` about the view's origin and moved to `origin` on the sheet.
    pub fn placed(&self, origin: DVec2, s: f64) -> Path2 {
        let m = |p: DVec2| origin + p * s;
        match self {
            Path2::Line { a, b } => Path2::Line { a: m(*a), b: m(*b) },
            Path2::Conic { c, u, v, t0, t1 } => {
                Path2::Conic { c: m(*c), u: *u * s, v: *v * s, t0: *t0, t1: *t1 }
            }
            Path2::Poly(p) => Path2::Poly(p.iter().map(|q| m(*q)).collect()),
        }
    }
}

/// The pen a line of a view is drawn with, or `None` when the view leaves it out.
pub fn pen_for(kind: LineKind, view: &View) -> Option<Pen> {
    let tangent = match view.tangent {
        TangentEdges::Visible => Some(Pen::Visible),
        TangentEdges::Thin => Some(Pen::Thin),
        TangentEdges::Removed => None,
    };
    match (kind, view.display) {
        (LineKind::Visible, _) => Some(Pen::Visible),
        (LineKind::Tangent, _) => tangent,
        (LineKind::Hidden | LineKind::HiddenTangent, DisplayStyle::HiddenRemoved) => None,
        (LineKind::Hidden, DisplayStyle::HiddenDashed) => Some(Pen::Hidden),
        (LineKind::Hidden, DisplayStyle::Wireframe) => Some(Pen::Visible),
        (LineKind::HiddenTangent, DisplayStyle::HiddenDashed) => tangent.map(|_| Pen::Hidden),
        (LineKind::HiddenTangent, DisplayStyle::Wireframe) => tangent,
    }
}

/// The space between neighbouring views, in sheet millimetres.
pub fn view_gap(size: SheetSize) -> f64 {
    (size.size_mm().1 * 0.06).max(12.0)
}

/// The title block's width, its first row's height and the other rows' height, in
/// millimetres (after ISO 7200).
pub const TITLE_WIDTH: f64 = 180.0;
const TITLE_ROW: f64 = 12.0;
const FIELD_ROW: f64 = 9.0;

/// The title block's height for `fields` fields: the first spans a row, the others go two
/// to a row.
pub fn title_height(fields: usize) -> f64 {
    match fields {
        0 => 0.0,
        n => TITLE_ROW + (n - 1).div_ceil(2) as f64 * FIELD_ROW,
    }
}

/// The area inside the frame where views go, above the title block: (lower-left,
/// upper-right).
fn view_area(size: SheetSize, block: &TitleBlock) -> (DVec2, DVec2) {
    let (w, h) = size.size_mm();
    let pad = 4.0;
    let bottom = (title_height(block.fields.len()) + pad).max(8.0);
    (
        DVec2::new(FRAME_MARGIN + pad, FRAME_MARGIN + bottom),
        DVec2::new(w - FRAME_MARGIN - pad, h - FRAME_MARGIN - pad),
    )
}

/// Places views whose extents (in model millimetres on each view's plane) are known, at
/// sheet scale `s`. Returns each view's origin on the sheet, before centring.
fn arrange(
    extents: &[(u32, ViewRole, (DVec2, DVec2))],
    projection: Projection,
    s: f64,
    gap: f64,
) -> Vec<(u32, DVec2, DVec2, DVec2)> {
    let get = |r: ViewRole| extents.iter().find(|e| e.1 == r).map(|e| e.2);
    let front = get(ViewRole::Front);
    let mut out: Vec<(u32, DVec2, DVec2, DVec2)> = Vec::new();
    let mut place = |id: u32, (lo, hi): (DVec2, DVec2), origin: DVec2| {
        out.push((id, origin + lo * s, origin + hi * s, origin));
    };
    let mut top_box = None;
    let mut side_box = None;
    if let Some((lo, hi)) = front {
        if let Some(e) = extents.iter().find(|e| e.1 == ViewRole::Front) {
            place(e.0, (lo, hi), DVec2::ZERO);
        }
        if let Some(e) = extents.iter().find(|e| e.1 == ViewRole::Top) {
            let (tlo, thi) = e.2;
            let y = match projection {
                Projection::ThirdAngle => hi.y * s + gap - tlo.y * s,
                Projection::FirstAngle => lo.y * s - gap - thi.y * s,
            };
            let o = DVec2::new(0.0, y);
            top_box = Some((o + tlo * s, o + thi * s));
            place(e.0, e.2, o);
        }
        if let Some(e) = extents.iter().find(|e| e.1 == ViewRole::Side) {
            let (slo, shi) = e.2;
            let o = DVec2::new(hi.x * s + gap - slo.x * s, 0.0);
            side_box = Some((o + slo * s, o + shi * s));
            place(e.0, e.2, o);
        }
    }
    if let Some(e) = extents.iter().find(|e| e.1 == ViewRole::Isometric) {
        let (ilo, ihi) = e.2;
        let size = (ihi - ilo) * s;
        // The corner the other views leave free: right of the front view, and above it
        // (third angle) or below it (first angle), clear of the top and side views.
        let (flo, fhi) = match front {
            Some((lo, hi)) => (lo * s, hi * s),
            None => (DVec2::ZERO, DVec2::ZERO),
        };
        let left = side_box
            .map_or(fhi.x + gap, |(slo, _)| slo.x)
            .max(top_box.map_or(f64::MIN, |(_, thi)| thi.x + gap));
        let corner = match projection {
            Projection::ThirdAngle => {
                let bottom = side_box.map_or(fhi.y, |(_, shi)| shi.y.max(fhi.y)) + gap;
                DVec2::new(left, bottom)
            }
            Projection::FirstAngle => {
                let top = side_box.map_or(flo.y, |(slo, _)| slo.y.min(flo.y)) - gap;
                DVec2::new(left, top - size.y)
            }
        };
        place(e.0, e.2, corner - ilo * s);
    }
    out
}

/// A section or detail view to place: its id, its extent in model millimetres on its
/// plane, its scale, and the room its label takes below it.
#[derive(Debug, Clone, Copy)]
struct Derived {
    id: u32,
    extent: (DVec2, DVec2),
    scale: f64,
    label: f64,
}

/// Places section and detail views in columns to the right of `placed` (the standard
/// views), top to bottom, starting a new column when one would grow taller than `room`.
fn arrange_derived(placed: &mut Vec<(u32, DVec2, DVec2, DVec2)>, derived: &[Derived], gap: f64, room: f64) {
    if derived.is_empty() {
        return;
    }
    let (glo, ghi) = if placed.is_empty() { (DVec2::ZERO, DVec2::ZERO) } else { union(placed) };
    let top = if placed.is_empty() { 0.0 } else { ghi.y };
    let mut x = if placed.is_empty() { 0.0 } else { ghi.x + gap };
    let bottom = if placed.is_empty() { top - room } else { (ghi.y - room).min(glo.y) };
    let mut y = top;
    let mut col_width: f64 = 0.0;
    for d in derived {
        let (lo, hi) = d.extent;
        let size = (hi - lo) * d.scale;
        let h = size.y + d.label;
        if y < top && y - h < bottom {
            x += col_width + gap;
            y = top;
            col_width = 0.0;
        }
        let vlo = DVec2::new(x, y - size.y);
        let origin = vlo - lo * d.scale;
        placed.push((d.id, vlo, vlo + size, origin));
        col_width = col_width.max(size.x);
        y -= h + gap;
    }
}

/// The extent of a view of a box, from its corners: what the views of a part will need
/// before they are projected.
pub fn box_extent(lo: DVec3, hi: DVec3, o: Orientation) -> (DVec2, DVec2) {
    let ax = o.axes();
    let mut a = DVec2::splat(f64::INFINITY);
    let mut b = DVec2::splat(f64::NEG_INFINITY);
    for i in 0..8 {
        let p = DVec3::new(
            if i & 1 == 0 { lo.x } else { hi.x },
            if i & 2 == 0 { lo.y } else { hi.y },
            if i & 4 == 0 { lo.z } else { hi.z },
        );
        let q = ax.project(p);
        a = a.min(q);
        b = b.max(q);
    }
    (a, b)
}

/// The largest standard scale at which views of the given extents fit on the sheet.
pub fn fit_scale(drawing: &Drawing, extent: impl Fn(&View) -> Option<(DVec2, DVec2)>) -> Scale {
    for scale in crate::sheet::STANDARD_SCALES {
        if fits(drawing, scale, &extent) {
            return scale;
        }
    }
    Scale { sheet: 1, model: 1000 }
}

/// True when every view, with the derived views' labels, fits the sheet at `scale`.
pub fn fits(drawing: &Drawing, scale: Scale, extent: &impl Fn(&View) -> Option<(DVec2, DVec2)>) -> bool {
    let sheet = drawing.sheet;
    let (alo, ahi) = view_area(sheet.size, &drawing.title_block);
    let room = ahi - alo;
    let gap = view_gap(sheet.size);
    let (standard, derived) = split_views(drawing, scale, |v| extent(v));
    let mut placed = arrange(&standard, sheet.projection, scale.factor(), gap);
    arrange_derived(&mut placed, &derived, gap, room.y);
    if placed.is_empty() {
        return true;
    }
    let (lo, hi) = union_with_labels(&placed, &derived);
    let size = hi - lo;
    size.x <= room.x + 1e-9 && size.y <= room.y + 1e-9
}

/// A standard view's id, role and extent in model millimetres on its plane.
type Extent = (u32, ViewRole, (DVec2, DVec2));

/// The standard views' extents, and the derived views to place, at sheet scale `scale`.
fn split_views(
    drawing: &Drawing,
    scale: Scale,
    extent: impl Fn(&View) -> Option<(DVec2, DVec2)>,
) -> (Vec<Extent>, Vec<Derived>) {
    let mut standard = Vec::new();
    let mut derived = Vec::new();
    for v in &drawing.views {
        let Some(e) = extent(v) else { continue };
        if v.role.is_derived() {
            let own = v.detail.as_ref().map(|d| d.scale);
            let vs = own.unwrap_or(scale);
            derived.push(Derived { id: v.id, extent: e, scale: vs.factor(), label: label_room(vs != scale) });
        } else {
            standard.push((v.id, v.role, e));
        }
    }
    (standard, derived)
}

/// The box round every placed view, with the derived views' labels below them.
fn union_with_labels(placed: &[(u32, DVec2, DVec2, DVec2)], derived: &[Derived]) -> (DVec2, DVec2) {
    let (mut lo, hi) = union(placed);
    for d in derived {
        if let Some(p) = placed.iter().find(|p| p.0 == d.id) {
            lo.y = lo.y.min(p.1.y - d.label);
        }
    }
    (lo, hi)
}

fn union(placed: &[(u32, DVec2, DVec2, DVec2)]) -> (DVec2, DVec2) {
    placed.iter().fold((DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY)), |(lo, hi), p| {
        (lo.min(p.1), hi.max(p.2))
    })
}

/// Lays the drawing out with the views that have been projected (`geometry`, by view id),
/// without the part: no model dimensions, and the title block's part values empty.
pub fn layout(drawing: &Drawing, geometry: &HashMap<u32, ViewGeometry>) -> SheetPicture {
    layout_full(drawing, geometry, None, "")
}

/// Lays the drawing out: views, dimensions and the title block. `part` gives model
/// dimensions their values and fills the title block; `name` is the drawing's name.
pub fn layout_full(
    drawing: &Drawing,
    geometry: &HashMap<u32, ViewGeometry>,
    part: Option<&PartData>,
    name: &str,
) -> SheetPicture {
    layout_with(drawing, geometry, part, name, None)
}

/// [`layout_full`], with each view's [`dims::pickables`] found beforehand (the app keeps
/// them while the views do not change).
pub fn layout_with(
    drawing: &Drawing,
    geometry: &HashMap<u32, ViewGeometry>,
    part: Option<&PartData>,
    name: &str,
    picks: Option<&HashMap<u32, Pickables>>,
) -> SheetPicture {
    let sheet: Sheet = drawing.sheet;
    let (w, h) = sheet.size.size_mm();
    let mut pic = SheetPicture { width: w, height: h, ..SheetPicture::default() };
    let frame = [
        DVec2::new(FRAME_MARGIN, FRAME_MARGIN),
        DVec2::new(w - FRAME_MARGIN, FRAME_MARGIN),
        DVec2::new(w - FRAME_MARGIN, h - FRAME_MARGIN),
        DVec2::new(FRAME_MARGIN, h - FRAME_MARGIN),
        DVec2::new(FRAME_MARGIN, FRAME_MARGIN),
    ];
    pic.strokes.push(Stroke { pen: Pen::Frame, path: Path2::Poly(frame.to_vec()), view: None });
    title_block(&mut pic, drawing, part, name);

    let s = sheet.scale.factor();
    let (standard, derived) =
        split_views(drawing, sheet.scale, |v| geometry.get(&v.id).and_then(|g| g.bounds()));
    let gap = view_gap(sheet.size);
    let (alo, ahi) = view_area(sheet.size, &drawing.title_block);
    let mut placed = arrange(&standard, sheet.projection, s, gap);
    arrange_derived(&mut placed, &derived, gap, (ahi - alo).y);
    if placed.is_empty() {
        return pic;
    }
    // Centre the group of views in the area inside the frame.
    let (lo, hi) = union_with_labels(&placed, &derived);
    let shift = (alo + ahi) * 0.5 - (lo + hi) * 0.5;
    for (id, vlo, vhi, origin) in placed {
        let origin = origin + shift;
        let vs = drawing.view_scale(id).factor();
        pic.views.push(PlacedView { id, lo: vlo + shift, hi: vhi + shift, origin, scale: vs });
        let (Some(view), Some(geom)) = (drawing.view(id), geometry.get(&id)) else { continue };
        for line in &geom.lines {
            if let Some(pen) = pen_for(line.kind, view) {
                pic.strokes.push(Stroke { pen, path: line.path.placed(origin, vs), view: Some(id) });
            }
        }
        for area in &geom.hatch {
            for h in section::hatch_lines(area, vs) {
                let pieces = match geom.clip {
                    Some((c, r)) => detail::clip_path(&h, c, r),
                    None => vec![h],
                };
                for p in pieces {
                    pic.strokes.push(Stroke { pen: Pen::Hatch, path: p.placed(origin, vs), view: Some(id) });
                }
            }
        }
        if let Some((c, r)) = geom.clip {
            pic.strokes.push(Stroke {
                pen: Pen::Thin,
                path: circle(origin + c * vs, r * vs),
                view: Some(id),
            });
        }
        if view.role.is_derived() {
            let below = (vlo + shift).y - LABEL_GAP - LABEL_HEIGHT;
            let x = 0.5 * ((vlo + shift).x + (vhi + shift).x);
            pic.labels.push(Note {
                pos: DVec2::new(x, below),
                height: LABEL_HEIGHT,
                text: view.title(sheet.projection).to_uppercase(),
                align: Align::Center,
            });
            let own = drawing.view_scale(id);
            if own != sheet.scale {
                pic.labels.push(Note {
                    pos: DVec2::new(x, below - 2.0 - SCALE_HEIGHT),
                    height: SCALE_HEIGHT,
                    text: format!("SCALE {own}"),
                    align: Align::Center,
                });
            }
        }
    }
    marks_on_parents(&mut pic, drawing, geometry);

    let mut found: HashMap<u32, Pickables> = HashMap::new();
    for d in &drawing.dimensions {
        let (Some(orientation), Some(pv)) =
            (drawing.view_orientation(d.view), pic.views.iter().find(|v| v.id == d.view).copied())
        else {
            continue;
        };
        let g = geometry.get(&d.view);
        let pk = match (picks.and_then(|m| m.get(&d.view)), g) {
            (Some(p), _) => Some(p),
            (None, Some(g)) if d.is_reference() => {
                Some(&*found.entry(d.view).or_insert_with(|| dims::pickables(g)))
            }
            _ => None,
        };
        let cx = ViewContext { part, geometry: g, orientation, scale: pv.scale };
        match dims::resolve(d, &cx, pk) {
            Some(r) => pic.dims.push(dims::picture(d.id, &r, d.is_reference(), pv.origin, pv.scale)),
            None if part.is_some() => pic.dims_missing.push(d.id),
            None => {}
        }
    }
    for c in &drawing.callouts {
        let (Some(orientation), Some(pv)) =
            (drawing.view_orientation(c.view), pic.views.iter().find(|v| v.id == c.view).copied())
        else {
            continue;
        };
        let Some(part) = part else { continue };
        let drawn = part
            .holes
            .iter()
            .find(|h| h.feature == c.feature)
            .and_then(|h| callout::picture(c, h, orientation, pv.origin, pv.scale, pv.hi.y));
        match drawn {
            Some(p) => pic.callouts.push(p),
            None => pic.callouts_missing.push(c.id),
        }
    }
    pic
}

/// A full circle on the sheet.
fn circle(c: DVec2, r: f64) -> Path2 {
    Path2::Conic { c, u: DVec2::X * r, v: DVec2::Y * r, t0: 0.0, t1: std::f64::consts::TAU }
}

/// Draws each section view's cutting line and each detail view's circle on its parent,
/// with their letters.
fn marks_on_parents(pic: &mut SheetPicture, drawing: &Drawing, geometry: &HashMap<u32, ViewGeometry>) {
    for v in &drawing.views {
        let Some(parent) = v.parent() else { continue };
        let (Some(pv), Some(po)) =
            (pic.views.iter().find(|p| p.id == parent).copied(), drawing.view_orientation(parent))
        else {
            continue;
        };
        let Some((glo, ghi)) = geometry.get(&parent).and_then(|g| g.bounds()) else { continue };
        let axes = po.axes();
        let to = |p: DVec2| pv.origin + p * pv.scale;
        if let Some(def) = &v.section {
            let (pts, a2, s2) = section::line_on_parent(def, &axes, glo, ghi, 6.0 / pv.scale);
            let pts: Vec<DVec2> = pts.into_iter().map(to).collect();
            let a2 = a2.normalize_or(DVec2::X);
            let s2 = s2.normalize_or(DVec2::Y);
            pic.strokes.push(Stroke { pen: Pen::Chain, path: Path2::Poly(pts.clone()), view: Some(parent) });
            // Thick strokes at the ends and either side of every corner.
            let n = pts.len();
            let thick = |a: DVec2, b: DVec2, len: f64| {
                a + (b - a).normalize_or(DVec2::ZERO) * len.min(a.distance(b) * 0.5)
            };
            let (first, last) = (pts[0], pts[n - 1]);
            pic.strokes.push(Stroke {
                pen: Pen::CutEnds,
                path: Path2::Line { a: first, b: thick(first, pts[1], 6.0) },
                view: Some(parent),
            });
            pic.strokes.push(Stroke {
                pen: Pen::CutEnds,
                path: Path2::Line { a: last, b: thick(last, pts[n - 2], 6.0) },
                view: Some(parent),
            });
            for i in 1..n.saturating_sub(1) {
                let c = pts[i];
                let path = Path2::Poly(vec![thick(c, pts[i - 1], 3.0), c, thick(c, pts[i + 1], 3.0)]);
                pic.strokes.push(Stroke { pen: Pen::CutEnds, path, view: Some(parent) });
            }
            for (end, out) in [(first, -a2), (last, a2)] {
                let tip = end + s2 * 7.0;
                let back = tip - s2 * 4.0;
                let side = a2 * 0.8;
                pic.strokes.push(Stroke {
                    pen: Pen::Dim,
                    path: Path2::Line { a: end, b: back },
                    view: Some(parent),
                });
                pic.fills.push((Pen::CutEnds, [tip, back + side, back - side]));
                pic.labels.push(Note {
                    pos: end + s2 * 4.5 + out * 3.5 - DVec2::Y * (LABEL_HEIGHT * 0.5),
                    height: LABEL_HEIGHT,
                    text: v.label.clone(),
                    align: Align::Center,
                });
            }
        }
        if let Some(def) = &v.detail {
            let (c, r) = def.circle(&axes);
            let (c, r) = (to(c), r * pv.scale);
            pic.strokes.push(Stroke { pen: Pen::Thin, path: circle(c, r), view: Some(parent) });
            let d = DVec2::new(1.0, 1.0).normalize();
            pic.labels.push(Note {
                pos: c + d * (r + 4.0) - DVec2::Y * (LABEL_HEIGHT * 0.5),
                height: LABEL_HEIGHT,
                text: v.label.clone(),
                align: Align::Center,
            });
        }
    }
}

/// The values the title block's placeholders are filled from.
pub fn title_values(drawing: &Drawing, part: Option<&PartData>, name: &str) -> Values {
    let sheet = drawing.sheet;
    let mut built_in = vec![
        ("DrawingName", name.to_string()),
        ("Scale", sheet.scale.to_string()),
        ("Size", sheet.size.short_label().to_string()),
        ("Projection", sheet.projection.label().to_string()),
    ];
    let mut properties = Vec::new();
    if let Some(p) = part {
        built_in.push(("Configuration", p.configuration.clone()));
        built_in.push(("FileName", p.file_name.clone()));
        built_in.push(("Material", p.material.clone().unwrap_or_default()));
        properties = p.properties.clone();
    }
    Values { properties, built_in }
}

/// `text` at letter height `height`, made smaller (down to 2 mm) and then shortened so it
/// fits `room` millimetres.
fn fit_text(text: &str, height: f64, room: f64) -> (String, f64) {
    let w = text_width(text);
    if w * height <= room {
        return (text.to_string(), height);
    }
    let h = (room / w.max(1e-9)).max(2.0).min(height);
    if w * h <= room {
        return (text.to_string(), h);
    }
    let mut cut: String = text.to_string();
    while !cut.is_empty() && text_width(&format!("{cut}...")) * h > room {
        cut.pop();
    }
    (format!("{}...", cut.trim_end()), h)
}

/// Draws the title block in the frame's lower right corner.
fn title_block(pic: &mut SheetPicture, drawing: &Drawing, part: Option<&PartData>, name: &str) {
    let fields = &drawing.title_block.fields;
    if fields.is_empty() {
        return;
    }
    let (w, _) = drawing.sheet.size.size_mm();
    let width = TITLE_WIDTH.min(w - 2.0 * FRAME_MARGIN);
    let height = title_height(fields.len());
    let (x0, x1, y0) = (w - FRAME_MARGIN - width, w - FRAME_MARGIN, FRAME_MARGIN);
    let top = y0 + height;
    let rect =
        |lo: DVec2, hi: DVec2| Path2::Poly(vec![lo, DVec2::new(hi.x, lo.y), hi, DVec2::new(lo.x, hi.y), lo]);
    pic.strokes.push(Stroke {
        pen: Pen::Frame,
        path: rect(DVec2::new(x0, y0), DVec2::new(x1, top)),
        view: None,
    });
    let values = title_values(drawing, part, name);
    let mut missing: Vec<String> = Vec::new();
    let mut cell = |pic: &mut SheetPicture, f: &title::TitleField, lo: DVec2, hi: DVec2, big: f64| {
        let (text, unknown) = title::fill(&f.text, &|n| values.get(n));
        for u in unknown {
            if !missing.iter().any(|m| m.eq_ignore_ascii_case(&u)) {
                missing.push(u);
            }
        }
        let room = hi.x - lo.x - 3.0;
        let (label, lh) = fit_text(&f.label, 1.8, room);
        pic.notes.push(Note {
            pos: DVec2::new(lo.x + 1.5, hi.y - 1.0 - lh),
            height: lh,
            text: label,
            align: Align::Left,
        });
        if !text.is_empty() {
            let (text, th) = fit_text(&text, big, room);
            // High enough that descenders clear the cell's bottom line.
            pic.notes.push(Note {
                pos: DVec2::new(lo.x + 1.5, lo.y + 0.4 * th),
                height: th,
                text,
                align: Align::Left,
            });
        }
    };
    cell(pic, &fields[0], DVec2::new(x0, top - TITLE_ROW), DVec2::new(x1, top), 5.0);
    let xm = (x0 + x1) * 0.5;
    let mut y = top - TITLE_ROW;
    let mut lines = vec![(DVec2::new(x0, y), DVec2::new(x1, y))];
    for (i, f) in fields[1..].iter().enumerate() {
        let left = i % 2 == 0;
        if left && i > 0 {
            y -= FIELD_ROW;
            lines.push((DVec2::new(x0, y), DVec2::new(x1, y)));
        }
        let (lo, hi) = if left {
            (DVec2::new(x0, y - FIELD_ROW), DVec2::new(xm, y))
        } else {
            (DVec2::new(xm, y - FIELD_ROW), DVec2::new(x1, y))
        };
        cell(pic, f, lo, hi, 3.5);
    }
    if fields.len() > 1 {
        lines.push((DVec2::new(xm, top - TITLE_ROW), DVec2::new(xm, y0)));
    }
    for (a, b) in lines {
        // The outline already draws the block's bottom edge.
        if (a.y - y0).abs() > 1e-9 {
            pic.strokes.push(Stroke { pen: Pen::Title, path: Path2::Line { a, b }, view: None });
        }
    }
    if part.is_some() {
        pic.title_missing = missing;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::DisplayStyle;

    fn extent(o: Orientation) -> (DVec2, DVec2) {
        box_extent(DVec3::ZERO, DVec3::new(40.0, 30.0, 20.0), o)
    }

    #[test]
    fn third_angle_puts_top_above_and_right_to_the_right() {
        let e = [
            (1, ViewRole::Front, extent(Orientation::Front)),
            (2, ViewRole::Top, extent(Orientation::Top)),
            (3, ViewRole::Side, extent(Orientation::Right)),
        ];
        let p = arrange(&e, Projection::ThirdAngle, 1.0, 10.0);
        let front = p[0];
        let top = p.iter().find(|v| v.0 == 2).copied().expect("top");
        let side = p.iter().find(|v| v.0 == 3).copied().expect("side");
        // Front 40 x 30 at the origin; top 40 x 20 above it, side 20 x 30 to its right.
        assert_eq!((front.1, front.2), (DVec2::new(0.0, 0.0), DVec2::new(40.0, 30.0)));
        assert_eq!((top.1, top.2), (DVec2::new(0.0, 40.0), DVec2::new(40.0, 60.0)));
        assert_eq!((side.1, side.2), (DVec2::new(50.0, 0.0), DVec2::new(70.0, 30.0)));
    }

    #[test]
    fn first_angle_puts_top_below_and_left_view_to_the_right() {
        let e = [
            (1, ViewRole::Front, extent(Orientation::Front)),
            (2, ViewRole::Top, extent(Orientation::Top)),
            (3, ViewRole::Side, extent(Orientation::Left)),
        ];
        let p = arrange(&e, Projection::FirstAngle, 2.0, 10.0);
        let top = p.iter().find(|v| v.0 == 2).copied().expect("top");
        let side = p.iter().find(|v| v.0 == 3).copied().expect("side");
        assert_eq!((top.1, top.2), (DVec2::new(0.0, -50.0), DVec2::new(80.0, -10.0)));
        assert_eq!((side.1, side.2), (DVec2::new(90.0, 0.0), DVec2::new(130.0, 60.0)));
    }

    #[test]
    fn the_isometric_overlaps_no_other_view() {
        for (projection, side) in
            [(Projection::FirstAngle, Orientation::Left), (Projection::ThirdAngle, Orientation::Right)]
        {
            let e = [
                (1, ViewRole::Front, extent(Orientation::Front)),
                (2, ViewRole::Top, extent(Orientation::Top)),
                (3, ViewRole::Side, extent(side)),
                (4, ViewRole::Isometric, extent(Orientation::Isometric)),
            ];
            let p = arrange(&e, projection, 1.0, 10.0);
            let iso = p.iter().find(|v| v.0 == 4).copied().expect("iso");
            for v in p.iter().filter(|v| v.0 != 4) {
                let apart = iso.1.x >= v.2.x || iso.2.x <= v.1.x || iso.1.y >= v.2.y || iso.2.y <= v.1.y;
                assert!(apart, "{projection:?}: isometric {iso:?} overlaps view {v:?}");
            }
            let front = p[0];
            // Beside the side view, above (third angle) or below (first angle) the front.
            assert_eq!(iso.1.x, 50.0);
            match projection {
                Projection::ThirdAngle => assert!(iso.1.y >= front.2.y + 10.0 - 1e-9),
                Projection::FirstAngle => assert!(iso.2.y <= front.1.y - 10.0 + 1e-9),
            }
        }
    }

    #[test]
    fn pens_follow_the_view_settings() {
        let mut v = View {
            id: 1,
            role: ViewRole::Front,
            display: DisplayStyle::HiddenRemoved,
            tangent: TangentEdges::Removed,
            section: None,
            detail: None,
            label: String::new(),
        };
        assert_eq!(pen_for(LineKind::Visible, &v), Some(Pen::Visible));
        assert_eq!(pen_for(LineKind::Hidden, &v), None);
        assert_eq!(pen_for(LineKind::Tangent, &v), None);
        v.display = DisplayStyle::HiddenDashed;
        v.tangent = TangentEdges::Thin;
        assert_eq!(pen_for(LineKind::Hidden, &v), Some(Pen::Hidden));
        assert_eq!(pen_for(LineKind::Tangent, &v), Some(Pen::Thin));
        assert_eq!(pen_for(LineKind::HiddenTangent, &v), Some(Pen::Hidden));
        v.display = DisplayStyle::Wireframe;
        assert_eq!(pen_for(LineKind::Hidden, &v), Some(Pen::Visible));
        assert_eq!(pen_for(LineKind::HiddenTangent, &v), Some(Pen::Thin));
    }

    #[test]
    fn fit_scale_picks_the_largest_standard_scale_that_fits() {
        let d = Drawing::standard("p.anvprt".into(), Sheet::default());
        let ex = |size: f64| {
            move |v: &View| {
                Some(box_extent(DVec3::ZERO, DVec3::splat(size), v.role.orientation(Projection::FirstAngle)))
            }
        };
        // A3: 420 x 297 with a 10 mm frame. A 30 mm cube's front and top views, a gap and
        // its isometric below them make 176 mm at 2:1, which fits the 265 mm of room; at
        // 5:1 they need 413 mm. A 50 mm cube needs 318 mm at 2:1, so it goes at 1:1.
        assert_eq!(fit_scale(&d, ex(30.0)), Scale { sheet: 2, model: 1 });
        assert_eq!(fit_scale(&d, ex(50.0)), Scale::FULL);
        assert_eq!(fit_scale(&d, ex(500.0)), Scale { sheet: 1, model: 10 });
    }
}
