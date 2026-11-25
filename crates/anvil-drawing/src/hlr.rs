//! Hidden-line projection of exact solids onto a view plane (see `docs/re/drawings.md`,
//! "Views" and "AnvilCAD decisions").
//!
//! A view shows three kinds of curve:
//!
//! - the solid's edges, each classed as a sharp edge, a tangent edge (between faces that
//!   meet smoothly) or a seam (a face meeting itself, or two pieces of one surface), which
//!   is never drawn;
//! - the silhouettes of curved faces, where the face turns away from the viewer: exact
//!   lines on cylinders and cones, an exact circle on spheres, curves traced on the exact
//!   surface for tori, and curves found on the display mesh and moved onto the surface
//!   for freeform faces;
//!
//! Every curve is cut where it passes behind a face: points along it are tested with a
//! ray towards the viewer against the exact faces of every body, and each change between
//! visible and hidden is found by bisection. The visible and hidden pieces are then
//! projected exactly (a line stays a line, a circle becomes a circle, an ellipse or a
//! line), and pieces lying on a piece already drawn are dropped, so a hidden edge behind
//! a visible one is not drawn twice.
//!
//! The work runs on all cores and reports progress through [`Progress`], which can also
//! cancel it.

use anvil_brep::boolean::FaceLocator;
use anvil_brep::{Curve, Frame, Solid, Surface};
use glam::{DVec2, DVec3};
use serde::{Deserialize, Serialize};
use std::f64::consts::{PI, TAU};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// The direction a view looks from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Orientation {
    /// Looking at the Front plane (XY) from +Z, Y up.
    Front,
    /// Looking down from +Y, X to the right.
    Top,
    /// Looking from +X, Y up.
    Right,
    /// Looking from -X, Y up.
    Left,
    /// Looking from (1, 1, 1), Y up.
    Isometric,
    /// Looking up from -Y, X to the right (section views only).
    Bottom,
    /// Looking at the back from -Z, Y up (section views only).
    Back,
}

impl Orientation {
    pub const ALL: [Orientation; 5] =
        [Orientation::Front, Orientation::Top, Orientation::Right, Orientation::Left, Orientation::Isometric];

    pub fn label(self) -> &'static str {
        match self {
            Orientation::Front => "Front",
            Orientation::Top => "Top",
            Orientation::Right => "Right",
            Orientation::Left => "Left",
            Orientation::Isometric => "Isometric",
            Orientation::Bottom => "Bottom",
            Orientation::Back => "Back",
        }
    }

    /// The orthographic orientation whose eye is `eye`, when it is one of the six axis
    /// directions.
    pub fn from_eye(eye: DVec3) -> Option<Orientation> {
        [
            Orientation::Front,
            Orientation::Back,
            Orientation::Top,
            Orientation::Bottom,
            Orientation::Right,
            Orientation::Left,
        ]
        .into_iter()
        .find(|o| o.axes().eye.dot(eye) > 1.0 - 1e-9)
    }

    /// True for the isometric, which is foreshortened.
    pub fn is_pictorial(self) -> bool {
        self == Orientation::Isometric
    }

    pub fn axes(self) -> ViewAxes {
        let (eye, up) = match self {
            Orientation::Front => (DVec3::Z, DVec3::Y),
            Orientation::Top => (DVec3::Y, -DVec3::Z),
            Orientation::Right => (DVec3::X, DVec3::Y),
            Orientation::Left => (-DVec3::X, DVec3::Y),
            Orientation::Isometric => (DVec3::ONE.normalize(), DVec3::Y),
            Orientation::Bottom => (-DVec3::Y, DVec3::Z),
            Orientation::Back => (-DVec3::Z, DVec3::Y),
        };
        let up = (up - eye * up.dot(eye)).normalize();
        ViewAxes { right: up.cross(eye), up, eye }
    }
}

/// A view's projection: model points map to `(p . right, p . up)` on the view plane, and
/// `eye` points from the model towards the viewer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewAxes {
    pub right: DVec3,
    pub up: DVec3,
    pub eye: DVec3,
}

impl ViewAxes {
    pub fn project(&self, p: DVec3) -> DVec2 {
        DVec2::new(p.dot(self.right), p.dot(self.up))
    }

    fn project_dir(&self, d: DVec3) -> DVec2 {
        self.project(d)
    }

    /// The model point on the view plane (depth 0) that projects to `p`.
    pub fn lift(&self, p: DVec2) -> DVec3 {
        self.right * p.x + self.up * p.y
    }
}

/// How a line of a view is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LineKind {
    /// A visible edge or silhouette.
    Visible,
    /// A visible tangent edge.
    Tangent,
    /// A hidden edge or silhouette.
    Hidden,
    /// A hidden tangent edge.
    HiddenTangent,
}

/// A curve on the view plane, in model millimetres.
#[derive(Debug, Clone, PartialEq)]
pub enum Path2 {
    Line {
        a: DVec2,
        b: DVec2,
    },
    /// `c + u cos t + v sin t` for `t` from `t0` to `t1`: a circle when `u` and `v` are
    /// perpendicular and of equal length, otherwise an ellipse.
    Conic {
        c: DVec2,
        u: DVec2,
        v: DVec2,
        t0: f64,
        t1: f64,
    },
    Poly(Vec<DVec2>),
}

impl Path2 {
    pub fn eval(&self, s: f64) -> DVec2 {
        match self {
            Path2::Line { a, b } => *a + (*b - *a) * s,
            Path2::Conic { c, u, v, .. } => *c + *u * s.cos() + *v * s.sin(),
            Path2::Poly(p) => poly_eval(p, s),
        }
    }

