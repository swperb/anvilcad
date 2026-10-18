//! Ruled lofts between two planar sections.
//!
//! Built from the specification in `docs/re/sweep-and-loft.md` (section "Loft", in
//! particular "Matching points between profiles: start points and connectors" and its
//! Decision, the loft testable statements 20 to 24 and 28, and "Recommended scope" item
//! 3). With two profiles and no end constraints each pair of corresponding points is
//! joined by a straight line, so every side face is a ruled surface.
//!
//! # Matching
//!
//! [`match_sections`] applies the spec's Decision:
//!
//! 1. Each closed section is one loop of lines and arcs (a circle is one closed
//!    segment). Both loops are oriented the same way: counter-clockwise (right-handed)
//!    about the loft direction, which runs from the first section's centroid to the
//!    second's.
//! 2. A loop starts at the vertex nearest its pick point. Without a pick, the second
//!    loop starts at the vertex nearest the first loop's start point projected onto its
//!    plane, and the first loop at the vertex with the smallest angle (counter-clockwise
//!    in its frame, from 0 to a full turn) from its frame's +X axis about its centroid.
//!    A single circle starts where the pick (or the projected start, or the +X ray from
//!    its centre) meets it radially. Ties go to the earlier vertex in loop order.
//! 3. Loops with the same number of segments match segment `i` to segment `i`, each
//!    parameterised by its own normalised arc length.
//! 4. Otherwise both loops are split at the union of their vertices' normalised
//!    arc-length parameters measured from the start point (a split piece keeps its
//!    parent's tag), and rule 3 applies.
//! 5. A point section matches every point of the loop.
//!
//! # Faces
//!
//! The start cap (first loop, [`FaceTag::StartCap`]) and end cap (second loop,
//! [`FaceTag::EndCap`]) are planes. Each matched segment pair gives one side face,
//! tagged `Side { contour: 0, segment }` with the first loop's segment tag (the second
//! loop's when the first section is a point). Side surfaces are exact where the ruled
//! surface is an analytic one:
//!
//! - two coplanar lines (or a line and the point): a plane;
//! - two arcs on one axis line whose matched points lie at the same angle about it: a
//!   cylinder or cone; an arc and a point on its axis: a pointed cone;
//! - two arcs of equal sweep: a degree (2, 1) rational B-spline patch whose rows are the
//!   arcs as exact rational quadratics (pieces of at most 90 degrees, equal weights, so
//!   equal parameters are equal fractions of each arc's length);
//! - two skew lines: a bilinear patch.
//!
//! A line or arc matched to an arc of another sweep is joined along equal fractions of
//! arc length, which no rational patch represents exactly: the arc is then written as a
//! quintic B-spline in its normalised arc length, built from the circle's exact position
//! and first two derivatives at steps of at most pi/32, which keeps it within about
//! 2e-11 of the radius of the true circle. The profile edges are always the loops' own
//! lines and circles. An arc lofted to a point off its axis would need a patch that
//! pinches to a point, which the kernel's parameter layout does not handle; that is
//! refused with [`KernelError::Unsupported`].
//!
//! The sections must lie strictly on either side of each other's planes, and the blend
//! of the two loops a quarter, half and three quarters of the way along (seen along the
//! loft direction) must be a simple loop turning the same way as the sections; a loft
//! that twists through itself is refused.

use crate::extrude::segment_curve;
use crate::geom::{Curve, Frame, Surface};
use crate::patch::SplineSurface;
use crate::profile::{Loop2, Segment, build_regions};
use crate::topo::{Coedge, Edge, EdgeId, Face, FaceTag, Loop, Solid, VertexId};
use crate::{KernelError, Result, check, tol};
use glam::{DVec2, DVec3};
use std::f64::consts::{FRAC_PI_2, PI, TAU};
use std::sync::Arc;

/// One section of a loft: a closed planar loop in `frame`'s xy plane, or a single point
/// (in world coordinates).
#[derive(Debug, Clone, PartialEq)]
pub enum Section {
    Loop { frame: Frame, outline: Loop2 },
    Point(DVec3),
}

impl Section {
    /// Number of segments of a loop; a point has none.
    pub fn segment_count(&self) -> usize {
        match self {
            Section::Loop { outline, .. } => outline.segments.len(),
            Section::Point(_) => 0,
        }
    }

    /// The world point at fraction `f` (0 to 1) of the arc length of segment `segment`.
    /// A point section returns its point for any segment. Out-of-range segments wrap.
    pub fn point_at(&self, segment: usize, f: f64) -> DVec3 {
        match self {
            Section::Loop { frame, outline } => {
                let n = outline.segments.len().max(1);
                match outline.segments.get(segment % n) {
                    Some(s) => frame.to_world(seg_point(s, f)),
                    None => frame.origin,
                }
            }
            Section::Point(p) => *p,
        }
    }
}

