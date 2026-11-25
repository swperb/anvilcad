//! Hole callouts (see `docs/re/drawings.md`, "Hole callouts"): a leader from a hole's edge
//! to text naming the hole's type and sizes, taken from the Hole feature's data.
//!
//! A callout stores only the view, the Hole feature and where its text has been moved to;
//! the text and the hole's place come from the rebuilt part every time the views update,
//! so the callout always says what the model has.

use crate::PartData;
use crate::dims::{ARROW, TEXT_HEIGHT, format_number};
use crate::hlr::{Orientation, Path2};
use crate::layout::{Align, Note, text_width};
use glam::{DVec2, DVec3};
use serde::{Deserialize, Serialize};

/// What a Hole feature makes, with every dimension resolved, for its callout.
#[derive(Debug, Clone, PartialEq)]
pub enum HoleType {
    Simple,
    Counterbore {
        diameter: f64,
        depth: f64,
    },
    Countersink {
        diameter: f64,
        angle: f64,
    },
    /// `thread` is the thread's name, as `M6x1` or `1/4-20 UNC`.
    Tapped {
        thread: String,
        depth: f64,
    },
}

/// One Hole feature, as a drawing needs it.
#[derive(Debug, Clone, PartialEq)]
pub struct HoleData {
    pub feature: u32,
    pub kind: HoleType,
    /// The main bore (the tap drill for a tapped hole).
    pub diameter: f64,
    /// Blind depth to the shoulder; `None` through all.
    pub depth: Option<f64>,
    /// A near-side countersink on a hole, counterbore or tapped hole: diameter and angle.
    pub near_sink: Option<(f64, f64)>,
    /// Each hole's centre on the face it is drilled from, in model space.
    pub centres: Vec<DVec3>,
    /// The direction the holes run into the material.
    pub axis: DVec3,
    /// The radius of the hole's widest circle at the face.
    pub outer_radius: f64,
}

impl HoleData {
    /// The callout's lines, top to bottom.
    pub fn callout_lines(&self) -> Vec<String> {
        let n = format_number;
        let deep = |d: Option<f64>| match d {
            Some(d) => format!("X {} DEEP", n(d)),
            None => "THRU".to_string(),
        };
        let mut first = format!("\u{d8}{} {}", n(self.diameter), deep(self.depth));
        if self.centres.len() > 1 {
            first = format!("{}X {first}", self.centres.len());
        }
        let mut out = vec![first];
        let sink = |d: f64, a: f64| format!("CSK \u{d8}{} X {}\u{b0}", n(d), n(a));
        match &self.kind {
            HoleType::Simple => {}
            HoleType::Counterbore { diameter, depth } => {
                out.push(format!("CBORE \u{d8}{} X {} DEEP", n(*diameter), n(*depth)));
            }
            HoleType::Countersink { diameter, angle } => out.push(sink(*diameter, *angle)),
            HoleType::Tapped { thread, depth } => out.push(format!("{thread} X {} DEEP", n(*depth))),
        }
        if let Some((d, a)) = self.near_sink {
            out.push(sink(d, a));
        }
        out
    }
}

/// A thread's name: the ISO designation from its major diameter and pitch, or an ANSI inch
/// size's name and series.
pub fn thread_name(size: Option<&anvil_model::hole::HoleSize>, major: f64, pitch: f64) -> String {
    match size {
        Some(s) if s.standard == anvil_model::hole::Standard::AnsiInch => {
            format!("{} UNC", s.size.trim_end_matches(" (custom)"))
        }
        _ => format!("M{}x{}", format_number(major), format_number(pitch)),
    }
}

/// A hole callout on a view.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HoleCallout {
    pub id: u32,
    pub view: u32,
    /// The Hole feature it calls out.
    pub feature: u32,
    /// How far the text has been moved from its default place, in model space on the
    /// view's plane.
    #[serde(default)]
    pub offset: [f64; 3],
}

/// A callout drawn on the sheet, in sheet millimetres.
#[derive(Debug, Clone, PartialEq)]
pub struct CalloutPicture {
    pub id: u32,
    /// The leader and its shoulder.
    pub lines: Vec<Path2>,
    pub arrow: [DVec2; 3],
    pub text: Vec<Note>,
    /// The box round the text, for picking and dragging it.
    pub text_box: (DVec2, DVec2),
}

