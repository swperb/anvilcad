//! A machine-independent tool path: moves (rapid, linear, arc), feeds, spindle,
//! coolant, tool changes and drilling cycles, in program coordinates (millimetres,
//! the tool axis along +Z). Post processors turn this into a machine's dialect.

use crate::geom::arc_steps;
use anvil_kernel::{Vec2, Vec3, v2, v3};
use serde::{Deserialize, Serialize};

/// Why a move is there. Simulation and verification treat kinds differently: only
/// `Cut` moves are held to the engagement limit, `Link` moves must not touch material,
/// and `Rapid` moves must stay clear of the stock.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MoveKind {
    /// Positioning at rapid rate.
    Rapid,
    /// Cutting at the programmed feed.
    Cut,
    /// Entering material: helix, ramp or plunge.
    Entry,
    /// Feed move through already cleared space between cuts.
    Link,
    /// Lead-in or lead-out arc or line of a finishing pass.
    Lead,
    /// Feed move away from the work before a rapid.
    Retract,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Coolant {
    Off,
    Flood,
    Mist,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SpindleDir {
    Clockwise,
    CounterClockwise,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Motion {
    Line,
    /// Circular arc in the XY plane about `center` (Z may change linearly: a helix).
    Arc {
        center: Vec2,
        ccw: bool,
    },
}

/// One motion. The start is wherever the previous move ended.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Move {
    pub to: Vec3,
    pub motion: Motion,
    pub kind: MoveKind,
    /// Feed, mm/min; `None` for rapids.
    pub feed: Option<f64>,
}

/// Canned drilling cycles.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum CycleKind {
    /// Feed to depth, rapid out (G81 style).
    Drill,
    /// Feed to depth, dwell, rapid out (G82 style).
    SpotDwell { dwell_s: f64 },
    /// Full retract between pecks (G83 style).
    Peck { peck: f64 },
    /// Short chip-breaking retract between pecks (G73 style).
    ChipBreak { peck: f64 },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DrillCycle {
    pub kind: CycleKind,
    /// Hole positions (XY).
    pub points: Vec<Vec2>,
    /// Top of the holes.
    pub top: f64,
    /// Rapid-to plane above each hole.
    pub r_plane: f64,
    /// Final tool-tip depth.
    pub z: f64,
    /// Plane to return to between holes.
    pub clearance: f64,
    pub feed: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Item {
    Comment(String),
    ToolChange {
        tool: u32,
        name: String,
    },
    Spindle {
        rpm: f64,
        dir: SpindleDir,
    },
    SpindleStop,
    Coolant(Coolant),
    Move(Move),
    Cycle(DrillCycle),
    Dwell {
        seconds: f64,
    },
    /// Optional stop for the operator (M1 style).
    OptionalStop,
}

/// A tool path for one or more operations.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Toolpath {
    pub items: Vec<Item>,
    /// Where the tool is before the first move (usually above the part at clearance).
    pub start: Option<Vec3>,
}

/// Distance and time totals.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PathStats {
    pub rapid_length: f64,
    pub feed_length: f64,
    pub cut_length: f64,
    /// Estimated feed time in minutes (rapids excluded).
    pub feed_minutes: f64,
    pub moves: usize,
}

impl Toolpath {
    pub fn new() -> Toolpath {
        Toolpath::default()
    }

    pub fn push(&mut self, item: Item) {
        self.items.push(item);
    }

    pub fn rapid(&mut self, to: Vec3) {
        self.items.push(Item::Move(Move { to, motion: Motion::Line, kind: MoveKind::Rapid, feed: None }));
    }

    pub fn line(&mut self, to: Vec3, kind: MoveKind, feed: f64) {
        self.items.push(Item::Move(Move { to, motion: Motion::Line, kind, feed: Some(feed) }));
    }

    pub fn arc(&mut self, to: Vec3, center: Vec2, ccw: bool, kind: MoveKind, feed: f64) {
        self.items.push(Item::Move(Move { to, motion: Motion::Arc { center, ccw }, kind, feed: Some(feed) }));
    }

    pub fn append(&mut self, other: Toolpath) {
        if self.start.is_none() {
            self.start = other.start;
        }
        self.items.extend(other.items);
    }

