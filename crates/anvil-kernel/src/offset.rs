//! Offsetting chains of lines and arcs in the plane, and the walls of thin features.
//!
//! A chain is a run of segments, each starting where the previous one ends. Its offset
//! moves every segment sideways by the same distance: lines move square to themselves
//! and arcs change radius. Where two segments meet smoothly the offsets meet too. At a
//! corner the offset segments are extended or cut back to where they cross, the way a
//! sharp corner offsets; if they do not cross (two arcs bending apart), a round joins
//! them about the corner.

use crate::math::{v2, Vec2};
use crate::profile::{Loop2, Seg2};
use std::f64::consts::TAU;

#[derive(Clone, Debug, PartialEq)]
pub enum OffsetError {
    /// The chain is empty or its segments do not join end to end.
    NotAChain,
    /// The offset is so large that a segment shrinks away or turns inside out.
    Collapsed,
}

impl std::fmt::Display for OffsetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OffsetError::NotAChain => write!(f, "the curves do not form one chain"),
            OffsetError::Collapsed => write!(f, "the offset is larger than the curves allow"),
        }
    }
}

impl std::error::Error for OffsetError {}

const EPS: f64 = 1e-9;

fn right(d: Vec2) -> Vec2 {
    v2(d.y, -d.x)
}

/// Unit direction of travel at the start or the end of a segment.
pub fn travel(s: &Seg2, at_end: bool) -> Vec2 {
    match *s {
        Seg2::Line { a, b, .. } => (b - a).normalized(),
        Seg2::Arc { start, sweep, .. } => {
            let t = if at_end { start + sweep } else { start };
            Vec2::from_angle(t).perp() * sweep.signum()
        }
    }
}

/// The supporting line (point, direction) or circle (centre, radius) of an offset segment.
#[derive(Clone, Copy)]
enum Support {
    Line(Vec2, Vec2),
    Circle(Vec2, f64),
}

fn support(s: &Seg2, d: f64) -> Result<Support, OffsetError> {
    match *s {
        Seg2::Line { a, b, .. } => {
            let u = (b - a).normalized();
            Ok(Support::Line(a + right(u) * d, u))
        }
        Seg2::Arc { center, radius, sweep, .. } => {
            let r = radius + d * sweep.signum();
            if r <= EPS {
                return Err(OffsetError::Collapsed);
            }
            Ok(Support::Circle(center, r))
        }
    }
}

fn crossings(a: Support, b: Support) -> Vec<Vec2> {
    match (a, b) {
        (Support::Line(p, u), Support::Line(q, w)) => {
            let den = u.cross(w);
            if den.abs() < EPS {
                return Vec::new();
            }
            vec![p + u * ((q - p).cross(w) / den)]
        }
        (Support::Line(p, u), Support::Circle(c, r)) | (Support::Circle(c, r), Support::Line(p, u)) => {
            let foot = p + u * (c - p).dot(u);
            let h2 = r * r - foot.distance(c).powi(2);
            if h2 < -EPS {
                Vec::new()
            } else {
                let h = h2.max(0.0).sqrt();
                vec![foot - u * h, foot + u * h]
            }
        }
        (Support::Circle(c1, r1), Support::Circle(c2, r2)) => {
            let dd = c1.distance(c2);
            if dd < EPS || dd > r1 + r2 + EPS || dd < (r1 - r2).abs() - EPS {
                return Vec::new();
            }
            let a = (r1 * r1 - r2 * r2 + dd * dd) / (2.0 * dd);
            let h = (r1 * r1 - a * a).max(0.0).sqrt();
            let u = (c2 - c1) / dd;
            let m = c1 + u * a;
            vec![m + u.perp() * h, m - u.perp() * h]
        }
    }
}

/// How two offset segments join at a corner of the original chain.
enum Join {
    /// Both pass through this point.
    Point(Vec2),
    /// The first ends at `.0`, a round about `.2` runs to `.1` where the second starts.
    Round(Vec2, Vec2, Vec2),
}

fn join(prev: &Seg2, next: &Seg2, d: f64) -> Result<Join, OffsetError> {
    let (u, w) = (travel(prev, true), travel(next, false));
    let p = next.start_point();
    if u.cross(w).abs() < 1e-9 && u.dot(w) > 0.0 {
        return Ok(Join::Point(p + right(w) * d));
    }
    let (a, b) = (p + right(u) * d, p + right(w) * d);
    let near = crossings(support(prev, d)?, support(next, d)?)
        .into_iter()
        .min_by(|x, y| x.distance(p).total_cmp(&y.distance(p)));
    match near {
        // A crossing far beyond the corner is the far side of a circle, not a corner.
        Some(x) if x.distance(p) <= 4.0 * d.abs() / (0.5 * (1.0 + u.dot(w))).sqrt().max(1e-3) => {
            Ok(Join::Point(x))
        }
        _ => Ok(Join::Round(a, b, p)),
    }
}

