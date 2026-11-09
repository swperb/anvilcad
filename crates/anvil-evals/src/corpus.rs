//! The hand-built corpus: the configurations that break B-rep kernels in practice.
//! Coincident and tangent faces, touching edges and vertices, near-tolerance gaps and
//! overlaps, very large and very small parts, and long chains of operations. Volumes
//! are given in closed form wherever one exists.

use crate::csg::{Tree, cone, cuboid, cylinder, sphere, torus};
use crate::{Case, Expect, Step};
use glam::DVec3;
use std::f64::consts::PI;

fn v(x: f64, y: f64, z: f64) -> DVec3 {
    DVec3::new(x, y, z)
}

/// Area of a circle of radius `r` on the far side of a line `d` from its centre.
fn segment_area(r: f64, d: f64) -> f64 {
    r * r * (d / r).acos() - d * (r * r - d * d).sqrt()
}

/// Area of the lens two circles of radius `r` with centres `d` apart share.
fn lens_area(r: f64, d: f64) -> f64 {
    2.0 * r * r * (d / (2.0 * r)).acos() - 0.5 * d * (4.0 * r * r - d * d).sqrt()
}

/// The overlapping-boxes case scaled by `s` and moved by `o`.
fn boxes(s: f64, o: DVec3) -> (Tree, Tree) {
    (cuboid(o, o + DVec3::splat(2.0 * s)), cuboid(o + DVec3::splat(s), o + DVec3::splat(3.0 * s)))
}

