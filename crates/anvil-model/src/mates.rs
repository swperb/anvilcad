//! Mates: geometric relations between assembly components, and the solver that places
//! components to satisfy them.
//!
//! This follows the specification in `docs/re/assemblies-and-mates.md` (sections "Standard
//! mates", "Solving and status" and "AnvilCAD decisions"). A mate relates two entities, each on a
//! component or on the assembly itself: points, straight and circular edges, planar,
//! cylindrical and spherical faces, reference planes, axes and points, and origins.
//!
//! The solver works on rigid placements. Every component that is not fixed has six
//! unknowns: a translation and a small rotation about its own origin. Mates become
//! residual equations, and a damped least-squares (Levenberg-Marquardt) iteration drives
//! them to zero, re-linearising after each accepted step. The damping keeps under-defined
//! components close to where they were, as dragging a part expects. Components linked by
//! mates are solved together; separate groups are solved on their own.
//!
//! After solving, the null space of the Jacobian gives the motions the mates still allow:
//! a component's remaining degrees of freedom are the rank of that null space restricted
//! to its six unknowns. A mate whose residual is left above the tolerance conflicts with
//! the others.

use crate::measure::Shape;
use anvil_kernel::{Frame, Vec3};
use glam::DVec3;
use serde::{Deserialize, Serialize};

/// Positions closer than this (mm) count as the same.
pub const LINEAR_TOLERANCE: f64 = 1e-6;
/// Directions closer than this (radians) count as the same.
pub const ANGULAR_TOLERANCE: f64 = 1e-8;
/// Eigenvalues of JᵀJ below this fraction of the largest count as zero.
const NULL_RATIO: f64 = 1e-10;

/// What a mate makes true.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum MateKind {
    /// The entities touch: points meet, a point lies on a line or plane, planes coincide.
    Coincident,
    /// Axes are collinear, or centres lie on an axis or on each other.
    Concentric,
    /// Directions are parallel.
    Parallel,
    /// Directions are at right angles.
    Perpendicular,
    /// A plane or cylinder touches a cylinder or sphere.
    Tangent,
    /// The entities are this far apart (mm). `flip` puts the second on the other side of
    /// the first, where a side applies.
    Distance { value: f64, flip: bool },
    /// The directions are at this angle (degrees).
    Angle { degrees: f64 },
    /// The second component keeps its placement relative to the first.
    Lock { relative: Frame },
}

impl MateKind {
    pub fn label(&self) -> &'static str {
        match self {
            MateKind::Coincident => "Coincident",
            MateKind::Concentric => "Concentric",
            MateKind::Parallel => "Parallel",
            MateKind::Perpendicular => "Perpendicular",
            MateKind::Tangent => "Tangent",
            MateKind::Distance { .. } => "Distance",
            MateKind::Angle { .. } => "Angle",
            MateKind::Lock { .. } => "Lock",
        }
    }
}

/// How the directions of the two entities line up (normals of planes, directions of
/// axes). `Closest` keeps whichever the components are nearer to when solving starts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Alignment {
    Aligned,
    AntiAligned,
    #[default]
    Closest,
}

/// One side of a mate, with its shape in the component's own coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Side {
    /// Index into the placements, or `None` for the assembly itself.
    pub component: Option<usize>,
    pub shape: Shape,
}

/// A mate ready for the solver.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Relation {
    pub kind: MateKind,
    pub alignment: Alignment,
    pub a: Side,
    pub b: Side,
}

/// Whether a mate holds after solving.
#[derive(Clone, Debug, PartialEq)]
pub enum MateState {
    Satisfied,
    /// The mate is left this far from holding (mm, or radians times the model size)
    /// because it conflicts with other mates.
    Unsatisfied(f64),
    /// A distance or angle mate that holds but repeats what other mates already fix, so
    /// that editing its value would break it.
    Redundant,
    /// The mate cannot apply to these entities.
    Invalid(String),
}

/// How well a component's position is defined.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Definition {
    Fixed,
    FullyDefined,
    /// Can still move in this many independent ways (1 to 6).
    UnderDefined(usize),
    /// A mate on this component could not be satisfied, or is a redundant distance or
    /// angle.
    OverDefined,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SolveReport {
    pub mates: Vec<MateState>,
    pub components: Vec<Definition>,
}

impl SolveReport {
    pub fn ok(&self) -> bool {
        self.mates.iter().all(|m| *m == MateState::Satisfied)
    }
}

fn d3(v: Vec3) -> DVec3 {
    DVec3::new(v.x, v.y, v.z)
}

fn v3(d: DVec3) -> Vec3 {
    anvil_kernel::v3(d.x, d.y, d.z)
}

/// A shape in world coordinates.
#[derive(Clone, Copy, Debug)]
enum W {
    Point(DVec3),
    Line(DVec3, DVec3),
    Plane(DVec3, DVec3),
    Circle(DVec3, DVec3, f64),
    Cylinder(DVec3, DVec3, f64),
    Sphere(DVec3, f64),
}

fn world(f: &Frame, s: &Shape) -> W {
    let p = |q: Vec3| d3(f.to_world3(q));
    let d = |q: Vec3| d3(f.dir_to_world(q)).normalize_or_zero();
    match *s {
        Shape::Point(q) => W::Point(p(q)),
        Shape::Line { origin, dir } => W::Line(p(origin), d(dir)),
        Shape::Plane { origin, normal } => W::Plane(p(origin), d(normal)),
        Shape::Circle { center, normal, radius } => W::Circle(p(center), d(normal), radius),
        Shape::Cylinder { origin, axis, radius } => W::Cylinder(p(origin), d(axis), radius),
        Shape::Sphere { center, radius } => W::Sphere(p(center), radius),
    }
}

impl W {
    /// A centre point: points, sphere and circle centres.
    fn center(&self) -> Option<DVec3> {
        match *self {
            W::Point(p) | W::Sphere(p, _) | W::Circle(p, _, _) => Some(p),
            _ => None,
        }
    }

    /// An axis: lines, cylinder axes and the axes of circles.
    fn axis(&self) -> Option<(DVec3, DVec3)> {
        match *self {
            W::Line(o, d) | W::Cylinder(o, d, _) | W::Circle(o, d, _) => Some((o, d)),
            _ => None,
        }
    }

