//! Planar profiles: closed contours of lines, arcs and elliptic arcs in a 2D frame.
//!
//! [`build_regions`] is the gatekeeper between the sketcher and the solid
//! modeller. It rejects open, degenerate and self-intersecting input, sorts
//! nested contours into regions with holes, and normalises orientation so the
//! outer boundary is counter-clockwise and holes are clockwise.

use crate::{KernelError, Result, tol};
use glam::DVec2;
use serde::{Deserialize, Serialize};
use std::f64::consts::TAU;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Segment {
    Line {
        a: DVec2,
        b: DVec2,
    },
    /// Arc from angle `start` sweeping `sweep` radians (positive is
    /// counter-clockwise). `|sweep| == TAU` is a full circle.
    Arc {
        center: DVec2,
        radius: f64,
        start: f64,
        sweep: f64,
    },
    /// Elliptic arc: the point at parameter `t` is `center + u a cos t + v b sin t`, with
    /// `u` the unit vector at angle `rotation` (the `a` axis) and `v` square to it, from
    /// `t = start` sweeping `sweep` (positive is counter-clockwise). `|sweep| == TAU` is a
    /// whole ellipse.
    Ellipse {
        center: DVec2,
        a: f64,
        b: f64,
        rotation: f64,
        start: f64,
        sweep: f64,
    },
}

impl Segment {
    pub fn circle(center: DVec2, radius: f64) -> Self {
        Segment::Arc { center, radius, start: 0.0, sweep: TAU }
    }

    /// Arc around `center` from `from` to the ray through `to`, or `None` if the
    /// radius is degenerate. Coincident ends give a full circle.
    pub fn arc_from_center(center: DVec2, from: DVec2, to: DVec2, ccw: bool) -> Option<Self> {
        let radius = (from - center).length();
        if radius < tol::LINEAR {
            return None;
        }
        let start = (from - center).to_angle();
        let end = (to - center).to_angle();
        let mut sweep = (end - start).rem_euclid(TAU);
        if sweep < 1e-12 {
            sweep = TAU;
        }
        if !ccw {
            sweep -= TAU;
        }
        Some(Segment::Arc { center, radius, start, sweep })
    }

    /// A whole ellipse with semi-axes `a` along the direction at angle `rotation` and `b`
    /// square to it, run counter-clockwise from the end of its `a` axis.
    pub fn ellipse(center: DVec2, a: f64, b: f64, rotation: f64) -> Self {
        Segment::Ellipse { center, a, b, rotation, start: 0.0, sweep: TAU }
    }

    /// An elliptic arc's axes, scaled: `(u a, v b)`.
    fn ellipse_axes(a: f64, b: f64, rotation: f64) -> (DVec2, DVec2) {
        let u = DVec2::from_angle(rotation);
        (u * a, u.perp() * b)
    }

    /// The point at parameter `t` of an ellipse.
    fn ellipse_at(center: DVec2, a: f64, b: f64, rotation: f64, t: f64) -> DVec2 {
        let (ua, vb) = Self::ellipse_axes(a, b, rotation);
        center + ua * t.cos() + vb * t.sin()
    }

    pub fn start(&self) -> DVec2 {
        match *self {
            Segment::Line { a, .. } => a,
            Segment::Arc { center, radius, start, .. } => center + DVec2::from_angle(start) * radius,
            Segment::Ellipse { center, a, b, rotation, start, .. } => {
                Self::ellipse_at(center, a, b, rotation, start)
            }
        }
    }

    pub fn end(&self) -> DVec2 {
        match *self {
            Segment::Line { b, .. } => b,
            Segment::Arc { center, radius, start, sweep } => {
                center + DVec2::from_angle(start + sweep) * radius
            }
            Segment::Ellipse { center, a, b, rotation, start, sweep } => {
                Self::ellipse_at(center, a, b, rotation, start + sweep)
            }
        }
    }