pub fn cases() -> Vec<Case> {
    let mut c = Vec::new();
    let plate = || cuboid(v(0.0, 0.0, 0.0), v(10.0, 10.0, 5.0));

    // --- Boxes ---------------------------------------------------------------------
    let (a, b) = boxes(1.0, DVec3::ZERO);
    c.push(Case::new("box/overlap-union", "box", a.clone().union(b.clone())).volume(15.0).bodies(1));
    c.push(Case::new("box/overlap-subtract", "box", a.clone().minus(b.clone())).volume(7.0).bodies(1));
    c.push(Case::new("box/overlap-intersect", "box", a.clone().and(b.clone())).volume(1.0).bodies(1));
    let big = cuboid(DVec3::ZERO, DVec3::splat(4.0));
    let small = cuboid(DVec3::ONE, DVec3::splat(2.0));
    c.push(Case::new("box/nested-union", "box", big.clone().union(small.clone())).volume(64.0).bodies(1));
    c.push(Case::new("box/nested-intersect", "box", big.clone().and(small.clone())).volume(1.0).bodies(1));
    c.push(
        Case::new("box/nested-subtract-void", "box", big.minus(small))
            .volume(63.0)
            .graceful()
            .note("a sealed hollow needs a second shell, which the kernel does not model yet"),
    );
    let unit = cuboid(DVec3::ZERO, DVec3::ONE);
    c.push(Case::new("box/identical-union", "box", unit.clone().union(unit.clone())).volume(1.0).bodies(1));
    c.push(Case::new("box/identical-intersect", "box", unit.clone().and(unit.clone())).volume(1.0).bodies(1));
    c.push(
        Case::new("box/identical-subtract", "box", unit.clone().minus(unit.clone()))
            .volume(0.0)
            .bodies(0)
            .note("subtracting a solid from itself leaves nothing"),
    );
    c.push(
        Case::new(
            "box/boss-on-face",
            "box",
            cuboid(v(0.0, 0.0, 0.0), v(10.0, 10.0, 2.0)).union(cuboid(v(3.0, 3.0, 2.0), v(7.0, 6.0, 5.0))),
        )
        .volume(236.0)
        .bodies(1)
        .genus(0),
    );
    c.push(
        Case::new("box/shared-face", "box", unit.clone().union(cuboid(v(1.0, 0.0, 0.0), v(2.0, 1.0, 1.0))))
            .volume(2.0)
            .bodies(1),
    );
    c.push(
        Case::new(
            "box/shared-partial-face",
            "box",
            unit.clone().union(cuboid(v(1.0, 0.25, 0.25), v(2.0, 0.75, 0.75))),
        )
        .volume(1.25)
        .bodies(1),
    );
    c.push(
        Case::new("box/touching-edge", "box", unit.clone().union(cuboid(v(1.0, 1.0, 0.0), v(2.0, 2.0, 1.0))))
            .volume(2.0)
            .graceful()
            .note("bodies that would share only an edge are refused: the kernel's solids are manifold"),
    );
    c.push(
        Case::new(
            "box/touching-vertex",
            "box",
            unit.clone().union(cuboid(v(1.0, 1.0, 1.0), v(2.0, 2.0, 2.0))),
        )
        .volume(2.0),
    );
    c.push(
        Case::new(
            "box/through-slot",
            "box",
            cuboid(v(0.0, 0.0, 0.0), v(10.0, 4.0, 2.0)).minus(cuboid(v(4.0, -1.0, 1.0), v(6.0, 5.0, 3.0))),
        )
        .volume(72.0)
        .bodies(1),
    );
    c.push(
        Case::new(
            "box/cut-in-two",
            "box",
            cuboid(v(0.0, 0.0, 0.0), v(10.0, 2.0, 2.0)).minus(cuboid(v(4.0, -1.0, -1.0), v(6.0, 3.0, 3.0))),
        )
        .volume(32.0)
        .bodies(2),
    );
    c.push(
        Case::new("box/flush-side-notch", "box", plate().minus(cuboid(v(8.0, 2.0, -1.0), v(10.0, 4.0, 6.0))))
            .volume(480.0)
            .bodies(1)
            .note("the tool's side lies in the plate's side: coplanar faces of opposite sense"),
    );
    c.push(
        Case::new("box/flush-top-pocket", "box", plate().minus(cuboid(v(2.0, 2.0, 3.0), v(8.0, 8.0, 5.0))))
            .volume(500.0 - 72.0)
            .bodies(1),
    );
    let rotated = Tree::Prim(crate::csg::Prim::Cuboid {
        frame: crate::csg::frame(DVec3::ZERO, DVec3::Z, v(1.0, 1.0, 0.0)),
        half: v(1.0, 1.0, 3.0),
    });
    let cube4 = cuboid(DVec3::splat(-2.0), DVec3::splat(2.0));
    c.push(
        Case::new("box/rotated-45-intersect", "box", cube4.clone().and(rotated.clone()))
            .volume(16.0)
            .bodies(1),
    );
    c.push(Case::new("box/rotated-45-subtract", "box", cube4.minus(rotated)).volume(48.0).bodies(1).genus(1));
    c.push(
        Case::new(
            "box/thin-wall",
            "box",
            cuboid(DVec3::ZERO, DVec3::splat(10.0)).minus(cuboid(v(0.01, 0.01, 0.01), v(9.99, 9.99, 11.0))),
        )
        .volume(1000.0 - 9.98 * 9.98 * 9.99)
        .bodies(1),
    );
    c.push(
        Case::new("box/foil", "box", plate().minus(cuboid(v(-1.0, -1.0, 1e-3), v(11.0, 11.0, 6.0))))
            .volume(0.1)
            .bodies(1)
            .note("a 1 um sheet left by a cut"),
    );

    // Near-tolerance gaps and overlaps between a boss and a plate.
    let base = || cuboid(v(0.0, 0.0, 0.0), v(10.0, 10.0, 2.0));
    for (tag, d) in [("1e-3", 1e-3), ("1e-4", 1e-4), ("1e-5", 1e-5)] {
        // A gap at the kernel's own tolerance (1e-5 mm) may close; a wider one must not.
        let gap = Case::new(
            format!("near/boss-gap-{tag}"),
            "near",
            base().union(cuboid(v(3.0, 3.0, 2.0 + d), v(7.0, 7.0, 5.0))),
        )
        .volume(200.0 + 16.0 * (3.0 - d));
        c.push(if d > 1e-5 { gap.bodies(2) } else { gap });
        c.push(
            Case::new(
                format!("near/boss-overlap-{tag}"),
                "near",
                base().union(cuboid(v(3.0, 3.0, 2.0 - d), v(7.0, 7.0, 5.0))),
            )
            .volume(248.0)
            .bodies(1),
        );
        c.push(
            Case::new(
                format!("near/side-offset-{tag}"),
                "near",
                base().union(cuboid(v(3.0, 3.0 + d, 2.0), v(10.0 + d, 7.0, 5.0))),
            )
            .volume(200.0 + 3.0 * (4.0 - d) * (7.0 + d))
            .bodies(1)
            .note("the boss overhangs the plate's side by a hair"),
        );
    }
    c.push(
        Case::new(
            "near/boss-gap-1e-7",
            "near",
            base().union(cuboid(v(3.0, 3.0, 2.0 + 1e-7), v(7.0, 7.0, 5.0))),
        )
        .volume(248.0)
        .note("a gap below the kernel's tolerance: merged or kept apart, either is right"),
    );

    // Scale and position.
    for (tag, s, o) in [
        ("tiny-0.01", 0.01, DVec3::ZERO),
        ("huge-1000", 1000.0, DVec3::ZERO),
        ("far-1e4", 1.0, DVec3::splat(1e4)),
        ("far-1e5", 1.0, DVec3::splat(1e5)),
    ] {
        let (a, b) = boxes(s, o);
        let s3 = s * s * s;
        c.push(
            Case::new(format!("scale/{tag}-union"), "scale", a.clone().union(b.clone()))
                .volume(15.0 * s3)
                .bodies(1),
        );
        c.push(Case::new(format!("scale/{tag}-subtract"), "scale", a.minus(b)).volume(7.0 * s3).bodies(1));
    }
    let far = DVec3::splat(1e4);
    c.push(
        Case::new(
            "scale/far-1e4-hole",
            "scale",
            cuboid(far, far + v(10.0, 10.0, 5.0)).minus(cylinder(
                far + v(5.0, 5.0, -1.0),
                DVec3::Z,
                2.0,
                7.0,
            )),
        )
        .volume(500.0 - 20.0 * PI)
        .bodies(1)
        .genus(1),
    );

    // --- Cylinders -------------------------------------------------------------------
    let hole = |x: f64, y: f64, r: f64, z0: f64, len: f64| cylinder(v(x, y, z0), DVec3::Z, r, len);
    c.push(
        Case::new("cyl/through-hole", "cyl", plate().minus(hole(5.0, 5.0, 2.0, -1.0, 7.0)))
            .volume(500.0 - 20.0 * PI)
            .bodies(1)
            .genus(1),
    );
    c.push(
        Case::new("cyl/blind-hole", "cyl", plate().minus(hole(5.0, 5.0, 2.0, 2.0, 4.0)))
            .volume(500.0 - 12.0 * PI)
            .bodies(1)
            .genus(0),
    );
    c.push(
        Case::new("cyl/hole-flush-both-ends", "cyl", plate().minus(hole(5.0, 5.0, 2.0, 0.0, 5.0)))
            .volume(500.0 - 20.0 * PI)
            .bodies(1)
            .genus(1)
            .note("the tool's caps lie in the plate's top and bottom"),
    );
    c.push(
        Case::new("cyl/hole-tangent-inside", "cyl", plate().minus(hole(2.0, 5.0, 2.0, -1.0, 7.0)))
            .volume(500.0 - 20.0 * PI)
            .graceful()
            .note("a wall of zero thickness along a line is refused: the kernel's solids are manifold"),
    );
    c.push(
        Case::new("cyl/hole-breaks-out", "cyl", plate().minus(hole(1.0, 5.0, 2.0, -1.0, 7.0)))
            .volume(500.0 - 5.0 * (4.0 * PI - segment_area(2.0, 1.0)))
            .bodies(1),
    );
    c.push(
        Case::new("cyl/hole-on-edge", "cyl", plate().minus(hole(0.0, 0.0, 2.0, -1.0, 7.0)))
            .volume(500.0 - 5.0 * PI)
            .bodies(1)
            .note("a hole centred on the plate's vertical edge leaves a quarter notch"),
    );
    c.push(
        Case::new(
            "cyl/overlapping-holes",
            "cyl",
            plate().minus(hole(5.0, 5.0, 1.0, -1.0, 7.0)).minus(hole(6.5, 5.0, 1.0, -1.0, 7.0)),
        )
        .volume(500.0 - 5.0 * (2.0 * PI - lens_area(1.0, 1.5)))
        .bodies(1)
        .genus(1),
    );
    c.push(
        Case::new(
            "cyl/coaxial-stack",
            "cyl",
            hole(0.0, 0.0, 2.0, 0.0, 5.0).union(hole(0.0, 0.0, 2.0, 3.0, 5.0)),
        )
        .volume(32.0 * PI)
        .bodies(1)
        .note("two cylinders of the same radius on one axis share a cylindrical face"),
    );
    let turned = |z0: f64, len: f64| {
        Tree::Prim(crate::csg::Prim::Cylinder {
            frame: crate::csg::frame(v(0.0, 0.0, z0), DVec3::Z, v(1.0, 0.7, 0.0)),
            radius: 2.0,
            z0: 0.0,
            z1: len,
        })
    };
    c.push(
        Case::new("cyl/coaxial-seams-apart", "cyl", hole(0.0, 0.0, 2.0, 0.0, 5.0).minus(turned(3.0, 5.0)))
            .volume(12.0 * PI)
            .bodies(1)
            .note(
                "the cylinders' seams lie at different angles, so each cap circle crosses the other's seam",
            ),
    );
    c.push(
        Case::new("cyl/coaxial-inside", "cyl", hole(0.0, 0.0, 2.0, 0.0, 8.0).minus(turned(3.0, 2.0)))
            .volume(24.0 * PI)
            .bodies(2)
            .note("a slice of the same radius cut from the middle leaves two cylinders"),
    );
    c.push(
        Case::new(
            "cyl/hole-continues-hole",
            "cyl",
            cuboid(v(-5.0, -5.0, 0.0), v(5.0, 5.0, 6.0))
                .minus(hole(0.0, 0.0, 2.0, -1.0, 4.0))
                .minus(turned(2.0, 5.0)),
        )
        .volume(600.0 - 24.0 * PI)
        .bodies(1)
        .genus(1)
        .note("a hole drilled from each side that meet in the middle"),
    );
    c.push(
        Case::new(
            "cyl/coaxial-tube",
            "cyl",
            hole(0.0, 0.0, 3.0, 0.0, 5.0).minus(hole(0.0, 0.0, 1.0, -1.0, 7.0)),
        )
        .volume(40.0 * PI)
        .bodies(1)
        .genus(1),
    );
    c.push(
        Case::new(
            "cyl/coaxial-step",
            "cyl",
            hole(0.0, 0.0, 3.0, 0.0, 5.0).union(hole(0.0, 0.0, 1.0, 5.0, 3.0)),
        )
        .volume(45.0 * PI + 3.0 * PI)
        .bodies(1),
    );
    c.push(
        Case::new("cyl/tangent-outside", "cyl", plate().union(hole(12.0, 5.0, 2.0, 0.0, 5.0)))
            .volume(500.0 + 20.0 * PI)
            .note("touching along a line: two bodies or a clear error are both fine"),
    );
    let cx = |r: f64| cylinder(v(-6.0, 0.0, 0.0), DVec3::X, r, 12.0);
    let cy = |r: f64| cylinder(v(0.0, -6.0, 0.0), DVec3::Y, r, 12.0);
    let cz = |r: f64| cylinder(v(0.0, 0.0, -6.0), DVec3::Z, r, 12.0);
    let block = || cuboid(DVec3::splat(-5.0), DVec3::splat(5.0));
    c.push(
        Case::new("cyl/cross-holes-equal", "cyl", block().minus(cx(2.0)).minus(cy(2.0)))
            .volume(1000.0 - (80.0 * PI - 128.0 / 3.0))
            .bodies(1)
            .note("equal-radius cross holes meet at points where their surfaces are tangent"),
    );
    c.push(Case::new("cyl/cross-holes-unequal", "cyl", block().minus(cx(2.0)).minus(cy(1.0))).bodies(1));
    c.push(
        Case::new("cyl/steinmetz-2", "cyl", cx(2.0).and(cy(2.0)))
            .volume(128.0 / 3.0)
            .bodies(1)
            .note("the two ellipses of equal cylinders cross where the surfaces are tangent"),
    );
    c.push(
        Case::new("cyl/steinmetz-3", "cyl", cx(2.0).and(cy(2.0)).and(cz(2.0)))
            .volume((16.0 - 8.0 * 2f64.sqrt()) * 8.0)
            .bodies(1)
            .note("three equal cylinders: six ellipse arcs meeting at tangent points"),
    );
    c.push(
        Case::new(
            "cyl/angled-hole",
            "cyl",
            plate().minus(cylinder(v(1.0, 5.0, -2.0), v(1.0, 0.0, 1.0), 1.0, 12.0)),
        )
        .bodies(1)
        .genus(1),
    );
    c.push(
        Case::new(
            "cyl/tee-unequal",
            "cyl",
            cylinder(v(-10.0, 0.0, 0.0), DVec3::X, 2.0, 20.0).union(cylinder(
                DVec3::ZERO,
                DVec3::Z,
                1.0,
                5.0,
            )),
        )
        .bodies(1),
    );
    c.push(
        Case::new(
            "cyl/tee-equal",
            "cyl",
            cylinder(v(-10.0, 0.0, 0.0), DVec3::X, 2.0, 20.0).union(cylinder(
                DVec3::ZERO,
                DVec3::Z,
                2.0,
                5.0,
            )),
        )
        .bodies(1)
        .note("equal-radius tee: the intersection curve has two singular points"),
    );

    // --- Spheres ---------------------------------------------------------------------
    c.push(
        Case::new(
            "sphere/octant",
            "sphere",
            sphere(DVec3::ZERO, 2.0).and(cuboid(DVec3::ZERO, DVec3::splat(3.0))),
        )
        .volume(4.0 * PI / 3.0)
        .bodies(1),
    );
    c.push(
        Case::new(
            "sphere/half",
            "sphere",
            sphere(DVec3::ZERO, 2.0).minus(cuboid(v(0.0, -3.0, -3.0), v(3.0, 3.0, 3.0))),
        )
        .volume(16.0 * PI / 3.0)
        .bodies(1)
        .note("the cutting plane passes through the ball's poles"),
    );
    c.push(
        Case::new("sphere/lens", "sphere", sphere(DVec3::ZERO, 2.0).and(sphere(v(2.0, 0.0, 0.0), 2.0)))
            .volume(10.0 * PI / 3.0)
            .bodies(1),
    );
    c.push(
        Case::new(
            "sphere/pair-union",
            "sphere",
            sphere(DVec3::ZERO, 2.0).union(sphere(v(2.0, 0.0, 0.0), 2.0)),
        )
        .volume(18.0 * PI)
        .bodies(1),
    );
    c.push(
        Case::new("sphere/dent", "sphere", plate().minus(sphere(v(5.0, 5.0, 5.0), 1.5)))
            .volume(500.0 - 2.25 * PI)
            .bodies(1),
    );
    c.push(
        Case::new("sphere/ball-on-corner", "sphere", plate().union(sphere(v(10.0, 10.0, 5.0), 1.0)))
            .volume(500.0 + 7.0 / 8.0 * 4.0 / 3.0 * PI)
            .bodies(1),
    );
    c.push(
        Case::new(
            "sphere/in-bore",
            "sphere",
            cylinder(v(0.0, 0.0, -3.0), DVec3::Z, 2.0, 6.0).minus(sphere(DVec3::ZERO, 2.0)),
        )
        .volume(24.0 * PI - 32.0 * PI / 3.0)
        .graceful()
        .note("pieces meeting only in a circle are refused: the kernel's solids are manifold"),
    );
    c.push(
        Case::new(
            "sphere/tangent-to-face",
            "sphere",
            cuboid(v(-3.0, -3.0, -1.0), v(3.0, 3.0, 1.0)).union(sphere(v(0.0, 0.0, 2.0), 1.0)),
        )
        .volume(72.0 + 4.0 * PI / 3.0)
        .note("touching at a point: two bodies or a clear error are both fine"),
    );

    // --- Cones and tori ----------------------------------------------------------------
    c.push(
        Case::new(
            "cone/through-box",
            "cone",
            cuboid(v(-3.0, -3.0, 0.0), v(3.0, 3.0, 4.0)).minus(cone(
                v(0.0, 0.0, -1.0),
                DVec3::Z,
                2.0,
                0.5,
                6.0,
            )),
        )
        .bodies(1)
        .genus(1),
    );
    c.push(
        Case::new("cone/countersink", "cone", plate().minus(cone(v(5.0, 5.0, 3.0), DVec3::Z, 0.0, 3.0, 3.0)))
            .volume(500.0 - 8.0 * PI / 3.0)
            .bodies(1)
            .note("a pointed cone whose cut stops at the plate's top"),
    );
    c.push(
        Case::new(
            "cone/countersink-breaks-out",
            "cone",
            plate().minus(cone(v(1.0, 5.0, 2.0), DVec3::Z, 0.0, 3.0, 3.0)),
        )
        .bodies(1)
        .note("the side wall, parallel to the cone's axis, cuts it in a hyperbola"),
    );
    c.push(
        Case::new(
            "cone/sliced-along-axis",
            "cone",
            cone(DVec3::ZERO, DVec3::Z, 2.0, 0.5, 4.0).and(cuboid(v(0.4, -3.0, -1.0), v(3.0, 3.0, 5.0))),
        )
        .bodies(1)
        .note("a slice parallel to the axis: hyperbolas on the side, lines on the caps"),
    );
    c.push(
        Case::new(
            "torus/half",
            "torus",
            torus(DVec3::ZERO, DVec3::Z, 4.0, 1.0).and(cuboid(v(-6.0, -6.0, 0.0), v(6.0, 6.0, 2.0))),
        )
        .volume(4.0 * PI * PI)
        .bodies(1)
        .note("the cutting plane passes through the tube's centre circle"),
    );
    c.push(
        Case::new(
            "torus/quarter-box",
            "torus",
            torus(DVec3::ZERO, DVec3::Z, 4.0, 1.0).and(cuboid(v(0.0, 0.0, -2.0), v(6.0, 6.0, 2.0))),
        )
        .volume(PI * PI * 2.0)
        .bodies(1),
    );
    c.push(
        Case::new(
            "torus/threaded-rod",
            "torus",
            torus(DVec3::ZERO, DVec3::Z, 4.0, 1.0).union(cylinder(v(4.0, 0.0, -3.0), DVec3::Z, 0.5, 6.0)),
        )
        .bodies(1),
    );
    c.push(
        Case::new(
            "torus/ring-groove",
            "torus",
            cylinder(v(0.0, 0.0, -2.0), DVec3::Z, 4.0, 4.0).minus(torus(DVec3::ZERO, DVec3::Z, 4.0, 1.0)),
        )
        .volume(64.0 * PI - (8.0 * PI * PI - torus_outer_half()))
        .bodies(1)
        .note("a groove whose centre circle lies on the cylinder's surface"),
    );

    // --- Fillets and chamfers ----------------------------------------------------------
    // The kernel blends an edge picked by a point on it; the reference is the exact shape
    // in CSG (a round is a square prism less a cylinder, a bevel a slab).
    let block = || cuboid(DVec3::ZERO, v(10.0, 6.0, 4.0));
    let corner = 1.0 - PI / 4.0;
    c.push(
        Case::new(
            "blend/fillet-box-edge",
            "blend",
            block().minus(round_off(v(0.0, 6.0, 4.0), DVec3::X, -DVec3::Y, -DVec3::Z, 1.0, 10.0)),
        )
        .features(block(), vec![Step::Fillet { at: v(5.0, 6.0, 4.0), radius: 1.0 }])
        .volume(240.0 - corner * 10.0)
        .bodies(1),
    );
    c.push(
        Case::new(
            "blend/fillet-small-radius",
            "blend",
            block().minus(round_off(v(0.0, 6.0, 4.0), DVec3::X, -DVec3::Y, -DVec3::Z, 0.05, 10.0)),
        )
        .features(block(), vec![Step::Fillet { at: v(5.0, 6.0, 4.0), radius: 0.05 }])
        .volume(240.0 - corner * 0.0025 * 10.0)
        .bodies(1),
    );
    let mut four = block();
    let mut steps = Vec::new();
    for (x, y, ux, uy) in
        [(0.0, 0.0, 1.0, 1.0), (10.0, 0.0, -1.0, 1.0), (10.0, 6.0, -1.0, -1.0), (0.0, 6.0, 1.0, -1.0)]
    {
        four = four.minus(round_off(v(x, y, 0.0), DVec3::Z, v(ux, 0.0, 0.0), v(0.0, uy, 0.0), 1.0, 4.0));
        steps.push(Step::Fillet { at: v(x, y, 2.0), radius: 1.0 });
    }
    c.push(
        Case::new("blend/fillet-four-vertical-edges", "blend", four)
            .features(block(), steps)
            .volume(240.0 - 4.0 * corner * 4.0)
            .change(-4.0 * corner * 4.0)
            .bodies(1)
            .note("each round ends on the top and bottom faces; the next edge is blended on the result"),
    );
    c.push(
        Case::new(
            "blend/chamfer-box-edge",
            "blend",
            block().minus(bevel_off(v(0.0, 6.0, 4.0), DVec3::X, -DVec3::Y, -DVec3::Z, 1.0, 10.0)),
        )
        .features(block(), vec![Step::Chamfer { at: v(5.0, 6.0, 4.0), distance: 1.0 }])
        .volume(235.0)
        .bodies(1),
    );
    let boss =
        || cuboid(v(0.0, 0.0, 0.0), v(10.0, 10.0, 2.0)).union(cuboid(v(3.0, 3.0, 2.0), v(7.0, 6.0, 5.0)));
    c.push(
        Case::new(
            "blend/fillet-boss-root",
            "blend",
            boss().union(round_off(v(3.0, 3.0, 2.0), DVec3::Y, -DVec3::X, DVec3::Z, 1.0, 3.0)),
        )
        .features(boss(), vec![Step::Fillet { at: v(3.0, 4.5, 2.0), radius: 1.0 }])
        .volume(236.0 + corner * 3.0)
        .change(corner * 3.0)
        .bodies(1)
        .note("a concave round adds material along the root of a boss and ends flush with its sides"),
    );
    let drilled = || plate().minus(hole(5.0, 5.0, 2.0, -1.0, 7.0));
    c.push(
        Case::new(
            "blend/fillet-hole-rim",
            "blend",
            drilled().minus(cylinder(v(5.0, 5.0, 4.5), DVec3::Z, 2.5, 1.5).minus(torus(
                v(5.0, 5.0, 4.5),
                DVec3::Z,
                2.5,
                0.5,
            ))),
        )
        .features(drilled(), vec![Step::Fillet { at: v(7.0, 5.0, 5.0), radius: 0.5 }])
        .change(-rim_round(2.0, 0.5))
        .bodies(1)
        .genus(1)
        .note("a round on a circular edge is a torus"),
    );
    c.push(
        Case::new(
            "blend/chamfer-hole-rim",
            "blend",
            drilled().minus(cone(v(5.0, 5.0, 4.5), DVec3::Z, 2.0, 3.5, 1.5)),
        )
        .features(drilled(), vec![Step::Chamfer { at: v(7.0, 5.0, 5.0), distance: 0.5 }])
        .volume(500.0 - 20.0 * PI - PI * 0.5 / 3.0 * (4.0 + 5.0 + 6.25) + 4.0 * PI * 0.5)
        .change(-(PI * 0.5 / 3.0 * (4.0 + 5.0 + 6.25) - 4.0 * PI * 0.5))
        .bodies(1)
        .genus(1),
    );

    // --- Chains and patterns (performance) ----------------------------------------------
    let mut grid = cuboid(DVec3::ZERO, v(50.0, 50.0, 5.0));
    for i in 0..5 {
        for j in 0..5 {
            grid = grid.minus(hole(5.0 + 10.0 * i as f64, 5.0 + 10.0 * j as f64, 1.5, -1.0, 7.0));
        }
    }
    c.push(
        Case::new("chain/plate-25-holes", "chain", grid)
            .volume(12500.0 - 25.0 * 2.25 * PI * 5.0)
            .bodies(1)
            .genus(25),
    );
    let mut grid = cuboid(DVec3::ZERO, v(80.0, 80.0, 5.0));
    for i in 0..8 {
        for j in 0..8 {
            grid = grid.minus(hole(5.0 + 10.0 * i as f64, 5.0 + 10.0 * j as f64, 1.5, -1.0, 7.0));
        }
    }
    c.push(
        Case::new("chain/plate-64-holes", "chain", grid)
            .volume(32000.0 - 64.0 * 2.25 * PI * 5.0)
            .bodies(1)
            .genus(64)
            .note("a pattern of holes: each cut must not slow down with the number before it"),
    );
    let mut stair = cuboid(DVec3::ZERO, v(2.0, 2.0, 1.0));
    for k in 1..12 {
        let k = k as f64;
        stair = stair.union(cuboid(v(k, 0.0, k * 0.5), v(k + 2.0, 2.0, k * 0.5 + 1.0)));
    }
    c.push(Case::new("chain/staircase-12", "chain", stair).bodies(1));
    let mut perforated = sphere(DVec3::ZERO, 5.0);
    for (i, axis) in [DVec3::X, DVec3::Y, DVec3::Z].into_iter().enumerate() {
        let r = 1.0 + 0.25 * i as f64;
        perforated = perforated.minus(cylinder(-axis * 6.0, axis, r, 12.0));
    }
    c.push(Case::new("chain/perforated-ball", "chain", perforated).bodies(1).genus(5));
    c
}

