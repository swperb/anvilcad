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

/// A cone frustum on the z axis: radius 2 at z = 0 widening to z = 3.
fn frustum(half_angle: f64) -> Solid {
    let mut s = crate::seam::tests::band_solid(half_angle);
    crate::seam::add_seams(&mut s).expect("seam");
    s
}

fn frustum_volume(r1: f64, half_angle: f64, h: f64) -> f64 {
    let r2 = r1 + h * half_angle.tan();
    PI * h / 3.0 * (r1 * r1 + r1 * r2 + r2 * r2)
}

#[test]
fn hole_through_a_cone() {
    let half_angle = 0.3;
    let cone = frustum(half_angle);
    let hole = cylinder(DVec3::ZERO, DVec3::Z, 1.0, -1.0, 4.0);
    let d = boolean(&cone, &hole, BoolOp::Subtract).expect("subtract");
    assert_eq!(d.len(), 1);
    let want = frustum_volume(2.0, half_angle, 3.0) - PI * 3.0;
    assert!(close(volume(&d), want, 2e-3), "{} vs {want}", volume(&d));
}

#[test]
fn cone_cut_across_its_axis() {
    let half_angle = 0.3;
    let cone = frustum(half_angle);
    // A wide cylinder from z = 1.5 up removes the top of the frustum.
    let top = cylinder(DVec3::ZERO, DVec3::Z, 10.0, 1.5, 5.0);
    let d = boolean(&cone, &top, BoolOp::Subtract).expect("subtract");
    let want = frustum_volume(2.0, half_angle, 1.5);
    assert!(close(volume(&d), want, 2e-3), "{} vs {want}", volume(&d));
    let i = boolean(&cone, &top, BoolOp::Intersect).expect("intersect");
    let r = 2.0 + 1.5 * half_angle.tan();
    assert!(close(volume(&i), frustum_volume(r, half_angle, 1.5), 2e-3));
}

#[test]
fn cones_on_one_axis() {
    // A narrower frustum widening the other way crosses the first in one circle.
    let a = frustum(0.3);
    let m = Frame { origin: DVec3::new(0.0, 0.0, 3.0), x: DVec3::X, y: DVec3::NEG_Y, z: DVec3::NEG_Z };
    let b = frustum(0.3).transformed(&m);
    let u = boolean(&a, &b, BoolOp::Union).expect("union");
    assert_eq!(u.len(), 1);
    let both = 2.0 * frustum_volume(2.0, 0.3, 3.0);
    let i = boolean(&a, &b, BoolOp::Intersect).expect("intersect");
    assert!(close(volume(&u) + volume(&i), both, 2e-3));
}

/// Volume shared by a cylinder of radius `big` along x and one of radius `small`
/// along z whose axes meet, by numerical integration across the thin one.
fn cross_volume(big: f64, small: f64) -> f64 {
    let n = 200_000;
    let h = 2.0 * small / n as f64;
    (0..n)
        .map(|i| {
            let y = -small + h * (i as f64 + 0.5);
            4.0 * (small * small - y * y).sqrt() * (big * big - y * y).sqrt() * h
        })
        .sum()
}

#[test]
fn hole_drilled_across_a_shaft() {
    let (big, small, len) = (5.0, 2.0, 20.0);
    let shaft = cylinder(DVec3::new(-10.0, 0.0, 0.0), DVec3::X, big, 0.0, len);
    let drill = cylinder(DVec3::new(1.0, 0.0, -8.0), DVec3::Z, small, 0.0, 16.0);
    let d = boolean(&shaft, &drill, BoolOp::Subtract).expect("subtract");
    assert_eq!(d.len(), 1);
    // Entry and exit edges are traced splines.
    assert!(d[0].edges.iter().filter(|e| matches!(e.curve, Curve::Spline(_))).count() >= 2);
    let want = PI * big * big * len - cross_volume(big, small);
    assert!(close(volume(&d), want, 2e-3), "{} vs {want}", volume(&d));
    let i = boolean(&shaft, &drill, BoolOp::Intersect).expect("intersect");
    assert!(close(volume(&i), cross_volume(big, small), 2e-3), "{}", volume(&i));
}