    pub fn midpoint(&self) -> DVec2 {
        match *self {
            Segment::Line { a, b } => (a + b) * 0.5,
            Segment::Arc { center, radius, start, sweep } => {
                center + DVec2::from_angle(start + sweep * 0.5) * radius
            }
            Segment::Ellipse { center, a, b, rotation, start, sweep } => {
                Self::ellipse_at(center, a, b, rotation, start + sweep * 0.5)
            }
        }
    }

    pub fn length(&self) -> f64 {
        match *self {
            Segment::Line { a, b } => (b - a).length(),
            Segment::Arc { radius, sweep, .. } => radius * sweep.abs(),
            Segment::Ellipse { a, b, start, sweep, .. } => {
                // The speed is sqrt(a^2 sin^2 t + b^2 cos^2 t): Simpson's rule over enough
                // steps to be exact to well below the linear tolerance.
                let n = 512;
                let f = |t: f64| (a * a * t.sin().powi(2) + b * b * t.cos().powi(2)).sqrt();
                let h = sweep / n as f64;
                let mut sum = f(start) + f(start + sweep);
                for i in 1..n {
                    sum += f(start + h * i as f64) * if i % 2 == 1 { 4.0 } else { 2.0 };
                }
                (sum * h / 3.0).abs()
            }
        }
    }

    /// The unit direction of travel at the fraction `f` of the way along (by parameter).
    pub fn travel_at(&self, f: f64) -> DVec2 {
        match *self {
            Segment::Line { a, b } => (b - a).normalize_or_zero(),
            Segment::Arc { start, sweep, .. } => DVec2::from_angle(start + sweep * f).perp() * sweep.signum(),
            Segment::Ellipse { a, b, rotation, start, sweep, .. } => {
                let (ua, vb) = Self::ellipse_axes(a, b, rotation);
                let t = start + sweep * f;
                ((vb * t.cos() - ua * t.sin()) * sweep.signum()).normalize_or_zero()
            }
        }
    }

    pub fn is_full_circle(&self) -> bool {
        matches!(*self, Segment::Arc { sweep, .. } if sweep.abs() >= TAU - 1e-12)
    }

    /// A whole circle or a whole ellipse: a segment that closes on itself.
    pub fn is_closed(&self) -> bool {
        match *self {
            Segment::Line { .. } => false,
            Segment::Arc { sweep, .. } | Segment::Ellipse { sweep, .. } => sweep.abs() >= TAU - 1e-12,
        }
    }

    pub fn reversed(&self) -> Self {
        match *self {
            Segment::Line { a, b } => Segment::Line { a: b, b: a },
            Segment::Arc { center, radius, start, sweep } => {
                Segment::Arc { center, radius, start: start + sweep, sweep: -sweep }
            }
            Segment::Ellipse { center, a, b, rotation, start, sweep } => {
                Segment::Ellipse { center, a, b, rotation, start: start + sweep, sweep: -sweep }
            }
        }
    }

    fn is_finite(&self) -> bool {
        match *self {
            Segment::Line { a, b } => tol::is_finite2(a) && tol::is_finite2(b),
            Segment::Arc { center, radius, start, sweep } => {
                tol::is_finite2(center) && radius.is_finite() && start.is_finite() && sweep.is_finite()
            }
            Segment::Ellipse { center, a, b, rotation, start, sweep } => {
                tol::is_finite2(center) && [a, b, rotation, start, sweep].iter().all(|x| x.is_finite())
            }
        }
    }

    /// Twice the signed area contribution, i.e. the integral of `x dy - y dx`.
    fn area2(&self) -> f64 {
        match *self {
            Segment::Line { a, b } => a.perp_dot(b),
            Segment::Arc { center: c, radius: r, start: s, sweep: w } => {
                let e = s + w;
                r * r * w + r * (c.x * (e.sin() - s.sin()) - c.y * (e.cos() - s.cos()))
            }
            Segment::Ellipse { center: c, a, b, rotation, start: s, sweep: w } => {
                // P = c + A cos t + B sin t: P x P' = c x (B cos t - A sin t) + A x B.
                let (ua, vb) = Self::ellipse_axes(a, b, rotation);
                let e = s + w;
                c.perp_dot(ua) * (e.cos() - s.cos())
                    + c.perp_dot(vb) * (e.sin() - s.sin())
                    + ua.perp_dot(vb) * w
            }
        }
    }

