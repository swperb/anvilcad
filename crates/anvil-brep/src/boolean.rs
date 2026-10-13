//! Boolean operations (union, subtract, intersect) on B-rep solids.
//!
//! The algorithm, in order:
//!
//! 1. Vertices of both solids go into one pool that merges points closer
//!    than [`TOL`], so geometry the solids share gets a single identity.
//! 2. Every edge of each solid is intersected with every face of the other;
//!    the crossing points join the pool and split the edges.
//! 3. For every pair of faces, the intersection curves of their surfaces are
//!    cut at the pooled vertices lying on both faces. Pieces inside both faces
//!    become new edges ("cuts"). Coplanar faces imprint each other's
//!    boundaries instead.
//! 4. Each face is split by its cuts: a half-edge arrangement is traced in
//!    the face's parameter space, giving new outer loops and holes.
//! 5. Every piece is classified as inside, outside or on the other solid,
//!    the pieces the operation keeps are collected, and the result is checked
//!    and split into connected bodies.
//!
//! Anything the algorithm cannot handle is reported as an error; it never
//! returns a solid that fails [`check::validate`].

use crate::check;
use crate::geom::{Curve, Surface};
use crate::intersect::{self, CurveHit, Ssi};
use crate::profile::point_in_polygon;
use crate::topo::{Coedge, Edge, EdgeId, Face, Loop, Solid, VertexId};
use crate::{KernelError, Result};
use glam::{DVec2, DVec3};
use spade::{ConstrainedDelaunayTriangulation, Point2, Triangulation};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::f64::consts::TAU;

/// Points closer than this are the same point.
const TOL: f64 = 1e-6;
/// Distance within which a point counts as lying on a curve or surface.
const ON: f64 = 1e-5;
/// Sampling used to build parameter-space polylines.
const CHORD: f64 = 0.02;
const MAX_ANGLE: f64 = TAU / 96.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoolOp {
    Union,
    /// The first solid minus the second.
    Subtract,
    Intersect,
}

fn fail<T>(msg: impl Into<String>) -> Result<T> {
    Err(KernelError::Boolean(msg.into()))
}

/// Combines two solids. The result may contain several bodies (or none).
pub fn boolean(a: &Solid, b: &Solid, op: BoolOp) -> Result<Vec<Solid>> {
    let (Some(ba), Some(bb)) = (a.bounds(), b.bounds()) else {
        return fail("empty solid");
    };
    if !boxes_overlap(ba, bb, ON) {
        return Ok(match op {
            BoolOp::Union => vec![a.clone(), b.clone()],
            BoolOp::Subtract => vec![a.clone()],
            BoolOp::Intersect => Vec::new(),
        });
    }
    Engine::new([a, b])?.run(op)
}

fn boxes_overlap(a: (DVec3, DVec3), b: (DVec3, DVec3), pad: f64) -> bool {
    a.0.x <= b.1.x + pad
        && b.0.x <= a.1.x + pad
        && a.0.y <= b.1.y + pad
        && b.0.y <= a.1.y + pad
        && a.0.z <= b.1.z + pad
        && b.0.z <= a.1.z + pad
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Loc {
    Inside,
    Boundary,
    Outside,
}

/// Precomputed data for point-in-face tests.
struct FaceInfo {
    /// Boundary loops in raw surface parameter space, unwrapped on
    /// cylinders.
    loops_uv: Vec<Vec<DVec2>>,
    u_range: (f64, f64),
    bbox: (DVec3, DVec3),
}

/// An edge in the shared pool.
#[derive(Debug, Clone)]
struct PEdge {
    curve: Curve,
    t0: f64,
    t1: f64,
    v0: usize,
    v1: usize,
}

impl PEdge {
    fn at(&self, f: f64) -> DVec3 {
        self.curve.eval(self.t0 + (self.t1 - self.t0) * f)
    }
}

/// A use of a pooled edge: (edge, reversed).
type Use = (usize, bool);

/// A vertex in a face's parameter space: (pooled vertex, period branch).
type Node = (usize, i64);

/// A kept piece: (piece index, outer loop, inner loops, flipped).
type KeptFace = (usize, Vec<Use>, Vec<Vec<Use>>, bool);

/// A face produced by splitting.
struct Piece {
    solid: usize,
    face: usize,
    outer: Vec<Use>,
    inners: Vec<Vec<Use>>,
    /// Loops in raw surface parameter space, for finding an interior point.
    uv: Vec<Vec<DVec2>>,
}

struct Engine<'a> {
    solids: [&'a Solid; 2],
    infos: [Vec<FaceInfo>; 2],
    pts: Vec<DVec3>,
    vmap: [Vec<usize>; 2],
    edges: Vec<PEdge>,
    /// Per solid edge: the pooled pieces in the edge's direction.
    pieces: [Vec<Vec<Use>>; 2],
    /// Per face: pooled edges cutting through its interior.
    cuts: [Vec<BTreeSet<usize>>; 2],
}

fn unwrap(prev: f64, u: f64, period: f64) -> f64 {
    u + period * ((prev - u) / period).round()
}

fn face_info(solid: &Solid, face: &Face) -> FaceInfo {
    let period = face.surface.u_period();
    let mut loops_uv = Vec::new();
    let mut lo = DVec3::splat(f64::INFINITY);
    let mut hi = DVec3::splat(f64::NEG_INFINITY);
    let mut umin = f64::INFINITY;
    let mut umax = f64::NEG_INFINITY;
    for l in face.loops() {
        let mut uv: Vec<DVec2> = Vec::new();
        for c in &l.coedges {
            let Some(e) = solid.edge(c.edge) else {
                continue;
            };
            let mut pts = e.curve.sample(e.t0, e.t1, CHORD, MAX_ANGLE);
            if c.reversed {
                pts.reverse();
            }
            pts.pop();
            for p in pts {
                lo = lo.min(p);
                hi = hi.max(p);
                let mut q = face.surface.uv(p);
                if let (Some(per), Some(prev)) = (period, uv.last()) {
                    q.x = unwrap(prev.x, q.x, per);
                }
                umin = umin.min(q.x);
                umax = umax.max(q.x);
                uv.push(q);
            }
        }
        loops_uv.push(uv);
    }
    if let Some(per) = period {
        crate::tess::align_holes(&mut loops_uv, per);
        // The range of u moves with the holes.
        (umin, umax) = loops_uv
            .iter()
            .flatten()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), p| (a.min(p.x), b.max(p.x)));
    }
    // The box is only a quick rejection test, so it errs wide: sampled curves fall
    // short of their true extremes (by r(1 - cos(step / 2))), and a torus face bulges
    // past its boundary (a fillet's crown, say) by up to its tube radius.
    let pad = match face.surface {
        Surface::Torus { minor, .. } => minor,
        _ => 0.0,
    } + 0.01 * (hi - lo).length()
        + ON;
    FaceInfo { loops_uv, u_range: (umin, umax), bbox: (lo - DVec3::splat(pad), hi + DVec3::splat(pad)) }
}

