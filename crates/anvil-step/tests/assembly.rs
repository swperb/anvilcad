//! Assembly placement: hand-written assembly structure around a 10 x 20 x 5 mm block
//! that AnvilCAD exported (see tests/data/NOTICE.md).

#![allow(clippy::expect_used, clippy::panic)]

use anvil_step::{Body, Import, import};
use glam::DVec3;

fn load(name: &str) -> Import {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data").join(name);
    import(&std::fs::read_to_string(path).expect("read")).expect("import")
}

fn mesh_bounds(b: &Body) -> (DVec3, DVec3) {
    let mut lo = DVec3::splat(f64::INFINITY);
    let mut hi = DVec3::splat(f64::NEG_INFINITY);
    for v in &b.mesh.vertices {
        lo = lo.min(DVec3::from_array(*v));
        hi = hi.max(DVec3::from_array(*v));
    }
    (lo, hi)
}

/// Checks a body's mesh and exact solid both span `lo..hi`.
fn assert_spans(b: &Body, lo: [f64; 3], hi: [f64; 3]) {
    let (lo, hi) = (DVec3::from_array(lo), DVec3::from_array(hi));
    let (mlo, mhi) = mesh_bounds(b);
    assert!((mlo - lo).length() < 1e-9 && (mhi - hi).length() < 1e-9, "{}: mesh {mlo}..{mhi}", b.name);
    let exact = b.exact.as_ref().unwrap_or_else(|| panic!("{} not exact: {:?}", b.name, b.not_exact));
    let (elo, ehi) = exact.bounds().expect("bounds");
    assert!((elo - lo).length() < 1e-9 && (ehi - hi).length() < 1e-9, "{}: exact {elo}..{ehi}", b.name);
    let v = anvil_brep::mass::properties(
        &anvil_brep::tess::tessellate(exact, &anvil_brep::tess::TessParams::default()).expect("tess"),
    )
    .volume;
    assert!((v - 1000.0).abs() < 1e-6, "{}: volume {v}", b.name);
}

fn near(a: DVec3, b: [f64; 3]) -> bool {
    (a - DVec3::from_array(b)).length() < 1e-12
}

/// TOP holds SUB-1, turned 90 degrees about z and moved 100 along x; SUB holds the
/// block twice, once moved 50 along z and once turned so its z runs along SUB's x
/// (that one written with the relationship's roles swapped).
#[test]
fn two_level_assembly_places_every_occurrence() {
    let imp = load("two_level_assembly.stp");
    assert!(!imp.warnings.iter().any(|w| w.contains("own coordinates")), "{:?}", imp.warnings);
    assert_eq!(imp.bodies.len(), 2);

    let names: Vec<&str> = imp.occurrences.iter().map(|o| o.name.as_str()).collect();
    assert_eq!(names, ["TOP", "SUB-1", "block (1)", "block (2)"]);
    let parents: Vec<Option<usize>> = imp.occurrences.iter().map(|o| o.parent).collect();
    assert_eq!(parents, [None, Some(0), Some(1), Some(1)]);
    let products: Vec<&str> = imp.occurrences.iter().map(|o| o.product.as_str()).collect();
    assert_eq!(products, ["TOP", "SUB", "block", "block"]);
    let bodies: Vec<&[usize]> = imp.occurrences.iter().map(|o| o.bodies.as_slice()).collect();
    assert_eq!(bodies, [&[][..], &[], &[0], &[1]]);

    let sub = &imp.occurrences[1].placement;
    assert!(
        near(sub.origin, [100.0, 0.0, 0.0]) && near(sub.x, [0.0, 1.0, 0.0]) && near(sub.z, [0.0, 0.0, 1.0])
    );
    let b = &imp.occurrences[3].placement;
    assert!(near(b.origin, [100.0, 30.0, 0.0]), "{}", b.origin);
    assert!(near(b.x, [-1.0, 0.0, 0.0]) && near(b.y, [0.0, 0.0, 1.0]) && near(b.z, [0.0, 1.0, 0.0]));

    assert_eq!(imp.bodies[0].name, "block (1)");
    assert_spans(&imp.bodies[0], [80.0, 0.0, 50.0], [100.0, 10.0, 55.0]);
    assert_eq!(imp.bodies[1].name, "block (2)");
    assert_spans(&imp.bodies[1], [90.0, 30.0, 0.0], [100.0, 35.0, 20.0]);
}

/// The block used twice through MAPPED_ITEMs, once moved to x = -50 and once flipped
/// upside down at x = 50; the NAUOs carry the instance names.
#[test]
fn mapped_items_place_occurrences() {
    let imp = load("mapped_item_assembly.stp");
    assert!(!imp.warnings.iter().any(|w| w.contains("own coordinates")), "{:?}", imp.warnings);
    let names: Vec<&str> = imp.occurrences.iter().map(|o| o.name.as_str()).collect();
    assert_eq!(names, ["TOP", "left", "right"]);
    assert_eq!(imp.bodies.len(), 2);
    assert_eq!(imp.bodies[0].name, "left");
    assert_spans(&imp.bodies[0], [-50.0, 0.0, 0.0], [-40.0, 20.0, 5.0]);
    assert_eq!(imp.bodies[1].name, "right");
    assert_spans(&imp.bodies[1], [50.0, -20.0, -5.0], [60.0, 0.0, 0.0]);
}

/// A placement that cannot be read leaves that occurrence in its part's coordinates,
/// with a warning, and still places the rest.
#[test]
fn unreadable_placement_falls_back_with_a_warning() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/two_level_assembly.stp");
    let text = std::fs::read_to_string(path).expect("read");
    let text = text.replace(
        "ITEM_DEFINED_TRANSFORMATION('','',#18,#263)",
        "ITEM_DEFINED_TRANSFORMATION('','',#18,#999)",
    );
    let imp = import(&text).expect("import");
    assert!(
        imp.warnings.iter().any(|w| w.starts_with("occurrence 'block (1)'") && w.contains("own coordinates")),
        "{:?}",
        imp.warnings
    );
    // block (1) stays where SUB puts its own origin; block (2) is unaffected.
    assert_spans(&imp.bodies[0], [80.0, 0.0, 0.0], [100.0, 10.0, 5.0]);
    assert_spans(&imp.bodies[1], [90.0, 30.0, 0.0], [100.0, 35.0, 20.0]);
}

/// Single parts are not assemblies: no occurrences, bodies as before.
#[test]
fn nist_parts_have_no_occurrences() {
    for name in ["nist_ctc_03_asme1_rc.stp", "nist_ftc_08_asme1_rc.stp"] {
        let imp = load(name);
        assert!(imp.occurrences.is_empty(), "{name}");
        assert!(!imp.warnings.iter().any(|w| w.contains("assembly")), "{name}: {:?}", imp.warnings);
    }
}
