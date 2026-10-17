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
    // Its fillets are tori, so the body is exact, and its mesh is the kernel's at the
    // importer's (coarser) tolerances. Finely tessellated, the exact solid converges on
    // 5129.0 mm^3; the coarse mesh runs a little low on the 1.5 mm fillets.
    let v = volume(&solid.mesh);
    assert!((v - 5129.0).abs() / 5129.0 < 3e-3, "{v}");
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

/// Twenty freeform (B-spline) faces: the part imports exact, and holes drilled into it
/// (one square to its largest freeform face) cut those faces with traced curves; what a
/// hole removes and what is left add back up to the whole. The file's arcs on that face
/// stray up to 6e-3 mm off its surface, so the cut must meet them tolerantly.
#[test]
fn ftc_07_freeform_faces_take_a_hole() {
    use anvil_brep::boolean::{BoolOp, boolean};
    use anvil_brep::profile::{Loop2, Segment, build_regions};
    let solid = exact_matches_mesh("nist_ftc_07_asme1_rd.stp");
    assert!(
        solid.faces.iter().filter(|f| matches!(f.surface, anvil_brep::Surface::BSpline(_))).count() >= 20
    );
    let (lo, hi) = solid.bounds().expect("bounds");
    let c = (lo + hi) * 0.5;
    let reach = (hi - lo).length();
    let vol = |s: &[anvil_brep::Solid]| -> f64 {
        s.iter()
            .map(|x| {
                anvil_brep::check::validate(x).expect("valid");
                let m = anvil_brep::tess::tessellate(x, &Default::default()).expect("tessellate");
                assert!(m.is_watertight());
                anvil_brep::mass::properties(&m).volume
            })
            .sum()
    };
    let whole = vol(std::slice::from_ref(&solid));
    // Aim one hole square at the largest freeform face, through a point on it.
    let mesh = anvil_brep::tess::tessellate(&solid, &Default::default()).expect("tessellate");
    let mut area = vec![0.0f64; solid.faces.len()];
    let mut biggest = vec![(0.0f64, DVec3::ZERO); solid.faces.len()];
    for (t, f) in mesh.triangles.iter().zip(&mesh.triangle_face) {
        let [a, b, d] = t.map(|i| mesh.positions[i as usize]);
        let ar = 0.5 * (b - a).cross(d - a).length();
        area[*f as usize] += ar;
        if ar > biggest[*f as usize].0 {
            biggest[*f as usize] = (ar, (a + b + d) / 3.0);
        }
    }
    let target = (0..solid.faces.len())
        .filter(|&i| matches!(solid.faces[i].surface, anvil_brep::Surface::BSpline(_)))
        .max_by(|&i, &j| area[i].total_cmp(&area[j]))
        .expect("a freeform face");
    let face = &solid.faces[target];
    let spot = biggest[target].1;
    let aim = face.normal_at(spot);
    let aim_hint = if aim.x.abs() < 0.9 { DVec3::X } else { DVec3::Y };
    let r_aim = 0.25 * area[target].sqrt();
    let mut through_freeform = 0;
    for (axis, hint, at, r) in [
        (aim, aim_hint, spot, r_aim),
        (DVec3::X, DVec3::Y, c, 0.15 * (hi - lo).min_element()),
        (DVec3::Z, DVec3::X, c, 0.15 * (hi - lo).min_element()),
    ] {
        let frame = anvil_brep::Frame::new(at - axis * reach, axis, hint).expect("frame");
        let disc = build_regions(&[Loop2::new(vec![Segment::circle(glam::DVec2::ZERO, r)])]).expect("region");
        let drill = anvil_brep::extrude::extrude(&frame, &disc[0], 0.0, 2.0 * reach).expect("drill");
        let left = boolean(&solid, &drill, BoolOp::Subtract).expect("subtract");
        let cut = boolean(&solid, &drill, BoolOp::Intersect).expect("intersect");
        let (l, k) = (vol(&left), vol(&cut));
        // The hole bites (at least a tenth of its radius deep), and what it removes and
        // what is left add back up to the whole, to a thousandth of what it removes.
        assert!(k > 0.1 * std::f64::consts::PI * r.powi(3), "{axis:?}: {k}");
        assert!((l + k - whole).abs() < 1e-3 * k, "{axis:?}: {l} + {k} vs {whole}");
        // A hole that crossed a freeform face leaves that face bounded by a traced edge on
        // the hole's wall.
        let wall = anvil_brep::Surface::Cylinder { frame, radius: r };
        let crossed = |s: &anvil_brep::Solid| {
            s.faces.iter().filter(|f| matches!(f.surface, anvil_brep::Surface::BSpline(_))).any(|f| {
                f.loops().flat_map(|l| &l.coedges).any(|c| {
                    let e = &s.edges[c.edge.index()];
                    matches!(e.curve, anvil_brep::Curve::Spline(_))
                        && wall.distance(e.curve.eval(0.5 * (e.t0 + e.t1))) < 1e-6
                })
            })
        };
        if left.iter().any(crossed) {
            through_freeform += 1;
        }
    }
    assert!(through_freeform >= 1, "no hole crossed the freeform faces");
}

