//! Sketch editing tools that cut and join curves: sketch fillet and chamfer, trim,
//! corner trim, extend, split and join, and the construction toggle.
//!
//! The behaviour follows `docs/re/sketch-tools.md`. Where that specification records an
//! AnvilCAD decision (what happens to a constrained corner, to relations on a trimmed
//! entity, to a split point) the code says so next to it. Every tool either finishes its
//! edit or returns an error and leaves the sketch exactly as it was.

use crate::curve::{intersect, intersect_unbounded, Curve2};
use crate::sketch::{Constraint, EntityId, Geom, PointId, Renumbering, Sketch, SketchError, ORIGIN};
use anvil_kernel::Vec2;
use std::f64::consts::{PI, TAU};

/// Lengths below this are treated as zero.
const EPS: f64 = 1e-9;

/// Options of the Sketch Fillet tool (sketch-tools.md, "Sketch Fillet").
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FilletOptions {
    pub radius: f64,
    /// "Keep constrained corners": a corner that carries relations or dimensions stays as
    /// a virtual sharp, held on both curves.
    pub keep_constrained_corners: bool,
    /// "Dimension each fillet": every fillet gets its own radius dimension. When off, the
    /// first fillet of a run is dimensioned and the others are made `Equal` to it.
    pub dimension_each: bool,
}

impl Default for FilletOptions {
    fn default() -> Self {
        FilletOptions { radius: 5.0, keep_constrained_corners: true, dimension_each: false }
    }
}

/// What a fillet or chamfer run added.
#[derive(Clone, Debug, PartialEq)]
pub struct CornerResult {
    /// The new fillet arcs or chamfer lines, one per corner, after renumbering.
    pub entities: Vec<EntityId>,
    /// Corners kept as virtual sharps, after renumbering.
    pub virtual_sharps: Vec<PointId>,
    /// Corners that were not kept were removed; this says how indices moved.
    pub renumbering: Renumbering,
}

/// The three kinds of sketch chamfer (sketch-tools.md, "Sketch Chamfer").
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ChamferKind {
    /// "Distance-distance": along the first line, then along the second.
    Distances(f64, f64),
    /// "Distance-distance" with "Equal distance".
    Equal(f64),
    /// "Angle-distance": the distance along the first line, and the angle at that end of
    /// the chamfer between the first line (pointing back to the corner) and the chamfer.
    AngleDistance { distance: f64, degrees: f64 },
}

/// Options of the Trim tool (sketch-tools.md, "Trim Entities").
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TrimOptions {
    /// "Keep trimmed entities as construction geometry".
    pub keep_as_construction: bool,
    /// "Ignore trimming of construction geometry".
    pub ignore_construction: bool,
}

/// What a trim did.
#[derive(Clone, Debug, PartialEq)]
pub struct TrimResult {
    /// The pieces of the trimmed entity that remain, after renumbering. Empty if it was
    /// deleted.
    pub kept: Vec<EntityId>,
    /// Trimmed-off pieces kept as construction geometry, after renumbering.
    pub construction: Vec<EntityId>,
    pub renumbering: Renumbering,
}

/// One place a cutting entity crosses the entity being trimmed.
#[derive(Clone, Copy, Debug)]
struct Cut {
    t: f64,
    at: Vec2,
    by: EntityId,
    /// An end point of the cutting entity at the crossing, shared instead of a new point.
    shared: Option<PointId>,
}

fn ends_of(g: Geom) -> Option<(PointId, PointId)> {
    match g {
        Geom::Line { a, b } | Geom::Arc { a, b, .. } => Some((a, b)),
        Geom::Circle { .. } => None,
    }
}

fn set_end(g: &mut Geom, old: PointId, new: PointId) {
    match g {
        Geom::Line { a, b } | Geom::Arc { a, b, .. } => {
            if *a == old {
                *a = new;
            } else if *b == old {
                *b = new;
            }
        }
        Geom::Circle { .. } => {}
    }
}

impl Sketch {
    pub(crate) fn check_entities(&self, ids: &[EntityId]) -> Result<(), SketchError> {
        if ids.iter().all(|&e| e < self.entities.len()) {
            Ok(())
        } else {
            Err(SketchError::Missing)
        }
    }

    pub(crate) fn check_points(&self, ids: &[PointId]) -> Result<(), SketchError> {
        if ids.iter().all(|&p| p < self.points.len()) {
            Ok(())
        } else {
            Err(SketchError::Missing)
        }
    }

    pub(crate) fn curve_of(&self, e: EntityId) -> Result<Curve2, SketchError> {
        Curve2::of(self, e).ok_or(SketchError::Missing)
    }

    /// The relation that holds point `p` on entity `e`: on its infinite line or full
    /// circle.
    pub(crate) fn on_curve(&self, p: PointId, e: EntityId) -> Constraint {
        if self.entities[e].is_line() {
            Constraint::PointOnLine { p, line: e }
        } else {
            Constraint::PointOnCircle { p, entity: e }
        }
    }

    /// Sets or clears the construction flag of entities (sketch-tools.md, "Construction
    /// Geometry").
    pub fn set_construction(&mut self, ids: &[EntityId], on: bool) -> Result<(), SketchError> {
        self.check_entities(ids)?;
        for &e in ids {
            self.entities[e].construction = on;
        }
        Ok(())
    }

    /// The Construction Geometry command: makes the entities construction geometry, or
    /// ordinary geometry if they all were construction already. Returns the new state.
    pub fn toggle_construction(&mut self, ids: &[EntityId]) -> Result<bool, SketchError> {
        self.check_entities(ids)?;
        let on = !ids.iter().all(|&e| self.entities[e].construction);
        self.set_construction(ids, on)?;
        Ok(on)
    }

