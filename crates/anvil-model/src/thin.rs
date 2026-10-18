//! Thin features: walls of a set thickness along sketch curves.
//!
//! The behaviour follows `docs/re/extrude-revolve.md` ("Extrude: Thin Feature" and
//! "Revolve: Thin Feature"). A sketch's curves are gathered into chains (open runs and
//! closed loops). Each chain is thickened to a wall:
//!
//! - One-Direction puts the whole thickness outside a closed loop, or to the left of an
//!   open chain walked from its first end; Reverse takes the other side.
//! - Mid-Plane puts half on each side.
//! - Two-Direction puts the first thickness on the One-Direction side and the second on
//!   the other.
//!
//! Corners stay sharp (mitred), unless an open chain asks for rounds: then each corner
//! between two lines is rounded so the wall's inside face has the given radius and the
//! outside face that radius plus the thickness.

use crate::document::{ThinKind, ThinWall};
use anvil_kernel::offset::{thin_wall, travel};
use anvil_kernel::{Loop2, Profile, Seg2};
use anvil_sketch::{Constraint, Geom, Sketch};
use std::collections::HashMap;

/// Added to a segment's source for the wall face on the far side of the sketch curve.
pub const LEFT_TAG: u32 = 1 << 20;
/// Sources of the faces across the ends of open walls (two per chain).
pub const CAP_TAG: u32 = 1 << 21;
/// Sources of the rounds an auto-fillet adds at corners.
pub const ROUND_TAG: u32 = 1 << 22;

/// A run of sketch curves, each starting where the last ended.
#[derive(Clone, Debug, PartialEq)]
pub struct Chain {
    pub segs: Vec<Seg2>,
    pub closed: bool,
}

/// Every chain of the sketch's non-construction curves. An open chain runs from the
/// loose end of its lowest-numbered curve; a closed one runs counter-clockwise. Errors
/// if three or more curves meet at a point.
pub fn chains(sk: &Sketch) -> Result<Vec<Chain>, String> {
    // Group points that coincide, by relation or by position.
    let n = sk.points.len();
    let mut root: Vec<usize> = (0..n).collect();
    fn find(r: &mut [usize], mut i: usize) -> usize {
        while r[i] != i {
            r[i] = r[r[i]];
            i = r[i];
        }
        i
    }
    for c in &sk.constraints {
        if let Constraint::Coincident { p, q } = *c {
            if p < n && q < n {
                let (a, b) = (find(&mut root, p), find(&mut root, q));
                root[a.max(b)] = a.min(b);
            }
        }
    }
    for i in 0..n {
        for j in i + 1..n {
            if sk.pos(i).distance(sk.pos(j)) < 1e-6 {
                let (a, b) = (find(&mut root, i), find(&mut root, j));
                root[a.max(b)] = a.min(b);
            }
        }
    }
    let r: Vec<usize> = (0..n).map(|i| find(&mut root, i)).collect();

    let mut out = Vec::new();
    // Curves as (entity, start node, end node).
    let mut edges: Vec<(usize, usize, usize)> = Vec::new();
    for (ei, e) in sk.entities.iter().enumerate() {
        if e.construction {
            continue;
        }
        match e.geom {
            Geom::Circle { c, r: rad } if rad > 1e-9 => out.push(Chain {
                segs: vec![Seg2::Arc {
                    center: sk.pos(c),
                    radius: rad,
                    start: 0.0,
                    sweep: std::f64::consts::TAU,
                    source: ei as u32,
                }],
                closed: true,
            }),
            Geom::Circle { .. } => {}
            Geom::Line { a, b } | Geom::Arc { a, b, .. } => edges.push((ei, r[a], r[b])),
        }
    }
    let mut at: HashMap<usize, Vec<usize>> = HashMap::new();
    for (k, &(_, s, t)) in edges.iter().enumerate() {
        at.entry(s).or_default().push(k);
        if t != s {
            at.entry(t).or_default().push(k);
        }
    }
    if let Some((_, list)) = at.iter().find(|(_, l)| l.len() > 2) {
        let e = edges[list[0]].0;
        return Err(format!("three or more curves meet at a point of sketch curve {}", e + 1));
    }
    let mut used = vec![false; edges.len()];
    // Open chains first, each from the loose end of its lowest curve.
    loop {
        let start = (0..edges.len())
            .filter(|&k| !used[k])
            .flat_map(|k| {
                let (_, s, t) = edges[k];
                [(k, s), (k, t)]
            })
            .find(|&(_, node)| at.get(&node).map_or(0, |l| l.len()) == 1);
        let Some((k0, node0)) = start else { break };
        out.push(walk(sk, &edges, &at, &mut used, k0, node0, false));
    }
    while let Some(k0) = (0..edges.len()).find(|&k| !used[k]) {
        let mut c = walk(sk, &edges, &at, &mut used, k0, edges[k0].1, true);
        if Loop2::new(c.segs.clone()).signed_area() < 0.0 {
            c.segs = Loop2::new(c.segs).reversed().segs;
        }
        out.push(c);
    }
    Ok(out)
}