    /// Points from start to end inclusive.
    pub fn sample(&self, chord: f64, max_angle: f64) -> Vec<DVec2> {
        match *self {
            Segment::Line { a, b } => vec![a, b],
            Segment::Arc { center, radius, start, sweep } => {
                let n = crate::geom::arc_steps(radius, sweep.abs(), chord, max_angle);
                (0..=n)
                    .map(|i| {
                        if i == n {
                            self.end()
                        } else {
                            let t = start + sweep * i as f64 / n as f64;
                            center + DVec2::from_angle(t) * radius
                        }
                    })
                    .collect()
            }
            Segment::Ellipse { center, a, b, rotation, start, sweep } => {
                // Even steps in the parameter, as many as the tightest bend (radius
                // b^2 / a at the ends of the long axis, travelled at speed b there... and at
                // speed a where it is flattest) needs: a circle of the short radius swept at
                // the long one's pace bounds both the chord and the turn.
                let (lo, hi) = (a.min(b), a.max(b));
                let n = crate::geom::arc_steps(lo * lo / hi, sweep.abs() * hi / lo, chord, max_angle);
                (0..=n)
                    .map(|i| {
                        if i == n {
                            self.end()
                        } else {
                            Self::ellipse_at(center, a, b, rotation, start + sweep * i as f64 / n as f64)
                        }
                    })
                    .collect()
            }
        }
    }

    fn angle_on_arc(&self, theta: f64) -> bool {
        match *self {
            Segment::Line { .. } | Segment::Ellipse { .. } => false,
            Segment::Arc { radius, start, sweep, .. } => {
                let eps = tol::LINEAR / radius.max(tol::LINEAR);
                let d = if sweep >= 0.0 {
                    (theta - start).rem_euclid(TAU)
                } else {
                    (start - theta).rem_euclid(TAU)
                };
                d <= sweep.abs() + eps || d >= TAU - eps
            }
        }
    }
}

/// A closed contour.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Loop2 {
    pub segments: Vec<Segment>,
    /// A caller-chosen number per segment (for example the sketch entity it came
    /// from). Faces swept from a segment carry its tag, which gives them a stable
    /// name across rebuilds. Defaults to the segment's index.
    #[serde(default)]
    pub tags: Vec<u32>,
}

impl Loop2 {
    pub fn new(segments: Vec<Segment>) -> Self {
        let tags = (0..segments.len() as u32).collect();
        Self { segments, tags }
    }

    /// A loop whose segments carry the given tags. Missing tags fall back to the
    /// segment index.
    pub fn with_tags(segments: Vec<Segment>, tags: Vec<u32>) -> Self {
        let tags = (0..segments.len()).map(|i| tags.get(i).copied().unwrap_or(i as u32)).collect();
        Self { segments, tags }
    }

    /// The tag of segment `i`.
    pub fn tag(&self, i: usize) -> u32 {
        self.tags.get(i).copied().unwrap_or(i as u32)
    }

    /// Positive when counter-clockwise.
    pub fn signed_area(&self) -> f64 {
        0.5 * self.segments.iter().map(Segment::area2).sum::<f64>()
    }

    pub fn reversed(&self) -> Self {
        let n = self.segments.len();
        Self {
            segments: self.segments.iter().rev().map(Segment::reversed).collect(),
            tags: (0..n).rev().map(|i| self.tag(i)).collect(),
        }
    }

    /// Closed polyline approximation (last point not repeated).
    pub fn polyline(&self, chord: f64, max_angle: f64) -> Vec<DVec2> {
        let mut pts = Vec::new();
        for s in &self.segments {
            let mut p = s.sample(chord, max_angle);
            p.pop();
            pts.extend(p);
        }
        pts
    }