    /// The two curves that meet at corner `x`. Construction curves are left out when two
    /// ordinary curves meet there, so a centre rectangle's diagonals do not get in the way.
    fn corner_pair(&self, x: PointId) -> Result<(EntityId, EntityId), SketchError> {
        self.check_points(&[x])?;
        let all = self.entities_ending_at(x);
        let solid: Vec<EntityId> = all.iter().copied().filter(|&e| !self.entities[e].construction).collect();
        match (solid.as_slice(), all.as_slice()) {
            ([a, b], _) | (_, [a, b]) => Ok((*a, *b)),
            _ => Err(SketchError::Unsupported("pick a corner where exactly two curves meet")),
        }
    }

    /// Unit direction from corner `x` into entity `e` along it.
    fn direction_into(&self, e: EntityId, x: PointId) -> Option<Vec2> {
        self.leaving_direction(e, x).map(|t| -t)
    }

    /// True if the corner carries something a fillet would otherwise lose: a relation or
    /// dimension, or another curve.
    fn corner_is_constrained(&self, x: PointId, pair: (EntityId, EntityId)) -> bool {
        self.point_is_constrained(x) || self.entities_using(x).iter().any(|&e| e != pair.0 && e != pair.1)
    }

    /// The fillet circle of radius `r` tangent to entities `e1` and `e2` near their shared
    /// corner `x`: (centre, tangent point on `e1`, tangent point on `e2`).
    ///
    /// The centre lies `r` from each curve, on the offset lines (±r) and offset circles
    /// (R ± r); of those crossings, the fillet is the one inside the angle between the
    /// curves at the corner whose tangent points lie on both entities, nearest the corner
    /// (sketch-tools.md, "Geometry", Inferred).
    fn fillet_circle(
        &self,
        e1: EntityId,
        e2: EntityId,
        x: PointId,
        r: f64,
    ) -> Result<(Vec2, Vec2, Vec2), SketchError> {
        let (c1, c2) = (self.curve_of(e1)?, self.curve_of(e2)?);
        let corner = self.pos(x);
        let (t1, t2) = match (self.direction_into(e1, x), self.direction_into(e2, x)) {
            (Some(a), Some(b)) => (a, b),
            _ => return Err(SketchError::Unsupported("a fillet joins two lines or arcs")),
        };
        if t1.cross(t2).abs() < 1e-9 {
            // Already tangent, or folded back on each other.
            return Err(SketchError::Unsupported("the curves are tangent or parallel at that corner"));
        }
        let bis = (t1 + t2).normalized();
        let offsets = |c: &Curve2| -> Vec<Curve2> {
            match *c {
                Curve2::Line { a, b } => {
                    let n = (b - a).normalized().perp() * r;
                    vec![Curve2::Line { a: a + n, b: b + n }, Curve2::Line { a: a - n, b: b - n }]
                }
                Curve2::Circle { c, r: rr } | Curve2::Arc { c, r: rr, .. } => {
                    let mut v = vec![Curve2::Circle { c, r: rr + r }];
                    if rr - r > EPS {
                        v.push(Curve2::Circle { c, r: rr - r });
                    }
                    v
                }
            }
        };
        let foot = |c: &Curve2, p: Vec2| -> Vec2 {
            match *c {
                Curve2::Line { a, b } => {
                    let d = (b - a).normalized();
                    a + d * (p - a).dot(d)
                }
                Curve2::Circle { c, r } | Curve2::Arc { c, r, .. } => c + (p - c).normalized() * r,
            }
        };
        let scale = corner.length().max(r).max(1.0);
        let tol = 1e-9 * scale;
        let mut best: Option<(f64, (Vec2, Vec2, Vec2))> = None;
        for o1 in offsets(&c1) {
            for o2 in offsets(&c2) {
                for c in intersect_unbounded(&o1, &o2) {
                    if (c - corner).dot(bis) <= 0.0 {
                        continue;
                    }
                    let (p1, p2) = (foot(&c1, c), foot(&c2, c));
                    if !c1.contains(p1, tol) || !c2.contains(p2, tol) {
                        continue;
                    }
                    let cost = p1.distance(corner) + p2.distance(corner);
                    if best.is_none_or(|(b, _)| cost < b) {
                        best = Some((cost, (c, p1, p2)));
                    }
                }
            }
        }
        let (_, (c, p1, p2)) = best.ok_or(SketchError::TooLarge)?;
        // A tangent point at the far end would shrink that curve away.
        for (e, p) in [(e1, p1), (e2, p2)] {
            if let Some((a, b)) = ends_of(self.entities[e].geom) {
                let far = if a == x { b } else { a };
                if self.pos(far).distance(p) <= 1e-7 * scale {
                    return Err(SketchError::TooLarge);
                }
            }
        }
        Ok((c, p1, p2))
    }

    /// Sketch Fillet at one corner. See [`Sketch::fillet_corners`].
    pub fn fillet_corner(
        &mut self,
        corner: PointId,
        opts: FilletOptions,
    ) -> Result<CornerResult, SketchError> {
        self.fillet_corners(&[corner], opts, None)
    }

