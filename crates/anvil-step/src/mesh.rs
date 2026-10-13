//! Tessellates read bodies of any supported surface type into triangle meshes.
//!
//! Every edge is sampled once and both faces that use it share those samples, so the
//! mesh is watertight wherever the source file's topology is. Faces are triangulated in
//! their surface's parameter space (a constrained Delaunay triangulation with interior
//! points) and the interior points are evaluated on the true surface. Faces whose
//! surface is not supported are triangulated flat across their boundary, with a warning.

use crate::reader::{Body, EdgeCurve, Face, LoopKind, Model};
use crate::shapes::{Curve, Surface};
use glam::{DVec2, DVec3};
use spade::{ConstrainedDelaunayTriangulation, Point2, Triangulation};
use std::collections::HashMap;
use std::f64::consts::{PI, TAU};

/// Chordal tolerance for curved geometry (mm).
pub const CHORD: f64 = 0.02;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct MeshFace {
    pub planar: bool,
    pub triangles: Vec<[u32; 3]>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Mesh {
    pub vertices: Vec<[f64; 3]>,
    /// One entry per source face, in the source's order.
    pub faces: Vec<MeshFace>,
}

struct Builder {
    mesh: Mesh,
    index: HashMap<[i64; 3], u32>,
}

fn key(p: DVec3) -> [i64; 3] {
    [(p.x * 1e7).round() as i64, (p.y * 1e7).round() as i64, (p.z * 1e7).round() as i64]
}

impl Builder {
    fn find(&self, p: DVec3) -> Option<u32> {
        self.index.get(&key(p)).copied()
    }

    fn vertex(&mut self, p: DVec3) -> u32 {
        let key = key(p);
        if let Some(&i) = self.index.get(&key) {
            return i;
        }
        let i = self.mesh.vertices.len() as u32;
        self.mesh.vertices.push([p.x, p.y, p.z]);
        self.index.insert(key, i);
        i
    }
}

/// Samples an edge from its start vertex to its end vertex.
fn sample_edge(model: &Model, e: &EdgeCurve) -> Vec<DVec3> {
    let (Some(&ps), Some(&pe)) = (model.vertices.get(&e.start), model.vertices.get(&e.end)) else {
        return Vec::new();
    };
    let closed = (ps - pe).length() < 1e-9;
    let mut pts = match &e.curve {
        None | Some(Curve::Line { .. }) => vec![ps, pe],
        Some(c) => {
            let t0 = c.invert(ps);
            let mut t1 = c.invert(pe);
            if c.is_periodic() {
                let mut sweep =
                    if e.same_sense { (t1 - t0).rem_euclid(TAU) } else { -(t0 - t1).rem_euclid(TAU) };
                if closed || sweep.abs() < 1e-12 {
                    sweep = if e.same_sense { TAU } else { -TAU };
                }
                t1 = t0 + sweep;
            } else if closed {
                let (lo, hi) = c.domain();
                // A closed curve whose vertex is not at the start of its domain: go
                // from the vertex to the domain's end and on round from its start.
                let ends_meet = (c.eval(lo) - c.eval(hi)).length() < 1e-9 * ps.length().max(1.0);
                let margin = 1e-6 * (hi - lo);
                if ends_meet && t0 > lo + margin && t0 < hi - margin {
                    let (first, second) = if e.same_sense {
                        (c.sample(t0, hi, CHORD), c.sample(lo, t0, CHORD))
                    } else {
                        (c.sample(t0, lo, CHORD), c.sample(hi, t0, CHORD))
                    };
                    let mut pts = first;
                    pts.extend(second.into_iter().skip(1));
                    return pin(pts, ps, pe);
                }
                let (a, b) = if e.same_sense { (lo, hi) } else { (hi, lo) };
                return pin(c.sample(a, b, CHORD), ps, pe);
            }
            c.sample(t0, t1, CHORD)
        }
    };
    if pts.len() < 2 {
        pts = vec![ps, pe];
    }
    pin(pts, ps, pe)
}

fn pin(mut pts: Vec<DVec3>, a: DVec3, b: DVec3) -> Vec<DVec3> {
    if let Some(f) = pts.first_mut() {
        *f = a;
    }
    if let Some(l) = pts.last_mut() {
        *l = b;
    }
    pts
}

/// Closed boundary polylines of a face (last point not repeated), in loop direction.
fn face_loops(model: &Model, samples: &HashMap<u64, Vec<DVec3>>, f: &Face) -> Vec<Vec<DVec3>> {
    let mut out = Vec::new();
    for b in &f.bounds {
        let mut pts = match &b.kind {
            LoopKind::Edges(edges) => {
                let mut pts = Vec::new();
                for oe in edges {
                    let Some(s) = samples.get(&oe.edge) else { continue };
                    let n = s.len();
                    if oe.forward {
                        pts.extend(s.iter().take(n.saturating_sub(1)));
                    } else {
                        pts.extend(s.iter().rev().take(n.saturating_sub(1)));
                    }
                }
                pts
            }
            LoopKind::Vertex(_) => continue,
            LoopKind::Poly(p) => p.clone(),
        };
        if !b.orientation {
            pts.reverse();
        }
        if pts.len() >= 3 {
            out.push(pts);
        }
    }
    let _ = model;
    out
}

fn newell(pts: &[DVec3]) -> DVec3 {
    let n = pts.len();
    (0..n).map(|i| pts[i].cross(pts[(i + 1) % n])).sum()
}

fn point_in_polygon(p: DVec2, poly: &[DVec2]) -> bool {
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

/// Shifts `v` by whole periods to be nearest `reference`.
fn unwrap(reference: f64, v: f64, period: f64) -> f64 {
    v + period * ((reference - v) / period).round()
}

/// Triangulates closed loops lying in a 2D domain. `lift` maps a domain point to 3D
/// (used for interior points). The domain is laid out so that counter-clockwise is
/// along the surface's natural normal; `outward` says whether the face's outward normal
/// agrees with it. Orientation comes from the 2D winding alone, which stays right for
/// long thin triangles where comparing 3D normals can mislead.
fn triangulate(
    b: &mut Builder,
    loops3: &[Vec<DVec3>],
    loops2: &[Vec<DVec2>],
    interior: &[DVec2],
    lift: &dyn Fn(DVec2) -> DVec3,
    outward: bool,
) -> Vec<[u32; 3]> {
    let mut cdt: ConstrainedDelaunayTriangulation<Point2<f64>> = ConstrainedDelaunayTriangulation::new();
    let mut map: HashMap<usize, u32> = HashMap::new();
    let mut refused = false;
    for (l3, l2) in loops3.iter().zip(loops2) {
        let mut handles = Vec::with_capacity(l2.len());
        for (p3, p2) in l3.iter().zip(l2) {
            let Ok(h) = cdt.insert(Point2::new(p2.x, p2.y)) else { continue };
            map.entry(h.index()).or_insert_with(|| b.vertex(*p3));
            handles.push(h);
        }
        for k in 0..handles.len() {
            let (h0, h1) = (handles[k], handles[(k + 1) % handles.len()]);
            if h0 != h1 {
                if cdt.can_add_constraint(h0, h1) {
                    cdt.add_constraint(h0, h1);
                } else {
                    refused = true;
                }
            }
        }
    }
    for q in interior {
        if let Ok(h) = cdt.insert(Point2::new(q.x, q.y)) {
            map.entry(h.index()).or_insert_with(|| b.vertex(lift(*q)));
        }
    }
    // Inside or outside, decided topologically: walk from the outer face across the
    // triangulation, flipping at every boundary edge. Unlike testing each triangle's
    // centroid, this stays exact for slivers along boundaries with collinear points.
    let parity = if refused { None } else { Some(boundary_parity(&cdt)) };
    let mut tris = Vec::new();
    for t in cdt.inner_faces() {
        let v = t.vertices();
        let uv: Vec<DVec2> = v.iter().map(|x| DVec2::new(x.position().x, x.position().y)).collect();
        let c = (uv[0] + uv[1] + uv[2]) / 3.0;
        let inside = match &parity {
            Some(p) => p.get(t.fix().index()).copied().unwrap_or(false),
            None => loops2.iter().filter(|l| point_in_polygon(c, l)).count() % 2 == 1,
        };
        if !inside {
            continue;
        }
        let ids: Vec<u32> = v.iter().filter_map(|x| map.get(&x.fix().index()).copied()).collect();
        let [i0, i1, i2] = ids[..] else { continue };
        if i0 == i1 || i1 == i2 || i0 == i2 {
            continue;
        }
        let ccw = (uv[1] - uv[0]).perp_dot(uv[2] - uv[0]) > 0.0;
        if ccw == outward { tris.push([i0, i1, i2]) } else { tris.push([i0, i2, i1]) }
    }
    tris
}

/// For every face of the triangulation, whether an odd number of boundary edges
/// separate it from the outer face.
fn boundary_parity(cdt: &ConstrainedDelaunayTriangulation<Point2<f64>>) -> Vec<bool> {
    let n = cdt.num_all_faces();
    let mut adj: Vec<Vec<(usize, bool)>> = vec![Vec::new(); n];
    for t in cdt.inner_faces() {
        let i = t.fix().index();
        for e in t.adjacent_edges() {
            let j = e.rev().face().fix().index();
            let wall = e.is_constraint_edge();
            if i < n && j < n {
                adj[i].push((j, wall));
                adj[j].push((i, wall));
            }
        }
    }
    let mut parity: Vec<Option<bool>> = vec![None; n];
    let outer = cdt.outer_face().fix().index();
    let mut queue = std::collections::VecDeque::from([outer]);
    if let Some(p) = parity.get_mut(outer) {
        *p = Some(false);
    }
    while let Some(f) = queue.pop_front() {
        let Some(pf) = parity[f] else { continue };
        for &(g, wall) in &adj[f] {
            if parity[g].is_none() {
                parity[g] = Some(pf != wall);
                queue.push_back(g);
            }
        }
    }
    parity.into_iter().map(|p| p.unwrap_or(false)).collect()
}

/// Points on a grid strictly inside the loops and away from their edges.
fn grid(loops2: &[Vec<DVec2>], step: DVec2) -> Vec<DVec2> {
    let all: Vec<DVec2> = loops2.iter().flatten().copied().collect();
    let Some(first) = all.first() else { return Vec::new() };
    let (lo, hi) = all.iter().fold((*first, *first), |(a, b), p| (a.min(*p), b.max(*p)));
    let nu = ((hi.x - lo.x) / step.x).ceil() as usize;
    let nv = ((hi.y - lo.y) / step.y).ceil() as usize;
    if nu < 2 && nv < 2 || nu.saturating_mul(nv) > 40_000 {
        return Vec::new();
    }
    let clearance = 0.3 * step.x.min(step.y);
    let near = |q: DVec2| {
        loops2.iter().any(|l| {
            (0..l.len()).any(|i| {
                let (a, b) = (l[i], l[(i + 1) % l.len()]);
                let ab = b - a;
                let t = ((q - a).dot(ab) / ab.length_squared().max(1e-300)).clamp(0.0, 1.0);
                (a + ab * t - q).length() < clearance
            })
        })
    };
    let mut out = Vec::new();
    for i in 1..nu {
        for j in 1..nv {
            let q = lo + DVec2::new(step.x * i as f64, step.y * j as f64);
            if loops2.iter().filter(|l| point_in_polygon(q, l)).count() % 2 == 1 && !near(q) {
                out.push(q);
            }
        }
    }
    out
}

fn flat_face(b: &mut Builder, loops3: &[Vec<DVec3>], normal: DVec3) -> Vec<[u32; 3]> {
    let Some(z) = normal.try_normalize() else { return Vec::new() };
    let x = z.any_orthonormal_vector();
    let y = z.cross(x);
    let loops2: Vec<Vec<DVec2>> =
        loops3.iter().map(|l| l.iter().map(|p| DVec2::new(p.dot(x), p.dot(y))).collect()).collect();
    // (x, y, z) is right-handed, so counter-clockwise in (x, y) is along z.
    triangulate(b, loops3, &loops2, &[], &|_| DVec3::ZERO, true)
}

fn coord(q: DVec2, dim: usize) -> f64 {
    if dim == 0 { q.x } else { q.y }
}

fn with_coord(mut q: DVec2, dim: usize, v: f64) -> DVec2 {
    if dim == 0 {
        q.x = v;
    } else {
        q.y = v;
    }
    q
}

/// How far a loop travels around a periodic parameter: about +TAU or -TAU for a loop
/// that winds once around the surface, zero for an ordinary closed loop.
fn winding(l: &[DVec2], dim: usize) -> f64 {
    match (l.first(), l.last()) {
        (Some(f), Some(e)) => unwrap(coord(*e, dim), coord(*f, dim), TAU) - coord(*f, dim),
        _ => 0.0,
    }
}

/// Faces of revolution are often bounded by loops that each go once around the axis
/// (a band between two circles, or a cap bounded by one circle), with no seam edge. In
/// parameter space such loops are open strips, not closed polygons. Two of them are
/// joined into one closed outline through a cut between existing vertices; a single one
/// is closed through the surface's pole or apex. No point is added on a real edge, so
/// neighbouring faces still share their vertices exactly.
fn close_wrapping(
    s: &Surface,
    sense: f64,
    loops3: Vec<Vec<DVec3>>,
    raw: Vec<Vec<DVec2>>,
    dim: usize,
) -> (Vec<Vec<DVec3>>, Vec<Vec<DVec2>>) {
    let wraps: Vec<usize> = (0..raw.len()).filter(|&i| winding(&raw[i], dim).abs() > PI).collect();
    // Holes (loops that do not wrap) must avoid the cut and sit on the same turn as the
    // joined outline. The cut goes through the middle of the widest gap between them.
    let holes: Vec<usize> = (0..raw.len()).filter(|i| !wraps.contains(i)).collect();
    let cut_at = |start: f64| -> f64 {
        let mut spans: Vec<(f64, f64)> = holes
            .iter()
            .filter_map(|&h| {
                let us: Vec<f64> = raw[h].iter().map(|q| coord(*q, dim)).collect();
                let lo = us.iter().copied().fold(f64::INFINITY, f64::min);
                let hi = us.iter().copied().fold(f64::NEG_INFINITY, f64::max);
                (lo.is_finite() && hi - lo < TAU).then(|| {
                    let width = hi - lo;
                    let lo = start + (lo - start).rem_euclid(TAU);
                    (lo, lo + width)
                })
            })
            .collect();
        if spans.is_empty() {
            return start;
        }
        spans.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut best = (0.0, start);
        for i in 0..spans.len() {
            let end = spans[i].1;
            let next = if i + 1 < spans.len() { spans[i + 1].0 } else { spans[0].0 + TAU };
            if next - end > best.0 {
                best = (next - end, 0.5 * (end + next));
            }
        }
        best.1
    };
    let place_holes = |out3: &mut Vec<Vec<DVec3>>, out2: &mut Vec<Vec<DVec2>>, lo: f64| {
        for &h in &holes {
            let mut l = raw[h].clone();
            let mean = l.iter().map(|q| coord(*q, dim)).sum::<f64>() / l.len().max(1) as f64;
            let shift = (lo + (mean - lo).rem_euclid(TAU)) - mean;
            for q in l.iter_mut() {
                *q = with_coord(*q, dim, coord(*q, dim) + shift);
            }
            out3.push(loops3[h].clone());
            out2.push(l);
        }
    };
    match wraps[..] {
        [a, b] => {
            let (a, b) = if winding(&raw[a], dim) > 0.0 { (a, b) } else { (b, a) };
            let (wa, wb) = (winding(&raw[a], dim), winding(&raw[b], dim));
            if wa <= 0.0 || wb >= 0.0 {
                return (loops3, raw);
            }
            // Start the first loop at its vertex nearest the cut.
            let Some(&first) = raw[a].first() else { return (loops3, raw) };
            let cut = cut_at(coord(first, dim));
            let na = raw[a].len();
            let ka = (0..na)
                .min_by(|&i, &j| {
                    let d = |k: usize| (unwrap(cut, coord(raw[a][k], dim), TAU) - cut).abs();
                    d(i).total_cmp(&d(j))
                })
                .unwrap_or(0);
            let mut pts3: Vec<DVec3> = (0..na).map(|i| loops3[a][(ka + i) % na]).collect();
            let mut uv: Vec<DVec2> = Vec::with_capacity(na + 1);
            let mut prev = unwrap(cut, coord(raw[a][ka], dim), TAU);
            for i in 0..na {
                let q = raw[a][(ka + i) % na];
                prev = unwrap(prev, coord(q, dim), TAU);
                uv.push(with_coord(q, dim, prev));
            }
            let a0 = uv[0];
            let a03 = pts3[0];
            let end = coord(a0, dim) + wa;
            pts3.push(a03);
            uv.push(with_coord(a0, dim, end));
            // Start the other loop at its vertex nearest the end of the first.
            let nb = raw[b].len();
            let Some(k) = (0..nb).min_by(|&i, &j| {
                let di = (unwrap(end, coord(raw[b][i], dim), TAU) - end).abs();
                let dj = (unwrap(end, coord(raw[b][j], dim), TAU) - end).abs();
                di.total_cmp(&dj)
            }) else {
                return (loops3, raw);
            };
            let mut prev = end;
            for step in 0..=nb {
                let i = (k + step) % nb;
                let mut q = raw[b][i];
                let v = unwrap(prev, coord(q, dim), TAU);
                q = with_coord(q, dim, v);
                prev = v;
                pts3.push(loops3[b][i]);
                uv.push(q);
            }
            let lo = coord(a0, dim);
            let mut out3 = vec![pts3];
            let mut out2 = vec![uv];
            place_holes(&mut out3, &mut out2, lo);
            (out3, out2)
        }
        [a] if dim == 0 => {
            let w = winding(&raw[a], dim);
            let up = (w > 0.0) == (sense > 0.0);
            let pole = match s {
                Surface::Sphere { .. } => Some(if up { PI / 2.0 } else { -PI / 2.0 }),
                Surface::Cone { radius, semi_angle, .. } if semi_angle.tan().abs() > 1e-12 => {
                    Some(-radius / semi_angle.tan())
                }
                _ => None,
            };
            let (Some(v), Some(&a0), Some(&a03)) = (pole, raw[a].first(), loops3[a].first()) else {
                return (loops3, raw);
            };
            let mut l3 = vec![loops3[a].clone()];
            let mut l2 = vec![raw[a].clone()];
            let (u0, u1) = (a0.x, a0.x + w);
            let a = 0;
            // Close the loop itself first (its start point, one turn later), then run
            // along the pole back to where it started.
            l3[a].push(a03);
            l2[a].push(DVec2::new(u1, a0.y));
            for u in [u1, u0] {
                let q = DVec2::new(u, v);
                l3[a].push(s.eval(q));
                l2[a].push(q);
            }
            place_holes(&mut l3, &mut l2, u0.min(u1));
            (l3, l2)
        }
        _ => (loops3, raw),
    }
}

/// The height parameter of a pole or apex, if `p` lies on one.
fn pole_height(s: &Surface, p: DVec3) -> Option<f64> {
    match s {
        Surface::Sphere { frame, radius } => {
            let l = frame.to_local(p);
            (DVec2::new(l.x, l.y).length() < 1e-7 * radius.max(1.0)).then_some(if l.z > 0.0 {
                PI / 2.0
            } else {
                -PI / 2.0
            })
        }
        Surface::Cone { frame, radius, semi_angle } => {
            let l = frame.to_local(p);
            let r = radius + l.z * semi_angle.tan();
            (DVec2::new(l.x, l.y).length() < 1e-7 * radius.abs().max(1.0) && r.abs() < 1e-6).then_some(l.z)
        }
        _ => None,
    }
}

/// Counts what is wrong with a face's triangles: edges covered twice, open edges that
/// are not on the face's loops, and loop edges not covered exactly once. Zero means the
/// face is a clean, manifold patch with exactly the boundary it should have.
fn defects(b: &Builder, loops3: &[Vec<DVec3>], tris: &[[u32; 3]]) -> usize {
    // How many times each loop edge appears. Once: a true boundary, covered from one
    // side. Twice: a seam, which the loop runs along in both directions, covered from
    // both sides.
    let mut uses: HashMap<(u32, u32), u32> = HashMap::new();
    for l in loops3 {
        let ids: Vec<Option<u32>> = l.iter().map(|p| b.find(*p)).collect();
        for k in 0..ids.len() {
            if let (Some(a), Some(c)) = (ids[k], ids[(k + 1) % ids.len()])
                && a != c
            {
                *uses.entry((a.min(c), a.max(c))).or_default() += 1;
            }
        }
    }
    let mut directed: HashMap<(u32, u32), u32> = HashMap::new();
    for t in tris {
        for k in 0..3 {
            *directed.entry((t[k], t[(k + 1) % 3])).or_default() += 1;
        }
    }
    let mut bad = directed.values().filter(|&&n| n > 1).count();
    for &(a, c) in directed.keys() {
        let open = !directed.contains_key(&(c, a));
        let boundary = uses.get(&(a.min(c), a.max(c))) == Some(&1);
        if open != boundary {
            bad += 1;
        }
    }
    bad + uses
        .keys()
        .filter(|&&(a, c)| !directed.contains_key(&(a, c)) && !directed.contains_key(&(c, a)))
        .count()
}

/// The file's loops run counter-clockwise about the face's outward normal, whatever
/// the surface's parameterisation. If the triangles run the other way along most of the
/// boundary, the whole face is flipped to agree with the loops.
fn orient_by_loops(b: &Builder, loops3: &[Vec<DVec3>], tris: &mut [[u32; 3]]) {
    let mut directed: std::collections::HashSet<(u32, u32)> = Default::default();
    for t in tris.iter() {
        for k in 0..3 {
            directed.insert((t[k], t[(k + 1) % 3]));
        }
    }
    let (mut agree, mut disagree) = (0usize, 0usize);
    for l in loops3 {
        let ids: Vec<Option<u32>> = l.iter().map(|p| b.find(*p)).collect();
        for k in 0..ids.len() {
            let (Some(a), Some(c)) = (ids[k], ids[(k + 1) % ids.len()]) else { continue };
            match (directed.contains(&(a, c)), directed.contains(&(c, a))) {
                (true, false) => agree += 1,
                (false, true) => disagree += 1,
                _ => {}
            }
        }
    }
    if disagree > agree {
        for t in tris.iter_mut() {
            t.swap(1, 2);
        }
    }
}

/// Moves holes by whole periods (in coordinate `dim`) into the range of the widest
/// loop when they landed outside it.
fn place_in_widest(raw: &mut [Vec<DVec2>], dim: usize) {
    let span = |l: &[DVec2]| {
        l.iter()
            .map(|q| coord(*q, dim))
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), x| (a.min(x), b.max(x)))
    };
    let Some(widest) = (0..raw.len()).max_by(|&i, &j| {
        let (a, b) = (span(&raw[i]), span(&raw[j]));
        (a.1 - a.0).total_cmp(&(b.1 - b.0))
    }) else {
        return;
    };
    let (lo, hi) = span(&raw[widest]);
    let inside = |a: f64, b: f64| a >= lo - 1e-9 && b <= hi + 1e-9;
    for (i, l) in raw.iter_mut().enumerate() {
        let (a, b) = span(l);
        // A loop that goes around (its unwrapped ends a period apart) stays put.
        let wraps = match (l.first(), l.last()) {
            (Some(f), Some(e)) => (coord(*e, dim) - coord(*f, dim)).abs() > PI,
            _ => false,
        };
        if i == widest || !a.is_finite() || wraps {
            continue;
        }
        // Only a hole that the move puts inside the outer loop's range: other loops
        // (a torus band's second rim, say) are where they belong already.
        let shift = TAU * ((lo - 0.5 * (a + b)) / TAU).ceil();
        if inside(a, b) || !inside(a + shift, b + shift) {
            continue;
        }
        for q in l.iter_mut() {
            *q = with_coord(*q, dim, coord(*q, dim) + shift);
        }
    }
}