    /// A point on the boundary that is not a vertex, for containment tests.
    fn probe_point(&self) -> DVec2 {
        self.segments.first().map_or(DVec2::ZERO, Segment::midpoint)
    }
}

/// A connected planar area: one counter-clockwise outer boundary and any
/// number of clockwise holes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Region {
    pub outer: Loop2,
    pub holes: Vec<Loop2>,
}

impl Region {
    pub fn area(&self) -> f64 {
        self.outer.signed_area() + self.holes.iter().map(Loop2::signed_area).sum::<f64>()
    }
}

/// Refuses loops with elliptic segments, for operations that take only lines and arcs
/// so far (`what` names the operation).
pub(crate) fn lines_and_arcs_only<'a>(loops: impl IntoIterator<Item = &'a Loop2>, what: &str) -> Result<()> {
    if loops.into_iter().flat_map(|l| &l.segments).any(|s| matches!(s, Segment::Ellipse { .. })) {
        return Err(KernelError::Unsupported(format!(
            "{what} a profile with an ellipse in it is not supported yet; only extruding one is"
        )));
    }
    Ok(())
}

/// Validates closed contours and groups them into regions.
pub fn build_regions(loops: &[Loop2]) -> Result<Vec<Region>> {
    if loops.is_empty() {
        return Err(KernelError::EmptyProfile);
    }
    for (li, l) in loops.iter().enumerate() {
        validate_loop(li, l)?;
    }
    check_intersections(loops)?;
    for (li, l) in loops.iter().enumerate() {
        if l.signed_area().abs() < tol::DEGENERATE {
            return Err(KernelError::DegenerateLoop(li));
        }
    }

    // Nesting depth: how many other contours contain this one.
    let polys: Vec<Vec<DVec2>> =
        loops.iter().map(|l| l.polyline(tol::CHORD * 0.1, tol::MAX_ANGLE * 0.25)).collect();
    let areas: Vec<f64> = loops.iter().map(|l| l.signed_area().abs()).collect();
    let n = loops.len();
    let mut containers: Vec<Vec<usize>> = vec![Vec::new(); n];
    for i in 0..n {
        let probe = loops[i].probe_point();
        for j in 0..n {
            if i != j && areas[j] > areas[i] && point_in_polygon(probe, &polys[j]) {
                containers[i].push(j);
            }
        }
    }

    let mut regions: Vec<Region> = Vec::new();
    let mut region_of: Vec<Option<usize>> = vec![None; n];
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by_key(|&i| containers[i].len());
    for i in order {
        let depth = containers[i].len();
        if depth.is_multiple_of(2) {
            let outer = if loops[i].signed_area() < 0.0 { loops[i].reversed() } else { loops[i].clone() };
            region_of[i] = Some(regions.len());
            regions.push(Region { outer, holes: Vec::new() });
        } else {
            // Parent is the smallest container, which has depth - 1.
            let parent = containers[i]
                .iter()
                .copied()
                .min_by(|&a, &b| areas[a].total_cmp(&areas[b]))
                .and_then(|p| region_of[p])
                .ok_or_else(|| KernelError::Topology("hole without a parent contour".into()))?;
            let hole = if loops[i].signed_area() > 0.0 { loops[i].reversed() } else { loops[i].clone() };
            regions[parent].holes.push(hole);
        }
    }
    Ok(regions)
}

fn validate_loop(li: usize, l: &Loop2) -> Result<()> {
    if l.segments.is_empty() {
        return Err(KernelError::DegenerateLoop(li));
    }
    for (si, s) in l.segments.iter().enumerate() {
        if !s.is_finite() {
            return Err(KernelError::NonFinite);
        }
        if s.length() < tol::LINEAR {
            return Err(KernelError::DegenerateSegment { contour: li, segment: si });
        }
        if let Segment::Arc { radius, .. } = s
            && *radius < tol::LINEAR
        {
            return Err(KernelError::DegenerateSegment { contour: li, segment: si });
        }
        if let Segment::Ellipse { a, b, .. } = s
            && a.min(*b) < tol::LINEAR
        {
            return Err(KernelError::DegenerateSegment { contour: li, segment: si });
        }
        let next = &l.segments[(si + 1) % l.segments.len()];
        let gap = (s.end() - next.start()).length();
        if gap > tol::LINEAR {
            return Err(KernelError::OpenLoop { contour: li, segment: si, gap });
        }
    }
    if l.segments.len() > 1 && l.segments.iter().any(Segment::is_closed) {
        return Err(KernelError::SelfIntersection { point: l.segments[0].start() });
    }
    Ok(())
}

