//! Hole features: standard drilled, counterbored, countersunk and tapped holes placed at
//! the points of a sketch.
//!
//! The behaviour follows the specification in `docs/re/hole-wizard.md`. Each hole is a
//! cutter made by turning a half-section about the hole's axis, so it is built exactly by
//! the exact kernel (planes, cylinders and cones) and falls back to the faceted kernel
//! like any revolve.

use crate::document::Dim;
use anvil_kernel::{v2, Vec2};
use anvil_sketch::{Geom, Sketch, ORIGIN};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Where a hole ends.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum HoleEnd {
    /// To a depth, measured from the placement face to the end of the full diameter.
    #[default]
    Blind,
    /// Through everything along the hole's axis.
    ThroughAll,
}

impl HoleEnd {
    pub const ALL: [HoleEnd; 2] = [HoleEnd::Blind, HoleEnd::ThroughAll];

    pub fn label(&self) -> &'static str {
        match self {
            HoleEnd::Blind => "Blind",
            HoleEnd::ThroughAll => "Through All",
        }
    }
}

/// A cone opening the hole at the face: its diameter at the face, and its included angle
/// (degrees).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Sink {
    pub diameter: Dim,
    pub angle: Dim,
}

/// The shape of the hole above its main bore.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum HoleKind {
    /// A plain drilled hole.
    Simple,
    /// A wider, flat-bottomed bore at the face, for a cap screw head.
    Counterbore { diameter: Dim, depth: Dim },
    /// A cone at the face, for a flat (countersunk) screw head.
    Countersink(Sink),
    /// A hole drilled at the tap drill diameter for a thread of `major` diameter and
    /// `pitch`, threaded to `thread_depth`. The thread is recorded, not modelled: the
    /// solid shows the tap drill hole, as threads are shown in drawings.
    Tapped { major: Dim, pitch: Dim, thread_depth: Dim },
}

impl HoleKind {
    pub const LABELS: [&'static str; 4] = ["Hole", "Counterbore", "Countersink", "Tapped hole"];

    pub fn label(&self) -> &'static str {
        Self::LABELS[self.index()]
    }

    pub fn index(&self) -> usize {
        match self {
            HoleKind::Simple => 0,
            HoleKind::Counterbore { .. } => 1,
            HoleKind::Countersink(_) => 2,
            HoleKind::Tapped { .. } => 3,
        }
    }
}

/// Which table a hole's sizes were taken from, kept so the editor can show and change
/// the size. The dimensions on the feature are what build the hole.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HoleSize {
    pub standard: Standard,
    /// The fastener size, e.g. "M6" or "1/4-20".
    pub size: String,
    #[serde(default)]
    pub fit: Fit,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Standard {
    #[default]
    IsoMetric,
    AnsiInch,
}

impl Standard {
    pub const ALL: [Standard; 2] = [Standard::IsoMetric, Standard::AnsiInch];

    pub fn label(&self) -> &'static str {
        match self {
            Standard::IsoMetric => "ISO metric",
            Standard::AnsiInch => "ANSI inch",
        }
    }

    pub fn sizes(&self) -> &'static [FastenerSize] {
        match self {
            Standard::IsoMetric => ISO_METRIC,
            Standard::AnsiInch => ANSI_INCH,
        }
    }

    /// Included angle of the standard's flat head screws, degrees.
    pub fn csink_angle(&self) -> f64 {
        match self {
            Standard::IsoMetric => CSINK_ANGLE,
            Standard::AnsiInch => 82.0,
        }
    }

    pub fn size(&self, name: &str) -> Option<&'static FastenerSize> {
        let name = name.trim_end_matches(" (custom)");
        self.sizes().iter().find(|s| s.name == name)
    }
}

/// How much clearance a clearance hole gives the screw.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Fit {
    Close,
    #[default]
    Normal,
    Loose,
}

impl Fit {
    pub const ALL: [Fit; 3] = [Fit::Close, Fit::Normal, Fit::Loose];

    pub fn label(&self) -> &'static str {
        match self {
            Fit::Close => "Close",
            Fit::Normal => "Normal",
            Fit::Loose => "Loose",
        }
    }
}