    pub(crate) fn range(&self) -> (f64, f64) {
        match self {
            Path2::Line { .. } => (0.0, 1.0),
            Path2::Conic { t0, t1, .. } => (*t0, *t1),
            Path2::Poly(p) => (0.0, p.len().saturating_sub(1) as f64),
        }
    }

    pub(crate) fn sub(&self, s0: f64, s1: f64) -> Path2 {
        match self {
            Path2::Line { .. } => Path2::Line { a: self.eval(s0), b: self.eval(s1) },
            Path2::Conic { c, u, v, .. } => Path2::Conic { c: *c, u: *u, v: *v, t0: s0, t1: s1 },
            Path2::Poly(p) => Path2::Poly(poly_sub(p, s0, s1)),
        }
    }

    /// Points along the path, at most `tol` from it (as chords of an arc).
    pub fn points(&self, tol: f64) -> Vec<DVec2> {
        match self {
            Path2::Line { a, b } => vec![*a, *b],
            Path2::Conic { u, v, t0, t1, .. } => {
                let r = u.length().max(v.length());
                let step = if r > tol { 2.0 * (1.0 - tol / r).clamp(-1.0, 1.0).acos() } else { PI / 2.0 };
                let n = (((t1 - t0).abs() / step.max(1e-3)).ceil() as usize).clamp(2, 2048);
                (0..=n).map(|i| self.eval(t0 + (t1 - t0) * i as f64 / n as f64)).collect()
            }
            Path2::Poly(p) => p.clone(),
        }
    }

    fn length_estimate(&self) -> f64 {
        let pts = match self {
            Path2::Conic { t0, t1, .. } => {
                (0..=32).map(|i| self.eval(t0 + (t1 - t0) * i as f64 / 32.0)).collect()
            }
            _ => self.points(1.0),
        };
        pts.windows(2).map(|w| (w[1] - w[0]).length()).sum()
    }

    pub(crate) fn bounds(&self) -> (DVec2, DVec2) {
        let pts = match self {
            Path2::Conic { t0, t1, .. } => {
                (0..=64).map(|i| self.eval(t0 + (t1 - t0) * i as f64 / 64.0)).collect()
            }
            _ => self.points(1.0),
        };
        let mut lo = DVec2::splat(f64::INFINITY);
        let mut hi = DVec2::splat(f64::NEG_INFINITY);
        for p in pts {
            lo = lo.min(p);
            hi = hi.max(p);
        }
        // Sampled conics can fall just short of their extremes.
        if let Path2::Conic { u, v, .. } = self {
            let pad = (u.length() + v.length()) * (1.0 - (PI / 64.0).cos());
            lo -= DVec2::splat(pad);
            hi += DVec2::splat(pad);
        }
        (lo, hi)
    }

    /// Distance from `p` to the path.
    fn distance(&self, p: DVec2) -> f64 {
        match self {
            Path2::Line { a, b } => segment_distance(p, *a, *b),
            Path2::Poly(pts) => {
                pts.windows(2).map(|w| segment_distance(p, w[0], w[1])).fold(f64::INFINITY, f64::min)
            }
            Path2::Conic { t0, t1, .. } => {
                const N: usize = 96;
                let (lo, hi) = (t0.min(*t1), t0.max(*t1));
                let h = (hi - lo) / N as f64;
                let d = |t: f64| (self.eval(t) - p).length_squared();
                let best = (0..=N).map(|i| lo + h * i as f64).min_by(|a, b| d(*a).total_cmp(&d(*b)));
                let Some(best) = best else { return f64::INFINITY };
                // Golden-section search round the nearest sample.
                let (mut a, mut b) = ((best - h).max(lo), (best + h).min(hi));
                let g = 0.5 * (5f64.sqrt() - 1.0);
                for _ in 0..60 {
                    let (x1, x2) = (b - g * (b - a), a + g * (b - a));
                    if d(x1) < d(x2) {
                        b = x2;
                    } else {
                        a = x1;
                    }
                }
                d(0.5 * (a + b)).min(d(best)).sqrt()
            }
        }
    }
}

fn segment_distance(p: DVec2, a: DVec2, b: DVec2) -> f64 {
    let ab = b - a;
    let l2 = ab.length_squared();
    let t = if l2 > 0.0 { ((p - a).dot(ab) / l2).clamp(0.0, 1.0) } else { 0.0 };
    (a + ab * t - p).length()
}

fn poly_eval<T>(p: &[T], s: f64) -> T
where
    T: Copy + std::ops::Add<Output = T> + std::ops::Sub<Output = T> + std::ops::Mul<f64, Output = T>,
{
    let n = p.len();
    if n == 1 {
        return p[0];
    }
    let s = s.clamp(0.0, (n - 1) as f64);
    let i = (s.floor() as usize).min(n - 2);
    let f = s - i as f64;
    p[i] + (p[i + 1] - p[i]) * f
}

fn poly_sub<T>(p: &[T], s0: f64, s1: f64) -> Vec<T>
where
    T: Copy + std::ops::Add<Output = T> + std::ops::Sub<Output = T> + std::ops::Mul<f64, Output = T>,
{
    let (a, b) = (s0.min(s1), s0.max(s1));
    let mut out = vec![poly_eval(p, a)];
    let mut k = a.floor() + 1.0;
    while k < b {
        out.push(poly_eval(p, k));
        k += 1.0;
    }
    out.push(poly_eval(p, b));
    out
}

