//! Planar geometry for 2.5D machining: regions (an outer loop with islands), loops with
//! arcs, exact point-to-boundary distances, and polygon offsetting.
//!
//! Offsetting uses the `cavalier_contours` crate (MIT OR Apache-2.0), which offsets
//! polylines made of lines and arcs and keeps the arcs exact. Inputs are cleaned before
//! they are handed over (repeated points, zero-area loops) and the call is guarded, so a
//! degenerate shape returns an error instead of stopping the program.

use crate::error::{CamError, Result};
use anvil_kernel::{Vec2, v2};
use cavalier_contours::polyline::{PlineSource, PlineSourceMut, Polyline};
use cavalier_contours::shape_algorithms::{Shape, ShapeOffsetOptions};
use serde::{Deserialize, Serialize};
use std::f64::consts::PI;

/// Points closer than this are the same point (mm).
pub const POINT_EPS: f64 = 1e-7;

/// Signed area of a closed polyline (positive when counter-clockwise).
pub fn signed_area(pts: &[Vec2]) -> f64 {
    let n = pts.len();
    let mut a = 0.0;
    for i in 0..n {
        let (p, q) = (pts[i], pts[(i + 1) % n]);
        a += p.cross(q);
    }
    a * 0.5
}

/// Even-odd point in polygon test.
pub fn point_in_loop(p: Vec2, pts: &[Vec2]) -> bool {
    let n = pts.len();
    let mut inside = false;
    let mut j = n.wrapping_sub(1);
    for i in 0..n {
        let (a, b) = (pts[i], pts[j]);
        if (a.y > p.y) != (b.y > p.y) {
            let x = a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x);
            if p.x < x {
                inside = !inside;
            }
        }
        j = i;
    }
    inside
}

/// Distance from `p` to the segment `a`-`b`.
pub fn seg_distance(p: Vec2, a: Vec2, b: Vec2) -> f64 {
    let ab = b - a;
    let l2 = ab.dot(ab);
    let t = if l2 < 1e-24 { 0.0 } else { ((p - a).dot(ab) / l2).clamp(0.0, 1.0) };
    (a + ab * t).distance(p)
}

/// Removes repeated and collinear points and checks a loop is usable.
fn clean_loop(pts: &[Vec2]) -> Result<Vec<Vec2>> {
    if pts.iter().any(|p| !p.x.is_finite() || !p.y.is_finite()) {
        return Err(CamError::InvalidGeometry("a loop has a coordinate that is not a finite number".into()));
    }
    let mut out: Vec<Vec2> = Vec::with_capacity(pts.len());
    for &p in pts {
        if out.last().is_none_or(|q| q.distance(p) > POINT_EPS) {
            out.push(p);
        }
    }
    while out.len() > 1 && out[0].distance(out[out.len() - 1]) <= POINT_EPS {
        out.pop();
    }
    // Drop points that lie on the line through their neighbours.
    let mut changed = true;
    while changed && out.len() >= 3 {
        changed = false;
        let n = out.len();
        for i in 0..n {
            let (a, b, c) = (out[(i + n - 1) % n], out[i], out[(i + 1) % n]);
            let len = a.distance(c).max(1e-12);
            if ((b - a).cross(c - a) / len).abs() < 1e-9 && (b - a).dot(c - b) >= 0.0 {
                out.remove(i);
                changed = true;
                break;
            }
        }
    }
    if out.len() < 3 || signed_area(&out).abs() < 1e-9 {
        return Err(CamError::InvalidGeometry(
            "a loop has fewer than three distinct points or no area".into(),
        ));
    }
    Ok(out)
}

/// A planar region to machine: one outer boundary with islands inside it. The outer loop
/// is stored counter-clockwise and islands clockwise.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Region {
    pub outer: Vec<Vec2>,
    pub islands: Vec<Vec<Vec2>>,
}

impl Region {
    /// Builds a region, cleaning and orienting its loops. Islands must lie inside the
    /// outer loop and must not overlap each other.
    pub fn new(outer: Vec<Vec2>, islands: Vec<Vec<Vec2>>) -> Result<Region> {
        let mut outer = clean_loop(&outer)?;
        if signed_area(&outer) < 0.0 {
            outer.reverse();
        }
        let mut cleaned = Vec::with_capacity(islands.len());
        for isl in islands {
            let mut isl = clean_loop(&isl)?;
            if signed_area(&isl) > 0.0 {
                isl.reverse();
            }
            if !isl.iter().all(|p| point_in_loop(*p, &outer)) {
                return Err(CamError::InvalidGeometry("an island is not inside the outer boundary".into()));
            }
            cleaned.push(isl);
        }
        Ok(Region { outer, islands: cleaned })
    }