/// Countersinks (cones 59 degrees from their axes): large holes drilled through the part
/// pass beside them, meeting each double cone on both nappes; only the face's own nappe
/// is kept. What a hole removes and what is left add back up to the whole.
#[test]
fn ctc_05_holes_beside_countersinks() {
    use anvil_brep::boolean::{BoolOp, boolean};
    use anvil_brep::profile::{Loop2, Segment, build_regions};
    let imp = load("nist_ctc_05_asme1_rd.stp");
    let solid = imp.bodies.iter().find_map(|b| b.exact.clone()).expect("exact");
    let (lo, hi) = solid.bounds().expect("bounds");
    let c = (lo + hi) * 0.5;
    let reach = (hi - lo).length();
    let r = 0.15 * (hi - lo).min_element();
    let vol = |s: &[anvil_brep::Solid]| -> f64 {
        s.iter()
            .map(|x| {
                anvil_brep::check::validate(x).expect("valid");
                let m = anvil_brep::tess::tessellate(x, &Default::default()).expect("tessellate");
                assert!(m.is_watertight());
                anvil_brep::mass::properties(&m).volume
            })
            .sum()
    };
    let whole = vol(std::slice::from_ref(&solid));
    let mut through_cones = 0;
    for (axis, hint) in [(DVec3::X, DVec3::Y), (DVec3::Y, DVec3::Z), (DVec3::Z, DVec3::X)] {
        let frame = anvil_brep::Frame::new(c - axis * reach, axis, hint).expect("frame");
        let disc = build_regions(&[Loop2::new(vec![Segment::circle(glam::DVec2::ZERO, r)])]).expect("region");
        let drill = anvil_brep::extrude::extrude(&frame, &disc[0], 0.0, 2.0 * reach).expect("drill");
        let left = boolean(&solid, &drill, BoolOp::Subtract).expect("subtract");
        let cut = boolean(&solid, &drill, BoolOp::Intersect).expect("intersect");
        let (l, k) = (vol(&left), vol(&cut));
        assert!(k > 0.01 * whole, "{axis:?}: {k}");
        assert!((l + k - whole).abs() < 1e-3 * k, "{axis:?}: {l} + {k} vs {whole}");
        // A cone face of what is left, bounded by a traced edge on the hole's wall.
        let wall = anvil_brep::Surface::Cylinder { frame, radius: r };
        if left.iter().any(|s| {
            s.faces.iter().filter(|f| matches!(f.surface, anvil_brep::Surface::Cone { .. })).any(|f| {
                f.loops().flat_map(|l| &l.coedges).any(|e| {
                    let e = &s.edges[e.edge.index()];
                    matches!(e.curve, anvil_brep::Curve::Spline(_))
                        && wall.distance(e.curve.eval(0.5 * (e.t0 + e.t1))) < 1e-6
                })
            })
        }) {
            through_cones += 1;
        }
    }
    assert!(through_cones >= 1, "no hole cut a cone");
}