    /// A direction for parallel, perpendicular and angle mates, and whether it is the
    /// normal of a plane (rather than the direction of a line).
    fn direction(&self) -> Option<(DVec3, bool)> {
        match *self {
            W::Line(_, d) | W::Cylinder(_, d, _) => Some((d, false)),
            W::Plane(_, n) | W::Circle(_, n, _) => Some((n, true)),
            _ => None,
        }
    }

    fn rank(&self) -> u8 {
        match self {
            W::Point(_) => 0,
            W::Line(..) => 1,
            W::Circle(..) => 2,
            W::Plane(..) => 3,
            W::Cylinder(..) => 4,
            W::Sphere(..) => 5,
        }
    }
}

/// Residuals of one mate. `sign` is the resolved alignment (+1 aligned, -1 anti-aligned)
/// and `scale` turns unitless direction errors into lengths.
struct Eqs<'a> {
    out: &'a mut Vec<f64>,
    scale: f64,
}

impl Eqs<'_> {
    fn len(&mut self, v: f64) {
        self.out.push(v);
    }
    fn len3(&mut self, v: DVec3) {
        self.out.extend([v.x, v.y, v.z]);
    }
    fn dir(&mut self, v: f64) {
        self.out.push(v * self.scale);
    }
    fn dir3(&mut self, v: DVec3) {
        self.out.extend([v.x * self.scale, v.y * self.scale, v.z * self.scale]);
    }
}

fn dist_to_line(p: DVec3, o: DVec3, d: DVec3) -> f64 {
    (p - o).cross(d).length()
}

/// Directions agree with the given sign (+1, -1, or 0 for either).
fn same_dir(e: &mut Eqs, a: DVec3, b: DVec3, sign: f64) {
    if sign == 0.0 {
        e.dir3(a.cross(b));
    } else {
        e.dir3(a - b * sign);
    }
}

/// Lines (or axes) lie on each other.
fn collinear(e: &mut Eqs, (oa, da): (DVec3, DVec3), (ob, db): (DVec3, DVec3), sign: f64) {
    same_dir(e, da, db, sign);
    e.len3((ob - oa).cross(da));
}

/// The sign a pair of directions is closest to.
fn closest_sign(a: DVec3, b: DVec3) -> f64 {
    if a.dot(b) < 0.0 {
        -1.0
    } else {
        1.0
    }
}

/// The alignment sign of a relation in its current position: +1 or -1 where directions
/// have a sense that matters, 0 where they do not.
fn resolve_sign(rel: &Relation, a: &W, b: &W) -> f64 {
    let (Some((da, _)), Some((db, _))) = (a.direction(), b.direction()) else {
        return match rel.alignment {
            Alignment::Aligned => 1.0,
            Alignment::AntiAligned => -1.0,
            Alignment::Closest => 0.0,
        };
    };
    match rel.alignment {
        Alignment::Aligned => 1.0,
        Alignment::AntiAligned => -1.0,
        Alignment::Closest => closest_sign(da, db),
    }
}

/// Which side of a plane `p` is on, +1 or -1.
fn side_of(p: DVec3, o: DVec3, n: DVec3) -> f64 {
    if (p - o).dot(n) < 0.0 {
        -1.0
    } else {
        1.0
    }
}

/// Writes the residuals of a mate between `a` and `b` (in world coordinates). `sign` is
/// the alignment and `side` the tangent side, both fixed when solving starts.
fn residuals(kind: &MateKind, a: &W, b: &W, sign: f64, side: f64, e: &mut Eqs) -> Result<(), String> {
    // Order the pair so that each case below is written once.
    let (a, b) = if a.rank() <= b.rank() { (a, b) } else { (b, a) };
    use W::*;
    let bad = || Err(format!("a {} mate cannot join these entities", kind_name(kind)));
    match kind {
        MateKind::Coincident => match (*a, *b) {
            (Point(p), Point(q)) => e.len3(p - q),
            (Point(p), Line(o, d)) => e.len3((p - o).cross(d)),
            (Point(p), Plane(o, n)) => e.len((p - o).dot(n)),
            (Point(p), Circle(c, n, r)) => {
                e.len((p - c).dot(n));
                e.len((p - c).length() - r);
            }
            (Point(p), Cylinder(o, d, r)) => e.len(dist_to_line(p, o, d) - r),
            (Point(p), Sphere(c, r)) => e.len((p - c).length() - r),
            (Line(oa, da), Line(ob, db)) => collinear(e, (oa, da), (ob, db), sign),
            (Line(o, d), Plane(po, n)) => {
                e.dir(d.dot(n));
                e.len((o - po).dot(n));
            }
            (Line(o, d), Cylinder(co, cd, r)) => {
                e.dir3(d.cross(cd));
                e.len(dist_to_line(o, co, cd) - r);
            }
            (Circle(ca, na, ra), Circle(cb, nb, rb)) if (ra - rb).abs() < 1e-9 => {
                same_dir(e, na, nb, sign);
                e.len3(ca - cb);
            }
            (Circle(c, cn, _), Plane(o, n)) => {
                same_dir(e, cn, n, sign);
                e.len((c - o).dot(n));
            }
            (Plane(oa, na), Plane(ob, nb)) => {
                same_dir(e, na, nb, sign);
                e.len((ob - oa).dot(na));
            }
            (Cylinder(oa, da, ra), Cylinder(ob, db, rb)) if (ra - rb).abs() < 1e-9 => {
                collinear(e, (oa, da), (ob, db), sign)
            }
            (Sphere(ca, ra), Sphere(cb, rb)) if (ra - rb).abs() < 1e-9 => e.len3(ca - cb),
            _ => return bad(),
        },
        MateKind::Concentric => {
            if let (Some(axa), Some(axb)) = (a.axis(), b.axis()) {
                collinear(e, axa, axb, sign);
            } else if let (Some(p), Some((o, d))) = (a.center(), b.axis()) {
                e.len3((p - o).cross(d));
            } else if let (Some(p), Some(q)) = (a.center(), b.center()) {
                e.len3(p - q);
            } else {
                return bad();
            }
        }
        MateKind::Parallel | MateKind::Perpendicular => {
            let (Some((da, pa)), Some((db, pb))) = (a.direction(), b.direction()) else { return bad() };
            // A line parallel to a plane is perpendicular to its normal, and the other
            // way round.
            let parallel_dirs = (*kind == MateKind::Parallel) == (pa == pb);
            if parallel_dirs {
                same_dir(e, da, db, sign);
            } else {
                e.dir(da.dot(db));
            }
        }
        MateKind::Angle { degrees } => {
            let (Some((da, pa)), Some((db, pb))) = (a.direction(), b.direction()) else { return bad() };
            let t = degrees.to_radians();
            let s = if sign == 0.0 { closest_sign(da, db) } else { sign };
            if pa == pb {
                e.dir(da.dot(db) * s - t.cos());
            } else {
                // The angle between a line and a plane.
                e.dir(da.dot(db) * s - t.sin());
            }
        }
        MateKind::Tangent => match (*a, *b) {
            (Plane(o, n), Cylinder(co, cd, r)) => {
                e.dir(cd.dot(n));
                e.len((co - o).dot(n) - side * r);
            }
            (Plane(o, n), Sphere(c, r)) => e.len((c - o).dot(n) - side * r),
            (Line(o, d), Cylinder(co, cd, r)) => {
                e.dir3(d.cross(cd));
                e.len(dist_to_line(o, co, cd) - r);
            }
            (Point(p), Cylinder(o, d, r)) => e.len(dist_to_line(p, o, d) - r),
            (Point(p), Sphere(c, r)) => e.len((p - c).length() - r),
            (Cylinder(oa, da, ra), Cylinder(ob, db, rb)) => {
                e.dir3(da.cross(db));
                e.len(dist_to_line(ob, oa, da) - (ra + rb));
            }
            (Cylinder(o, d, r), Sphere(c, rs)) => e.len(dist_to_line(c, o, d) - (r + rs)),
            (Sphere(ca, ra), Sphere(cb, rb)) => e.len((ca - cb).length() - (ra + rb)),
            _ => return bad(),
        },
        MateKind::Distance { value, flip } => {
            let v = *value;
            let s = if *flip { -1.0 } else { 1.0 };
            // Round entities are measured from their centres and axes.
            let reduce = |w: &W| match *w {
                Sphere(c, _) | Circle(c, _, _) => Point(c),
                Cylinder(o, d, _) => Line(o, d),
                other => other,
            };
            let (a, b) = (reduce(a), reduce(b));
            let (a, b) = if a.rank() <= b.rank() { (a, b) } else { (b, a) };
            match (a, b) {
                (Point(p), Point(q)) => e.len((p - q).length() - v),
                (Point(p), Line(o, d)) => e.len(dist_to_line(p, o, d) - v),
                (Point(p), Plane(o, n)) => e.len((p - o).dot(n) - s * v),
                (Line(oa, da), Line(ob, db)) => {
                    same_dir(e, da, db, if sign == 0.0 { 0.0 } else { sign });
                    e.len(dist_to_line(ob, oa, da) - v);
                }
                (Line(o, d), Plane(po, n)) => {
                    e.dir(d.dot(n));
                    e.len((o - po).dot(n) - s * v);
                }
                (Plane(oa, na), Plane(ob, nb)) => {
                    same_dir(e, na, nb, sign);
                    e.len((ob - oa).dot(na) - s * v);
                }
                _ => return bad(),
            }
        }
        MateKind::Lock { .. } => return bad(),
    }
    Ok(())
}