/// One line of a view.
#[derive(Debug, Clone, PartialEq)]
pub struct ViewLine {
    pub kind: LineKind,
    pub path: Path2,
}

/// A cut face of a section view to be hatched: its boundary loops on the view plane
/// (outer and holes alike; hatching fills by the even-odd rule), the body it was cut from
/// and its material's pattern.
#[derive(Debug, Clone, PartialEq)]
pub struct HatchArea {
    pub loops: Vec<Vec<DVec2>>,
    pub body: usize,
    pub pattern: crate::section::HatchPattern,
}

/// The lines of one view, in model millimetres on the view plane.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ViewGeometry {
    pub lines: Vec<ViewLine>,
    /// Cut faces to hatch (section views, and detail views of them).
    pub hatch: Vec<HatchArea>,
    /// The circle a detail view is clipped to: centre and radius on the view plane.
    pub clip: Option<(DVec2, f64)>,
}

impl ViewGeometry {
    /// The box round every line, visible or not, or `None` for an empty view. A detail
    /// view's box is its circle's.
    pub fn bounds(&self) -> Option<(DVec2, DVec2)> {
        if let Some((c, r)) = self.clip {
            return Some((c - DVec2::splat(r), c + DVec2::splat(r)));
        }
        let mut out: Option<(DVec2, DVec2)> = None;
        for l in &self.lines {
            let (a, b) = l.path.bounds();
            out = Some(match out {
                Some((lo, hi)) => (lo.min(a), hi.max(b)),
                None => (a, b),
            });
        }
        out
    }
}

/// Progress of a projection, shared with the thread that shows it. Work made of several
/// projections (a drawing's views) is split into stages, one per projection.
#[derive(Debug, Default)]
pub struct Progress {
    done: AtomicUsize,
    total: AtomicUsize,
    stage: AtomicUsize,
    stages: AtomicUsize,
    cancel: AtomicBool,
}

impl Progress {
    pub fn new() -> Progress {
        Progress::default()
    }

    /// Curves finished and curves in all, in the current stage.
    pub fn counts(&self) -> (usize, usize) {
        (self.done.load(Ordering::Relaxed), self.total.load(Ordering::Relaxed))
    }

    /// Splits the work into `n` stages of equal weight.
    pub fn set_stages(&self, n: usize) {
        self.stages.store(n, Ordering::Relaxed);
        self.stage.store(0, Ordering::Relaxed);
    }

    /// Starts stage `i` (from 0).
    pub fn begin_stage(&self, i: usize) {
        self.done.store(0, Ordering::Relaxed);
        self.total.store(0, Ordering::Relaxed);
        self.stage.store(i, Ordering::Relaxed);
    }

    /// How much of the work is done, from 0 to 1, or `None` before any projecting has
    /// started (while the part is still being loaded).
    pub fn fraction(&self) -> Option<f32> {
        let (done, total) = self.counts();
        let stages = self.stages.load(Ordering::Relaxed).max(1);
        let stage = self.stage.load(Ordering::Relaxed);
        if total == 0 && stage == 0 {
            return None;
        }
        let within = if total == 0 { 0.0 } else { done as f32 / total as f32 };
        Some(((stage as f32 + within) / stages as f32).clamp(0.0, 1.0))
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    pub fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    fn add_total(&self, n: usize) {
        self.total.fetch_add(n, Ordering::Relaxed);
    }

    fn step(&self) {
        self.done.fetch_add(1, Ordering::Relaxed);
    }

    #[cfg(test)]
    pub(crate) fn add_total_for_test(&self, n: usize) {
        self.add_total(n);
    }

    #[cfg(test)]
    pub(crate) fn step_for_test(&self) {
        self.step();
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectError {
    Cancelled,
}

impl std::fmt::Display for ProjectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProjectError::Cancelled => write!(f, "the view update was cancelled"),
        }
    }
}

impl std::error::Error for ProjectError {}

/// How a model edge is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EdgeClass {
    Sharp,
    Tangent,
    Seam,
}

/// Faces meeting at less than this angle make a tangent edge (`docs/re/drawings.md`,
/// "Tangent edges": under one degree).
const TANGENT_ANGLE: f64 = PI / 180.0;

fn classify_edge(solid: &Solid, e: usize) -> EdgeClass {
    let Some(edge) = solid.edges.get(e) else { return EdgeClass::Sharp };
    let id = anvil_brep::EdgeId(e as u32);
    let faces = solid.edge_faces(id);
    let uses = |f: &anvil_brep::topo::Face| {
        f.loops().flat_map(|l| l.coedges.iter()).filter(|c| c.edge == id).count()
    };
    match faces.as_slice() {
        [f] => match solid.face(*f) {
            Some(face) if uses(face) >= 2 => EdgeClass::Seam,
            _ => EdgeClass::Sharp,
        },
        [a, b] => {
            let (Some(fa), Some(fb)) = (solid.face(*a), solid.face(*b)) else { return EdgeClass::Sharp };
            if fa.surface.coincident(&fb.surface, 1e-7) {
                return EdgeClass::Seam;
            }
            let m = edge.curve.eval(0.5 * (edge.t0 + edge.t1));
            let (na, nb) = (fa.normal_at(m), fb.normal_at(m));
            if na.dot(nb) >= TANGENT_ANGLE.cos() { EdgeClass::Tangent } else { EdgeClass::Sharp }
        }
        _ => EdgeClass::Sharp,
    }
}