/// Space between text lines, in letter heights.
const LINE_PITCH: f64 = 1.6;
/// The leader's horizontal shoulder, in sheet millimetres.
const SHOULDER: f64 = 4.0;

/// True when a view seen from `o` looks along `axis`.
fn square_to(o: Orientation, axis: DVec3) -> bool {
    !o.is_pictorial() && o.axes().eye.dot(axis).abs() > 1.0 - 1e-6
}

/// Draws a callout of `hole` on a view seen from `o`, whose model origin falls at `origin`
/// on the sheet at scale `s` and whose top is at `view_top` on the sheet; `None` when the
/// view does not look along the hole.
///
/// By default the leader rises to the right from the hole, steeply, until the text clears
/// the view's top by 2 mm, so the text sits in the space above the view rather than over
/// the part or the view beside it.
pub fn picture(
    c: &HoleCallout,
    hole: &HoleData,
    o: Orientation,
    origin: DVec2,
    s: f64,
    view_top: f64,
) -> Option<CalloutPicture> {
    if !square_to(o, hole.axis) {
        return None;
    }
    let ax = o.axes();
    let centre = ax.project(*hole.centres.first()?);
    let r = hole.outer_radius;
    let rows = hole.callout_lines().len().max(1) as f64;
    let c_sheet = origin + centre * s;
    let rise = (view_top + 2.0 + TEXT_HEIGHT * 0.5 + (rows - 1.0) * TEXT_HEIGHT * LINE_PITCH - c_sheet.y)
        .max(r * s + 8.0);
    let default = DVec2::new(rise * 0.5, rise) / s;
    let knee = centre + default + ax.project(DVec3::from_array(c.offset));
    let dir = (knee - centre).normalize_or(DVec2::X);
    let tip = origin + (centre + dir * r) * s;
    let k = origin + knee * s;
    let right = knee.x >= centre.x;
    let end = k + DVec2::X * if right { SHOULDER } else { -SHOULDER };
    let back = tip + (k - tip).normalize_or(DVec2::X) * ARROW.0;
    let side = (k - tip).normalize_or(DVec2::X).perp() * (ARROW.1 * 0.5);
    let lines_text = hole.callout_lines();
    let gap = 1.0;
    let x = end.x + if right { gap } else { -gap };
    let align = if right { Align::Left } else { Align::Right };
    let mut text = Vec::new();
    let mut width: f64 = 0.0;
    for (i, t) in lines_text.iter().enumerate() {
        width = width.max(text_width(t) * TEXT_HEIGHT);
        text.push(Note {
            pos: DVec2::new(x, end.y - TEXT_HEIGHT * 0.5 - i as f64 * TEXT_HEIGHT * LINE_PITCH),
            height: TEXT_HEIGHT,
            text: t.clone(),
            align,
        });
    }
    let rows = lines_text.len().max(1) as f64;
    let top = end.y + TEXT_HEIGHT * 0.5 + 0.5;
    let bottom = end.y - TEXT_HEIGHT * 0.5 - (rows - 1.0) * TEXT_HEIGHT * LINE_PITCH - 0.5;
    let (x0, x1) = if right { (x - 0.5, x + width + 0.5) } else { (x - width - 0.5, x + 0.5) };
    Some(CalloutPicture {
        id: c.id,
        lines: vec![Path2::Line { a: tip, b: k }, Path2::Line { a: k, b: end }],
        arrow: [tip, back + side, back - side],
        text,
        text_box: (DVec2::new(x0, bottom), DVec2::new(x1, top)),
    })
}

/// Callouts for every Hole feature not yet called out: each goes in the first front, top
/// or side view that looks into its holes from the side they are drilled from, or failing
/// that from the other side.
pub fn hole_callouts(drawing: &crate::Drawing, part: &PartData) -> Vec<HoleCallout> {
    use crate::document::ViewRole;
    let projection = drawing.sheet.projection;
    let order = [ViewRole::Front, ViewRole::Top, ViewRole::Side];
    let views: Vec<(u32, Orientation)> = order
        .iter()
        .filter_map(|r| {
            drawing.views.iter().find(|v| v.role == *r).map(|v| (v.id, r.orientation(projection)))
        })
        .collect();
    let mut out = Vec::new();
    for h in &part.holes {
        if drawing.callouts.iter().any(|c| c.feature == h.feature) {
            continue;
        }
        let into = views.iter().find(|(_, o)| o.axes().eye.dot(-h.axis) > 1.0 - 1e-6);
        let from_back = views.iter().find(|(_, o)| square_to(*o, h.axis));
        if let Some((view, _)) = into.or(from_back) {
            out.push(HoleCallout { id: 0, view: *view, feature: h.feature, offset: [0.0; 3] });
        }
    }
    out
}