impl<'a> Engine<'a> {
    fn new(solids: [&'a Solid; 2]) -> Result<Self> {
        let infos = solids.map(|s| s.faces.iter().map(|f| face_info(s, f)).collect());
        let mut eng = Self {
            solids,
            infos,
            pts: Vec::new(),
            vmap: [Vec::new(), Vec::new()],
            edges: Vec::new(),
            pieces: [Vec::new(), Vec::new()],
            cuts: [
                vec![BTreeSet::new(); solids[0].faces.len()],
                vec![BTreeSet::new(); solids[1].faces.len()],
            ],
        };
        for (s, solid) in solids.iter().enumerate() {
            let ids = solid.vertices.iter().map(|v| eng.add_point(v.point)).collect();
            eng.vmap[s] = ids;
        }
        Ok(eng)
    }

    fn add_point(&mut self, p: DVec3) -> usize {
        if let Some(i) = self.pts.iter().position(|q| (*q - p).length() < TOL) {
            return i;
        }
        self.pts.push(p);
        self.pts.len() - 1
    }

    /// Finds or creates a pooled edge. Returns the edge and whether the
    /// requested direction is opposite to the stored one.
    fn add_edge(&mut self, curve: Curve, t0: f64, t1: f64, v0: usize, v1: usize) -> Use {
        let new = PEdge { curve, t0, t1, v0, v1 };
        let q1 = new.at(0.25);
        let mid = new.at(0.5);
        for (i, e) in self.edges.iter().enumerate() {
            let same_ends = (e.v0 == v0 && e.v1 == v1) || (e.v0 == v1 && e.v1 == v0);
            if !same_ends || (e.at(0.5) - mid).length() > ON {
                continue;
            }
            if (e.at(0.25) - q1).length() < ON {
                return (i, false);
            }
            if (e.at(0.75) - q1).length() < ON {
                return (i, true);
            }
        }
        self.edges.push(new);
        (self.edges.len() - 1, false)
    }

    fn surface(&self, s: usize, f: usize) -> &Surface {
        &self.solids[s].faces[f].surface
    }

    /// Where a point lies relative to a face.
    fn locate(&self, s: usize, f: usize, p: DVec3) -> Loc {
        locate_in(self.solids[s], f, &self.infos[s][f], p)
    }

    fn run(mut self, op: BoolOp) -> Result<Vec<Solid>> {
        self.intersect_edges_with_faces();
        self.split_edges();
        self.intersect_faces()?;
        let mut pieces = self.split_faces()?;
        let mut keep = self.select(&pieces, op)?;
        self.heal_whole_faces(&mut pieces, &mut keep);
        self.assemble(&pieces, &keep)
    }

    /// Step 2: crossings of each solid's edges with the other's faces.
    fn intersect_edges_with_faces(&mut self) {
        let mut found = Vec::new();
        for s in 0..2 {
            let o = 1 - s;
            for e in &self.solids[s].edges {
                for (g, face) in self.solids[o].faces.iter().enumerate() {
                    let (lo, hi) = self.infos[o][g].bbox;
                    let ebox = edge_box(e);
                    if !boxes_overlap(ebox, (lo, hi), ON) {
                        continue;
                    }
                    if let CurveHit::Params(ts) =
                        intersect::curve_surface(&e.curve, e.t0, e.t1, &face.surface, TOL)
                    {
                        for t in ts {
                            let p = e.curve.eval(t);
                            if self.locate(o, g, p) != Loc::Outside {
                                found.push(p);
                            }
                        }
                    }
                }
            }
        }
        for p in found {
            self.add_point(p);
        }
    }

    /// Splits every edge at the pooled points lying on it.
    fn split_edges(&mut self) {
        for s in 0..2 {
            let solid = self.solids[s];
            let mut all = Vec::with_capacity(solid.edges.len());
            for e in &solid.edges {
                let v0 = self.vmap[s][e.start.index()];
                let v1 = self.vmap[s][e.end.index()];
                let dir = (e.t1 - e.t0).signum();
                let mut cuts: Vec<(f64, usize)> = self
                    .pts
                    .iter()
                    .enumerate()
                    .filter(|(i, p)| {
                        *i != v0 && *i != v1 && e.curve.distance_to(**p, e.t0, e.t1) < TOL * 10.0
                    })
                    .map(|(i, p)| (e.curve.param_in_range(*p, e.t0, e.t1), i))
                    .filter(|(t, _)| (t - e.t0) * dir > 0.0 && (e.t1 - t) * dir > 0.0)
                    .collect();
                cuts.sort_by(|a, b| ((a.0 - b.0) * dir).total_cmp(&0.0));
                let mut list = Vec::new();
                let (mut t, mut v) = (e.t0, v0);
                for (tc, vc) in cuts.into_iter().chain(std::iter::once((e.t1, v1))) {
                    list.push(self.add_edge(e.curve.clone(), t, tc, v, vc));
                    t = tc;
                    v = vc;
                }
                all.push(list);
            }
            self.pieces[s] = all;
        }
    }

    /// The pooled pieces of a face's boundary, loop by loop, in use order.
    fn boundary(&self, s: usize, f: usize) -> Vec<Vec<Use>> {
        self.solids[s].faces[f]
            .loops()
            .map(|l| {
                let mut out = Vec::new();
                for c in &l.coedges {
                    let ps = &self.pieces[s][c.edge.index()];
                    if c.reversed {
                        out.extend(ps.iter().rev().map(|(e, r)| (*e, !r)));
                    } else {
                        out.extend(ps.iter().copied());
                    }
                }
                out
            })
            .collect()
    }

    /// Step 3: intersection pieces between every pair of faces.
    fn intersect_faces(&mut self) -> Result<()> {
        let (na, nb) = (self.solids[0].faces.len(), self.solids[1].faces.len());
        for fa in 0..na {
            for fb in 0..nb {
                if !boxes_overlap(self.infos[0][fa].bbox, self.infos[1][fb].bbox, ON) {
                    continue;
                }
                let ssi = intersect::surfaces(self.surface(0, fa), self.surface(1, fb), TOL)?;
                match ssi {
                    Ssi::None => {}
                    Ssi::Coincident => self.imprint_coincident(fa, fb)?,
                    Ssi::Curves(curves) => {
                        for c in curves {
                            self.cut_along(fa, fb, c);
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn imprint_coincident(&mut self, fa: usize, fb: usize) -> Result<()> {
        let planar = matches!(self.surface(0, fa), Surface::Plane { .. });
        for (s, f, o, g) in [(1, fb, 0, fa), (0, fa, 1, fb)] {
            let uses: Vec<Use> = self.boundary(s, f).into_iter().flatten().collect();
            for (e, _) in uses {
                let mid = self.edges[e].at(0.5);
                if self.locate(o, g, mid) == Loc::Inside {
                    if !planar {
                        return Err(KernelError::Unsupported("overlapping coincident curved faces".into()));
                    }
                    self.cuts[o][g].insert(e);
                }
            }
        }
        Ok(())
    }

    fn cut_along(&mut self, fa: usize, fb: usize, curve: Curve) {
        let mut verts: Vec<(f64, usize)> = self
            .pts
            .iter()
            .enumerate()
            .filter(|(_, p)| {
                curve.distance_to_curve(**p) < ON
                    && self.locate(0, fa, **p) != Loc::Outside
                    && self.locate(1, fb, **p) != Loc::Outside
            })
            .map(|(i, p)| (curve.param_of(*p), i))
            .collect();
        verts.sort_by(|a, b| a.0.total_cmp(&b.0));
        verts.dedup_by(|a, b| a.1 == b.1);

        let mut spans: Vec<(f64, f64, usize, usize)> = Vec::new();
        if let Some(period) = curve.period() {
            if verts.is_empty() {
                // A closed curve that touches no edge: give it a vertex.
                let p = curve.eval(0.0);
                if self.locate(0, fa, p) == Loc::Inside && self.locate(1, fb, p) == Loc::Inside {
                    let v = self.add_point(p);
                    let t = curve.param_of(p);
                    spans.push((t, t + period, v, v));
                }
            } else {
                for i in 0..verts.len() {
                    let (t, v) = verts[i];
                    let (mut tn, vn) = verts[(i + 1) % verts.len()];
                    if tn <= t {
                        tn += period;
                    }
                    spans.push((t, tn, v, vn));
                }
            }
        } else {
            for w in verts.windows(2) {
                if w[0].1 != w[1].1 {
                    spans.push((w[0].0, w[1].0, w[0].1, w[1].1));
                }
            }
        }

        for (t0, t1, v0, v1) in spans {
            let mid = curve.eval(0.5 * (t0 + t1));
            let la = self.locate(0, fa, mid);
            let lb = self.locate(1, fb, mid);
            if la == Loc::Outside || lb == Loc::Outside {
                continue;
            }
            if la == Loc::Boundary && lb == Loc::Boundary {
                continue;
            }
            let (e, _) = self.add_edge(curve.clone(), t0, t1, v0, v1);
            if la == Loc::Inside {
                self.cuts[0][fa].insert(e);
            }
            if lb == Loc::Inside {
                self.cuts[1][fb].insert(e);
            }
        }
    }

    /// Step 4: split each face by its cuts.
    fn split_faces(&self) -> Result<Vec<Piece>> {
        let mut out = Vec::new();
        for s in 0..2 {
            for f in 0..self.solids[s].faces.len() {
                let boundary = self.boundary(s, f);
                let on_boundary: BTreeSet<usize> = boundary.iter().flatten().map(|(e, _)| *e).collect();
                let cuts: Vec<usize> =
                    self.cuts[s][f].iter().copied().filter(|e| !on_boundary.contains(e)).collect();
                if cuts.is_empty() {
                    let mut loops = boundary.into_iter();
                    out.push(Piece {
                        solid: s,
                        face: f,
                        outer: loops.next().unwrap_or_default(),
                        inners: loops.collect(),
                        uv: self.infos[s][f].loops_uv.clone(),
                    });
                } else {
                    out.extend(self.arrange(s, f, boundary, cuts)?);
                }
            }
        }
        Ok(out)
    }

    /// Polyline of a pooled edge from v0 to v1, ends pinned to the pool.
    fn polyline(&self, e: usize) -> Vec<DVec3> {
        let pe = &self.edges[e];
        let mut pts = pe.curve.sample(pe.t0, pe.t1, CHORD, MAX_ANGLE);
        if let Some(first) = pts.first_mut() {
            *first = self.pts[pe.v0];
        }
        if let Some(last) = pts.last_mut() {
            *last = self.pts[pe.v1];
        }
        pts
    }

    /// Traces the faces of the arrangement of a face's boundary and cuts.
    fn arrange(&self, s: usize, f: usize, boundary: Vec<Vec<Use>>, cuts: Vec<usize>) -> Result<Vec<Piece>> {
        let face = &self.solids[s].faces[f];
        let surf = &face.surface;
        let period = surf.u_period();
        let flip = if face.reversed { -1.0 } else { 1.0 };
        // Face parameter space: counter-clockwise is counter-clockwise about
        // the face's outward normal.
        let to_uv = |p: DVec3| {
            let q = surf.uv(p);
            DVec2::new(q.x, q.y * flip)
        };

        struct Half {
            edge: usize,
            reversed: bool,
            uv: Vec<DVec2>,
            from: (usize, i64),
            to: (usize, i64),
        }
        let principal = |v: usize| to_uv(self.pts[v]).x;
        let node = |v: usize, u: f64| -> (usize, i64) {
            match period {
                Some(per) => (v, ((u - principal(v)) / per).round() as i64),
                None => (v, 0),
            }
        };

        let mut halves: Vec<Half> = Vec::new();
        let (mut umin, mut umax) = (f64::INFINITY, f64::NEG_INFINITY);
        for l in &boundary {
            let mut prev: Option<f64> = None;
            for &(e, rev) in l {
                let mut pts = self.polyline(e);
                if rev {
                    pts.reverse();
                }
                let mut uv: Vec<DVec2> = Vec::with_capacity(pts.len());
                for p in pts {
                    let mut q = to_uv(p);
                    if let (Some(per), Some(pu)) = (period, uv.last().map(|x: &DVec2| x.x).or(prev)) {
                        q.x = unwrap(pu, q.x, per);
                    }
                    uv.push(q);
                }
                prev = uv.last().map(|q| q.x);
                for q in &uv {
                    umin = umin.min(q.x);
                    umax = umax.max(q.x);
                }
                let (v0, v1) = if rev {
                    (self.edges[e].v1, self.edges[e].v0)
                } else {
                    (self.edges[e].v0, self.edges[e].v1)
                };
                let (a, b) = (uv[0].x, uv[uv.len() - 1].x);
                halves.push(Half { edge: e, reversed: rev, from: node(v0, a), to: node(v1, b), uv });
            }
        }
        for &e in &cuts {
            let pts = self.polyline(e);
            let mut uv: Vec<DVec2> = Vec::with_capacity(pts.len());
            for p in pts {
                let mut q = to_uv(p);
                if let (Some(per), Some(last)) = (period, uv.last()) {
                    q.x = unwrap(last.x, q.x, per);
                }
                uv.push(q);
            }
            if let Some(per) = period {
                let mid = uv[uv.len() / 2].x;
                let center = 0.5 * (umin + umax);
                let mut best = 0.0;
                let mut best_err = f64::INFINITY;
                for k in -3..=3 {
                    let m = mid + per * k as f64;
                    let err = if m < umin - ON {
                        umin - m
                    } else if m > umax + ON {
                        m - umax
                    } else {
                        (m - center).abs() * 1e-9
                    };
                    if err < best_err {
                        best_err = err;
                        best = per * k as f64;
                    }
                }
                for q in uv.iter_mut() {
                    q.x += best;
                }
            }
            let pe = &self.edges[e];
            let (a, b) = (uv[0].x, uv[uv.len() - 1].x);
            let rev_uv: Vec<DVec2> = uv.iter().rev().copied().collect();
            halves.push(Half { edge: e, reversed: false, from: node(pe.v0, a), to: node(pe.v1, b), uv });
            halves.push(Half {
                edge: e,
                reversed: true,
                from: node(pe.v1, b),
                to: node(pe.v0, a),
                uv: rev_uv,
            });
        }

        // Drop dangling cuts: cut edges with an end that nothing else
        // touches cannot bound a region.
        loop {
            let mut touch: HashMap<(usize, i64), BTreeSet<usize>> = HashMap::new();
            for h in &halves {
                touch.entry(h.from).or_default().insert(h.edge);
                touch.entry(h.to).or_default().insert(h.edge);
            }
            let dangling: BTreeSet<usize> = halves
                .iter()
                .filter(|h| h.from != h.to && cuts.contains(&h.edge))
                .filter(|h| [h.from, h.to].iter().any(|n| touch[n].len() == 1))
                .map(|h| h.edge)
                .collect();
            if dangling.is_empty() {
                break;
            }
            halves.retain(|h| !dangling.contains(&h.edge));
        }

        let dir_angle = |uv: &[DVec2]| -> (f64, f64) {
            let d = uv[1.min(uv.len() - 1)] - uv[0];
            let turn = if uv.len() > 2 { (uv[1] - uv[0]).perp_dot(uv[2] - uv[1]).signum() } else { 0.0 };
            (d.y.atan2(d.x), turn)
        };
        // Per node: (half-edge, leaving angle, turn direction).
        let mut outgoing: HashMap<Node, Vec<(usize, f64, f64)>> = HashMap::new();
        for (i, h) in halves.iter().enumerate() {
            let (a, t) = dir_angle(&h.uv);
            outgoing.entry(h.from).or_default().push((i, a, t));
        }

        let mut used = vec![false; halves.len()];
        let mut cycles: Vec<Vec<usize>> = Vec::new();
        for start in 0..halves.len() {
            if used[start] {
                continue;
            }
            let mut cycle = Vec::new();
            let mut h = start;
            loop {
                if used[h] {
                    if h == start {
                        break;
                    }
                    return fail("face split did not close");
                }
                used[h] = true;
                cycle.push(h);
                let half = &halves[h];
                let back: Vec<DVec2> = half.uv.iter().rev().copied().collect();
                let (twin_angle, twin_turn) = dir_angle(&back);
                let Some(cands) = outgoing.get(&half.to) else {
                    return fail("face split reached a dead end");
                };
                let mut best: Option<(usize, f64, f64)> = None;
                for &(c, ang, turn) in cands {
                    let other = &halves[c];
                    if other.edge == half.edge && other.reversed != half.reversed && cands.len() > 1 {
                        continue;
                    }
                    let mut diff = (twin_angle - ang).rem_euclid(TAU);
                    if diff < 1e-9 {
                        // Leaving along the same tangent: the one curving
                        // right comes first clockwise.
                        diff = if turn < twin_turn { 1e-9 } else { TAU - 1e-9 };
                    }
                    if best.is_none_or(|(_, d, _)| diff < d) {
                        best = Some((c, diff, turn));
                    }
                }
                let Some((next, _, _)) = best else {
                    return fail("face split found no way on");
                };
                h = next;
                if cycle.len() > halves.len() + 1 {
                    return fail("face split looped");
                }
            }
            cycles.push(cycle);
        }

        // Outer loops are counter-clockwise, holes clockwise.
        let mut outers: Vec<(Vec<usize>, Vec<DVec2>, f64)> = Vec::new();
        let mut holes: Vec<(Vec<usize>, Vec<DVec2>)> = Vec::new();
        for c in cycles {
            let poly: Vec<DVec2> = c
                .iter()
                .flat_map(|&h| {
                    let uv = &halves[h].uv;
                    uv[..uv.len() - 1].to_vec()
                })
                .collect();
            let area = signed_area(&poly);
            if area.abs() < 1e-12 {
                continue;
            }
            if area > 0.0 {
                outers.push((c, poly, area));
            } else {
                holes.push((c, poly));
            }
        }
        let mut pieces: Vec<Piece> = Vec::new();
        let unflip =
            |poly: &[DVec2]| -> Vec<DVec2> { poly.iter().map(|p| DVec2::new(p.x, p.y * flip)).collect() };
        let uses =
            |c: &[usize]| -> Vec<Use> { c.iter().map(|&h| (halves[h].edge, halves[h].reversed)).collect() };
        for (c, poly, _) in &outers {
            pieces.push(Piece {
                solid: s,
                face: f,
                outer: uses(c),
                inners: Vec::new(),
                uv: vec![unflip(poly)],
            });
        }
        for (c, poly) in holes {
            // Holes run clockwise, so the face enclosing a hole lies just to
            // the left of its edges. Probing on the edge itself would be
            // ambiguous when the hole's twin is another piece's outer loop.
            let (a, b) = (poly[0], poly[1.min(poly.len() - 1)]);
            let d = b - a;
            let len = d.length().max(1e-12);
            let probe = (a + b) * 0.5 + d.perp() / len * (len * 0.01).min(1e-4);
            let host = outers
                .iter()
                .enumerate()
                .filter(|(_, (_, op, _))| point_in_polygon(probe, op))
                .min_by(|a, b| a.1.2.total_cmp(&b.1.2))
                .map(|(i, _)| i);
            let Some(i) = host else {
                return fail("a hole has no enclosing loop");
            };
            pieces[i].inners.push(uses(&c));
            pieces[i].uv.push(unflip(&poly));
        }
        Ok(pieces)
    }

    /// Step 5: decide which pieces the operation keeps, and whether each is
    /// flipped.
    fn select(&self, pieces: &[Piece], op: BoolOp) -> Result<Vec<(usize, bool)>> {
        let mut keep = Vec::new();
        for (i, piece) in pieces.iter().enumerate() {
            let face = &self.solids[piece.solid].faces[piece.face];
            let Some(uv) = interior_point(&piece.uv) else {
                continue; // Degenerate sliver.
            };
            let p = face.surface.eval_uv(uv);
            let o = 1 - piece.solid;
            let class = self.classify(o, p, face.normal_at(p))?;
            let first = piece.solid == 0;
            use Class::*;
            let k = match (op, class, first) {
                (BoolOp::Union, Out, _) => Some(false),
                (BoolOp::Union, OnSame, true) => Some(false),
                (BoolOp::Intersect, In, _) => Some(false),
                (BoolOp::Intersect, OnSame, true) => Some(false),
                (BoolOp::Subtract, Out, true) => Some(false),
                (BoolOp::Subtract, OnOpposite, true) => Some(false),
                (BoolOp::Subtract, In, false) => Some(true),
                _ => None,
            };
            if let Some(flip) = k {
                keep.push((i, flip));
            }
        }
        Ok(keep)
    }

    /// A face that was split but whose pieces all survive the same way is
    /// put back whole, so the result carries no leftover seams.
    fn heal_whole_faces(&self, pieces: &mut Vec<Piece>, keep: &mut Vec<(usize, bool)>) {
        let mut total: HashMap<(usize, usize), usize> = HashMap::new();
        for p in pieces.iter() {
            *total.entry((p.solid, p.face)).or_default() += 1;
        }
        let mut kept: HashMap<(usize, usize), Vec<(usize, bool)>> = HashMap::new();
        for &(i, flip) in keep.iter() {
            kept.entry((pieces[i].solid, pieces[i].face)).or_default().push((i, flip));
        }
        for ((s, f), list) in kept {
            let n = total[&(s, f)];
            if n < 2 || list.len() != n || list.iter().any(|(_, fl)| *fl != list[0].1) {
                continue;
            }
            let flip = list[0].1;
            keep.retain(|(i, _)| !list.iter().any(|(j, _)| j == i));
            let mut loops = self.boundary(s, f).into_iter();
            pieces.push(Piece {
                solid: s,
                face: f,
                outer: loops.next().unwrap_or_default(),
                inners: loops.collect(),
                uv: self.infos[s][f].loops_uv.clone(),
            });
            keep.push((pieces.len() - 1, flip));
        }
    }

    fn classify(&self, o: usize, p: DVec3, normal: DVec3) -> Result<Class> {
        for (g, face) in self.solids[o].faces.iter().enumerate() {
            if face.surface.distance(p) < ON && self.locate(o, g, p) != Loc::Outside {
                return Ok(if face.normal_at(p).dot(normal) > 0.0 {
                    Class::OnSame
                } else {
                    Class::OnOpposite
                });
            }
        }
        let solid = self.solids[o];
        let inside = point_in_solid(solid, p, |g, q| self.locate(o, g, q))?;
        Ok(if inside { Class::In } else { Class::Out })
    }

    fn assemble(&self, pieces: &[Piece], keep: &[(usize, bool)]) -> Result<Vec<Solid>> {
        // Count uses of each pooled edge in each direction.
        let mut fwd: HashMap<usize, u32> = HashMap::new();
        let mut bwd: HashMap<usize, u32> = HashMap::new();
        let oriented = |uses: &[Use], flip: bool| -> Vec<Use> {
            if flip { uses.iter().rev().map(|(e, r)| (*e, !r)).collect() } else { uses.to_vec() }
        };
        let mut faces: Vec<KeptFace> = Vec::new();
        for &(i, flip) in keep {
            let p = &pieces[i];
            let outer = oriented(&p.outer, flip);
            let inners: Vec<Vec<Use>> = p.inners.iter().map(|l| oriented(l, flip)).collect();
            for (e, r) in outer.iter().chain(inners.iter().flatten()) {
                *if *r { &mut bwd } else { &mut fwd }.entry(*e).or_default() += 1;
            }
            faces.push((i, outer, inners, flip));
        }
        for e in fwd.keys().chain(bwd.keys()) {
            let (f, b) = (fwd.get(e).copied().unwrap_or(0), bwd.get(e).copied().unwrap_or(0));
            if f == b && f > 1 {
                return Err(KernelError::Unsupported(
                    "zero-thickness geometry (bodies would touch along an edge)".into(),
                ));
            }
            if f != 1 || b != 1 {
                let pe = &self.edges[*e];
                let users: Vec<String> = faces
                    .iter()
                    .filter(|(_, o, i, _)| o.iter().chain(i.iter().flatten()).any(|(x, _)| x == e))
                    .map(|(pi, _, _, fl)| {
                        let p = &pieces[*pi];
                        format!("s{}f{}{}", p.solid, p.face, if *fl { "~" } else { "" })
                    })
                    .collect();
                return fail(format!(
                    "the result is not a closed solid (edge {:?} -> {:?} used {}/{} by {:?})",
                    self.pts[pe.v0],
                    self.pts[pe.v1],
                    fwd.get(e).unwrap_or(&0),
                    bwd.get(e).unwrap_or(&0),
                    users
                ));
            }
        }
        if faces.is_empty() {
            return Ok(Vec::new());
        }

        // Group faces into shells through shared edges.
        let n = faces.len();
        let mut parent: Vec<usize> = (0..n).collect();
        fn find(p: &mut [usize], mut i: usize) -> usize {
            while p[i] != i {
                p[i] = p[p[i]];
                i = p[i];
            }
            i
        }
        let mut owner_of: HashMap<usize, usize> = HashMap::new();
        for (fi, (_, outer, inners, _)) in faces.iter().enumerate() {
            for (e, _) in outer.iter().chain(inners.iter().flatten()) {
                if let Some(&other) = owner_of.get(e) {
                    let (a, b) = (find(&mut parent, fi), find(&mut parent, other));
                    parent[a] = b;
                } else {
                    owner_of.insert(*e, fi);
                }
            }
        }
        let mut shells: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        for fi in 0..n {
            let r = find(&mut parent, fi);
            shells.entry(r).or_default().push(fi);
        }

        let mut out = Vec::new();
        for members in shells.values() {
            let mut solid = Solid::default();
            let mut vids: HashMap<usize, VertexId> = HashMap::new();
            let mut eids: HashMap<usize, EdgeId> = HashMap::new();
            let mut vertex = |solid: &mut Solid, v: usize| -> VertexId {
                *vids.entry(v).or_insert_with(|| solid.add_vertex(self.pts[v]))
            };
            let mut to_loop = |solid: &mut Solid, uses: &[Use]| -> Loop {
                Loop {
                    coedges: uses
                        .iter()
                        .map(|&(e, r)| {
                            let id = match eids.get(&e) {
                                Some(id) => *id,
                                None => {
                                    let pe = &self.edges[e];
                                    let start = vertex(solid, pe.v0);
                                    let end = vertex(solid, pe.v1);
                                    let id = solid.add_edge(Edge {
                                        curve: pe.curve.clone(),
                                        start,
                                        end,
                                        t0: pe.t0,
                                        t1: pe.t1,
                                    });
                                    eids.insert(e, id);
                                    id
                                }
                            };
                            Coedge { edge: id, reversed: r }
                        })
                        .collect(),
                }
            };
            for &fi in members {
                let (pi, outer, inners, flip) = &faces[fi];
                let src = &self.solids[pieces[*pi].solid].faces[pieces[*pi].face];
                let outer = to_loop(&mut solid, outer);
                let inners = inners.iter().map(|l| to_loop(&mut solid, l)).collect();
                solid.add_face(Face {
                    surface: src.surface,
                    reversed: src.reversed != *flip,
                    outer,
                    inners,
                    tag: src.tag,
                    owner: src.owner,
                });
            }
            check::validate(&solid)?;
            out.push(solid);
        }
        // A shell lying inside another one is a void, not a separate body.
        if out.len() > 1 {
            for i in 0..out.len() {
                let Some(probe) = shell_probe(&out[i]) else {
                    continue;
                };
                for (j, other) in out.iter().enumerate() {
                    if i == j {
                        continue;
                    }
                    let infos: Vec<FaceInfo> = other.faces.iter().map(|f| face_info(other, f)).collect();
                    let loc = |g: usize, q: DVec3| locate_in(other, g, &infos[g], q);
                    if point_in_solid(other, probe, loc)? {
                        return Err(KernelError::Unsupported(
                            "a cut that leaves an empty pocket inside the part".into(),
                        ));
                    }
                }
            }
        }
        Ok(out)
    }
}

/// True when `p` lies strictly inside `solid`. Points on the boundary count
/// as inside.
pub fn contains_point(solid: &Solid, p: DVec3) -> Result<bool> {
    let infos: Vec<FaceInfo> = solid.faces.iter().map(|f| face_info(solid, f)).collect();
    for (g, info) in infos.iter().enumerate() {
        if locate_in(solid, g, info, p) != Loc::Outside {
            return Ok(true);
        }
    }
    point_in_solid(solid, p, |g, q| locate_in(solid, g, &infos[g], q))
}

/// True when `p` lies on face `f` of `solid` (inside or on its boundary).
pub fn face_contains(solid: &Solid, f: usize, p: DVec3) -> bool {
    let Some(face) = solid.faces.get(f) else {
        return false;
    };
    locate_in(solid, f, &face_info(solid, face), p) != Loc::Outside
}

/// Where a point lies relative to face `f` of `solid`.
fn locate_in(solid: &Solid, f: usize, info: &FaceInfo, p: DVec3) -> Loc {
    let face = &solid.faces[f];
    if face.surface.distance(p) > ON {
        return Loc::Outside;
    }
    let (lo, hi) = info.bbox;
    if p.cmplt(lo).any() || p.cmpgt(hi).any() {
        return Loc::Outside;
    }
    for l in face.loops() {
        for c in &l.coedges {
            if let Some(e) = solid.edge(c.edge)
                && e.curve.distance_to(p, e.t0, e.t1) < ON
            {
                return Loc::Boundary;
            }
        }
    }
    let uv = face.surface.uv(p);
    let shifts: Vec<f64> = match face.surface.u_period() {
        Some(per) => (-2..=2).map(|k| per * k as f64).collect(),
        None => vec![0.0],
    };
    for sh in shifts {
        let q = DVec2::new(uv.x + sh, uv.y);
        if q.x < info.u_range.0 - ON || q.x > info.u_range.1 + ON {
            continue;
        }
        let crossings = info.loops_uv.iter().filter(|l| point_in_polygon(q, l)).count();
        if crossings % 2 == 1 {
            return Loc::Inside;
        }
    }
    Loc::Outside
}

/// Exact point-in-solid test: rays are intersected with the true surfaces
/// and the hits located on the faces. Rays that graze an edge or run along
/// a surface are discarded, and the remaining rays vote.
fn point_in_solid(solid: &Solid, p: DVec3, locate: impl Fn(usize, DVec3) -> Loc) -> Result<bool> {
    const DIRS: [[f64; 3]; 7] = [
        [0.5773, 0.6132, 0.5390],
        [-0.3712, 0.8216, 0.4327],
        [0.7136, -0.2236, -0.6643],
        [-0.6124, -0.3536, 0.7150],
        [0.1830, -0.9659, 0.1830],
        [-0.8165, 0.4082, -0.4082],
        [0.2673, 0.5345, -0.8018],
    ];
    let (mut inside, mut outside) = (0, 0);
    'rays: for d in DIRS {
        let d = DVec3::from_array(d).normalize();
        let ray = Curve::Line { origin: p, dir: d };
        let mut hits: Vec<f64> = Vec::new();
        for (g, face) in solid.faces.iter().enumerate() {
            match intersect::curve_surface(&ray, ON, 1e9, &face.surface, TOL) {
                CurveHit::LiesOn => continue 'rays,
                CurveHit::Params(ts) => {
                    for t in ts {
                        match locate(g, ray.eval(t)) {
                            Loc::Inside => hits.push(t),
                            Loc::Boundary => continue 'rays,
                            Loc::Outside => {}
                        }
                    }
                }
            }
        }
        hits.sort_by(f64::total_cmp);
        hits.dedup_by(|a, b| (*a - *b).abs() < TOL);
        if hits.len() % 2 == 1 {
            inside += 1;
        } else {
            outside += 1;
        }
        if inside >= 3 || outside >= 3 {
            break;
        }
    }
    if inside == outside {
        return fail("could not tell whether a point is inside the other solid");
    }
    Ok(inside > outside)
}

/// A point on a shell's surface: the interior point of its first face.
fn shell_probe(solid: &Solid) -> Option<DVec3> {
    let face = solid.faces.first()?;
    let info = face_info(solid, face);
    let uv = interior_point(&info.loops_uv)?;
    Some(face.surface.eval_uv(uv))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Class {
    In,
    Out,
    OnSame,
    OnOpposite,
}

fn edge_box(e: &Edge) -> (DVec3, DVec3) {
    let pts = e.curve.sample(e.t0, e.t1, 0.0, 0.3);
    let mut lo = DVec3::splat(f64::INFINITY);
    let mut hi = DVec3::splat(f64::NEG_INFINITY);
    for p in pts {
        lo = lo.min(p);
        hi = hi.max(p);
    }
    (lo, hi)
}

fn signed_area(poly: &[DVec2]) -> f64 {
    let n = poly.len();
    (0..n).map(|i| poly[i].perp_dot(poly[(i + 1) % n])).sum::<f64>() * 0.5
}

/// A point well inside a region given by loops (even-odd), found from a
/// constrained Delaunay triangulation: the centroid of the largest interior
/// triangle.
fn interior_point(loops: &[Vec<DVec2>]) -> Option<DVec2> {
    let mut cdt: ConstrainedDelaunayTriangulation<Point2<f64>> = ConstrainedDelaunayTriangulation::new();
    for l in loops {
        let handles: Vec<_> = l.iter().filter_map(|p| cdt.insert(Point2::new(p.x, p.y)).ok()).collect();
        for k in 0..handles.len() {
            let (a, b) = (handles[k], handles[(k + 1) % handles.len()]);
            if a != b && cdt.can_add_constraint(a, b) {
                cdt.add_constraint(a, b);
            }
        }
    }
    let mut best: Option<(f64, DVec2)> = None;
    for tri in cdt.inner_faces() {
        let v = tri.vertices().map(|v| DVec2::new(v.position().x, v.position().y));
        let c = (v[0] + v[1] + v[2]) / 3.0;
        let area = (v[1] - v[0]).perp_dot(v[2] - v[0]).abs();
        let inside = loops.iter().filter(|l| point_in_polygon(c, l)).count() % 2 == 1;
        if inside && best.is_none_or(|(a, _)| area > a) {
            best = Some((area, c));
        }
    }
    best.map(|(_, c)| c)
}

#[cfg(test)]
mod tests;