    /// Position after the last move, if any.
    pub fn end(&self) -> Option<Vec3> {
        let mut pos = self.start;
        for it in &self.items {
            match it {
                Item::Move(m) => pos = Some(m.to),
                Item::Cycle(c) => {
                    if let Some(p) = c.points.last() {
                        pos = Some(v3(p.x, p.y, c.clearance));
                    }
                }
                _ => {}
            }
        }
        pos
    }

    /// The same path with drilling cycles written out as plain moves, so a simulator
    /// or a machine without canned cycles can run it.
    pub fn expanded(&self) -> Toolpath {
        let mut out = Toolpath { items: Vec::with_capacity(self.items.len()), start: self.start };
        for it in &self.items {
            match it {
                Item::Cycle(c) => expand_cycle(c, &mut out),
                other => out.items.push(other.clone()),
            }
        }
        out
    }

    /// Straight segments that follow every move (arcs flattened within `tol`), with the
    /// move each came from. Drilling cycles are expanded first.
    pub fn segments(&self, tol: f64) -> Vec<Segment> {
        let ex = self.expanded();
        let mut out = Vec::new();
        let Some(mut pos) = ex.start.or_else(|| ex.first_position()) else { return out };
        for (idx, it) in ex.items.iter().enumerate() {
            let Item::Move(m) = it else { continue };
            match m.motion {
                Motion::Line => {
                    out.push(Segment { a: pos, b: m.to, kind: m.kind, item: idx, feed: m.feed });
                }
                Motion::Arc { center, ccw } => {
                    for (a, b) in arc_points(pos, m.to, center, ccw, tol).windows(2).map(|w| (w[0], w[1])) {
                        out.push(Segment { a, b, kind: m.kind, item: idx, feed: m.feed });
                    }
                }
            }
            pos = m.to;
        }
        out
    }

    fn first_position(&self) -> Option<Vec3> {
        self.items.iter().find_map(|it| if let Item::Move(m) = it { Some(m.to) } else { None })
    }

    /// Lengths and feed time, with arcs and helices measured exactly.
    pub fn stats(&self) -> PathStats {
        let ex = self.expanded();
        let mut s = PathStats::default();
        let Some(mut pos) = ex.start.or_else(|| ex.first_position()) else { return s };
        for m in ex.moves() {
            let l = match m.motion {
                Motion::Line => pos.distance(m.to),
                Motion::Arc { center, ccw } => {
                    let r = (v2(pos.x, pos.y) - center).length();
                    let planar = r * arc_sweep(pos, m.to, center, ccw).abs();
                    (planar * planar + (m.to.z - pos.z).powi(2)).sqrt()
                }
            };
            s.moves += 1;
            match m.kind {
                MoveKind::Rapid => s.rapid_length += l,
                k => {
                    s.feed_length += l;
                    if k == MoveKind::Cut {
                        s.cut_length += l;
                    }
                    if let Some(f) = m.feed.filter(|f| *f > 0.0) {
                        s.feed_minutes += l / f;
                    }
                }
            }
            pos = m.to;
        }
        s
    }

    /// Every move, for checks.
    pub fn moves(&self) -> impl Iterator<Item = &Move> {
        self.items.iter().filter_map(|it| if let Item::Move(m) = it { Some(m) } else { None })
    }
}

/// One straight piece of a tool path.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Segment {
    pub a: Vec3,
    pub b: Vec3,
    pub kind: MoveKind,
    /// Index of the item in the expanded tool path.
    pub item: usize,
    pub feed: Option<f64>,
}

/// Points along an arc (helical when the Z values differ) from `from` to `to`, both
/// included. A full circle is assumed when start and end coincide in XY.
pub fn arc_points(from: Vec3, to: Vec3, center: Vec2, ccw: bool, tol: f64) -> Vec<Vec3> {
    let r0 = v2(from.x, from.y) - center;
    let r1 = v2(to.x, to.y) - center;
    let radius = r0.length().max(1e-9);
    let a0 = r0.angle();
    let mut a1 = r1.angle();
    if ccw {
        while a1 <= a0 + 1e-12 {
            a1 += std::f64::consts::TAU;
        }
    } else {
        while a1 >= a0 - 1e-12 {
            a1 -= std::f64::consts::TAU;
        }
    }
    let sweep = a1 - a0;
    let n = arc_steps(radius, sweep, tol);
    let mut out = Vec::with_capacity(n + 1);
    for k in 0..=n {
        let t = k as f64 / n as f64;
        let r = r0.length() + (r1.length() - r0.length()) * t;
        let p = center + Vec2::from_angle(a0 + sweep * t) * r;
        out.push(v3(p.x, p.y, from.z + (to.z - from.z) * t));
    }
    if let Some(last) = out.last_mut() {
        *last = to;
    }
    out
}

