//! Boundary representation of solids.
//!
//! In this first kernel generation a solid is a closed set of planar convex polygons.
//! Each polygon carries a [`FaceTag`] naming the model face it belongs to and the exact
//! surface that face lies on (plane, cylinder, surface of revolution). Exact geometry is
//! kept in the tags and in the profiles that built the solid; booleans run on the
//! faceted boundary. Exact curved-surface booleans are the next kernel milestone.

use crate::math::{Aabb, Frame, Vec3};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Stable name of a model face: the feature that created it plus a feature-local index.
/// Feature-local indices are 0 = start cap, 1 = end cap, 2 + n = side face from sketch entity n.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct FaceId {
    pub feature: u32,
    pub index: u32,
}

impl FaceId {
    pub const START_CAP: u32 = 0;
    pub const END_CAP: u32 = 1;
    pub fn side(feature: u32, entity: u32) -> FaceId {
        FaceId { feature, index: 2 + entity }
    }
}

/// An edge named by the two faces it separates (stored in sorted order). This survives
/// rebuilds as well as the face ids themselves do.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct EdgeRef {
    pub a: FaceId,
    pub b: FaceId,
}

impl EdgeRef {
    pub fn new(a: FaceId, b: FaceId) -> EdgeRef {
        if a <= b {
            EdgeRef { a, b }
        } else {
            EdgeRef { a: b, b: a }
        }
    }
}