fn walk(
    sk: &Sketch,
    edges: &[(usize, usize, usize)],
    at: &HashMap<usize, Vec<usize>>,
    used: &mut [bool],
    k0: usize,
    node0: usize,
    closed: bool,
) -> Chain {
    let mut segs = Vec::new();
    let (mut k, mut from) = (k0, node0);
    loop {
        used[k] = true;
        let (ei, s, t) = edges[k];
        let forward = s == from;
        segs.push(segment(sk, ei, forward));
        let to = if forward { t } else { s };
        match at.get(&to).and_then(|l| l.iter().copied().find(|&j| !used[j])) {
            Some(j) => {
                k = j;
                from = to;
            }
            None => break,
        }
    }
    Chain { segs, closed }
}

fn segment(sk: &Sketch, ei: usize, forward: bool) -> Seg2 {
    let seg = match sk.entities[ei].geom {
        Geom::Line { a, b } => Seg2::Line { a: sk.pos(a), b: sk.pos(b), source: ei as u32 },
        Geom::Arc { c, a, b } => {
            let (cc, pa, pb) = (sk.pos(c), sk.pos(a), sk.pos(b));
            let start = (pa - cc).angle();
            let sweep = anvil_sketch::sketch::arc_sweep(start, (pb - cc).angle());
            Seg2::Arc { center: cc, radius: pa.distance(cc), start, sweep, source: ei as u32 }
        }
        Geom::Circle { c, r } => Seg2::Arc {
            center: sk.pos(c),
            radius: r,
            start: 0.0,
            sweep: std::f64::consts::TAU,
            source: ei as u32,
        },
    };
    if forward {
        seg
    } else {
        seg.reversed()
    }
}

/// Thicknesses (left, right) of the wall for a chain.
fn sides(w: &ThinWall, closed: bool, t1: f64, t2: f64) -> (f64, f64) {
    // The One-Direction side: outside (right of a counter-clockwise loop), or left of
    // an open chain.
    let (first, other) = match w.kind {
        ThinKind::OneDirection => (t1, 0.0),
        ThinKind::MidPlane => (0.5 * t1, 0.5 * t1),
        ThinKind::TwoDirection => (t1, t2),
    };
    let (first, other) =
        if w.reverse && w.kind != ThinKind::MidPlane { (other, first) } else { (first, other) };
    if closed {
        (other, first)
    } else {
        (first, other)
    }
}

/// Rounds the corners between lines of an open chain. `radius_for(turns_left)` gives
/// the chain's own radius at a corner that turns left (true) or right.
fn round_corners(segs: &[Seg2], radius_for: impl Fn(bool) -> f64) -> Result<Vec<Seg2>, String> {
    let mut out: Vec<Seg2> = Vec::new();
    let mut cur = segs.first().copied().ok_or("an empty chain")?;
    for (k, next) in segs.iter().skip(1).enumerate() {
        let (Seg2::Line { a, b, source }, Seg2::Line { b: nb, source: ns, .. }) = (cur, *next) else {
            out.push(cur);
            cur = *next;
            continue;
        };
        let (u, w) = (travel(&cur, true), travel(next, false));
        let turn = u.cross(w);
        if turn.abs() < 1e-9 {
            out.push(cur);
            cur = *next;
            continue;
        }
        let rad = radius_for(turn > 0.0);
        let half = 0.5 * (-u.dot(w)).clamp(-1.0, 1.0).acos();
        let setback = rad / half.tan();
        if setback >= a.distance(b) - 1e-9 || setback >= b.distance(nb) - 1e-9 {
            return Err("the corner round does not fit on the sketch lines".into());
        }
        let p = b - u * setback;
        let q = b + w * setback;
        let normal = if turn > 0.0 { u.perp() } else { -u.perp() };
        let center = p + normal * rad;
        let start = (p - center).angle();
        // The round turns through the corner's turning angle.
        let sweep = u.cross(w).atan2(u.dot(w));
        out.push(Seg2::Line { a, b: p, source });
        out.push(Seg2::Arc { center, radius: rad, start, sweep, source: ROUND_TAG + k as u32 });
        cur = Seg2::Line { a: q, b: nb, source: ns };
    }
    out.push(cur);
    Ok(out)
}