    /// An axis-aligned rectangle.
    pub fn rectangle(min: Vec2, max: Vec2) -> Result<Region> {
        Region::new(vec![min, v2(max.x, min.y), max, v2(min.x, max.y)], Vec::new())
    }

    /// A circle approximated by a polygon whose vertices lie on the circle.
    pub fn circle(center: Vec2, radius: f64, segments: usize) -> Result<Region> {
        Region::new(circle_points(center, radius, segments), Vec::new())
    }

    /// All loops: the outer first, then the islands.
    pub fn loops(&self) -> impl Iterator<Item = &Vec<Vec2>> {
        std::iter::once(&self.outer).chain(self.islands.iter())
    }

    pub fn contains(&self, p: Vec2) -> bool {
        point_in_loop(p, &self.outer) && !self.islands.iter().any(|i| point_in_loop(p, i))
    }

    pub fn area(&self) -> f64 {
        signed_area(&self.outer) + self.islands.iter().map(|i| signed_area(i)).sum::<f64>()
    }

    /// (min, max) corners of the outer loop.
    pub fn bounds(&self) -> (Vec2, Vec2) {
        let mut lo = v2(f64::INFINITY, f64::INFINITY);
        let mut hi = v2(f64::NEG_INFINITY, f64::NEG_INFINITY);
        for p in &self.outer {
            lo = v2(lo.x.min(p.x), lo.y.min(p.y));
            hi = v2(hi.x.max(p.x), hi.y.max(p.y));
        }
        (lo, hi)
    }

    /// Distance from `p` to the nearest boundary segment of any loop.
    pub fn boundary_distance(&self, p: Vec2) -> f64 {
        let mut best = f64::INFINITY;
        for l in self.loops() {
            let n = l.len();
            for i in 0..n {
                best = best.min(seg_distance(p, l[i], l[(i + 1) % n]));
            }
        }
        best
    }

    /// Every boundary segment.
    pub fn segments(&self) -> Vec<(Vec2, Vec2)> {
        let mut out = Vec::new();
        for l in self.loops() {
            let n = l.len();
            for i in 0..n {
                out.push((l[i], l[(i + 1) % n]));
            }
        }
        out
    }
}

/// Points on a circle, counter-clockwise.
pub fn circle_points(center: Vec2, radius: f64, segments: usize) -> Vec<Vec2> {
    let n = segments.max(8);
    (0..n).map(|i| center + Vec2::from_angle(2.0 * PI * i as f64 / n as f64) * radius).collect()
}

/// A closed loop of lines and arcs. `bulges[i]` describes the segment from `pts[i]` to
/// `pts[i + 1]` (wrapping): zero for a line, otherwise tan(sweep / 4) with positive values
/// for counter-clockwise arcs.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ArcLoop {
    pub pts: Vec<Vec2>,
    pub bulges: Vec<f64>,
}

/// An arc described by its centre, radius, start angle and signed sweep.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ArcGeom {
    pub center: Vec2,
    pub radius: f64,
    pub start: f64,
    pub sweep: f64,
}

/// The arc from `p0` to `p1` with bulge `b`, or `None` for a straight segment.
pub fn bulge_arc(p0: Vec2, p1: Vec2, b: f64) -> Option<ArcGeom> {
    if b.abs() < 1e-12 || p0.distance(p1) < POINT_EPS {
        return None;
    }
    let sweep = 4.0 * b.atan();
    let chord = p1 - p0;
    let c = chord.length();
    let left = chord.perp() / c;
    let d = (c * 0.5) / (sweep * 0.5).tan();
    let center = (p0 + p1) * 0.5 + left * d;
    let radius = center.distance(p0);
    Some(ArcGeom { center, radius, start: (p0 - center).angle(), sweep })
}

impl ArcLoop {
    /// A loop of straight segments.
    pub fn from_polygon(pts: &[Vec2]) -> ArcLoop {
        ArcLoop { pts: pts.to_vec(), bulges: vec![0.0; pts.len()] }
    }

    pub fn len(&self) -> usize {
        self.pts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pts.is_empty()
    }