/// Every pair of segments may only meet at the vertex they share as
/// neighbours in the same contour.
fn check_intersections(loops: &[Loop2]) -> Result<()> {
    let all: Vec<(usize, usize, &Segment)> = loops
        .iter()
        .enumerate()
        .flat_map(|(li, l)| l.segments.iter().enumerate().map(move |(si, s)| (li, si, s)))
        .collect();
    let bboxes: Vec<(DVec2, DVec2)> = all.iter().map(|(_, _, s)| bbox(s)).collect();
    for i in 0..all.len() {
        for j in (i + 1)..all.len() {
            let (bi, bj) = (bboxes[i], bboxes[j]);
            let pad = tol::LINEAR;
            if bi.1.x + pad < bj.0.x
                || bj.1.x + pad < bi.0.x
                || bi.1.y + pad < bj.0.y
                || bj.1.y + pad < bi.0.y
            {
                continue;
            }
            let (li, si, a) = all[i];
            let (lj, sj, b) = all[j];
            let mut allowed: Vec<DVec2> = Vec::new();
            if li == lj {
                let n = loops[li].segments.len();
                if (si + 1) % n == sj {
                    allowed.push(a.end());
                }
                if (sj + 1) % n == si {
                    allowed.push(b.end());
                }
            }
            for p in intersect(a, b) {
                let shared = allowed.iter().any(|v| (*v - p).length() < 1e-5);
                if !shared {
                    return Err(KernelError::SelfIntersection { point: p });
                }
            }
        }
    }
    Ok(())
}

fn bbox(s: &Segment) -> (DVec2, DVec2) {
    match *s {
        Segment::Line { a, b } => (a.min(b), a.max(b)),
        Segment::Arc { center, radius, .. } => (center - DVec2::splat(radius), center + DVec2::splat(radius)),
        Segment::Ellipse { center, a, b, rotation, .. } => {
            // The whole ellipse's box: half-widths sqrt((a ux)^2 + (b vx)^2) and so on.
            let (ua, vb) = Segment::ellipse_axes(a, b, rotation);
            let half = DVec2::new(ua.x.hypot(vb.x), ua.y.hypot(vb.y));
            (center - half, center + half)
        }
    }
}

/// Intersection points of two segments. Overlapping collinear or co-circular
/// pieces report the ends of the overlap, so any real overlap is visible to
/// the caller as a point that is not a shared vertex.
pub fn intersect(a: &Segment, b: &Segment) -> Vec<DVec2> {
    match (a, b) {
        (Segment::Ellipse { .. }, _) | (_, Segment::Ellipse { .. }) => with_ellipse(a, b),
        (Segment::Line { a: p0, b: p1 }, Segment::Line { a: q0, b: q1 }) => line_line(*p0, *p1, *q0, *q1),
        (Segment::Line { a: p0, b: p1 }, arc @ Segment::Arc { .. })
        | (arc @ Segment::Arc { .. }, Segment::Line { a: p0, b: p1 }) => line_arc(*p0, *p1, arc),
        (Segment::Arc { .. }, Segment::Arc { .. }) => arc_arc(a, b),
    }
}