#[test]
fn boss_partly_into_a_shaft() {
    // A boss whose axis misses the shaft's, so it only bites one side.
    let shaft = cylinder(DVec3::new(-10.0, 0.0, 0.0), DVec3::X, 4.0, 0.0, 20.0);
    let boss = cylinder(DVec3::new(0.0, 3.5, 0.0), DVec3::Z, 1.5, 0.0, 10.0);
    let u = boolean(&shaft, &boss, BoolOp::Union).expect("union");
    let i = boolean(&shaft, &boss, BoolOp::Intersect).expect("intersect");
    let both = PI * 16.0 * 20.0 + PI * 2.25 * 10.0;
    assert_eq!(u.len(), 1);
    assert!(close(volume(&u) + volume(&i), both, 2e-3), "{} + {}", volume(&u), volume(&i));
}

#[test]
fn slanted_cut_across_a_cone() {
    let cone = frustum(0.3);
    // A big box tilted about x, with only its tilted face crossing the cone.
    let b = cuboid(DVec3::new(-20.0, -20.0, 0.0), DVec3::new(20.0, 20.0, 20.0));
    let angle = 0.4f64;
    let m = Frame {
        origin: DVec3::new(0.0, 0.0, 1.5),
        x: DVec3::X,
        y: DVec3::new(0.0, angle.cos(), angle.sin()),
        z: DVec3::new(0.0, -angle.sin(), angle.cos()),
    };
    let b = b.transformed(&m);
    let d = boolean(&cone, &b, BoolOp::Subtract).expect("subtract");
    let i = boolean(&cone, &b, BoolOp::Intersect).expect("intersect");
    let all = frustum_volume(2.0, 0.3, 3.0);
    assert!(close(volume(&d) + volume(&i), all, 2e-3), "{} + {}", volume(&d), volume(&i));
    assert!(volume(&d) > 0.2 * all && volume(&i) > 0.2 * all);
}

fn ring(major: f64, minor: f64) -> Solid {
    let mut s = crate::seam::tests::half_round_ring(major, minor);
    crate::seam::add_seams(&mut s).expect("seam");
    s
}

fn ring_volume(major: f64, minor: f64) -> f64 {
    0.5 * PI * minor * minor * 2.0 * PI * (major + 4.0 * minor / (3.0 * PI))
}

/// The uncut ring tessellated like the results, so the comparisons below see only
/// what the boolean did (on a 1.5 mm tube the default chords lose about 0.3%).
fn ring_mesh_volume() -> f64 {
    let v = volume(&[ring(6.0, 1.5)]);
    assert!(close(v, ring_volume(6.0, 1.5), 5e-3), "{v}");
    v
}

#[test]
fn ring_cut_across_its_axis() {
    let r = ring(6.0, 1.5);
    let top = cuboid(DVec3::new(-20.0, -20.0, 0.0), DVec3::new(20.0, 20.0, 20.0));
    let d = boolean(&r, &top, BoolOp::Subtract).expect("subtract");
    let i = boolean(&r, &top, BoolOp::Intersect).expect("intersect");
    let whole = ring_mesh_volume();
    assert!(close(volume(&d), 0.5 * whole, 2e-3), "{}", volume(&d));
    assert!(close(volume(&i), 0.5 * whole, 2e-3), "{}", volume(&i));
}

#[test]
fn ring_cut_through_its_axis() {
    let r = ring(6.0, 1.5);
    let half = cuboid(DVec3::new(0.0, -20.0, -20.0), DVec3::new(20.0, 20.0, 20.0));
    let d = boolean(&r, &half, BoolOp::Subtract).expect("subtract");
    assert_eq!(d.len(), 1);
    assert!(close(volume(&d), 0.5 * ring_mesh_volume(), 2e-3), "{}", volume(&d));
}