    /// Flattens the loop to a polyline within `tol` of the arcs (vertices on the arcs).
    pub fn flatten(&self, tol: f64) -> Vec<Vec2> {
        let n = self.pts.len();
        let mut out = Vec::new();
        for i in 0..n {
            let (p0, p1) = (self.pts[i], self.pts[(i + 1) % n]);
            out.push(p0);
            if let Some(a) = bulge_arc(p0, p1, self.bulges.get(i).copied().unwrap_or(0.0)) {
                let k = arc_steps(a.radius, a.sweep, tol);
                for j in 1..k {
                    let t = a.start + a.sweep * j as f64 / k as f64;
                    out.push(a.center + Vec2::from_angle(t) * a.radius);
                }
            }
        }
        out
    }

    /// Signed area including arc segments (positive counter-clockwise).
    pub fn signed_area(&self) -> f64 {
        let n = self.pts.len();
        let mut a = signed_area(&self.pts);
        for i in 0..n {
            if let Some(g) = bulge_arc(self.pts[i], self.pts[(i + 1) % n], self.bulges[i]) {
                // Circular segment between chord and arc.
                let s = g.sweep;
                a += 0.5 * g.radius * g.radius * (s - s.sin());
            }
        }
        a
    }

    /// Total length.
    pub fn length(&self) -> f64 {
        let n = self.pts.len();
        (0..n)
            .map(|i| {
                let (p0, p1) = (self.pts[i], self.pts[(i + 1) % n]);
                match bulge_arc(p0, p1, self.bulges[i]) {
                    Some(a) => a.radius * a.sweep.abs(),
                    None => p0.distance(p1),
                }
            })
            .sum()
    }

    /// The loop traversed the other way.
    pub fn reversed(&self) -> ArcLoop {
        let n = self.pts.len();
        let mut pts = Vec::with_capacity(n);
        let mut bulges = Vec::with_capacity(n);
        for i in 0..n {
            // Segment i (pts[i] -> pts[i+1]) becomes pts[i+1] -> pts[i].
            let j = n - 1 - i;
            pts.push(self.pts[(j + 1) % n]);
            bulges.push(-self.bulges[j]);
        }
        ArcLoop { pts, bulges }
    }

    /// The same loop starting at vertex `k`.
    pub fn rotated(&self, k: usize) -> ArcLoop {
        let n = self.pts.len();
        if n == 0 {
            return self.clone();
        }
        let k = k % n;
        let mut pts = self.pts[k..].to_vec();
        pts.extend_from_slice(&self.pts[..k]);
        let mut bulges = self.bulges[k..].to_vec();
        bulges.extend_from_slice(&self.bulges[..k]);
        ArcLoop { pts, bulges }
    }
}

/// Number of chords that keep an arc within `tol`.
pub fn arc_steps(radius: f64, sweep: f64, tol: f64) -> usize {
    let tol = tol.max(1e-6);
    let step = if radius > tol { 2.0 * (1.0 - tol / radius).clamp(-1.0, 1.0).acos() } else { PI / 4.0 };
    let step = step.clamp(1e-3, PI / 8.0);
    ((sweep.abs() / step).ceil() as usize).clamp(1, 100_000)
}

fn to_pline(l: &ArcLoop) -> Polyline<f64> {
    let mut pl = Polyline::new_closed();
    for (p, b) in l.pts.iter().zip(&l.bulges) {
        pl.add(p.x, p.y, *b);
    }
    pl
}

fn from_pline(pl: &Polyline<f64>) -> ArcLoop {
    let mut pts = Vec::with_capacity(pl.vertex_count());
    let mut bulges = Vec::with_capacity(pl.vertex_count());
    for v in pl.iter_vertexes() {
        pts.push(v2(v.x, v.y));
        bulges.push(v.bulge);
    }
    ArcLoop { pts, bulges }
}

/// Result of offsetting a region: loops that bound material-free tool-centre paths.
/// Outer loops are counter-clockwise, hole loops clockwise.
#[derive(Clone, Debug, Default)]
pub struct OffsetResult {
    pub loops: Vec<ArcLoop>,
}

/// Offsets every loop of `region` by `distance`: positive moves the boundary into the
/// region (the outer loop shrinks and islands grow), negative moves it out. Returns the
/// resulting loops, which may be more or fewer than the input (a narrow neck splits a
/// region; a large offset makes it vanish).
pub fn offset_region(region: &Region, distance: f64) -> Result<OffsetResult> {
    let loops: Vec<ArcLoop> = region.loops().map(|l| ArcLoop::from_polygon(l)).collect();
    offset_loops(&loops, distance)
}

