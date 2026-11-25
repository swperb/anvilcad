//! Section views and hatching (see `docs/re/drawings.md`, "Section views" and "Hatching").
//!
//! A section view cuts the part with a cutting line drawn on a parent view. The line is
//! horizontal or vertical on the parent and runs through the points the user clicked, in
//! order along it, stepping square to itself halfway between neighbouring points (an
//! offset section). The material between the viewer and the cutting surface is removed:
//! the cutting surface is extruded along the parent's line of sight into a prism holding
//! everything on the viewer's side, and that prism is subtracted from each exact body.
//! The section view then looks the way the line's arrows point, which is always one of
//! the six standard orthographic directions, and its cut faces are hatched after the
//! part's material.

use crate::hlr::{self, HatchArea, Orientation, Path2, Progress, ProjectError, ViewAxes, ViewGeometry};
use anvil_brep::boolean::{self, BoolOp};
use anvil_brep::profile::{Loop2, Segment, build_regions};
use anvil_brep::{Frame, Solid, Surface};
use glam::{DVec2, DVec3};
use serde::{Deserialize, Serialize};
use std::f64::consts::PI;

/// A section view's cutting line, in model space so it does not depend on how the parent
/// view is turned (the side view turns between left and right with the projection).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SectionDef {
    /// The view the cutting line is drawn on.
    pub parent: u32,
    /// The direction of sight, which the arrows point along: a unit axis direction.
    pub look: [f64; 3],
    /// The direction the cutting line runs: a unit axis direction square to `look`.
    pub along: [f64; 3],
    /// The points the line passes through, on the parent's plane.
    pub points: Vec<[f64; 3]>,
}

/// One straight piece of a cutting line: from `a0` to `a1` along the line (unbounded at
/// the line's ends), at `s` along the direction of sight.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cut {
    pub a0: f64,
    pub a1: f64,
    pub s: f64,
}

/// Why a section could not be cut, in words for the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectionError(pub String);

impl std::fmt::Display for SectionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

fn v3(a: [f64; 3]) -> DVec3 {
    DVec3::from_array(a)
}

impl SectionDef {
    /// A cutting line on a view seen along `parent_axes`, horizontal on the view (along
    /// its right) or vertical (along its up), through `points` (on the view's plane, in
    /// model millimetres). A horizontal line looks down the view and a vertical line looks
    /// to its left; [`SectionDef::flip`] turns it round.
    pub fn new(parent: u32, parent_axes: &ViewAxes, horizontal: bool, points: &[DVec2]) -> SectionDef {
        let (along, look) = if horizontal {
            (parent_axes.right, -parent_axes.up)
        } else {
            (parent_axes.up, -parent_axes.right)
        };
        SectionDef {
            parent,
            look: snap(look).to_array(),
            along: snap(along).to_array(),
            points: points.iter().map(|p| parent_axes.lift(*p).to_array()).collect(),
        }
    }

    pub fn look(&self) -> DVec3 {
        v3(self.look)
    }

    pub fn along(&self) -> DVec3 {
        v3(self.along)
    }

    /// Turns the direction of sight round.
    pub fn flip(&mut self) {
        self.look = (-self.look()).to_array();
    }

    /// The direction the section view looks from, or `None` for a line that is not square
    /// to the axes (a damaged file).
    pub fn orientation(&self) -> Option<Orientation> {
        if self.look().dot(self.along()).abs() > 1e-9 {
            return None;
        }
        Orientation::from_eye(-self.look())
    }

    /// The line's straight pieces, in order along it. Neighbouring points at the same depth
    /// make one piece.
    pub fn cuts(&self) -> Vec<Cut> {
        let (a, l) = (self.along(), self.look());
        let mut pts: Vec<(f64, f64)> = self.points.iter().map(|p| (v3(*p).dot(a), v3(*p).dot(l))).collect();
        pts.sort_by(|x, y| x.0.total_cmp(&y.0));
        let mut out: Vec<Cut> = Vec::new();
        let mut last_a = f64::NEG_INFINITY;
        for (pa, ps) in pts {
            match out.last_mut() {
                Some(c) if (c.s - ps).abs() <= 1e-9 * (1.0 + ps.abs()) => {}
                Some(c) => {
                    let j = 0.5 * (last_a + pa);
                    c.a1 = j;
                    out.push(Cut { a0: j, a1: f64::INFINITY, s: ps });
                }
                None => out.push(Cut { a0: f64::NEG_INFINITY, a1: f64::INFINITY, s: ps }),
            }
            last_a = pa;
        }
        out
    }