/// The piece of an offset segment's support from `a` to `b`, keeping the original's
/// direction. Fails if it would run backwards.
fn trimmed(s: &Seg2, d: f64, a: Vec2, b: Vec2) -> Result<Seg2, OffsetError> {
    match *s {
        Seg2::Line { a: oa, b: ob, source } => {
            if (b - a).dot(ob - oa) <= EPS * (ob - oa).length() {
                return Err(OffsetError::Collapsed);
            }
            Ok(Seg2::Line { a, b, source })
        }
        Seg2::Arc { center, radius, sweep, source, .. } => {
            let r = radius + d * sweep.signum();
            if r <= EPS {
                return Err(OffsetError::Collapsed);
            }
            let start = (a - center).angle();
            let end = (b - center).angle();
            let mut sw =
                if sweep > 0.0 { (end - start).rem_euclid(TAU) } else { -(start - end).rem_euclid(TAU) };
            if sw.abs() < 1e-12 {
                sw = if sweep.abs() > std::f64::consts::PI { TAU * sweep.signum() } else { 0.0 };
            }
            // An arc that comes out sweeping much more than it did has turned inside out.
            if sw == 0.0 || sw.abs() > sweep.abs() + std::f64::consts::PI {
                return Err(OffsetError::Collapsed);
            }
            Ok(Seg2::Arc { center, radius: r, start, sweep: sw, source })
        }
    }
}

fn retag(s: Seg2, add: u32) -> Seg2 {
    match s {
        Seg2::Line { a, b, source } => Seg2::Line { a, b, source: source.wrapping_add(add) },
        Seg2::Arc { center, radius, start, sweep, source } => {
            Seg2::Arc { center, radius, start, sweep, source: source.wrapping_add(add) }
        }
    }
}

fn round(a: Vec2, b: Vec2, c: Vec2, d: f64, source: u32) -> Seg2 {
    let start = (a - c).angle();
    let end = (b - c).angle();
    // The round turns the way the corner turns, seen from the offset side.
    let sweep = if d > 0.0 { -(start - end).rem_euclid(TAU) } else { (end - start).rem_euclid(TAU) };
    Seg2::Arc { center: c, radius: d.abs(), start, sweep, source }
}

/// The chain moved `d` to the right of its direction of travel (left when negative).
/// A closed chain's last segment ends where its first starts.
pub fn offset_chain(segs: &[Seg2], closed: bool, d: f64) -> Result<Vec<Seg2>, OffsetError> {
    let n = segs.len();
    if n == 0 {
        return Err(OffsetError::NotAChain);
    }
    for i in 0..n - 1 {
        if segs[i].end_point().distance(segs[i + 1].start_point()) > 1e-6 {
            return Err(OffsetError::NotAChain);
        }
    }
    if d == 0.0 {
        return Ok(segs.to_vec());
    }
    if closed && n == 1 {
        if let Seg2::Arc { center, radius, start, sweep, source } = segs[0] {
            let r = radius + d * sweep.signum();
            if r <= EPS {
                return Err(OffsetError::Collapsed);
            }
            return Ok(vec![Seg2::Arc { center, radius: r, start, sweep, source }]);
        }
        return Err(OffsetError::NotAChain);
    }
    // joins[i] is the corner where segment i starts.
    let mut joins: Vec<Option<Join>> = Vec::with_capacity(n);
    for i in 0..n {
        if i == 0 && !closed {
            joins.push(None);
        } else {
            joins.push(Some(join(&segs[(i + n - 1) % n], &segs[i], d)?));
        }
    }
    let mut out = Vec::new();
    for i in 0..n {
        let start = match &joins[i] {
            None => segs[i].start_point() + right(travel(&segs[i], false)) * d,
            Some(Join::Point(p)) => *p,
            Some(Join::Round(_, b, _)) => *b,
        };
        let end = match joins.get((i + 1) % n).filter(|_| closed || i + 1 < n) {
            Some(Some(Join::Point(p))) => *p,
            Some(Some(Join::Round(a, _, _))) => *a,
            _ => segs[i].end_point() + right(travel(&segs[i], true)) * d,
        };
        if let Some(Join::Round(a, b, c)) = &joins[i] {
            out.push(round(*a, *b, *c, d, segs[i].source()));
        }
        out.push(trimmed(&segs[i], d, start, end)?);
    }
    Ok(out)
}