/// Fraction (of a normalised arc length) under which two parameters are the same.
const SAME_FRACTION: f64 = 1e-9;

/// Angles and unit directions closer than this are equal.
const ANGULAR: f64 = 1e-9;

/// Largest angle of one quintic span approximating an arc.
const QUINTIC_STEP: f64 = PI / 32.0;

/// A loop placed in space.
#[derive(Debug, Clone)]
struct Ring {
    frame: Frame,
    outline: Loop2,
    centroid: DVec3,
}

#[derive(Debug, Clone)]
enum Prep {
    Ring(Ring),
    Point(DVec3),
}

impl Prep {
    fn centroid(&self) -> DVec3 {
        match self {
            Prep::Ring(r) => r.centroid,
            Prep::Point(p) => *p,
        }
    }

    fn point_at(&self, i: usize, f: f64) -> DVec3 {
        match self {
            Prep::Ring(r) => r.frame.to_world(seg_point(&r.outline.segments[i], f)),
            Prep::Point(p) => *p,
        }
    }

    fn segment(&self, i: usize) -> Option<&Segment> {
        match self {
            Prep::Ring(r) => r.outline.segments.get(i),
            Prep::Point(_) => None,
        }
    }

    fn into_section(self) -> Section {
        match self {
            Prep::Ring(r) => Section::Loop { frame: r.frame, outline: r.outline },
            Prep::Point(p) => Section::Point(p),
        }
    }
}

/// Both sections, matched, with the unit loft direction and the segment count.
struct Matched {
    a: Prep,
    b: Prep,
    dir: DVec3,
    n: usize,
}

fn invalid<T>(msg: &str) -> Result<T> {
    Err(KernelError::InvalidInput(msg.into()))
}

fn refuse<T>(msg: &str) -> Result<T> {
    Err(KernelError::Unsupported(msg.into()))
}

/// The point at fraction `f` of a segment's length.
fn seg_point(s: &Segment, f: f64) -> DVec2 {
    match *s {
        Segment::Line { a, b } => {
            if f >= 1.0 {
                b
            } else {
                a + (b - a) * f
            }
        }
        Segment::Arc { center, radius, start, sweep } => {
            center + DVec2::from_angle(start + sweep * f) * radius
        }
    }
}

/// The piece of a segment between fractions `g0` and `g1` of its length.
fn seg_piece(s: &Segment, g0: f64, g1: f64) -> Segment {
    match *s {
        Segment::Line { .. } => Segment::Line { a: seg_point(s, g0), b: seg_point(s, g1) },
        Segment::Arc { center, radius, start, sweep } => {
            Segment::Arc { center, radius, start: start + sweep * g0, sweep: sweep * (g1 - g0) }
        }
    }
}

fn check_frame(f: &Frame) -> Result<()> {
    if ![f.origin, f.x, f.y, f.z].iter().all(|v| tol::is_finite3(*v)) {
        return Err(KernelError::NonFinite);
    }
    let unit = |v: DVec3| (v.length() - 1.0).abs() < 1e-9;
    if unit(f.x) && unit(f.y) && f.x.dot(f.y).abs() < 1e-9 && (f.x.cross(f.y) - f.z).length() < 1e-9 {
        Ok(())
    } else {
        Err(KernelError::InvalidFrame)
    }
}

/// The area centroid of a counter-clockwise loop, exactly, by Green's theorem.
fn centroid2(l: &Loop2) -> DVec2 {
    let (mut mx, mut my) = (0.0, 0.0);
    for s in &l.segments {
        match *s {
            Segment::Line { a, b } => {
                let d = b - a;
                mx += 0.5 * d.y * (a.x * a.x + a.x * d.x + d.x * d.x / 3.0);
                my -= 0.5 * d.x * (a.y * a.y + a.y * d.y + d.y * d.y / 3.0);
            }
            Segment::Arc { center: c, radius: r, start, sweep } => {
                let fx = |t: f64| {
                    r * (c.x * c.x * t.sin()
                        + 2.0 * c.x * r * (0.5 * t + 0.25 * (2.0 * t).sin())
                        + r * r * (t.sin() - t.sin().powi(3) / 3.0))
                };
                let fy = |t: f64| {
                    r * (-c.y * c.y * t.cos()
                        + 2.0 * c.y * r * (0.5 * t - 0.25 * (2.0 * t).sin())
                        + r * r * (-t.cos() + t.cos().powi(3) / 3.0))
                };
                let e = start + sweep;
                mx += 0.5 * (fx(e) - fx(start));
                my += 0.5 * (fy(e) - fy(start));
            }
        }
    }
    let a = l.signed_area();
    DVec2::new(mx / a, my / a)
}

