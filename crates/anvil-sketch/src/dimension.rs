//! The Dimension tool: one command that works out which dimension to add from what the
//! user picked and where they place the value, and the drafting geometry (extension
//! lines, dimension lines, arrows) used to draw dimensions.
//!
//! The behaviour follows the specification in `docs/re/sketch-dimension.md`, written from
//! published SOLIDWORKS help on how its sketch dimensioning tool infers dimensions.

use crate::sketch::{ccw_angle, Constraint, EntityId, Geom, PointId, Sketch};
use anvil_kernel::{v2, Vec2};
use std::f64::consts::{FRAC_PI_4, PI};

/// Something picked with the Dimension tool.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pick {
    Point(PointId),
    Entity(EntityId),
}

/// The most picks a single dimension uses (an arc and its two end points).
pub const MAX_PICKS: usize = 3;

/// Lines closer to parallel than this (in radians) are dimensioned by distance, not angle.
const PARALLEL_TOL: f64 = 1e-6;

/// Which way a linear dimension between two places measures, chosen by where its value is
/// placed: beside the pair it measures across, above or below it measures along x, and to
/// the left or right it measures along y.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Linear {
    Aligned,
    Horizontal,
    Vertical,
}

fn linear_direction(p: Vec2, q: Vec2, at: Vec2) -> Linear {
    let d = q - p;
    let len = d.length();
    // Points that already line up horizontally or vertically only have the one distance.
    if d.y.abs() <= 1e-9 * len.max(1.0) {
        return Linear::Horizontal;
    }
    if d.x.abs() <= 1e-9 * len.max(1.0) {
        return Linear::Vertical;
    }
    let (lo, hi) = (v2(p.x.min(q.x), p.y.min(q.y)), v2(p.x.max(q.x), p.y.max(q.y)));
    let inside_x = at.x >= lo.x && at.x <= hi.x;
    let inside_y = at.y >= lo.y && at.y <= hi.y;
    match (inside_x, inside_y) {
        (true, false) => Linear::Horizontal,
        (false, true) => Linear::Vertical,
        _ => Linear::Aligned,
    }
}

fn axis_aligned(p: Vec2, q: Vec2) -> bool {
    let d = q - p;
    let tol = 1e-9 * d.length().max(1.0);
    d.x.abs() <= tol || d.y.abs() <= tol
}

/// A distance between two points, horizontal, vertical or aligned depending on `at`.
fn point_distance(sk: &Sketch, p: PointId, q: PointId, at: Vec2) -> Option<Constraint> {
    let (a, b) = (sk.pos(p), sk.pos(q));
    if a.distance(b) < 1e-12 {
        return None;
    }
    Some(match linear_direction(a, b, at) {
        Linear::Aligned => Constraint::Distance { p, q, value: a.distance(b) },
        Linear::Horizontal => Constraint::HDistance { p, q, value: b.x - a.x },
        Linear::Vertical => Constraint::VDistance { p, q, value: b.y - a.y },
    })
}

fn line_points(sk: &Sketch, e: EntityId) -> Option<(PointId, PointId)> {
    match sk.entities.get(e)?.geom {
        Geom::Line { a, b } => Some((a, b)),
        _ => None,
    }
}

fn center_of(sk: &Sketch, e: EntityId) -> Option<PointId> {
    match sk.entities.get(e)?.geom {
        Geom::Circle { c, .. } | Geom::Arc { c, .. } => Some(c),
        Geom::Line { .. } => None,
    }
}

fn is_axis(sk: &Sketch, e: EntityId) -> bool {
    sk.entities.get(e).is_some_and(|x| x.construction && x.is_line())
}

/// Which side of the line through `axis` a position lies on: +1, -1, or 0 on the line.
fn side(sk: &Sketch, axis: EntityId, at: Vec2) -> f64 {
    let Some((a, b)) = line_points(sk, axis) else { return 0.0 };
    let (pa, pb) = (sk.pos(a), sk.pos(b));
    let s = (pb - pa).cross(at - pa);
    if s.abs() < 1e-12 {
        0.0
    } else {
        s.signum()
    }
}

/// From a point to a line: a diameter when the line is a centerline and the value is
/// placed on its far side, otherwise the perpendicular distance.
fn point_to_line(sk: &Sketch, p: PointId, line: EntityId, at: Vec2) -> Option<Constraint> {
    let value = sk.measure(&Constraint::PointLineDistance { p, line, value: 0.0 })?;
    if value < 1e-12 {
        return None;
    }
    let s = side(sk, line, sk.pos(p));
    if is_axis(sk, line) && s != 0.0 && side(sk, line, at) == -s {
        return Some(Constraint::AxisDiameter { p, axis: line, value: 2.0 * value });
    }
    Some(Constraint::PointLineDistance { p, line, value })
}

