//! Closed 2D profiles (an outer loop plus holes) built from exact lines and arcs.
//! These are what sketches hand to the solid-building operations.

use crate::math::{v2, Vec2};
use std::f64::consts::TAU;

/// One boundary segment. `source` identifies the sketch entity it came from, which
/// lets generated faces keep a stable identity across rebuilds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Seg2 {
    Line {
        a: Vec2,
        b: Vec2,
        source: u32,
    },
    /// Arc from angle `start` sweeping by `sweep` radians (positive = counter-clockwise).
    Arc {
        center: Vec2,
        radius: f64,
        start: f64,
        sweep: f64,
        source: u32,
    },
}

impl Seg2 {
    pub fn source(&self) -> u32 {
        match *self {
            Seg2::Line { source, .. } | Seg2::Arc { source, .. } => source,
        }
    }
    pub fn start_point(&self) -> Vec2 {
        match *self {
            Seg2::Line { a, .. } => a,
            Seg2::Arc { center, radius, start, .. } => center + Vec2::from_angle(start) * radius,
        }
    }
    pub fn end_point(&self) -> Vec2 {
        match *self {
            Seg2::Line { b, .. } => b,
            Seg2::Arc { center, radius, start, sweep, .. } => {
                center + Vec2::from_angle(start + sweep) * radius
            }
        }
    }
    pub fn reversed(&self) -> Seg2 {
        match *self {
            Seg2::Line { a, b, source } => Seg2::Line { a: b, b: a, source },
            Seg2::Arc { center, radius, start, sweep, source } => {
                Seg2::Arc { center, radius, start: start + sweep, sweep: -sweep, source }
            }
        }
    }
    pub fn is_arc(&self) -> bool {
        matches!(self, Seg2::Arc { .. })
    }

    /// Points along the segment, excluding the end point. `tol` is the max chord deviation.
    pub fn sample(&self, tol: f64, out: &mut Vec<Vec2>) {
        match *self {
            Seg2::Line { a, .. } => out.push(a),
            Seg2::Arc { center, radius, start, sweep, .. } => {
                let n = arc_segments(radius, sweep, tol);
                for i in 0..n {
                    let t = start + sweep * (i as f64 / n as f64);
                    out.push(center + Vec2::from_angle(t) * radius);
                }
            }
        }
    }
}

/// Number of chords used to approximate an arc within `tol`, capped at 10 degrees per chord.
pub fn arc_segments(radius: f64, sweep: f64, tol: f64) -> usize {
    let max_step = 10f64.to_radians();
    let step = if radius > tol {
        (2.0 * (1.0 - tol / radius).clamp(-1.0, 1.0).acos()).min(max_step)
    } else {
        max_step
    };
    ((sweep.abs() / step).ceil() as usize).max(if sweep.abs() > TAU - 1e-9 { 12 } else { 1 })
}

/// A closed chain of segments, each one starting where the previous ended.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Loop2 {
    pub segs: Vec<Seg2>,
}

/// One sampled boundary point plus the index of the segment that starts there.
#[derive(Clone, Copy, Debug)]
pub struct LoopSample {
    pub p: Vec2,
    pub seg: usize,
}

impl Loop2 {
    pub fn new(segs: Vec<Seg2>) -> Loop2 {
        Loop2 { segs }
    }

    pub fn circle(center: Vec2, radius: f64, source: u32) -> Loop2 {
        Loop2::new(vec![Seg2::Arc { center, radius, start: 0.0, sweep: TAU, source }])
    }

    pub fn polygon(points: &[Vec2], first_source: u32) -> Loop2 {
        let n = points.len();
        Loop2::new(
            (0..n)
                .map(|i| Seg2::Line { a: points[i], b: points[(i + 1) % n], source: first_source + i as u32 })
                .collect(),
        )
    }

    pub fn rectangle(min: Vec2, max: Vec2, first_source: u32) -> Loop2 {
        Loop2::polygon(&[min, v2(max.x, min.y), max, v2(min.x, max.y)], first_source)
    }

    /// Sampled polyline (closed implicitly, last point not repeated).
    pub fn samples(&self, tol: f64) -> Vec<LoopSample> {
        let mut out = Vec::new();
        let mut pts = Vec::new();
        for (i, s) in self.segs.iter().enumerate() {
            pts.clear();
            s.sample(tol, &mut pts);
            out.extend(pts.iter().map(|&p| LoopSample { p, seg: i }));
        }
        // Drop accidental duplicates (e.g. zero-length segments).
        out.dedup_by(|b, a| a.p.distance(b.p) < 1e-9);
        if out.len() > 1 && out[0].p.distance(out[out.len() - 1].p) < 1e-9 {
            out.pop();
        }
        out
    }