    /// Where the line steps, along it.
    pub fn jogs(&self) -> Vec<f64> {
        self.cuts().iter().skip(1).map(|c| c.a0).collect()
    }
}

/// The nearest axis direction to `v`.
fn snap(v: DVec3) -> DVec3 {
    let a = v.abs();
    if a.x >= a.y && a.x >= a.z {
        DVec3::X * v.x.signum()
    } else if a.y >= a.z {
        DVec3::Y * v.y.signum()
    } else {
        DVec3::Z * v.z.signum()
    }
}

/// The box round every body.
fn bounds(solids: &[Solid]) -> Option<(DVec3, DVec3)> {
    let mut out: Option<(DVec3, DVec3)> = None;
    for s in solids {
        if let Some((a, b)) = s.bounds() {
            out = Some(match out {
                Some((lo, hi)) => (lo.min(a), hi.max(b)),
                None => (a, b),
            });
        }
    }
    out
}

/// The prism holding everything on the viewer's side of the cutting surface, reaching
/// past `lo`..`hi` on every side.
fn cutter(def: &SectionDef, lo: DVec3, hi: DVec3) -> Result<Solid, SectionError> {
    let (along, look) = (def.along(), def.look());
    let depth = along.cross(look);
    let corners: Vec<DVec3> = (0..8)
        .map(|i| {
            DVec3::new(
                if i & 1 == 0 { lo.x } else { hi.x },
                if i & 2 == 0 { lo.y } else { hi.y },
                if i & 4 == 0 { lo.z } else { hi.z },
            )
        })
        .collect();
    let range = |d: DVec3| {
        corners
            .iter()
            .map(|p| p.dot(d))
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), t| (a.min(t), b.max(t)))
    };
    let pad = 0.1 * (hi - lo).length() + 1.0;
    let (amin, amax) = range(along);
    let (smin, _) = range(look);
    let (zmin, zmax) = range(depth);
    let cuts = def.cuts();
    if cuts.is_empty() {
        return Err(SectionError("the cutting line has no points; place it again".into()));
    }
    let slo = cuts.iter().map(|c| c.s).fold(smin, f64::min) - pad;
    let (a0, a1) = (amin - pad, amax + pad);
    let a0 = cuts.iter().skip(1).map(|c| c.a0).fold(a0, |m, j| m.min(j - pad));
    let a1 = cuts.iter().skip(1).map(|c| c.a0).fold(a1, |m, j| m.max(j + pad));
    // In the frame (x along the line, y along the sight, z along the parent's depth).
    let mut poly = vec![DVec2::new(a0, slo), DVec2::new(a1, slo)];
    for c in cuts.iter().rev() {
        poly.push(DVec2::new(c.a1.min(a1), c.s));
        poly.push(DVec2::new(c.a0.max(a0), c.s));
    }
    poly.dedup_by(|a, b| a.distance(*b) < 1e-12);
    let n = poly.len();
    let l = Loop2::new((0..n).map(|i| Segment::Line { a: poly[i], b: poly[(i + 1) % n] }).collect());
    let regions = build_regions(&[l])
        .map_err(|e| SectionError(format!("the cutting line makes no shape to cut with ({e})")))?;
    let Some(region) = regions.first() else {
        return Err(SectionError("the cutting line makes no shape to cut with".into()));
    };
    let frame = Frame { origin: DVec3::ZERO, x: along, y: look, z: depth };
    anvil_brep::extrude::extrude(&frame, region, zmin - pad, zmax + pad)
        .map_err(|e| SectionError(format!("the cutting tool could not be made ({e})")))
}