/// The angle between two crossing lines, in the sector of the four that `at` lies in.
fn line_angle(sk: &Sketch, a: EntityId, b: EntityId, at: Vec2) -> Option<Constraint> {
    let (a0, a1) = line_points(sk, a)?;
    let (b0, b1) = line_points(sk, b)?;
    let (pa, pb) = (sk.pos(a0), sk.pos(b0));
    let (u, w) = (sk.pos(a1) - pa, sk.pos(b1) - pb);
    let den = u.cross(w);
    if den.abs() <= PARALLEL_TOL * u.length() * w.length() {
        return None;
    }
    // Where the two (infinite) lines cross.
    let x = pa + u * ((pb - pa).cross(w) / den);
    let mut d = at - x;
    if d.length() < 1e-12 {
        d = (u.normalized() + w.normalized()) * 0.5;
    }
    // The sector containing `d` is bounded by one ray of each line; measure from the
    // ray that `d` is counter-clockwise of, so the value is the sector's angle.
    for (fa, fb) in [(false, false), (true, false), (false, true), (true, true)] {
        let ru = if fa { -u } else { u };
        let rw = if fb { -w } else { w };
        for (first, second, ra, rb, flip_a, flip_b) in [(a, b, ru, rw, fa, fb), (b, a, rw, ru, fb, fa)] {
            let sector = ccw_angle(ra, rb);
            if sector > 0.0 && sector < PI && ccw_angle(ra, d) <= sector {
                return Some(Constraint::RayAngle {
                    a: first,
                    b: second,
                    flip_a,
                    flip_b,
                    degrees: sector.to_degrees(),
                });
            }
        }
    }
    None
}

/// Works out the dimension for `picks` with its value placed at `at`, valued at what the
/// geometry measures now. Returns `None` when the picks do not make a dimension.
pub fn infer(sk: &Sketch, picks: &[Pick], at: Vec2) -> Option<Constraint> {
    use Pick::{Entity as E, Point as P};
    let valid = |pk: &Pick| match *pk {
        P(p) => p < sk.points.len(),
        E(e) => sk.entities.get(e).is_some_and(|x| x.points().iter().all(|&p| p < sk.points.len())),
    };
    if !picks.iter().all(valid) {
        return None;
    }
    let geom = |e: EntityId| sk.entities.get(e).map(|x| x.geom);
    match *picks {
        [E(e)] => match geom(e)? {
            // A line's own length when placed beside it, or when it is already horizontal
            // or vertical; otherwise its horizontal or vertical extent.
            Geom::Line { a, b } => match point_distance(sk, a, b, at)? {
                c @ (Constraint::HDistance { .. } | Constraint::VDistance { .. })
                    if !axis_aligned(sk.pos(a), sk.pos(b)) =>
                {
                    Some(c)
                }
                _ => Some(Constraint::Length { line: e, value: sk.line_length(e) }),
            },
            Geom::Circle { r, .. } => Some(Constraint::Diameter { entity: e, value: 2.0 * r }),
            Geom::Arc { .. } => Some(Constraint::Radius { entity: e, value: sk.radius(e) }),
        },
        [P(p), P(q)] if p != q => point_distance(sk, p, q, at),
        [P(p), E(e)] | [E(e), P(p)] => match geom(e)? {
            Geom::Line { .. } => point_to_line(sk, p, e, at),
            Geom::Circle { .. } | Geom::Arc { .. } => point_distance(sk, center_of(sk, e)?, p, at),
        },
        [E(e), E(f)] if e != f => match (geom(e)?, geom(f)?) {
            (Geom::Line { .. }, Geom::Line { .. }) => {
                if let Some(c) = line_angle(sk, e, f, at) {
                    return Some(c);
                }
                // Parallel lines: the distance between them, from the end of the first
                // nearest the placement, or a diameter when the second is a centerline.
                let (a, b) = line_points(sk, e)?;
                let p = if sk.pos(a).distance(at) <= sk.pos(b).distance(at) { a } else { b };
                point_to_line(sk, p, f, at)
            }
            (Geom::Line { .. }, _) => point_to_line(sk, center_of(sk, f)?, e, at),
            (_, Geom::Line { .. }) => point_to_line(sk, center_of(sk, e)?, f, at),
            _ => point_distance(sk, center_of(sk, e)?, center_of(sk, f)?, at),
        },
        // An arc and both its end points: the length along the arc.
        [E(e), P(p), P(q)] | [P(p), E(e), P(q)] | [P(p), P(q), E(e)] => match geom(e)? {
            Geom::Arc { a, b, .. } if (p == a && q == b) || (p == b && q == a) => {
                let value = sk.measure(&Constraint::ArcLength { entity: e, value: 0.0 })?;
                Some(Constraint::ArcLength { entity: e, value })
            }
            _ => None,
        },
        _ => None,
    }
}