/// A hole made at every point of a sketch.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HoleFeature {
    /// The sketch whose free points (points that are not the origin and belong to no
    /// line, arc or circle) place the holes. The holes run square to its plane.
    pub sketch: u32,
    pub kind: HoleKind,
    /// The main bore. For a tapped hole this is the tap drill diameter.
    pub diameter: Dim,
    pub end: HoleEnd,
    /// Blind depth to the end of the full diameter; the drill point lies beyond it.
    pub depth: Dim,
    /// Included angle of the drill point (degrees) for a blind hole, or `None` for a
    /// flat bottom.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drill_angle: Option<Dim>,
    /// A cone at the face on top of a counterbore, plain or tapped hole.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub near_sink: Option<Sink>,
    /// Drill out of the sketch plane's normal side instead of into it.
    #[serde(default)]
    pub reverse: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<HoleSize>,
}

impl HoleFeature {
    /// A hole of `kind` sized for `size` from `standard`, with sizes filled from the
    /// standard's tables. Returns `None` for a size the standard does not have.
    pub fn standard(
        sketch: u32,
        kind: usize,
        standard: Standard,
        size: &str,
        fit: Fit,
    ) -> Option<HoleFeature> {
        let s = standard.size(size)?;
        let mut h = HoleFeature {
            sketch,
            kind: HoleKind::Simple,
            diameter: Dim::Value(s.clearance(fit)),
            end: HoleEnd::ThroughAll,
            depth: Dim::Value(3.0 * s.major),
            drill_angle: Some(Dim::Value(DRILL_ANGLE)),
            near_sink: None,
            reverse: false,
            size: Some(HoleSize { standard, size: size.into(), fit }),
        };
        h.set_kind(kind);
        Some(h)
    }

    /// Changes the hole type, refilling the type's sizes from the hole's standard size
    /// where it has one.
    pub fn set_kind(&mut self, kind: usize) {
        let std = self.size.as_ref().and_then(|z| z.standard.size(&z.size).map(|s| (s, z.fit)));
        let csink_angle = self.size.as_ref().map_or(CSINK_ANGLE, |z| z.standard.csink_angle());
        let major = std.map(|(s, _)| s.major).unwrap_or_else(|| self.diameter_hint());
        let fit = std.map(|(_, f)| f).unwrap_or_default();
        let fallback = FastenerSize::proportional(major);
        let s = std.map(|(s, _)| s).unwrap_or(&fallback);
        self.kind = match kind {
            1 => HoleKind::Counterbore {
                diameter: Dim::Value(s.cbore_diameter),
                depth: Dim::Value(s.cbore_depth),
            },
            2 => HoleKind::Countersink(Sink {
                diameter: Dim::Value(s.csink_diameter),
                angle: Dim::Value(csink_angle),
            }),
            3 => HoleKind::Tapped {
                major: Dim::Value(s.major),
                pitch: Dim::Value(s.pitch),
                thread_depth: Dim::Value(2.0 * s.major),
            },
            _ => HoleKind::Simple,
        };
        self.diameter = Dim::Value(if kind == 3 { s.tap_drill } else { s.clearance(fit) });
        if kind == 3 {
            self.end = HoleEnd::Blind;
            self.depth = Dim::Value(2.5 * s.major);
        }
    }

    /// The fastener diameter this hole most likely suits, for a hole with no size.
    fn diameter_hint(&self) -> f64 {
        match (&self.kind, &self.diameter) {
            (HoleKind::Tapped { major: Dim::Value(m), .. }, _) => *m,
            (_, Dim::Value(d)) => *d / 1.1,
            _ => 6.0,
        }
    }

