//! Turns sketch geometry into closed kernel profiles (outer loops with holes).

use crate::sketch::{Constraint, Geom, PointId, Sketch};
use anvil_kernel::profile::{point_in_polygon, Loop2, Profile, Seg2};
use anvil_kernel::Vec2;
use std::f64::consts::TAU;

#[derive(Clone, Debug, PartialEq)]
pub enum ProfileError {
    /// No closed geometry at all.
    Empty,
    /// A contour endpoint is not connected to exactly one other segment.
    OpenContour { point: PointId },
    /// A point joins three or more segments.
    Branching { point: PointId },
    /// Two contours (or one contour with itself) cross.
    SelfIntersecting,
}

impl std::fmt::Display for ProfileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProfileError::Empty => write!(f, "sketch contains no closed contours"),
            ProfileError::OpenContour { point } => {
                write!(f, "open contour: point {point} is not connected")
            }
            ProfileError::Branching { point } => {
                write!(f, "contour branches at point {point}")
            }
            ProfileError::SelfIntersecting => write!(f, "contours intersect each other"),
        }
    }
}

impl std::error::Error for ProfileError {}

const MERGE_TOL: f64 = 1e-6;

impl Sketch {
    /// Groups points that are coincident, by constraint or by position.
    fn point_roots(&self) -> Vec<PointId> {
        let n = self.points.len();
        let mut parent: Vec<usize> = (0..n).collect();
        fn find(p: &mut Vec<usize>, mut i: usize) -> usize {
            while p[i] != i {
                p[i] = p[p[i]];
                i = p[i];
            }
            i
        }
        let union = |p: &mut Vec<usize>, a: usize, b: usize| {
            let (ra, rb) = (find(p, a), find(p, b));
            if ra != rb {
                p[ra.max(rb)] = ra.min(rb);
            }
        };
        for c in &self.constraints {
            if let Constraint::Coincident { p, q } = *c {
                union(&mut parent, p, q);
            }
        }
        for i in 0..n {
            for j in i + 1..n {
                if self.pos(i).distance(self.pos(j)) < MERGE_TOL {
                    union(&mut parent, i, j);
                }
            }
        }
        (0..n).map(|i| find(&mut parent, i)).collect()
    }

    /// Closed loops formed by non-construction geometry, each oriented arbitrarily.
    pub fn loops(&self) -> Result<Vec<Loop2>, ProfileError> {
        let roots = self.point_roots();
        let mut loops = Vec::new();

        // Edges between point groups: (entity, start root, end root).
        let mut edges: Vec<(usize, usize, usize)> = Vec::new();
        for (ei, e) in self.entities.iter().enumerate() {
            if e.construction {
                continue;
            }
            match e.geom {
                Geom::Circle { c, r } => {
                    if r > MERGE_TOL {
                        loops.push(Loop2::circle(self.pos(c), r, ei as u32));
                    }
                }
                Geom::Line { a, b } => {
                    if roots[a] != roots[b] {
                        edges.push((ei, roots[a], roots[b]));
                    }
                }
                Geom::Arc { a, b, .. } => edges.push((ei, roots[a], roots[b])),
            }
        }

        let mut incident: std::collections::HashMap<usize, Vec<usize>> = Default::default();
        for (k, &(_, s, t)) in edges.iter().enumerate() {
            incident.entry(s).or_default().push(k);
            incident.entry(t).or_default().push(k);
        }
        for (&node, list) in &incident {
            if list.len() == 1 {
                return Err(ProfileError::OpenContour { point: node });
            }
            if list.len() > 2 {
                return Err(ProfileError::Branching { point: node });
            }
        }

        let mut used = vec![false; edges.len()];
        for start in 0..edges.len() {
            if used[start] {
                continue;
            }
            let mut segs = Vec::new();
            let mut k = start;
            let mut from = edges[start].1;
            loop {
                used[k] = true;
                let (ei, s, t) = edges[k];
                let forward = s == from;
                segs.push(self.segment(ei, forward));
                let to = if forward { t } else { s };
                // Full arcs (start == end) close on themselves.
                let next = incident[&to].iter().copied().find(|&j| j != k && !used[j]);
                match next {
                    Some(j) => {
                        k = j;
                        from = to;
                    }
                    None => break,
                }
            }
            loops.push(Loop2::new(segs));
        }
        Ok(loops)
    }