    /// Sketch Fillet at each corner (a point where two lines or arcs end), with radius
    /// `opts.radius` (sketch-tools.md, "Sketch Fillet"). Each corner gets an arc tangent
    /// to both curves, which are cut back to the tangent points and share their new end
    /// points with the arc.
    ///
    /// The first fillet gets a `Radius` dimension, and later ones `Equal` to it, unless
    /// `dimension_each` is set; `equal_to` continues an earlier run instead. A corner with
    /// relations, dimensions or other curves on it is kept as a virtual sharp held on both
    /// curves (the same `PointId`, so nothing on it is lost) when
    /// `keep_constrained_corners` is set; otherwise the corner point and whatever was on
    /// it go. If any corner cannot be filleted the sketch is left unchanged.
    pub fn fillet_corners(
        &mut self,
        corners: &[PointId],
        opts: FilletOptions,
        equal_to: Option<EntityId>,
    ) -> Result<CornerResult, SketchError> {
        if !opts.radius.is_finite() || opts.radius <= EPS {
            return Err(SketchError::Degenerate);
        }
        self.check_points(corners)?;
        if let Some(e) = equal_to {
            self.check_entities(&[e])?;
        }
        let backup = self.clone();
        let r = self.fillet_run(corners, opts, equal_to);
        if r.is_err() {
            *self = backup;
        }
        r
    }

    fn fillet_run(
        &mut self,
        corners: &[PointId],
        opts: FilletOptions,
        mut first: Option<EntityId>,
    ) -> Result<CornerResult, SketchError> {
        let mut arcs = Vec::new();
        let mut kept = Vec::new();
        let mut doomed = Vec::new();
        for &x in corners {
            let (e1, e2) = self.corner_pair(x)?;
            let (c, p1, p2) = self.fillet_circle(e1, e2, x, opts.radius)?;
            let constrained = self.corner_is_constrained(x, (e1, e2));
            let (t1, t2, pc) = (self.add_point(p1), self.add_point(p2), self.add_point(c));
            set_end(&mut self.entities[e1].geom, x, t1);
            set_end(&mut self.entities[e2].geom, x, t2);
            let arc = if (p1 - c).cross(p2 - c) > 0.0 {
                self.add_arc(pc, t1, t2)
            } else {
                self.add_arc(pc, t2, t1)
            };
            for e in [e1, e2] {
                let rel = if self.entities[e].is_line() {
                    Constraint::Tangent { line: e, entity: arc }
                } else {
                    Constraint::TangentCircles { a: e, b: arc, inside: self.circles_inside(e, arc) }
                };
                self.add_constraint(rel);
            }
            match first {
                Some(f) if !opts.dimension_each => {
                    self.add_constraint(Constraint::Equal { a: f, b: arc });
                }
                _ => {
                    self.add_constraint(Constraint::Radius { entity: arc, value: opts.radius });
                    first = first.or(Some(arc));
                }
            }
            if constrained && opts.keep_constrained_corners {
                // The virtual sharp (sketch-tools.md, "The corner point and its
                // dimensions", Decision).
                let (a, b) = (self.on_curve(x, e1), self.on_curve(x, e2));
                self.add_constraint(a);
                self.add_constraint(b);
                kept.push(x);
            } else {
                doomed.push(x);
            }
            arcs.push(arc);
        }
        let ren = self.remove_points(&doomed);
        Ok(CornerResult {
            entities: arcs,
            virtual_sharps: kept.iter().filter_map(|&p| ren.point(p)).collect(),
            renumbering: ren,
        })
    }

    /// Sketch Fillet between two curves that need not touch: both are first extended or
    /// cut back to their virtual corner (the parts picked are kept), then that corner is
    /// filleted.
    pub fn fillet_entities(
        &mut self,
        e1: EntityId,
        pick1: Vec2,
        e2: EntityId,
        pick2: Vec2,
        opts: FilletOptions,
        equal_to: Option<EntityId>,
    ) -> Result<CornerResult, SketchError> {
        let backup = self.clone();
        let r = self.trim_corner(e1, pick1, e2, pick2).and_then(|(x, ren)| {
            let eq = equal_to.and_then(|e| ren.entity(e));
            let f = self.fillet_corners(&[x], opts, eq)?;
            Ok(CornerResult { renumbering: ren.then(f.renumbering.clone()), ..f })
        });
        if r.is_err() {
            *self = backup;
        }
        r
    }