    /// Resolves every dimension and checks the hole is a shape that can be made.
    pub fn resolve(&self, vars: &BTreeMap<String, f64>) -> Result<HoleSection, String> {
        let num =
            |d: &Dim, what: &str| -> Result<f64, String> { d.eval(vars).map_err(|e| format!("{what}: {e}")) };
        let sink = |s: &Sink, what: &str| -> Result<(f64, f64), String> {
            Ok((num(&s.diameter, &format!("{what} diameter"))?, num(&s.angle, &format!("{what} angle"))?))
        };
        let mut sec = HoleSection {
            diameter: num(&self.diameter, "diameter")?,
            depth: match self.end {
                HoleEnd::Blind => Some(num(&self.depth, "depth")?),
                HoleEnd::ThroughAll => None,
            },
            drill_angle: match (&self.drill_angle, self.end) {
                (Some(a), HoleEnd::Blind) => Some(num(a, "drill angle")?),
                _ => None,
            },
            counterbore: None,
            sink: self.near_sink.as_ref().map(|s| sink(s, "countersink")).transpose()?,
        };
        match &self.kind {
            HoleKind::Simple => {}
            HoleKind::Counterbore { diameter, depth } => {
                sec.counterbore =
                    Some((num(diameter, "counterbore diameter")?, num(depth, "counterbore depth")?));
            }
            HoleKind::Countersink(s) => sec.sink = Some(sink(s, "countersink")?),
            HoleKind::Tapped { major, pitch, thread_depth } => {
                let (m, p, t) = (
                    num(major, "thread diameter")?,
                    num(pitch, "pitch")?,
                    num(thread_depth, "thread depth")?,
                );
                if !wider(m, sec.diameter) {
                    return Err("the thread diameter must be larger than the tap drill".into());
                }
                if !wider(p, 0.0) || !wider(t, 0.0) {
                    return Err("the thread pitch and depth must be positive".into());
                }
                if sec.depth.is_some_and(|d| t > d + 1e-9) {
                    return Err("the thread cannot be deeper than the hole".into());
                }
            }
        }
        sec.check()?;
        Ok(sec)
    }
}

/// Included angle of a standard twist drill point, degrees.
pub const DRILL_ANGLE: f64 = 118.0;
/// Included angle of an ISO metric flat head countersink, degrees.
pub const CSINK_ANGLE: f64 = 90.0;

/// A hole's resolved dimensions, in millimetres and degrees.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HoleSection {
    pub diameter: f64,
    /// `None` for a through hole.
    pub depth: Option<f64>,
    pub drill_angle: Option<f64>,
    /// Diameter and depth.
    pub counterbore: Option<(f64, f64)>,
    /// Diameter at the face and included angle.
    pub sink: Option<(f64, f64)>,
}

impl HoleSection {
    fn check(&self) -> Result<(), String> {
        let positive = |v: f64| v.is_finite() && v > 0.0;
        if !positive(self.diameter) {
            return Err("the hole diameter must be positive".into());
        }
        if let Some(d) = self.depth {
            if !positive(d) {
                return Err("the hole depth must be positive".into());
            }
        }
        if let Some(a) = self.drill_angle {
            if !(a > 0.0 && a < 180.0) {
                return Err("the drill point angle must be between 0 and 180 degrees".into());
            }
        }
        let mut bore = self.diameter;
        let mut top = 0.0;
        if let Some((d, h)) = self.counterbore {
            if !wider(d, self.diameter) {
                return Err("the counterbore must be wider than the hole".into());
            }
            if !positive(h) {
                return Err("the counterbore depth must be positive".into());
            }
            if self.depth.is_some_and(|depth| h >= depth) {
                return Err("the counterbore must be shallower than the hole".into());
            }
            bore = d;
            top = h;
        }
        if let Some((d, a)) = self.sink {
            if !(a > 0.0 && a < 180.0) {
                return Err("the countersink angle must be between 0 and 180 degrees".into());
            }
            if !wider(d, bore) {
                return Err("the countersink must be wider than the bore it opens".into());
            }
            let sink_depth = 0.5 * (d - bore) / (0.5 * a).to_radians().tan();
            let floor = if self.counterbore.is_some() { top } else { self.depth.unwrap_or(f64::INFINITY) };
            if sink_depth >= floor {
                return Err(if self.counterbore.is_some() {
                    "the countersink is deeper than the counterbore".into()
                } else {
                    "the countersink is deeper than the hole".into()
                });
            }
        }
        Ok(())
    }