/// Intersections where either segment is elliptic: each ellipse as a fine polyline (its
/// points within 1e-7 of the curve), crossed piece by piece with the other segment. This
/// serves to reject loops that cross, where the points need to be found to within the
/// tolerance at which a shared vertex is allowed, not exactly.
fn with_ellipse(a: &Segment, b: &Segment) -> Vec<DVec2> {
    let pieces = |s: &Segment| -> Vec<Segment> {
        match s {
            Segment::Ellipse { .. } => {
                s.sample(1e-7, 0.05).windows(2).map(|w| Segment::Line { a: w[0], b: w[1] }).collect()
            }
            other => vec![*other],
        }
    };
    let mut out: Vec<DVec2> = Vec::new();
    for pa in pieces(a) {
        for pb in pieces(b) {
            for p in intersect(&pa, &pb) {
                if !out.iter().any(|q| (*q - p).length() < 1e-6) {
                    out.push(p);
                }
            }
        }
    }
    out
}

fn line_line(p0: DVec2, p1: DVec2, q0: DVec2, q1: DVec2) -> Vec<DVec2> {
    let r = p1 - p0;
    let s = q1 - q0;
    let denom = r.perp_dot(s);
    let rl = r.length();
    let sl = s.length();
    if denom.abs() <= 1e-12 * rl * sl {
        // Parallel. Collinear if q0 lies on the line through p0, p1.
        if (q0 - p0).perp_dot(r).abs() / rl > tol::LINEAR {
            return Vec::new();
        }
        let rr = r.dot(r);
        let t0 = (q0 - p0).dot(r) / rr;
        let t1 = (q1 - p0).dot(r) / rr;
        let lo = t0.min(t1).max(0.0);
        let hi = t0.max(t1).min(1.0);
        let eps = tol::LINEAR / rl;
        if hi < lo - eps {
            return Vec::new();
        }
        if hi - lo <= eps {
            return vec![p0 + r * lo];
        }
        return vec![p0 + r * lo, p0 + r * hi, p0 + r * (0.5 * (lo + hi))];
    }
    let t = (q0 - p0).perp_dot(s) / denom;
    let u = (q0 - p0).perp_dot(r) / denom;
    let et = tol::LINEAR / rl;
    let eu = tol::LINEAR / sl;
    if t >= -et && t <= 1.0 + et && u >= -eu && u <= 1.0 + eu { vec![p0 + r * t] } else { Vec::new() }
}

fn line_arc(p0: DVec2, p1: DVec2, arc: &Segment) -> Vec<DVec2> {
    let Segment::Arc { center, radius, .. } = *arc else {
        return Vec::new();
    };
    let d = p1 - p0;
    let f = p0 - center;
    let a = d.dot(d);
    let b = 2.0 * f.dot(d);
    let c = f.dot(f) - radius * radius;
    let disc = b * b - 4.0 * a * c;
    let len = a.sqrt();
    // Distance from the centre to the infinite line, compared with the radius.
    let dist = (f.perp_dot(d) / len).abs();
    let roots: Vec<f64> = if dist > radius + tol::LINEAR {
        Vec::new()
    } else if disc <= 0.0 || (radius - dist) < tol::LINEAR {
        vec![-b / (2.0 * a)]
    } else {
        let sq = disc.sqrt();
        vec![(-b - sq) / (2.0 * a), (-b + sq) / (2.0 * a)]
    };
    let eps = tol::LINEAR / len;
    roots
        .into_iter()
        .filter(|t| *t >= -eps && *t <= 1.0 + eps)
        .map(|t| p0 + d * t)
        .filter(|p| arc.angle_on_arc((*p - center).to_angle()))
        .collect()
}

