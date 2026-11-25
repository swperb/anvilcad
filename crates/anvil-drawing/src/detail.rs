//! Detail views (see `docs/re/drawings.md`, "Detail views"): a circle on a parent view,
//! and the parent's lines and hatching inside it, drawn at a larger scale.
//!
//! A detail view is not projected again: its lines are the parent's, clipped to the
//! circle, in the parent's own view-plane coordinates. Only the scale it is drawn at
//! differs, so hatching (spaced on paper) is made at the detail's scale and clipped too.

use crate::hlr::{Path2, ViewAxes, ViewGeometry, ViewLine};
use crate::sheet::{STANDARD_SCALES, Scale};
use glam::{DVec2, DVec3};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DetailDef {
    /// The view the circle is drawn on.
    pub parent: u32,
    /// The circle's centre, in model space on the parent's plane.
    pub centre: [f64; 3],
    /// The circle's radius in model millimetres.
    pub radius: f64,
    /// The scale the detail is drawn at.
    pub scale: Scale,
}

impl DetailDef {
    /// The circle on a view seen along `axes`: centre and radius on its plane.
    pub fn circle(&self, axes: &ViewAxes) -> (DVec2, f64) {
        (axes.project(DVec3::from_array(self.centre)), self.radius)
    }
}

/// The scale a new detail of a view drawn at `parent` starts at: twice as large, rounded
/// up to the next standard scale (at most the largest).
pub fn default_scale(parent: Scale) -> Scale {
    let want = 2.0 * parent.factor();
    STANDARD_SCALES.iter().rev().find(|s| s.factor() >= want - 1e-12).copied().unwrap_or(STANDARD_SCALES[0])
}

/// Parameter values in `[s0, s1]` where the path crosses the circle, found by sampling and
/// bisection, with whether each span between them is inside.
fn spans(p: &Path2, c: DVec2, r: f64) -> Vec<(f64, f64, bool)> {
    let (s0, s1) = p.range();
    let inside = |s: f64| p.eval(s).distance_squared(c) <= r * r;
    let n = match p {
        Path2::Line { .. } => 1,
        Path2::Conic { .. } => 96,
        Path2::Poly(pts) => pts.len().saturating_sub(1).max(1) * 4,
    };
    let at = |i: usize| s0 + (s1 - s0) * i as f64 / n as f64;
    let mut cuts = vec![s0];
    match p {
        Path2::Line { a, b } => {
            // Exact: |a + t d - c|^2 = r^2.
            let d = *b - *a;
            let f = *a - c;
            let (qa, qb, qc) = (d.length_squared(), 2.0 * f.dot(d), f.length_squared() - r * r);
            let disc = qb * qb - 4.0 * qa * qc;
            if qa > 0.0 && disc > 0.0 {
                let sq = disc.sqrt();
                for t in [(-qb - sq) / (2.0 * qa), (-qb + sq) / (2.0 * qa)] {
                    if t > 0.0 && t < 1.0 {
                        cuts.push(t);
                    }
                }
            }
        }
        _ => {
            for i in 0..n {
                let (mut a, mut b) = (at(i), at(i + 1));
                if inside(a) == inside(b) {
                    continue;
                }
                let state = inside(a);
                for _ in 0..52 {
                    let m = 0.5 * (a + b);
                    if inside(m) == state {
                        a = m;
                    } else {
                        b = m;
                    }
                }
                cuts.push(0.5 * (a + b));
            }
        }
    }
    cuts.push(s1);
    cuts.windows(2).map(|w| (w[0], w[1], inside(0.5 * (w[0] + w[1])))).collect()
}

/// The pieces of `p` inside the circle.
pub fn clip_path(p: &Path2, c: DVec2, r: f64) -> Vec<Path2> {
    let sp = spans(p, c, r);
    if sp.len() == 1 {
        return if sp[0].2 { vec![p.clone()] } else { Vec::new() };
    }
    sp.into_iter().filter(|s| s.2 && s.1 > s.0).map(|(a, b, _)| p.sub(a, b)).collect()
}

/// A detail of `parent`: its lines and hatching inside the circle.
pub fn detail_geometry(parent: &ViewGeometry, c: DVec2, r: f64) -> ViewGeometry {
    let mut lines = Vec::new();
    for l in &parent.lines {
        for path in clip_path(&l.path, c, r) {
            lines.push(ViewLine { kind: l.kind, path });
        }
    }
    ViewGeometry { lines, hatch: parent.hatch.clone(), clip: Some((c, r)) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hlr::LineKind;
    use std::f64::consts::{PI, TAU};

    #[test]
    fn lines_and_arcs_are_cut_at_the_circle() {
        let c = DVec2::ZERO;
        let line = Path2::Line { a: DVec2::new(-10.0, 3.0), b: DVec2::new(10.0, 3.0) };
        let parts = clip_path(&line, c, 5.0);
        assert_eq!(parts.len(), 1);
        let Path2::Line { a, b } = parts[0] else { panic!("a line") };
        assert!(a.distance(DVec2::new(-4.0, 3.0)) < 1e-12 && b.distance(DVec2::new(4.0, 3.0)) < 1e-12);
        assert!(
            clip_path(&Path2::Line { a: DVec2::new(-10.0, 6.0), b: DVec2::new(10.0, 6.0) }, c, 5.0)
                .is_empty()
        );
        // A circle of radius 4 centred 4 to the right: inside a radius-4 circle at the origin
        // from -60 to 60 degrees (seen from its own centre, 120 to 240).
        let ring =
            Path2::Conic { c: DVec2::new(4.0, 0.0), u: DVec2::X * 4.0, v: DVec2::Y * 4.0, t0: 0.0, t1: TAU };
        let parts = clip_path(&ring, c, 4.0);
        assert_eq!(parts.len(), 1);
        let Path2::Conic { t0, t1, .. } = parts[0] else { panic!("an arc") };
        assert!((t0 - 2.0 * PI / 3.0).abs() < 1e-9 && (t1 - 4.0 * PI / 3.0).abs() < 1e-9, "{t0} {t1}");
        let g = ViewGeometry {
            lines: vec![ViewLine { kind: LineKind::Visible, path: line }],
            ..ViewGeometry::default()
        };
        let d = detail_geometry(&g, c, 5.0);
        assert_eq!(d.bounds(), Some((DVec2::splat(-5.0), DVec2::splat(5.0))));
    }

    #[test]
    fn details_start_at_twice_the_scale() {
        assert_eq!(default_scale(Scale::FULL), Scale { sheet: 2, model: 1 });
        assert_eq!(default_scale(Scale { sheet: 1, model: 2 }), Scale::FULL);
        assert_eq!(default_scale(Scale { sheet: 2, model: 1 }), Scale { sheet: 5, model: 1 });
        assert_eq!(default_scale(Scale { sheet: 50, model: 1 }), Scale { sheet: 50, model: 1 });
    }
}
