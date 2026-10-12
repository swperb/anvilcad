use super::*;
use crate::extrude::extrude;
use crate::geom::Frame;
use crate::mass;
use crate::profile::{Loop2, Segment, build_regions};
use crate::tess::{self, TessParams};
use proptest::prelude::*;
use std::f64::consts::PI;

fn cuboid(lo: DVec3, hi: DVec3) -> Solid {
    let pts =
        [DVec2::new(lo.x, lo.y), DVec2::new(hi.x, lo.y), DVec2::new(hi.x, hi.y), DVec2::new(lo.x, hi.y)];
    let l = Loop2::new((0..4).map(|i| Segment::Line { a: pts[i], b: pts[(i + 1) % 4] }).collect());
    let r = build_regions(&[l]).expect("region");
    extrude(&Frame::world(), &r[0], lo.z, hi.z).expect("box")
}

/// Cylinder of radius `r` around an axis through `c` along `axis`, from
/// `z0` to `z1` measured along the axis.
fn cylinder(c: DVec3, axis: DVec3, r: f64, z0: f64, z1: f64) -> Solid {
    let hint = if axis.x.abs() < 0.9 { DVec3::X } else { DVec3::Y };
    let f = Frame::new(c, axis, hint).expect("frame");
    let l = Loop2::new(vec![Segment::circle(DVec2::ZERO, r)]);
    let reg = build_regions(&[l]).expect("region");
    extrude(&f, &reg[0], z0, z1).expect("cylinder")
}

fn volume(solids: &[Solid]) -> f64 {
    solids
        .iter()
        .map(|s| {
            check::validate(s).expect("valid");
            let m = tess::tessellate(s, &TessParams::default()).expect("tess");
            assert!(m.is_watertight(), "not watertight");
            mass::properties(&m).volume
        })
        .sum()
}

fn close(a: f64, b: f64, rel: f64) -> bool {
    (a - b).abs() <= rel * b.abs().max(1.0)
}

#[test]
fn overlapping_boxes() {
    let a = cuboid(DVec3::ZERO, DVec3::splat(2.0));
    let b = cuboid(DVec3::splat(1.0), DVec3::splat(3.0));
    let u = boolean(&a, &b, BoolOp::Union).expect("union");
    assert_eq!(u.len(), 1);
    assert!(close(volume(&u), 15.0, 1e-9));
    let d = boolean(&a, &b, BoolOp::Subtract).expect("subtract");
    assert!(close(volume(&d), 7.0, 1e-9));
    let i = boolean(&a, &b, BoolOp::Intersect).expect("intersect");
    assert!(close(volume(&i), 1.0, 1e-9));
}

#[test]
fn disjoint_boxes() {
    let a = cuboid(DVec3::ZERO, DVec3::ONE);
    let b = cuboid(DVec3::splat(5.0), DVec3::splat(6.0));
    assert_eq!(boolean(&a, &b, BoolOp::Union).expect("union").len(), 2);
    assert!(boolean(&a, &b, BoolOp::Intersect).expect("intersect").is_empty());
}

#[test]
fn boss_on_face_merges_into_one_body() {
    // A smaller block standing on top of a plate, sharing the plate's top
    // plane: the typical sketch-on-face boss.
    let plate = cuboid(DVec3::ZERO, DVec3::new(10.0, 10.0, 2.0));
    let boss = cuboid(DVec3::new(3.0, 3.0, 2.0), DVec3::new(7.0, 6.0, 5.0));
    let u = boolean(&plate, &boss, BoolOp::Union).expect("union");
    assert_eq!(u.len(), 1);
    assert!(close(volume(&u), 200.0 + 36.0, 1e-9));
    assert_eq!(check::genus(&u[0]).expect("genus"), 0);
}

#[test]
fn boss_flush_with_an_edge() {
    let plate = cuboid(DVec3::ZERO, DVec3::new(10.0, 10.0, 2.0));
    let boss = cuboid(DVec3::new(0.0, 0.0, 2.0), DVec3::new(10.0, 4.0, 6.0));
    let u = boolean(&plate, &boss, BoolOp::Union).expect("union");
    assert_eq!(u.len(), 1);
    assert!(close(volume(&u), 200.0 + 160.0, 1e-9));
}

