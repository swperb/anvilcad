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

impl Builder {
    fn vertex(&mut self, p: DVec3) -> u32 {
        let key = [(p.x * 1e7).round() as i64, (p.y * 1e7).round() as i64, (p.z * 1e7).round() as i64];
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
/// (used for interior points), `normal` gives the outward normal at a domain point.
fn triangulate(
    b: &mut Builder,
    loops3: &[Vec<DVec3>],
    loops2: &[Vec<DVec2>],
    interior: &[DVec2],
    lift: &dyn Fn(DVec2) -> DVec3,
    normal: &dyn Fn(DVec2) -> DVec3,
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
        let p = |i: u32| {
            let a = b.mesh.vertices[i as usize];
            DVec3::new(a[0], a[1], a[2])
        };
        let n = (p(i1) - p(i0)).cross(p(i2) - p(i0));
        if n.dot(normal(c)) < 0.0 { tris.push([i0, i2, i1]) } else { tris.push([i0, i1, i2]) }
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
    triangulate(b, loops3, &loops2, &[], &|_| DVec3::ZERO, &|_| z)
}

fn curved_face(b: &mut Builder, s: &Surface, sense: f64, loops3: &[Vec<DVec3>]) -> Vec<[u32; 3]> {
    let (pu, pv) = s.periodic();
    // Raw parameters, unwrapped along each loop and then onto the first loop's branch.
    let mut raw: Vec<Vec<DVec2>> = Vec::new();
    for l in loops3 {
        let mut out: Vec<DVec2> = Vec::with_capacity(l.len());
        for p in l {
            let mut q = s.invert(*p);
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
    let interior = grid(&loops2, step);
    let lift = |q: DVec2| s.eval(q / metric);
    let normal = |q: DVec2| s.normal(q / metric) * sense;
    triangulate(b, loops3, &loops2, &interior, &lift, &normal)
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
        let triangles = match &f.surface {
            Some(Surface::Plane { frame }) => flat_face(&mut b, &loops3, frame.z * sense),
            Some(s) if !loops3.is_empty() => curved_face(&mut b, s, sense, &loops3),
            Some(s) => closed_surface(&mut b, s, sense),
            None => {
                if f.surface_type != "POLY" {
                    warnings.push(format!("face #{} ({}) was approximated as flat", f.id, f.surface_type));
                }
                let n = loops3.first().map(|l| newell(l)).unwrap_or(DVec3::Z);
                flat_face(&mut b, &loops3, n)
            }
        };
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