/// Volume of the outer half (radius above the centre circle) of a torus R=4, r=1: the
/// half-disc's centroid sits 4r/(3 pi) outside the centre circle (Pappus).
fn torus_outer_half() -> f64 {
    let (big, r) = (4.0, 1.0);
    let half_disc = PI * r * r / 2.0;
    half_disc * 2.0 * PI * (big + 4.0 * r / (3.0 * PI))
}

/// The volume a round of radius `r` takes off the rim of a hole of radius `big`: the
/// corner's cross-section (a square of side `r` less a quarter disc, area
/// `(1 - pi / 4) r^2`, whose centroid sits `r / (6 (1 - pi / 4))` from the disc's centre)
/// swept round the axis (Pappus).
fn rim_round(big: f64, r: f64) -> f64 {
    let k = 1.0 - PI / 4.0;
    let centroid = big + r - r / (6.0 * k);
    2.0 * PI * centroid * k * r * r
}

/// The material a round of radius `r` takes off a convex edge (or, with `u` and `v`
/// pointing out of the part, adds in a concave corner): the square of side `r` between
/// the faces less the quarter disc, swept `len` along `axis` from `e`. `u` and `v` point
/// from the edge along its two faces.
fn round_off(e: DVec3, axis: DVec3, u: DVec3, v: DVec3, r: f64, len: f64) -> Tree {
    let mid = e + axis * (0.5 * len);
    let prism = Tree::Prim(crate::csg::Prim::Cuboid {
        frame: crate::csg::frame(mid + (u + v) * (0.5 * r), axis, u),
        half: DVec3::new(0.5 * r, 0.5 * r, 0.5 * len),
    });
    let rod = Tree::Prim(crate::csg::Prim::Cylinder {
        frame: crate::csg::frame(e + (u + v) * r, axis, u),
        radius: r,
        z0: -1.0,
        z1: len + 1.0,
    });
    prism.minus(rod)
}

/// The material an equal-distance bevel of `d` takes off a convex edge: the slab beyond
/// the plane through the points `d` along each face, swept `len` along `axis` from `e`.
fn bevel_off(e: DVec3, axis: DVec3, u: DVec3, v: DVec3, d: f64, len: f64) -> Tree {
    let w = (u + v).normalize();
    let big = 10.0 * d;
    Tree::Prim(crate::csg::Prim::Cuboid {
        frame: crate::csg::frame(e + axis * (0.5 * len) + w * (d / 2f64.sqrt() - big), axis, u - v),
        half: DVec3::new(big, big, 0.5 * len),
    })
}

/// The cases whose expectation is [`Expect::Pass`].
pub fn must_pass() -> Vec<Case> {
    cases().into_iter().filter(|c| c.expect == Expect::Pass).collect()
}