/// The bodies left once the material on the viewer's side of the cutting line is removed.
pub fn cut_bodies(def: &SectionDef, bodies: &[Solid]) -> Result<Vec<Solid>, SectionError> {
    let Some((lo, hi)) = bounds(bodies) else {
        return Err(SectionError("the part has no body to cut".into()));
    };
    let tool = cutter(def, lo, hi)?;
    let mut out = Vec::new();
    for b in bodies {
        let kept = boolean::boolean(b, &tool, BoolOp::Subtract).map_err(|e| {
            SectionError(format!(
                "the part cannot be cut along this line ({e}); move the cutting line a little, off edges and faces"
            ))
        })?;
        out.extend(kept);
    }
    if out.is_empty() {
        return Err(SectionError(
            "the cutting line leaves nothing to see; flip its direction or move it onto the part".into(),
        ));
    }
    Ok(out)
}

/// A point on a face's boundary.
fn face_point(solid: &Solid, face: &anvil_brep::topo::Face) -> Option<DVec3> {
    let c = face.outer.coedges.first()?;
    let e = solid.edge(c.edge)?;
    Some(e.curve.eval(e.t0))
}

/// The cut faces of `solids`: planar faces on one of the line's cutting planes, facing the
/// viewer. Returns (body, face).
fn cut_faces(def: &SectionDef, solids: &[Solid], size: f64) -> Vec<(usize, usize)> {
    let look = def.look();
    let cuts = def.cuts();
    let tol = 1e-6 * size;
    let mut out = Vec::new();
    for (b, s) in solids.iter().enumerate() {
        for (fi, f) in s.faces.iter().enumerate() {
            let Surface::Plane { .. } = f.surface else { continue };
            let Some(p) = face_point(s, f) else { continue };
            if f.normal_at(p).dot(look) > -1.0 + 1e-9 {
                continue;
            }
            let (pa, ps) = (p.dot(def.along()), p.dot(look));
            let on = cuts.iter().any(|c| (ps - c.s).abs() <= tol && pa >= c.a0 - tol && pa <= c.a1 + tol);
            if on {
                out.push((b, fi));
            }
        }
    }
    out
}

/// A face's boundary loops, sampled within `chord`, in the order their coedges run.
fn face_loops(solid: &Solid, face: &anvil_brep::topo::Face, chord: f64) -> Vec<Vec<DVec3>> {
    let mut out = Vec::new();
    for l in face.loops() {
        let mut pts: Vec<DVec3> = Vec::new();
        for c in &l.coedges {
            let Some(e) = solid.edge(c.edge) else { continue };
            let mut seg = e.curve.sample(e.t0, e.t1, chord, PI / 36.0);
            if c.reversed {
                seg.reverse();
            }
            for p in seg {
                if pts.last().is_none_or(|q| q.distance(p) > 1e-12) {
                    pts.push(p);
                }
            }
        }
        if pts.len() > 1 && pts.first().zip(pts.last()).is_some_and(|(a, b)| a.distance(*b) < 1e-9) {
            pts.pop();
        }
        if pts.len() >= 3 {
            out.push(pts);
        }
    }
    out
}