/// A drawable model edge segment with the faces on either side.
#[derive(Clone, Copy, Debug)]
pub struct ModelEdge {
    pub seg: [Vec3; 2],
    pub faces: EdgeRef,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum SurfaceKind {
    Plane,
    Cylinder {
        axis_origin: Vec3,
        axis_dir: Vec3,
        radius: f64,
    },
    Revolved {
        axis_origin: Vec3,
        axis_dir: Vec3,
    },
    /// Any other curved surface (for example imported freeform faces).
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FaceTag {
    pub id: FaceId,
    pub surface: SurfaceKind,
}

/// A planar convex polygon with outward normal. `w` is the plane offset (normal . p = w).
#[derive(Clone, Debug)]
pub struct Polygon {
    pub verts: Vec<Vec3>,
    pub normal: Vec3,
    pub w: f64,
    pub tag: FaceTag,
}

impl Polygon {
    /// Builds a polygon, computing its plane with Newell's method. Returns `None` for
    /// degenerate input.
    pub fn new(verts: Vec<Vec3>, tag: FaceTag) -> Option<Polygon> {
        if verts.len() < 3 {
            return None;
        }
        let mut n = Vec3::ZERO;
        let k = verts.len();
        for i in 0..k {
            let (a, b) = (verts[i], verts[(i + 1) % k]);
            n.x += (a.y - b.y) * (a.z + b.z);
            n.y += (a.z - b.z) * (a.x + b.x);
            n.z += (a.x - b.x) * (a.y + b.y);
        }
        let len = n.length();
        if len < 1e-14 {
            return None;
        }
        let normal = n / len;
        let centroid = verts.iter().fold(Vec3::ZERO, |s, &p| s + p) / k as f64;
        Some(Polygon { w: normal.dot(centroid), verts, normal, tag })
    }

    pub fn flip(&mut self) {
        self.verts.reverse();
        self.normal = -self.normal;
        self.w = -self.w;
    }

    pub fn area(&self) -> f64 {
        let mut a = Vec3::ZERO;
        for i in 1..self.verts.len().saturating_sub(1) {
            a += (self.verts[i] - self.verts[0]).cross(self.verts[i + 1] - self.verts[0]);
        }
        a.length() * 0.5
    }
}

#[derive(Clone, Debug, Default)]
pub struct Solid {
    pub polygons: Vec<Polygon>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MassProperties {
    pub volume: f64,
    pub surface_area: f64,
    pub centroid: Vec3,
}

/// Triangle mesh for rendering and export.
#[derive(Clone, Debug, Default)]
pub struct Mesh {
    pub positions: Vec<Vec3>,
    pub normals: Vec<Vec3>,
    /// Which face each vertex belongs to (parallel to `positions`).
    pub faces: Vec<FaceId>,
    pub indices: Vec<[u32; 3]>,
}

#[derive(Clone, Copy, Debug)]
pub struct RayHit {
    pub t: f64,
    pub point: Vec3,
    pub normal: Vec3,
    pub tag: FaceTag,
}

impl Solid {
    pub fn new(polygons: Vec<Polygon>) -> Solid {
        Solid { polygons }
    }

    pub fn is_empty(&self) -> bool {
        self.polygons.is_empty()
    }

    pub fn bounds(&self) -> Aabb {
        let mut b = Aabb::empty();
        for p in &self.polygons {
            for &v in &p.verts {
                b.add(v);
            }
        }
        b
    }

    pub fn flip(&mut self) {
        for p in &mut self.polygons {
            p.flip();
        }
    }

    pub fn transformed(&self, f: &Frame) -> Solid {
        Solid::new(
            self.polygons
                .iter()
                .filter_map(|p| {
                    let mut tag = p.tag;
                    tag.surface = match tag.surface {
                        SurfaceKind::Plane => SurfaceKind::Plane,
                        SurfaceKind::Other => SurfaceKind::Other,
                        SurfaceKind::Cylinder { axis_origin, axis_dir, radius } => SurfaceKind::Cylinder {
                            axis_origin: f.to_world3(axis_origin),
                            axis_dir: f.dir_to_world(axis_dir),
                            radius,
                        },
                        SurfaceKind::Revolved { axis_origin, axis_dir } => SurfaceKind::Revolved {
                            axis_origin: f.to_world3(axis_origin),
                            axis_dir: f.dir_to_world(axis_dir),
                        },
                    };
                    Polygon::new(p.verts.iter().map(|&v| f.to_world3(v)).collect(), tag)
                })
                .collect(),
        )
    }

    /// The solid reflected in the plane through `origin` with normal `normal`. Each
    /// polygon's winding is reversed so normals still point out.
    pub fn mirrored(&self, origin: Vec3, normal: Vec3) -> Solid {
        let n = normal.normalized();
        let pt = |v: Vec3| v - n * (2.0 * (v - origin).dot(n));
        let dir = |v: Vec3| v - n * (2.0 * v.dot(n));
        Solid::new(
            self.polygons
                .iter()
                .filter_map(|p| {
                    let mut tag = p.tag;
                    tag.surface = match tag.surface {
                        SurfaceKind::Plane => SurfaceKind::Plane,
                        SurfaceKind::Other => SurfaceKind::Other,
                        SurfaceKind::Cylinder { axis_origin, axis_dir, radius } => SurfaceKind::Cylinder {
                            axis_origin: pt(axis_origin),
                            axis_dir: dir(axis_dir),
                            radius,
                        },
                        SurfaceKind::Revolved { axis_origin, axis_dir } => {
                            SurfaceKind::Revolved { axis_origin: pt(axis_origin), axis_dir: dir(axis_dir) }
                        }
                    };
                    Polygon::new(p.verts.iter().rev().map(|&v| pt(v)).collect(), tag)
                })
                .collect(),
        )
    }

    /// Volume, area and centroid via the divergence theorem over a fan triangulation.
    pub fn mass_properties(&self) -> MassProperties {
        let mut vol = 0.0;
        let mut area = 0.0;
        let mut c = Vec3::ZERO;
        for p in &self.polygons {
            let v0 = p.verts[0];
            for i in 1..p.verts.len() - 1 {
                let (v1, v2) = (p.verts[i], p.verts[i + 1]);
                let cr = v1.cross(v2);
                let tv = v0.dot(cr) / 6.0;
                vol += tv;
                c += (v0 + v1 + v2) * (tv / 4.0);
                area += (v1 - v0).cross(v2 - v0).length() * 0.5;
            }
        }
        MassProperties {
            volume: vol,
            surface_area: area,
            centroid: if vol.abs() > 1e-12 { c / vol } else { Vec3::ZERO },
        }
    }

    /// Closest intersection of a ray with the boundary.
    pub fn raycast(&self, origin: Vec3, dir: Vec3) -> Option<RayHit> {
        let mut best: Option<RayHit> = None;
        for p in &self.polygons {
            let denom = p.normal.dot(dir);
            if denom.abs() < 1e-12 {
                continue;
            }
            let t = (p.w - p.normal.dot(origin)) / denom;
            if t < 0.0 || best.is_some_and(|b| t >= b.t) {
                continue;
            }
            let hit = origin + dir * t;
            // Convex polygon containment.
            let k = p.verts.len();
            let inside = (0..k).all(|i| {
                let (a, b) = (p.verts[i], p.verts[(i + 1) % k]);
                (b - a).cross(hit - a).dot(p.normal) >= -1e-9
            });
            if inside {
                best = Some(RayHit { t, point: hit, normal: p.normal, tag: p.tag });
            }
        }
        best
    }

    /// All polygons belonging to a face.
    pub fn face_polygons(&self, id: FaceId) -> impl Iterator<Item = &Polygon> {
        self.polygons.iter().filter(move |p| p.tag.id == id)
    }

    /// If `id` names a planar face, returns a sketch frame on it. The origin is the world
    /// origin projected onto the plane so it does not move when the face is resized.
    pub fn planar_face_frame(&self, id: FaceId) -> Option<Frame> {
        let p = self.face_polygons(id).next()?;
        if p.tag.surface != SurfaceKind::Plane {
            return None;
        }
        let n = p.normal;
        let origin = n * p.w;
        let hint = [Vec3::X, Vec3::Y, Vec3::Z]
            .into_iter()
            .min_by(|a, b| a.dot(n).abs().partial_cmp(&b.dot(n).abs()).unwrap())
            .unwrap();
        Some(Frame::from_normal(origin, n, hint))
    }

    /// Distinct face ids present in the solid, sorted.
    pub fn face_ids(&self) -> Vec<FaceId> {
        let mut ids: Vec<FaceId> = self.polygons.iter().map(|p| p.tag.id).collect();
        ids.sort();
        ids.dedup();
        ids
    }

    /// Triangulates the boundary. Planar faces get flat normals; curved faces get smooth
    /// normals averaged across polygons of the same face.
    pub fn tessellate(&self) -> Mesh {
        let mut mesh = Mesh::default();
        // Smooth normal accumulation keyed by (face, quantized position).
        let q = |v: Vec3| {
            let s = 1e5;
            ((v.x * s).round() as i64, (v.y * s).round() as i64, (v.z * s).round() as i64)
        };
        let mut smooth: HashMap<(FaceId, (i64, i64, i64)), Vec3> = HashMap::new();
        for p in &self.polygons {
            if p.tag.surface != SurfaceKind::Plane {
                let a = p.area();
                for &v in &p.verts {
                    *smooth.entry((p.tag.id, q(v))).or_insert(Vec3::ZERO) += p.normal * a;
                }
            }
        }
        for p in &self.polygons {
            let base = mesh.positions.len() as u32;
            for &v in &p.verts {
                let n = if p.tag.surface == SurfaceKind::Plane {
                    p.normal
                } else {
                    let s = smooth[&(p.tag.id, q(v))].normalized();
                    // Keep hard edges where the smoothed normal deviates a lot.
                    if s.dot(p.normal) > 0.5 {
                        s
                    } else {
                        p.normal
                    }
                };
                mesh.positions.push(v);
                mesh.normals.push(n);
                mesh.faces.push(p.tag.id);
            }
            for i in 1..p.verts.len() as u32 - 1 {
                mesh.indices.push([base, base + i, base + i + 1]);
            }
        }
        mesh
    }

    /// Line segments along face boundaries (edges between different faces, or creases),
    /// for drawing model edges.
    pub fn feature_edges(&self) -> Vec<[Vec3; 2]> {
        self.model_edges().into_iter().map(|e| e.seg).collect()
    }

    /// Segments shared by exactly two polygons: (segment, polygon index, polygon index).
    pub fn shared_segments(&self) -> Vec<([Vec3; 2], usize, usize)> {
        let q = |v: Vec3| {
            let s = 1e4;
            ((v.x * s).round() as i64, (v.y * s).round() as i64, (v.z * s).round() as i64)
        };
        type Key = ((i64, i64, i64), (i64, i64, i64));
        let mut map: HashMap<Key, Vec<(usize, [Vec3; 2])>> = HashMap::new();
        for (pi, p) in self.polygons.iter().enumerate() {
            let k = p.verts.len();
            for i in 0..k {
                let (a, b) = (p.verts[i], p.verts[(i + 1) % k]);
                let (ka, kb) = (q(a), q(b));
                if ka == kb {
                    continue;
                }
                let key = if ka < kb { (ka, kb) } else { (kb, ka) };
                map.entry(key).or_default().push((pi, [a, b]));
            }
        }
        map.into_values().filter(|u| u.len() == 2).map(|u| (u[0].1, u[0].0, u[1].0)).collect()
    }

    /// The model edge passing nearest to `p`, if within `tol`.
    pub fn edge_near(&self, p: Vec3, tol: f64) -> Option<EdgeRef> {
        let mut best: Option<(f64, EdgeRef)> = None;
        for e in self.model_edges() {
            let (a, b) = (e.seg[0], e.seg[1]);
            let ab = b - a;
            let t =
                if ab.length_sq() < 1e-18 { 0.0 } else { ((p - a).dot(ab) / ab.length_sq()).clamp(0.0, 1.0) };
            let d = (a + ab * t).distance(p);
            if d <= tol && best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, e.faces));
            }
        }
        best.map(|b| b.1)
    }