fn curved_face(
    b: &mut Builder,
    s: &Surface,
    sense: f64,
    loops3: &[Vec<DVec3>],
    with_interior: bool,
) -> Vec<[u32; 3]> {
    let (pu, pv) = s.periodic();
    // A loop point on a pole or apex has no single longitude; it becomes a short
    // segment between the longitudes of the boundary on either side of it.
    let mut split3: Vec<Vec<DVec3>> = Vec::with_capacity(loops3.len());
    let mut split2: Vec<Vec<DVec2>> = Vec::with_capacity(loops3.len());
    for l in loops3 {
        let qs: Vec<DVec2> = l.iter().map(|p| s.invert(*p)).collect();
        let n = l.len();
        let (mut o3, mut o2) = (Vec::with_capacity(n + 2), Vec::with_capacity(n + 2));
        for i in 0..n {
            match pole_height(s, l[i]) {
                Some(v) if n > 2 => {
                    let (before, after) = (qs[(i + n - 1) % n].x, qs[(i + 1) % n].x);
                    o3.extend([l[i], l[i]]);
                    o2.extend([DVec2::new(before, v), DVec2::new(after, v)]);
                }
                _ => {
                    o3.push(l[i]);
                    o2.push(qs[i]);
                }
            }
        }
        split3.push(o3);
        split2.push(o2);
    }
    let loops3 = &split3[..];
    // Raw parameters, unwrapped along each loop and then onto the first loop's branch.
    let mut raw: Vec<Vec<DVec2>> = Vec::new();
    for l in &split2 {
        let mut out: Vec<DVec2> = Vec::with_capacity(l.len());
        for &q0 in l {
            let mut q = q0;
            if let Some(prev) = out.last() {
                if pu {
                    q.x = unwrap(prev.x, q.x, TAU);
                }
                if pv {
                    q.y = unwrap(prev.y, q.y, TAU);
                }
            }
            out.push(q);
        }
        raw.push(out);
    }
    if let Some(r0) = raw.first().and_then(|l| l.first()).copied() {
        for l in raw.iter_mut().skip(1) {
            if let Some(f) = l.first().copied() {
                let du = if pu { unwrap(r0.x, f.x, TAU) - f.x } else { 0.0 };
                let dv = if pv { unwrap(r0.y, f.y, TAU) - f.y } else { 0.0 };
                for q in l.iter_mut() {
                    *q += DVec2::new(du, dv);
                }
            }
        }
    }
    // Matching first points is not enough on its own: a hole at -90 degrees in a face
    // running from 0 to 360 would sit outside it. Holes move by whole periods so their
    // middles fall in the range of the widest loop (the outer one).
    for (periodic, dim) in [(pu, 0), (pv, 1)] {
        if periodic {
            place_in_widest(&mut raw, dim);
        }
    }
    let mut loops3: Vec<Vec<DVec3>> = loops3.to_vec();
    if pu {
        (loops3, raw) = close_wrapping(s, sense, loops3, raw, 0);
    }
    if pv {
        (loops3, raw) = close_wrapping(s, sense, loops3, raw, 1);
    }
    let loops3 = &loops3[..];
    // Scale parameters to millimetres around the face so the triangulation sees the
    // surface's real proportions.
    let all: Vec<DVec2> = raw.iter().flatten().copied().collect();
    let centre = all.iter().copied().sum::<DVec2>() / all.len().max(1) as f64;
    let metric = s.metric(centre);
    let loops2: Vec<Vec<DVec2>> = raw.iter().map(|l| l.iter().map(|q| *q * metric).collect()).collect();
    // Interior spacing: fine enough to keep chords within tolerance on curved surfaces.
    let size = all.iter().fold(DVec2::ZERO, |m, q| m.max((*q - centre).abs())) * metric;
    let curvature_step = match s {
        Surface::Cylinder { radius, .. } | Surface::Sphere { radius, .. } => chord_step(*radius),
        Surface::Cone { radius, .. } => chord_step(radius.max(1.0)),
        Surface::Torus { minor, .. } => chord_step(*minor),
        _ => (size.max_element() / 12.0).max(0.05),
    };
    let step = DVec2::splat(curvature_step.max(size.max_element() / 150.0).max(1e-3));
    let interior = if with_interior { grid(&loops2, step) } else { Vec::new() };
    let lift = |q: DVec2| s.eval(q / metric);
    triangulate(b, loops3, &loops2, &interior, &lift, sense > 0.0)
}

