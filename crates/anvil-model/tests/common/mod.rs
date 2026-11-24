//! Measurements shared by the format fixture generator and the format fixture tests.
//!
//! The generator records these values next to each fixture when the fixture is saved;
//! the tests measure the reopened file the same way and compare. Keep this file free of
//! anything newer than the oldest build that saves fixtures, so that it can be copied
//! into that build's tree and run there (see `fixture_generator.rs`).
#![allow(dead_code)]

use anvil_kernel::Solid;
use anvil_model::BuildState;
use serde_json::{json, Value};
use std::collections::HashMap;

/// The number of separate pieces in a faceted solid: polygons that share a vertex belong
/// to the same piece.
pub fn pieces(solid: &Solid) -> usize {
    let n = solid.polygons.len();
    let mut parent: Vec<usize> = (0..n).collect();
    fn root(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    let key = |v: anvil_kernel::Vec3| {
        let q = |x: f64| (x * 1e6).round() as i64;
        (q(v.x), q(v.y), q(v.z))
    };
    let mut seen: HashMap<(i64, i64, i64), usize> = HashMap::new();
    for (i, p) in solid.polygons.iter().enumerate() {
        for &v in &p.verts {
            match seen.get(&key(v)) {
                Some(&j) => {
                    let (a, b) = (root(&mut parent, i), root(&mut parent, j));
                    parent[a] = b;
                }
                None => {
                    seen.insert(key(v), i);
                }
            }
        }
    }
    (0..n).filter(|&i| root(&mut parent, i) == i).count()
}

/// The number of bodies a rebuilt part has: the exact kernel's bodies while the part is
/// exact, otherwise the pieces of the faceted body.
pub fn body_count(st: &BuildState) -> usize {
    match &st.exact {
        Some(bodies) => bodies.len(),
        None => pieces(&st.body),
    }
}

/// Body count, volume, surface area, face count and bounding box of some solids taken
/// together.
pub fn geometry(solids: &[&Solid], bodies: usize) -> Value {
    let (mut volume, mut area, mut faces) = (0.0, 0.0, 0);
    let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
    for s in solids {
        let m = s.mass_properties();
        volume += m.volume;
        area += m.surface_area;
        faces += s.face_ids().len();
        let b = s.bounds();
        for (k, (a, z)) in
            [(b.min.x, b.max.x), (b.min.y, b.max.y), (b.min.z, b.max.z)].into_iter().enumerate()
        {
            lo[k] = lo[k].min(a);
            hi[k] = hi[k].max(z);
        }
    }
    json!({
        "bodies": bodies,
        "volume": volume,
        "surface_area": area,
        "faces": faces,
        "bbox_min": lo,
        "bbox_max": hi,
    })
}

/// The geometry of a rebuilt part.
pub fn part_geometry(st: &BuildState) -> Value {
    geometry(&[&st.body], body_count(st))
}
