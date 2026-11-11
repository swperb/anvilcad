//! Checks on what the kernel returns: a deep audit of each body's B-rep and its display
//! mesh, and comparisons with the analytic reference.

use crate::csg::Tree;
use anvil_brep::tess::{self, TessParams, TriMesh};
use anvil_brep::{Solid, check, mass};
use glam::DVec3;
use serde::Serialize;

/// The tessellation the app shows (the kernel's default).
pub fn display_params() -> TessParams {
    TessParams::default()
}

/// A finer tessellation, for volumes and areas compared at tight tolerances.
pub fn fine_params() -> TessParams {
    TessParams { chord: 1e-3, max_angle: 0.02 }
}

/// How far an edge or vertex may stray from the surfaces it bounds. The kernel's own
/// "on surface" distance is 1e-5 mm.
pub const ON_SURFACE: f64 = 1e-5;

#[derive(Debug, Clone, Default, Serialize)]
pub struct Audit {
    /// Problems that make the body wrong. Empty when the body is sound.
    pub errors: Vec<String>,
    /// Oddities that are not wrong in themselves (sliver triangles, short edges).
    pub warnings: Vec<String>,
    pub faces: usize,
    pub edges: usize,
    pub vertices: usize,
    pub triangles: usize,
    pub genus: Option<u32>,
    /// Largest distance found from an edge to a surface it bounds.
    pub max_edge_gap: f64,
}

/// Audits one body: the kernel's own validity check, then geometry against topology
/// (every edge and vertex lies on the surfaces of the faces that use it), then the
/// display mesh (closed, consistently oriented, outward, normals agreeing with the
/// faces).
pub fn audit(solid: &Solid, mesh: &TriMesh) -> Audit {
    let mut a = Audit {
        faces: solid.faces.len(),
        edges: solid.edges.len(),
        vertices: solid.vertices.len(),
        triangles: mesh.triangles.len(),
        ..Audit::default()
    };
    if let Err(e) = check::validate(solid) {
        a.errors.push(format!("validate: {e}"));
    }
    a.genus = check::genus(solid).ok();

    // Edges on their faces' surfaces.
    let mut users: Vec<Vec<usize>> = vec![Vec::new(); solid.edges.len()];
    for (fi, f) in solid.faces.iter().enumerate() {
        for l in f.loops() {
            for c in &l.coedges {
                if let Some(u) = users.get_mut(c.edge.index()) {
                    u.push(fi);
                }
            }
        }
    }
    let mut bad_edges = 0;
    for (ei, e) in solid.edges.iter().enumerate() {
        let span = (e.curve.eval(e.t1) - e.curve.eval(e.t0)).length();
        if span < 1e-4 && !e.curve.is_periodic() {
            a.warnings.push(format!("edge {ei} is only {span:.2e} mm long"));
        }
        for k in 1..8 {
            let t = e.t0 + (e.t1 - e.t0) * k as f64 / 8.0;
            let p = e.curve.eval(t);
            for &fi in &users[ei] {
                let gap = solid.faces[fi].surface.distance(p);
                if !gap.is_finite() {
                    a.errors.push(format!("edge {ei} gives a non-finite distance to face {fi}"));
                    continue;
                }
                a.max_edge_gap = a.max_edge_gap.max(gap);
                if gap > ON_SURFACE * (1.0 + p.length() * 1e-6) {
                    bad_edges += 1;
                    if bad_edges <= 5 {
                        a.errors.push(format!("edge {ei} is {gap:.2e} mm off the surface of face {fi}"));
                    }
                }
            }
        }
    }
    if bad_edges > 5 {
        a.errors.push(format!("{} more edge samples off their surfaces", bad_edges - 5));
    }
    audit_mesh(solid, mesh, &mut a);
    a
}