/// Signed sweep of an arc move (positive counter-clockwise), as [`arc_points`] reads it.
pub fn arc_sweep(from: Vec3, to: Vec3, center: Vec2, ccw: bool) -> f64 {
    let a0 = (v2(from.x, from.y) - center).angle();
    let mut a1 = (v2(to.x, to.y) - center).angle();
    if ccw {
        while a1 <= a0 + 1e-12 {
            a1 += std::f64::consts::TAU;
        }
    } else {
        while a1 >= a0 - 1e-12 {
            a1 -= std::f64::consts::TAU;
        }
    }
    a1 - a0
}

fn expand_cycle(c: &DrillCycle, out: &mut Toolpath) {
    for p in &c.points {
        let at = |z: f64| v3(p.x, p.y, z);
        out.rapid(at(c.clearance));
        out.rapid(at(c.r_plane));
        match c.kind {
            CycleKind::Drill => {
                out.line(at(c.z), MoveKind::Entry, c.feed);
            }
            CycleKind::SpotDwell { dwell_s } => {
                out.line(at(c.z), MoveKind::Entry, c.feed);
                out.push(Item::Dwell { seconds: dwell_s });
            }
            CycleKind::Peck { peck } | CycleKind::ChipBreak { peck } => {
                let full = matches!(c.kind, CycleKind::Peck { .. });
                // Pecks are measured from the R plane, as controls do; at most 10 000 per hole.
                let peck = peck.max((c.r_plane - c.z).abs() / 10_000.0).max(0.01);
                let mut depth = c.r_plane;
                let mut first = true;
                loop {
                    let next = (depth - peck).max(c.z);
                    if !first && full {
                        // Back down to just above the previous bottom before feeding on.
                        out.rapid(at(depth + 0.5));
                    }
                    out.line(at(next), MoveKind::Entry, c.feed);
                    first = false;
                    depth = next;
                    if depth <= c.z + 1e-9 {
                        break;
                    }
                    out.rapid(at(if full { c.r_plane } else { depth + 0.5 }));
                }
            }
        }
        out.rapid(at(c.clearance));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arc_points_quarter() {
        let pts = arc_points(v3(1.0, 0.0, 0.0), v3(0.0, 1.0, -1.0), Vec2::ZERO, true, 0.001);
        assert!(pts.len() > 10);
        for p in &pts {
            assert!((v2(p.x, p.y).length() - 1.0).abs() < 1e-9);
        }
        assert_eq!(*pts.last().unwrap(), v3(0.0, 1.0, -1.0));
        assert!(
            (arc_sweep(v3(1.0, 0.0, 0.0), v3(0.0, 1.0, 0.0), Vec2::ZERO, false) + 1.5 * std::f64::consts::PI)
                .abs()
                < 1e-9
        );
    }

    #[test]
    fn peck_cycle_expands_to_depth() {
        let mut tp = Toolpath::new();
        tp.push(Item::Cycle(DrillCycle {
            kind: CycleKind::Peck { peck: 3.0 },
            points: vec![v2(5.0, 5.0)],
            top: 0.0,
            r_plane: 2.0,
            z: -10.0,
            clearance: 10.0,
            feed: 100.0,
        }));
        let ex = tp.expanded();
        let feeds: Vec<f64> = ex.moves().filter(|m| m.kind == MoveKind::Entry).map(|m| m.to.z).collect();
        assert_eq!(feeds, vec![-1.0, -4.0, -7.0, -10.0]);
        assert_eq!(ex.end().unwrap(), v3(5.0, 5.0, 10.0));
    }
}