    /// Sketch Chamfer at a corner where two lines meet (sketch-tools.md, "Sketch
    /// Chamfer"). `first` names the line "Distance 1" (and the angle) is measured on; by
    /// default the line with the lower index. The corner is kept as a virtual sharp, with
    /// the chamfer's distances (and angle) as dimensions measured from it, when it is
    /// constrained and `keep_constrained_corner` is set; otherwise it is removed and the
    /// chamfer has no dimensions.
    pub fn chamfer_corner(
        &mut self,
        corner: PointId,
        first: Option<EntityId>,
        kind: ChamferKind,
        keep_constrained_corner: bool,
    ) -> Result<CornerResult, SketchError> {
        let (mut e1, mut e2) = self.corner_pair(corner)?;
        if !self.entities[e1].is_line() || !self.entities[e2].is_line() {
            return Err(SketchError::Unsupported("a sketch chamfer joins two lines"));
        }
        if e1 > e2 {
            std::mem::swap(&mut e1, &mut e2);
        }
        if first == Some(e2) {
            std::mem::swap(&mut e1, &mut e2);
        }
        let x = self.pos(corner);
        let far = |e: EntityId| match self.entities[e].geom {
            Geom::Line { a, b } => self.pos(if a == corner { b } else { a }),
            _ => x,
        };
        let (f1, f2) = (far(e1), far(e2));
        let (u1, u2) = ((f1 - x).normalized(), (f2 - x).normalized());
        let theta = u1.dot(u2).clamp(-1.0, 1.0).acos();
        if theta < 1e-9 || PI - theta < 1e-9 {
            return Err(SketchError::Unsupported("the lines are parallel at that corner"));
        }
        let (d1, d2) = match kind {
            ChamferKind::Distances(a, b) => (a, b),
            ChamferKind::Equal(d) => (d, d),
            ChamferKind::AngleDistance { distance, degrees } => {
                let alpha = degrees.to_radians();
                let opposite = PI - alpha - theta;
                if !(alpha > 0.0 && opposite > 1e-9) {
                    return Err(SketchError::Degenerate);
                }
                (distance, distance * alpha.sin() / opposite.sin())
            }
        };
        if !(d1.is_finite() && d2.is_finite()) || d1 <= EPS || d2 <= EPS {
            return Err(SketchError::Degenerate);
        }
        let scale = x.length().max(1.0);
        if d1 >= f1.distance(x) - 1e-9 * scale || d2 >= f2.distance(x) - 1e-9 * scale {
            return Err(SketchError::TooLarge);
        }
        let constrained = self.corner_is_constrained(corner, (e1, e2));
        let (p1, p2) = (self.add_point(x + u1 * d1), self.add_point(x + u2 * d2));
        set_end(&mut self.entities[e1].geom, corner, p1);
        set_end(&mut self.entities[e2].geom, corner, p2);
        let chamfer = self.add_line(p1, p2);
        let mut kept = Vec::new();
        let mut doomed = Vec::new();
        if constrained && keep_constrained_corner {
            let (a, b) = (self.on_curve(corner, e1), self.on_curve(corner, e2));
            self.add_constraint(a);
            self.add_constraint(b);
            match kind {
                // The specification asks for one distance and an Equal between the two;
                // AnvilCAD has no Equal between point distances, so both get the value.
                ChamferKind::Distances(..) | ChamferKind::Equal(_) => {
                    self.add_constraint(Constraint::Distance { p: corner, q: p1, value: d1 });
                    self.add_constraint(Constraint::Distance { p: corner, q: p2, value: d2 });
                }
                ChamferKind::AngleDistance { distance, degrees } => {
                    self.add_constraint(Constraint::Distance { p: corner, q: p1, value: distance });
                    self.add_constraint(self.chamfer_angle(e1, p1, chamfer, degrees));
                }
            }
            kept.push(corner);
        } else {
            doomed.push(corner);
        }
        let ren = self.remove_points(&doomed);
        Ok(CornerResult {
            entities: vec![chamfer],
            virtual_sharps: kept.iter().filter_map(|&p| ren.point(p)).collect(),
            renumbering: ren,
        })
    }

    /// The angle-distance chamfer's angle: at `p1`, from line `first` pointing back
    /// towards the corner, to the chamfer line. The rays are chosen so that the stored
    /// counter-clockwise angle is the one the user gave.
    fn chamfer_angle(&self, first: EntityId, p1: PointId, chamfer: EntityId, degrees: f64) -> Constraint {
        // `first` now ends at p1; its ray towards the (virtual) corner runs away from its
        // other end.
        let flip_a = matches!(self.entities[first].geom, Geom::Line { a, .. } if a == p1);
        let c = Constraint::RayAngle { a: first, b: chamfer, flip_a, flip_b: false, degrees };
        match self.measure(&c) {
            Some(v) if (v - degrees).abs() < 1e-6 => c,
            _ => Constraint::RayAngle { a: chamfer, b: first, flip_a: false, flip_b: flip_a, degrees },
        }
    }

    /// Where other entities cross entity `e`, as parameters along it (see
    /// [`Curve2::param`]), leaving out crossings at its own ends.
    fn cuts_on(&self, e: EntityId) -> Result<Vec<Cut>, SketchError> {
        let ce = self.curve_of(e)?;
        let (lo, hi) = ce.range();
        let closed = matches!(ce, Curve2::Circle { .. });
        let mut out: Vec<Cut> = Vec::new();
        for f in 0..self.entities.len() {
            if f == e {
                continue;
            }
            let cf = self.curve_of(f)?;
            let scale = 1.0_f64.max(match cf {
                Curve2::Line { a, b } => a.length().max(b.length()),
                Curve2::Circle { c, r } | Curve2::Arc { c, r, .. } => c.length() + r,
            });
            for at in intersect(&ce, &cf, 1e-9 * scale) {
                let t = ce.param(at);
                let step = match ce {
                    Curve2::Line { a, b } => 1e-9 * scale / a.distance(b).max(EPS),
                    Curve2::Circle { r, .. } | Curve2::Arc { r, .. } => 1e-9 * scale / r.max(EPS),
                };
                if !closed && (t <= lo + step || t >= hi - step) {
                    continue;
                }
                let shared = ends_of(self.entities[f].geom).and_then(|(a, b)| {
                    [a, b].into_iter().find(|&p| self.pos(p).distance(at) <= 1e-9 * scale)
                });
                if out.iter().any(|c| c.at.distance(at) <= 1e-9 * scale) {
                    continue;
                }
                out.push(Cut { t, at, by: f, shared });
            }
        }
        out.sort_by(|a, b| a.t.total_cmp(&b.t));
        Ok(out)
    }