/// A sphere face, laid out by stereographic projection from the point opposite its
/// middle (as the exact kernel does): no poles, no seam, and angles kept. `None` for a
/// face with no clear middle, which the general path then takes.
fn sphere_face(
    b: &mut Builder,
    centre: DVec3,
    radius: f64,
    sense: f64,
    loops3: &[Vec<DVec3>],
) -> Option<Vec<[u32; 3]>> {
    // The face's vector area points into its middle (loops run counter-clockwise about
    // the face normal, which `sense` turns inward for a concave face).
    let area: DVec3 =
        loops3.iter().map(|l| newell(&l.iter().map(|p| *p - centre).collect::<Vec<_>>())).sum::<DVec3>()
            * sense;
    let z = area.try_normalize().filter(|_| area.length() > 1e-6 * radius * radius)?;
    let x = z.any_orthonormal_vector();
    let y = z.cross(x);
    let to_uv = |p: DVec3| {
        let d = (p - centre).normalize_or(z);
        DVec2::new(d.dot(x), d.dot(y)) * (2.0 * radius / (1.0 + d.dot(z)).max(1e-12))
    };
    let lift = |q: DVec2| {
        let q = q / (2.0 * radius);
        let s = q.length_squared();
        centre + (x * (2.0 * q.x) + y * (2.0 * q.y) + z * (1.0 - s)) * (radius / (1.0 + s))
    };
    let loops2: Vec<Vec<DVec2>> = loops3.iter().map(|l| l.iter().map(|p| to_uv(*p)).collect()).collect();
    let mut step = chord_step(radius);
    let mut interior = grid(&loops2, DVec2::splat(step));
    // The grid refuses very fine layouts; widen until it fits rather than go without.
    while interior.is_empty() && step < 4.0 * radius {
        step *= 1.5;
        interior = grid(&loops2, DVec2::splat(step));
    }
    // Counter-clockwise in (x, y) is about z, the outward normal at the face's middle.
    Some(triangulate(b, loops3, &loops2, &interior, &lift, sense > 0.0))
}