    /// The half-section of the cutter: points `(radius, height)` round a closed outline,
    /// with the face at height 0 and the material below it. The outline starts on the
    /// axis at the face and ends on the axis at the bottom. `through` is the depth a
    /// through hole reaches.
    pub fn outline(&self, through: f64) -> Vec<Vec2> {
        let r = 0.5 * self.diameter;
        let mut pts = vec![v2(0.0, 0.0)];
        let mut y = 0.0;
        // The radius the countersink opens onto.
        let below_sink = self.counterbore.map_or(r, |(d, _)| 0.5 * d);
        if let Some((d, a)) = self.sink {
            let rs = 0.5 * d;
            pts.push(v2(rs, 0.0));
            y = -(rs - below_sink) / (0.5 * a).to_radians().tan();
            pts.push(v2(below_sink, y));
        }
        if let Some((d, h)) = self.counterbore {
            if pts.len() == 1 {
                pts.push(v2(0.5 * d, 0.0));
            }
            pts.push(v2(0.5 * d, -h));
            pts.push(v2(r, -h));
            y = -h;
        }
        if pts.len() == 1 {
            pts.push(v2(r, y));
        }
        let depth = self.depth.unwrap_or(through);
        pts.push(v2(r, -depth));
        match self.drill_angle {
            Some(a) if self.depth.is_some() => pts.push(v2(0.0, -depth - r / (0.5 * a).to_radians().tan())),
            _ => pts.push(v2(0.0, -depth)),
        }
        pts
    }

    /// The cutter's volume, from the outline (Pappus), for checks.
    pub fn volume(&self, through: f64) -> f64 {
        let p = self.outline(through);
        let mut v = 0.0;
        for i in 0..p.len() {
            let (a, b) = (p[i], p[(i + 1) % p.len()]);
            // Volume swept by the segment's trapezoid to the axis.
            v += (a.y - b.y) * (a.x * a.x + a.x * b.x + b.x * b.x);
        }
        (std::f64::consts::PI / 3.0 * v).abs()
    }
}

/// `a > b`, false when either is NaN.
fn wider(a: f64, b: f64) -> bool {
    a > b
}

/// Where the holes go: the sketch's free points, in sketch coordinates.
pub fn positions(sketch: &Sketch) -> Vec<Vec2> {
    let mut used = vec![false; sketch.points.len()];
    if let Some(u) = used.get_mut(ORIGIN) {
        *u = true;
    }
    for e in &sketch.entities {
        let ids: &[usize] = match &e.geom {
            Geom::Line { a, b } => &[*a, *b],
            Geom::Circle { c, .. } => &[*c],
            Geom::Arc { c, a, b } => &[*c, *a, *b],
        };
        for &p in ids {
            if let Some(u) = used.get_mut(p) {
                *u = true;
            }
        }
    }
    (0..sketch.points.len()).filter(|&p| !used[p]).map(|p| sketch.pos(p)).collect()
}

/// Sizes for one fastener, in millimetres.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FastenerSize {
    pub name: &'static str,
    /// Nominal (major) thread diameter.
    pub major: f64,
    /// Coarse thread pitch.
    pub pitch: f64,
    pub tap_drill: f64,
    /// Clearance hole diameters: close, normal, loose.
    pub clearance: [f64; 3],
    /// Counterbore for a socket head cap screw.
    pub cbore_diameter: f64,
    pub cbore_depth: f64,
    /// Countersink diameter at the face for a flat head screw.
    pub csink_diameter: f64,
}

impl FastenerSize {
    pub fn clearance(&self, fit: Fit) -> f64 {
        self.clearance[fit as usize]
    }

    /// Sizes in proportion to a nominal diameter, for holes with no standard size.
    fn proportional(major: f64) -> FastenerSize {
        let m = major.max(0.1);
        FastenerSize {
            name: "",
            major: m,
            pitch: 0.15 * m,
            tap_drill: 0.85 * m,
            clearance: [1.05 * m, 1.1 * m, 1.2 * m],
            cbore_diameter: 1.8 * m,
            cbore_depth: m,
            csink_diameter: 2.0 * m,
        }
    }
}