fn arc_arc(a: &Segment, b: &Segment) -> Vec<DVec2> {
    let (Segment::Arc { center: c0, radius: r0, .. }, Segment::Arc { center: c1, radius: r1, .. }) = (*a, *b)
    else {
        return Vec::new();
    };
    let dv = c1 - c0;
    let d = dv.length();
    if d < tol::LINEAR {
        if (r0 - r1).abs() > tol::LINEAR {
            return Vec::new();
        }
        // Same circle: report any endpoint of one arc that lies on the other,
        // plus a mid point if the arcs genuinely overlap.
        let mut pts = Vec::new();
        for (s, o) in [(a, b), (b, a)] {
            for p in [s.start(), s.end()] {
                if o.angle_on_arc((p - c0).to_angle()) {
                    pts.push(p);
                }
            }
            if let Segment::Arc { start, sweep, .. } = *s {
                let mid = start + sweep * 0.5;
                if o.angle_on_arc(mid) {
                    pts.push(c0 + DVec2::from_angle(mid) * r0);
                }
            }
        }
        return pts;
    }
    if d > r0 + r1 + tol::LINEAR || d < (r0 - r1).abs() - tol::LINEAR {
        return Vec::new();
    }
    let x = (d * d + r0 * r0 - r1 * r1) / (2.0 * d);
    let h2 = r0 * r0 - x * x;
    let u = dv / d;
    let base = c0 + u * x;
    let pts = if h2 <= tol::LINEAR * tol::LINEAR {
        vec![base]
    } else {
        let h = h2.sqrt();
        vec![base + u.perp() * h, base - u.perp() * h]
    };
    pts.into_iter()
        .filter(|p| a.angle_on_arc((*p - c0).to_angle()) && b.angle_on_arc((*p - c1).to_angle()))
        .collect()
}

