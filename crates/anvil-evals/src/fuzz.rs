//! Random cases. Positions and sizes are drawn from a coarse grid most of the time, so
//! faces line up, cylinders share axes and spheres touch planes far more often than
//! uniform random numbers would make them: the coincidences real parts are full of.
//! Every case is reproducible from its seed.

use crate::Case;
use crate::csg::{Op, Prim, Tree, frame};
use crate::rng::Rng;
use glam::DVec3;

/// A grid coordinate in `[-2, 2]` on halves, or (rarely) an arbitrary value.
fn coord(r: &mut Rng) -> f64 {
    if r.chance(0.8) { (r.below(9) as f64 - 4.0) * 0.5 } else { r.range(-2.0, 2.0) }
}

fn size(r: &mut Rng) -> f64 {
    if r.chance(0.8) { *r.pick(&[0.5, 1.0, 1.5, 2.0, 2.5]) } else { r.range(0.3, 2.5) }
}

fn axis(r: &mut Rng) -> DVec3 {
    if r.chance(0.75) {
        *r.pick(&[DVec3::X, DVec3::Y, DVec3::Z])
    } else if r.chance(0.5) {
        // 45 degrees in a coordinate plane.
        *r.pick(&[DVec3::new(1.0, 1.0, 0.0), DVec3::new(0.0, 1.0, 1.0), DVec3::new(1.0, 0.0, 1.0)])
    } else {
        DVec3::new(r.range(-1.0, 1.0), r.range(-1.0, 1.0), r.range(-1.0, 1.0)).normalize_or(DVec3::Z)
    }
}

fn point(r: &mut Rng) -> DVec3 {
    DVec3::new(coord(r), coord(r), coord(r))
}

/// A random primitive. `kinds` limits which ones appear (0 box, 1 cylinder, 2 sphere,
/// 3 cone, 4 torus).
pub fn prim(r: &mut Rng, kinds: &[usize]) -> Prim {
    match *r.pick(kinds) {
        0 => {
            let z = axis(r);
            let x = if r.chance(0.7) { z.any_orthonormal_vector() } else { axis(r) };
            Prim::Cuboid {
                frame: frame(point(r), z, x),
                half: DVec3::new(size(r), size(r), size(r)) * 0.5 + 0.1,
            }
        }
        1 => {
            let len = size(r) * 2.0;
            Prim::Cylinder {
                frame: frame(point(r), axis(r), DVec3::X),
                radius: size(r) * 0.5 + 0.1,
                z0: -0.5 * len,
                z1: 0.5 * len,
            }
        }
        2 => Prim::Sphere { center: point(r), radius: size(r) * 0.5 + 0.2 },
        3 => {
            let r0 = size(r) * 0.5 + 0.1;
            let r1 = if r.chance(0.3) { 0.0 } else { size(r) * 0.4 };
            Prim::Cone { frame: frame(point(r), axis(r), DVec3::X), r0, r1, height: size(r) + 0.2 }
        }
        _ => {
            let minor = size(r) * 0.2 + 0.1;
            Prim::Torus {
                frame: frame(point(r), axis(r), DVec3::X),
                major: minor + size(r) * 0.5 + 0.2,
                minor,
            }
        }
    }
}

/// A random tree of `n` primitives combined left to right.
pub fn tree(r: &mut Rng, n: usize, kinds: &[usize]) -> Tree {
    let mut t = Tree::Prim(prim(r, kinds));
    for _ in 1..n.max(1) {
        let op = *r.pick(&[Op::Union, Op::Subtract, Op::Intersect, Op::Subtract]);
        t = Tree::op(op, t, Tree::Prim(prim(r, kinds)));
    }
    t
}

/// Case `index` of the stream for `seed`.
pub fn case(seed: u64, index: u64, kinds: &[usize], max_prims: usize) -> Case {
    let mut r = Rng::new(seed.wrapping_mul(0x1000_0000_01B3).wrapping_add(index));
    let n = 2 + r.below(max_prims.max(2) - 1);
    let t = tree(&mut r, n, kinds);
    Case::new(format!("fuzz/{seed}-{index}"), "fuzz", t).graceful()
}

/// True when a tree's result is empty or so thin that a reference volume is unreliable;
/// such cases are skipped.
pub fn degenerate(t: &Tree) -> bool {
    let (lo, hi) = t.bounds();
    (hi - lo).min_element() <= 0.05 || t.volume(48) < 1e-3
}