/// The wall of a thin feature: the chain thickened `left` to its left and `right` to
/// its right (either may be zero, not both). An open chain gives one closed loop, capped
/// square across its ends; a closed chain gives two loops, the region between them.
/// Segments on the right keep their sources; those on the left have `left_tag` added,
/// and the caps across the ends are `cap_source` and `cap_source + 1`, so every face of
/// the wall can be named apart.
pub fn thin_wall(
    segs: &[Seg2],
    closed: bool,
    left: f64,
    right_d: f64,
    cap_source: u32,
    left_tag: u32,
) -> Result<Vec<Loop2>, OffsetError> {
    if left < 0.0 || right_d < 0.0 || left + right_d <= EPS {
        return Err(OffsetError::Collapsed);
    }
    let r = offset_chain(segs, closed, right_d)?;
    let l: Vec<Seg2> = offset_chain(segs, closed, -left)?.into_iter().map(|s| retag(s, left_tag)).collect();
    if closed {
        return Ok(vec![Loop2::new(r), Loop2::new(l)]);
    }
    // Right side forwards, across the end, left side backwards, across the start.
    let mut loop_segs = r.clone();
    let (re, le) = (r.last().map(|s| s.end_point()), l.last().map(|s| s.end_point()));
    let (rs, ls) = (r.first().map(|s| s.start_point()), l.first().map(|s| s.start_point()));
    let (Some(re), Some(le), Some(rs), Some(ls)) = (re, le, rs, ls) else {
        return Err(OffsetError::NotAChain);
    };
    loop_segs.push(Seg2::Line { a: re, b: le, source: cap_source });
    loop_segs.extend(l.iter().rev().map(|s| s.reversed()));
    loop_segs.push(Seg2::Line { a: ls, b: rs, source: cap_source + 1 });
    loop_segs.retain(|s| match *s {
        Seg2::Line { a, b, .. } => a.distance(b) > 1e-9,
        _ => true,
    });
    Ok(vec![Loop2::new(loop_segs)])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::Profile;
    use std::f64::consts::PI;

    fn line(a: (f64, f64), b: (f64, f64), s: u32) -> Seg2 {
        Seg2::Line { a: v2(a.0, a.1), b: v2(b.0, b.1), source: s }
    }

    fn area(loops: Vec<Loop2>) -> f64 {
        let mut it = loops.into_iter();
        let outer = it.next().unwrap();
        Profile::new(outer, it.collect()).area()
    }

    #[test]
    fn square_offsets_with_sharp_corners() {
        let sq = Loop2::rectangle(v2(0.0, 0.0), v2(10.0, 10.0), 0);
        // Counter-clockwise, so right of travel is outside.
        let out = offset_chain(&sq.segs, true, 2.0).unwrap();
        assert!((Loop2::new(out).signed_area() - 196.0).abs() < 1e-9);
        let inn = offset_chain(&sq.segs, true, -2.0).unwrap();
        assert!((Loop2::new(inn).signed_area() - 36.0).abs() < 1e-9);
        assert_eq!(offset_chain(&sq.segs, true, -6.0), Err(OffsetError::Collapsed));
    }

    #[test]
    fn slot_offsets_smoothly() {
        let slot = vec![
            line((0.0, -5.0), (20.0, -5.0), 0),
            Seg2::Arc { center: v2(20.0, 0.0), radius: 5.0, start: -0.5 * PI, sweep: PI, source: 1 },
            line((20.0, 5.0), (0.0, 5.0), 2),
            Seg2::Arc { center: v2(0.0, 0.0), radius: 5.0, start: 0.5 * PI, sweep: PI, source: 3 },
        ];
        let out = Loop2::new(offset_chain(&slot, true, 1.0).unwrap());
        assert!(out.is_closed(1e-9));
        assert!((out.signed_area() - (20.0 * 12.0 + PI * 36.0)).abs() < 1e-9);
    }

    #[test]
    fn thin_wall_of_an_open_l() {
        // An L of legs 20 and 10, thickened 2 to the left (inside the turn).
        let l = vec![line((0.0, 0.0), (20.0, 0.0), 0), line((20.0, 0.0), (20.0, 10.0), 1)];
        let w = thin_wall(&l, false, 2.0, 0.0, 10, 100).unwrap();
        assert_eq!(w.len(), 1);
        assert!(w[0].is_closed(1e-9));
        // Area of an L wall: 2 * (20 + 10 - 2).
        assert!((w[0].signed_area().abs() - 56.0).abs() < 1e-9, "{}", w[0].signed_area());
        // Mid-plane, 1 each side: 2 * (20 + 10) exactly, the corner adds and removes 1.
        let w = thin_wall(&l, false, 1.0, 1.0, 10, 100).unwrap();
        assert!((w[0].signed_area().abs() - 60.0).abs() < 1e-9, "{}", w[0].signed_area());
    }

    #[test]
    fn thin_wall_of_a_circle_is_a_ring() {
        let c = Loop2::circle(v2(0.0, 0.0), 10.0, 0);
        let w = thin_wall(&c.segs, true, 0.0, 2.0, 5, 100).unwrap();
        assert!((area(w) - PI * (144.0 - 100.0)).abs() < 1e-9);
    }

    #[test]
    fn arcs_bending_apart_get_a_round() {
        // Down one arc into a cusp at the origin and up another: offset to the right
        // both circles shrink to radius 9, 20 apart, so they never meet and a round
        // about the cusp joins them.
        let a =
            Seg2::Arc { center: v2(-10.0, 0.0), radius: 10.0, start: 0.5 * PI, sweep: -0.5 * PI, source: 0 };
        let b = Seg2::Arc { center: v2(10.0, 0.0), radius: 10.0, start: PI, sweep: -0.5 * PI, source: 1 };
        let out = offset_chain(&[a, b], false, 1.0).unwrap();
        assert_eq!(out.len(), 3);
        for w in out.windows(2) {
            assert!(w[0].end_point().distance(w[1].start_point()) < 1e-9);
        }
        assert!(matches!(out[1], Seg2::Arc { radius, .. } if (radius - 1.0).abs() < 1e-12));
    }
}