    /// The face containing point `p` (within `tol` of its surface).
    pub fn face_at(&self, p: Vec3, tol: f64) -> Option<FaceId> {
        let mut best: Option<(f64, FaceId)> = None;
        for poly in &self.polygons {
            let d = (poly.normal.dot(p) - poly.w).abs();
            if d > tol || best.is_some_and(|(bd, _)| d >= bd) {
                continue;
            }
            let q = p - poly.normal * (poly.normal.dot(p) - poly.w);
            let k = poly.verts.len();
            let inside = (0..k).all(|i| {
                let (a, b) = (poly.verts[i], poly.verts[(i + 1) % k]);
                (b - a).cross(q - a).dot(poly.normal) >= -tol * (b - a).length()
            });
            if inside {
                best = Some((d, poly.tag.id));
            }
        }
        best.map(|b| b.1)
    }

    /// Visible model edges (between different faces, or sharp creases), each with the two
    /// faces it separates.
    pub fn model_edges(&self) -> Vec<ModelEdge> {
        self.shared_segments()
            .into_iter()
            .filter_map(|(seg, i, j)| {
                let (p0, p1) = (&self.polygons[i], &self.polygons[j]);
                (p0.tag.id != p1.tag.id || p0.normal.dot(p1.normal) < 0.94)
                    .then(|| ModelEdge { seg, faces: EdgeRef::new(p0.tag.id, p1.tag.id) })
            })
            .collect()
    }