/// Even-odd point in polygon.
pub fn point_in_polygon(p: DVec2, poly: &[DVec2]) -> bool {
    let mut inside = false;
    let n = poly.len();
    if n < 3 {
        return false;
    }
    let mut j = n - 1;
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Loop2 {
        let p = [DVec2::new(x0, y0), DVec2::new(x1, y0), DVec2::new(x1, y1), DVec2::new(x0, y1)];
        Loop2::new((0..4).map(|i| Segment::Line { a: p[i], b: p[(i + 1) % 4] }).collect())
    }

    #[test]
    fn rectangle_area_and_orientation() {
        let r = rect(0.0, 0.0, 4.0, 3.0);
        assert!((r.signed_area() - 12.0).abs() < 1e-12);
        assert!((r.reversed().signed_area() + 12.0).abs() < 1e-12);
    }

    #[test]
    fn circle_area() {
        let c = Loop2::new(vec![Segment::circle(DVec2::new(3.0, -1.0), 2.0)]);
        assert!((c.signed_area() - PI * 4.0).abs() < 1e-12);
    }

    #[test]
    fn ellipse_measures() {
        // Semi-axes 5 and 3, turned a quarter: area 15 pi, perimeter 20 E(16/25).
        let e = Segment::ellipse(DVec2::new(1.0, 2.0), 5.0, 3.0, PI / 2.0);
        assert!(e.is_closed() && !e.is_full_circle());
        assert!((e.length() - 25.526_998_863_398_127).abs() < 1e-9);
        assert!((e.start() - DVec2::new(1.0, 7.0)).length() < 1e-12);
        assert!((e.midpoint() - DVec2::new(1.0, -3.0)).length() < 1e-12);
        // Running counter-clockwise from the end of its long axis, it heads in -x.
        assert!((e.travel_at(0.0) - DVec2::new(-1.0, 0.0)).length() < 1e-12);
        let l = Loop2::new(vec![e]);
        assert!((l.signed_area() - 15.0 * PI).abs() < 1e-9);
        assert!((l.reversed().signed_area() + 15.0 * PI).abs() < 1e-9);
        // Half of it closed by its long axis: the D-shape of area 15 pi / 2.
        let half = Loop2::new(vec![
            Segment::Ellipse {
                center: DVec2::ZERO,
                a: 5.0,
                b: 3.0,
                rotation: 0.0,
                start: -PI / 2.0,
                sweep: PI,
            },
            Segment::Line { a: DVec2::new(0.0, 3.0), b: DVec2::new(0.0, -3.0) },
        ]);
        assert!((half.signed_area() - 7.5 * PI).abs() < 1e-9);
        // An elliptic hole in a plate, and one that runs out of it, crossing its side.
        let hole = Loop2::new(vec![Segment::ellipse(DVec2::new(5.0, 5.0), 4.0, 2.0, 0.3)]);
        let r = build_regions(&[rect(0.0, 0.0, 10.0, 10.0), hole]).expect("plate");
        assert!((r[0].area() - (100.0 - 8.0 * PI)).abs() < 1e-9);
        let out = Loop2::new(vec![Segment::ellipse(DVec2::new(9.0, 5.0), 4.0, 2.0, 0.0)]);
        assert!(build_regions(&[rect(0.0, 0.0, 10.0, 10.0), out]).is_err());
    }

    #[test]
    fn slot_area() {
        // Stadium: two semicircles of radius 1 joined by lines of length 4.
        let l = Loop2::new(vec![
            Segment::Line { a: DVec2::new(0.0, -1.0), b: DVec2::new(4.0, -1.0) },
            Segment::Arc { center: DVec2::new(4.0, 0.0), radius: 1.0, start: -PI / 2.0, sweep: PI },
            Segment::Line { a: DVec2::new(4.0, 1.0), b: DVec2::new(0.0, 1.0) },
            Segment::Arc { center: DVec2::new(0.0, 0.0), radius: 1.0, start: PI / 2.0, sweep: PI },
        ]);
        assert!((l.signed_area() - (8.0 + PI)).abs() < 1e-9);
        assert!(build_regions(&[l]).is_ok());
    }

    #[test]
    fn hole_detection_and_orientation() {
        let outer = rect(0.0, 0.0, 10.0, 10.0).reversed();
        let hole = Loop2::new(vec![Segment::circle(DVec2::new(5.0, 5.0), 2.0)]);
        let regions = build_regions(&[hole, outer]).expect("valid");
        assert_eq!(regions.len(), 1);
        assert!(regions[0].outer.signed_area() > 0.0);
        assert_eq!(regions[0].holes.len(), 1);
        assert!(regions[0].holes[0].signed_area() < 0.0);
        assert!((regions[0].area() - (100.0 - PI * 4.0)).abs() < 1e-9);
    }

    #[test]
    fn island_in_hole_is_its_own_region() {
        let regions =
            build_regions(&[rect(0.0, 0.0, 10.0, 10.0), rect(2.0, 2.0, 8.0, 8.0), rect(4.0, 4.0, 6.0, 6.0)])
                .expect("valid");
        assert_eq!(regions.len(), 2);
    }

    #[test]
    fn rejects_overlapping_rectangles() {
        let err = build_regions(&[rect(0.0, 0.0, 2.0, 2.0), rect(1.0, 1.0, 3.0, 3.0)]);
        assert!(matches!(err, Err(KernelError::SelfIntersection { .. })));
    }

    #[test]
    fn rejects_bowtie() {
        let p = [DVec2::new(0.0, 0.0), DVec2::new(2.0, 2.0), DVec2::new(2.0, 0.0), DVec2::new(0.0, 2.0)];
        let l = Loop2::new((0..4).map(|i| Segment::Line { a: p[i], b: p[(i + 1) % 4] }).collect());
        assert!(matches!(build_regions(&[l]), Err(KernelError::SelfIntersection { .. })));
    }

    #[test]
    fn rejects_open_loop() {
        let l = Loop2::new(vec![
            Segment::Line { a: DVec2::ZERO, b: DVec2::X },
            Segment::Line { a: DVec2::X, b: DVec2::Y },
        ]);
        assert!(matches!(build_regions(&[l]), Err(KernelError::OpenLoop { .. })));
    }

    #[test]
    fn rejects_circle_crossing_rectangle() {
        let c = Loop2::new(vec![Segment::circle(DVec2::new(0.0, 0.0), 1.0)]);
        assert!(build_regions(&[rect(0.0, 0.0, 5.0, 5.0), c]).is_err());
    }

    #[test]
    fn rejects_nan() {
        let l = Loop2::new(vec![Segment::circle(DVec2::new(f64::NAN, 0.0), 1.0)]);
        assert_eq!(build_regions(&[l]), Err(KernelError::NonFinite));
    }

    #[test]
    fn rejects_empty() {
        assert_eq!(build_regions(&[]), Err(KernelError::EmptyProfile));
    }
}
