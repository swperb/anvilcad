//! Real-world STEP files from NIST's CAD test models (see tests/data/NOTICE.md).

#![allow(clippy::expect_used, clippy::panic)]

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
    // Its fillets are tori, so the body is exact too. Finely tessellated, the exact
    // solid converges on 5129.0 mm^3; the importer's coarser mesh above runs about
    // 0.5% low on the 1.5 mm fillets.
    let exact = solid.exact.as_ref().unwrap_or_else(|| panic!("not exact: {:?}", solid.not_exact));
    assert!(exact.faces.iter().any(|f| matches!(f.surface, anvil_brep::Surface::Torus { .. })));
    let params = anvil_brep::tess::TessParams { chord: 2e-4, max_angle: 0.01 };
    let m = anvil_brep::tess::tessellate(exact, &params).expect("tessellate");
    assert!(m.is_watertight());
    let v = anvil_brep::mass::properties(&m).volume;
    assert!((v - 5129.0).abs() / 5129.0 < 2e-4, "{v}");
}

/// Sphere patches with a vertex on the pole, in inch units. Meshed by stereographic
/// projection, the three NIST versions of this part (from different CAD systems) agree
/// on its volume to 0.001%: 3323431.7 here, 3323411.4 (AP242 e2), 3323425.8 (stc_06).
#[test]
fn ftc_06_pole_vertices() {
    let imp = load("nist_ftc_06_asme1_rd.stp");
    assert_eq!(imp.length_unit, "inch");
    let solid = imp.bodies.iter().find(|b| b.solid).expect("solid");
    assert_eq!(open_edges(&solid.mesh), 0);
    let v = volume(&solid.mesh);
    assert!((v - 3323425.0).abs() / 3323425.0 < 5e-5, "{v}");
}

/// The exact solid of a file, checked to tessellate watertight in the kernel with the
/// same volume as the importer's own mesh, and to survive an exact STEP round trip.
fn exact_matches_mesh(name: &str) -> anvil_brep::Solid {
    let imp = load(name);
    let body = imp.bodies.iter().find(|b| b.solid).expect("solid");
    let exact = body.exact.clone().unwrap_or_else(|| panic!("not exact: {:?}", body.not_exact));
    let tess = |s: &anvil_brep::Solid| {
        let m = anvil_brep::tess::tessellate(s, &Default::default()).expect("tessellate");
        assert!(m.is_watertight());
        anvil_brep::mass::properties(&m).volume
    };
    let (v, want) = (tess(&exact), volume(&body.mesh));
    assert!((v - want).abs() / want < 1e-3, "{v} vs {want}");
    let again =
        import(&anvil_step::export::write(std::slice::from_ref(&exact), "round trip")).expect("import");
    let back = again.bodies[0].exact.as_ref().expect("exact after round trip");
    assert!((tess(back) - v).abs() / v < 1e-6);
    exact
}

/// Countersinks and chamfered holes: cone faces import as exact cones.
#[test]
fn ftc_09_cones_are_exact() {
    let exact = exact_matches_mesh("nist_ftc_09_asme1_rd.stp");
    assert!(exact.faces.iter().any(|f| matches!(f.surface, anvil_brep::Surface::Cone { .. })));
}

/// Cylinder faces bounded by two circles and no seam get a seam in the exact solid.
#[test]
fn ctc_03_seamless_bands_are_exact() {
    exact_matches_mesh("nist_ctc_03_asme1_rc.stp");
}

/// Written to 0.0002 in: some vertices sit up to 3e-5 mm off their edges, more than
/// the kernel allows, so they are moved to where the edges meet and it comes in exact.
#[test]
fn ftc_08_loose_vertices_are_healed() {
    let imp = load("nist_ftc_08_asme1_rc.stp");
    assert!(imp.bodies[0].exact.is_some(), "{:?}", imp.bodies[0].not_exact);
    exact_matches_mesh("nist_ftc_08_asme1_rc.stp");
}