    /// A point for a cut: the cutting entity's own end point if the cut is there,
    /// otherwise a new point held on the cutting entity.
    fn cut_point(&mut self, cut: &Cut) -> PointId {
        match cut.shared {
            Some(p) => p,
            None => {
                let p = self.add_point(cut.at);
                let rel = self.on_curve(p, cut.by);
                self.add_constraint(rel);
                p
            }
        }
    }

    /// Relations of entity `e` that also make sense for a second piece cut from it
    /// (sketch-tools.md, "What happens to relations", Decision): horizontal, vertical,
    /// parallel and perpendicular for lines; concentric and equal radius for arcs.
    fn copy_piece_relations(&mut self, e: EntityId, piece: EntityId) {
        let line = self.entities[e].is_line();
        let swap = |x: EntityId| if x == e { piece } else { x };
        let copies: Vec<Constraint> = self
            .constraints
            .iter()
            .filter_map(|c| {
                use Constraint::*;
                match *c {
                    Horizontal { line: l } if line && l == e => Some(Horizontal { line: piece }),
                    Vertical { line: l } if line && l == e => Some(Vertical { line: piece }),
                    Parallel { a, b } if line && (a == e || b == e) && a != b => {
                        Some(Parallel { a: swap(a), b: swap(b) })
                    }
                    Perpendicular { a, b } if line && (a == e || b == e) => {
                        Some(Perpendicular { a: swap(a), b: swap(b) })
                    }
                    Concentric { a, b } if !line && (a == e || b == e) => {
                        Some(Concentric { a: swap(a), b: swap(b) })
                    }
                    Equal { a, b } if !line && (a == e || b == e) => Some(Equal { a: swap(a), b: swap(b) }),
                    _ => None,
                }
            })
            .collect();
        for c in copies {
            self.add_constraint(c);
        }
    }

    /// Power trim (and Trim to closest) at `pick` on entity `e` (sketch-tools.md, "Trim
    /// Entities"): the part of `e` under the pick is cut back to the nearest crossings
    /// with other entities on either side. An entity crossed by nothing is deleted, and a
    /// circle needs two crossings to become an arc.
    ///
    /// The piece holding `e`'s first end point keeps `e`'s id and its relations. A cut end
    /// is held on the entity that cut it, or shares that entity's end point when the cut
    /// is there. When a middle part is cut out, the second piece is a new entity that
    /// copies the relations that still apply. Points left unused are removed with their
    /// relations, unless `keep_as_construction` keeps the removed parts as construction
    /// geometry.
    pub fn trim_at(&mut self, e: EntityId, pick: Vec2, opts: TrimOptions) -> Result<TrimResult, SketchError> {
        self.check_entities(&[e])?;
        if opts.ignore_construction && self.entities[e].construction {
            return Err(SketchError::Unsupported("construction geometry is not trimmed"));
        }
        let backup = self.clone();
        let r = self.trim_inner(e, pick, opts);
        if r.is_err() {
            *self = backup;
        }
        r
    }

    fn trim_inner(&mut self, e: EntityId, pick: Vec2, opts: TrimOptions) -> Result<TrimResult, SketchError> {
        let ce = self.curve_of(e)?;
        let cuts = self.cuts_on(e)?;
        let construction = self.entities[e].construction;
        if cuts.is_empty() {
            if opts.keep_as_construction {
                self.entities[e].construction = true;
                let ren = Renumbering::identity(self);
                return Ok(TrimResult { kept: vec![], construction: vec![e], renumbering: ren });
            }
            let ren = self.delete_entities_renumbered(&[e]);
            return Ok(TrimResult { kept: vec![], construction: vec![], renumbering: ren });
        }
        let tp = ce.param(pick);
        let mut kept = vec![e];
        let mut cons = Vec::new();
        let mut loose = Vec::new();
        match self.entities[e].geom {
            Geom::Circle { c, .. } => {
                if cuts.len() < 2 {
                    return Err(SketchError::NoIntersection);
                }
                let tp = tp.rem_euclid(TAU);
                let hi = cuts.iter().find(|k| k.t > tp).unwrap_or(&cuts[0]);
                let lo = cuts.iter().rev().find(|k| k.t < tp).unwrap_or(&cuts[cuts.len() - 1]);
                let (hi, lo) = (*hi, *lo);
                let (ph, pl) = (self.cut_point(&hi), self.cut_point(&lo));
                self.entities[e].geom = Geom::Arc { c, a: ph, b: pl };
                if opts.keep_as_construction {
                    let piece = self.add_arc(c, pl, ph);
                    self.entities[piece].construction = true;
                    cons.push(piece);
                }
            }
            g => {
                let Some((a, b)) = ends_of(g) else { return Err(SketchError::Missing) };
                let lo = cuts.iter().rev().find(|k| k.t < tp).copied();
                let hi = cuts.iter().find(|k| k.t > tp).copied();
                let piece = |sk: &mut Sketch, p: PointId, q: PointId| -> EntityId {
                    match g {
                        Geom::Arc { c, .. } => sk.add_arc(c, p, q),
                        _ => sk.add_line(p, q),
                    }
                };
                match (lo, hi) {
                    (None, None) => return Err(SketchError::NoIntersection),
                    (None, Some(h)) => {
                        let ph = self.cut_point(&h);
                        set_end(&mut self.entities[e].geom, a, ph);
                        if opts.keep_as_construction {
                            let k = piece(self, a, ph);
                            self.entities[k].construction = true;
                            cons.push(k);
                        } else {
                            loose.push(a);
                        }
                    }
                    (Some(l), None) => {
                        let pl = self.cut_point(&l);
                        set_end(&mut self.entities[e].geom, b, pl);
                        if opts.keep_as_construction {
                            let k = piece(self, pl, b);
                            self.entities[k].construction = true;
                            cons.push(k);
                        } else {
                            loose.push(b);
                        }
                    }
                    (Some(l), Some(h)) => {
                        let (pl, ph) = (self.cut_point(&l), self.cut_point(&h));
                        set_end(&mut self.entities[e].geom, b, pl);
                        let second = piece(self, ph, b);
                        self.entities[second].construction = construction;
                        self.copy_piece_relations(e, second);
                        kept.push(second);
                        if opts.keep_as_construction {
                            let k = piece(self, pl, ph);
                            self.entities[k].construction = true;
                            cons.push(k);
                        }
                    }
                }
            }
        }
        // Old end points that nothing uses any more go, with their relations.
        loose.retain(|&p| p != ORIGIN && self.entities_using(p).is_empty());
        let ren = self.remove_points(&loose);
        Ok(TrimResult {
            kept: kept.iter().filter_map(|&k| ren.entity(k)).collect(),
            construction: cons.iter().filter_map(|&k| ren.entity(k)).collect(),
            renumbering: ren,
        })
    }