#[test]
fn ring_with_a_coaxial_bore() {
    // A cylinder through the tube meets the torus in two circles.
    let r = ring(6.0, 1.5);
    let bore = cylinder(DVec3::new(0.0, 0.0, -5.0), DVec3::Z, 6.75, 0.0, 10.0);
    let i = boolean(&r, &bore, BoolOp::Intersect).expect("intersect");
    let d = boolean(&r, &bore, BoolOp::Subtract).expect("subtract");
    assert!(close(volume(&i) + volume(&d), ring_mesh_volume(), 2e-3), "{} + {}", volume(&i), volume(&d));
    assert!(volume(&i) > 0.1 && volume(&d) > 0.1);
}

/// A ball of radius `r` about the origin cut by the plane z = `z0`, keeping the part
/// above: a sphere cap face over a flat disc. With `z0` = 0 it is a hemisphere; with
/// `z0` < 0 the cap is more than half the sphere.
fn ball_cut(r: f64, z0: f64) -> Solid {
    use crate::topo::{Coedge, Edge, Face, FaceTag, Loop};
    let rc = (r * r - z0 * z0).sqrt();
    let mut s = Solid::default();
    let circle = Frame { origin: DVec3::new(0.0, 0.0, z0), ..Frame::world() };
    let v = s.add_vertex(DVec3::new(rc, 0.0, z0));
    let e = s.add_edge(Edge {
        curve: Curve::Circle { frame: circle, radius: rc },
        start: v,
        end: v,
        t0: 0.0,
        t1: std::f64::consts::TAU,
    });
    let c = |reversed| Loop { coedges: vec![Coedge { edge: e, reversed }] };
    s.add_face(Face {
        surface: Surface::Sphere { frame: Frame::world(), radius: r },
        reversed: false,
        outer: c(false),
        inners: vec![],
        tag: FaceTag::Imported { index: 0 },
        owner: 0,
    });
    s.add_face(Face {
        surface: Surface::Plane { frame: circle },
        reversed: true,
        outer: c(true),
        inners: vec![],
        tag: FaceTag::Imported { index: 1 },
        owner: 0,
    });
    s
}

/// Volume of a ball of radius `r` above z = `z0`.
fn ball_cut_volume(r: f64, z0: f64) -> f64 {
    let h = r - z0;
    PI * h * h * (3.0 * r - h) / 3.0
}

#[test]
fn sphere_caps_tessellate_closed() {
    for z0 in [0.0, -4.0, 3.0] {
        let b = ball_cut(8.0, z0);
        check::validate(&b).expect("valid");
        // Finely tessellated, the volume converges on the exact one. The default 5 degree
        // steps lose 0.2 to 0.4% on a sphere, which curves both ways (more on a shallow
        // cap, where the dome is a smaller share of the volume).
        let m = tess::tessellate(&b, &TessParams { chord: 1e-4, max_angle: 0.01 }).expect("tess");
        assert!(m.is_watertight());
        let v = mass::properties(&m).volume;
        let want = ball_cut_volume(8.0, z0);
        assert!(close(v, want, 1e-4), "z0 {z0}: {v} vs {want}");
        let coarse = volume(&[b]);
        assert!(close(coarse, want, 5e-3), "z0 {z0}: default tessellation {coarse} vs {want}");
    }
}

#[test]
fn hemisphere_cut_by_a_plane_and_a_coaxial_bore() {
    let (r, h) = (8.0, ball_cut_volume(8.0, 0.0));
    let dome = ball_cut(r, 0.0);
    let whole = volume(std::slice::from_ref(&dome));
    // Across the axis: the top cap goes, a circle where the plane meets the sphere.
    let top = cuboid(DVec3::new(-20.0, -20.0, 4.0), DVec3::new(20.0, 20.0, 20.0));
    let d = boolean(&dome, &top, BoolOp::Subtract).expect("subtract");
    let want = h - ball_cut_volume(r, 4.0);
    assert!(close(volume(&d), want * whole / h, 3e-3), "{} vs {want}", volume(&d));
    // A bore on the axis: circles where the cylinder meets the sphere.
    let bore = cylinder(DVec3::new(0.0, 0.0, -1.0), DVec3::Z, 3.0, 0.0, 20.0);
    let d = boolean(&dome, &bore, BoolOp::Subtract).expect("subtract");
    let i = boolean(&dome, &bore, BoolOp::Intersect).expect("intersect");
    assert_eq!(d.len(), 1);
    assert!(close(volume(&d) + volume(&i), whole, 3e-3), "{} + {}", volume(&d), volume(&i));
}