fn kind_name(k: &MateKind) -> String {
    k.label().to_lowercase()
}

/// Turns the frame by the rotation vector `w` about its origin, and moves it by `t`.
fn moved(f: &Frame, t: DVec3, w: DVec3) -> Frame {
    let angle = w.length();
    let rot = |v: Vec3| -> Vec3 {
        if angle < 1e-300 {
            return v;
        }
        let k = w / angle;
        let v = d3(v);
        v3(v * angle.cos() + k.cross(v) * angle.sin() + k * k.dot(v) * (1.0 - angle.cos()))
    };
    Frame { origin: v3(d3(f.origin) + t), x: rot(f.x), y: rot(f.y), z: rot(f.z) }
}

/// Re-orthonormalises a frame against drift.
fn orthonormal(f: &Frame) -> Frame {
    let z = d3(f.z).normalize_or(DVec3::Z);
    let x = (d3(f.x) - z * z.dot(d3(f.x))).normalize_or(z.any_orthonormal_vector());
    let y = z.cross(x);
    Frame { origin: f.origin, x: v3(x), y: v3(y), z: v3(z) }
}

/// `inner` (given in `outer`'s coordinates) in world coordinates.
pub fn compose(outer: &Frame, inner: &Frame) -> Frame {
    Frame {
        origin: outer.to_world3(inner.origin),
        x: outer.dir_to_world(inner.x),
        y: outer.dir_to_world(inner.y),
        z: outer.dir_to_world(inner.z),
    }
}

/// `f` in `of`'s coordinates.
pub fn relative(of: &Frame, f: &Frame) -> Frame {
    let local = |d: Vec3| anvil_kernel::v3(d.dot(of.x), d.dot(of.y), d.dot(of.z));
    Frame { origin: of.to_local3(f.origin), x: local(f.x), y: local(f.y), z: local(f.z) }
}

/// What a distance or angle mate between two placed entities would measure now: the
/// signed distance in millimetres (negative on the side `flip` selects) or the angle in
/// degrees. `None` for other kinds and for pairs the mate cannot join.
pub fn measure_now(
    kind: &MateKind,
    alignment: Alignment,
    a: (&Frame, &Shape),
    b: (&Frame, &Shape),
) -> Option<f64> {
    let (wa, wb) = (world(a.0, a.1), world(b.0, b.1));
    let rel = Relation {
        kind: *kind,
        alignment,
        a: Side { component: None, shape: *a.1 },
        b: Side { component: None, shape: *b.1 },
    };
    let sign = resolve_sign(&rel, &wa, &wb);
    match kind {
        MateKind::Distance { .. } => {
            let mut out = Vec::new();
            let probe = MateKind::Distance { value: 0.0, flip: false };
            residuals(&probe, &wa, &wb, sign, 1.0, &mut Eqs { out: &mut out, scale: 1.0 }).ok()?;
            out.last().copied()
        }
        MateKind::Angle { .. } => {
            let ((da, pa), (db, pb)) = (wa.direction()?, wb.direction()?);
            let s = if sign == 0.0 { closest_sign(da, db) } else { sign };
            let c = (da.dot(db) * s).clamp(-1.0, 1.0);
            Some(if pa == pb { c.acos() } else { c.asin() }.to_degrees())
        }
        _ => None,
    }
}