    /// Inserts vertices that lie in the middle of other polygons' edges, so every edge is
    /// shared exactly by two polygons. Booleans leave such T-junctions behind; removing
    /// them makes the boundary watertight for export and edge display.
    pub fn repair_t_junctions(&mut self) {
        const EPS: f64 = 1e-6;
        let q = |v: Vec3| {
            let s = 1e7;
            ((v.x * s).round() as i64, (v.y * s).round() as i64, (v.z * s).round() as i64)
        };
        let mut seen = std::collections::HashSet::new();
        let mut verts: Vec<Vec3> = Vec::new();
        for p in &self.polygons {
            for &v in &p.verts {
                if seen.insert(q(v)) {
                    verts.push(v);
                }
            }
        }
        let coord = |v: Vec3, axis: usize| [v.x, v.y, v.z][axis];
        let sorted: Vec<Vec<usize>> = (0..3)
            .map(|axis| {
                let mut idx: Vec<usize> = (0..verts.len()).collect();
                idx.sort_by(|&a, &b| coord(verts[a], axis).partial_cmp(&coord(verts[b], axis)).unwrap());
                idx
            })
            .collect();

        let mut on_edge: Vec<(f64, Vec3)> = Vec::new();
        for p in &mut self.polygons {
            let k = p.verts.len();
            let mut out = Vec::with_capacity(k);
            for i in 0..k {
                let (a, b) = (p.verts[i], p.verts[(i + 1) % k]);
                out.push(a);
                let d = b - a;
                let len2 = d.length_sq();
                if len2 < EPS * EPS {
                    continue;
                }
                // Search along the axis where the edge is shortest: the narrowest window.
                let ext = [d.x.abs(), d.y.abs(), d.z.abs()];
                let axis = (0..3).min_by(|&x, &y| ext[x].partial_cmp(&ext[y]).unwrap()).unwrap();
                let (lo, hi) =
                    (coord(a, axis).min(coord(b, axis)) - EPS, coord(a, axis).max(coord(b, axis)) + EPS);
                let list = &sorted[axis];
                let start = list.partition_point(|&j| coord(verts[j], axis) < lo);
                on_edge.clear();
                for &j in &list[start..] {
                    let v = verts[j];
                    if coord(v, axis) > hi {
                        break;
                    }
                    let t = (v - a).dot(d) / len2;
                    if t <= 1e-9 || t >= 1.0 - 1e-9 {
                        continue;
                    }
                    if (a + d * t - v).length_sq() < EPS * EPS && v.distance(a) > EPS && v.distance(b) > EPS {
                        on_edge.push((t, v));
                    }
                }
                if !on_edge.is_empty() {
                    on_edge.sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap());
                    let mut last = a;
                    for &(_, v) in &on_edge {
                        if v.distance(last) > EPS {
                            out.push(v);
                            last = v;
                        }
                    }
                }
            }
            p.verts = out;
        }
    }

    /// Edges used by a number of polygons other than two. Empty for a closed, watertight
    /// boundary.
    pub fn open_edges(&self) -> usize {
        let q = |v: Vec3| {
            let s = 1e5;
            ((v.x * s).round() as i64, (v.y * s).round() as i64, (v.z * s).round() as i64)
        };
        let mut count: HashMap<((i64, i64, i64), (i64, i64, i64)), i32> = HashMap::new();
        for p in &self.polygons {
            let k = p.verts.len();
            for i in 0..k {
                let (a, b) = (q(p.verts[i]), q(p.verts[(i + 1) % k]));
                if a != b {
                    // Directed: +1 one way, -1 the other. Closed manifolds cancel out.
                    if a < b {
                        *count.entry((a, b)).or_default() += 1;
                    } else {
                        *count.entry((b, a)).or_default() -= 1;
                    }
                }
            }
        }
        count.values().filter(|&&c| c != 0).count()
    }
}