#[test]
fn hole_drilled_sideways_through_a_dome() {
    // A cylinder across the dome, off its axis: traced curves on the sphere.
    let dome = ball_cut(8.0, 0.0);
    let whole = volume(std::slice::from_ref(&dome));
    let pin = cylinder(DVec3::new(-20.0, 1.0, 3.0), DVec3::X, 1.5, 0.0, 40.0);
    let d = boolean(&dome, &pin, BoolOp::Subtract).expect("subtract");
    let i = boolean(&dome, &pin, BoolOp::Intersect).expect("intersect");
    assert_eq!(d.len(), 1);
    assert!(d[0].edges.iter().any(|e| matches!(e.curve, Curve::Spline(_))));
    assert!(close(volume(&d) + volume(&i), whole, 3e-3), "{} + {}", volume(&d), volume(&i));
    assert!(volume(&i) > 10.0);
}

/// A pointed cone with its apex at the origin, opening up to radius `r` at height `h`,
/// closed by a flat top: one cone face laid out across its axis, over a disc.
fn pointed_cone(r: f64, h: f64) -> Solid {
    use crate::topo::{Coedge, Edge, Face, FaceTag, Loop};
    let mut s = Solid::default();
    let top = Frame { origin: DVec3::new(0.0, 0.0, h), ..Frame::world() };
    let v = s.add_vertex(DVec3::new(r, 0.0, h));
    let e = s.add_edge(Edge {
        curve: Curve::Circle { frame: top, radius: r },
        start: v,
        end: v,
        t0: 0.0,
        t1: std::f64::consts::TAU,
    });
    let l = |reversed| Loop { coedges: vec![Coedge { edge: e, reversed }] };
    s.add_face(Face {
        surface: Surface::Cone { frame: top, radius: r, half_angle: (r / h).atan(), apex: true },
        reversed: false,
        outer: l(true),
        inners: vec![],
        tag: FaceTag::Imported { index: 0 },
        owner: 0,
    });
    s.add_face(Face {
        surface: Surface::Plane { frame: top },
        reversed: false,
        outer: l(false),
        inners: vec![],
        tag: FaceTag::Imported { index: 1 },
        owner: 0,
    });
    s
}

#[test]
fn pointed_cone_tessellates_closed() {
    let c = pointed_cone(5.0, 10.0);
    check::validate(&c).expect("valid");
    let m = tess::tessellate(&c, &TessParams { chord: 1e-4, max_angle: 0.01 }).expect("tess");
    assert!(m.is_watertight());
    let want = PI * 25.0 * 10.0 / 3.0;
    let v = mass::properties(&m).volume;
    assert!(close(v, want, 1e-4), "{v} vs {want}");
    assert!(close(volume(&[c]), want, 3e-3));
}

#[test]
fn drill_point_socket() {
    // The cone's tip sunk 4 deep into a block: the block loses a small cone.
    let c = pointed_cone(5.0, 10.0);
    let block = cuboid(DVec3::new(-10.0, -10.0, -10.0), DVec3::new(10.0, 10.0, 4.0));
    let d = boolean(&block, &c, BoolOp::Subtract).expect("subtract");
    assert_eq!(d.len(), 1);
    let tip = PI * 2.0 * 2.0 * 4.0 / 3.0;
    let want = 20.0 * 20.0 * 14.0 - tip;
    assert!(close(volume(&d), want, 1e-4), "{} vs {want}", volume(&d));
    let i = boolean(&block, &c, BoolOp::Intersect).expect("intersect");
    assert!(close(volume(&i), tip, 3e-3), "{} vs {tip}", volume(&i));
}