/// Places the components to satisfy the relations. `placements` are updated in place;
/// `fixed` components never move.
pub fn solve(placements: &mut [Frame], fixed: &[bool], relations: &[Relation]) -> SolveReport {
    let n = placements.len();
    let is_fixed = |c: Option<usize>| c.is_none_or(|i| fixed.get(i).copied().unwrap_or(true));
    let mut mates = vec![MateState::Satisfied; relations.len()];
    // Check each relation once, up front, so an impossible pairing is reported and left
    // out rather than spoiling the solve.
    let mut usable = vec![true; relations.len()];
    for (i, r) in relations.iter().enumerate() {
        let bad_index = [r.a.component, r.b.component].iter().flatten().any(|&c| c >= n);
        if bad_index {
            mates[i] = MateState::Invalid("the mate refers to a missing component".into());
            usable[i] = false;
            continue;
        }
        if r.a.component.is_some() && r.a.component == r.b.component {
            mates[i] = MateState::Invalid("both entities are on the same component".into());
            usable[i] = false;
            continue;
        }
        if !matches!(r.kind, MateKind::Lock { .. }) {
            let fa = r.a.component.map(|c| placements[c]).unwrap_or(Frame::WORLD);
            let fb = r.b.component.map(|c| placements[c]).unwrap_or(Frame::WORLD);
            let (wa, wb) = (world(&fa, &r.a.shape), world(&fb, &r.b.shape));
            let mut out = Vec::new();
            if let Err(why) = residuals(&r.kind, &wa, &wb, 1.0, 1.0, &mut Eqs { out: &mut out, scale: 1.0 }) {
                mates[i] = MateState::Invalid(why);
                usable[i] = false;
            }
        }
    }

    // Group the free components that mates join.
    let mut parent: Vec<usize> = (0..n).collect();
    fn find(p: &mut [usize], mut i: usize) -> usize {
        while p[i] != i {
            p[i] = p[p[i]];
            i = p[i];
        }
        i
    }
    for (r, _) in relations.iter().zip(&usable).filter(|(_, u)| **u) {
        if let (Some(a), Some(b)) = (r.a.component, r.b.component) {
            if !is_fixed(Some(a)) && !is_fixed(Some(b)) {
                let (ra, rb) = (find(&mut parent, a), find(&mut parent, b));
                parent[ra] = rb;
            }
        }
    }
    let mut groups: std::collections::BTreeMap<usize, Vec<usize>> = Default::default();
    for c in (0..n).filter(|&c| !is_fixed(Some(c))) {
        let root = find(&mut parent, c);
        groups.entry(root).or_default().push(c);
    }

    let mut components: Vec<Definition> = (0..n)
        .map(|c| if is_fixed(Some(c)) { Definition::Fixed } else { Definition::UnderDefined(6) })
        .collect();
    for members in groups.values() {
        let rels: Vec<usize> = (0..relations.len())
            .filter(|&i| usable[i])
            .filter(|&i| {
                let r = &relations[i];
                [r.a.component, r.b.component].iter().flatten().any(|c| members.contains(c))
            })
            .collect();
        let mut g = Group::new(placements, members, relations, &rels);
        g.pre_align(placements);
        g.solve(placements);
        let residual = g.mate_residuals(placements);
        let dofs = g.freedom(placements);
        let tol = LINEAR_TOLERANCE.max(g.scale * ANGULAR_TOLERANCE) * 10.0;
        let full_rank = g.rank(placements, None);
        for (k, &ri) in rels.iter().enumerate() {
            if residual[k] > tol {
                mates[ri] = MateState::Unsatisfied(residual[k]);
            } else if matches!(relations[ri].kind, MateKind::Distance { .. } | MateKind::Angle { .. })
                && g.rank(placements, Some(k)) == full_rank
            {
                // Dropping it frees nothing: the other mates already fix its value.
                mates[ri] = MateState::Redundant;
            }
        }
        for (k, &c) in members.iter().enumerate() {
            components[c] =
                if dofs[k] == 0 { Definition::FullyDefined } else { Definition::UnderDefined(dofs[k]) };
        }
    }
    // A component on an unsatisfied or redundant mate is over-defined.
    for (i, m) in mates.iter().enumerate() {
        if matches!(m, MateState::Unsatisfied(_) | MateState::Redundant) {
            for c in [relations[i].a.component, relations[i].b.component].into_iter().flatten() {
                if components.get(c).is_some_and(|d| *d != Definition::Fixed) {
                    components[c] = Definition::OverDefined;
                }
            }
        }
    }
    SolveReport { mates, components }
}

/// One group of components solved together.
struct Group<'a> {
    members: Vec<usize>,
    relations: Vec<&'a Relation>,
    /// The resolved alignment and tangent side of each relation.
    signs: Vec<(f64, f64)>,
    /// Model size (mm), so that rotations and directions weigh like lengths.
    scale: f64,
}

