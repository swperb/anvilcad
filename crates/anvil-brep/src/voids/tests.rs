use super::*;
use crate::boolean::{BoolOp, boolean, contains_point};
use crate::check;
use crate::extrude::extrude;
use crate::geom::Frame;
use crate::profile::{Loop2, Segment, build_regions};
use crate::revolve::revolve;
use crate::shell::shell;
use glam::{DVec2, DVec3};
use std::f64::consts::{PI, TAU};

fn cuboid(lo: DVec3, hi: DVec3) -> Solid {
    let pts =
        [DVec2::new(lo.x, lo.y), DVec2::new(hi.x, lo.y), DVec2::new(hi.x, hi.y), DVec2::new(lo.x, hi.y)];
    let l = Loop2::new((0..4).map(|i| Segment::Line { a: pts[i], b: pts[(i + 1) % 4] }).collect());
    let r = build_regions(&[l]).expect("region");
    extrude(&Frame::world(), &r[0], lo.z, hi.z).expect("box")
}

/// A ball of radius `r` about `c`: a half disc turned about the frame's y axis.
fn ball(c: DVec3, r: f64) -> Solid {
    let half = Loop2::new(vec![
        Segment::Arc { center: DVec2::ZERO, radius: r, start: -0.5 * PI, sweep: PI },
        Segment::Line { a: DVec2::new(0.0, r), b: DVec2::new(0.0, -r) },
    ]);
    let region = build_regions(&[half]).expect("region").remove(0);
    let frame = Frame { origin: c, ..Frame::world() };
    revolve(&frame, &region, DVec2::ZERO, DVec2::Y, TAU).expect("ball")
}

fn one(mut v: Vec<Solid>) -> Solid {
    assert_eq!(v.len(), 1, "expected one body");
    v.remove(0)
}

/// Volume and area integrated exactly, and the volume from a fine tessellation, which
/// must be watertight.
fn measure(s: &Solid) -> (f64, f64, f64) {
    check::validate(s).expect("valid");
    let m = mass::exact_properties(s).expect("exact");
    let mesh = tess::tessellate(s, &tess::TessParams { chord: 1e-3, max_angle: 0.02 }).expect("tess");
    assert!(mesh.is_watertight(), "not watertight");
    (m.volume, m.area, mass::properties(&mesh).volume)
}

fn close(a: f64, b: f64, rel: f64) -> bool {
    (a - b).abs() <= rel * b.abs().max(1.0)
}

#[test]
fn subtracting_a_box_wholly_inside_leaves_a_void() {
    let s = one(boolean(
        &cuboid(DVec3::ZERO, DVec3::splat(4.0)),
        &cuboid(DVec3::ONE, DVec3::splat(2.0)),
        BoolOp::Subtract,
    )
    .expect("subtract"));
    assert_eq!(shell_faces(&s).len(), 2);
    assert_eq!(check::genus(&s).expect("genus"), 0);
    let (v, a, mesh) = measure(&s);
    assert!(close(v, 63.0, 1e-9), "{v}");
    assert!(close(a, 96.0 + 6.0, 1e-9), "{a}");
    assert!(close(mesh, 63.0, 1e-9), "{mesh}");
    let (outer, voids) = outer_and_voids(&s).expect("shells");
    assert_eq!((outer.faces.len(), voids.len()), (6, 1));
    assert!(close(signed_volume(&voids[0]).expect("volume"), -1.0, 1e-9));
    // The void is outside the material; the wall around it is inside.
    assert!(!contains_point(&s, DVec3::splat(1.5)).expect("locate"));
    assert!(contains_point(&s, DVec3::splat(0.5)).expect("locate"));
    assert!(!contains_point(&s, DVec3::splat(5.0)).expect("locate"));
}

#[test]
fn sealed_box_shell() {
    // brep-bench shell/box-hollowed: abc - (a - 2t)(b - 2t)(c - 2t) = 288, area 584.
    let k = cuboid(DVec3::ZERO, DVec3::new(10.0, 8.0, 6.0));
    let s = one(shell(&k, &[], 1.0).expect("shell"));
    assert_eq!(shell_faces(&s).len(), 2);
    let (v, a, mesh) = measure(&s);
    assert!(close(v, 288.0, 1e-9), "{v}");
    assert!(close(a, 584.0, 1e-9), "{a}");
    assert!(close(mesh, 288.0, 1e-9), "{mesh}");
}

#[test]
fn sealed_ball_shell() {
    // brep-bench shell/sphere-hollowed: 4 pi (R^3 - (R - t)^3) / 3 = 244 pi / 3, area
    // 4 pi (R^2 + (R - t)^2) = 164 pi.
    let s = one(shell(&ball(DVec3::ZERO, 5.0), &[], 1.0).expect("shell"));
    assert_eq!(shell_faces(&s).len(), 2);
    let (v, a, mesh) = measure(&s);
    assert!(close(v, 244.0 * PI / 3.0, 1e-8), "{v}");
    assert!(close(a, 164.0 * PI, 1e-8), "{a}");
    assert!(close(mesh, 244.0 * PI / 3.0, 1e-4), "{mesh}");
}