#[test]
fn pointed_cone_cut_at_an_angle() {
    let c = pointed_cone(5.0, 10.0);
    let whole = volume(std::slice::from_ref(&c));
    let b = cuboid(DVec3::new(-20.0, -20.0, 0.0), DVec3::new(20.0, 20.0, 20.0));
    let angle = 0.3f64;
    let m = Frame {
        origin: DVec3::new(0.0, 0.0, 6.0),
        x: DVec3::X,
        y: DVec3::new(0.0, angle.cos(), angle.sin()),
        z: DVec3::new(0.0, -angle.sin(), angle.cos()),
    };
    let b = b.transformed(&m);
    let d = boolean(&c, &b, BoolOp::Subtract).expect("subtract");
    let i = boolean(&c, &b, BoolOp::Intersect).expect("intersect");
    assert!(close(volume(&d) + volume(&i), whole, 3e-3), "{} + {}", volume(&d), volume(&i));
    assert!(d[0].edges.iter().any(|e| matches!(e.curve, Curve::Spline(_))));
}

#[test]
fn cut_through_a_drill_point() {
    // A pointed cone (apex at the origin, radius 5 at height 10) and a 3 mm cylinder
    // across it along y whose wall runs through the apex: the cut's edge on the cone
    // passes through the tip, so the cone face is split there.
    let c = pointed_cone(5.0, 10.0);
    let bar = cylinder(DVec3::new(3.0, 0.0, 0.0), DVec3::Y, 3.0, -20.0, 20.0);
    // What the bar takes: at height z the cone's disc (radius z / 2) within the strip
    // |x - 3| <= sqrt(9 - z^2), integrated over 0 <= z <= 3 by Simpson's rule.
    let disc_left_of = |x: f64, r: f64| {
        let u = (x / r).clamp(-1.0, 1.0);
        r * r * (u.asin() + u * (1.0 - u * u).sqrt() + PI / 2.0)
    };
    let slice = |z: f64| {
        let (r, w) = (z / 2.0, (9.0 - z * z).max(0.0).sqrt());
        if r <= 0.0 { 0.0 } else { disc_left_of(3.0 + w, r) - disc_left_of(3.0 - w, r) }
    };
    let n = 20_000;
    let h = 3.0 / n as f64;
    let taken = (0..=n)
        .map(|i| {
            let k = if i == 0 || i == n {
                1.0
            } else if i % 2 == 1 {
                4.0
            } else {
                2.0
            };
            k * slice(i as f64 * h)
        })
        .sum::<f64>()
        * h
        / 3.0;
    let fine = |s: &[Solid]| -> f64 {
        s.iter()
            .map(|x| {
                check::validate(x).expect("valid");
                let m = tess::tessellate(x, &TessParams { chord: 1e-4, max_angle: 0.01 }).expect("tess");
                assert!(m.is_watertight(), "not watertight");
                mass::properties(&m).volume
            })
            .sum()
    };
    let i = boolean(&c, &bar, BoolOp::Intersect).expect("intersect");
    assert!(close(fine(&i), taken, 1e-4), "{} vs {taken}", fine(&i));
    let d = boolean(&c, &bar, BoolOp::Subtract).expect("subtract");
    let whole = PI * 25.0 * 10.0 / 3.0;
    assert!(close(fine(&d), whole - taken, 1e-4), "{} vs {}", fine(&d), whole - taken);
    // The cut's edges on the cone end at its apex.
    assert!(d.iter().flat_map(|s| &s.vertices).any(|v| v.point.length() < 1e-9));
}

/// A 30 x 20 block 5 deep whose top is a freeform bump (a bicubic patch over the same
/// rectangle, flat along its edges), and its volume by integrating the patch's height.
fn bumped_block() -> (Solid, f64) {
    let patch = crate::patch::tests::bump();
    let mut b = cuboid(DVec3::new(0.0, 0.0, -5.0), DVec3::new(30.0, 20.0, 0.0));
    let top = b
        .faces
        .iter()
        .position(|f| matches!(&f.surface, Surface::Plane { frame } if frame.origin.z.abs() < 1e-12 && frame.z.z.abs() > 0.5))
        .expect("top face");
    let Surface::Plane { frame } = &b.faces[top].surface else { unreachable!() };
    // The patch's natural normal is up; the face keeps the plane's outward sense.
    let up = frame.z.z > 0.0;
    b.faces[top].reversed = b.faces[top].reversed == up;
    b.faces[top].surface = Surface::BSpline(std::sync::Arc::new(patch.clone()));
    // Height under the patch: z over the area it covers in x and y, by the midpoint rule.
    let ((s0, s1), (t0, t1)) = patch.domain();
    let n = 300;
    let (hs, ht) = ((s1 - s0) / n as f64, (t1 - t0) / n as f64);
    let mut bump = 0.0;
    for i in 0..n {
        for j in 0..n {
            let st = DVec2::new(s0 + hs * (i as f64 + 0.5), t0 + ht * (j as f64 + 0.5));
            let (a, c) = patch.partials(st);
            bump += patch.eval_st(st).z * (a.x * c.y - a.y * c.x).abs() * hs * ht;
        }
    }
    (b, 30.0 * 20.0 * 5.0 + bump)
}