fn prepare(s: &Section) -> Result<Prep> {
    match s {
        Section::Point(p) => {
            if !tol::is_finite3(*p) {
                return Err(KernelError::NonFinite);
            }
            Ok(Prep::Point(*p))
        }
        Section::Loop { frame, outline } => {
            check_frame(frame)?;
            let regions = build_regions(std::slice::from_ref(outline))?;
            let [region] = regions.as_slice() else {
                return invalid("a loft section must be one closed loop");
            };
            if !region.holes.is_empty() {
                return invalid("a loft section must be one closed loop");
            }
            let outline = region.outer.clone();
            let centroid = frame.to_world(centroid2(&outline));
            Ok(Prep::Ring(Ring { frame: *frame, outline, centroid }))
        }
    }
}

/// The loop rotated to start at segment `k`.
fn rotated(l: &Loop2, k: usize) -> Loop2 {
    let n = l.segments.len();
    Loop2::with_tags(
        (0..n).map(|i| l.segments[(i + k) % n]).collect(),
        (0..n).map(|i| l.tag((i + k) % n)).collect(),
    )
}

/// A single circle restarted at angle `theta`.
fn circle_from(l: &Loop2, theta: f64) -> Loop2 {
    match l.segments.first() {
        Some(&Segment::Arc { center, radius, sweep, .. }) => {
            Loop2::with_tags(vec![Segment::Arc { center, radius, start: theta, sweep }], vec![l.tag(0)])
        }
        _ => l.clone(),
    }
}

fn is_circle(l: &Loop2) -> bool {
    l.segments.len() == 1 && l.segments[0].is_full_circle()
}

/// Index of the smallest value, ties (within `slack`) going to the earliest.
fn first_min(values: &[f64], slack: f64) -> usize {
    let min = values.iter().copied().fold(f64::INFINITY, f64::min);
    values.iter().position(|v| *v <= min + slack).unwrap_or(0)
}

/// The ring started at the vertex nearest `target` (a world point).
fn start_nearest(r: &Ring, target: DVec3) -> Loop2 {
    if is_circle(&r.outline) {
        let l = r.frame.to_local(target);
        let theta = match r.outline.segments[0] {
            Segment::Arc { center, .. } => {
                let d = DVec2::new(l.x, l.y) - center;
                if d.length() < tol::LINEAR { 0.0 } else { d.to_angle() }
            }
            Segment::Line { .. } => 0.0,
        };
        return circle_from(&r.outline, theta);
    }
    let d: Vec<f64> =
        r.outline.segments.iter().map(|s| (r.frame.to_world(s.start()) - target).length()).collect();
    let scale = d.iter().copied().fold(1.0, f64::max);
    rotated(&r.outline, first_min(&d, 1e-12 * scale))
}

/// The ring started at the vertex at the smallest angle from its frame's +X axis
/// about its centroid.
fn start_first(r: &Ring) -> Loop2 {
    if is_circle(&r.outline) {
        return circle_from(&r.outline, 0.0);
    }
    let c = centroid2(&r.outline);
    let angles: Vec<f64> = r
        .outline
        .segments
        .iter()
        .map(|s| {
            let a = (s.start() - c).to_angle().rem_euclid(TAU);
            if a > TAU - ANGULAR { 0.0 } else { a }
        })
        .collect();
    rotated(&r.outline, first_min(&angles, ANGULAR))
}

/// Normalised arc-length parameters of a loop's vertices (the first is 0).
fn vertex_fractions(l: &Loop2) -> Vec<f64> {
    let total: f64 = l.segments.iter().map(Segment::length).sum();
    let mut acc = 0.0;
    l.segments
        .iter()
        .map(|s| {
            let f = acc / total;
            acc += s.length();
            f
        })
        .collect()
}

/// The loop split at every parameter of `cuts` that is not already a vertex.
fn split_loop(l: &Loop2, cuts: &[f64]) -> Loop2 {
    let total: f64 = l.segments.iter().map(Segment::length).sum();
    let n = l.segments.len();
    let (mut segs, mut tags) = (Vec::new(), Vec::new());
    let mut f0 = 0.0;
    for (k, s) in l.segments.iter().enumerate() {
        let f1 = if k + 1 == n { 1.0 } else { f0 + s.length() / total };
        let mut prev = 0.0;
        let inner = cuts
            .iter()
            .filter(|&&u| u > f0 + SAME_FRACTION && u < f1 - SAME_FRACTION)
            .map(|u| (u - f0) / (f1 - f0));
        for g in inner.chain(std::iter::once(1.0)) {
            segs.push(if prev == 0.0 && g == 1.0 { *s } else { seg_piece(s, prev, g) });
            tags.push(l.tag(k));
            prev = g;
        }
        f0 = f1;
    }
    Loop2::with_tags(segs, tags)
}