fn chord_step(radius: f64) -> f64 {
    let angle = if CHORD < radius { 2.0 * (1.0 - CHORD / radius).acos() } else { PI / 4.0 };
    (radius * angle.min(PI / 18.0)).max(1e-3)
}

/// Tessellates every face of `body`.
pub fn tessellate(model: &Model, body: &Body, warnings: &mut Vec<String>) -> Mesh {
    let mut samples: HashMap<u64, Vec<DVec3>> = HashMap::new();
    for f in &body.faces {
        for bnd in &f.bounds {
            if let LoopKind::Edges(edges) = &bnd.kind {
                for oe in edges {
                    if !samples.contains_key(&oe.edge)
                        && let Some(e) = model.edges.get(&oe.edge)
                    {
                        samples.insert(oe.edge, sample_edge(model, e));
                    }
                }
            }
        }
    }
    let mut b = Builder { mesh: Mesh::default(), index: HashMap::new() };
    for f in &body.faces {
        let loops3 = face_loops(model, &samples, f);
        let sense = if f.same_sense { 1.0 } else { -1.0 };
        let planar = matches!(f.surface, Some(Surface::Plane { .. })) || f.surface_type == "POLY";
        let mut triangles = match &f.surface {
            Some(Surface::Plane { frame }) => flat_face(&mut b, &loops3, frame.z * sense),
            Some(s) if !loops3.is_empty() => {
                // Try the full tessellation, then without interior points, then flat;
                // keep the first clean result, or the least broken one.
                let mut best: Option<(usize, Vec<[u32; 3]>)> = None;
                for attempt in 0..3 {
                    let sphere = match s {
                        Surface::Sphere { frame, radius } if attempt == 0 => {
                            sphere_face(&mut b, frame.origin, *radius, sense, &loops3)
                        }
                        _ => None,
                    };
                    let tris = match attempt {
                        0 if sphere.is_some() => sphere.unwrap_or_default(),
                        0 => curved_face(&mut b, s, sense, &loops3, true),
                        1 => curved_face(&mut b, s, sense, &loops3, false),
                        _ => {
                            let n = loops3.first().map(|l| newell(l)).unwrap_or(DVec3::Z);
                            flat_face(&mut b, &loops3, n)
                        }
                    };
                    let d = defects(&b, &loops3, &tris);
                    if best.as_ref().is_none_or(|(bd, _)| d < *bd) {
                        best = Some((d, tris));
                    }
                    if d == 0 {
                        break;
                    }
                }
                let (d, tris) = best.unwrap_or_default();
                if d > 0 && std::env::var("STEP_DEBUG").is_ok() {
                    eprintln!("STEP_DEBUG face #{} {} still has {d} defects", f.id, f.surface_type);
                }
                tris
            }
            Some(s) => closed_surface(&mut b, s, sense),
            None => {
                if f.surface_type != "POLY" {
                    warnings.push(format!("face #{} ({}) was approximated as flat", f.id, f.surface_type));
                }
                let n = loops3.first().map(|l| newell(l)).unwrap_or(DVec3::Z);
                flat_face(&mut b, &loops3, n)
            }
        };
        orient_by_loops(&b, &loops3, &mut triangles);
        b.mesh.faces.push(MeshFace { planar, triangles });
    }
    let faceted = body.faces.iter().all(|f| f.surface_type == "POLY");
    if faceted { merge_facets(b.mesh) } else { b.mesh }
}

