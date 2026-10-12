//! Real-world STEP files from NIST's CAD test models (see tests/data/NOTICE.md).

#![allow(clippy::expect_used)]

use anvil_step::{Mesh, import};
use glam::DVec3;
use std::collections::HashMap;

fn load(name: &str) -> anvil_step::Import {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data").join(name);
    let text = String::from_utf8_lossy(&std::fs::read(path).expect("read")).into_owned();
    import(&text).expect("import")
}

fn open_edges(m: &Mesh) -> usize {
    let mut count: HashMap<(u32, u32), i32> = HashMap::new();
    for t in m.faces.iter().flat_map(|f| &f.triangles) {
        for k in 0..3 {
            *count.entry((t[k], t[(k + 1) % 3])).or_default() += 1;
        }
    }
    count.iter().filter(|&(&(a, b), &n)| n != 1 || count.get(&(b, a)) != Some(&1)).count()
}

fn volume(m: &Mesh) -> f64 {
    let p = |i: u32| DVec3::from_array(m.vertices[i as usize]);
    m.faces.iter().flat_map(|f| &f.triangles).map(|t| p(t[0]).dot(p(t[1]).cross(p(t[2]))) / 6.0).sum()
}

/// Cylinders and tori bounded by two circles and no seam edge, the way several CAD
/// systems write faces of revolution.
#[test]
fn ftc_11_bands_without_seams() {
    let imp = load("nist_ftc_11_asme1_rb.stp");
    let solid = imp.bodies.iter().find(|b| b.solid).expect("solid");
    assert_eq!(open_edges(&solid.mesh), 0);
    let v = volume(&solid.mesh);
    assert!((v - 5103.7).abs() / 5103.7 < 2e-3, "{v}");
}

/// Sphere patches with a vertex on the pole, in inch units.
#[test]
fn ftc_06_pole_vertices() {
    let imp = load("nist_ftc_06_asme1_rd.stp");
    assert_eq!(imp.length_unit, "inch");
    let solid = imp.bodies.iter().find(|b| b.solid).expect("solid");
    assert_eq!(open_edges(&solid.mesh), 0);
    let v = volume(&solid.mesh);
    assert!((v - 3327243.1).abs() / 3327243.1 < 2e-3, "{v}");
}