fn audit_mesh(solid: &Solid, mesh: &TriMesh, a: &mut Audit) {
    if mesh.triangles.is_empty() {
        a.errors.push("the display mesh is empty".into());
        return;
    }
    if mesh.positions.iter().chain(&mesh.normals).any(|p| !p.is_finite()) {
        a.errors.push("the display mesh has a non-finite position or normal".into());
        return;
    }
    if !mesh.is_watertight() {
        let open = open_edges(mesh);
        let at = open.first().map(|(p, q)| format!(" (one from {p:.5} to {q:.5})")).unwrap_or_default();
        a.errors.push(format!(
            "the display mesh is not watertight: {} triangle edges not used once each way{at}",
            open.len()
        ));
    }
    let props = mass::properties(mesh);
    if props.volume <= 0.0 {
        a.errors.push(format!(
            "the display mesh encloses a volume of {:.3e} (faces point inwards)",
            props.volume
        ));
    }
    // Triangles against their faces' normals, and vertex normals against triangles.
    let mut flipped_area = 0.0;
    let mut area = 0.0;
    let mut bad_normals = 0usize;
    let mut bad_at = DVec3::ZERO;
    let mut slivers = 0usize;
    for (ti, t) in mesh.triangles.iter().enumerate() {
        let [p, q, r] = t.map(|i| mesh.positions[i as usize]);
        let cross = (q - p).cross(r - p);
        let ar = 0.5 * cross.length();
        area += ar;
        let longest = (q - p).length().max((r - q).length()).max((p - r).length());
        if ar < 1e-12 * longest.max(1.0) {
            slivers += 1;
            continue;
        }
        let n = cross.normalize_or_zero();
        let fi = mesh.triangle_face.get(ti).copied().unwrap_or(0) as usize;
        if let Some(f) = solid.faces.get(fi) {
            let c = (p + q + r) / 3.0;
            // Project the centroid onto the surface direction by using its normal there.
            if f.normal_at(c).dot(n) < 0.0 {
                flipped_area += ar;
            }
        }
        for &i in t {
            if mesh.normals.get(i as usize).is_some_and(|m| m.dot(n) < -0.2) {
                bad_normals += 1;
                bad_at = mesh.positions[i as usize];
            }
        }
    }
    if flipped_area > 1e-6 * area.max(1e-12) {
        a.errors.push(format!(
            "{:.3}% of the mesh area faces against its B-rep face's normal",
            100.0 * flipped_area / area
        ));
    }
    if bad_normals > 0 {
        a.errors.push(format!(
            "{bad_normals} mesh vertex normals point against their triangles (one at {:.4} {:.4} {:.4})",
            bad_at.x, bad_at.y, bad_at.z
        ));
    }
    if slivers > 0 {
        a.warnings.push(format!("{slivers} zero-area triangles"));
    }
}

/// The generalised winding number of a closed mesh about `p`: 1 inside, 0 outside,
/// robust to small cracks (van Oosterom and Strackee's solid angle).
pub fn winding(mesh: &TriMesh, p: DVec3) -> f64 {
    let mut sum = 0.0;
    for t in &mesh.triangles {
        let [a, b, c] = t.map(|i| mesh.positions[i as usize] - p);
        let (la, lb, lc) = (a.length(), b.length(), c.length());
        let num = a.dot(b.cross(c));
        let den = la * lb * lc + a.dot(b) * lc + b.dot(c) * la + c.dot(a) * lb;
        sum += 2.0 * num.atan2(den);
    }
    sum / (4.0 * std::f64::consts::PI)
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Membership {
    pub samples: usize,
    /// Samples too close to a primitive's boundary to judge, skipped.
    pub skipped: usize,
    pub wrong: usize,
    /// The first few wrong points, with whether the reference says inside.
    pub examples: Vec<([f64; 3], bool)>,
}

/// Compares inside and outside at points spread through the reference's box: the
/// kernel's meshes (by winding number) against the reference tree.
pub fn membership(tree: &Tree, meshes: &[TriMesh], n: usize, margin: f64, seed: u64) -> Membership {
    let (lo, hi) = tree.bounds();
    let pad = (hi - lo) * 0.05 + DVec3::splat(margin);
    let (lo, hi) = (lo - pad, hi + pad);
    let mut rng = crate::rng::Rng::new(seed);
    let mut m = Membership { samples: n, ..Membership::default() };
    for _ in 0..n {
        let p = DVec3::new(rng.range(lo.x, hi.x), rng.range(lo.y, hi.y), rng.range(lo.z, hi.z));
        if tree.boundary_distance(p) < margin {
            m.skipped += 1;
            continue;
        }
        let want = tree.contains(p);
        let w: f64 = meshes.iter().map(|mesh| winding(mesh, p)).sum();
        let got = w > 0.5;
        if got != want {
            m.wrong += 1;
            if m.examples.len() < 5 {
                m.examples.push((p.to_array(), want));
            }
        }
    }
    m
}

/// Tessellates every body, returning the first failure as text.
pub fn meshes(bodies: &[Solid], params: &TessParams) -> Result<Vec<TriMesh>, String> {
    bodies.iter().map(|b| tess::tessellate(b, params).map_err(|e| e.to_string())).collect()
}

/// Triangle edges not used exactly once in each direction, by position.
pub fn open_edges(mesh: &TriMesh) -> Vec<(DVec3, DVec3)> {
    use std::collections::HashMap;
    let key = |p: DVec3| (p.x.to_bits(), p.y.to_bits(), p.z.to_bits());
    let mut count: HashMap<((u64, u64, u64), (u64, u64, u64)), (i32, DVec3, DVec3)> = HashMap::new();
    for t in &mesh.triangles {
        for k in 0..3 {
            let (p, q) = (mesh.positions[t[k] as usize], mesh.positions[t[(k + 1) % 3] as usize]);
            count.entry((key(p), key(q))).or_insert((0, p, q)).0 += 1;
        }
    }
    // An edge is closed when it is used once each way.
    let mut open = Vec::new();
    for (k, (n, p, q)) in &count {
        let back = count.get(&(k.1, k.0)).map_or(0, |b| b.0);
        if *n != 1 || back != 1 {
            open.push((*p, *q));
        }
    }
    open
}