/// Points of a ring (world), finely enough spaced to bound its arcs.
fn ring_samples(r: &Ring) -> Vec<DVec3> {
    r.outline.polyline(0.0, 0.05).into_iter().map(|p| r.frame.to_world(p)).collect()
}

/// The ring's plane normal, turned to face along `dir`.
fn facing(r: &Ring, dir: DVec3) -> DVec3 {
    if r.frame.z.dot(dir) < 0.0 { -r.frame.z } else { r.frame.z }
}

fn match_inner(a: &Section, b: &Section, picks: [Option<DVec3>; 2]) -> Result<Matched> {
    if picks.iter().flatten().any(|p| !tol::is_finite3(*p)) {
        return Err(KernelError::NonFinite);
    }
    let (mut pa, mut pb) = (prepare(a)?, prepare(b)?);
    if matches!((&pa, &pb), (Prep::Point(_), Prep::Point(_))) {
        return invalid("a loft needs at least one closed section");
    }
    let dir = (pb.centroid() - pa.centroid())
        .try_normalize()
        .filter(|_| (pb.centroid() - pa.centroid()).length() > tol::LINEAR)
        .ok_or_else(|| KernelError::InvalidInput("the loft sections are not apart".into()))?;

    // Orient both loops counter-clockwise about the loft direction.
    for p in [&mut pa, &mut pb] {
        if let Prep::Ring(r) = p {
            let c = r.frame.z.dot(dir);
            if c.abs() < 1e-6 {
                return invalid("a loft section's plane runs along the loft");
            }
            if c < 0.0 {
                r.outline = r.outline.reversed();
            }
        }
    }

    // Each section must lie strictly beyond the other's plane.
    let samples = |p: &Prep| match p {
        Prep::Ring(r) => ring_samples(r),
        Prep::Point(q) => vec![*q],
    };
    if let Prep::Ring(r) = &pa {
        let n = facing(r, dir);
        if samples(&pb).iter().any(|q| (*q - r.frame.origin).dot(n) <= tol::LINEAR) {
            return invalid("the second loft section crosses the first section's plane");
        }
    }
    if let Prep::Ring(r) = &pb {
        let n = facing(r, dir);
        if samples(&pa).iter().any(|q| (*q - r.frame.origin).dot(n) >= -tol::LINEAR) {
            return invalid("the first loft section crosses the second section's plane");
        }
    }

    // Start points.
    if let Prep::Ring(r) = &mut pa {
        r.outline = match picks[0] {
            Some(p) => start_nearest(r, p),
            None => start_first(r),
        };
    }
    let a_start = match &pa {
        Prep::Ring(r) => r.frame.to_world(r.outline.segments[0].start()),
        Prep::Point(p) => *p,
    };
    let a_is_point = matches!(pa, Prep::Point(_));
    if let Prep::Ring(r) = &mut pb {
        r.outline = match picks[1] {
            Some(p) => start_nearest(r, p),
            None if a_is_point => start_first(r),
            None => {
                let n = r.frame.z;
                start_nearest(r, a_start - n * (a_start - r.frame.origin).dot(n))
            }
        };
    }

    // Equal segment counts.
    let n = match (&mut pa, &mut pb) {
        (Prep::Ring(ra), Prep::Ring(rb)) => {
            if ra.outline.segments.len() != rb.outline.segments.len() {
                let mut cuts: Vec<f64> =
                    vertex_fractions(&ra.outline).into_iter().chain(vertex_fractions(&rb.outline)).collect();
                cuts.sort_by(f64::total_cmp);
                cuts.dedup_by(|x, y| (*x - *y).abs() <= SAME_FRACTION);
                ra.outline = split_loop(&ra.outline, &cuts);
                rb.outline = split_loop(&rb.outline, &cuts);
                if ra.outline.segments.len() != rb.outline.segments.len() {
                    return refuse("the loft sections' vertices could not be matched");
                }
            }
            ra.outline.segments.len()
        }
        (Prep::Ring(r), Prep::Point(_)) | (Prep::Point(_), Prep::Ring(r)) => r.outline.segments.len(),
        (Prep::Point(_), Prep::Point(_)) => return invalid("a loft needs at least one closed section"),
    };
    Ok(Matched { a: pa, b: pb, dir, n })
}

/// Matches the points of two loft sections (see the module documentation): both loops
/// come back counter-clockwise about the loft direction, started at their matched start
/// points and split to the same number of segments, segment `i` of one corresponding to
/// segment `i` of the other along equal fractions of each one's arc length (see
/// [`Section::point_at`]). A point section comes back unchanged and corresponds to
/// every segment. A closed section must be one loop; at least one must be closed.
pub fn match_sections(a: &Section, b: &Section, picks: [Option<DVec3>; 2]) -> Result<[Section; 2]> {
    let m = match_inner(a, b, picks)?;
    Ok([m.a.into_section(), m.b.into_section()])
}