#[test]
fn bodies_nested_in_each_others_hollows() {
    // brep-bench boolean/nested-shells: (B10 - B8) + (B6 - B4) + B2, each inside the
    // hollow of the one before. Three bodies, 4 pi (10^3 - 8^3 + 6^3 - 4^3 + 2^3) / 3.
    let outer = one(boolean(&ball(DVec3::ZERO, 10.0), &ball(DVec3::X, 8.0), BoolOp::Subtract).expect("a"));
    let middle = one(boolean(
        &ball(DVec3::new(1.0, 1.0, 0.0), 6.0),
        &ball(DVec3::new(1.0, 1.0, 1.0), 4.0),
        BoolOp::Subtract,
    )
    .expect("b"));
    let two = boolean(&outer, &middle, BoolOp::Union).expect("union");
    assert_eq!(two.len(), 2);
    // The small ball sits in the middle body's void, which sits in the outer one's.
    let mut all = vec![outer];
    all.extend(boolean(&middle, &ball(DVec3::new(1.0, 1.5, 1.5), 2.0), BoolOp::Union).expect("union"));
    assert_eq!(all.len(), 3);
    let shells: Vec<usize> = all.iter().map(|s| shell_faces(s).len()).collect();
    assert_eq!(shells.iter().sum::<usize>(), 5);
    let (mut v, mut a) = (0.0, 0.0);
    for s in &all {
        let m = measure(s);
        v += m.0;
        a += m.1;
    }
    let want_v = 4.0 * PI * (1000.0 - 512.0 + 216.0 - 64.0 + 8.0) / 3.0;
    let want_a = 4.0 * PI * (100.0 + 64.0 + 36.0 + 16.0 + 4.0);
    assert!(close(v, want_v, 1e-8), "{v} vs {want_v}");
    assert!(close(a, want_a, 1e-8), "{a} vs {want_a}");
}

#[test]
fn booleans_on_a_solid_with_a_void() {
    let hollow = one(shell(&cuboid(DVec3::ZERO, DVec3::new(10.0, 8.0, 6.0)), &[], 1.0).expect("shell"));
    // Drilled through both walls the void opens: one shell, a through hole.
    let pin = {
        let l = Loop2::new(vec![Segment::circle(DVec2::new(5.0, 4.0), 1.0)]);
        let r = build_regions(&[l]).expect("region");
        extrude(&Frame::world(), &r[0], -1.0, 7.0).expect("pin")
    };
    let drilled = one(boolean(&hollow, &pin, BoolOp::Subtract).expect("drill"));
    assert_eq!(shell_faces(&drilled).len(), 1);
    assert_eq!(check::genus(&drilled).expect("genus"), 1);
    let (v, _, _) = measure(&drilled);
    assert!(close(v, 288.0 - 2.0 * PI, 1e-9), "{v}");
    // A block inside the void is a body of its own; filling the void makes it solid.
    let inside =
        boolean(&hollow, &cuboid(DVec3::splat(2.0), DVec3::splat(3.0)), BoolOp::Union).expect("union");
    assert_eq!(inside.len(), 2);
    let full =
        one(boolean(&hollow, &cuboid(DVec3::ONE, DVec3::new(9.0, 7.0, 5.0)), BoolOp::Union).expect("fill"));
    assert_eq!(shell_faces(&full).len(), 1);
    assert!(close(measure(&full).0, 480.0, 1e-9));
    // Cutting it in half through the void leaves two open trays.
    let halves =
        boolean(&hollow, &cuboid(DVec3::new(-1.0, -1.0, 3.0), DVec3::new(11.0, 9.0, 7.0)), BoolOp::Subtract)
            .expect("halve");
    assert_eq!(halves.len(), 1);
    assert!(close(measure(&halves[0]).0, 240.0 - 8.0 * 6.0 * 2.0, 1e-9));
}

#[test]
fn bad_shell_layouts_are_refused() {
    let big = cuboid(DVec3::ZERO, DVec3::splat(4.0));
    let small = cuboid(DVec3::ONE, DVec3::splat(2.0));
    // A void facing the wrong way: two outer shells.
    assert!(check::validate(&join(&[big.clone(), small.clone()])).is_err());
    // A void outside the outer shell.
    let away = cuboid(DVec3::splat(6.0), DVec3::splat(7.0));
    assert!(check::validate(&join(&[big.clone(), flipped(&away)])).is_err());
    // Only voids.
    assert!(check::validate(&join(&[flipped(&big), flipped(&small)])).is_err());
    // A void inside another void.
    let tiny = cuboid(DVec3::splat(1.25), DVec3::splat(1.75));
    assert!(check::validate(&join(&[big.clone(), flipped(&small), flipped(&tiny)])).is_err());
    // The right way round it is valid, and splits back into its shells.
    let good = join(&[big.clone(), flipped(&small)]);
    check::validate(&good).expect("valid");
    let parts = split(&good).expect("split");
    assert_eq!(parts.len(), 2);
    assert_eq!(parts[0].faces.len(), big.faces.len());
    // Exact integration is good to about 1e-9 of the size.
    assert!(close(signed_volume(&parts[0]).expect("volume"), 64.0, 1e-8));
    assert!(close(signed_volume(&parts[1]).expect("volume"), -1.0, 1e-8));
    assert!(close(signed_volume(&flipped(&parts[1])).expect("volume"), 1.0, 1e-8));
}