/// Projects a section view: cuts the bodies, draws what is left as seen along the arrows
/// with the line's shoulders left out, and collects the cut faces to hatch.
pub fn project_section(
    def: &SectionDef,
    bodies: &[Solid],
    pattern: HatchPattern,
    progress: &Progress,
) -> Result<Result<ViewGeometry, SectionError>, ProjectError> {
    let Some(o) = def.orientation() else {
        return Ok(Err(SectionError(
            "the cutting line is not square to the view; delete the section and make it again".into(),
        )));
    };
    let cut = match cut_bodies(def, bodies) {
        Ok(c) => c,
        Err(e) => return Ok(Err(e)),
    };
    let axes = o.axes();
    let size = bounds(&cut).map_or(1.0, |(lo, hi)| (hi - lo).length().max(1.0));
    let along = def.along();
    let jogs = def.jogs();
    let tol = 1e-6 * size;
    let in_jog = |p: DVec3| {
        let a = p.dot(along);
        jogs.iter().any(|j| (a - j).abs() <= tol)
    };
    let skip = |b: usize, e: usize| {
        cut.get(b).and_then(|s| s.edges.get(e)).is_some_and(|edge| {
            let m = 0.5 * (edge.t0 + edge.t1);
            in_jog(edge.curve.eval(edge.t0)) && in_jog(edge.curve.eval(m)) && in_jog(edge.curve.eval(edge.t1))
        })
    };
    let mut g = hlr::project_except(&cut, &axes, progress, &skip)?;
    let chord = 2e-4 * size;
    for (b, fi) in cut_faces(def, &cut, size) {
        let (Some(s), Some(f)) = (cut.get(b), cut.get(b).and_then(|s| s.faces.get(fi))) else { continue };
        let loops: Vec<Vec<DVec2>> = face_loops(s, f, chord)
            .into_iter()
            .map(|l| l.into_iter().map(|p| axes.project(p)).collect())
            .collect();
        if !loops.is_empty() {
            g.hatch.push(HatchArea { loops, body: b, pattern });
        }
    }
    Ok(Ok(g))
}