/// Offsets loops of lines and arcs (outer loops counter-clockwise, holes clockwise) by
/// `distance`, positive into the material-free side as for [`offset_region`].
pub fn offset_loops(loops: &[ArcLoop], distance: f64) -> Result<OffsetResult> {
    if !distance.is_finite() {
        return Err(CamError::InvalidParameter { name: "offset", reason: "must be finite".into() });
    }
    let mut plines = Vec::new();
    for l in loops {
        if l.pts.len() < 2 || l.bulges.len() != l.pts.len() {
            return Err(CamError::InvalidGeometry("malformed loop".into()));
        }
        if l.pts.iter().any(|p| !p.x.is_finite() || !p.y.is_finite())
            || l.bulges.iter().any(|b| !b.is_finite())
        {
            return Err(CamError::InvalidGeometry("a loop has a value that is not finite".into()));
        }
        let pl = to_pline(l);
        // `None` means there was nothing to remove.
        let pl = pl.remove_repeat_pos(1e-7).unwrap_or(pl);
        if pl.vertex_count() < 2 || pl.area().abs() < 1e-9 {
            continue;
        }
        plines.push(pl);
    }
    if plines.is_empty() {
        return Ok(OffsetResult::default());
    }
    if distance.abs() < 1e-12 {
        return Ok(OffsetResult { loops: plines.iter().map(from_pline).collect() });
    }
    // The offset library asserts on some degenerate inputs; keep such a failure from
    // unwinding into the caller.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let shape = Shape::from_plines(plines);
        shape.parallel_offset(distance, &ShapeOffsetOptions::new())
    }))
    .map_err(|_| CamError::InvalidGeometry("the offset could not be computed for this shape".into()))?;
    let mut out = Vec::new();
    for ip in result.ccw_plines.iter().chain(result.cw_plines.iter()) {
        if ip.polyline.vertex_count() >= 2 {
            out.push(from_pline(&ip.polyline));
        }
    }
    Ok(OffsetResult { loops: out })
}

/// A spatial bucket index over segments, for fast nearest-boundary distance queries.
#[derive(Clone, Debug)]
pub struct SegIndex {
    segs: Vec<(Vec2, Vec2)>,
    origin: Vec2,
    cell: f64,
    nx: usize,
    ny: usize,
    buckets: Vec<Vec<u32>>,
}

impl SegIndex {
    /// Builds an index with buckets of about `cell` mm.
    pub fn new(segs: Vec<(Vec2, Vec2)>, cell: f64) -> SegIndex {
        let mut lo = v2(f64::INFINITY, f64::INFINITY);
        let mut hi = v2(f64::NEG_INFINITY, f64::NEG_INFINITY);
        for (a, b) in &segs {
            for p in [a, b] {
                lo = v2(lo.x.min(p.x), lo.y.min(p.y));
                hi = v2(hi.x.max(p.x), hi.y.max(p.y));
            }
        }
        if segs.is_empty() {
            return SegIndex { segs, origin: Vec2::ZERO, cell: 1.0, nx: 0, ny: 0, buckets: Vec::new() };
        }
        let ext = (hi - lo).x.max((hi - lo).y).max(1e-6);
        // Keep the bucket grid bounded whatever the cell size asked for.
        let cell = cell.max(ext / 512.0).max(1e-6);
        let nx = (((hi.x - lo.x) / cell).floor() as usize + 1).min(1024);
        let ny = (((hi.y - lo.y) / cell).floor() as usize + 1).min(1024);
        let mut buckets = vec![Vec::new(); nx * ny];
        for (k, (a, b)) in segs.iter().enumerate() {
            let (i0, j0) = Self::bucket_of(lo, cell, nx, ny, v2(a.x.min(b.x), a.y.min(b.y)));
            let (i1, j1) = Self::bucket_of(lo, cell, nx, ny, v2(a.x.max(b.x), a.y.max(b.y)));
            for j in j0..=j1 {
                for i in i0..=i1 {
                    buckets[j * nx + i].push(k as u32);
                }
            }
        }
        SegIndex { segs, origin: lo, cell, nx, ny, buckets }
    }

    fn bucket_of(origin: Vec2, cell: f64, nx: usize, ny: usize, p: Vec2) -> (usize, usize) {
        let i = ((p.x - origin.x) / cell).floor().clamp(0.0, (nx - 1) as f64) as usize;
        let j = ((p.y - origin.y) / cell).floor().clamp(0.0, (ny - 1) as f64) as usize;
        (i, j)
    }