/// Faceted bodies list every polygon as its own face. Neighbouring facets that meet
/// at less than this angle are merged into one face, so model edges appear only at
/// real creases and a faceted cylinder picks as one face.
const SMOOTH: f64 = 20.0 * PI / 180.0;

fn merge_facets(mesh: Mesh) -> Mesh {
    let n = mesh.faces.len();
    let p = |i: u32| DVec3::from_array(mesh.vertices[i as usize]);
    let normals: Vec<DVec3> = mesh
        .faces
        .iter()
        .map(|f| {
            f.triangles
                .iter()
                .map(|t| (p(t[1]) - p(t[0])).cross(p(t[2]) - p(t[0])))
                .sum::<DVec3>()
                .normalize_or_zero()
        })
        .collect();
    // Faces on each undirected edge.
    let mut by_edge: HashMap<(u32, u32), Vec<usize>> = HashMap::new();
    for (fi, f) in mesh.faces.iter().enumerate() {
        for t in &f.triangles {
            for k in 0..3 {
                let (a, b) = (t[k], t[(k + 1) % 3]);
                let list = by_edge.entry((a.min(b), a.max(b))).or_default();
                if !list.contains(&fi) {
                    list.push(fi);
                }
            }
        }
    }
    let mut parent: Vec<usize> = (0..n).collect();
    fn find(p: &mut [usize], mut i: usize) -> usize {
        while p[i] != i {
            p[i] = p[p[i]];
            i = p[i];
        }
        i
    }
    let cos = SMOOTH.cos();
    for faces in by_edge.values() {
        if let [a, b] = faces[..]
            && normals[a].dot(normals[b]) > cos
        {
            let (ra, rb) = (find(&mut parent, a), find(&mut parent, b));
            if ra != rb {
                parent[ra.max(rb)] = ra.min(rb);
            }
        }
    }
    let mut groups: Vec<MeshFace> = Vec::new();
    let mut slot: HashMap<usize, usize> = HashMap::new();
    let mut first_normal: Vec<DVec3> = Vec::new();
    for (fi, f) in mesh.faces.into_iter().enumerate() {
        let r = find(&mut parent, fi);
        let g = *slot.entry(r).or_insert_with(|| {
            groups.push(MeshFace { planar: true, triangles: Vec::new() });
            first_normal.push(normals[fi]);
            groups.len() - 1
        });
        if normals[fi].dot(first_normal[g]) < 1.0 - 1e-9 {
            groups[g].planar = false;
        }
        groups[g].triangles.extend(f.triangles);
    }
    Mesh { vertices: mesh.vertices, faces: groups }
}