/// A 3D curve to be cut into visible and hidden pieces.
enum Geom3 {
    Curve(Curve),
    /// A polyline lying on a surface, parameterised by vertex index.
    Poly(Vec<DVec3>),
}

impl Geom3 {
    fn eval(&self, t: f64) -> DVec3 {
        match self {
            Geom3::Curve(c) => c.eval(t),
            Geom3::Poly(p) => poly_eval(p, t),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    Edge,
    Tangent,
    Silhouette,
}

struct Candidate {
    geom: Geom3,
    t0: f64,
    t1: f64,
    role: Role,
    /// For silhouettes: the body and face the curve must stay on.
    face: Option<(usize, usize)>,
}

/// Projects `solids` as seen along `axes`: every edge and silhouette, cut into visible and
/// hidden pieces. Tangent edges come back as [`LineKind::Tangent`] and
/// [`LineKind::HiddenTangent`] so the caller can show or hide them without projecting
/// again.
pub fn project(solids: &[Solid], axes: &ViewAxes, progress: &Progress) -> Result<ViewGeometry, ProjectError> {
    project_except(solids, axes, progress, &|_, _| false)
}

/// [`project`], leaving out the edges `skip(body, edge)` picks (a section view's
/// shoulders).
pub fn project_except(
    solids: &[Solid],
    axes: &ViewAxes,
    progress: &Progress,
    skip: &(dyn Fn(usize, usize) -> bool + Sync),
) -> Result<ViewGeometry, ProjectError> {
    let locators: Vec<FaceLocator> = solids.iter().map(FaceLocator::new).collect();
    let size = model_size(solids);
    let mut candidates = Vec::new();
    for (b, s) in solids.iter().enumerate() {
        for (e, edge) in s.edges.iter().enumerate() {
            if skip(b, e) {
                continue;
            }
            let role = match classify_edge(s, e) {
                EdgeClass::Seam => continue,
                EdgeClass::Sharp => Role::Edge,
                EdgeClass::Tangent => Role::Tangent,
            };
            candidates.push(Candidate {
                geom: Geom3::Curve(edge.curve.clone()),
                t0: edge.t0,
                t1: edge.t1,
                role,
                face: None,
            });
        }
        silhouettes(s, b, axes, size, &mut candidates);
    }
    progress.add_total(candidates.len());

    let ctx = Ctx { locators: &locators, axes, size, progress };
    let next = AtomicUsize::new(0);
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get()).min(candidates.len().max(1));
    let mut results: Vec<(usize, Vec<ViewLine>)> = Vec::new();
    std::thread::scope(|scope| {
        let workers: Vec<_> = (0..threads)
            .map(|_| {
                scope.spawn(|| {
                    let mut out = Vec::new();
                    loop {
                        let i = next.fetch_add(1, Ordering::Relaxed);
                        let Some(c) = candidates.get(i) else { break };
                        if progress.cancelled() {
                            break;
                        }
                        out.push((i, ctx.pieces(c)));
                        progress.step();
                    }
                    out
                })
            })
            .collect();
        for w in workers {
            // A worker only panics on a bug; its curves are then missing from the view.
            if let Ok(r) = w.join() {
                results.extend(r);
            }
        }
    });
    if progress.cancelled() {
        return Err(ProjectError::Cancelled);
    }
    results.sort_by_key(|(i, _)| *i);
    let lines: Vec<ViewLine> = results.into_iter().flat_map(|(_, l)| l).collect();
    Ok(ViewGeometry {
        lines: join_lines(remove_overlaps(lines, size, progress)?, size),
        ..ViewGeometry::default()
    })
}

/// Joins straight pieces of the same kind that continue one another, so an edge cut where
/// another line met it is drawn as one line again.
fn join_lines(lines: Vec<ViewLine>, size: f64) -> Vec<ViewLine> {
    let tol = 1e-9 * size;
    // Pieces split where another line covered the rest end within the overlap tolerance
    // of each other.
    let gap = 1e-6 * size;
    let (mut straight, mut out): (Vec<ViewLine>, Vec<ViewLine>) =
        lines.into_iter().partition(|l| matches!(l.path, Path2::Line { .. }));
    let mut i = 0;
    while i < straight.len() {
        let mut merged = false;
        for j in (i + 1)..straight.len() {
            if straight[i].kind != straight[j].kind {
                continue;
            }
            let (Path2::Line { a, b }, Path2::Line { a: c, b: d }) = (&straight[i].path, &straight[j].path)
            else {
                continue;
            };
            let dir = (*b - *a).normalize_or_zero();
            let off = |p: DVec2| (p - *a).perp_dot(dir).abs();
            if off(*c) > tol || off(*d) > tol {
                continue;
            }
            // Collinear: join when they touch or overlap.
            let t = |p: DVec2| (p - *a).dot(dir);
            let (s0, s1) = (0.0, t(*b));
            let (u0, u1) = (t(*c).min(t(*d)), t(*c).max(t(*d)));
            if u0 > s1 + gap || u1 < s0 - gap {
                continue;
            }
            let (lo, hi) = (s0.min(u0), s1.max(u1));
            let start = *a;
            straight[i].path = Path2::Line { a: start + dir * lo, b: start + dir * hi };
            straight.swap_remove(j);
            merged = true;
            break;
        }
        if !merged {
            i += 1;
        }
    }
    out.extend(straight);
    out
}

/// The length of the diagonal of the box round all the solids (at least 1 mm).
fn model_size(solids: &[Solid]) -> f64 {
    let mut lo = DVec3::splat(f64::INFINITY);
    let mut hi = DVec3::splat(f64::NEG_INFINITY);
    for s in solids {
        if let Some((a, b)) = s.bounds() {
            lo = lo.min(a);
            hi = hi.max(b);
        }
    }
    if lo.x.is_finite() { (hi - lo).length().max(1.0) } else { 1.0 }
}

/// Every point of the face's boundary, sampled, for working out how far it reaches.
fn face_points(solid: &Solid, face: &anvil_brep::topo::Face) -> Vec<DVec3> {
    let mut pts = Vec::new();
    for l in face.loops() {
        for c in &l.coedges {
            if let Some(e) = solid.edge(c.edge) {
                pts.extend(e.curve.sample(e.t0, e.t1, 0.0, PI / 16.0));
            }
        }
    }
    pts
}

/// The silhouettes of the solid's curved faces as seen along `axes`.
fn silhouettes(solid: &Solid, body: usize, axes: &ViewAxes, size: f64, out: &mut Vec<Candidate>) {
    let e = axes.eye;
    let mut mesh = None;
    for (fi, face) in solid.faces.iter().enumerate() {
        let on = Some((body, fi));
        // A line along `dir` through `origin`, long enough to cross the face.
        let line = |origin: DVec3, dir: DVec3, out: &mut Vec<Candidate>| {
            let pts = face_points(solid, face);
            let ts = pts.iter().map(|p| (*p - origin).dot(dir));
            let (lo, hi) = ts.fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), t| (a.min(t), b.max(t)));
            if lo.is_finite() {
                let pad = 1e-3 * size;
                out.push(Candidate {
                    geom: Geom3::Curve(Curve::Line { origin, dir }),
                    t0: lo - pad,
                    t1: hi + pad,
                    role: Role::Silhouette,
                    face: on,
                });
            }
        };
        match face.surface {
            Surface::Plane { .. } => {}
            Surface::Cylinder { frame, radius } => {
                let w = frame.z.cross(e);
                if w.length() < 1e-9 {
                    continue;
                }
                let w = w.normalize();
                for s in [1.0, -1.0] {
                    line(frame.origin + w * (s * radius), frame.z, out);
                }
            }
            Surface::Cone { frame, radius, half_angle, .. } => {
                let (a, b, c) = (frame.x.dot(e), frame.y.dot(e), half_angle.tan() * frame.z.dot(e));
                let r = a.hypot(b);
                if r < 1e-12 || c.abs() > r {
                    continue;
                }
                let base = b.atan2(a);
                let spread = (c / r).clamp(-1.0, 1.0).acos();
                for th in [base + spread, base - spread] {
                    let d = frame.x * th.cos() + frame.y * th.sin();
                    let dir = (d * half_angle.tan() + frame.z).normalize();
                    line(frame.origin + d * radius, dir, out);
                }
            }
            Surface::Sphere { frame, radius } => {
                let f = Frame { origin: frame.origin, x: axes.right, y: axes.up, z: e };
                out.push(Candidate {
                    geom: Geom3::Curve(Curve::Circle { frame: f, radius }),
                    t0: 0.0,
                    t1: TAU,
                    role: Role::Silhouette,
                    face: on,
                });
            }
            Surface::Torus { frame, major, minor, .. } => {
                // Seen square to its axis, the tube's outline is exactly its top and bottom
                // circles and the two tube circles across the view; the curves below only
                // reach those as a limit.
                if frame.z.dot(e).abs() < 1e-6 {
                    for side in [1.0, -1.0] {
                        let f = Frame { origin: frame.origin + frame.z * (side * minor), ..frame };
                        out.push(Candidate {
                            geom: Geom3::Curve(Curve::Circle { frame: f, radius: major }),
                            t0: 0.0,
                            t1: TAU,
                            role: Role::Silhouette,
                            face: on,
                        });
                    }
                    let across = frame.z.cross(e);
                    if across.length() > 1e-9 {
                        let d0 = across.normalize();
                        for d in [d0, -d0] {
                            let f = Frame {
                                origin: frame.origin + d * major,
                                x: d,
                                y: frame.z,
                                z: d.cross(frame.z),
                            };
                            out.push(Candidate {
                                geom: Geom3::Curve(Curve::Circle { frame: f, radius: minor }),
                                t0: 0.0,
                                t1: TAU,
                                role: Role::Silhouette,
                                face: on,
                            });
                        }
                    }
                    continue;
                }
                // On the tube circle at angle phi round the axis, the normal is
                // cos(psi) d + sin(psi) z; it is square to the eye at two values of psi.
                const N: usize = 360;
                for side in [0.0, PI] {
                    let pts: Vec<DVec3> = (0..=N)
                        .map(|i| {
                            let phi = TAU * i as f64 / N as f64;
                            let d = frame.x * phi.cos() + frame.y * phi.sin();
                            let psi = (-d.dot(e)).atan2(frame.z.dot(e)) + side;
                            let n = d * psi.cos() + frame.z * psi.sin();
                            frame.origin + d * major + n * minor
                        })
                        .collect();
                    out.push(Candidate {
                        t0: 0.0,
                        t1: (pts.len() - 1) as f64,
                        geom: Geom3::Poly(pts),
                        role: Role::Silhouette,
                        face: on,
                    });
                }
            }
            Surface::BSpline(_) => {
                let mesh = mesh.get_or_insert_with(|| {
                    anvil_brep::tess::tessellate(solid, &anvil_brep::tess::TessParams::default()).ok()
                });
                if let Some(m) = mesh {
                    for poly in mesh_silhouette(m, fi, face, e) {
                        out.push(Candidate {
                            t0: 0.0,
                            t1: (poly.len() - 1) as f64,
                            geom: Geom3::Poly(poly),
                            role: Role::Silhouette,
                            face: on,
                        });
                    }
                }
            }
        }
    }
}