/// An arc placed in space.
#[derive(Debug, Clone, Copy)]
struct Arc3 {
    c: DVec3,
    x: DVec3,
    y: DVec3,
    z: DVec3,
    r: f64,
    start: f64,
    sweep: f64,
}

impl Arc3 {
    fn new(frame: &Frame, s: &Segment) -> Option<Arc3> {
        match *s {
            Segment::Arc { center, radius, start, sweep } => Some(Arc3 {
                c: frame.to_world(center),
                x: frame.x,
                y: frame.y,
                z: frame.z,
                r: radius,
                start,
                sweep,
            }),
            Segment::Line { .. } => None,
        }
    }

    fn radial(&self, t: f64) -> DVec3 {
        self.x * t.cos() + self.y * t.sin()
    }

    fn at(&self, t: f64) -> DVec3 {
        self.c + self.radial(t) * self.r
    }

    /// The sweep, signed about `dir`.
    fn sweep_about(&self, dir: DVec3) -> f64 {
        if self.z.dot(dir) < 0.0 { -self.sweep } else { self.sweep }
    }
}

/// Newell's normal of a polygon (unnormalised; its length is twice the area).
fn newell(pts: &[DVec3]) -> DVec3 {
    let n = pts.len();
    (0..n).fold(DVec3::ZERO, |acc, i| {
        let (p, q) = (pts[i], pts[(i + 1) % n]);
        acc + DVec3::new((p.y - q.y) * (p.z + q.z), (p.z - q.z) * (p.x + q.x), (p.x - q.x) * (p.y + q.y))
    })
}

/// The plane through a polygon given in face-loop order, if it is flat.
fn flat_plane(pts: &[DVec3]) -> Result<Option<Surface>> {
    let normal =
        newell(pts).try_normalize().ok_or_else(|| KernelError::Topology("a loft side has no area".into()))?;
    let o = pts[0];
    let size = pts.iter().map(|p| (*p - o).length()).fold(1.0, f64::max);
    if pts.iter().any(|p| (*p - o).dot(normal).abs() > 1e-9 * size) {
        return Ok(None);
    }
    let hint = pts
        .iter()
        .map(|p| *p - o)
        .find(|d| d.cross(normal).length() > 1e-9 * size)
        .unwrap_or(normal.any_orthonormal_vector());
    Ok(Some(Surface::Plane { frame: Frame::new(o, normal, hint)? }))
}

fn bezier_knots(degree: usize, spans: usize) -> Vec<f64> {
    let mut k = vec![0.0; degree + 1];
    for j in 1..spans {
        k.extend(std::iter::repeat_n(j as f64 / spans as f64, degree));
    }
    k.extend(vec![1.0; degree + 1]);
    k
}

/// A ruled patch from two rows of control points over the same knots: `s` along the
/// rows, `t` from row `a` (0) to row `b` (1).
fn ruled_patch(degree: usize, knots: Vec<f64>, a: &[DVec3], b: &[DVec3], weights: &[f64]) -> Result<Surface> {
    let m = a.len();
    let mut points = Vec::with_capacity(2 * m);
    let mut w = Vec::with_capacity(if weights.is_empty() { 0 } else { 2 * m });
    for i in 0..m {
        points.push(a[i]);
        points.push(b[i]);
        if let Some(wi) = weights.get(i) {
            w.push(*wi);
            w.push(*wi);
        }
    }
    let patch = SplineSurface::new((degree, 1), knots, vec![0.0, 0.0, 1.0, 1.0], (m, 2), points, w)?;
    Ok(Surface::BSpline(Arc::new(patch)))
}

/// Number of rational quadratic pieces for an arc (each at most a quarter turn).
fn quarter_pieces(sweep: f64) -> usize {
    ((sweep.abs() / FRAC_PI_2) - 1e-9).ceil().max(1.0) as usize
}

/// An arc as an exact rational quadratic in `k` equal pieces: points and weights.
fn rational_row(arc: &Arc3, k: usize) -> (Vec<DVec3>, Vec<f64>) {
    let phi = arc.sweep / k as f64;
    let w = (0.5 * phi).cos();
    let mut pts = vec![arc.at(arc.start)];
    let mut ws = vec![1.0];
    for j in 0..k {
        let t0 = arc.start + phi * j as f64;
        pts.push(arc.c + arc.radial(t0 + 0.5 * phi) * (arc.r / w));
        pts.push(if j + 1 == k { arc.at(arc.start + arc.sweep) } else { arc.at(t0 + phi) });
        ws.extend([w, 1.0]);
    }
    (pts, ws)
}