/// The wall profiles for a sketch, with thicknesses already evaluated.
pub fn wall_profiles(
    sk: &Sketch,
    w: &ThinWall,
    t1: f64,
    t2: f64,
    fillet: Option<f64>,
) -> Result<Vec<Profile>, String> {
    if t1 <= 0.0 || (w.kind == ThinKind::TwoDirection && t2 < 0.0) {
        return Err("the wall thickness must be positive".into());
    }
    let mut profiles = Vec::new();
    for (ci, chain) in chains(sk)?.into_iter().enumerate() {
        let (left, right) = sides(w, chain.closed, t1, t2);
        let segs = match (fillet, chain.closed) {
            (Some(r), false) if r > 0.0 => round_corners(&chain.segs, |turns_left| {
                // The inside of a left turn is on the left.
                r + if turns_left { left } else { right }
            })?,
            _ => chain.segs.clone(),
        };
        let loops = thin_wall(&segs, chain.closed, left, right, CAP_TAG + 2 * ci as u32, LEFT_TAG)
            .map_err(|e| format!("thin wall: {e}"))?;
        let mut loops = loops.into_iter();
        let Some(first) = loops.next() else { continue };
        match loops.next() {
            Some(second) => {
                let (outer, inner) = if first.signed_area().abs() >= second.signed_area().abs() {
                    (first, second)
                } else {
                    (second, first)
                };
                if inner.signed_area().abs() < 1e-9 {
                    profiles.push(Profile::new(outer, vec![]));
                } else {
                    profiles.push(Profile::new(outer, vec![inner]));
                }
            }
            None => profiles.push(Profile::new(first, vec![])),
        }
    }
    if profiles.is_empty() {
        return Err("the sketch has no curves to thicken".into());
    }
    Ok(profiles)
}

/// The cap regions of a capped thin wall: each closed chain's whole outer outline.
pub fn outlines(walls: &[Profile]) -> Vec<Profile> {
    walls.iter().map(|p| Profile::new(p.outer.clone(), vec![])).collect()
}

/// The hollows inside capped walls: each wall's inner loop as a region.
pub fn hollows(walls: &[Profile]) -> Vec<Profile> {
    walls.iter().flat_map(|p| p.holes.iter().map(|h| Profile::new(h.clone(), vec![]))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use anvil_kernel::v2;
    use std::f64::consts::PI;

    fn l_chain() -> Sketch {
        // (0, 10) -> (0, 0) -> (20, 0), drawn in that order.
        let mut s = Sketch::new();
        let a = s.add_point(v2(0.0, 10.0));
        let b = s.add_point(v2(0.0, 0.0));
        let c = s.add_point(v2(20.0, 0.0));
        s.add_line(a, b);
        s.add_line(b, c);
        s
    }

    fn area(p: &[Profile]) -> f64 {
        p.iter().map(|p| p.area()).sum()
    }

    #[test]
    fn open_l_walls() {
        let s = l_chain();
        let mut w = ThinWall::new(2.0);
        w.kind = ThinKind::MidPlane;
        assert!((area(&wall_profiles(&s, &w, 2.0, 0.0, None).unwrap()) - 60.0).abs() < 1e-9);
        // One-Direction goes left of (0,10)->(0,0)->(20,0): the concave side.
        w.kind = ThinKind::OneDirection;
        assert!((area(&wall_profiles(&s, &w, 2.0, 0.0, None).unwrap()) - 56.0).abs() < 1e-9);
        w.reverse = true;
        assert!((area(&wall_profiles(&s, &w, 2.0, 0.0, None).unwrap()) - 64.0).abs() < 1e-9);
    }

    #[test]
    fn rounded_l_walls() {
        // Spec statement 25: inside radius 1, wall 2 on the concave side.
        let s = l_chain();
        let w = ThinWall::new(2.0);
        let a = area(&wall_profiles(&s, &w, 2.0, 0.0, Some(1.0)).unwrap());
        assert!((a - (48.0 + 2.0 * PI)).abs() < 1e-9, "{a}");
        let mut w = w;
        w.reverse = true;
        let a = area(&wall_profiles(&s, &w, 2.0, 0.0, Some(1.0)).unwrap());
        assert!((a - (56.0 + 2.0 * PI)).abs() < 1e-9, "{a}");
    }

    #[test]
    fn closed_rectangle_walls() {
        // Spec statement 21.
        let mut s = Sketch::new();
        s.add_rectangle(v2(0.0, 0.0), v2(20.0, 10.0));
        let mut w = ThinWall::new(1.0);
        assert!((area(&wall_profiles(&s, &w, 1.0, 0.0, None).unwrap()) - 64.0).abs() < 1e-9);
        w.reverse = true;
        assert!((area(&wall_profiles(&s, &w, 1.0, 0.0, None).unwrap()) - 56.0).abs() < 1e-9);
        w.reverse = false;
        // Half each side: 1 mm in all gives 21 x 11 - 19 x 9.
        w.kind = ThinKind::MidPlane;
        assert!((area(&wall_profiles(&s, &w, 1.0, 0.0, None).unwrap()) - 60.0).abs() < 1e-9);
        w.kind = ThinKind::TwoDirection;
        assert!((area(&wall_profiles(&s, &w, 1.0, 2.0, None).unwrap()) - 168.0).abs() < 1e-9);
    }

    #[test]
    fn branching_is_an_error() {
        let mut s = l_chain();
        let p = s.add_point(v2(-5.0, 0.0));
        // A third line from the L's corner.
        s.add_line(2, p);
        assert!(chains(&s).is_err());
    }
}