/// Where a freeform face turns away from the eye, found on the display mesh (where the
/// exact normal changes sign across a triangle) and moved onto the surface.
fn mesh_silhouette(
    m: &anvil_brep::tess::TriMesh,
    fi: usize,
    face: &anvil_brep::topo::Face,
    eye: DVec3,
) -> Vec<Vec<DVec3>> {
    let mut segs: Vec<(DVec3, DVec3)> = Vec::new();
    for (t, tri) in m.triangles.iter().enumerate() {
        if m.triangle_face.get(t).copied() != Some(fi as u32) {
            continue;
        }
        let p = tri.map(|i| m.positions[i as usize]);
        let g = p.map(|q| face.normal_at(q).dot(eye));
        let mut cut = Vec::new();
        for k in 0..3 {
            let (a, b) = (k, (k + 1) % 3);
            if (g[a] > 0.0) != (g[b] > 0.0) {
                let f = g[a] / (g[a] - g[b]);
                cut.push(p[a] + (p[b] - p[a]) * f);
            }
        }
        if let [a, b] = cut[..] {
            segs.push((a, b));
        }
    }
    // Chain the pieces into polylines.
    let key = |p: DVec3| ((p.x * 1e6).round() as i64, (p.y * 1e6).round() as i64, (p.z * 1e6).round() as i64);
    let mut used = vec![false; segs.len()];
    let mut out = Vec::new();
    for s in 0..segs.len() {
        if used[s] {
            continue;
        }
        used[s] = true;
        let mut chain = vec![segs[s].0, segs[s].1];
        for forward in [true, false] {
            loop {
                let end = if forward { chain[chain.len() - 1] } else { chain[0] };
                let next = (0..segs.len())
                    .find(|&j| !used[j] && (key(segs[j].0) == key(end) || key(segs[j].1) == key(end)));
                let Some(j) = next else { break };
                used[j] = true;
                let other = if key(segs[j].0) == key(end) { segs[j].1 } else { segs[j].0 };
                if forward {
                    chain.push(other);
                } else {
                    chain.insert(0, other);
                }
            }
        }
        let snapped: Vec<DVec3> = chain.iter().map(|p| face.surface.eval_uv(face.surface.uv(*p))).collect();
        out.push(snapped);
    }
    out
}