#[test]
fn pocket_from_the_top() {
    let block = cuboid(DVec3::ZERO, DVec3::new(10.0, 10.0, 5.0));
    let tool = cuboid(DVec3::new(2.0, 2.0, 3.0), DVec3::new(8.0, 8.0, 5.0));
    let d = boolean(&block, &tool, BoolOp::Subtract).expect("pocket");
    assert_eq!(d.len(), 1);
    assert!(close(volume(&d), 500.0 - 72.0, 1e-9));
}

#[test]
fn through_hole() {
    let plate = cuboid(DVec3::ZERO, DVec3::new(10.0, 10.0, 2.0));
    let pin = cylinder(DVec3::new(5.0, 5.0, 0.0), DVec3::Z, 2.0, -1.0, 3.0);
    let d = boolean(&plate, &pin, BoolOp::Subtract).expect("hole");
    assert_eq!(d.len(), 1);
    assert_eq!(check::genus(&d[0]).expect("genus"), 1);
    assert!(close(volume(&d), 200.0 - PI * 8.0, 0.005));
}

#[test]
fn flush_hole_shares_the_plate_faces() {
    // Tool exactly as thick as the plate: coplanar caps on both sides.
    let plate = cuboid(DVec3::ZERO, DVec3::new(10.0, 10.0, 2.0));
    let pin = cylinder(DVec3::new(5.0, 5.0, 0.0), DVec3::Z, 2.0, 0.0, 2.0);
    let d = boolean(&plate, &pin, BoolOp::Subtract).expect("hole");
    assert_eq!(d.len(), 1);
    assert_eq!(check::genus(&d[0]).expect("genus"), 1);
    assert!(close(volume(&d), 200.0 - PI * 8.0, 0.005));
}

#[test]
fn sideways_hole_through_a_block() {
    let block = cuboid(DVec3::ZERO, DVec3::new(10.0, 6.0, 6.0));
    let pin = cylinder(DVec3::new(0.0, 3.0, 3.0), DVec3::X, 1.5, -1.0, 11.0);
    let d = boolean(&block, &pin, BoolOp::Subtract).expect("hole");
    assert_eq!(d.len(), 1);
    assert_eq!(check::genus(&d[0]).expect("genus"), 1);
    assert!(close(volume(&d), 360.0 - PI * 2.25 * 10.0, 0.005));
}

#[test]
fn round_boss_on_a_plate() {
    let plate = cuboid(DVec3::ZERO, DVec3::new(10.0, 10.0, 2.0));
    let boss = cylinder(DVec3::new(5.0, 5.0, 0.0), DVec3::Z, 3.0, 2.0, 7.0);
    let u = boolean(&plate, &boss, BoolOp::Union).expect("union");
    assert_eq!(u.len(), 1);
    assert!(close(volume(&u), 200.0 + PI * 45.0, 0.005));
}

#[test]
fn cylinder_poking_out_of_a_box_side() {
    let block = cuboid(DVec3::ZERO, DVec3::new(10.0, 10.0, 10.0));
    let rod = cylinder(DVec3::new(5.0, 5.0, 5.0), DVec3::X, 2.0, 0.0, 15.0);
    let u = boolean(&block, &rod, BoolOp::Union).expect("union");
    assert_eq!(u.len(), 1);
    assert!(close(volume(&u), 1000.0 + PI * 4.0 * 10.0, 0.005));
    let i = boolean(&block, &rod, BoolOp::Intersect).expect("intersect");
    assert!(close(volume(&i), PI * 4.0 * 5.0, 0.005));
}

#[test]
fn slanted_cut_through_a_cylinder() {
    // A box rotated 45 degrees about Y slices a vertical cylinder: the
    // intersection curves are ellipses.
    let rod = cylinder(DVec3::ZERO, DVec3::Z, 2.0, 0.0, 10.0);
    let f = Frame::new(DVec3::new(0.0, 0.0, 5.0), DVec3::new(1.0, 0.0, 1.0), DVec3::Y).expect("frame");
    let pts =
        [DVec2::new(-10.0, -10.0), DVec2::new(10.0, -10.0), DVec2::new(10.0, 10.0), DVec2::new(-10.0, 10.0)];
    let l = Loop2::new((0..4).map(|i| Segment::Line { a: pts[i], b: pts[(i + 1) % 4] }).collect());
    let r = build_regions(&[l]).expect("region");
    let slab = extrude(&f, &r[0], 0.0, 20.0).expect("slab");
    let d = boolean(&rod, &slab, BoolOp::Subtract).expect("cut");
    assert_eq!(d.len(), 1);
    // The plane z = 5 - x splits the rod; the kept half has volume
    // pi r^2 * 5 exactly (the slant cancels out by symmetry).
    assert!(close(volume(&d), PI * 4.0 * 5.0, 0.01));
}