impl<'a> Group<'a> {
    fn new(placements: &[Frame], members: &[usize], all: &'a [Relation], rels: &[usize]) -> Group<'a> {
        let relations: Vec<&Relation> = rels.iter().map(|&i| &all[i]).collect();
        let mut extent: f64 = 1.0;
        let mut signs = Vec::new();
        for r in &relations {
            let fa = r.a.component.map(|c| placements[c]).unwrap_or(Frame::WORLD);
            let fb = r.b.component.map(|c| placements[c]).unwrap_or(Frame::WORLD);
            let (wa, wb) = (world(&fa, &r.a.shape), world(&fb, &r.b.shape));
            for (w, f) in [(&wa, &fa), (&wb, &fb)] {
                let p = match *w {
                    W::Point(p) | W::Line(p, _) | W::Plane(p, _) | W::Circle(p, _, _) => p,
                    W::Cylinder(p, _, _) | W::Sphere(p, _) => p,
                };
                extent = extent.max((p - d3(f.origin)).length());
            }
            let sign = resolve_sign(r, &wa, &wb);
            // The side of a plane a tangent cylinder or sphere is on.
            let (lo, hi) = if wa.rank() <= wb.rank() { (wa, wb) } else { (wb, wa) };
            let side = match (lo, hi) {
                (W::Plane(o, n), W::Cylinder(c, _, _) | W::Sphere(c, _)) => side_of(c, o, n),
                _ => 1.0,
            };
            signs.push((sign, side));
        }
        Group { members: members.to_vec(), relations, signs, scale: extent }
    }

    /// Turns components whose directions point the wrong way for an aligned or
    /// anti-aligned mate, so the iteration does not start at a saddle (exactly opposite
    /// directions give it no gradient to follow).
    fn pre_align(&self, placements: &mut [Frame]) {
        for (r, &(sign, _)) in self.relations.iter().zip(&self.signs) {
            if matches!(r.kind, MateKind::Lock { .. }) {
                continue;
            }
            let fa = r.a.component.map(|c| placements[c]).unwrap_or(Frame::WORLD);
            let fb = r.b.component.map(|c| placements[c]).unwrap_or(Frame::WORLD);
            let (Some((da, _)), Some((db, _))) =
                (world(&fa, &r.a.shape).direction(), world(&fb, &r.b.shape).direction())
            else {
                continue;
            };
            let free = |c: Option<usize>| c.filter(|c| self.members.contains(c));
            let Some((comp, from, other)) = (match (free(r.b.component), free(r.a.component)) {
                (Some(c), _) => Some((c, db, da)),
                (None, Some(c)) => Some((c, da, db)),
                (None, None) => None,
            }) else {
                continue;
            };
            if let MateKind::Angle { degrees } = r.kind {
                // At 0 or 180 degrees the angle has no gradient: try turning by the angle
                // about a few axes square to the direction and keep the best.
                let target = degrees.to_radians();
                let now = from.cross(other).length();
                if now > 1e-4 || target.sin().abs() < 1e-9 {
                    continue;
                }
                let cost = |pl: &[Frame]| self.residuals(pl).0.iter().map(|v| v * v).sum::<f64>();
                let mut best = (cost(placements), placements[comp]);
                let e1 = from.any_orthonormal_vector();
                let e2 = from.cross(e1);
                for k in 0..8 {
                    let t = k as f64 * std::f64::consts::PI / 8.0;
                    for s in [1.0, -1.0] {
                        let axis = e1 * t.cos() + e2 * t.sin();
                        let mut trial = placements.to_vec();
                        trial[comp] = orthonormal(&moved(&placements[comp], DVec3::ZERO, axis * target * s));
                        let c = cost(&trial);
                        if c < best.0 {
                            best = (c, trial[comp]);
                        }
                    }
                }
                placements[comp] = best.1;
                continue;
            }
            if sign == 0.0 || from.dot(other) * sign > -0.5 {
                continue;
            }
            let to = other * sign;
            let axis = from.cross(to);
            let w = if axis.length() < 1e-9 {
                from.any_orthonormal_vector() * std::f64::consts::PI
            } else {
                axis.normalize() * from.dot(to).clamp(-1.0, 1.0).acos()
            };
            placements[comp] = orthonormal(&moved(&placements[comp], DVec3::ZERO, w));
        }
    }

    fn frames(&self, placements: &[Frame], x: &[f64]) -> Vec<Frame> {
        let mut out = placements.to_vec();
        for (k, &c) in self.members.iter().enumerate() {
            let t = DVec3::new(x[6 * k], x[6 * k + 1], x[6 * k + 2]);
            let w = DVec3::new(x[6 * k + 3], x[6 * k + 4], x[6 * k + 5]) / self.scale;
            out[c] = moved(&placements[c], t, w);
        }
        out
    }

    /// All residuals, and the slice each relation owns.
    fn residuals(&self, frames: &[Frame]) -> (Vec<f64>, Vec<std::ops::Range<usize>>) {
        let mut out = Vec::new();
        let mut spans = Vec::new();
        for (r, &(sign, side)) in self.relations.iter().zip(&self.signs) {
            let start = out.len();
            let fa = r.a.component.map(|c| frames[c]).unwrap_or(Frame::WORLD);
            let fb = r.b.component.map(|c| frames[c]).unwrap_or(Frame::WORLD);
            let mut e = Eqs { out: &mut out, scale: self.scale };
            if let MateKind::Lock { relative: rel } = r.kind {
                let want = compose(&fa, &rel);
                e.len3(d3(fb.origin) - d3(want.origin));
                e.dir3(d3(fb.x) - d3(want.x));
                e.dir3(d3(fb.y) - d3(want.y));
            } else {
                let (wa, wb) = (world(&fa, &r.a.shape), world(&fb, &r.b.shape));
                // Pairs were checked before solving.
                let _ = residuals(&r.kind, &wa, &wb, sign, side, &mut e);
            }
            spans.push(start..out.len());
        }
        (out, spans)
    }

    fn mate_residuals(&self, placements: &[Frame]) -> Vec<f64> {
        let (r, spans) = self.residuals(placements);
        spans.iter().map(|s| r[s.clone()].iter().map(|v| v * v).sum::<f64>().sqrt()).collect()
    }

    fn jacobian(&self, placements: &[Frame], r0: &[f64]) -> Vec<Vec<f64>> {
        let nx = 6 * self.members.len();
        let h = 1e-7;
        let mut cols = Vec::with_capacity(nx);
        for j in 0..nx {
            let mut x = vec![0.0; nx];
            x[j] = h;
            let (rp, _) = self.residuals(&self.frames(placements, &x));
            x[j] = -h;
            let (rm, _) = self.residuals(&self.frames(placements, &x));
            let col: Vec<f64> = if rp.len() == r0.len() && rm.len() == r0.len() {
                rp.iter().zip(&rm).map(|(p, m)| (p - m) / (2.0 * h)).collect()
            } else {
                vec![0.0; r0.len()]
            };
            cols.push(col);
        }
        cols
    }

    fn solve(&mut self, placements: &mut [Frame]) {
        let nx = 6 * self.members.len();
        let cost = |r: &[f64]| r.iter().map(|v| v * v).sum::<f64>();
        let (mut r, _) = self.residuals(placements);
        let mut c = cost(&r);
        let mut lambda = 1e-3;
        for _ in 0..200 {
            if c < 1e-24 {
                break;
            }
            let jc = self.jacobian(placements, &r);
            // Normal equations.
            let mut a = vec![vec![0.0; nx]; nx];
            let mut g = vec![0.0; nx];
            for i in 0..nx {
                for j in i..nx {
                    let v: f64 = jc[i].iter().zip(&jc[j]).map(|(p, q)| p * q).sum();
                    a[i][j] = v;
                    a[j][i] = v;
                }
                g[i] = -jc[i].iter().zip(&r).map(|(p, q)| p * q).sum::<f64>();
            }
            // Step only along directions the mates constrain: motions they leave free get
            // no gradient, and solving for them would only amplify rounding noise.
            let (vals, vecs) = jacobi_eigen(a);
            let top = vals.iter().cloned().fold(0.0, f64::max);
            let live: Vec<(f64, f64, usize)> = (0..nx)
                .filter(|&k| vals[k] > top * NULL_RATIO)
                .map(|k| (vals[k], (0..nx).map(|i| vecs[i][k] * g[i]).sum::<f64>(), k))
                .collect();
            let mut improved = false;
            for _ in 0..12 {
                let mut step = vec![0.0; nx];
                for &(val, proj, k) in &live {
                    let t = proj / (val * (1.0 + lambda));
                    for (i, s) in step.iter_mut().enumerate() {
                        *s += t * vecs[i][k];
                    }
                }
                let trial = self.frames(placements, &step);
                let (rt, _) = self.residuals(&trial);
                let ct = cost(&rt);
                if ct.is_finite() && ct < c {
                    for &m in &self.members {
                        placements[m] = orthonormal(&trial[m]);
                    }
                    let small = step.iter().map(|v| v * v).sum::<f64>().sqrt() < 1e-14;
                    r = rt;
                    c = ct;
                    lambda = (lambda / 3.0).max(1e-12);
                    improved = !small;
                    break;
                }
                lambda *= 4.0;
            }
            if !improved {
                break;
            }
        }
    }

    /// The rank of the mate equations' Jacobian, leaving out relation `skip`.
    fn rank(&self, placements: &[Frame], skip: Option<usize>) -> usize {
        let nx = 6 * self.members.len();
        let (r, spans) = self.residuals(placements);
        let jc = self.jacobian(placements, &r);
        let rows: Vec<usize> = (0..r.len()).filter(|i| skip.is_none_or(|s| !spans[s].contains(i))).collect();
        let mut a = vec![vec![0.0; nx]; nx];
        for i in 0..nx {
            for j in i..nx {
                let v: f64 = rows.iter().map(|&k| jc[i][k] * jc[j][k]).sum();
                a[i][j] = v;
                a[j][i] = v;
            }
        }
        let (vals, _) = jacobi_eigen(a);
        let top = vals.iter().cloned().fold(0.0, f64::max).max(1.0);
        vals.iter().filter(|&&v| v > top * NULL_RATIO).count()
    }

    /// The degrees of freedom each member has left.
    fn freedom(&self, placements: &[Frame]) -> Vec<usize> {
        let nx = 6 * self.members.len();
        let (r, _) = self.residuals(placements);
        let jc = self.jacobian(placements, &r);
        let mut a = vec![vec![0.0; nx]; nx];
        for i in 0..nx {
            for j in i..nx {
                let v: f64 = jc[i].iter().zip(&jc[j]).map(|(p, q)| p * q).sum();
                a[i][j] = v;
                a[j][i] = v;
            }
        }
        let (vals, vecs) = jacobi_eigen(a);
        let top = vals.iter().cloned().fold(0.0, f64::max).max(1.0);
        let null: Vec<usize> = (0..nx).filter(|&k| vals[k] <= top * NULL_RATIO).collect();
        (0..self.members.len())
            .map(|m| {
                // The null space restricted to this member's six unknowns.
                let mut p = vec![vec![0.0; 6]; 6];
                for &k in &null {
                    for i in 0..6 {
                        for j in 0..6 {
                            p[i][j] += vecs[6 * m + i][k] * vecs[6 * m + j][k];
                        }
                    }
                }
                let (pv, _) = jacobi_eigen(p);
                pv.iter().filter(|&&v| v > 1e-6).count()
            })
            .collect()
    }
}

/// Eigenvalues and eigenvectors (as columns) of a symmetric matrix, by cyclic Jacobi
/// rotations.
fn jacobi_eigen(mut a: Vec<Vec<f64>>) -> (Vec<f64>, Vec<Vec<f64>>) {
    let n = a.len();
    let mut v = vec![vec![0.0; n]; n];
    for (i, row) in v.iter_mut().enumerate() {
        row[i] = 1.0;
    }
    for _ in 0..100 {
        let off: f64 = (0..n)
            .flat_map(|i| (0..n).filter(move |&j| j != i).map(move |j| (i, j)))
            .map(|(i, j)| a[i][j] * a[i][j])
            .sum();
        let diag: f64 = (0..n).map(|i| a[i][i] * a[i][i]).sum();
        if off <= 1e-30 * diag.max(1e-300) {
            break;
        }
        for p in 0..n {
            for q in p + 1..n {
                if a[p][q].abs() < 1e-300 {
                    continue;
                }
                let theta = (a[q][q] - a[p][p]) / (2.0 * a[p][q]);
                let t = theta.signum() / (theta.abs() + (theta * theta + 1.0).sqrt());
                let t = if theta == 0.0 { 1.0 } else { t };
                let c = 1.0 / (t * t + 1.0).sqrt();
                let s = t * c;
                for row in a.iter_mut() {
                    let (akp, akq) = (row[p], row[q]);
                    row[p] = c * akp - s * akq;
                    row[q] = s * akp + c * akq;
                }
                let (rp, rq) = (a[p].clone(), a[q].clone());
                for (k, (apk, aqk)) in rp.into_iter().zip(rq).enumerate() {
                    a[p][k] = c * apk - s * aqk;
                    a[q][k] = s * apk + c * aqk;
                }
                for row in v.iter_mut() {
                    let (vp, vq) = (row[p], row[q]);
                    row[p] = c * vp - s * vq;
                    row[q] = s * vp + c * vq;
                }
            }
        }
    }
    ((0..n).map(|i| a[i][i]).collect(), v)
}

#[cfg(test)]
mod tests {
    use super::*;
    use anvil_kernel::v3 as vec3;