    /// True when some segment passes within `r` of `p`.
    pub fn any_within(&self, p: Vec2, r: f64) -> bool {
        if self.nx == 0 {
            return false;
        }
        let (i0, j0) = Self::bucket_of(self.origin, self.cell, self.nx, self.ny, p - v2(r, r));
        let (i1, j1) = Self::bucket_of(self.origin, self.cell, self.nx, self.ny, p + v2(r, r));
        for j in j0..=j1 {
            for i in i0..=i1 {
                for &k in &self.buckets[j * self.nx + i] {
                    let (a, b) = self.segs[k as usize];
                    if seg_distance(p, a, b) < r {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Distance to the nearest segment, searching at most `max_r` away (returns
    /// `max_r` when nothing is closer).
    pub fn distance_within(&self, p: Vec2, max_r: f64) -> f64 {
        if self.nx == 0 {
            return max_r;
        }
        let (i0, j0) = Self::bucket_of(self.origin, self.cell, self.nx, self.ny, p - v2(max_r, max_r));
        let (i1, j1) = Self::bucket_of(self.origin, self.cell, self.nx, self.ny, p + v2(max_r, max_r));
        let mut best = max_r;
        for j in j0..=j1 {
            for i in i0..=i1 {
                for &k in &self.buckets[j * self.nx + i] {
                    let (a, b) = self.segs[k as usize];
                    best = best.min(seg_distance(p, a, b));
                }
            }
        }
        best
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn region_orients_and_contains() {
        let r = Region::new(
            vec![v2(0.0, 0.0), v2(0.0, 10.0), v2(10.0, 10.0), v2(10.0, 0.0)],
            vec![vec![v2(4.0, 4.0), v2(6.0, 4.0), v2(6.0, 6.0), v2(4.0, 6.0)]],
        )
        .unwrap();
        assert!(signed_area(&r.outer) > 0.0);
        assert!(signed_area(&r.islands[0]) < 0.0);
        assert!((r.area() - 96.0).abs() < 1e-9);
        assert!(r.contains(v2(1.0, 1.0)));
        assert!(!r.contains(v2(5.0, 5.0)));
        assert!((r.boundary_distance(v2(5.0, 2.0)) - 2.0).abs() < 1e-12);
    }

    #[test]
    fn bad_regions_are_errors() {
        assert!(Region::new(vec![v2(0.0, 0.0), v2(1.0, 0.0)], vec![]).is_err());
        assert!(Region::new(vec![v2(0.0, 0.0), v2(1.0, 0.0), v2(f64::NAN, 1.0)], vec![]).is_err());
        let outside = vec![v2(20.0, 20.0), v2(21.0, 20.0), v2(21.0, 21.0)];
        assert!(Region::new(vec![v2(0.0, 0.0), v2(10.0, 0.0), v2(10.0, 10.0)], vec![outside]).is_err());
    }

    #[test]
    fn offset_square_inward_and_outward() {
        let r = Region::rectangle(v2(0.0, 0.0), v2(20.0, 10.0)).unwrap();
        let inner = offset_region(&r, 2.0).unwrap();
        assert_eq!(inner.loops.len(), 1);
        assert!((inner.loops[0].signed_area() - 16.0 * 6.0).abs() < 1e-6);
        let outer = offset_region(&r, -2.0).unwrap();
        assert_eq!(outer.loops.len(), 1);
        // Rounded corners: 24 x 14 minus the four corner squares plus a full circle.
        let want = 20.0 * 10.0 + 2.0 * 2.0 * (20.0 + 10.0) + PI * 4.0;
        assert!((outer.loops[0].signed_area() - want).abs() < 1e-6, "{}", outer.loops[0].signed_area());
        assert!(outer.loops[0].bulges.iter().any(|b| b.abs() > 0.0));
        // Too large an inward offset leaves nothing.
        assert!(offset_region(&r, 6.0).unwrap().loops.is_empty());
    }

    #[test]
    fn arc_loop_reverse_keeps_area() {
        let r = Region::rectangle(v2(0.0, 0.0), v2(20.0, 10.0)).unwrap();
        let outer = offset_region(&r, -2.0).unwrap();
        let l = &outer.loops[0];
        assert!((l.reversed().signed_area() + l.signed_area()).abs() < 1e-9);
        let flat = l.flatten(0.001);
        assert!((signed_area(&flat) - l.signed_area()).abs() < 0.05);
    }
}