/// True if adding dimension `ci` as a driving dimension over-defines the sketch: either
/// it conflicts with the other constraints, or it is redundant and removes no degree of
/// freedom. Such a dimension is better made a reference dimension.
pub fn over_defines(sk: &Sketch, ci: usize) -> bool {
    if ci >= sk.constraints.len() {
        return false;
    }
    let mut without = sk.clone();
    without.annotation_mut(ci).reference = true;
    let before = without.solve();
    let mut with = sk.clone();
    with.annotation_mut(ci).reference = false;
    let after = with.solve();
    if !before.converged {
        // Already over defined; only a dimension that adds a new conflict counts.
        return after.unsatisfied.contains(&ci);
    }
    !after.converged || after.dof >= before.dof
}

/// Where a dimension's value is drawn by default, in sketch coordinates. A placed
/// dimension adds its [`crate::DimAnnotation::offset`] to this.
pub fn anchor(sk: &Sketch, c: &Constraint) -> Option<Vec2> {
    use Constraint::*;
    let pos = |p: PointId| sk.points.get(p).map(|q| q.pos());
    let mid = |e: EntityId| {
        let (a, b) = line_points(sk, e)?;
        Some((pos(a)? + pos(b)?) * 0.5)
    };
    let center = |e: EntityId| pos(center_of(sk, e)?);
    let rim = |e: EntityId| Some(center(e)? + Vec2::from_angle(FRAC_PI_4) * sk.radius(e));
    let foot = |p: PointId, e: EntityId| {
        let (a, b) = line_points(sk, e)?;
        let (pa, pb, q) = (pos(a)?, pos(b)?, pos(p)?);
        let d = pb - pa;
        let t = if d.dot(d) < 1e-24 { 0.0 } else { (q - pa).dot(d) / d.dot(d) };
        Some(pa + d * t)
    };
    match *c {
        Length { line, .. } => mid(line),
        Radius { entity, .. } | Diameter { entity, .. } => rim(entity),
        ArcLength { entity, .. } => match sk.entities.get(entity)?.geom {
            Geom::Arc { c, a, b } => {
                let (cc, pa, pb) = (pos(c)?, pos(a)?, pos(b)?);
                let a0 = (pa - cc).angle();
                let sweep = crate::sketch::arc_sweep(a0, (pb - cc).angle());
                Some(cc + Vec2::from_angle(a0 + sweep * 0.5) * pa.distance(cc))
            }
            _ => None,
        },
        Distance { p, q, .. } => Some((pos(p)? + pos(q)?) * 0.5),
        // Horizontal distances sit level with the second point, vertical ones in line
        // with it, so a pair of them on the same points never overlap.
        HDistance { p, q, .. } => Some(v2((pos(p)?.x + pos(q)?.x) * 0.5, pos(q)?.y)),
        VDistance { p, q, .. } => Some(v2(pos(q)?.x, (pos(p)?.y + pos(q)?.y) * 0.5)),
        PointLineDistance { p, line, .. } => Some((pos(p)? + foot(p, line)?) * 0.5),
        AxisDiameter { p, axis, .. } => foot(p, axis),
        Angle { a, b, .. } => Some((mid(a)? + mid(b)?) * 0.5),
        RayAngle { a, b, flip_a, flip_b, .. } => {
            let (vertex, ra, rb) = rays(sk, a, b, flip_a, flip_b)?;
            let r = 0.5 * mid(a)?.distance(vertex).max(mid(b)?.distance(vertex));
            let a0 = ra.angle();
            Some(vertex + Vec2::from_angle(a0 + ccw_angle(ra, rb) * 0.5) * r)
        }
        // Between the offset curve and its base: square across from the offset line's
        // middle, or between the two rims of concentric circles.
        Offset { base, entity, .. } => match (line_points(sk, base), line_points(sk, entity)) {
            (Some(_), Some(_)) => {
                let m = mid(entity)?;
                let (a, b) = line_points(sk, base)?;
                let (pa, pb) = (pos(a)?, pos(b)?);
                let d = (pb - pa).normalized();
                let f = pa + d * (m - pa).dot(d);
                Some((m + f) * 0.5)
            }
            _ => Some(
                center(base)? + Vec2::from_angle(FRAC_PI_4) * 0.5 * (sk.radius(base) + sk.radius(entity)),
            ),
        },
        _ => None,
    }
}