    fn plane(o: Vec3, n: Vec3) -> Shape {
        Shape::Plane { origin: o, normal: n }
    }

    fn rel(
        kind: MateKind,
        alignment: Alignment,
        a: (Option<usize>, Shape),
        b: (Option<usize>, Shape),
    ) -> Relation {
        Relation {
            kind,
            alignment,
            a: Side { component: a.0, shape: a.1 },
            b: Side { component: b.0, shape: b.1 },
        }
    }

    fn at(x: f64, y: f64, z: f64) -> Frame {
        Frame { origin: vec3(x, y, z), ..Frame::WORLD }
    }

    /// A 10 mm cube's faces in its own coordinates (corner at the origin).
    fn top() -> Shape {
        plane(vec3(0.0, 0.0, 10.0), Vec3::Z)
    }
    fn bottom() -> Shape {
        plane(Vec3::ZERO, -Vec3::Z)
    }
    fn left() -> Shape {
        plane(Vec3::ZERO, -Vec3::X)
    }
    fn front() -> Shape {
        plane(Vec3::ZERO, -Vec3::Y)
    }

    #[test]
    fn stacking_a_block_on_a_fixed_block() {
        let mut p = vec![Frame::WORLD, at(3.0, -4.0, 25.0)];
        let fixed = [true, false];
        let mates = [
            rel(MateKind::Coincident, Alignment::AntiAligned, (Some(0), top()), (Some(1), bottom())),
            rel(MateKind::Coincident, Alignment::Aligned, (Some(0), left()), (Some(1), left())),
            rel(MateKind::Coincident, Alignment::Aligned, (Some(0), front()), (Some(1), front())),
        ];
        let rep = solve(&mut p, &fixed, &mates);
        assert!(rep.ok(), "{rep:?}");
        assert!((p[1].origin - vec3(0.0, 0.0, 10.0)).length() < 1e-7, "{:?}", p[1].origin);
        assert!((p[1].z - Vec3::Z).length() < 1e-7);
        assert!((p[1].x - Vec3::X).length() < 1e-7);
        assert_eq!(rep.components, vec![Definition::Fixed, Definition::FullyDefined]);
    }