    /// Trim away inside or outside (sketch-tools.md, "Trim Entities"): two boundary
    /// curves, counted as unlimited, and lines or arcs to trim, each of which must cross
    /// each boundary exactly once (or neither, and is then left alone). Inside removes the
    /// part between the boundaries; outside keeps only that part. Cut ends are held on the
    /// boundaries. The case where a curve crosses one boundary only is refused.
    pub fn trim_away(
        &mut self,
        bounds: [EntityId; 2],
        targets: &[EntityId],
        inside: bool,
    ) -> Result<TrimResult, SketchError> {
        self.check_entities(&bounds)?;
        self.check_entities(targets)?;
        let backup = self.clone();
        let r = self.trim_away_inner(bounds, targets, inside);
        if r.is_err() {
            *self = backup;
        }
        r
    }

    fn trim_away_inner(
        &mut self,
        bounds: [EntityId; 2],
        targets: &[EntityId],
        inside: bool,
    ) -> Result<TrimResult, SketchError> {
        let mut kept = Vec::new();
        let mut loose = Vec::new();
        for &e in targets {
            if bounds.contains(&e) {
                continue;
            }
            let ce = self.curve_of(e)?;
            let Some((a, b)) = ends_of(self.entities[e].geom) else {
                return Err(SketchError::Unsupported("only lines and arcs are trimmed this way"));
            };
            let mut cuts = Vec::new();
            for by in bounds {
                let cb = self.curve_of(by)?;
                let scale = 1.0_f64.max(self.pos(a).length().max(self.pos(b).length()));
                let hits: Vec<Vec2> = intersect_unbounded(&ce, &cb)
                    .into_iter()
                    .filter(|p| ce.contains(*p, 1e-9 * scale))
                    .collect();
                match hits.as_slice() {
                    [] => {}
                    [at] => {
                        let shared = ends_of(self.entities[by].geom).and_then(|(p, q)| {
                            [p, q].into_iter().find(|&k| self.pos(k).distance(*at) <= 1e-9 * scale)
                        });
                        cuts.push(Cut { t: ce.param(*at), at: *at, by, shared });
                    }
                    _ => return Err(SketchError::Unsupported("each curve must cross each boundary once")),
                }
            }
            match cuts.len() {
                0 => continue,
                1 => return Err(SketchError::Unsupported("each curve must cross both boundaries")),
                _ => {}
            }
            cuts.sort_by(|x, y| x.t.total_cmp(&y.t));
            let (lo, hi) = (cuts[0], cuts[1]);
            let (pl, ph) = (self.cut_point(&lo), self.cut_point(&hi));
            if inside {
                set_end(&mut self.entities[e].geom, b, pl);
                let second = match self.entities[e].geom {
                    Geom::Arc { c, .. } => self.add_arc(c, ph, b),
                    _ => self.add_line(ph, b),
                };
                self.entities[second].construction = self.entities[e].construction;
                self.copy_piece_relations(e, second);
                kept.extend([e, second]);
            } else {
                set_end(&mut self.entities[e].geom, a, pl);
                set_end(&mut self.entities[e].geom, b, ph);
                loose.extend([a, b]);
                kept.push(e);
            }
        }
        loose.retain(|&p| p != ORIGIN && self.entities_using(p).is_empty());
        let ren = self.remove_points(&loose);
        Ok(TrimResult {
            kept: kept.iter().filter_map(|&k| ren.entity(k)).collect(),
            construction: Vec::new(),
            renumbering: ren,
        })
    }

    /// Which end of open entity `e` a pick on the far side of `x` from it moves: the end
    /// beyond `x` as seen from the pick, so the picked part is kept.
    fn moving_end(&self, e: EntityId, pick: Vec2, x: Vec2) -> Result<PointId, SketchError> {
        let ce = self.curve_of(e)?;
        let (a, b) =
            ends_of(self.entities[e].geom).ok_or(SketchError::Unsupported("circles have no ends to move"))?;
        let (tp, tx) = (ce.param(pick), ce.param(x));
        let (_, hi) = ce.range();
        // Past an end, that end moves; otherwise the end on the far side of x from the pick.
        Ok(if tx <= 0.0 || (tx < hi && tp >= tx) { a } else { b })
    }