#[test]
fn freeform_top_tessellates_closed() {
    let (b, want) = bumped_block();
    check::validate(&b).expect("valid");
    let m = tess::tessellate(&b, &TessParams { chord: 1e-4, max_angle: 0.01 }).expect("tess");
    assert!(m.is_watertight());
    let v = mass::properties(&m).volume;
    assert!(close(v, want, 2e-5), "{v} vs {want}");
    assert!(close(volume(&[b]), want, 2e-3));
}

#[test]
fn freeform_block_cut_below_its_patch() {
    // A hole up from the bottom that stops short of the freeform top: the patch is
    // never intersected, but rays through it decide what is inside.
    let (b, _) = bumped_block();
    let whole = volume(std::slice::from_ref(&b));
    let hole = cylinder(DVec3::new(15.0, 10.0, -10.0), DVec3::Z, 3.0, 0.0, 8.0);
    let d = boolean(&b, &hole, BoolOp::Subtract).expect("subtract");
    assert_eq!(d.len(), 1);
    let removed = PI * 9.0 * 3.0;
    assert!(close(volume(&d), whole - removed, 2e-3), "{} vs {}", volume(&d), whole - removed);
}

/// The bump's height above (x, y), from a ray down onto the patch.
fn bump_height(x: f64, y: f64) -> f64 {
    let patch = crate::patch::tests::bump();
    let hits = patch.line_hits(DVec3::new(x, y, 100.0), DVec3::NEG_Z, 0.0, 200.0);
    hits.first().map_or(0.0, |t| 100.0 - t)
}

/// Integral over the disc of radius `r` at (cx, cy) of `f(x, y)`, by the midpoint rule.
fn over_disc(cx: f64, cy: f64, r: f64, f: impl Fn(f64, f64) -> f64) -> f64 {
    let n = 160;
    let h = 2.0 * r / n as f64;
    let mut sum = 0.0;
    for i in 0..n {
        for j in 0..n {
            let (x, y) = (cx - r + h * (i as f64 + 0.5), cy - r + h * (j as f64 + 0.5));
            if (x - cx).powi(2) + (y - cy).powi(2) <= r * r {
                sum += f(x, y) * h * h;
            }
        }
    }
    sum
}

#[test]
fn hole_through_a_freeform_top() {
    let (b, _) = bumped_block();
    let whole = volume(std::slice::from_ref(&b));
    let hole = cylinder(DVec3::new(15.0, 10.0, -10.0), DVec3::Z, 3.0, 0.0, 30.0);
    let d = boolean(&b, &hole, BoolOp::Subtract).expect("subtract");
    let i = boolean(&b, &hole, BoolOp::Intersect).expect("intersect");
    assert_eq!(d.len(), 1);
    // The cut edges on the patch are traced curves.
    assert!(d[0].edges.iter().any(|e| matches!(e.curve, Curve::Spline(_))));
    assert!(close(volume(&d) + volume(&i), whole, 2e-3), "{} + {} vs {whole}", volume(&d), volume(&i));
    let plug = over_disc(15.0, 10.0, 3.0, |x, y| 5.0 + bump_height(x, y));
    assert!(close(volume(&i), plug, 3e-3), "{} vs {plug}", volume(&i));
}