    #[test]
    fn one_face_mate_leaves_three_freedoms() {
        let mut p = vec![Frame::WORLD, at(3.0, -4.0, 25.0)];
        let mates =
            [rel(MateKind::Coincident, Alignment::AntiAligned, (Some(0), top()), (Some(1), bottom()))];
        let rep = solve(&mut p, &[true, false], &mates);
        assert!(rep.ok());
        // It lands on the face without sliding sideways.
        assert!((p[1].origin - vec3(3.0, -4.0, 10.0)).length() < 1e-6, "{:?}", p[1].origin);
        assert_eq!(rep.components[1], Definition::UnderDefined(3));
    }

    #[test]
    fn a_turned_block_turns_over_to_meet_the_face() {
        // Upside down and tilted: the bottom face points up.
        let f = Frame {
            origin: vec3(0.0, 0.0, 30.0),
            x: Vec3::X,
            y: vec3(0.0, -0.8, 0.6),
            z: vec3(0.0, -0.6, -0.8),
        };
        let mut p = vec![Frame::WORLD, f];
        let mates =
            [rel(MateKind::Coincident, Alignment::AntiAligned, (Some(0), top()), (Some(1), bottom()))];
        let rep = solve(&mut p, &[true, false], &mates);
        assert!(rep.ok(), "{rep:?}");
        assert!((p[1].z - Vec3::Z).length() < 1e-7, "{:?}", p[1].z);
        assert!((p[1].origin.z - 10.0).abs() < 1e-7);
    }

    #[test]
    fn pin_in_hole_concentric_and_seated() {
        // A plate with a hole along z at (20, 10), top face at z = 5; a pin of radius 4
        // whose axis is its local z, with its shoulder face at local z = 0 facing down.
        let hole = Shape::Cylinder { origin: vec3(20.0, 10.0, 0.0), axis: Vec3::Z, radius: 4.0 };
        let plate_top = plane(vec3(0.0, 0.0, 5.0), Vec3::Z);
        let pin = Shape::Cylinder { origin: Vec3::ZERO, axis: Vec3::Z, radius: 4.0 };
        let shoulder = plane(Vec3::ZERO, -Vec3::Z);
        let mut p = vec![Frame::WORLD, at(-7.0, 30.0, 40.0)];
        let mates = [
            rel(MateKind::Concentric, Alignment::Closest, (Some(0), hole), (Some(1), pin)),
            rel(MateKind::Coincident, Alignment::AntiAligned, (Some(0), plate_top), (Some(1), shoulder)),
        ];
        let rep = solve(&mut p, &[true, false], &mates);
        assert!(rep.ok(), "{rep:?}");
        assert!((p[1].origin - vec3(20.0, 10.0, 5.0)).length() < 1e-6, "{:?}", p[1].origin);
        // Free to spin about the pin's axis only.
        assert_eq!(rep.components[1], Definition::UnderDefined(1));
    }

    #[test]
    fn distance_and_angle() {
        let mut p = vec![Frame::WORLD, at(0.0, 0.0, 50.0)];
        let mates = [
            rel(
                MateKind::Distance { value: 12.5, flip: false },
                Alignment::AntiAligned,
                (Some(0), top()),
                (Some(1), bottom()),
            ),
            rel(MateKind::Angle { degrees: 30.0 }, Alignment::Aligned, (Some(0), left()), (Some(1), left())),
        ];
        let rep = solve(&mut p, &[true, false], &mates);
        assert!(rep.ok(), "{rep:?}");
        assert!((p[1].origin.z - 22.5).abs() < 1e-7, "{:?}", p[1].origin);
        assert!((p[1].z - Vec3::Z).length() < 1e-7);
        let ang = p[1].x.dot(Vec3::X).clamp(-1.0, 1.0).acos().to_degrees();
        assert!((ang - 30.0).abs() < 1e-6, "{ang}");
        // The faces stay parallel at the distance and the angle sets the turn, so the
        // block can only slide in x and y.
        assert_eq!(rep.components[1], Definition::UnderDefined(2));
    }

    #[test]
    fn conflicting_mates_are_reported() {
        let mut p = vec![Frame::WORLD, at(0.0, 0.0, 20.0)];
        let mates = [
            rel(MateKind::Coincident, Alignment::AntiAligned, (Some(0), top()), (Some(1), bottom())),
            rel(
                MateKind::Distance { value: 5.0, flip: false },
                Alignment::AntiAligned,
                (Some(0), top()),
                (Some(1), bottom()),
            ),
        ];
        let rep = solve(&mut p, &[true, false], &mates);
        assert!(!rep.ok());
        assert!(rep.mates.iter().any(|m| matches!(m, MateState::Unsatisfied(_))));
        assert_eq!(rep.components[1], Definition::OverDefined);
    }

