//! Display tessellation.
//!
//! Each edge is sampled once and the samples are shared by both faces that use
//! it, so the mesh is watertight by construction. Faces are triangulated in
//! their surface's parameter space with a constrained Delaunay triangulation.

use crate::geom::Surface;
use crate::topo::{Face, Solid};
use crate::{KernelError, Result, tol};
use glam::{DVec2, DVec3};
use spade::{ConstrainedDelaunayTriangulation, Point2, Triangulation};
use std::collections::HashMap;
use std::f64::consts::TAU;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TessParams {
    /// Maximum distance between a curve and its chords (mm).
    pub chord: f64,
    /// Maximum angle subtended by one chord (radians).
    pub max_angle: f64,
}

impl Default for TessParams {
    fn default() -> Self {
        Self { chord: tol::CHORD, max_angle: tol::MAX_ANGLE }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct TriMesh {
    pub positions: Vec<DVec3>,
    pub normals: Vec<DVec3>,
    pub triangles: Vec<[u32; 3]>,
    /// Index of the B-rep face each triangle belongs to.
    pub triangle_face: Vec<u32>,
    /// Polyline for each B-rep edge, indexed like `Solid::edges`.
    pub edges: Vec<Vec<DVec3>>,
}

impl TriMesh {
    /// True when every triangle edge is matched by exactly one opposite edge
    /// in another triangle (closed and consistently oriented).
    pub fn is_watertight(&self) -> bool {
        let key = |p: DVec3| (p.x.to_bits(), p.y.to_bits(), p.z.to_bits());
        let mut ids: HashMap<(u64, u64, u64), u32> = HashMap::new();
        let canon: Vec<u32> = self
            .positions
            .iter()
            .map(|p| {
                let n = ids.len() as u32;
                *ids.entry(key(*p)).or_insert(n)
            })
            .collect();
        let mut directed: HashMap<(u32, u32), i32> = HashMap::new();
        for t in &self.triangles {
            for k in 0..3 {
                let a = canon[t[k] as usize];
                let b = canon[t[(k + 1) % 3] as usize];
                *directed.entry((a, b)).or_default() += 1;
            }
        }
        directed.iter().all(|(&(a, b), &n)| n == 1 && directed.get(&(b, a)) == Some(&1))
    }

    pub fn bounds(&self) -> Option<(DVec3, DVec3)> {
        let first = *self.positions.first()?;
        Some(self.positions.iter().fold((first, first), |(lo, hi), p| (lo.min(*p), hi.max(*p))))
    }
}

pub fn tessellate(solid: &Solid, params: &TessParams) -> Result<TriMesh> {
    let mut edges = Vec::with_capacity(solid.edges.len());
    for e in &solid.edges {
        let mut pts = e.curve.sample(e.t0, e.t1, params.chord, params.max_angle);
        // Pin the ends to the vertices so neighbouring edges share exact points.
        let (Some(a), Some(b)) = (solid.vertex(e.start), solid.vertex(e.end)) else {
            return Err(KernelError::Tessellation("edge without vertices".into()));
        };
        if let Some(first) = pts.first_mut() {
            *first = a.point;
        }
        if let Some(last) = pts.last_mut() {
            *last = b.point;
        }
        edges.push(pts);
    }
    let mut mesh = TriMesh { edges, ..TriMesh::default() };
    for (fi, face) in solid.faces.iter().enumerate() {
        tessellate_face(face, fi as u32, &mut mesh, params)
            .map_err(|e| KernelError::Tessellation(format!("face {fi}: {e}")))?;
    }
    Ok(mesh)
}

fn tessellate_face(face: &Face, face_index: u32, mesh: &mut TriMesh, params: &TessParams) -> Result<()> {
    // Closed 3D polylines for each loop, without the repeated end point.
    let mut loops3: Vec<Vec<DVec3>> = Vec::new();
    for l in face.loops() {
        let mut pts = Vec::new();
        for c in &l.coedges {
            let samples = mesh
                .edges
                .get(c.edge.index())
                .ok_or_else(|| KernelError::Tessellation("missing edge".into()))?;
            let n = samples.len();
            if c.reversed {
                pts.extend(samples.iter().rev().take(n.saturating_sub(1)));
            } else {
                pts.extend(samples.iter().take(n.saturating_sub(1)));
            }
        }
        if pts.len() < 2 {
            return Err(KernelError::Tessellation("loop has too few points".into()));
        }
        loops3.push(pts);
    }

    let mut loops_uv: Vec<Vec<DVec2>> = loops3.iter().map(|l| to_uv(&face.surface, l)).collect();
    if let Some(period) = face.surface.u_period() {
        align_holes(&mut loops_uv, period);
    }

    let mut cdt: ConstrainedDelaunayTriangulation<Point2<f64>> = ConstrainedDelaunayTriangulation::new();
    // spade vertex index -> mesh vertex index
    let mut vmap: Vec<Option<u32>> = Vec::new();
    for (l3, luv) in loops3.iter().zip(&loops_uv) {
        let mut handles = Vec::with_capacity(l3.len());
        for (p, uv) in l3.iter().zip(luv) {
            let h = cdt
                .insert(Point2::new(uv.x, uv.y))
                .map_err(|e| KernelError::Tessellation(format!("{e:?}")))?;
            let hi = h.index();
            if vmap.len() <= hi {
                vmap.resize(hi + 1, None);
            }
            if vmap[hi].is_none() {
                vmap[hi] = Some(mesh.positions.len() as u32);
                mesh.positions.push(*p);
                mesh.normals.push(face.normal_at(*p));
            }
            handles.push(h);
        }
        for k in 0..handles.len() {
            let (a, b) = (handles[k], handles[(k + 1) % handles.len()]);
            if a == b {
                continue;
            }
            if !cdt.can_add_constraint(a, b) {
                return Err(KernelError::Tessellation("boundary crosses itself in parameter space".into()));
            }
            cdt.add_constraint(a, b);
        }
    }

    // Curved faces get interior points on a grid, so triangles stay small in
    // the curved direction instead of spanning chords across the surface.
    if let Surface::Cylinder { radius: scale, .. }
    | Surface::Cone { radius: scale, .. }
    | Surface::Torus { major: scale, .. }
    | Surface::Sphere { radius: scale, .. } = face.surface
    {
        // The angle step comes from the widest point of the face; `u` is the angle
        // times `scale`.
        let frame = face.surface.frame();
        let radius = loops3
            .iter()
            .flatten()
            .map(|p| {
                let l = frame.to_local(*p);
                DVec2::new(l.x, l.y).length()
            })
            .fold(0.0, f64::max);
        let step_for = |radius: f64| {
            let mut a = params.max_angle.max(1e-3);
            if params.chord > 0.0 && params.chord < radius {
                a = a.min(2.0 * (1.0 - params.chord / radius).acos());
            }
            a
        };
        // A sphere's parameters are lengths near the face's middle, at its own radius.
        let step_angle =
            if matches!(face.surface, Surface::Sphere { .. }) { step_for(scale) } else { step_for(radius) };
        // A cone laid out across its axis has lengths for parameters, like a sphere.
        let across_axis = matches!(face.surface, Surface::Cone { apex: true, .. });
        let mut du = if across_axis { radius * step_angle } else { scale * step_angle };
        let all: Vec<DVec2> = loops_uv.iter().flatten().copied().collect();
        let (mut lo, mut hi) = (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY));
        for p in &all {
            lo = lo.min(*p);
            hi = hi.max(*p);
        }
        // Along a torus's tube `v` is the tube angle times `minor`.
        let v_step = match face.surface {
            Surface::Torus { minor, .. } => minor * step_for(minor),
            Surface::Sphere { radius, .. } => radius * step_angle,
            _ => radius * step_angle,
        };
        // Cylinders and cones are straight along `v`, so a few rows do; a sphere is not.
        let dv = if across_axis {
            du
        } else if matches!(face.surface, Surface::Sphere { .. }) {
            v_step
        } else {
            v_step.max((hi.y - lo.y) / 64.0)
        };
        // A torus curves both ways, so long thin triangles across its tube tilt badly
        // (and show as creases): keep its grid cells within 2:1.
        if matches!(face.surface, Surface::Torus { .. }) {
            du = du.min(2.0 * dv);
        }
        // At most 200 000 grid points: past that, widen the spacing rather than drop it.
        let cells = ((hi.x - lo.x) / du).max(1.0) * ((hi.y - lo.y) / dv).max(1.0);
        let widen = (cells / 200_000.0).sqrt().max(1.0);
        let (du, dv) = (du * widen, dv * widen);
        let nu = ((hi.x - lo.x) / du).ceil() as usize;
        let nv = ((hi.y - lo.y) / dv).ceil() as usize;
        if du.is_finite() && dv.is_finite() && du > 0.0 && dv > 0.0 {
            let near_boundary = |q: DVec2| {
                loops_uv.iter().any(|l| {
                    (0..l.len()).any(|i| {
                        let (a, b) = (l[i], l[(i + 1) % l.len()]);
                        let ab = b - a;
                        let t = ((q - a).dot(ab) / ab.length_squared().max(1e-30)).clamp(0.0, 1.0);
                        (a + ab * t - q).length() < 0.3 * du.min(dv)
                    })
                })
            };
            // Candidate points: a grid, or on a cone laid out across its axis, rings round
            // the apex with enough points on each to keep the angle step (a square grid
            // would leave the tight curvature near the tip too coarse).
            let candidates: Vec<DVec2> = if across_axis {
                let reach = loops_uv.iter().flatten().map(|q| q.length()).fold(0.0, f64::max);
                let per_turn = (TAU / step_angle).ceil().max(8.0);
                let mut dr = du;
                while (reach / dr) * (per_turn + TAU * reach / dr) > 200_000.0 {
                    dr *= 1.5;
                }
                let mut pts = vec![DVec2::ZERO];
                let rings = (reach / dr).ceil() as usize;
                for k in 1..rings {
                    let rho = dr * k as f64;
                    let n = per_turn.max((TAU * rho / dr).ceil()) as usize;
                    // Stagger alternate rings so triangles between them are not slivers.
                    let shift = if k % 2 == 0 { 0.5 } else { 0.0 };
                    pts.extend((0..n).map(|j| {
                        let a = TAU * (j as f64 + shift) / n as f64;
                        DVec2::new(a.cos(), a.sin()) * rho
                    }));
                }
                pts
            } else {
                (1..nu)
                    .flat_map(|i| {
                        (0..=nv).map(move |j| DVec2::new(lo.x + du * i as f64, lo.y + dv * j as f64))
                    })
                    .collect()
            };
            for q in candidates {
                let inside =
                    loops_uv.iter().filter(|l| crate::profile::point_in_polygon(q, l)).count() % 2 == 1;
                if !inside || near_boundary(q) {
                    continue;
                }
                let h = cdt
                    .insert(Point2::new(q.x, q.y))
                    .map_err(|e| KernelError::Tessellation(format!("{e:?}")))?;
                let hi = h.index();
                if vmap.len() <= hi {
                    vmap.resize(hi + 1, None);
                }
                if vmap[hi].is_none() {
                    let p = face.surface.eval_uv(q);
                    vmap[hi] = Some(mesh.positions.len() as u32);
                    mesh.positions.push(p);
                    mesh.normals.push(face.normal_at(p));
                }
            }
        }
    }

    // A triangle is inside when reaching it from outside crosses the boundary an odd
    // number of times. Unlike a centroid test, this is right for the slivers that
    // nearly collinear boundary points (a circle on a cylinder, say) leave behind.
    let parity = boundary_parity(&cdt);
    for tri in cdt.inner_faces() {
        if !parity.get(tri.fix().index()).copied().unwrap_or(false) {
            continue;
        }
        let vs = tri.vertices();
        let uv: Vec<DVec2> = vs.iter().map(|v| DVec2::new(v.position().x, v.position().y)).collect();
        let mut idx = [0u32; 3];
        for (k, v) in vs.iter().enumerate() {
            idx[k] = vmap
                .get(v.fix().index())
                .copied()
                .flatten()
                .ok_or_else(|| KernelError::Tessellation("unmapped vertex".into()))?;
        }
        let ccw = (uv[1] - uv[0]).perp_dot(uv[2] - uv[0]) > 0.0;
        if ccw == face.reversed {
            idx.swap(1, 2);
        }
        mesh.triangles.push(idx);
        mesh.triangle_face.push(face_index);
    }
    Ok(())
}

/// On a surface that wraps around, puts each inner loop on the outer loop's branch
/// of the angle: shifted by whole periods so its middle falls in the outer loop's
/// range of `u`. (Matching first points is not enough: a hole at -90 degrees in a
/// face running from 0 to 360 would land outside it.)
pub(crate) fn align_holes(loops: &mut [Vec<DVec2>], period: f64) {
    let Some((outer, inners)) = loops.split_first_mut() else { return };
    let Some(lo) = outer.iter().map(|p| p.x).min_by(f64::total_cmp) else { return };
    for l in inners {
        let (a, b) = l.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), p| (a.min(p.x), b.max(p.x)));
        if !a.is_finite() {
            continue;
        }
        let mid = 0.5 * (a + b);
        let shift = period * ((lo - mid) / period).ceil();
        for p in l.iter_mut() {
            p.x += shift;
        }
    }
}

/// For every face of the triangulation, whether it lies inside the constraints.
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

/// Maps a closed loop on a surface to parameter space. Parameters are scaled
/// to millimetres so the Delaunay criterion sees true proportions.
fn to_uv(surface: &Surface, pts: &[DVec3]) -> Vec<DVec2> {
    match surface {
        Surface::Plane { frame } => pts
            .iter()
            .map(|p| {
                let l = frame.to_local(*p);
                DVec2::new(l.x, l.y)
            })
            .collect(),
        Surface::Sphere { .. } => pts.iter().map(|p| surface.uv(*p)).collect(),
        Surface::Cone { apex: true, .. } => pts.iter().map(|p| surface.uv(*p)).collect(),
        Surface::Cylinder { .. } | Surface::Cone { .. } | Surface::Torus { .. } => {
            let period = surface.u_period().unwrap_or(TAU);
            let mut prev: Option<f64> = None;
            pts.iter()
                .map(|p| {
                    let mut q = surface.uv(*p);
                    if let Some(pr) = prev {
                        q.x += period * ((pr - q.x) / period).round();
                    }
                    prev = Some(q.x);
                    q
                })
                .collect()
        }
    }
}
