//! An analytic reference model: constructive solid geometry over exact primitives.
//!
//! Every evaluation case is described twice: as a tree of primitives and boolean
//! operations here, which can answer "is this point inside?" and "where does this ray
//! cross the boundary?" exactly, and as the same tree built with the kernel. The
//! kernel's answer is then checked against this one. Nothing here uses the kernel's
//! geometry code, so a bug in the kernel cannot hide itself by being repeated in the
//! reference.

use anvil_brep::boolean::{self, BoolOp};
use anvil_brep::profile::{Loop2, Segment, build_regions};
use anvil_brep::{Frame, KernelError, Solid, extrude, revolve};
use glam::{DVec2, DVec3};
use serde::{Deserialize, Serialize};
use std::f64::consts::{PI, TAU};

/// A solid primitive placed by an orthonormal frame.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Prim {
    /// A box centred on `frame.origin` with half extents `half` along the frame's axes.
    Cuboid {
        frame: Frame,
        half: DVec3,
    },
    /// A cylinder around `frame.z`, from `z0` to `z1` along it.
    Cylinder {
        frame: Frame,
        radius: f64,
        z0: f64,
        z1: f64,
    },
    /// A cone frustum around `frame.z`: radius `r0` at `z = 0`, `r1` at `z = height`.
    /// One radius may be zero (a pointed cone).
    Cone {
        frame: Frame,
        r0: f64,
        r1: f64,
        height: f64,
    },
    Sphere {
        center: DVec3,
        radius: f64,
    },
    /// A ring torus around `frame.z` (`major > minor`).
    Torus {
        frame: Frame,
        major: f64,
        minor: f64,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Tree {
    Prim(Prim),
    Op(Op, Box<Tree>, Box<Tree>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Op {
    Union,
    Subtract,
    Intersect,
}

impl Op {
    fn apply(self, a: bool, b: bool) -> bool {
        match self {
            Op::Union => a || b,
            Op::Subtract => a && !b,
            Op::Intersect => a && b,
        }
    }

    pub fn kernel(self) -> BoolOp {
        match self {
            Op::Union => BoolOp::Union,
            Op::Subtract => BoolOp::Subtract,
            Op::Intersect => BoolOp::Intersect,
        }
    }
}

/// One stretch of a ray inside a solid: it enters at `t0` through a boundary with
/// outward normal `n0` and leaves at `t1` through one with outward normal `n1`. Infinite
/// ends have zero normals.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Span {
    pub t0: f64,
    pub n0: DVec3,
    pub t1: f64,
    pub n1: DVec3,
}

/// Builds a frame, falling back to the world frame for a degenerate normal (callers
/// only pass well-formed directions).
pub fn frame(origin: DVec3, z: DVec3, x_hint: DVec3) -> Frame {
    let hint = if z.normalize_or_zero().dot(x_hint.normalize_or_zero()).abs() > 0.99 {
        if z.x.abs() < 0.9 { DVec3::X } else { DVec3::Y }
    } else {
        x_hint
    };
    Frame::new(origin, z, hint).unwrap_or(Frame { origin, ..Frame::world() })
}

pub fn cuboid(lo: DVec3, hi: DVec3) -> Tree {
    Tree::Prim(Prim::Cuboid {
        frame: Frame { origin: (lo + hi) * 0.5, ..Frame::world() },
        half: (hi - lo) * 0.5,
    })
}

pub fn cylinder(base: DVec3, axis: DVec3, radius: f64, length: f64) -> Tree {
    Tree::Prim(Prim::Cylinder { frame: frame(base, axis, DVec3::X), radius, z0: 0.0, z1: length })
}

pub fn cone(base: DVec3, axis: DVec3, r0: f64, r1: f64, height: f64) -> Tree {
    Tree::Prim(Prim::Cone { frame: frame(base, axis, DVec3::X), r0, r1, height })
}

pub fn sphere(center: DVec3, radius: f64) -> Tree {
    Tree::Prim(Prim::Sphere { center, radius })
}

pub fn torus(center: DVec3, axis: DVec3, major: f64, minor: f64) -> Tree {
    Tree::Prim(Prim::Torus { frame: frame(center, axis, DVec3::X), major, minor })
}

impl Tree {
    pub fn op(op: Op, a: Tree, b: Tree) -> Tree {
        Tree::Op(op, Box::new(a), Box::new(b))
    }
    pub fn union(self, b: Tree) -> Tree {
        Tree::op(Op::Union, self, b)
    }
    pub fn minus(self, b: Tree) -> Tree {
        Tree::op(Op::Subtract, self, b)
    }
    pub fn and(self, b: Tree) -> Tree {
        Tree::op(Op::Intersect, self, b)
    }

    pub fn contains(&self, p: DVec3) -> bool {
        match self {
            Tree::Prim(q) => q.contains(p),
            Tree::Op(op, a, b) => op.apply(a.contains(p), b.contains(p)),
        }
    }

    /// The smallest distance from `p` to the boundary of any primitive in the tree. A
    /// point closer than a tolerance to some primitive's boundary is ambiguous: the
    /// kernel may round it either way.
    pub fn boundary_distance(&self, p: DVec3) -> f64 {
        match self {
            Tree::Prim(q) => q.sdf(p).abs(),
            Tree::Op(_, a, b) => a.boundary_distance(p).min(b.boundary_distance(p)),
        }
    }

    pub fn spans(&self, o: DVec3, d: DVec3) -> Vec<Span> {
        match self {
            Tree::Prim(q) => q.spans(o, d),
            Tree::Op(op, a, b) => combine(*op, &a.spans(o, d), &b.spans(o, d)),
        }
    }

    /// A box containing the solid. For a subtraction or intersection it is the first
    /// operand's box (or the overlap), which is always enough.
    pub fn bounds(&self) -> (DVec3, DVec3) {
        match self {
            Tree::Prim(q) => q.bounds(),
            Tree::Op(Op::Union, a, b) => {
                let (a, b) = (a.bounds(), b.bounds());
                (a.0.min(b.0), a.1.max(b.1))
            }
            Tree::Op(Op::Subtract, a, _) => a.bounds(),
            Tree::Op(Op::Intersect, a, b) => {
                let (a, b) = (a.bounds(), b.bounds());
                (a.0.max(b.0), a.1.min(b.1))
            }
        }
    }

    pub fn prims(&self) -> Vec<&Prim> {
        match self {
            Tree::Prim(q) => vec![q],
            Tree::Op(_, a, b) => {
                let mut v = a.prims();
                v.extend(b.prims());
                v
            }
        }
    }

    /// The reference volume, integrated exactly along a grid of `n` by `n` rays
    /// parallel to world z (each ray's inside length is exact; only the grid is
    /// approximate, so the error falls as the square of the cell size away from
    /// silhouettes).
    pub fn volume(&self, n: usize) -> f64 {
        let (lo, hi) = self.bounds();
        let size = hi - lo;
        if size.x <= 0.0 || size.y <= 0.0 || size.z <= 0.0 {
            return 0.0;
        }
        let (dx, dy) = (size.x / n as f64, size.y / n as f64);
        let z0 = lo.z - 1.0 - size.z;
        let mut sum = 0.0;
        for i in 0..n {
            for j in 0..n {
                let o = DVec3::new(lo.x + (i as f64 + 0.5) * dx, lo.y + (j as f64 + 0.5) * dy, z0);
                for s in self.spans(o, DVec3::Z) {
                    if s.t0.is_finite() && s.t1.is_finite() {
                        sum += s.t1 - s.t0;
                    }
                }
            }
        }
        sum * dx * dy
    }

    /// Builds the tree with the kernel. A union whose operands stay apart keeps both
    /// bodies; later operations apply to each body.
    pub fn build(&self) -> Result<Vec<Solid>, KernelError> {
        match self {
            Tree::Prim(q) => Ok(vec![q.build()?]),
            Tree::Op(op, a, b) => {
                let a = a.build()?;
                let b = b.build()?;
                apply(*op, a, b)
            }
        }
    }
}

/// Applies a boolean to two sets of bodies with the kernel.
pub fn apply(op: Op, a: Vec<Solid>, b: Vec<Solid>) -> Result<Vec<Solid>, KernelError> {
    match op {
        Op::Union => {
            let mut bodies = a;
            for tool in b {
                // Merge the tool with every body it touches, one at a time.
                let mut merged = tool;
                let mut rest = Vec::new();
                for body in bodies {
                    let r = boolean::boolean(&body, &merged, BoolOp::Union)?;
                    if r.len() == 1 {
                        merged = r.into_iter().next().unwrap_or_default();
                    } else {
                        rest.push(body);
                    }
                }
                rest.push(merged);
                bodies = rest;
            }
            Ok(bodies)
        }
        Op::Subtract => {
            let mut bodies = a;
            for tool in &b {
                let mut next = Vec::new();
                for body in &bodies {
                    next.extend(boolean::boolean(body, tool, BoolOp::Subtract)?);
                }
                bodies = next;
            }
            Ok(bodies)
        }
        Op::Intersect => {
            let mut out = Vec::new();
            for x in &a {
                for y in &b {
                    out.extend(boolean::boolean(x, y, BoolOp::Intersect)?);
                }
            }
            Ok(out)
        }
    }
}

/// Combines two sorted, disjoint span lists by a boolean operation.
fn combine(op: Op, a: &[Span], b: &[Span]) -> Vec<Span> {
    // Boundary events: (t, normal, from b, entering).
    let mut ev: Vec<(f64, DVec3, bool, bool)> = Vec::with_capacity(2 * (a.len() + b.len()));
    for (list, from_b) in [(a, false), (b, true)] {
        for s in list {
            ev.push((s.t0, s.n0, from_b, true));
            ev.push((s.t1, s.n1, from_b, false));
        }
    }
    ev.sort_by(|x, y| x.0.total_cmp(&y.0));
    let (mut in_a, mut in_b) = (false, false);
    let mut out = Vec::new();
    let mut open: Option<(f64, DVec3)> = None;
    for (t, n, from_b, entering) in ev {
        let before = op.apply(in_a, in_b);
        if from_b {
            in_b = entering;
        } else {
            in_a = entering;
        }
        let after = op.apply(in_a, in_b);
        if before == after {
            continue;
        }
        // A boundary of b bounds a subtraction from the other side.
        let n = if from_b && op == Op::Subtract { -n } else { n };
        if after {
            open = Some((t, n));
        } else if let Some((t0, n0)) = open.take()
            && t - t0 > 1e-12
        {
            out.push(Span { t0, n0, t1: t, n1: n });
        }
    }
    out
}

fn span_and(a: Span, b: Span) -> Option<Span> {
    let (t0, n0) = if a.t0 >= b.t0 { (a.t0, a.n0) } else { (b.t0, b.n0) };
    let (t1, n1) = if a.t1 <= b.t1 { (a.t1, a.n1) } else { (b.t1, b.n1) };
    (t1 > t0).then_some(Span { t0, n0, t1, n1 })
}

fn whole() -> Span {
    Span { t0: f64::NEG_INFINITY, n0: DVec3::ZERO, t1: f64::INFINITY, n1: DVec3::ZERO }
}

/// The span of a ray (local coordinates) between two planes `z = lo` and `z = hi`.
fn slab(o: f64, d: f64, lo: f64, hi: f64, axis: DVec3) -> Option<Span> {
    if d.abs() < 1e-15 {
        return (o > lo && o < hi).then(whole);
    }
    let (ta, tb) = ((lo - o) / d, (hi - o) / d);
    Some(if ta < tb {
        Span { t0: ta, n0: -axis, t1: tb, n1: axis }
    } else {
        Span { t0: tb, n0: axis, t1: ta, n1: -axis }
    })
}

/// Roots of `a t^2 + b t + c`, in order, when there are two.
fn quadratic(a: f64, b: f64, c: f64) -> Option<(f64, f64)> {
    let disc = b * b - 4.0 * a * c;
    if disc <= 0.0 || a == 0.0 {
        return None;
    }
    let s = disc.sqrt();
    // The stable form avoids cancellation.
    let q = -0.5 * (b + b.signum() * s);
    let (r1, r2) = (q / a, if q != 0.0 { c / q } else { -q / a });
    Some((r1.min(r2), r1.max(r2)))
}

impl Prim {
    pub fn contains(&self, p: DVec3) -> bool {
        self.sdf(p) < 0.0
    }

    /// Signed distance (negative inside). Exact for boxes, cylinders, spheres and tori;
    /// a close lower bound for cones.
    pub fn sdf(&self, p: DVec3) -> f64 {
        match self {
            Prim::Cuboid { frame, half } => {
                let q = frame.to_local(p).abs() - *half;
                q.max(DVec3::ZERO).length() + q.max_element().min(0.0)
            }
            Prim::Cylinder { frame, radius, z0, z1 } => {
                let l = frame.to_local(p);
                let mid = 0.5 * (z0 + z1);
                let d = DVec2::new(l.truncate().length() - radius, (l.z - mid).abs() - 0.5 * (z1 - z0));
                d.max(DVec2::ZERO).length() + d.max_element().min(0.0)
            }
            Prim::Cone { frame, r0, r1, height } => {
                let l = frame.to_local(p);
                let rho = l.truncate().length();
                let k = (r1 - r0) / height;
                let side = (rho - (r0 + k * l.z)) / (1.0 + k * k).sqrt();
                let cap = (l.z - height).max(-l.z);
                side.max(cap)
            }
            Prim::Sphere { center, radius } => (p - *center).length() - radius,
            Prim::Torus { frame, major, minor } => {
                let l = frame.to_local(p);
                DVec2::new(l.truncate().length() - major, l.z).length() - minor
            }
        }
    }

    pub fn bounds(&self) -> (DVec3, DVec3) {
        let boxed = |frame: &Frame, lo: DVec3, hi: DVec3| {
            let mut a = DVec3::splat(f64::INFINITY);
            let mut b = DVec3::splat(f64::NEG_INFINITY);
            for i in 0..8 {
                let c = DVec3::new(
                    if i & 1 == 0 { lo.x } else { hi.x },
                    if i & 2 == 0 { lo.y } else { hi.y },
                    if i & 4 == 0 { lo.z } else { hi.z },
                );
                let w = frame.to_world3(c);
                a = a.min(w);
                b = b.max(w);
            }
            (a, b)
        };
        match self {
            Prim::Cuboid { frame, half } => boxed(frame, -*half, *half),
            Prim::Cylinder { frame, radius, z0, z1 } => {
                boxed(frame, DVec3::new(-radius, -radius, *z0), DVec3::new(*radius, *radius, *z1))
            }
            Prim::Cone { frame, r0, r1, height } => {
                let r = r0.max(*r1);
                boxed(frame, DVec3::new(-r, -r, 0.0), DVec3::new(r, r, *height))
            }
            Prim::Sphere { center, radius } => (*center - *radius, *center + *radius),
            Prim::Torus { frame, major, minor } => {
                let r = major + minor;
                boxed(frame, DVec3::new(-r, -r, -minor), DVec3::new(r, r, *minor))
            }
        }
    }

    /// Exact volume.
    pub fn volume(&self) -> f64 {
        match self {
            Prim::Cuboid { half, .. } => 8.0 * half.x * half.y * half.z,
            Prim::Cylinder { radius, z0, z1, .. } => PI * radius * radius * (z1 - z0),
            Prim::Cone { r0, r1, height, .. } => PI * height / 3.0 * (r0 * r0 + r0 * r1 + r1 * r1),
            Prim::Sphere { radius, .. } => 4.0 / 3.0 * PI * radius.powi(3),
            Prim::Torus { major, minor, .. } => 2.0 * PI * PI * major * minor * minor,
        }
    }

    /// Exact boundary area.
    pub fn area(&self) -> f64 {
        match self {
            Prim::Cuboid { half, .. } => 8.0 * (half.x * half.y + half.y * half.z + half.x * half.z),
            Prim::Cylinder { radius, z0, z1, .. } => TAU * radius * (z1 - z0) + TAU * radius * radius,
            Prim::Cone { r0, r1, height, .. } => {
                let slant = ((r1 - r0).powi(2) + height * height).sqrt();
                PI * (r0 + r1) * slant + PI * (r0 * r0 + r1 * r1)
            }
            Prim::Sphere { radius, .. } => 4.0 * PI * radius * radius,
            Prim::Torus { major, minor, .. } => 4.0 * PI * PI * major * minor,
        }
    }

    /// Where the ray `o + t d` is inside the primitive.
    pub fn spans(&self, o: DVec3, d: DVec3) -> Vec<Span> {
        match self {
            Prim::Cuboid { frame, half } => {
                let (lo, ld) = (frame.to_local(o), frame.to_local(o + d) - frame.to_local(o));
                let mut s = whole();
                for (i, axis) in [frame.x, frame.y, frame.z].into_iter().enumerate() {
                    let h = half[i];
                    match slab(lo[i], ld[i], -h, h, axis).and_then(|x| span_and(s, x)) {
                        Some(x) => s = x,
                        None => return Vec::new(),
                    }
                }
                vec![s]
            }
            Prim::Cylinder { frame, radius, z0, z1 } => cone_spans(frame, o, d, *radius, 0.0, *z0, *z1),
            Prim::Cone { frame, r0, r1, height } => {
                cone_spans(frame, o, d, *r0, (r1 - r0) / height, 0.0, *height)
            }
            Prim::Sphere { center, radius } => {
                let oc = o - *center;
                let b = 2.0 * oc.dot(d);
                let c = oc.length_squared() - radius * radius;
                match quadratic(d.length_squared(), b, c) {
                    Some((t0, t1)) => vec![Span {
                        t0,
                        n0: (o + d * t0 - *center).normalize_or_zero(),
                        t1,
                        n1: (o + d * t1 - *center).normalize_or_zero(),
                    }],
                    None => Vec::new(),
                }
            }
            Prim::Torus { frame, major, minor } => torus_spans(frame, o, d, *major, *minor),
        }
    }

    /// Builds the primitive with the kernel's own feature operations: boxes and
    /// cylinders are extrusions, cones, spheres and tori revolutions, as a user would
    /// model them.
    pub fn build(&self) -> Result<Solid, KernelError> {
        let line = |a: DVec2, b: DVec2| Segment::Line { a, b };
        match self {
            Prim::Cuboid { frame, half } => {
                let p = [
                    DVec2::new(-half.x, -half.y),
                    DVec2::new(half.x, -half.y),
                    DVec2::new(half.x, half.y),
                    DVec2::new(-half.x, half.y),
                ];
                let l = Loop2::new((0..4).map(|i| line(p[i], p[(i + 1) % 4])).collect());
                let r = build_regions(&[l])?;
                extrude::extrude(frame, first(&r)?, -half.z, half.z)
            }
            Prim::Cylinder { frame, radius, z0, z1 } => {
                let r = build_regions(&[Loop2::new(vec![Segment::circle(DVec2::ZERO, *radius)])])?;
                extrude::extrude(frame, first(&r)?, *z0, *z1)
            }
            Prim::Cone { frame, r0, r1, height } => {
                let mut pts = vec![DVec2::ZERO];
                if *r0 > 0.0 {
                    pts.push(DVec2::new(*r0, 0.0));
                }
                if *r1 > 0.0 {
                    pts.push(DVec2::new(*r1, *height));
                }
                pts.push(DVec2::new(0.0, *height));
                let n = pts.len();
                let l = Loop2::new((0..n).map(|i| line(pts[i], pts[(i + 1) % n])).collect());
                turn(frame, l)
            }
            Prim::Sphere { center, radius } => {
                let l = Loop2::new(vec![
                    Segment::Arc { center: DVec2::ZERO, radius: *radius, start: -0.5 * PI, sweep: PI },
                    line(DVec2::new(0.0, *radius), DVec2::new(0.0, -*radius)),
                ]);
                turn(&Frame { origin: *center, ..Frame::world() }, l)
            }
            Prim::Torus { frame, major, minor } => {
                turn(frame, Loop2::new(vec![Segment::circle(DVec2::new(*major, 0.0), *minor)]))
            }
        }
    }
}

fn first(r: &[anvil_brep::Region]) -> Result<&anvil_brep::Region, KernelError> {
    r.first().ok_or(KernelError::EmptyProfile)
}

/// Revolves a profile drawn with x as the radius and y along `frame.z` a full turn.
fn turn(frame: &Frame, profile: Loop2) -> Result<Solid, KernelError> {
    // The profile plane holds the radial direction and the axis.
    let plane = Frame { origin: frame.origin, x: frame.x, y: frame.z, z: -frame.y };
    let r = build_regions(&[profile])?;
    revolve::revolve(&plane, first(&r)?, DVec2::ZERO, DVec2::Y, TAU)
}

/// Spans of a ray through a frustum around `frame.z` between `z0` and `z1` whose radius
/// is `r + k (z - z0)` (a cylinder when `k` is zero).
fn cone_spans(frame: &Frame, o: DVec3, d: DVec3, r: f64, k: f64, z0: f64, z1: f64) -> Vec<Span> {
    let lo = frame.to_local(o);
    let ld = frame.to_local(o + d) - lo;
    // rho(z) = r + k (z - z0) = c0 + k z.
    let c0 = r - k * z0;
    let a = ld.x * ld.x + ld.y * ld.y - k * k * ld.z * ld.z;
    let b = 2.0 * (lo.x * ld.x + lo.y * ld.y - k * (c0 + k * lo.z) * ld.z);
    let c = lo.x * lo.x + lo.y * lo.y - (c0 + k * lo.z).powi(2);
    let normal = |t: f64| {
        let p = lo + ld * t;
        let g = DVec3::new(p.x, p.y, -k * (c0 + k * p.z));
        frame.apply_dir(g.normalize_or_zero())
    };
    let side = |t0: f64, t1: f64| Span {
        t0,
        n0: if t0.is_finite() { normal(t0) } else { DVec3::ZERO },
        t1,
        n1: if t1.is_finite() { normal(t1) } else { DVec3::ZERO },
    };
    // Where the ray is inside the (double) cone surface.
    let inside: Vec<Span> = if a.abs() < 1e-14 {
        if b.abs() < 1e-14 {
            if c < 0.0 { vec![whole()] } else { Vec::new() }
        } else {
            let t = -c / b;
            if b > 0.0 { vec![side(f64::NEG_INFINITY, t)] } else { vec![side(t, f64::INFINITY)] }
        }
    } else {
        match quadratic(a, b, c) {
            Some((t1, t2)) if a > 0.0 => vec![side(t1, t2)],
            Some((t1, t2)) => vec![side(f64::NEG_INFINITY, t1), side(t2, f64::INFINITY)],
            None if a < 0.0 => vec![whole()],
            None => Vec::new(),
        }
    };
    let Some(cap) = slab(lo.z, ld.z, z0, z1, frame.z) else {
        return Vec::new();
    };
    inside.into_iter().filter_map(|s| span_and(s, cap)).collect()
}

fn torus_spans(frame: &Frame, o: DVec3, d: DVec3, major: f64, minor: f64) -> Vec<Span> {
    let lo = frame.to_local(o);
    let ld = frame.to_local(o + d) - lo;
    let normal = |t: f64| {
        let p = lo + ld * t;
        let s = p.length_squared() + major * major - minor * minor;
        let g = DVec3::new(
            4.0 * p.x * s - 8.0 * major * major * p.x,
            4.0 * p.y * s - 8.0 * major * major * p.y,
            4.0 * p.z * s,
        );
        frame.apply_dir(g.normalize_or_zero())
    };
    // Only the part of the ray inside the bounding sphere can meet the torus.
    let reach = major + minor;
    let Some((ta, tb)) =
        quadratic(ld.length_squared(), 2.0 * lo.dot(ld), lo.length_squared() - reach * reach)
    else {
        return Vec::new();
    };
    // Along the ray the implicit function (|p|^2 + R^2 - r^2)^2 - 4 R^2 (x^2 + y^2) is a
    // quartic in t; its real roots are isolated exactly between the roots of its
    // derivatives, so no crossing is missed however thin.
    let sum = [lo.length_squared() + major * major - minor * minor, 2.0 * lo.dot(ld), ld.length_squared()];
    let rho = [lo.x * lo.x + lo.y * lo.y, 2.0 * (lo.x * ld.x + lo.y * ld.y), ld.x * ld.x + ld.y * ld.y];
    let mut q = [0.0; 5];
    for i in 0..3 {
        for j in 0..3 {
            q[i + j] += sum[i] * sum[j];
        }
        q[i] -= 4.0 * major * major * rho[i];
    }
    let roots = poly_roots(&q, ta, tb);
    let f = |t: f64| horner(&q, t);
    // Pair up the roots into spans, keeping only stretches where the ray is inside.
    let mut cuts = vec![ta];
    cuts.extend(roots);
    cuts.push(tb);
    let mut spans = Vec::new();
    for w in cuts.windows(2) {
        if w[1] - w[0] > 1e-12 && f(0.5 * (w[0] + w[1])) < 0.0 {
            spans.push(Span { t0: w[0], n0: normal(w[0]), t1: w[1], n1: normal(w[1]) });
        }
    }
    spans
}

/// `c[0] + c[1] t + c[2] t^2 + ...`.
fn horner(c: &[f64], t: f64) -> f64 {
    c.iter().rev().fold(0.0, |acc, &k| acc * t + k)
}

/// The real roots of a polynomial in `[lo, hi]`, in order. Each root of the derivative
/// splits the interval into pieces on which the polynomial is monotone, and each piece
/// with a sign change holds exactly one root, found by bisection.
fn poly_roots(c: &[f64], lo: f64, hi: f64) -> Vec<f64> {
    let mut c = c.to_vec();
    while c.len() > 1 && c.last().is_some_and(|k| k.abs() < 1e-300) {
        c.pop();
    }
    if c.len() <= 1 {
        return Vec::new();
    }
    let deriv: Vec<f64> = c.iter().enumerate().skip(1).map(|(i, k)| k * i as f64).collect();
    let mut knots = vec![lo];
    knots.extend(poly_roots(&deriv, lo, hi));
    knots.push(hi);
    let mut roots = Vec::new();
    for w in knots.windows(2) {
        let (mut a, mut b) = (w[0], w[1]);
        let (fa, fb) = (horner(&c, a), horner(&c, b));
        if fa == 0.0 {
            if roots.last().is_none_or(|r: &f64| (a - r).abs() > 1e-12) {
                roots.push(a);
            }
            continue;
        }
        if (fa < 0.0) == (fb < 0.0) {
            continue;
        }
        for _ in 0..200 {
            let m = 0.5 * (a + b);
            if m <= a || m >= b {
                break;
            }
            if (horner(&c, m) < 0.0) == (fa < 0.0) {
                a = m;
            } else {
                b = m;
            }
        }
        roots.push(0.5 * (a + b));
    }
    roots
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn polynomial_roots_are_isolated() {
        // (t - 1)(t - 2)(t - 2.000001)(t + 3): a near-double root is still split.
        let r = [1.0, 2.0, 2.000001, -3.0];
        let mut c = vec![1.0];
        for x in r {
            let mut n = vec![0.0; c.len() + 1];
            for (i, k) in c.iter().enumerate() {
                n[i] -= k * x;
                n[i + 1] += k;
            }
            c = n;
        }
        let got = poly_roots(&c, -10.0, 10.0);
        assert_eq!(got.len(), 4, "{got:?}");
        for (g, w) in got.iter().zip([-3.0, 1.0, 2.0, 2.000001]) {
            // A near-double root is conditioned like the square root of the rounding error.
            assert!((g - w).abs() < 1e-8, "{got:?}");
        }
    }

    #[test]
    fn reference_volumes_match_formulas() {
        let cases = [
            cuboid(DVec3::ZERO, DVec3::new(2.0, 3.0, 4.0)),
            cylinder(DVec3::ZERO, DVec3::new(1.0, 1.0, 0.3), 1.5, 4.0),
            cone(DVec3::ONE, DVec3::Y, 2.0, 0.5, 3.0),
            sphere(DVec3::new(1.0, -2.0, 3.0), 2.0),
            torus(DVec3::ZERO, DVec3::new(0.0, 1.0, 1.0), 4.0, 1.0),
        ];
        for t in cases {
            let Tree::Prim(p) = &t else { continue };
            let v = t.volume(400);
            assert!((v - p.volume()).abs() < 2e-4 * p.volume(), "{p:?}: {v} vs {}", p.volume());
        }
    }

    #[test]
    fn subtraction_and_intersection_volumes() {
        let a = cuboid(DVec3::ZERO, DVec3::splat(2.0));
        let b = cuboid(DVec3::splat(1.0), DVec3::splat(3.0));
        assert!((a.clone().minus(b.clone()).volume(200) - 7.0).abs() < 1e-9);
        assert!((a.clone().and(b.clone()).volume(200) - 1.0).abs() < 1e-9);
        // The union's silhouette does not fall on grid lines, so it is only close.
        assert!((a.union(b).volume(300) - 15.0).abs() < 1e-3);
    }

    #[test]
    fn containment_agrees_with_spans() {
        let t = cuboid(DVec3::ZERO, DVec3::splat(4.0))
            .minus(cylinder(DVec3::new(2.0, 2.0, -1.0), DVec3::Z, 1.0, 6.0))
            .union(sphere(DVec3::new(4.0, 4.0, 4.0), 1.5));
        let o = DVec3::new(2.3, 2.1, -10.0);
        let spans = t.spans(o, DVec3::Z);
        for k in 0..400 {
            let z = -2.0 + k as f64 * 0.02;
            let inside = spans.iter().any(|s| z + 10.0 > s.t0 && z + 10.0 < s.t1);
            if spans.iter().all(|s| (s.t0 - z - 10.0).abs() > 1e-9 && (s.t1 - z - 10.0).abs() > 1e-9) {
                assert_eq!(inside, t.contains(o + DVec3::Z * (z + 10.0)), "z = {z}");
            }
        }
    }
}