    #[test]
    fn a_distance_that_repeats_other_mates_over_defines() {
        // Coincident faces already fix the gap at zero; a 0 mm distance on the same faces
        // agrees but is redundant. A parallel mate repeating them is allowed.
        let mut p = vec![Frame::WORLD, at(0.0, 0.0, 20.0)];
        let mates = [
            rel(MateKind::Coincident, Alignment::AntiAligned, (Some(0), top()), (Some(1), bottom())),
            rel(MateKind::Parallel, Alignment::AntiAligned, (Some(0), top()), (Some(1), bottom())),
            rel(
                MateKind::Distance { value: 0.0, flip: false },
                Alignment::AntiAligned,
                (Some(0), top()),
                (Some(1), bottom()),
            ),
        ];
        let rep = solve(&mut p, &[true, false], &mates);
        assert_eq!(rep.mates, vec![MateState::Satisfied, MateState::Satisfied, MateState::Redundant]);
        assert_eq!(rep.components[1], Definition::OverDefined);
        assert!((p[1].origin.z - 10.0).abs() < 1e-7);
    }

    #[test]
    fn lock_keeps_relative_placement_and_chains() {
        // Component 1 sits on fixed 0; component 2 is locked to 1.
        let rel_frame = Frame { origin: vec3(5.0, 0.0, 0.0), x: Vec3::Y, y: -Vec3::X, z: Vec3::Z };
        let mut p = vec![Frame::WORLD, at(1.0, 2.0, 30.0), at(-50.0, 0.0, 0.0)];
        let mates = [
            rel(MateKind::Coincident, Alignment::AntiAligned, (Some(0), top()), (Some(1), bottom())),
            rel(MateKind::Coincident, Alignment::Aligned, (Some(0), left()), (Some(1), left())),
            rel(MateKind::Coincident, Alignment::Aligned, (Some(0), front()), (Some(1), front())),
            Relation {
                kind: MateKind::Lock { relative: rel_frame },
                alignment: Alignment::Closest,
                a: Side { component: Some(1), shape: Shape::Point(Vec3::ZERO) },
                b: Side { component: Some(2), shape: Shape::Point(Vec3::ZERO) },
            },
        ];
        let rep = solve(&mut p, &[true, false, false], &mates);
        assert!(rep.ok(), "{rep:?}");
        assert!((p[2].origin - vec3(5.0, 0.0, 10.0)).length() < 1e-6, "{:?}", p[2].origin);
        assert!((p[2].x - Vec3::Y).length() < 1e-6);
        assert_eq!(rep.components[2], Definition::FullyDefined);
    }

    #[test]
    fn tangent_cylinder_on_plane_and_sphere_on_sphere() {
        let roller = Shape::Cylinder { origin: Vec3::ZERO, axis: Vec3::X, radius: 6.0 };
        let ball = Shape::Sphere { center: Vec3::ZERO, radius: 3.0 };
        let socket = Shape::Sphere { center: vec3(0.0, 0.0, 0.0), radius: 2.0 };
        let mut p = vec![Frame::WORLD, at(0.0, 0.0, 40.0), at(10.0, 0.0, 0.0)];
        let mates = [
            rel(MateKind::Tangent, Alignment::Closest, (Some(0), top()), (Some(1), roller)),
            rel(MateKind::Tangent, Alignment::Closest, (Some(0), socket), (Some(2), ball)),
        ];
        let rep = solve(&mut p, &[true, false, false], &mates);
        assert!(rep.ok(), "{rep:?}");
        assert!((p[1].origin.z - 16.0).abs() < 1e-7, "{:?}", p[1].origin);
        assert!(p[1].dir_to_world(Vec3::X).z.abs() < 1e-7);
        assert!((p[2].origin.length() - 5.0).abs() < 1e-7, "{:?}", p[2].origin);
    }

    #[test]
    fn mating_to_the_assembly_and_invalid_pairs() {
        let mut p = vec![at(0.0, 0.0, 7.0)];
        let mates = [
            rel(
                MateKind::Coincident,
                Alignment::Aligned,
                (None, plane(Vec3::ZERO, Vec3::Z)),
                (Some(0), bottom()),
            ),
            rel(MateKind::Tangent, Alignment::Closest, (None, Shape::Point(Vec3::ZERO)), (Some(0), top())),
        ];
        let rep = solve(&mut p, &[false], &mates);
        assert_eq!(rep.mates[0], MateState::Satisfied);
        assert!(matches!(rep.mates[1], MateState::Invalid(_)));
        // Aligned: the bottom's outward normal (-z) points along +z, so the block flips.
        assert!((p[0].z + Vec3::Z).length() < 1e-7, "{:?}", p[0].z);
        assert!(p[0].origin.z.abs() < 1e-7);
    }

    #[test]
    fn measuring_before_mating() {
        let f = at(0.0, 0.0, 18.0);
        let d = measure_now(
            &MateKind::Distance { value: 0.0, flip: false },
            Alignment::AntiAligned,
            (&Frame::WORLD, &top()),
            (&f, &bottom()),
        );
        assert_eq!(d, Some(8.0));
        let below = at(0.0, 0.0, -18.0);
        let d = measure_now(
            &MateKind::Distance { value: 0.0, flip: false },
            Alignment::AntiAligned,
            (&Frame::WORLD, &top()),
            (&below, &bottom()),
        );
        assert_eq!(d, Some(-28.0));
        let turned =
            Frame { origin: Vec3::ZERO, x: vec3(0.6, 0.8, 0.0), y: vec3(-0.8, 0.6, 0.0), z: Vec3::Z };
        let a = measure_now(
            &MateKind::Angle { degrees: 0.0 },
            Alignment::Aligned,
            (&Frame::WORLD, &left()),
            (&turned, &left()),
        )
        .unwrap_or_default();
        assert!((a - 0.8f64.atan2(0.6).to_degrees()).abs() < 1e-9, "{a}");
    }

    #[test]
    fn frames_compose_and_relate() {
        let a = Frame { origin: vec3(1.0, 2.0, 3.0), x: Vec3::Y, y: -Vec3::X, z: Vec3::Z };
        let b = Frame { origin: vec3(-4.0, 0.5, 9.0), x: Vec3::Z, y: Vec3::X, z: Vec3::Y };
        let r = relative(&a, &b);
        let back = compose(&a, &r);
        assert!((back.origin - b.origin).length() < 1e-12);
        assert!((back.x - b.x).length() < 1e-12 && (back.z - b.z).length() < 1e-12);
    }
}