#[test]
fn plane_cut_across_a_freeform_bump() {
    let (b, _) = bumped_block();
    let whole = volume(std::slice::from_ref(&b));
    let top = cuboid(DVec3::new(-10.0, -10.0, 3.0), DVec3::new(40.0, 30.0, 20.0));
    let d = boolean(&b, &top, BoolOp::Subtract).expect("subtract");
    let i = boolean(&b, &top, BoolOp::Intersect).expect("intersect");
    assert!(close(volume(&d) + volume(&i), whole, 2e-3));
    // The crown above z = 3, integrated under the patch.
    let n = 300;
    let (hx, hy) = (30.0 / n as f64, 20.0 / n as f64);
    let mut crown = 0.0;
    for a in 0..n {
        for c in 0..n {
            let z = bump_height(hx * (a as f64 + 0.5), hy * (c as f64 + 0.5));
            crown += (z - 3.0).max(0.0) * hx * hy;
        }
    }
    let fine: f64 = i
        .iter()
        .map(|s| {
            let m = tess::tessellate(s, &TessParams { chord: 1e-4, max_angle: 0.01 }).expect("tess");
            assert!(m.is_watertight());
            mass::properties(&m).volume
        })
        .sum();
    // The crown is a thin dome, so check it finely tessellated (the default 5 degree
    // steps lose about 1% of so thin a shape).
    assert!(close(fine, crown, 3e-4), "{fine} vs {crown}");
}

#[test]
fn two_freeform_blocks_combine() {
    let (a, _) = bumped_block();
    let m = Frame { origin: DVec3::new(25.0, -5.0, 1.0), x: DVec3::Y, y: DVec3::NEG_X, z: DVec3::Z };
    let b = a.transformed(&m);
    let both = volume(std::slice::from_ref(&a)) + volume(std::slice::from_ref(&b));
    let u = boolean(&a, &b, BoolOp::Union).expect("union");
    let i = boolean(&a, &b, BoolOp::Intersect).expect("intersect");
    assert_eq!(u.len(), 1);
    assert!(close(volume(&u) + volume(&i), both, 3e-3), "{} + {} vs {both}", volume(&u), volume(&i));
}

/// The seam test's cylinder band (radius 2, height 3, no seam) with its cylinder given as
/// a B-spline patch that wraps: straight along its height, an exact rational circle
/// around (whose natural normal points inward).
#[test]
fn wrapping_patch_band_gets_a_seam() {
    let mut s = crate::seam::tests::band_solid(0.0);
    let w = std::f64::consts::FRAC_1_SQRT_2;
    let ring = [
        (1.0, 0.0, 1.0),
        (1.0, 1.0, w),
        (0.0, 1.0, 1.0),
        (-1.0, 1.0, w),
        (-1.0, 0.0, 1.0),
        (-1.0, -1.0, w),
        (0.0, -1.0, 1.0),
        (1.0, -1.0, w),
        (1.0, 0.0, 1.0),
    ];
    let mut points = Vec::new();
    let mut weights = Vec::new();
    for z in [0.0, 3.0] {
        for (x, y, wt) in ring {
            points.push(DVec3::new(2.0 * x, 2.0 * y, z));
            weights.push(wt);
        }
    }
    let knots_t = vec![0.0, 0.0, 0.0, 0.25, 0.25, 0.5, 0.5, 0.75, 0.75, 1.0, 1.0, 1.0];
    let patch =
        crate::patch::SplineSurface::new((1, 2), vec![0.0, 0.0, 1.0, 1.0], knots_t, (2, 9), points, weights)
            .expect("patch");
    assert_eq!(patch.wraps, Some(crate::patch::Wrap::T));
    s.faces[2].surface = Surface::BSpline(std::sync::Arc::new(patch));
    s.faces[2].reversed = true;
    check::validate(&s).expect("valid before");
    crate::seam::add_seams(&mut s).expect("seam");
    check::validate(&s).expect("valid after");
    assert!(s.faces[2].inners.is_empty());
    let m = tess::tessellate(&s, &TessParams { chord: 1e-4, max_angle: 0.01 }).expect("tess");
    assert!(m.is_watertight());
    let v = mass::properties(&m).volume;
    let want = PI * 4.0 * 3.0;
    assert!(close(v, want, 1e-4), "{v} vs {want}");
}