/// Spans of a quintic approximation of a segment by arc length.
fn quintic_spans(s: &Segment) -> usize {
    match *s {
        Segment::Line { .. } => 1,
        Segment::Arc { sweep, .. } => (sweep.abs() / QUINTIC_STEP).ceil().max(1.0) as usize,
    }
}

/// A segment as a piecewise quintic in its normalised arc length, `spans` Bezier
/// pieces: a line exactly, an arc from its exact position and first two derivatives
/// at each joint.
fn quintic_row(frame: &Frame, s: &Segment, spans: usize) -> Vec<DVec3> {
    match Arc3::new(frame, s) {
        None => {
            let (a, b) = (frame.to_world(s.start()), frame.to_world(s.end()));
            let m = 5 * spans;
            (0..=m).map(|i| if i == m { b } else { a + (b - a) * (i as f64 / m as f64) }).collect()
        }
        Some(arc) => {
            let d = arc.sweep / spans as f64;
            let jet = |t: f64| {
                let u = arc.radial(t);
                let du = arc.y * t.cos() - arc.x * t.sin();
                (arc.c + u * arc.r, du * (arc.r * d), -u * (arc.r * d * d))
            };
            let mut pts = vec![arc.at(arc.start)];
            for j in 0..spans {
                let t0 = arc.start + d * j as f64;
                let (p0, d0, s0) = jet(t0);
                let (p1, d1, s1) = jet(t0 + d);
                let p1 = if j + 1 == spans { arc.at(arc.start + arc.sweep) } else { p1 };
                pts.extend([
                    p0 + d0 / 5.0,
                    p0 + d0 * 0.4 + s0 / 20.0,
                    p1 - d1 * 0.4 + s1 / 20.0,
                    p1 - d1 / 5.0,
                    p1,
                ]);
            }
            pts
        }
    }
}

/// A cylinder or cone through two arcs on one axis line whose matched points lie at
/// the same angle about it, with whether the face is reversed.
fn coaxial(a: &Arc3, b: &Arc3, dir: DVec3) -> Option<(Surface, bool)> {
    let axis = if a.z.dot(dir) < 0.0 { -a.z } else { a.z };
    if a.z.cross(b.z).length() > ANGULAR {
        return None;
    }
    let off = b.c - a.c;
    let h = off.dot(axis);
    if (off - axis * h).length() > tol::LINEAR || h <= tol::LINEAR {
        return None;
    }
    let (ua, ub) = (a.radial(a.start), b.radial(b.start));
    let (sa, sb) = (a.sweep_about(dir), b.sweep_about(dir));
    if (ua - ub).length() > ANGULAR || (sa - sb).abs() > ANGULAR {
        return None;
    }
    let reversed = sa < 0.0;
    let frame_at = |o: DVec3, z: DVec3, x: DVec3| Frame { origin: o, x, y: z.cross(x), z };
    let surface = if (a.r - b.r).abs() <= tol::LINEAR * 1e-3 {
        Surface::Cylinder { frame: frame_at(a.c, axis, ua), radius: a.r }
    } else {
        let half_angle = ((b.r - a.r).abs() / h).atan();
        // Framed at the narrow end, opening along its z.
        let (frame, radius) =
            if b.r > a.r { (frame_at(a.c, axis, ua), a.r) } else { (frame_at(b.c, -axis, ub), b.r) };
        Surface::Cone { frame, radius, half_angle, apex: false }
    };
    Some((surface, reversed))
}

/// The surface of the side between matched segments of two loops, and whether the face
/// is reversed against it.
fn side_between(fa: &Frame, sa: &Segment, fb: &Frame, sb: &Segment, dir: DVec3) -> Result<(Surface, bool)> {
    match (Arc3::new(fa, sa), Arc3::new(fb, sb)) {
        (None, None) => {
            let (a0, a1) = (fa.to_world(sa.start()), fa.to_world(sa.end()));
            let (b0, b1) = (fb.to_world(sb.start()), fb.to_world(sb.end()));
            if let Some(p) = flat_plane(&[a0, a1, b1, b0])? {
                return Ok((p, false));
            }
            Ok((ruled_patch(1, vec![0.0, 0.0, 1.0, 1.0], &[a0, a1], &[b0, b1], &[])?, false))
        }
        (Some(arc_a), Some(arc_b)) => {
            if let Some(s) = coaxial(&arc_a, &arc_b, dir) {
                return Ok(s);
            }
            if (arc_a.sweep.abs() - arc_b.sweep.abs()).abs() <= ANGULAR {
                let k = quarter_pieces(arc_a.sweep);
                let (ra, w) = rational_row(&arc_a, k);
                let (rb, _) = rational_row(&arc_b, k);
                return Ok((ruled_patch(2, bezier_knots(2, k), &ra, &rb, &w)?, false));
            }
            quintic_side(fa, sa, fb, sb)
        }
        _ => quintic_side(fa, sa, fb, sb),
    }
}