#[test]
fn contained_tool() {
    let big = cuboid(DVec3::ZERO, DVec3::splat(10.0));
    let small = cuboid(DVec3::splat(2.0), DVec3::splat(4.0));
    let u = boolean(&big, &small, BoolOp::Union).expect("union");
    assert!(close(volume(&u), 1000.0, 1e-9));
    assert!(matches!(boolean(&big, &small, BoolOp::Subtract), Err(KernelError::Unsupported(_))));
    let i = boolean(&big, &small, BoolOp::Intersect).expect("intersect");
    assert!(close(volume(&i), 8.0, 1e-9));
}

#[test]
fn identical_boxes() {
    let a = cuboid(DVec3::ZERO, DVec3::splat(2.0));
    let u = boolean(&a, &a.clone(), BoolOp::Union).expect("union");
    assert!(close(volume(&u), 8.0, 1e-9));
    let d = boolean(&a, &a.clone(), BoolOp::Subtract).expect("subtract");
    assert!(d.is_empty());
}

fn overlap(a0: f64, a1: f64, b0: f64, b1: f64) -> f64 {
    (a1.min(b1) - a0.max(b0)).max(0.0)
}

prop_compose! {
    fn grid_box()(x in 0i32..6, y in 0i32..6, z in 0i32..6, w in 1i32..5, d in 1i32..5, h in 1i32..5) -> (DVec3, DVec3) {
        let lo = DVec3::new(x as f64, y as f64, z as f64);
        (lo, lo + DVec3::new(w as f64, d as f64, h as f64))
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    /// Boxes on an integer grid share faces, edges and corners all the time,
    /// which is exactly where boolean code tends to break.
    #[test]
    fn grid_boxes_match_inclusion_exclusion((alo, ahi) in grid_box(), (blo, bhi) in grid_box()) {
        let a = cuboid(alo, ahi);
        let b = cuboid(blo, bhi);
        let va = (ahi - alo).element_product();
        let vb = (bhi - blo).element_product();
        let vi = overlap(alo.x, ahi.x, blo.x, bhi.x)
            * overlap(alo.y, ahi.y, blo.y, bhi.y)
            * overlap(alo.z, ahi.z, blo.z, bhi.z);
        let inside = alo.cmple(blo).all() && bhi.cmple(ahi).all() && (alo.cmplt(blo).all() && bhi.cmplt(ahi).all());

        // Results that would be non-manifold are refused, never wrong.
        let zero_thickness = |e: &KernelError| matches!(e, KernelError::Unsupported(m) if m.contains("zero-thickness"));
        match boolean(&a, &b, BoolOp::Union) {
            Ok(u) => prop_assert!(close(volume(&u), va + vb - vi, 1e-9), "union {}", volume(&u)),
            Err(e) => prop_assert!(zero_thickness(&e), "union failed: {e}"),
        }
        match boolean(&a, &b, BoolOp::Intersect) {
            Ok(i) => prop_assert!(close(volume(&i), vi, 1e-9), "intersect {}", volume(&i)),
            Err(e) => prop_assert!(zero_thickness(&e), "intersect failed: {e}"),
        }
        match boolean(&a, &b, BoolOp::Subtract) {
            Ok(d) => prop_assert!(close(volume(&d), va - vi, 1e-9), "subtract {}", volume(&d)),
            Err(KernelError::Unsupported(m)) if m.contains("pocket") => prop_assert!(inside),
            Err(e) => prop_assert!(zero_thickness(&e), "subtract failed: {e}"),
        }
    }
}

#[test]
fn timing() {
    let plate = cuboid(DVec3::ZERO, DVec3::new(100.0, 60.0, 10.0));
    let mut body = plate;
    let t = std::time::Instant::now();
    for i in 0..6 {
        let pin = cylinder(DVec3::new(10.0 + 15.0 * i as f64, 30.0, 0.0), DVec3::Z, 4.0, -1.0, 11.0);
        body = boolean(&body, &pin, BoolOp::Subtract).expect("hole").remove(0);
    }
    let ms = t.elapsed().as_secs_f64() * 1000.0;
    eprintln!("six holes: {ms:.1} ms, {} faces", body.faces.len());
    assert_eq!(check::genus(&body).expect("genus"), 6);
}