/// Where the value of dimension `ci` is drawn: its anchor plus any placed offset.
pub fn label_position(sk: &Sketch, ci: usize) -> Option<Vec2> {
    let at = anchor(sk, sk.constraints.get(ci)?)?;
    Some(at + sk.annotation(ci).map_or(Vec2::ZERO, |a| a.offset))
}

/// The crossing point of two lines and the two chosen rays from it.
fn rays(sk: &Sketch, a: EntityId, b: EntityId, flip_a: bool, flip_b: bool) -> Option<(Vec2, Vec2, Vec2)> {
    let (a0, a1) = line_points(sk, a)?;
    let (b0, b1) = line_points(sk, b)?;
    let (pa, pb) = (sk.pos(a0), sk.pos(b0));
    let (u, w) = (sk.pos(a1) - pa, sk.pos(b1) - pb);
    let den = u.cross(w);
    if den.abs() < 1e-12 {
        return None;
    }
    let x = pa + u * ((pb - pa).cross(w) / den);
    Some((x, if flip_a { -u } else { u }, if flip_b { -w } else { w }))
}

/// Lines and arrows that draw one dimension, in sketch coordinates.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Graphic {
    /// Extension lines and dimension lines.
    pub lines: Vec<(Vec2, Vec2)>,
    /// Dimension arcs, for angles and arc lengths: center, radius, start angle and
    /// counter-clockwise sweep (radians).
    pub arcs: Vec<(Vec2, f64, f64, f64)>,
    /// Arrowheads: the tip and the direction the arrow points.
    pub arrows: Vec<(Vec2, Vec2)>,
}

impl Graphic {
    fn dim_line(&mut self, a: Vec2, b: Vec2) {
        self.lines.push((a, b));
        let d = (b - a).normalized();
        if d.length() > 0.5 {
            self.arrows.push((a, -d));
            self.arrows.push((b, d));
        }
    }
}

/// The drafting geometry for dimension `c` with its value drawn at `label`: extension
/// lines from the measured geometry out to a dimension line through the label, with an
/// arrow at each end.
pub fn graphic(sk: &Sketch, c: &Constraint, label: Vec2) -> Graphic {
    graphic_inner(sk, c, label).unwrap_or_default()
}