fn quintic_side(fa: &Frame, sa: &Segment, fb: &Frame, sb: &Segment) -> Result<(Surface, bool)> {
    let spans = quintic_spans(sa).max(quintic_spans(sb));
    let (ra, rb) = (quintic_row(fa, sa, spans), quintic_row(fb, sb, spans));
    Ok((ruled_patch(5, bezier_knots(5, spans), &ra, &rb, &[])?, false))
}

/// The surface of the side between a loop's segment and the point `p`. `face` is the
/// side's corners in face-loop order (for a line).
fn side_to_point(f: &Frame, s: &Segment, p: DVec3, dir: DVec3, face: &[DVec3]) -> Result<(Surface, bool)> {
    let Some(arc) = Arc3::new(f, s) else {
        return match flat_plane(face)? {
            Some(plane) => Ok((plane, false)),
            None => Err(KernelError::Topology("a loft side to a point is not flat".into())),
        };
    };
    let to_centre = arc.c - p;
    let h = to_centre.length();
    if to_centre.cross(arc.z).length() > tol::LINEAR || h <= tol::LINEAR {
        return refuse("lofting an arc to a point off its axis");
    }
    let z = to_centre / h;
    let frame = Frame::new(arc.c, z, arc.x)?;
    let surface = Surface::Cone { frame, radius: arc.r, half_angle: (arc.r / h).atan(), apex: true };
    Ok((surface, arc.sweep_about(dir) < 0.0))
}

/// Proper crossing of segments `p0 p1` and `q0 q1` in the plane.
fn crosses(p0: DVec2, p1: DVec2, q0: DVec2, q1: DVec2, eps: f64) -> bool {
    let o = |a: DVec2, b: DVec2, c: DVec2| (b - a).perp_dot(c - a);
    let (d1, d2) = (o(p0, p1, q0), o(p0, p1, q1));
    let (d3, d4) = (o(q0, q1, p0), o(q0, q1, p1));
    let clear = |x: f64| x.abs() > eps;
    clear(d1) && clear(d2) && clear(d3) && clear(d4) && d1 * d2 < 0.0 && d3 * d4 < 0.0
}

/// Refuses a loft whose sections, blended a quarter, half and three quarters of the way,
/// do not stay simple loops turning counter-clockwise about the loft direction.
fn check_blend(m: &Matched) -> Result<()> {
    let u = m.dir.any_orthonormal_vector();
    let v = m.dir.cross(u);
    let samples: Vec<usize> = (0..m.n)
        .map(|i| {
            let sweep = [m.a.segment(i), m.b.segment(i)]
                .into_iter()
                .flatten()
                .map(|s| match *s {
                    Segment::Arc { sweep, .. } => sweep.abs(),
                    Segment::Line { .. } => 0.0,
                })
                .fold(0.0, f64::max);
            if sweep == 0.0 { 1 } else { ((sweep / 0.15).ceil() as usize).max(4) }
        })
        .collect();
    for s in [0.25, 0.5, 0.75] {
        let mut poly = Vec::new();
        for (i, &k) in samples.iter().enumerate() {
            for j in 0..k {
                let f = j as f64 / k as f64;
                let p = m.a.point_at(i, f) * (1.0 - s) + m.b.point_at(i, f) * s;
                poly.push(DVec2::new(p.dot(u), p.dot(v)));
            }
        }
        let n = poly.len();
        let area: f64 = (0..n).map(|i| poly[i].perp_dot(poly[(i + 1) % n])).sum::<f64>() * 0.5;
        let size = poly.iter().map(|p| (*p - poly[0]).length()).fold(1.0, f64::max);
        if area <= 1e-9 * size * size {
            return invalid("the loft twists through itself");
        }
        let eps = 1e-12 * size * size;
        for i in 0..n {
            for j in (i + 2)..n {
                if i == 0 && j == n - 1 {
                    continue;
                }
                if crosses(poly[i], poly[(i + 1) % n], poly[j], poly[(j + 1) % n], eps) {
                    return invalid("the loft twists through itself");
                }
            }
        }
    }
    Ok(())
}