/// A face covering a whole closed surface (a full sphere or torus with no edges).
fn closed_surface(b: &mut Builder, s: &Surface, sense: f64) -> Vec<[u32; 3]> {
    let (u0, u1, v0, v1) = match s {
        Surface::Sphere { .. } => (0.0, TAU, -PI / 2.0, PI / 2.0),
        Surface::Torus { .. } => (0.0, TAU, 0.0, TAU),
        _ => return Vec::new(),
    };
    let (nu, nv) = (72usize, 36usize);
    let mut ids = vec![vec![0u32; nv + 1]; nu + 1];
    for (i, row) in ids.iter_mut().enumerate() {
        for (j, id) in row.iter_mut().enumerate() {
            let uv = DVec2::new(u0 + (u1 - u0) * i as f64 / nu as f64, v0 + (v1 - v0) * j as f64 / nv as f64);
            *id = b.vertex(s.eval(uv));
        }
    }
    let mut tris = Vec::new();
    for i in 0..nu {
        for j in 0..nv {
            let (a, c, d, e) = (ids[i][j], ids[i + 1][j], ids[i + 1][j + 1], ids[i][j + 1]);
            for t in [[a, c, d], [a, d, e]] {
                if t[0] != t[1] && t[1] != t[2] && t[0] != t[2] {
                    tris.push(if sense > 0.0 { t } else { [t[0], t[2], t[1]] });
                }
            }
        }
    }
    tris
}