    fn segment(&self, ei: usize, forward: bool) -> Seg2 {
        let seg = match self.entities[ei].geom {
            Geom::Line { a, b } => Seg2::Line { a: self.pos(a), b: self.pos(b), source: ei as u32 },
            Geom::Arc { c, a, b } => {
                let (cc, pa, pb) = (self.pos(c), self.pos(a), self.pos(b));
                let start = (pa - cc).angle();
                let mut sweep = ((pb - cc).angle() - start).rem_euclid(TAU);
                if sweep < 1e-12 {
                    sweep = TAU;
                }
                Seg2::Arc { center: cc, radius: pa.distance(cc), start, sweep, source: ei as u32 }
            }
            Geom::Circle { .. } => unreachable!("circles are handled as whole loops"),
        };
        if forward {
            seg
        } else {
            seg.reversed()
        }
    }

    /// Builds extrudable regions: every loop at even nesting depth becomes an outer
    /// boundary, and loops directly inside it become its holes.
    pub fn profiles(&self) -> Result<Vec<Profile>, ProfileError> {
        let loops = self.loops()?;
        if loops.is_empty() {
            return Err(ProfileError::Empty);
        }
        let tol = 0.01;
        let polys: Vec<Vec<Vec2>> = loops.iter().map(|l| l.polyline(tol)).collect();
        if any_crossings(&polys) {
            return Err(ProfileError::SelfIntersecting);
        }
        let areas: Vec<f64> = loops.iter().map(|l| l.signed_area().abs()).collect();
        // A loop is inside another if a point on it is inside the other's polygon.
        let n = loops.len();
        let contains = |outer: usize, inner: usize| -> bool {
            outer != inner && areas[outer] > areas[inner] && {
                let a = polys[inner][0];
                let b = polys[inner][1 % polys[inner].len()];
                point_in_polygon((a + b) * 0.5, &polys[outer])
            }
        };
        let depth: Vec<usize> = (0..n).map(|i| (0..n).filter(|&j| contains(j, i)).count()).collect();
        let mut profiles = Vec::new();
        for i in 0..n {
            if !depth[i].is_multiple_of(2) {
                continue;
            }
            let holes = (0..n)
                .filter(|&j| depth[j] == depth[i] + 1 && contains(i, j))
                .map(|j| loops[j].clone())
                .collect();
            profiles.push(Profile::new(loops[i].clone(), holes));
        }
        Ok(profiles)
    }
}

fn any_crossings(polys: &[Vec<Vec2>]) -> bool {
    let segs: Vec<(usize, usize, Vec2, Vec2)> = polys
        .iter()
        .enumerate()
        .flat_map(|(li, p)| {
            let n = p.len();
            (0..n).map(move |i| (li, i, p[i], p[(i + 1) % n]))
        })
        .collect();
    for i in 0..segs.len() {
        for j in i + 1..segs.len() {
            let (la, ia, a0, a1) = segs[i];
            let (lb, ib, b0, b1) = segs[j];
            if la == lb {
                let n = polys[la].len();
                if (ia + 1) % n == ib || (ib + 1) % n == ia {
                    continue; // adjacent segments share an endpoint
                }
            }
            if proper_intersect(a0, a1, b0, b1) {
                return true;
            }
        }
    }
    false
}

fn proper_intersect(a0: Vec2, a1: Vec2, b0: Vec2, b1: Vec2) -> bool {
    let d1 = (a1 - a0).cross(b0 - a0);
    let d2 = (a1 - a0).cross(b1 - a0);
    let d3 = (b1 - b0).cross(a0 - b0);
    let d4 = (b1 - b0).cross(a1 - b0);
    let eps = 1e-12;
    ((d1 > eps && d2 < -eps) || (d1 < -eps && d2 > eps))
        && ((d3 > eps && d4 < -eps) || (d3 < -eps && d4 > eps))
}