macro_rules! sizes {
    ($($name:literal $major:literal $pitch:literal $tap:literal [$c:literal $n:literal $l:literal] $cbd:literal $cbh:literal $csd:literal;)*) => {
        &[$(FastenerSize {
            name: $name,
            major: $major,
            pitch: $pitch,
            tap_drill: $tap,
            clearance: [$c, $n, $l],
            cbore_diameter: $cbd,
            cbore_depth: $cbh,
            csink_diameter: $csd,
        }),*]
    };
}

/// ISO metric coarse sizes. The sources and the rules behind each column are in
/// `docs/re/hole-wizard.md`: ISO 261 coarse pitches; tap drills at the usual drill size
/// near the nominal diameter less the pitch; ISO 273 fine, medium and coarse clearance
/// holes; DIN 974-1 counterbores for ISO 4762 socket head cap screws, as deep as the
/// head is high plus 0.5 mm; and countersinks to the ISO 10642 flat head diameter.
pub const ISO_METRIC: &[FastenerSize] = sizes![
    "M2"   2.0  0.4  1.6  [2.2  2.4  2.6]  4.4  2.5  4.48;
    "M2.5" 2.5  0.45 2.05 [2.7  2.9  3.1]  5.5  3.0  5.6;
    "M3"   3.0  0.5  2.5  [3.2  3.4  3.6]  6.5  3.5  6.72;
    "M4"   4.0  0.7  3.3  [4.3  4.5  4.8]  8.0  4.5  8.96;
    "M5"   5.0  0.8  4.2  [5.3  5.5  5.8]  10.0 5.5  11.2;
    "M6"   6.0  1.0  5.0  [6.4  6.6  7.0]  11.0 6.5  13.44;
    "M8"   8.0  1.25 6.8  [8.4  9.0  10.0] 15.0 8.5  17.92;
    "M10"  10.0 1.5  8.5  [10.5 11.0 12.0] 18.0 10.5 22.4;
    "M12"  12.0 1.75 10.2 [13.0 13.5 14.5] 20.0 12.5 26.88;
    "M16"  16.0 2.0  14.0 [17.0 17.5 18.5] 26.0 16.5 33.6;
    "M20"  20.0 2.5  17.5 [21.0 22.0 24.0] 33.0 20.5 40.32;
];

const IN: f64 = 25.4;

macro_rules! inch_sizes {
    ($($name:literal $major:literal $tpi:literal $tap:literal [$c:literal $n:literal] $head:literal $height:literal;)*) => {
        &[$(FastenerSize {
            name: $name,
            major: $major * IN,
            pitch: IN / $tpi,
            tap_drill: $tap * IN,
            clearance: [$c * IN, $n * IN, ($n + 1.0 / 64.0) * IN],
            cbore_diameter: ($head + 1.0 / 32.0) * IN,
            cbore_depth: ($height + 0.02) * IN,
            csink_diameter: 2.0 * $major * IN,
        }),*]
    };
}