fn graphic_inner(sk: &Sketch, c: &Constraint, label: Vec2) -> Option<Graphic> {
    use Constraint::*;
    let pos = |p: PointId| sk.points.get(p).map(|q| q.pos());
    let mut g = Graphic::default();
    // Draws a linear dimension between `p` and `q` measured along `dir` (unit).
    let linear = |g: &mut Graphic, p: Vec2, q: Vec2, dir: Vec2| {
        let n = dir.perp();
        let off = (label - p).dot(n);
        let (ep, eq) = (p + n * off, q + n * (off + (p - q).dot(n)));
        // Extension lines, from the geometry out to the dimension line.
        if p.distance(ep) > 1e-9 {
            g.lines.push((p, ep));
        }
        if q.distance(eq) > 1e-9 {
            g.lines.push((q, eq));
        }
        g.dim_line(ep, eq);
    };
    match *c {
        Distance { p, q, .. } => {
            let (a, b) = (pos(p)?, pos(q)?);
            linear(&mut g, a, b, (b - a).normalized());
        }
        Length { line, .. } => {
            let (p, q) = line_points(sk, line)?;
            let (a, b) = (pos(p)?, pos(q)?);
            linear(&mut g, a, b, (b - a).normalized());
        }
        HDistance { p, q, .. } => linear(&mut g, pos(p)?, pos(q)?, v2(1.0, 0.0)),
        VDistance { p, q, .. } => linear(&mut g, pos(p)?, pos(q)?, v2(0.0, 1.0)),
        PointLineDistance { p, line, .. } | AxisDiameter { p, axis: line, .. } => {
            let (a, b) = line_points(sk, line)?;
            let (pa, pb, q) = (pos(a)?, pos(b)?, pos(p)?);
            let d = (pb - pa).normalized();
            let foot = pa + d * (q - pa).dot(d);
            // The far end mirrors the point across the axis for a diameter.
            let far = if matches!(c, AxisDiameter { .. }) { foot * 2.0 - q } else { foot };
            let n = (far - q).normalized();
            if n.length() < 0.5 {
                return None;
            }
            linear(&mut g, q, far, n);
        }
        Radius { entity, .. } | Diameter { entity, .. } => {
            let cc = pos(center_of(sk, entity)?)?;
            let r = sk.radius(entity);
            let mut d = (label - cc).normalized();
            if d.length() < 0.5 {
                d = Vec2::from_angle(FRAC_PI_4);
            }
            let rim = cc + d * r;
            let start = if matches!(c, Diameter { .. }) { cc - d * r } else { cc };
            g.lines.push((start, label));
            g.arrows.push((rim, d));
            if matches!(c, Diameter { .. }) {
                g.arrows.push((start, -d));
            }
        }
        ArcLength { entity, .. } => {
            let Geom::Arc { c: ci, a, b } = sk.entities.get(entity)?.geom else { return None };
            let (cc, pa, pb) = (pos(ci)?, pos(a)?, pos(b)?);
            let a0 = (pa - cc).angle();
            let sweep = crate::sketch::arc_sweep(a0, (pb - cc).angle());
            let r = label.distance(cc);
            let (ea, eb) = (cc + Vec2::from_angle(a0) * r, cc + Vec2::from_angle(a0 + sweep) * r);
            g.lines.push((pa, ea));
            g.lines.push((pb, eb));
            g.arcs.push((cc, r, a0, sweep));
            g.arrows.push((ea, -Vec2::from_angle(a0).perp()));
            g.arrows.push((eb, Vec2::from_angle(a0 + sweep).perp()));
        }
        Offset { base, entity, .. } => match (line_points(sk, base), line_points(sk, entity)) {
            (Some((a, b)), Some((p, q))) => {
                let (pa, pb) = (pos(a)?, pos(b)?);
                let m = (pos(p)? + pos(q)?) * 0.5;
                let d = (pb - pa).normalized();
                let f = pa + d * (m - pa).dot(d);
                let n = (m - f).normalized();
                if n.length() < 0.5 {
                    return None;
                }
                linear(&mut g, f, m, n);
            }
            _ => {
                let cc = pos(center_of(sk, base)?)?;
                let mut d = (label - cc).normalized();
                if d.length() < 0.5 {
                    d = Vec2::from_angle(FRAC_PI_4);
                }
                g.dim_line(cc + d * sk.radius(base), cc + d * sk.radius(entity));
            }
        },
        RayAngle { a, b, flip_a, flip_b, .. } => {
            let (x, ra, rb) = rays(sk, a, b, flip_a, flip_b)?;
            let r = label.distance(x).max(1e-9);
            let (a0, sweep) = (ra.angle(), ccw_angle(ra, rb));
            let (ea, eb) = (x + ra.normalized() * r, x + rb.normalized() * r);
            // Extension lines run along each ray from its nearer line end, when the arc
            // lies beyond the drawn line.
            for (line, end) in [(a, ea), (b, eb)] {
                let (p, q) = line_points(sk, line)?;
                let (pp, pq) = (pos(p)?, pos(q)?);
                let near = if pp.distance(end) <= pq.distance(end) { pp } else { pq };
                if near.distance(x) < end.distance(x) {
                    g.lines.push((near, end));
                }
            }
            g.arcs.push((x, r, a0, sweep));
            g.arrows.push((ea, -Vec2::from_angle(a0).perp()));
            g.arrows.push((eb, Vec2::from_angle(a0 + sweep).perp()));
        }
        Angle { .. } => {}
        _ => return None,
    }
    Some(g)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ORIGIN;
    use Pick::{Entity as E, Point as P};

    fn near(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    /// A line from (0,0) to (30,40), length 50.
    fn slanted() -> (Sketch, EntityId, PointId, PointId) {
        let mut s = Sketch::new();
        let a = s.add_point(v2(0.0, 0.0));
        let b = s.add_point(v2(30.0, 40.0));
        let l = s.add_line(a, b);
        (s, l, a, b)
    }

    #[test]
    fn line_placement_picks_aligned_horizontal_or_vertical() {
        let (s, l, a, b) = slanted();
        // Beside the line, off its diagonal: aligned length.
        assert_eq!(infer(&s, &[E(l)], v2(5.0, 30.0)), Some(Constraint::Length { line: l, value: 50.0 }));
        // Below the line's span: horizontal.
        assert_eq!(
            infer(&s, &[E(l)], v2(15.0, -8.0)),
            Some(Constraint::HDistance { p: a, q: b, value: 30.0 })
        );
        // Right of it: vertical.
        assert_eq!(
            infer(&s, &[E(l)], v2(45.0, 20.0)),
            Some(Constraint::VDistance { p: a, q: b, value: 40.0 })
        );
        // Outside both spans, off a corner: aligned.
        assert_eq!(infer(&s, &[E(l)], v2(50.0, 60.0)), Some(Constraint::Length { line: l, value: 50.0 }));
    }

    #[test]
    fn horizontal_line_only_has_its_length() {
        let mut s = Sketch::new();
        let a = s.add_point(v2(-5.0, 2.0));
        let b = s.add_point(v2(15.0, 2.0));
        let l = s.add_line(a, b);
        for at in [v2(5.0, 10.0), v2(30.0, 2.0), v2(30.0, 30.0)] {
            assert_eq!(infer(&s, &[E(l)], at), Some(Constraint::Length { line: l, value: 20.0 }));
        }
        // Its end points, picked as points, give the horizontal distance instead.
        assert_eq!(
            infer(&s, &[P(a), P(b)], v2(5.0, 10.0)),
            Some(Constraint::HDistance { p: a, q: b, value: 20.0 })
        );
    }

    #[test]
    fn two_points_follow_placement() {
        let mut s = Sketch::new();
        let p = s.add_point(v2(10.0, 5.0));
        assert_eq!(
            infer(&s, &[P(ORIGIN), P(p)], v2(5.0, -3.0)),
            Some(Constraint::HDistance { p: ORIGIN, q: p, value: 10.0 })
        );
        assert_eq!(
            infer(&s, &[P(ORIGIN), P(p)], v2(-4.0, 2.0)),
            Some(Constraint::VDistance { p: ORIGIN, q: p, value: 5.0 })
        );
        // Inside both spans, near the points' diagonal: aligned.
        let Some(Constraint::Distance { value, .. }) = infer(&s, &[P(ORIGIN), P(p)], v2(3.0, 4.0)) else {
            panic!("expected an aligned distance")
        };
        assert!(near(value, 125f64.sqrt()));
        assert_eq!(infer(&s, &[P(p), P(p)], v2(0.0, 0.0)), None);
        assert_eq!(infer(&s, &[P(p)], v2(0.0, 0.0)), None);
        // Picks that are not in the sketch make no dimension rather than a panic.
        assert_eq!(infer(&s, &[P(p), P(99)], v2(0.0, 0.0)), None);
        assert_eq!(infer(&s, &[E(7)], v2(0.0, 0.0)), None);
    }

    #[test]
    fn point_to_line_is_perpendicular() {
        let (mut s, l, _, _) = slanted();
        let p = s.add_point(v2(40.0, 0.0));
        // Distance from (40,0) to the line 4x - 3y = 0 is 160/5 = 32.
        assert_eq!(
            infer(&s, &[P(p), E(l)], v2(30.0, 10.0)),
            Some(Constraint::PointLineDistance { p, line: l, value: 32.0 })
        );
        assert_eq!(
            infer(&s, &[E(l), P(p)], v2(30.0, 10.0)),
            Some(Constraint::PointLineDistance { p, line: l, value: 32.0 })
        );
    }

    #[test]
    fn centerline_gives_radius_near_and_diameter_far() {
        let mut s = Sketch::new();
        let a = s.add_point(v2(0.0, -10.0));
        let b = s.add_point(v2(0.0, 10.0));
        let axis = s.add_line(a, b);
        s.entities[axis].construction = true;
        let p = s.add_point(v2(12.0, 3.0));
        assert_eq!(
            infer(&s, &[P(p), E(axis)], v2(6.0, 6.0)),
            Some(Constraint::PointLineDistance { p, line: axis, value: 12.0 })
        );
        assert_eq!(
            infer(&s, &[P(p), E(axis)], v2(-6.0, 6.0)),
            Some(Constraint::AxisDiameter { p, axis, value: 24.0 })
        );
        // An ordinary line never gives a diameter.
        s.entities[axis].construction = false;
        assert_eq!(
            infer(&s, &[P(p), E(axis)], v2(-6.0, 6.0)),
            Some(Constraint::PointLineDistance { p, line: axis, value: 12.0 })
        );
    }

    #[test]
    fn circle_diameter_arc_radius_and_arc_length() {
        let mut s = Sketch::new();
        let c = s.add_point(v2(0.0, 0.0));
        let circle = s.add_circle(c, 7.5);
        assert_eq!(
            infer(&s, &[E(circle)], v2(20.0, 0.0)),
            Some(Constraint::Diameter { entity: circle, value: 15.0 })
        );
        let a = s.add_point(v2(10.0, 0.0));
        let b = s.add_point(v2(0.0, 10.0));
        let arc = s.add_arc(c, a, b);
        assert_eq!(infer(&s, &[E(arc)], v2(9.0, 9.0)), Some(Constraint::Radius { entity: arc, value: 10.0 }));
        let Some(Constraint::ArcLength { value, .. }) = infer(&s, &[E(arc), P(b), P(a)], v2(9.0, 9.0)) else {
            panic!("expected an arc length")
        };
        assert!(near(value, 5.0 * PI), "{value}");
        // Points that are not the arc's ends make no dimension.
        assert_eq!(infer(&s, &[E(arc), P(a), P(c)], v2(9.0, 9.0)), None);
    }

    #[test]
    fn rounds_dimension_to_their_centers() {
        let mut s = Sketch::new();
        let c1 = s.add_point(v2(0.0, 0.0));
        let c2 = s.add_point(v2(20.0, 0.0));
        let k1 = s.add_circle(c1, 2.0);
        let k2 = s.add_circle(c2, 3.0);
        assert_eq!(
            infer(&s, &[E(k1), E(k2)], v2(10.0, 8.0)),
            Some(Constraint::HDistance { p: c1, q: c2, value: 20.0 })
        );
        let p = s.add_point(v2(0.0, 9.0));
        assert_eq!(
            infer(&s, &[E(k1), P(p)], v2(-5.0, 4.0)),
            Some(Constraint::VDistance { p: c1, q: p, value: 9.0 })
        );
        let q = s.add_point(v2(-4.0, -6.0));
        let r = s.add_point(v2(36.0, -6.0));
        let l = s.add_line(q, r);
        assert_eq!(
            infer(&s, &[E(l), E(k2)], v2(20.0, -3.0)),
            Some(Constraint::PointLineDistance { p: c2, line: l, value: 6.0 })
        );
    }

    #[test]
    fn parallel_lines_give_their_distance() {
        let mut s = Sketch::new();
        let l1 = {
            let a = s.add_point(v2(0.0, 0.0));
            let b = s.add_point(v2(10.0, 0.0));
            s.add_line(a, b)
        };
        let l2 = {
            let a = s.add_point(v2(2.0, 4.0));
            let b = s.add_point(v2(-8.0, 4.0));
            s.add_line(a, b)
        };
        let Some(Constraint::PointLineDistance { p, line, value }) = infer(&s, &[E(l1), E(l2)], v2(9.0, 2.0))
        else {
            panic!("expected a distance")
        };
        assert_eq!((line, value), (l2, 4.0));
        assert!(near(s.pos(p).x, 10.0), "measured from the end nearest the placement");
    }

    #[test]
    fn angle_sector_follows_placement() {
        let mut s = Sketch::new();
        // Line a along +x, line b at 60 degrees, crossing at the origin.
        let a = {
            let p = s.add_point(v2(-10.0, 0.0));
            let q = s.add_point(v2(10.0, 0.0));
            s.add_line(p, q)
        };
        let b = {
            let p = s.add_point(v2(0.0, 0.0));
            let q = s.add_point(Vec2::from_angle(PI / 3.0) * 10.0);
            s.add_line(p, q)
        };
        let deg = |at: Vec2| match infer(&s, &[E(a), E(b)], at) {
            Some(Constraint::RayAngle { degrees, .. }) => degrees,
            other => panic!("expected an angle, got {other:?}"),
        };
        // Between +x and the 60 degree ray.
        assert!(near(deg(Vec2::from_angle(0.5) * 5.0), 60.0));
        // Between the 60 degree ray and -x.
        assert!(near(deg(Vec2::from_angle(2.0) * 5.0), 120.0));
        // Below the x axis: the opposite sectors.
        assert!(near(deg(Vec2::from_angle(-2.6) * 5.0), 60.0));
        assert!(near(deg(Vec2::from_angle(-1.0) * 5.0), 120.0));
        // Whichever sector, the constraint measures what the geometry shows.
        for t in [0.5, 2.0, -2.6, -1.0] {
            let c = infer(&s, &[E(a), E(b)], Vec2::from_angle(t) * 5.0).unwrap();
            assert!(near(s.measure(&c).unwrap(), c.value().unwrap()));
        }
    }

    #[test]
    fn angle_dimension_drives_the_sector_it_was_placed_in() {
        let mut s = Sketch::new();
        let o = s.add_point(v2(0.0, 0.0));
        let x = s.add_point(v2(10.0, 0.0));
        let y = s.add_point(Vec2::from_angle(1.0) * 10.0);
        let a = s.add_line(o, x);
        let b = s.add_line(o, y);
        s.add_constraint(Constraint::Coincident { p: o, q: ORIGIN });
        s.add_constraint(Constraint::Horizontal { line: a });
        let mut c = infer(&s, &[E(a), E(b)], v2(-3.0, 4.0)).unwrap();
        c.set_value(135.0);
        s.add_constraint(c);
        assert!(s.solve().converged);
        // The sector between line b and the -x ray is now 135 degrees, so b is at 45.
        let d = s.pos(y) - s.pos(o);
        assert!(near(d.y.atan2(d.x).to_degrees(), 45.0), "{d:?}");
    }

    #[test]
    fn arc_length_and_axis_diameter_drive_geometry() {
        let mut s = Sketch::new();
        let a = s.add_point(v2(10.0, 0.0));
        let b = s.add_point(v2(0.0, 10.0));
        let arc = s.add_arc(ORIGIN, a, b);
        s.add_constraint(Constraint::HorizontalPoints { p: ORIGIN, q: a });
        s.add_constraint(Constraint::VerticalPoints { p: ORIGIN, q: b });
        s.add_constraint(Constraint::ArcLength { entity: arc, value: 4.0 * PI });
        assert!(s.solve().converged);
        assert!(near(s.radius(arc), 8.0), "{}", s.radius(arc));

        let mut s = Sketch::new();
        let p = s.add_point(v2(0.0, 0.0));
        let q = s.add_point(v2(0.0, 10.0));
        let axis = s.add_line(p, q);
        let r = s.add_point(v2(4.0, 2.0));
        s.add_constraint(Constraint::AxisDiameter { p: r, axis, value: 30.0 });
        assert!(s.solve().converged);
        assert!(near(
            s.measure(&Constraint::PointLineDistance { p: r, line: axis, value: 0.0 }).unwrap(),
            15.0
        ));
    }

    #[test]
    fn redundant_and_conflicting_dimensions_over_define() {
        let mut s = Sketch::new();
        let l = s.add_rectangle(v2(0.0, 0.0), v2(40.0, 20.0));
        s.add_constraint(Constraint::Length { line: l[0], value: 40.0 });
        let fresh = s.add_constraint(Constraint::Length { line: l[1], value: 20.0 });
        assert!(!over_defines(&s, fresh));
        s.solve();
        // The top edge is already 40 long through the rectangle's relations.
        let again = s.add_constraint(Constraint::Length { line: l[2], value: 40.0 });
        assert!(over_defines(&s, again));
        s.constraints[again].set_value(55.0);
        assert!(over_defines(&s, again));
    }

    #[test]
    fn reference_dimensions_follow_the_geometry() {
        let mut s = Sketch::new();
        let l = s.add_rectangle(v2(0.0, 0.0), v2(40.0, 20.0));
        s.add_constraint(Constraint::Length { line: l[0], value: 40.0 });
        let r = s.add_constraint(Constraint::Length { line: l[2], value: 1.0 });
        s.set_reference(r, true);
        assert_eq!(s.constraints[r].value(), Some(40.0));
        // Changing the driving width moves the top edge, and the reference follows it.
        s.constraints[r - 1].set_value(64.0);
        let res = s.solve();
        assert!(res.converged, "{res:?}");
        assert!(near(s.constraints[r].value().unwrap(), 64.0));
    }

    #[test]
    fn annotations_survive_renumbering() {
        let mut s = Sketch::new();
        let l = s.add_rectangle(v2(0.0, 0.0), v2(40.0, 20.0));
        let c = s.add_point(v2(20.0, 10.0));
        let circle = s.add_circle(c, 3.0);
        let len = s.add_constraint(Constraint::Length { line: l[0], value: 40.0 });
        let dia = s.add_constraint(Constraint::Diameter { entity: circle, value: 6.0 });
        s.annotation_mut(dia).offset = v2(5.0, 5.0);
        s.set_reference(dia, true);
        let map = s.delete_entities(&[l[3]]);
        let (nlen, ndia) = (map[len].unwrap(), map[dia].unwrap());
        assert_eq!(s.constraints[nlen], Constraint::Length { line: 0, value: 40.0 });
        assert_eq!(s.constraints[ndia], Constraint::Diameter { entity: 3, value: 6.0 });
        assert!(s.is_reference(ndia));
        assert_eq!(s.annotation(ndia).map(|a| a.offset), Some(v2(5.0, 5.0)));
        s.remove_constraint(nlen);
        assert!(s.is_reference(ndia - 1));
        // Deleting the circle drops its annotation.
        s.delete_entities(&[3]);
        assert!(s.annotations.is_empty());
    }

    #[test]
    fn linear_graphic_has_extension_lines_to_the_label() {
        let (s, _, a, b) = slanted();
        let c = Constraint::HDistance { p: a, q: b, value: 30.0 };
        let g = graphic(&s, &c, v2(15.0, -10.0));
        // Extension lines drop to y = -10, and the dimension line runs from x = 0 to 30.
        assert!(g.lines.contains(&(v2(0.0, 0.0), v2(0.0, -10.0))));
        assert!(g.lines.contains(&(v2(30.0, 40.0), v2(30.0, -10.0))));
        assert!(g.lines.contains(&(v2(0.0, -10.0), v2(30.0, -10.0))));
        assert_eq!(g.arrows, vec![(v2(0.0, -10.0), v2(-1.0, 0.0)), (v2(30.0, -10.0), v2(1.0, 0.0))]);
    }
}