    /// A point that only `e` uses at its end `p`: `p` itself if nothing else uses it,
    /// otherwise a new point at the same place, put in `e`.
    fn own_end(&mut self, e: EntityId, p: PointId) -> PointId {
        if p != ORIGIN && self.entities_using(p) == [e] {
            return p;
        }
        let q = self.add_point(self.pos(p));
        set_end(&mut self.entities[e].geom, p, q);
        q
    }

    /// Corner trim (sketch-tools.md, "Trim Entities", Corner): extends or cuts back two
    /// lines or arcs until they meet, keeping the picked part of each. The two ends become
    /// one point, which is returned (after renumbering).
    pub fn trim_corner(
        &mut self,
        e1: EntityId,
        pick1: Vec2,
        e2: EntityId,
        pick2: Vec2,
    ) -> Result<(PointId, Renumbering), SketchError> {
        self.check_entities(&[e1, e2])?;
        if e1 == e2 {
            return Err(SketchError::Unsupported("pick two different curves"));
        }
        let (c1, c2) = (self.curve_of(e1)?, self.curve_of(e2)?);
        let x = intersect_unbounded(&c1, &c2)
            .into_iter()
            .min_by(|p, q| {
                (p.distance(pick1) + p.distance(pick2)).total_cmp(&(q.distance(pick1) + q.distance(pick2)))
            })
            .ok_or(SketchError::NoIntersection)?;
        let (m1, m2) = (self.moving_end(e1, pick1, x)?, self.moving_end(e2, pick2, x)?);
        let backup = self.clone();
        let p1 = self.own_end(e1, m1);
        let p2 = self.own_end(e2, m2);
        for (e, p) in [(e1, p1), (e2, p2)] {
            self.points[p].x = x.x;
            self.points[p].y = x.y;
            if let Some((a, b)) = ends_of(self.entities[e].geom) {
                if self.pos(a).distance(self.pos(b)) <= 1e-9 * x.length().max(1.0) {
                    *self = backup;
                    return Err(SketchError::Degenerate);
                }
            }
        }
        let ren = self.merge_points(p1, p2)?;
        let corner = ren.point(p1).or(ren.point(p2)).ok_or(SketchError::Missing)?;
        Ok((corner, ren))
    }

    /// Extend Entities (sketch-tools.md, "Extend Entities"): lengthens line or arc `e` at
    /// the end nearer `pick` to the nearest entity in the way. The new end is held on that
    /// entity, or shares its end point. Returns the moved end point.
    pub fn extend(&mut self, e: EntityId, pick: Vec2) -> Result<(PointId, Renumbering), SketchError> {
        self.check_entities(&[e])?;
        let ce = self.curve_of(e)?;
        let (a, b) =
            ends_of(self.entities[e].geom).ok_or(SketchError::Unsupported("a circle cannot be extended"))?;
        let (_, hi) = ce.range();
        let at_b = ce.param(pick) >= 0.5 * hi;
        let scale = 1.0_f64.max(self.pos(a).length().max(self.pos(b).length()));
        let mut best: Option<(f64, Vec2, EntityId)> = None;
        for f in 0..self.entities.len() {
            if f == e {
                continue;
            }
            let cf = self.curve_of(f)?;
            for x in intersect_unbounded(&ce, &cf) {
                if !cf.contains(x, 1e-9 * scale) {
                    continue;
                }
                // How far past the end the crossing lies, along the curve.
                let beyond = match ce {
                    Curve2::Line { a: pa, b: pb } => {
                        let t = ce.param(x);
                        let len = pa.distance(pb);
                        if at_b {
                            (t - 1.0) * len
                        } else {
                            -t * len
                        }
                    }
                    Curve2::Arc { c, r, start, sweep } => {
                        let ang = (x - c).angle();
                        let d = if at_b {
                            (ang - start - sweep).rem_euclid(TAU)
                        } else {
                            (start - ang).rem_euclid(TAU)
                        };
                        if d > TAU - sweep - 1e-12 {
                            -1.0
                        } else {
                            d * r
                        }
                    }
                    Curve2::Circle { .. } => -1.0,
                };
                if beyond > 1e-9 * scale && best.is_none_or(|(d, _, _)| beyond < d) {
                    best = Some((beyond, x, f));
                }
            }
        }
        let (_, x, by) = best.ok_or(SketchError::NoIntersection)?;
        let end = if at_b { b } else { a };
        let p = self.own_end(e, end);
        self.points[p].x = x.x;
        self.points[p].y = x.y;
        let shared = ends_of(self.entities[by].geom)
            .and_then(|(fa, fb)| [fa, fb].into_iter().find(|&q| self.pos(q).distance(x) <= 1e-9 * scale));
        match shared {
            Some(q) => {
                let ren = self.merge_points(q, p)?;
                let at = ren.point(q).ok_or(SketchError::Missing)?;
                Ok((at, ren))
            }
            None => {
                let rel = self.on_curve(p, by);
                self.add_constraint(rel);
                Ok((p, Renumbering::identity(self)))
            }
        }
    }