    pub fn polyline(&self, tol: f64) -> Vec<Vec2> {
        self.samples(tol).into_iter().map(|s| s.p).collect()
    }

    /// Signed area of the exact loop (positive when counter-clockwise).
    pub fn signed_area(&self) -> f64 {
        let mut a = 0.0;
        for s in &self.segs {
            match *s {
                Seg2::Line { a: p, b: q, .. } => a += 0.5 * p.cross(q),
                Seg2::Arc { center, radius, start, sweep, .. } => {
                    // Green's theorem over the arc: 1/2 * integral of (x dy - y dx)
                    // = 1/2 * [r^2 t + cx r sin t - cy r cos t] from t0 to t1.
                    let (c, r) = (center, radius);
                    let (t0, t1) = (start, start + sweep);
                    a += 0.5
                        * (r * r * sweep + c.x * r * (t1.sin() - t0.sin()) + c.y * r * (t0.cos() - t1.cos()));
                }
            }
        }
        a
    }

    pub fn reversed(&self) -> Loop2 {
        Loop2::new(self.segs.iter().rev().map(|s| s.reversed()).collect())
    }

    /// Checks that each segment ends where the next begins.
    pub fn is_closed(&self, tol: f64) -> bool {
        let n = self.segs.len();
        n > 0 && (0..n).all(|i| self.segs[i].end_point().distance(self.segs[(i + 1) % n].start_point()) < tol)
    }
}

/// A region to extrude or revolve: one counter-clockwise outer loop and clockwise holes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Profile {
    pub outer: Loop2,
    pub holes: Vec<Loop2>,
}

impl Profile {
    pub fn new(outer: Loop2, holes: Vec<Loop2>) -> Profile {
        let mut p = Profile { outer, holes };
        p.normalize_orientation();
        p
    }

    pub fn normalize_orientation(&mut self) {
        if self.outer.signed_area() < 0.0 {
            self.outer = self.outer.reversed();
        }
        for h in &mut self.holes {
            if h.signed_area() > 0.0 {
                *h = h.reversed();
            }
        }
    }

    pub fn area(&self) -> f64 {
        self.outer.signed_area() + self.holes.iter().map(|h| h.signed_area()).sum::<f64>()
    }

    pub fn loops(&self) -> impl Iterator<Item = &Loop2> {
        std::iter::once(&self.outer).chain(self.holes.iter())
    }
}

/// Even-odd point in polygon test.
pub fn point_in_polygon(p: Vec2, poly: &[Vec2]) -> bool {
    let mut inside = false;
    let n = poly.len();
    let mut j = n.wrapping_sub(1);
    for i in 0..n {
        let (a, b) = (poly[i], poly[j]);
        if (a.y > p.y) != (b.y > p.y) {
            let x = a.x + (p.y - a.y) * (b.x - a.x) / (b.y - a.y);
            if p.x < x {
                inside = !inside;
            }
        }
        j = i;
    }
    inside
}

pub fn polygon_signed_area(poly: &[Vec2]) -> f64 {
    let n = poly.len();
    (0..n).map(|i| poly[i].cross(poly[(i + 1) % n])).sum::<f64>() * 0.5
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    #[test]
    fn areas() {
        let sq = Loop2::rectangle(v2(0.0, 0.0), v2(2.0, 3.0), 0);
        assert!((sq.signed_area() - 6.0).abs() < 1e-12);
        assert!((sq.reversed().signed_area() + 6.0).abs() < 1e-12);
        let c = Loop2::circle(v2(5.0, -1.0), 2.0, 0);
        assert!((c.signed_area() - PI * 4.0).abs() < 1e-9);
        let p = Profile::new(sq, vec![Loop2::circle(v2(1.0, 1.5), 0.5, 9)]);
        assert!((p.area() - (6.0 - PI * 0.25)).abs() < 1e-9);
    }

    #[test]
    fn half_disc_area() {
        // D-shape: arc from angle 0 to PI, closed by a line along the diameter.
        let l = Loop2::new(vec![
            Seg2::Arc { center: v2(1.0, 1.0), radius: 1.0, start: 0.0, sweep: PI, source: 0 },
            Seg2::Line { a: v2(0.0, 1.0), b: v2(2.0, 1.0), source: 1 },
        ]);
        assert!(l.is_closed(1e-9));
        assert!((l.signed_area() - PI / 2.0).abs() < 1e-9);
    }
}