struct Ctx<'a> {
    locators: &'a [FaceLocator<'a>],
    axes: &'a ViewAxes,
    size: f64,
    progress: &'a Progress,
}

/// Bisection steps used to place a change of visibility.
const BISECT: usize = 52;

impl Ctx<'_> {
    fn visible(&self, p: DVec3) -> bool {
        let t_min = 1e-7 * self.size + 1e-6;
        !self.locators.iter().any(|l| l.ray_blocked(p, self.axes.eye, t_min))
    }

    fn on_face(&self, (b, f): (usize, usize), p: DVec3) -> bool {
        self.locators.get(b).is_some_and(|l| l.on_face(f, p))
    }

    /// How many samples to test along a curve: enough that a gap of a few thousandths of
    /// the model's size is not missed.
    fn samples(&self, c: &Candidate, t0: f64, t1: f64) -> usize {
        let len: f64 = (0..16)
            .map(|i| {
                let (a, b) = (t0 + (t1 - t0) * i as f64 / 16.0, t0 + (t1 - t0) * (i + 1) as f64 / 16.0);
                (c.geom.eval(b) - c.geom.eval(a)).length()
            })
            .sum();
        let n = ((len / self.size) * 400.0).ceil() as usize;
        let n = match c.geom {
            Geom3::Poly(ref p) => n.max(p.len() - 1),
            Geom3::Curve(Curve::Line { .. }) => n,
            Geom3::Curve(_) => n.max(24),
        };
        n.clamp(8, 1200)
    }

    /// The parameter ranges of `[t0, t1]` where `test` holds, each end placed by bisection.
    fn split(&self, c: &Candidate, t0: f64, t1: f64, test: impl Fn(DVec3) -> bool) -> Vec<(f64, f64, bool)> {
        let n = self.samples(c, t0, t1);
        // Test just inside the ends: points at a vertex graze every face meeting there.
        let inset = 1e-6 * (t1 - t0);
        let at = |i: usize| -> f64 {
            match i {
                0 => t0 + inset,
                i if i == n => t1 - inset,
                i => t0 + (t1 - t0) * i as f64 / n as f64,
            }
        };
        let states: Vec<bool> = (0..=n).map(|i| test(c.geom.eval(at(i)))).collect();
        let mut out: Vec<(f64, f64, bool)> = Vec::new();
        let mut start = t0;
        for i in 0..n {
            if states[i] == states[i + 1] {
                continue;
            }
            let (mut a, mut b) = (at(i), at(i + 1));
            for _ in 0..BISECT {
                let m = 0.5 * (a + b);
                if test(c.geom.eval(m)) == states[i] {
                    a = m;
                } else {
                    b = m;
                }
            }
            let t = 0.5 * (a + b);
            out.push((start, t, states[i]));
            start = t;
        }
        out.push((start, t1, states[n]));
        self.drop_specks(c, out)
    }

    /// Folds runs too short to see (where a curve passes just by an edge) into the runs
    /// beside them.
    fn drop_specks(&self, c: &Candidate, mut runs: Vec<(f64, f64, bool)>) -> Vec<(f64, f64, bool)> {
        let speck = 2e-4 * self.size;
        let len = |r: &(f64, f64, bool)| (c.geom.eval(r.1) - c.geom.eval(r.0)).length();
        if runs.len() > 1 {
            for i in 0..runs.len() {
                if len(&runs[i]) < speck {
                    let neighbour = if i > 0 { runs[i - 1].2 } else { runs[i + 1].2 };
                    runs[i].2 = neighbour;
                }
            }
        }
        let mut merged: Vec<(f64, f64, bool)> = Vec::new();
        for r in runs {
            match merged.last_mut() {
                Some(last) if last.2 == r.2 => last.1 = r.1,
                _ => merged.push(r),
            }
        }
        merged
    }

    /// Moves the end of a silhouette, found by bisection to within the face test's
    /// tolerance, onto the face's boundary edge it crosses there.
    fn snap_end(&self, c: &Candidate, (b, f): (usize, usize), t: f64) -> f64 {
        let Geom3::Curve(curve) = &c.geom else { return t };
        let Some(solid) = self.locators.get(b).map(|l| l.solid()) else { return t };
        let Some(face) = solid.faces.get(f) else { return t };
        let p = curve.eval(t);
        let near = 1e-3 * self.size;
        // An edge the silhouette runs along (a seam) does not end it.
        let step = 1e-3 * (c.t1 - c.t0);
        let tangent = (curve.eval(t + step) - curve.eval(t - step)).normalize_or_zero();
        let along = |e: &anvil_brep::topo::Edge| {
            let u = e.curve.param_in_range(p, e.t0, e.t1);
            let h = 1e-6 * (e.t1 - e.t0).abs().max(1e-9);
            let et = (e.curve.eval(u + h) - e.curve.eval(u - h)).normalize_or_zero();
            e.curve.distance_to(p, e.t0, e.t1) < 1e-3 * self.size && et.cross(tangent).length() < 1e-6
        };
        let nearest = face
            .loops()
            .flat_map(|l| l.coedges.iter())
            .filter_map(|co| solid.edge(co.edge))
            .filter(|e| !along(e))
            .map(|e| (e.curve.distance_to(p, e.t0, e.t1), e))
            .min_by(|a, b| a.0.total_cmp(&b.0));
        let Some((d, e)) = nearest else { return t };
        if d > near {
            return t;
        }
        // Alternate between the closest points of the silhouette and the edge, keeping to
        // the stretch of the silhouette round the old end.
        let (lo, hi) = (t - 100.0 * step, t + 100.0 * step);
        let mut s = t;
        for _ in 0..32 {
            let q = e.curve.eval(e.curve.param_in_range(curve.eval(s), e.t0, e.t1));
            let next = curve.param_in_range(q, lo, hi);
            let done = (next - s).abs() < 1e-14 * (c.t1 - c.t0).abs().max(1.0);
            s = next;
            if done {
                break;
            }
        }
        let moved = (curve.eval(s) - p).length();
        let gap = e.curve.distance_to(curve.eval(s), e.t0, e.t1);
        // Keep the old end if the edge only grazes the silhouette.
        if moved <= near && gap <= d { s } else { t }
    }

    fn pieces(&self, c: &Candidate) -> Vec<ViewLine> {
        let ranges = match c.face {
            Some(f) => self
                .split(c, c.t0, c.t1, |p| self.on_face(f, p))
                .into_iter()
                .filter(|r| r.2)
                .map(|r| (self.snap_end(c, f, r.0), self.snap_end(c, f, r.1)))
                .collect(),
            None => vec![(c.t0, c.t1)],
        };
        let mut out = Vec::new();
        for (a, b) in ranges {
            if self.progress.cancelled() {
                break;
            }
            for (s0, s1, vis) in self.split(c, a, b, |p| self.visible(p)) {
                let kind = match (c.role, vis) {
                    (Role::Tangent, true) => LineKind::Tangent,
                    (Role::Tangent, false) => LineKind::HiddenTangent,
                    (_, true) => LineKind::Visible,
                    (_, false) => LineKind::Hidden,
                };
                if let Some(path) = self.project_piece(&c.geom, s0, s1) {
                    out.push(ViewLine { kind, path });
                }
            }
        }
        out
    }

    /// The piece of `geom` from `t0` to `t1` on the view plane, or `None` when it is seen
    /// end-on.
    fn project_piece(&self, geom: &Geom3, t0: f64, t1: f64) -> Option<Path2> {
        let ax = self.axes;
        let tiny = 1e-9 * self.size;
        let path = match geom {
            Geom3::Curve(Curve::Line { .. }) => {
                Path2::Line { a: ax.project(geom.eval(t0)), b: ax.project(geom.eval(t1)) }
            }
            Geom3::Curve(Curve::Circle { frame, radius }) => conic(
                ax.project(frame.origin),
                ax.project_dir(frame.x) * *radius,
                ax.project_dir(frame.y) * *radius,
                t0,
                t1,
            ),
            Geom3::Curve(Curve::Ellipse { frame, a, b }) => conic(
                ax.project(frame.origin),
                ax.project_dir(frame.x) * *a,
                ax.project_dir(frame.y) * *b,
                t0,
                t1,
            ),
            Geom3::Curve(c @ Curve::Spline(_)) => {
                let chord = 2e-4 * self.size;
                Path2::Poly(c.sample(t0, t1, chord, PI / 36.0).into_iter().map(|p| ax.project(p)).collect())
            }
            Geom3::Poly(p) => Path2::Poly(poly_sub(p, t0, t1).into_iter().map(|q| ax.project(q)).collect()),
        };
        let (lo, hi) = path.bounds();
        let extent = match &path {
            Path2::Line { a, b } => (*b - *a).length(),
            Path2::Conic { .. } => path.length_estimate(),
            Path2::Poly(_) => (hi - lo).length(),
        };
        (extent > tiny).then_some(path)
    }
}