    /// Split Entities (sketch-tools.md, "Split Entities"): splits line or arc `e` at the
    /// point nearest `at` into two pieces sharing a new split point. Split lines stay
    /// `Collinear` and split arcs share their centre and stay `Coradial`, so deleting the
    /// split point can join them again ([`Sketch::join_at`]). Those relations stand in for
    /// copying the original's orientation relations to the second piece. Returns the split
    /// point and the new piece.
    pub fn split_at(&mut self, e: EntityId, at: Vec2) -> Result<(PointId, EntityId), SketchError> {
        self.check_entities(&[e])?;
        let ce = self.curve_of(e)?;
        let (lo, hi) = ce.range();
        let t = ce.param(at);
        let slack = match ce {
            Curve2::Line { a, b } => 1e-9 / a.distance(b).max(EPS),
            _ => 1e-9,
        };
        let construction = self.entities[e].construction;
        match self.entities[e].geom {
            Geom::Line { b, .. } if t > lo + slack && t < hi - slack => {
                let p = self.add_point(ce.at(t));
                set_end(&mut self.entities[e].geom, b, p);
                let piece = self.add_line(p, b);
                self.entities[piece].construction = construction;
                self.add_constraint(Constraint::Collinear { a: e, b: piece });
                Ok((p, piece))
            }
            Geom::Arc { c, b, .. } if t > lo + slack && t < hi - slack => {
                let p = self.add_point(ce.at(t));
                set_end(&mut self.entities[e].geom, b, p);
                let piece = self.add_arc(c, p, b);
                self.entities[piece].construction = construction;
                self.add_constraint(Constraint::Coradial { a: e, b: piece });
                Ok((p, piece))
            }
            Geom::Circle { .. } => Err(SketchError::Unsupported("a circle needs two split points")),
            _ => Err(SketchError::Degenerate),
        }
    }

    /// Splits circle `e` at the points nearest `p1` and `p2` into two coradial arcs. The
    /// circle's id goes to the arc from the first point counter-clockwise to the second.
    pub fn split_circle(
        &mut self,
        e: EntityId,
        p1: Vec2,
        p2: Vec2,
    ) -> Result<(PointId, PointId, EntityId), SketchError> {
        self.check_entities(&[e])?;
        let Geom::Circle { c, r } = self.entities[e].geom else {
            return Err(SketchError::Unsupported("split a line or arc with one point"));
        };
        let o = self.pos(c);
        let (u1, u2) = ((p1 - o).normalized(), (p2 - o).normalized());
        if u1.length() < 0.5 || u2.length() < 0.5 || u1.distance(u2) <= 1e-9 {
            return Err(SketchError::Degenerate);
        }
        let (a, b) = (self.add_point(o + u1 * r), self.add_point(o + u2 * r));
        self.entities[e].geom = Geom::Arc { c, a, b };
        let piece = self.add_arc(c, b, a);
        self.entities[piece].construction = self.entities[e].construction;
        self.add_constraint(Constraint::Coradial { a: e, b: piece });
        Ok((a, b, piece))
    }

    /// True if point `p` is a split point [`Sketch::join_at`] can remove.
    pub fn can_join_at(&self, p: PointId) -> bool {
        self.join_pair(p).is_some()
    }

    fn join_pair(&self, p: PointId) -> Option<(EntityId, EntityId)> {
        if p >= self.points.len() || self.entities_using(p).len() != 2 {
            return None;
        }
        let [e1, e2] = self.entities_ending_at(p)[..] else { return None };
        let (g1, g2) = (self.entities[e1].geom, self.entities[e2].geom);
        let related = self.constraints.iter().any(|c| {
            matches!(*c, Constraint::Collinear { a, b } | Constraint::Coradial { a, b }
                if (a == e1 && b == e2) || (a == e2 && b == e1))
        });
        let fits = match (g1, g2) {
            (Geom::Line { a, b }, Geom::Line { a: c, b: d }) => {
                // Collinear, running away from p in opposite directions.
                let at = self.pos(p);
                let u1 = self.pos(if a == p { b } else { a }) - at;
                let u2 = self.pos(if c == p { d } else { c }) - at;
                let scale = u1.length().max(u2.length()).max(1.0);
                u1.cross(u2).abs() <= 1e-9 * scale * scale && u1.dot(u2) < 0.0
            }
            (Geom::Arc { c: c1, a: a1, .. }, Geom::Arc { c: c2, a: a2, .. }) => {
                c1 == c2 && (a1 == p) != (a2 == p)
            }
            _ => false,
        };
        (related || fits).then_some((e1, e2))
    }

    /// Deletes split point `p`, joining the two collinear lines or coradial arcs that meet
    /// there back into one (sketch-tools.md, "Split Entities", Decision). Two arcs whose
    /// other ends meet become a circle again. Returns the joined entity.
    pub fn join_at(&mut self, p: PointId) -> Result<(EntityId, Renumbering), SketchError> {
        let (e1, e2) = self
            .join_pair(p)
            .ok_or(SketchError::Unsupported("that point does not join two pieces of one curve"))?;
        let (keep, gone) = match (self.entities[e1].geom, self.entities[e2].geom) {
            // The arc that ends at p keeps its start and takes the other's end.
            (Geom::Arc { b, .. }, Geom::Arc { .. }) if b == p => (e1, e2),
            (Geom::Arc { .. }, Geom::Arc { .. }) => (e2, e1),
            _ => (e1.min(e2), e1.max(e2)),
        };
        let other_end = |g: Geom| ends_of(g).map(|(a, b)| if a == p { b } else { a });
        let far = other_end(self.entities[gone].geom).ok_or(SketchError::Missing)?;
        set_end(&mut self.entities[keep].geom, p, far);
        if let Geom::Arc { c, a, b } = self.entities[keep].geom {
            if a == b {
                let r = self.pos(a).distance(self.pos(c));
                self.entities[keep].geom = Geom::Circle { c, r };
            }
        }
        // The split point goes with the second piece, which alone still uses it.
        let ren = self.delete_entities_renumbered(&[gone]);
        let joined = ren.entity(keep).ok_or(SketchError::Missing)?;
        Ok((joined, ren))
    }
}