/// A family of parallel hatch lines: its turn from the base angle, its period, and its
/// lines within a period as (offset, dashes), in sheet millimetres.
type Family = (f64, f64, Vec<(f64, &'static [f64])>);

/// How a material is hatched (see `docs/re/drawings.md`, "Hatching").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum HatchPattern {
    /// Single lines: no material, or one not listed.
    #[default]
    General,
    /// Pairs of lines: steels and irons.
    Steel,
    /// Solid lines alternating with dashed lines: copper alloys.
    Copper,
    /// Solid lines alternating with long-dash dot lines: aluminium and magnesium.
    LightAlloy,
    /// Lines both ways: plastics.
    Plastic,
}

impl HatchPattern {
    /// The pattern for a material, by its name and whether it is a plastic.
    pub fn for_material(name: Option<&str>, plastic: bool) -> HatchPattern {
        if plastic {
            return HatchPattern::Plastic;
        }
        let Some(n) = name.map(str::to_lowercase) else { return HatchPattern::General };
        let has = |w: &[&str]| w.iter().any(|x| n.contains(x));
        if has(&["steel", "iron", "aisi", "astm", "17-4"]) {
            HatchPattern::Steel
        } else if has(&["brass", "bronze", "copper"]) {
            HatchPattern::Copper
        } else if has(&["alumin", "magnes", "1060", "5052", "6061", "7075"]) {
            HatchPattern::LightAlloy
        } else {
            HatchPattern::General
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            HatchPattern::General => "General",
            HatchPattern::Steel => "Steel",
            HatchPattern::Copper => "Copper alloy",
            HatchPattern::LightAlloy => "Light alloy",
            HatchPattern::Plastic => "Plastic",
        }
    }

    /// The families of parallel lines: (angle turned from the body's base angle, period,
    /// lines within a period as (offset, dashes)), in sheet millimetres. Dashes alternate
    /// on and off; an empty list is a solid line.
    fn families(self) -> Vec<Family> {
        const DASHED: &[f64] = &[2.0, 1.0];
        const CHAIN: &[f64] = &[6.0, 0.75, 0.5, 0.75];
        match self {
            HatchPattern::General => vec![(0.0, 3.0, vec![(0.0, &[])])],
            HatchPattern::Steel => vec![(0.0, 4.0, vec![(0.0, &[]), (0.8, &[])])],
            HatchPattern::Copper => vec![(0.0, 4.0, vec![(0.0, &[]), (2.0, DASHED)])],
            HatchPattern::LightAlloy => vec![(0.0, 4.0, vec![(0.0, &[]), (2.0, CHAIN)])],
            HatchPattern::Plastic => vec![(0.0, 3.0, vec![(0.0, &[])]), (PI / 2.0, 3.0, vec![(0.0, &[])])],
        }
    }
}

/// The hatch lines of an area, in model millimetres on the view plane, for a view drawn at
/// `scale` (sheet millimetres per model millimetre), so their spacing is fixed on paper.
/// Lines run at 45 degrees on even bodies and 135 degrees on odd ones.
pub fn hatch_lines(area: &HatchArea, scale: f64) -> Vec<Path2> {
    let per = 1.0 / scale.max(1e-12);
    let base = if area.body.is_multiple_of(2) { PI / 4.0 } else { 3.0 * PI / 4.0 };
    let mut out = Vec::new();
    for (turn, period, lines) in area.pattern.families() {
        let th = base + turn;
        let (u, n) = (DVec2::from_angle(th), DVec2::from_angle(th + PI / 2.0));
        let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
        for p in area.loops.iter().flatten() {
            let t = p.dot(n);
            lo = lo.min(t);
            hi = hi.max(t);
        }
        if !lo.is_finite() {
            continue;
        }
        let step = period * per;
        let k0 = (lo / step).floor() as i64 - 1;
        let k1 = (hi / step).ceil() as i64 + 1;
        // Far too many lines means a scale no one draws at; stop rather than stall.
        if k1 - k0 > 20_000 {
            continue;
        }
        for k in k0..=k1 {
            for (off, dashes) in &lines {
                let c = k as f64 * step + off * per;
                let mut xs: Vec<f64> = Vec::new();
                for lp in &area.loops {
                    for i in 0..lp.len() {
                        let (p, q) = (lp[i], lp[(i + 1) % lp.len()]);
                        let (dp, dq) = (p.dot(n) - c, q.dot(n) - c);
                        if (dp > 0.0) != (dq > 0.0) {
                            let t = dp / (dp - dq);
                            xs.push((p + (q - p) * t).dot(u));
                        }
                    }
                }
                xs.sort_by(f64::total_cmp);
                for w in xs.chunks_exact(2) {
                    let (x0, x1) = (w[0], w[1]);
                    if x1 - x0 <= 1e-12 {
                        continue;
                    }
                    let at = |x: f64| n * c + u * x;
                    if dashes.is_empty() {
                        out.push(Path2::Line { a: at(x0), b: at(x1) });
                        continue;
                    }
                    // Dashes on a grid from the view's origin, so neighbouring areas match.
                    let total: f64 = dashes.iter().sum::<f64>() * per;
                    let mut x = (x0 / total).floor() * total;
                    while x < x1 {
                        let mut d = x;
                        for (i, len) in dashes.iter().enumerate() {
                            let e = d + len * per;
                            if i % 2 == 0 {
                                let (a, b) = (d.max(x0), e.min(x1));
                                if b > a {
                                    out.push(Path2::Line { a: at(a), b: at(b) });
                                }
                            }
                            d = e;
                        }
                        x += total;
                    }
                }
            }
        }
    }
    out
}

/// The cutting line as drawn on its parent view, in the parent's plane (model
/// millimetres): the polyline through its pieces and steps, reaching `margin` past the
/// parent's lines `lo`..`hi`. Also returns the line's direction and the direction of sight
/// on the parent.
pub fn line_on_parent(
    def: &SectionDef,
    parent: &ViewAxes,
    lo: DVec2,
    hi: DVec2,
    margin: f64,
) -> (Vec<DVec2>, DVec2, DVec2) {
    let a2 = parent.project(def.along());
    let s2 = parent.project(def.look());
    let corners = [lo, DVec2::new(hi.x, lo.y), hi, DVec2::new(lo.x, hi.y)];
    let (amin, amax) = corners
        .iter()
        .map(|p| p.dot(a2))
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), t| (a.min(t), b.max(t)));
    let at = |a: f64, s: f64| a2 * a + s2 * s;
    let cuts = def.cuts();
    let mut pts = Vec::new();
    for (i, c) in cuts.iter().enumerate() {
        let a0 = if i == 0 { amin - margin } else { c.a0 };
        let a1 = if i + 1 == cuts.len() { amax + margin } else { c.a1 };
        pts.push(at(a0, c.s));
        pts.push(at(a1, c.s));
    }
    (pts, a2, s2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_full_section_is_one_cut_and_an_offset_section_steps_halfway() {
        let front = Orientation::Front.axes();
        let full = SectionDef::new(9, &front, true, &[DVec2::new(5.0, 10.0)]);
        assert_eq!(full.cuts(), vec![Cut { a0: f64::NEG_INFINITY, a1: f64::INFINITY, s: -10.0 }]);
        // Looking down the front view's horizontal line: the view is the top view.
        assert_eq!(full.orientation(), Some(Orientation::Top));
        let mut off = SectionDef::new(9, &front, false, &[DVec2::new(5.0, 30.0), DVec2::new(10.0, 10.0)]);
        // A vertical line on the front view looks left (along -X), so it is seen from +X.
        assert_eq!(off.orientation(), Some(Orientation::Right));
        assert_eq!(off.jogs(), vec![20.0]);
        assert_eq!(off.cuts()[0].s, -10.0);
        assert_eq!(off.cuts()[1].s, -5.0);
        off.flip();
        assert_eq!(off.orientation(), Some(Orientation::Left));
        // Points at the same depth make one piece.
        let same = SectionDef::new(9, &front, true, &[DVec2::new(0.0, 10.0), DVec2::new(30.0, 10.0)]);
        assert_eq!(same.cuts().len(), 1);
    }

    #[test]
    fn materials_pick_their_hatch() {
        assert_eq!(HatchPattern::for_material(Some("AISI 1045 Steel"), false), HatchPattern::Steel);
        assert_eq!(HatchPattern::for_material(Some("6061 Alloy"), false), HatchPattern::LightAlloy);
        assert_eq!(HatchPattern::for_material(Some("Brass"), false), HatchPattern::Copper);
        assert_eq!(HatchPattern::for_material(Some("ABS"), true), HatchPattern::Plastic);
        assert_eq!(HatchPattern::for_material(Some("Titanium Ti-6Al-4V"), false), HatchPattern::General);
        assert_eq!(HatchPattern::for_material(None, false), HatchPattern::General);
    }

    #[test]
    fn hatching_fills_a_square_at_45_degrees_with_fixed_paper_spacing() {
        let sq = vec![DVec2::ZERO, DVec2::new(30.0, 0.0), DVec2::new(30.0, 30.0), DVec2::new(0.0, 30.0)];
        let area = HatchArea { loops: vec![sq], body: 0, pattern: HatchPattern::General };
        let lines = hatch_lines(&area, 1.0);
        // Lines x + y = c (normal at 135 degrees), 3 mm apart across the square's
        // 30 * sqrt(2) mm diagonal: 15 lines, from -21 to 21 mm off the centre line.
        assert_eq!(lines.len(), 15);
        let total: f64 = lines
            .iter()
            .map(|l| match l {
                Path2::Line { a, b } => a.distance(*b),
                _ => 0.0,
            })
            .sum();
        // Hatch length times spacing is the area.
        assert!((total * 3.0 - 900.0).abs() < 30.0, "{total}");
        for l in &lines {
            if let Path2::Line { a, b } = l {
                let d = (*b - *a).normalize();
                assert!((d.x.abs() - d.y.abs()).abs() < 1e-9, "45 degrees");
            }
        }
        // At 1:2 the lines are 6 model millimetres apart: half as many.
        assert_eq!(hatch_lines(&area, 0.5).len(), 7);
        // A hole is left empty.
        let hole = vec![
            DVec2::new(10.0, 10.0),
            DVec2::new(20.0, 10.0),
            DVec2::new(20.0, 20.0),
            DVec2::new(10.0, 20.0),
        ];
        let holed = HatchArea { loops: vec![area.loops[0].clone(), hole], ..area.clone() };
        let inside = DVec2::new(15.0, 15.0);
        for l in hatch_lines(&holed, 1.0) {
            if let Path2::Line { a, b } = l {
                let d = b - a;
                let t = ((inside - a).dot(d) / d.length_squared()).clamp(0.0, 1.0);
                assert!((a + d * t).distance(inside) > 1.0, "no hatch in the hole");
            }
        }
    }
}
