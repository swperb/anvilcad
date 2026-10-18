//! Design-for-manufacturing checks on a built part, for CNC machining or sheet metal.
//!
//! The checks read the faceted body: every polygon carries its face's name and exact
//! surface (plane or cylinder axis and radius), so faces, holes and bends are found
//! from that, and wall thickness is measured by casting rays through the part. Each
//! finding names the faces it is about, so the app can show them.
//!
//! The limits are common shop rules of thumb, scaled by the part's material where it
//! matters (plastics need thicker walls; each metal has its own smallest bend radius).
//! They flag what is likely to be costly or to fail, not what is impossible everywhere.

use crate::material::{Material, MaterialClass};
use anvil_kernel::{v3, FaceId, Solid, SurfaceKind, Vec3};
use std::collections::{BTreeMap, HashMap};
use std::f64::consts::TAU;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Process {
    CncMachining,
    SheetMetal,
}

impl Process {
    pub const ALL: [Process; 2] = [Process::CncMachining, Process::SheetMetal];

    pub fn label(self) -> &'static str {
        match self {
            Process::CncMachining => "CNC machining",
            Process::SheetMetal => "Sheet metal",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    /// Cannot be made as drawn, or will likely fail.
    Error,
    /// Can be made, at extra cost or risk.
    Warning,
    /// Worth knowing.
    Note,
}

impl Severity {
    pub fn label(self) -> &'static str {
        match self {
            Severity::Error => "Error",
            Severity::Warning => "Warning",
            Severity::Note => "Note",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Finding {
    pub severity: Severity,
    /// Short name of the rule, e.g. "Thin wall".
    pub rule: &'static str,
    pub message: String,
    pub faces: Vec<FaceId>,
    /// How many places this applies to (identical findings are merged).
    pub count: usize,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DfmReport {
    pub findings: Vec<Finding>,
    /// Sheet thickness, for a sheet metal check of a uniform-thickness part.
    pub thickness: Option<f64>,
    /// Bends found in a sheet metal part.
    pub bends: usize,
}

impl DfmReport {
    /// Places flagged at severity `s`.
    pub fn count(&self, s: Severity) -> usize {
        self.findings.iter().filter(|f| f.severity == s).map(|f| f.count).sum()
    }
}

/// Thinnest wall worth machining when no material is assigned (mm).
const DEFAULT_MIN_WALL: f64 = 0.8;
/// Smallest common drill (mm).
const MIN_DRILL: f64 = 1.0;
/// Smallest inside radius a common end mill leaves (mm).
const MIN_MILL_RADIUS: f64 = 0.5;
/// How many diameters deep a standard drill or end mill reaches.
const STANDARD_REACH: f64 = 4.0;
/// Beyond this many diameters, special tooling (gun drills, long reach mills).
const MAX_REACH: f64 = 10.0;
/// Rays cast per check, spread over the faces by area.
const RAY_BUDGET: usize = 600;

/// One face of the part, gathered from its polygons.
struct Face {
    id: FaceId,
    kind: SurfaceKind,
    polys: Vec<usize>,
    area: f64,
}

/// A cylindrical face seen about its axis.
#[derive(Clone, Copy, Debug)]
struct Round {
    id: FaceId,
    origin: Vec3,
    axis: Vec3,
    radius: f64,
    /// True when the material is outside the cylinder (a hole or inside corner).
    concave: bool,
    /// Angle covered round the axis.
    coverage: f64,
    /// Extent along the axis.
    length: f64,
    /// Middle of the face along the axis, on the axis.
    centre: Vec3,
}

/// A wall thickness reading.
#[derive(Clone, Copy, Debug)]
struct Sample {
    face: FaceId,
    hit: FaceId,
    thickness: f64,
    area: f64,
    point: Vec3,
}

/// A bounding-volume tree over the body's polygons, so rays test only the few
/// polygons near them (a check casts hundreds of rays, and imported parts can have a
/// million polygons).
struct Bvh {
    nodes: Vec<Node>,
    order: Vec<usize>,
}

struct Node {
    lo: Vec3,
    hi: Vec3,
    /// The node's range in `order`; inner nodes also name their two children.
    start: usize,
    end: usize,
    children: Option<(usize, usize)>,
}

struct Hit {
    t: f64,
    normal: Vec3,
    face: FaceId,
}

impl Bvh {
    fn new(body: &Solid) -> Bvh {
        let boxes: Vec<(Vec3, Vec3, Vec3)> = body
            .polygons
            .iter()
            .map(|p| {
                let lo = p.verts.iter().fold(p.verts[0], |a, v| a.min(*v));
                let hi = p.verts.iter().fold(p.verts[0], |a, v| a.max(*v));
                (lo, hi, (lo + hi) * 0.5)
            })
            .collect();
        let mut order: Vec<usize> = (0..boxes.len()).collect();
        let mut nodes: Vec<Node> = Vec::new();
        // Iterative build: (node index, start, end).
        nodes.push(Node { lo: Vec3::ZERO, hi: Vec3::ZERO, start: 0, end: order.len(), children: None });
        let mut stack = vec![0usize];
        while let Some(ni) = stack.pop() {
            let (start, end) = (nodes[ni].start, nodes[ni].end);
            let (mut lo, mut hi) = (
                v3(f64::INFINITY, f64::INFINITY, f64::INFINITY),
                v3(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY),
            );
            for &i in &order[start..end] {
                lo = lo.min(boxes[i].0);
                hi = hi.max(boxes[i].1);
            }
            nodes[ni].lo = lo;
            nodes[ni].hi = hi;
            if end - start <= 8 {
                continue;
            }
            let ext = hi - lo;
            let axis = if ext.x >= ext.y && ext.x >= ext.z {
                0
            } else if ext.y >= ext.z {
                1
            } else {
                2
            };
            let key = |i: &usize| {
                let c = boxes[*i].2;
                [c.x, c.y, c.z][axis]
            };
            let mid = (start + end) / 2;
            order[start..end].select_nth_unstable_by(mid - start, |a, b| key(a).total_cmp(&key(b)));
            let (l, r) = (nodes.len(), nodes.len() + 1);
            nodes.push(Node { lo, hi, start, end: mid, children: None });
            nodes.push(Node { lo, hi, start: mid, end, children: None });
            nodes[ni].children = Some((l, r));
            stack.push(l);
            stack.push(r);
        }
        Bvh { nodes, order }
    }

    fn raycast(&self, body: &Solid, origin: Vec3, dir: Vec3) -> Option<Hit> {
        let inv = v3(1.0 / dir.x, 1.0 / dir.y, 1.0 / dir.z);
        let slab = |n: &Node, best: f64| {
            let (mut t0, mut t1) = (0.0f64, best);
            for (o, i, lo, hi) in [
                (origin.x, inv.x, n.lo.x, n.hi.x),
                (origin.y, inv.y, n.lo.y, n.hi.y),
                (origin.z, inv.z, n.lo.z, n.hi.z),
            ] {
                let (a, b) = ((lo - 1e-9 - o) * i, (hi + 1e-9 - o) * i);
                let (a, b) = if a <= b { (a, b) } else { (b, a) };
                // A ray parallel to the slab and outside it gives NaN; treat as a miss.
                if a.is_nan() || b.is_nan() {
                    if o < lo - 1e-9 || o > hi + 1e-9 {
                        return false;
                    }
                    continue;
                }
                t0 = t0.max(a);
                t1 = t1.min(b);
            }
            t0 <= t1
        };
        let mut best: Option<Hit> = None;
        let mut stack = vec![0usize];
        while let Some(ni) = stack.pop() {
            let n = &self.nodes[ni];
            if !slab(n, best.as_ref().map_or(f64::INFINITY, |b| b.t)) {
                continue;
            }
            if let Some((l, r)) = n.children {
                stack.push(l);
                stack.push(r);
                continue;
            }
            for &pi in &self.order[n.start..n.end] {
                let p = &body.polygons[pi];
                let denom = p.normal.dot(dir);
                if denom.abs() < 1e-12 {
                    continue;
                }
                let t = (p.w - p.normal.dot(origin)) / denom;
                if t < 0.0 || best.as_ref().is_some_and(|b| t >= b.t) {
                    continue;
                }
                let q = origin + dir * t;
                let k = p.verts.len();
                let inside = (0..k).all(|i| {
                    let (a, b) = (p.verts[i], p.verts[(i + 1) % k]);
                    (b - a).cross(q - a).dot(p.normal) >= -1e-9
                });
                if inside {
                    best = Some(Hit { t, normal: p.normal, face: p.tag.id });
                }
            }
        }
        best
    }
}

struct Part<'a> {
    body: &'a Solid,
    bvh: Bvh,
    faces: Vec<Face>,
    index: HashMap<FaceId, usize>,
    /// Segments of the edges between each pair of faces.
    edges: BTreeMap<(FaceId, FaceId), Vec<[Vec3; 2]>>,
}

impl<'a> Part<'a> {
    fn new(body: &'a Solid) -> Part<'a> {
        let mut faces: Vec<Face> = Vec::new();
        let mut index: HashMap<FaceId, usize> = HashMap::new();
        for (pi, p) in body.polygons.iter().enumerate() {
            let fi = *index.entry(p.tag.id).or_insert_with(|| {
                faces.push(Face { id: p.tag.id, kind: p.tag.surface, polys: Vec::new(), area: 0.0 });
                faces.len() - 1
            });
            faces[fi].polys.push(pi);
            faces[fi].area += poly_area(&p.verts);
        }
        let mut edges: BTreeMap<(FaceId, FaceId), Vec<[Vec3; 2]>> = BTreeMap::new();
        for e in body.model_edges() {
            if e.faces.a != e.faces.b {
                edges.entry((e.faces.a, e.faces.b)).or_default().push(e.seg);
            }
        }
        Part { body, bvh: Bvh::new(body), faces, index, edges }
    }

    fn face(&self, id: FaceId) -> Option<&Face> {
        self.index.get(&id).map(|&i| &self.faces[i])
    }

    fn neighbours(&self, id: FaceId) -> impl Iterator<Item = (FaceId, &Vec<[Vec3; 2]>)> {
        self.edges.iter().filter_map(move |(&(a, b), segs)| {
            if a == id {
                Some((b, segs))
            } else if b == id {
                Some((a, segs))
            } else {
                None
            }
        })
    }

    fn round(&self, f: &Face) -> Option<Round> {
        let SurfaceKind::Cylinder { axis_origin, axis_dir, radius } = f.kind else {
            return None;
        };
        let axis = axis_dir.normalized();
        let x = axis.any_perpendicular().normalized();
        let y = axis.cross(x);
        let mut angles = Vec::new();
        let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
        let mut votes = 0.0;
        for &pi in &f.polys {
            let p = &self.body.polygons[pi];
            for v in &p.verts {
                let d = *v - axis_origin;
                let h = d.dot(axis);
                lo = lo.min(h);
                hi = hi.max(h);
                angles.push(d.dot(y).atan2(d.dot(x)).rem_euclid(TAU));
            }
            let c = centroid(&p.verts);
            let radial = (c - axis_origin) - axis * (c - axis_origin).dot(axis);
            votes += p.normal.dot(radial.normalized()) * poly_area(&p.verts);
        }
        angles.sort_by(f64::total_cmp);
        let n = angles.len();
        let gap = (0..n)
            .map(|i| if i + 1 < n { angles[i + 1] - angles[i] } else { angles[0] + TAU - angles[i] })
            .fold(0.0, f64::max);
        Some(Round {
            id: f.id,
            origin: axis_origin,
            axis,
            radius,
            concave: votes < 0.0,
            coverage: TAU - gap,
            length: hi - lo,
            centre: axis_origin + axis * (0.5 * (lo + hi)),
        })
    }

    fn rounds(&self) -> Vec<Round> {
        self.faces.iter().filter_map(|f| self.round(f)).collect()
    }

    /// Wall thickness samples: for points spread over the faces by area, the distance
    /// through the material to the facing surface, with the face hit.
    fn thickness_samples(&self) -> Vec<Sample> {
        let total: f64 = self.faces.iter().map(|f| f.area).sum();
        let mut out = Vec::new();
        if total <= 0.0 {
            return out;
        }
        for f in &self.faces {
            // At least one ray per face, the rest by area.
            let want = ((f.area / total) * RAY_BUDGET as f64).ceil().max(1.0) as usize;
            let mut polys: Vec<usize> = f.polys.clone();
            polys.sort_by(|&a, &b| {
                poly_area(&self.body.polygons[b].verts).total_cmp(&poly_area(&self.body.polygons[a].verts))
            });
            let step = (polys.len() / want).max(1);
            for &pi in polys.iter().step_by(step).take(want) {
                let p = &self.body.polygons[pi];
                let c = centroid(&p.verts);
                let inward = p.normal * -1.0;
                let start = c + inward * 1e-6;
                if let Some(hit) =
                    self.bvh.raycast(self.body, start, inward).filter(|h| h.normal.dot(p.normal) < -0.5)
                {
                    out.push(Sample {
                        face: f.id,
                        hit: hit.face,
                        thickness: hit.t + 1e-6,
                        area: poly_area(&p.verts),
                        point: c,
                    });
                }
            }
        }
        out
    }

    /// True when a straight line from `p` along `dir` leaves the part without hitting it.
    fn open_towards(&self, p: Vec3, dir: Vec3) -> bool {
        self.bvh.raycast(self.body, p + dir * 1e-4, dir).is_none()
    }
}

fn centroid(v: &[Vec3]) -> Vec3 {
    v.iter().fold(Vec3::ZERO, |a, b| a + *b) * (1.0 / v.len().max(1) as f64)
}

fn poly_area(v: &[Vec3]) -> f64 {
    (1..v.len().saturating_sub(1)).map(|i| (v[i] - v[0]).cross(v[i + 1] - v[0]).length() * 0.5).sum()
}

fn seg_distance(p: Vec3, s: [Vec3; 2]) -> f64 {
    let ab = s[1] - s[0];
    let t = if ab.length_sq() < 1e-18 { 0.0 } else { ((p - s[0]).dot(ab) / ab.length_sq()).clamp(0.0, 1.0) };
    (s[0] + ab * t).distance(p)
}

/// The value at half the total weight.
fn weighted_median(mut v: Vec<(f64, f64)>) -> f64 {
    v.sort_by(|a, b| a.0.total_cmp(&b.0));
    let total: f64 = v.iter().map(|s| s.1).sum();
    let mut acc = 0.0;
    for (x, w) in &v {
        acc += w;
        if acc >= 0.5 * total {
            return *x;
        }
    }
    v.last().map(|s| s.0).unwrap_or(0.0)
}

fn is_hole(r: &Round) -> bool {
    r.concave && r.coverage > TAU - 0.2
}

fn mm(v: f64) -> String {
    format!("{v:.2} mm")
}

/// Runs the checks for `process` on `body`.
pub fn check(body: &Solid, material: Option<&Material>, process: Process) -> DfmReport {
    let part = Part::new(body);
    let mut report = DfmReport::default();
    if part.faces.is_empty() {
        return report;
    }
    match process {
        Process::CncMachining => machining(&part, material, &mut report),
        Process::SheetMetal => sheet_metal(&part, material, &mut report),
    }
    // Merge identical findings (the four corners of one pocket, say).
    let mut merged: Vec<Finding> = Vec::new();
    for f in report.findings.drain(..) {
        match merged
            .iter_mut()
            .find(|m| m.severity == f.severity && m.rule == f.rule && m.message == f.message)
        {
            Some(m) => {
                m.count += f.count;
                m.faces.extend(f.faces);
                m.faces.sort();
                m.faces.dedup();
            }
            None => merged.push(f),
        }
    }
    merged.sort_by(|a, b| a.severity.cmp(&b.severity).then(a.rule.cmp(b.rule)));
    report.findings = merged;
    report
}

fn machining(part: &Part, material: Option<&Material>, report: &mut DfmReport) {
    let min_wall = material.map(|m| m.min_wall).filter(|w| *w > 0.0).unwrap_or(DEFAULT_MIN_WALL);
    let what = material.map(|m| m.name.clone()).unwrap_or_else(|| "a machined part".into());

    // Thin walls: the thinnest reading between each pair of facing surfaces.
    let mut thinnest: BTreeMap<(FaceId, FaceId), f64> = BTreeMap::new();
    for s in part.thickness_samples() {
        let key = if s.face <= s.hit { (s.face, s.hit) } else { (s.hit, s.face) };
        let e = thinnest.entry(key).or_insert(f64::INFINITY);
        *e = e.min(s.thickness);
    }
    for ((a, b), t) in thinnest {
        if t < min_wall {
            report.findings.push(Finding {
                severity: if t < 0.5 * min_wall { Severity::Error } else { Severity::Warning },
                rule: "Thin wall",
                message: format!(
                    "Wall is {} thick; {what} needs at least {} to machine without chatter or breaking.",
                    mm(t),
                    mm(min_wall)
                ),
                faces: vec![a, b],
                count: 1,
            });
        }
    }

    // Holes and inside corners.
    for r in part.rounds() {
        if !r.concave {
            continue;
        }
        let d = 2.0 * r.radius;
        if is_hole(&r) {
            if d < MIN_DRILL {
                report.findings.push(Finding {
                    severity: Severity::Warning,
                    rule: "Small hole",
                    message: format!("Hole is {} across; below {} needs micro drills.", mm(d), mm(MIN_DRILL)),
                    faces: vec![r.id],
                    count: 1,
                });
            }
            let ratio = r.length / d;
            if ratio > MAX_REACH {
                report.findings.push(Finding {
                    severity: Severity::Warning,
                    rule: "Deep hole",
                    message: format!(
                        "Hole is {ratio:.1} diameters deep ({} by {}); past {MAX_REACH} it needs gun drilling.",
                        mm(d),
                        mm(r.length)
                    ),
                    faces: vec![r.id],
                    count: 1,
                });
            } else if ratio > STANDARD_REACH {
                report.findings.push(Finding {
                    severity: Severity::Note,
                    rule: "Deep hole",
                    message: format!(
                        "Hole is {ratio:.1} diameters deep; standard drills reach about {STANDARD_REACH}.",
                    ),
                    faces: vec![r.id],
                    count: 1,
                });
            }
        } else {
            // An inside corner radius: the end mill that cuts it is no larger.
            if r.radius < MIN_MILL_RADIUS {
                report.findings.push(Finding {
                    severity: Severity::Warning,
                    rule: "Tight inside radius",
                    message: format!(
                        "Inside radius is {}; below {} needs very small end mills.",
                        mm(r.radius),
                        mm(MIN_MILL_RADIUS)
                    ),
                    faces: vec![r.id],
                    count: 1,
                });
            }
            let ratio = r.length / d;
            if ratio > STANDARD_REACH {
                report.findings.push(Finding {
                    severity: Severity::Warning,
                    rule: "Deep inside corner",
                    message: format!(
                        "A {} radius runs {} deep ({ratio:.1} tool diameters); a larger radius lets a stiffer tool reach.",
                        mm(r.radius),
                        mm(r.length)
                    ),
                    faces: vec![r.id],
                    count: 1,
                });
            }
        }
    }

    // Sharp inside corners that only the side of a tool could reach: concave edges
    // between two flat faces where neither face is open along its own normal (so no
    // tool can come in square to it and cut the corner with its end).
    // Per pair of faces: total length, a point on the edge, and the two normals.
    let mut corners: BTreeMap<(FaceId, FaceId), (f64, Vec3, Vec3, Vec3)> = BTreeMap::new();
    for (seg, i, j) in part.body.shared_segments() {
        let (pi, pj) = (&part.body.polygons[i], &part.body.polygons[j]);
        if pi.tag.id == pj.tag.id
            || pi.tag.surface != SurfaceKind::Plane
            || pj.tag.surface != SurfaceKind::Plane
        {
            continue;
        }
        let mid = (seg[0] + seg[1]) * 0.5;
        // Concave: each polygon's body lies on the outer side of the other's plane.
        let concave = pi.normal.dot(pj.normal) < 0.99 && (centroid(&pj.verts) - mid).dot(pi.normal) > 1e-9;
        if !concave {
            continue;
        }
        let key = if pi.tag.id <= pj.tag.id { (pi.tag.id, pj.tag.id) } else { (pj.tag.id, pi.tag.id) };
        let (ni, nj) = if pi.tag.id <= pj.tag.id { (pi.normal, pj.normal) } else { (pj.normal, pi.normal) };
        let e = corners.entry(key).or_insert((0.0, mid, ni, nj));
        e.0 += (seg[1] - seg[0]).length();
        e.1 = mid;
    }
    for ((a, b), (length, mid, na, nb)) in corners {
        // Step a little onto each face, away from the corner (at a concave corner each
        // face runs off along the other's normal), and look out along its own normal.
        let step = 1e-3;
        if part.open_towards(mid + nb * step, na) || part.open_towards(mid + na * step, nb) {
            continue;
        }
        report.findings.push(Finding {
            severity: Severity::Warning,
            rule: "Sharp inside corner",
            message: format!(
                "Inside corner {} long is sharp, but a rotating tool leaves its own radius here; add a fillet (at least {}, larger for deep pockets).",
                mm(length),
                mm(MIN_MILL_RADIUS.max(length / 6.0))
            ),
            faces: vec![a, b],
            count: 1,
        });
    }
}

fn sheet_metal(part: &Part, material: Option<&Material>, report: &mut DfmReport) {
    let ratio = match material {
        None => {
            report.findings.push(Finding {
                severity: Severity::Note,
                rule: "No material",
                message: "Assign a material to check bend radii against it; using 1x thickness.".into(),
                faces: vec![],
                count: 1,
            });
            1.0
        }
        Some(m) => match (m.class, m.min_bend_ratio) {
            (MaterialClass::Metal, Some(r)) => r,
            _ => {
                report.findings.push(Finding {
                    severity: Severity::Error,
                    rule: "Material",
                    message: format!("{} is not formed as sheet metal.", m.name),
                    faces: vec![],
                    count: 1,
                });
                return;
            }
        },
    };

    // Thickness: the part must be one thickness throughout. Readings taken near a
    // face's boundary are left out: on the sheet's narrow edge faces every point is,
    // and rays from there run along the sheet, not through it.
    let all = part.thickness_samples();
    let rough = weighted_median(all.iter().map(|s| (s.thickness, s.area)).collect());
    let samples: Vec<Sample> = all
        .into_iter()
        .filter(|s| {
            let edge = part
                .neighbours(s.face)
                .flat_map(|(_, segs)| segs.iter())
                .map(|seg| seg_distance(s.point, *seg))
                .fold(f64::INFINITY, f64::min);
            edge > 0.6 * rough
        })
        .collect();
    if samples.is_empty() {
        return;
    }
    let mut sorted: Vec<(f64, f64)> = samples.iter().map(|s| (s.thickness, s.area)).collect();
    sorted.sort_by(|a, b| a.0.total_cmp(&b.0));
    let total: f64 = sorted.iter().map(|s| s.1).sum();
    let t = weighted_median(sorted.clone());
    let off: Vec<&Sample> = samples.iter().filter(|s| (s.thickness - t).abs() > 0.03 * t).collect();
    let off_area: f64 = off.iter().map(|s| s.area).sum();
    if off_area > 0.15 * total {
        let mut faces: Vec<FaceId> = off.iter().flat_map(|s| [s.face, s.hit]).collect();
        faces.sort();
        faces.dedup();
        let (lo, hi) = (sorted[0].0, sorted[sorted.len() - 1].0);
        report.findings.push(Finding {
            severity: Severity::Error,
            rule: "Not one thickness",
            message: format!(
                "Sheet metal is one thickness throughout; this part runs from {} to {} (mostly {}).",
                mm(lo),
                mm(hi),
                mm(t)
            ),
            faces,
            count: 1,
        });
        return;
    }
    report.thickness = Some(t);
    let close = |a: f64, b: f64| (a - b).abs() <= 0.05 * t.max(1e-9);

    // Bends: a concave partial cylinder with a convex one on the same axis, t larger.
    let rounds = part.rounds();
    let mut bends: Vec<(Round, Round)> = Vec::new();
    for inner in rounds.iter().filter(|r| r.concave && !is_hole(r)) {
        let outer = rounds.iter().find(|o| {
            !o.concave
                && o.axis.cross(inner.axis).length() < 1e-3
                && (o.origin - inner.origin).cross(inner.axis).length() < 0.05 * t
                && close(o.radius - inner.radius, t)
        });
        if let Some(o) = outer {
            bends.push((*inner, *o));
        }
    }
    report.bends = bends.len();
    let min_radius = ratio * t;
    for (inner, outer) in &bends {
        if inner.radius + 1e-9 < min_radius {
            report.findings.push(Finding {
                severity: Severity::Error,
                rule: "Tight bend",
                message: format!(
                    "Inside bend radius is {}; {} needs at least {ratio}x thickness ({}) or it may crack.",
                    mm(inner.radius),
                    material.map(|m| m.name.as_str()).unwrap_or("this material"),
                    mm(min_radius)
                ),
                faces: vec![inner.id, outer.id],
                count: 1,
            });
        }
    }
    let bend_faces: Vec<FaceId> = bends.iter().flat_map(|(i, o)| [i.id, o.id]).collect();

    // Flanges: each flat face next to a bend must be long enough for the brake to hold.
    let min_flange = 4.0 * t;
    // Each leg has an inside and an outside face; report it once.
    let mut seen: Vec<(FaceId, Vec3)> = Vec::new();
    for (inner, outer) in &bends {
        for bend in [inner, outer] {
            for (other, segs) in part.neighbours(bend.id) {
                let Some(f) = part.face(other) else { continue };
                if f.kind != SurfaceKind::Plane || bend_faces.contains(&other) {
                    continue;
                }
                // Flanges run on from the bend; the sheet's edge faces are square to its axis.
                let Some(&p0) = f.polys.first() else { continue };
                let n = part.body.polygons[p0].normal;
                if n.dot(bend.axis).abs() > 0.1
                    || seen.iter().any(|(id, m)| *id == inner.id && m.dot(n).abs() > 0.99)
                {
                    continue;
                }
                seen.push((inner.id, n));
                // Flange length: how far the face reaches from the bend line, square to it.
                let Some(line) = segs.first() else { continue };
                let along = bend.axis;
                let length = f
                    .polys
                    .iter()
                    .flat_map(|&pi| part.body.polygons[pi].verts.iter())
                    .map(|v| {
                        let d = *v - line[0];
                        (d - along * d.dot(along)).length()
                    })
                    .fold(0.0, f64::max);
                if length + 1e-9 < min_flange {
                    report.findings.push(Finding {
                        severity: Severity::Warning,
                        rule: "Short flange",
                        message: format!(
                            "Flange reaches {} from its bend; a press brake needs about 4x thickness ({}).",
                            mm(length),
                            mm(min_flange)
                        ),
                        faces: vec![other, inner.id],
                        count: 1,
                    });
                }
            }
        }
    }

    // Holes through the sheet: size, and distance to edges, other holes and bends.
    let min_hole = if material.is_some_and(|m| m.name.contains("Stainless") || m.name.contains("304")) {
        1.2 * t
    } else {
        t
    };
    let holes: Vec<&Round> = rounds.iter().filter(|r| is_hole(r)).collect();
    for h in &holes {
        let d = 2.0 * h.radius;
        if d + 1e-9 < min_hole {
            report.findings.push(Finding {
                severity: Severity::Warning,
                rule: "Small hole",
                message: format!(
                    "Hole is {} across; holes smaller than {} punch poorly and may need drilling.",
                    mm(d),
                    mm(min_hole)
                ),
                faces: vec![h.id],
                count: 1,
            });
        }
        // The flat faces around the hole, and every other edge of those faces.
        let plates: Vec<FaceId> = part
            .neighbours(h.id)
            .filter(|(o, _)| part.face(*o).is_some_and(|f| f.kind == SurfaceKind::Plane))
            .map(|(o, _)| o)
            .collect();
        let mut nearest: Option<(f64, FaceId, bool)> = None;
        for plate in &plates {
            for (other, segs) in part.neighbours(*plate) {
                if other == h.id || plates.contains(&other) {
                    continue;
                }
                let gap =
                    segs.iter().map(|s| seg_distance(h.centre, *s)).fold(f64::INFINITY, f64::min) - h.radius;
                let near_bend = bend_faces.contains(&other);
                if nearest.is_none_or(|n| gap < n.0) {
                    nearest = Some((gap, other, near_bend));
                }
            }
        }
        if let Some((gap, other, near_bend)) = nearest {
            let bend_r = bends.iter().map(|b| b.0.radius).fold(0.0, f64::max);
            let need = if near_bend { 2.5 * t + bend_r } else { 2.0 * t };
            if gap + 1e-9 < need {
                let what = if near_bend { "a bend" } else { "an edge or another hole" };
                report.findings.push(Finding {
                    severity: Severity::Warning,
                    rule: "Hole too close",
                    message: format!(
                        "Hole edge is {} from {what}; keep at least {} so it does not distort.",
                        mm(gap.max(0.0)),
                        mm(need)
                    ),
                    faces: vec![h.id, other],
                    count: 1,
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anvil_kernel::{FaceTag, Polygon};

    /// An axis-aligned box from `lo` to `hi`, each side its own face of `feature`.
    fn cuboid(lo: Vec3, hi: Vec3, feature: u32) -> Solid {
        let c = |x: bool, y: bool, z: bool| {
            v3(if x { hi.x } else { lo.x }, if y { hi.y } else { lo.y }, if z { hi.z } else { lo.z })
        };
        let quads = [
            [c(false, false, false), c(false, true, false), c(true, true, false), c(true, false, false)],
            [c(false, false, true), c(true, false, true), c(true, true, true), c(false, true, true)],
            [c(false, false, false), c(true, false, false), c(true, false, true), c(false, false, true)],
            [c(false, true, false), c(false, true, true), c(true, true, true), c(true, true, false)],
            [c(false, false, false), c(false, false, true), c(false, true, true), c(false, true, false)],
            [c(true, false, false), c(true, true, false), c(true, true, true), c(true, false, true)],
        ];
        let polys = quads
            .iter()
            .enumerate()
            .filter_map(|(i, q)| {
                let tag = FaceTag { id: FaceId { feature, index: i as u32 }, surface: SurfaceKind::Plane };
                Polygon::new(q.to_vec(), tag)
            })
            .collect();
        Solid::new(polys)
    }

    fn material(name: &str) -> Material {
        Material::library().into_iter().find(|m| m.name == name).expect("material")
    }

    #[test]
    fn thin_plate_is_flagged_for_plastic_but_not_steel() {
        let plate = cuboid(v3(0.0, 0.0, 0.0), v3(50.0, 30.0, 1.2), 1);
        let steel = check(&plate, Some(&material("ASTM A36 Steel")), Process::CncMachining);
        assert_eq!(steel.count(Severity::Error) + steel.count(Severity::Warning), 0, "{:?}", steel.findings);
        let delrin = check(&plate, Some(&material("Delrin (Acetal Homopolymer)")), Process::CncMachining);
        let thin: Vec<&Finding> = delrin.findings.iter().filter(|f| f.rule == "Thin wall").collect();
        assert_eq!(thin.len(), 1, "{:?}", delrin.findings);
        assert_eq!(thin[0].faces.len(), 2);
    }

    #[test]
    fn flat_sheet_has_its_thickness_and_plastic_is_refused() {
        let plate = cuboid(v3(0.0, 0.0, 0.0), v3(80.0, 40.0, 2.0), 1);
        let r = check(&plate, Some(&material("AISI 316 Stainless Steel")), Process::SheetMetal);
        assert!((r.thickness.expect("thickness") - 2.0).abs() < 1e-6);
        assert_eq!(r.bends, 0);
        assert_eq!(r.count(Severity::Error), 0, "{:?}", r.findings);
        let r = check(&plate, Some(&material("UHMW Polyethylene")), Process::SheetMetal);
        assert_eq!(r.count(Severity::Error), 1);
    }

    #[test]
    fn block_is_not_one_thickness() {
        // An L made of a thin plate and a thick block.
        let mut polys = cuboid(v3(0.0, 0.0, 0.0), v3(60.0, 40.0, 2.0), 1).polygons;
        polys.extend(cuboid(v3(0.0, 50.0, 0.0), v3(20.0, 70.0, 20.0), 2).polygons);
        let r = check(&Solid::new(polys), Some(&material("ASTM A36 Steel")), Process::SheetMetal);
        assert!(r.findings.iter().any(|f| f.rule == "Not one thickness"), "{:?}", r.findings);
    }

    // ----- Parts built from features -----

    use crate::*;
    use anvil_kernel::v2;

    fn extrude(
        doc: &mut Document,
        plane: PlaneRef,
        sketch: anvil_sketch::Sketch,
        depth: f64,
        op: BodyOp,
    ) -> u32 {
        let mut sf = SketchFeature::new(plane);
        sf.sketch = sketch;
        let sk = doc.add_feature(FeatureKind::Sketch(sf));
        doc.add_feature(FeatureKind::Extrude(ExtrudeFeature {
            sketch: sk,
            end: if depth > 0.0 { EndCondition::Blind } else { EndCondition::ThroughAll },
            depth: Dim::Value(depth.abs()),
            reverse: false,
            op,
            ..Default::default()
        }))
    }

    fn rect(w: f64, h: f64, cx: f64, cy: f64) -> anvil_sketch::Sketch {
        let mut s = anvil_sketch::Sketch::default();
        s.add_rectangle(v2(cx - w / 2.0, cy - h / 2.0), v2(cx + w / 2.0, cy + h / 2.0));
        s
    }

    fn circles(list: &[(f64, f64, f64)]) -> anvil_sketch::Sketch {
        let mut s = anvil_sketch::Sketch::default();
        for &(x, y, d) in list {
            let c = s.add_point(v2(x, y));
            s.add_circle(c, d / 2.0);
        }
        s
    }

    fn built(doc: &mut Document) -> Solid {
        let st = doc.rebuild();
        assert!(st.errors.is_empty(), "{:?}", st.errors);
        st.body
    }

    #[test]
    fn pocket_corners_and_a_deep_hole() {
        let mut doc = Document::new();
        let block = extrude(&mut doc, PlaneRef::Top, rect(60.0, 40.0, 0.0, 0.0), 30.0, BodyOp::Boss);
        let top = PlaneRef::Face(FaceId { feature: block, index: FaceId::END_CAP });
        extrude(&mut doc, top, rect(30.0, 20.0, -10.0, 0.0), 10.0, BodyOp::Cut);
        extrude(&mut doc, top, circles(&[(20.0, 10.0, 2.0)]), -1.0, BodyOp::Cut);
        let body = built(&mut doc);
        let r = check(&body, Some(&material("6061 Alloy")), Process::CncMachining);
        let corners: usize =
            r.findings.iter().filter(|f| f.rule == "Sharp inside corner").map(|f| f.count).sum();
        // The pocket's four upright corners; its floor edges are cut by the tool's end.
        assert_eq!(corners, 4, "{:#?}", r.findings);
        let deep: Vec<&Finding> = r.findings.iter().filter(|f| f.rule == "Deep hole").collect();
        assert_eq!(deep.len(), 1, "{:#?}", r.findings);
        assert_eq!(deep[0].severity, Severity::Warning);
        assert!(r.findings.iter().all(|f| f.rule != "Thin wall"), "{:#?}", r.findings);
    }

    /// An L bracket bent from `t` thick sheet with inside radius `radius`, its legs
    /// reaching `leg` from the outside corner, `width` wide.
    fn bracket(t: f64, radius: f64, leg: f64, width: f64) -> Document {
        let ro = radius + t;
        let mut s = anvil_sketch::Sketch::default();
        let c = s.add_point(v2(ro, ro));
        let p = |s: &mut anvil_sketch::Sketch, x: f64, y: f64| s.add_point(v2(x, y));
        let (o_left, o_bottom) = (p(&mut s, 0.0, ro), p(&mut s, ro, 0.0));
        let (i_left, i_bottom) = (p(&mut s, t, ro), p(&mut s, ro, t));
        let (b1, b2) = (p(&mut s, leg, 0.0), p(&mut s, leg, t));
        let (v1, v2_) = (p(&mut s, t, leg), p(&mut s, 0.0, leg));
        s.add_arc(c, o_left, o_bottom);
        s.add_line(o_bottom, b1);
        s.add_line(b1, b2);
        s.add_line(b2, i_bottom);
        s.add_arc(c, i_left, i_bottom);
        s.add_line(i_left, v1);
        s.add_line(v1, v2_);
        s.add_line(v2_, o_left);
        let mut doc = Document::new();
        extrude(&mut doc, PlaneRef::Front, s, width, BodyOp::Boss);
        doc
    }

    #[test]
    fn bracket_bend_radius_and_flanges() {
        let steel = material("ASTM A36 Steel");
        let mut good = bracket(2.0, 2.0, 40.0, 30.0);
        let r = check(&built(&mut good), Some(&steel), Process::SheetMetal);
        assert!((r.thickness.expect("thickness") - 2.0).abs() < 0.05, "{r:#?}");
        assert_eq!(r.bends, 1, "{r:#?}");
        assert_eq!(r.count(Severity::Error) + r.count(Severity::Warning), 0, "{:#?}", r.findings);
        // 6061-T6 wants 2.5x thickness: the same bend is too tight for it.
        let r = check(&built(&mut good), Some(&material("6061 Alloy")), Process::SheetMetal);
        assert!(
            r.findings.iter().any(|f| f.rule == "Tight bend" && f.severity == Severity::Error),
            "{:#?}",
            r.findings
        );
        // Legs only 5 mm past the bend are too short to hold in a brake.
        let mut short = bracket(2.0, 2.0, 9.0, 30.0);
        let r = check(&built(&mut short), Some(&steel), Process::SheetMetal);
        let short: usize = r.findings.iter().filter(|f| f.rule == "Short flange").map(|f| f.count).sum();
        assert_eq!(short, 2, "{:#?}", r.findings);
    }

    #[test]
    fn holes_near_an_edge_and_too_small_to_punch() {
        let mut doc = Document::new();
        let plate = extrude(&mut doc, PlaneRef::Top, rect(80.0, 40.0, 0.0, 0.0), 2.0, BodyOp::Boss);
        let top = PlaneRef::Face(FaceId { feature: plate, index: FaceId::END_CAP });
        extrude(
            &mut doc,
            top,
            circles(&[(37.0, 0.0, 3.0), (0.0, 0.0, 1.0), (-20.0, 0.0, 6.0)]),
            -1.0,
            BodyOp::Cut,
        );
        let r = check(&built(&mut doc), Some(&material("ASTM A36 Steel")), Process::SheetMetal);
        let rules: Vec<&str> = r.findings.iter().map(|f| f.rule).collect();
        assert_eq!(rules.iter().filter(|r| **r == "Hole too close").count(), 1, "{:#?}", r.findings);
        assert_eq!(rules.iter().filter(|r| **r == "Small hole").count(), 1, "{:#?}", r.findings);
    }

    #[test]
    fn block_inertia_matches_the_formula() {
        let mut doc = Document::new();
        extrude(&mut doc, PlaneRef::Top, rect(40.0, 20.0, 0.0, 0.0), 10.0, BodyOp::Boss);
        doc.material = Some(material("6061 Alloy"));
        let st = doc.rebuild();
        let m = st.mass_report(&doc);
        let mass = m.mass.expect("mass");
        assert!((mass - 21.6).abs() < 1e-9, "{mass}");
        let p = m.principal.expect("principal moments");
        let want =
            [mass / 12.0 * (400.0 + 100.0), mass / 12.0 * (1600.0 + 100.0), mass / 12.0 * (1600.0 + 400.0)];
        for (a, b) in p.iter().zip(want) {
            assert!((a - b).abs() / b < 1e-9, "{p:?} vs {want:?}");
        }
    }
}