/// Unified coarse (UNC) inch sizes, held in millimetres. Tap drills and the close and free
/// clearance drills are the usual number, letter and fractional drills; loose is the free
/// drill plus 1/64 in. Counterbores clear an ASME B18.3 socket head cap screw head by
/// 1/32 in across and 0.02 in deep. Countersinks are 82 degrees, twice the nominal
/// diameter across. See `docs/re/hole-wizard.md`.
pub const ANSI_INCH: &[FastenerSize] = inch_sizes![
    "#4-40"   0.112  40.0 0.0890 [0.1160 0.1285] 0.183 0.112;
    "#6-32"   0.138  32.0 0.1065 [0.1440 0.1495] 0.226 0.138;
    "#8-32"   0.164  32.0 0.1360 [0.1695 0.1770] 0.270 0.164;
    "#10-24"  0.190  24.0 0.1495 [0.1960 0.2010] 0.312 0.190;
    "1/4-20"  0.250  20.0 0.2010 [0.2570 0.2660] 0.375 0.250;
    "5/16-18" 0.3125 18.0 0.2570 [0.3230 0.3320] 0.469 0.3125;
    "3/8-16"  0.375  16.0 0.3125 [0.3860 0.3970] 0.5625 0.375;
    "1/2-13"  0.500  13.0 0.4219 [0.5156 0.5313] 0.750 0.500;
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::*;
    use crate::BuildState;
    use anvil_kernel::FaceId;
    use std::f64::consts::PI;

    /// A 40 x 40 x 20 block on the Front plane, and a sketch on its top face with a free
    /// point at each of `at`.
    fn block_with_points(at: &[(f64, f64)]) -> (Document, u32, u32) {
        let mut doc = Document::new();
        let mut base = SketchFeature::new(PlaneRef::Front);
        base.sketch.add_rectangle(v2(-20.0, -20.0), v2(20.0, 20.0));
        let base = doc.add_feature(FeatureKind::Sketch(base));
        let ex = doc.add_feature(FeatureKind::Extrude(ExtrudeFeature {
            sketch: base,
            end: EndCondition::Blind,
            depth: Dim::Value(20.0),
            reverse: false,
            op: BodyOp::Boss,
            ..Default::default()
        }));
        let mut sf = SketchFeature::new(PlaneRef::Face(FaceId { feature: ex, index: FaceId::END_CAP }));
        for &(x, y) in at {
            sf.sketch.add_point(v2(x, y));
        }
        let sk = doc.add_feature(FeatureKind::Sketch(sf));
        (doc, ex, sk)
    }

    fn exact_volume(st: &BuildState) -> f64 {
        let fine = anvil_brep::tess::TessParams { chord: 1e-4, max_angle: 0.005 };
        st.exact
            .as_ref()
            .expect("the part should still be exact")
            .iter()
            .map(|b| {
                anvil_brep::mass::properties(&anvil_brep::tess::tessellate(b, &fine).expect("tess")).volume
            })
            .sum()
    }

    const BLOCK: f64 = 40.0 * 40.0 * 20.0;

    /// Cuts `hole` and checks the exact and faceted volumes removed against `removed`.
    fn check_cut(
        at: &[(f64, f64)],
        hole: impl FnOnce(u32) -> HoleFeature,
        removed: f64,
    ) -> (Document, BuildState) {
        let (mut doc, _, sk) = block_with_points(at);
        doc.add_feature(FeatureKind::Hole(hole(sk)));
        let st = doc.rebuild();
        assert!(st.errors.is_empty(), "{:?}", st.errors);
        assert_eq!(st.faceted_from, None);
        let expected = BLOCK - removed;
        let v = exact_volume(&st);
        assert!((v - expected).abs() / expected < 1e-6, "exact {v} vs {expected}");
        let f = st.body.mass_properties().volume;
        assert!((f - expected).abs() / expected < 2e-3, "faceted {f} vs {expected}");
        (doc, st)
    }

    fn simple(sketch: u32, diameter: f64, end: HoleEnd, depth: f64) -> HoleFeature {
        HoleFeature {
            sketch,
            kind: HoleKind::Simple,
            diameter: Dim::Value(diameter),
            end,
            depth: Dim::Value(depth),
            drill_angle: Some(Dim::Value(DRILL_ANGLE)),
            near_sink: None,
            reverse: false,
            size: None,
        }
    }

    #[test]
    fn blind_drilled_hole_has_a_118_degree_point() {
        let (r, depth) = (4.0, 12.0);
        let tip = r / (59.0f64).to_radians().tan();
        let removed = PI * r * r * depth + PI * r * r * tip / 3.0;
        let (_, st) = check_cut(&[(0.0, 0.0)], |sk| simple(sk, 2.0 * r, HoleEnd::Blind, depth), removed);
        // The point ends below the full diameter, inside the block.
        let b = st.body.bounds();
        assert!((b.max.z - 20.0).abs() < 1e-9 && b.min.z.abs() < 1e-9);
    }

    #[test]
    fn flat_bottomed_blind_hole() {
        let removed = PI * 9.0 * 7.5;
        check_cut(
            &[(5.0, -5.0)],
            |sk| HoleFeature { drill_angle: None, ..simple(sk, 6.0, HoleEnd::Blind, 7.5) },
            removed,
        );
    }

    #[test]
    fn two_through_counterbores_for_m6_cap_screws() {
        let s = Standard::IsoMetric.size("M6").expect("M6");
        let removed = 2.0
            * PI
            * (0.25 * s.cbore_diameter.powi(2) * s.cbore_depth
                + 0.25 * 6.6f64.powi(2) * (20.0 - s.cbore_depth));
        let (doc, _) = check_cut(
            &[(-10.0, 0.0), (10.0, 0.0)],
            |sk| HoleFeature::standard(sk, 1, Standard::IsoMetric, "M6", Fit::Normal).expect("M6"),
            removed,
        );
        // The standard sizes are what the feature stores.
        let Some(FeatureKind::Hole(h)) = doc.features.last().map(|f| &f.kind) else { unreachable!() };
        assert_eq!(h.diameter, Dim::Value(6.6));
        assert_eq!(h.end, HoleEnd::ThroughAll);
        assert_eq!(h.kind, HoleKind::Counterbore { diameter: Dim::Value(11.0), depth: Dim::Value(6.5) });
    }

    #[test]
    fn through_countersink_at_90_degrees() {
        let (rs, r) = (6.0, 3.0);
        let h = rs - r; // 90 degrees: the cone drops as fast as it narrows.
        let removed = PI * h / 3.0 * (rs * rs + rs * r + r * r) + PI * r * r * (20.0 - h);
        check_cut(
            &[(0.0, 0.0)],
            |sk| HoleFeature {
                kind: HoleKind::Countersink(Sink { diameter: Dim::Value(12.0), angle: Dim::Value(90.0) }),
                ..simple(sk, 6.0, HoleEnd::ThroughAll, 0.0)
            },
            removed,
        );
    }

    #[test]
    fn tapped_hole_is_drilled_at_the_tap_drill() {
        let s = Standard::IsoMetric.size("M5").expect("M5");
        let r = 0.5 * s.tap_drill;
        let depth = 2.5 * s.major;
        let tip = r / (59.0f64).to_radians().tan();
        let removed = PI * r * r * depth + PI * r * r * tip / 3.0;
        let (doc, _) = check_cut(
            &[(0.0, 0.0)],
            |sk| HoleFeature::standard(sk, 3, Standard::IsoMetric, "M5", Fit::Normal).expect("M5"),
            removed,
        );
        let Some(FeatureKind::Hole(h)) = doc.features.last().map(|f| &f.kind) else { unreachable!() };
        assert_eq!(h.diameter, Dim::Value(4.2));
        assert_eq!(h.end, HoleEnd::Blind);
    }

    #[test]
    fn counterbore_with_a_near_side_countersink() {
        let (rc, hc, r, rs) = (5.5, 6.0, 3.3, 6.5);
        let h = rs - rc;
        let removed =
            PI * h / 3.0 * (rs * rs + rs * rc + rc * rc) + PI * rc * rc * (hc - h) + PI * r * r * (20.0 - hc);
        check_cut(
            &[(0.0, 0.0)],
            |sk| HoleFeature {
                kind: HoleKind::Counterbore { diameter: Dim::Value(2.0 * rc), depth: Dim::Value(hc) },
                near_sink: Some(Sink { diameter: Dim::Value(2.0 * rs), angle: Dim::Value(90.0) }),
                ..simple(sk, 2.0 * r, HoleEnd::ThroughAll, 0.0)
            },
            removed,
        );
    }

    #[test]
    fn hole_follows_a_variable_and_patterns() {
        let (mut doc, _, sk) = block_with_points(&[(-15.0, 0.0)]);
        doc.variables.push(GlobalVariable { name: "d".into(), expr: "5".into(), comment: String::new() });
        let hole = doc.add_feature(FeatureKind::Hole(HoleFeature {
            diameter: Dim::Expr("d".into()),
            ..simple(sk, 0.0, HoleEnd::ThroughAll, 0.0)
        }));
        let frame = doc.rebuild().frames[&sk];
        doc.add_feature(FeatureKind::LinearPattern(LinearPatternFeature {
            features: vec![hole],
            direction: Axis::Line { origin: anvil_kernel::Vec3::ZERO, dir: frame.x },
            spacing: Dim::Value(10.0),
            count: Dim::Value(3.0),
        }));
        let st = doc.rebuild();
        assert!(st.errors.is_empty(), "{:?}", st.errors);
        let expected = BLOCK - 3.0 * PI * 6.25 * 20.0;
        let v = exact_volume(&st);
        assert!((v - expected).abs() / expected < 1e-6, "{v} vs {expected}");
    }

    #[test]
    fn section_volume_matches_the_formula() {
        let sec = HoleSection {
            diameter: 6.6,
            depth: None,
            drill_angle: Some(118.0),
            counterbore: Some((11.0, 6.5)),
            sink: None,
        };
        let expected = PI * (5.5f64.powi(2) * 6.5 + 3.3f64.powi(2) * 13.5);
        assert!((sec.volume(20.0) - expected).abs() < 1e-9);
    }

    #[test]
    fn impossible_holes_are_errors() {
        let vars = BTreeMap::new();
        let mut h = simple(0, 6.0, HoleEnd::Blind, 10.0);
        h.kind = HoleKind::Counterbore { diameter: Dim::Value(5.0), depth: Dim::Value(3.0) };
        assert_eq!(h.resolve(&vars).err().as_deref(), Some("the counterbore must be wider than the hole"));
        h.kind = HoleKind::Counterbore { diameter: Dim::Value(10.0), depth: Dim::Value(12.0) };
        assert_eq!(
            h.resolve(&vars).err().as_deref(),
            Some("the counterbore must be shallower than the hole")
        );
        h.kind = HoleKind::Countersink(Sink { diameter: Dim::Value(40.0), angle: Dim::Value(90.0) });
        assert_eq!(h.resolve(&vars).err().as_deref(), Some("the countersink is deeper than the hole"));
        h.kind = HoleKind::Tapped {
            major: Dim::Value(8.0),
            pitch: Dim::Value(1.25),
            thread_depth: Dim::Value(11.0),
        };
        assert_eq!(h.resolve(&vars).err().as_deref(), Some("the thread cannot be deeper than the hole"));
        h.kind = HoleKind::Simple;
        h.diameter = Dim::Value(-1.0);
        assert_eq!(h.resolve(&vars).err().as_deref(), Some("the hole diameter must be positive"));

        // No points to drill at: the feature fails and the block stays whole.
        let (mut doc, _, sk) = block_with_points(&[]);
        let id = doc.add_feature(FeatureKind::Hole(simple(sk, 6.0, HoleEnd::ThroughAll, 0.0)));
        let st = doc.rebuild();
        assert!(st.errors[&id].contains("no points"));
        assert!((st.body.mass_properties().volume - BLOCK).abs() < 1e-6);
    }

    #[test]
    fn only_free_points_place_holes() {
        let mut s = Sketch::new();
        let a = s.add_point(v2(1.0, 2.0));
        let b = s.add_point(v2(3.0, 4.0));
        s.add_line(a, b);
        let c = s.add_point(v2(5.0, 6.0));
        s.add_circle(c, 1.0);
        s.add_point(v2(7.0, 8.0));
        assert_eq!(positions(&s), vec![v2(7.0, 8.0)]);
    }

    #[test]
    fn holes_survive_save_and_load() {
        let (mut doc, _, sk) = block_with_points(&[(0.0, 0.0)]);
        doc.add_feature(FeatureKind::Hole(
            HoleFeature::standard(sk, 2, Standard::IsoMetric, "M5", Fit::Close).expect("M5"),
        ));
        let back = Document::from_json(&doc.to_json()).expect("load");
        assert_eq!(back.features, doc.features);
    }
}
