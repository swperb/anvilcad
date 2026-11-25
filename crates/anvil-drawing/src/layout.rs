//! Laying a drawing out on its sheet: where each view goes, which lines it draws and with
//! which pen (see `docs/re/drawings.md`, "Standard views" and "Line styles"). The result,
//! a [`SheetPicture`], is what both the app and the PDF writer draw.
//!
//! Sheet coordinates are millimetres from the paper's lower-left corner, y up.

use crate::document::{DisplayStyle, Drawing, TangentEdges, View, ViewRole};
use crate::hlr::{LineKind, Orientation, Path2, ViewGeometry};
use crate::sheet::{FRAME_MARGIN, Projection, Scale, Sheet, SheetSize};
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
    /// The drawing frame.
    Frame,
}

impl Pen {
    /// Line width in millimetres.
    pub fn width(self) -> f64 {
        match self {
            Pen::Visible | Pen::Frame => 0.5,
            Pen::Thin | Pen::Hidden => 0.25,
        }
    }

    /// Dash and gap lengths in millimetres, for dashed pens.
    pub fn dash(self) -> Option<(f64, f64)> {
        match self {
            Pen::Hidden => Some((3.0, 1.0)),
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

/// Text on the sheet, its baseline ending at `pos` (right-aligned).
#[derive(Debug, Clone, PartialEq)]
pub struct Note {
    pub pos: DVec2,
    /// Letter height in millimetres.
    pub height: f64,
    pub text: String,
}

/// Where a view landed: the box round its lines, in sheet millimetres.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlacedView {
    pub id: u32,
    pub lo: DVec2,
    pub hi: DVec2,
    /// Where the model's origin falls on the sheet.
    pub origin: DVec2,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct SheetPicture {
    pub width: f64,
    pub height: f64,
    pub strokes: Vec<Stroke>,
    pub notes: Vec<Note>,
    pub views: Vec<PlacedView>,
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

/// Height kept free along the bottom of the frame for the scale and projection note.
const NOTE_BAND: f64 = 8.0;

/// The area inside the frame where views go: (lower-left, upper-right).
fn view_area(size: SheetSize) -> (DVec2, DVec2) {
    let (w, h) = size.size_mm();
    let pad = 4.0;
    (
        DVec2::new(FRAME_MARGIN + pad, FRAME_MARGIN + NOTE_BAND),
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
    let sheet = drawing.sheet;
    let extents: Vec<(u32, ViewRole, (DVec2, DVec2))> =
        drawing.views.iter().filter_map(|v| extent(v).map(|e| (v.id, v.role, e))).collect();
    let (alo, ahi) = view_area(sheet.size);
    let room = ahi - alo;
    let gap = view_gap(sheet.size);
    for scale in crate::sheet::STANDARD_SCALES {
        let placed = arrange(&extents, sheet.projection, scale.factor(), gap);
        let (lo, hi) = union(&placed);
        let size = hi - lo;
        if size.x <= room.x && size.y <= room.y {
            return scale;
        }
    }
    Scale { sheet: 1, model: 1000 }
}

fn union(placed: &[(u32, DVec2, DVec2, DVec2)]) -> (DVec2, DVec2) {
    placed.iter().fold((DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY)), |(lo, hi), p| {
        (lo.min(p.1), hi.max(p.2))
    })
}

/// Lays the drawing out with the views that have been projected (`geometry`, by view id).
/// Views not yet projected, or empty, are left out.
pub fn layout(drawing: &Drawing, geometry: &HashMap<u32, ViewGeometry>) -> SheetPicture {
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
    pic.notes.push(Note {
        pos: DVec2::new(w - FRAME_MARGIN - 3.0, FRAME_MARGIN + 3.0),
        height: 3.5,
        text: format!("Scale {}   {} projection", sheet.scale, sheet.projection.label().to_lowercase()),
    });

    let extents: Vec<(u32, ViewRole, (DVec2, DVec2))> = drawing
        .views
        .iter()
        .filter_map(|v| geometry.get(&v.id).and_then(|g| g.bounds()).map(|b| (v.id, v.role, b)))
        .collect();
    let s = sheet.scale.factor();
    let placed = arrange(&extents, sheet.projection, s, view_gap(sheet.size));
    if placed.is_empty() {
        return pic;
    }
    // Centre the group of views in the area inside the frame.
    let (lo, hi) = union(&placed);
    let (alo, ahi) = view_area(sheet.size);
    let shift = (alo + ahi) * 0.5 - (lo + hi) * 0.5;
    for (id, vlo, vhi, origin) in placed {
        let origin = origin + shift;
        pic.views.push(PlacedView { id, lo: vlo + shift, hi: vhi + shift, origin });
        let (Some(view), Some(geom)) = (drawing.view(id), geometry.get(&id)) else { continue };
        for line in &geom.lines {
            if let Some(pen) = pen_for(line.kind, view) {
                pic.strokes.push(Stroke { pen, path: line.path.placed(origin, s), view: Some(id) });
            }
        }
    }
    pic
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