/// Moves a callout's text by `delta` (model millimetres on a view seen from `o`).
pub fn move_text(c: &mut HoleCallout, o: Orientation, delta: DVec2) {
    let ax = o.axes();
    let d = DVec3::from_array(c.offset) + ax.lift(delta);
    c.offset = d.to_array();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hole(kind: HoleType, depth: Option<f64>, count: usize) -> HoleData {
        HoleData {
            feature: 7,
            kind,
            diameter: 6.6,
            depth,
            near_sink: None,
            centres: vec![DVec3::ZERO; count],
            axis: -DVec3::Y,
            outer_radius: 3.3,
        }
    }

    #[test]
    fn callout_text_for_each_hole_type() {
        assert_eq!(hole(HoleType::Simple, None, 1).callout_lines(), vec!["\u{d8}6.6 THRU"]);
        assert_eq!(hole(HoleType::Simple, Some(10.0), 4).callout_lines(), vec!["4X \u{d8}6.6 X 10 DEEP"]);
        assert_eq!(
            hole(HoleType::Counterbore { diameter: 11.0, depth: 6.4 }, None, 2).callout_lines(),
            vec!["2X \u{d8}6.6 THRU", "CBORE \u{d8}11 X 6.4 DEEP"]
        );
        assert_eq!(
            hole(HoleType::Countersink { diameter: 12.4, angle: 90.0 }, None, 1).callout_lines(),
            vec!["\u{d8}6.6 THRU", "CSK \u{d8}12.4 X 90\u{b0}"]
        );
        let mut tapped = hole(HoleType::Tapped { thread: "M6x1".into(), depth: 12.0 }, Some(15.0), 1);
        tapped.diameter = 5.0;
        assert_eq!(tapped.callout_lines(), vec!["\u{d8}5 X 15 DEEP", "M6x1 X 12 DEEP"]);
        tapped.near_sink = Some((7.0, 90.0));
        assert_eq!(tapped.callout_lines().last().map(String::as_str), Some("CSK \u{d8}7 X 90\u{b0}"));
    }

    #[test]
    fn thread_names_follow_the_standard() {
        use anvil_model::hole::{Fit, HoleSize, Standard};
        assert_eq!(thread_name(None, 6.0, 1.0), "M6x1");
        let iso = HoleSize { standard: Standard::IsoMetric, size: "M8".into(), fit: Fit::Normal };
        assert_eq!(thread_name(Some(&iso), 8.0, 1.25), "M8x1.25");
        let inch = HoleSize { standard: Standard::AnsiInch, size: "1/4-20".into(), fit: Fit::Normal };
        assert_eq!(thread_name(Some(&inch), 6.35, 1.27), "1/4-20 UNC");
    }

    #[test]
    fn the_leader_ends_on_the_hole_and_the_text_follows_a_drag() {
        let h = hole(HoleType::Simple, None, 1);
        let mut c = HoleCallout { id: 3, view: 2, feature: 7, offset: [0.0; 3] };
        // The top view looks down -Y, into the hole.
        let p = picture(&c, &h, Orientation::Top, DVec2::new(100.0, 100.0), 2.0, 130.0).expect("square on");
        let tip = p.arrow[0];
        assert!((tip.distance(DVec2::new(100.0, 100.0)) - 6.6).abs() < 1e-9, "on the circle at 2:1");
        assert_eq!(p.text[0].align, Align::Left);
        // The text's lowest point clears the view's top by 2 mm.
        assert!((p.text_box.0.y + 0.5 - 132.0).abs() < 1e-9, "{:?}", p.text_box);
        assert!(picture(&c, &h, Orientation::Front, DVec2::ZERO, 1.0, 0.0).is_none(), "side on");
        // Dragged to the left: the text goes left of the shoulder.
        move_text(&mut c, Orientation::Top, DVec2::new(-30.0, 0.0));
        let p = picture(&c, &h, Orientation::Top, DVec2::ZERO, 1.0, 0.0).expect("square on");
        assert_eq!(p.text[0].align, Align::Right);
    }
}