/// The projection of `c + u cos t + v sin t`: a conic, or the line it covers when the
/// curve is seen edge-on.
fn conic(c: DVec2, u: DVec2, v: DVec2, t0: f64, t1: f64) -> Path2 {
    let scale = u.length().max(v.length());
    if u.perp_dot(v).abs() > 1e-9 * scale * scale {
        return Path2::Conic { c, u, v, t0, t1 };
    }
    let w = if u.length() >= v.length() { u } else { v };
    let w = if w.length() > 0.0 { w.normalize() } else { DVec2::X };
    let (a, b) = (u.dot(w), v.dot(w));
    let s = |t: f64| a * t.cos() + b * t.sin();
    let (lo_t, hi_t) = (t0.min(t1), t0.max(t1));
    let mut ts = vec![lo_t, hi_t];
    let crit = b.atan2(a);
    let mut k = ((lo_t - crit) / PI).ceil();
    while crit + k * PI <= hi_t {
        ts.push(crit + k * PI);
        k += 1.0;
    }
    let (smin, smax) = ts
        .iter()
        .map(|t| s(*t))
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), x| (lo.min(x), hi.max(x)));
    Path2::Line { a: c + w * smin, b: c + w * smax }
}

/// Drawn order: a line wins over any later line lying on it.
fn rank(k: LineKind) -> u8 {
    match k {
        LineKind::Visible => 0,
        LineKind::Tangent => 1,
        LineKind::Hidden => 2,
        LineKind::HiddenTangent => 3,
    }
}