/// A ruled loft between two sections, at least one a closed loop, matched as
/// [`match_sections`] does. `picks` optionally gives each loop's pick point (world),
/// near which its start vertex is chosen. The returned solid has passed
/// [`check::validate`].
pub fn loft_ruled(a: &Section, b: &Section, picks: [Option<DVec3>; 2]) -> Result<Solid> {
    let m = match_inner(a, b, picks)?;
    check_blend(&m)?;
    let n = m.n;
    let mut solid = Solid::default();
    let fwd = |edge| Coedge { edge, reversed: false };
    let rev = |edge| Coedge { edge, reversed: true };

    // A circle lofted to a point has no seam: the side is a pointed cone bounded by the
    // circle alone.
    let pointed = n == 1 && (matches!(m.a, Prep::Point(_)) || matches!(m.b, Prep::Point(_)));

    let vertices = |p: &Prep, solid: &mut Solid| -> Vec<VertexId> {
        match p {
            Prep::Ring(r) => {
                r.outline.segments.iter().map(|s| solid.add_vertex(r.frame.to_world(s.start()))).collect()
            }
            Prep::Point(q) if !pointed => vec![solid.add_vertex(*q)],
            Prep::Point(_) => Vec::new(),
        }
    };
    let va = vertices(&m.a, &mut solid);
    let vb = vertices(&m.b, &mut solid);
    let profile = |p: &Prep, vs: &[VertexId], solid: &mut Solid| -> Vec<EdgeId> {
        match p {
            Prep::Ring(r) => r
                .outline
                .segments
                .iter()
                .enumerate()
                .map(|(k, s)| {
                    let (curve, t0, t1) = segment_curve(&r.frame, s);
                    solid.add_edge(Edge { curve, start: vs[k], end: vs[(k + 1) % n], t0, t1 })
                })
                .collect(),
            Prep::Point(_) => Vec::new(),
        }
    };
    let ea = profile(&m.a, &va, &mut solid);
    let eb = profile(&m.b, &vb, &mut solid);
    let mut rulings = Vec::new();
    if !pointed {
        for i in 0..n {
            let (s, e) = (va[i % va.len()], vb[i % vb.len()]);
            let (ps, pe) = (solid.vertices[s.index()].point, solid.vertices[e.index()].point);
            let len = (pe - ps).length();
            if len <= tol::LINEAR {
                return invalid("matched loft points coincide");
            }
            rulings.push(solid.add_edge(Edge {
                curve: Curve::Line { origin: ps, dir: (pe - ps) / len },
                start: s,
                end: e,
                t0: 0.0,
                t1: len,
            }));
        }
    }

    // Caps.
    if let Prep::Ring(r) = &m.a {
        solid.add_face(Face {
            surface: Surface::Plane { frame: r.frame },
            reversed: r.frame.z.dot(m.dir) > 0.0,
            outer: Loop { coedges: ea.iter().rev().copied().map(rev).collect() },
            inners: Vec::new(),
            tag: FaceTag::StartCap,
            owner: 0,
        });
    }
    if let Prep::Ring(r) = &m.b {
        solid.add_face(Face {
            surface: Surface::Plane { frame: r.frame },
            reversed: r.frame.z.dot(m.dir) < 0.0,
            outer: Loop { coedges: eb.iter().copied().map(fwd).collect() },
            inners: Vec::new(),
            tag: FaceTag::EndCap,
            owner: 0,
        });
    }

    // Sides.
    let tags = match (&m.a, &m.b) {
        (Prep::Ring(r), _) | (Prep::Point(_), Prep::Ring(r)) => r.outline.clone(),
        (Prep::Point(_), Prep::Point(_)) => return invalid("a loft needs at least one closed section"),
    };
    for i in 0..n {
        let next = (i + 1) % n;
        let (surface, reversed) = match (&m.a, &m.b) {
            (Prep::Ring(ra), Prep::Ring(rb)) => {
                side_between(&ra.frame, &ra.outline.segments[i], &rb.frame, &rb.outline.segments[i], m.dir)?
            }
            (Prep::Ring(r), Prep::Point(p)) => {
                let face = [m.a.point_at(i, 0.0), m.a.point_at(i, 1.0), *p];
                side_to_point(&r.frame, &r.outline.segments[i], *p, m.dir, &face)?
            }
            (Prep::Point(p), Prep::Ring(r)) => {
                let face = [*p, m.b.point_at(i, 1.0), m.b.point_at(i, 0.0)];
                side_to_point(&r.frame, &r.outline.segments[i], *p, m.dir, &face)?
            }
            (Prep::Point(_), Prep::Point(_)) => return invalid("a loft needs at least one closed section"),
        };
        let mut coedges = Vec::with_capacity(4);
        if let Some(e) = ea.get(i) {
            coedges.push(fwd(*e));
        }
        if let Some(r) = rulings.get(next) {
            coedges.push(fwd(*r));
        }
        if let Some(e) = eb.get(i) {
            coedges.push(rev(*e));
        }
        if let Some(r) = rulings.get(i) {
            coedges.push(rev(*r));
        }
        solid.add_face(Face {
            surface,
            reversed,
            outer: Loop { coedges },
            inners: Vec::new(),
            tag: FaceTag::Side { contour: 0, segment: tags.tag(i) },
            owner: 0,
        });
    }
    check::validate(&solid)?;
    Ok(solid)
}

#[cfg(test)]
mod tests;