/// Drops the parts of lines that lie on a line drawn before them, visible lines first.
fn remove_overlaps(
    mut lines: Vec<ViewLine>,
    size: f64,
    progress: &Progress,
) -> Result<Vec<ViewLine>, ProjectError> {
    lines.sort_by_key(|l| rank(l.kind));
    // Coincident projections of exact geometry agree to rounding; a line that only
    // crosses another, or touches it tangentially, stays this close to it for under
    // `min_cover`, which is too short to matter on paper.
    let eps = 1e-7 * size;
    let min_cover = 1e-3 * size;
    let mut kept: Vec<(ViewLine, (DVec2, DVec2))> = Vec::new();
    for line in lines {
        if progress.cancelled() {
            return Err(ProjectError::Cancelled);
        }
        let (lo, hi) = line.path.bounds();
        let near: Vec<&Path2> = kept
            .iter()
            .filter(|(_, (a, b))| {
                a.x <= hi.x + eps && b.x >= lo.x - eps && a.y <= hi.y + eps && b.y >= lo.y - eps
            })
            .map(|(l, _)| &l.path)
            .collect();
        let covered = |p: DVec2| near.iter().any(|q| q.distance(p) <= eps);
        let (s0, s1) = line.path.range();
        let len = line.path.length_estimate();
        let n = (((len / size) * 400.0).ceil() as usize).clamp(8, 1200);
        let inset = 1e-6 * (s1 - s0);
        let at = |i: usize| match i {
            0 => s0 + inset,
            i if i == n => s1 - inset,
            i => s0 + (s1 - s0) * i as f64 / n as f64,
        };
        if near.is_empty() {
            let b = line.path.bounds();
            kept.push((line, b));
            continue;
        }
        let states: Vec<bool> = (0..=n).map(|i| covered(line.path.eval(at(i)))).collect();
        let mut start = s0;
        let mut spans = Vec::new();
        for i in 0..n {
            if states[i] == states[i + 1] {
                continue;
            }
            let (mut a, mut b) = (at(i), at(i + 1));
            for _ in 0..BISECT {
                let m = 0.5 * (a + b);
                if covered(line.path.eval(m)) == states[i] {
                    a = m;
                } else {
                    b = m;
                }
            }
            let t = 0.5 * (a + b);
            spans.push((start, t, states[i]));
            start = t;
        }
        spans.push((start, s1, states[n]));
        for sp in &mut spans {
            let whole = sp.0 == s0 && sp.1 == s1;
            if sp.2 && !whole && line.path.sub(sp.0, sp.1).length_estimate() < min_cover {
                sp.2 = false;
            }
        }
        let mut merged: Vec<(f64, f64, bool)> = Vec::new();
        for sp in spans {
            match merged.last_mut() {
                Some(last) if last.2 == sp.2 => last.1 = sp.1,
                _ => merged.push(sp),
            }
        }
        for (a, b, cov) in merged {
            if cov {
                continue;
            }
            let path = if (a, b) == (s0, s1) { line.path.clone() } else { line.path.sub(a, b) };
            if path.length_estimate() <= 100.0 * eps {
                continue;
            }
            let bb = path.bounds();
            kept.push((ViewLine { kind: line.kind, path }, bb));
        }
    }
    Ok(kept.into_iter().map(|(l, _)| l).collect())
}
