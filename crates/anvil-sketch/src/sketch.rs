//! Sketch data model and editing operations.

use anvil_kernel::{v2, Vec2};
use serde::{Deserialize, Serialize};

pub type PointId = usize;
pub type EntityId = usize;

/// Index of the sketch origin, which every sketch has and which is always fixed.
pub const ORIGIN: PointId = 0;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub fn pos(&self) -> Vec2 {
        v2(self.x, self.y)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Geom {
    Line {
        a: PointId,
        b: PointId,
    },
    Circle {
        c: PointId,
        r: f64,
    },
    /// Counter-clockwise arc from `a` to `b` around center `c`.
    Arc {
        c: PointId,
        a: PointId,
        b: PointId,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Entity {
    pub geom: Geom,
    #[serde(default)]
    pub construction: bool,
}

impl Entity {
    pub fn points(&self) -> Vec<PointId> {
        match self.geom {
            Geom::Line { a, b } => vec![a, b],
            Geom::Circle { c, .. } => vec![c],
            Geom::Arc { c, a, b } => vec![c, a, b],
        }
    }
    pub fn is_line(&self) -> bool {
        matches!(self.geom, Geom::Line { .. })
    }
    pub fn is_round(&self) -> bool {
        !self.is_line()
    }
}

/// Geometric and dimensional constraints. Dimensional values are in millimetres or degrees.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Constraint {
    Fix {
        p: PointId,
        x: f64,
        y: f64,
    },
    Coincident {
        p: PointId,
        q: PointId,
    },
    Horizontal {
        line: EntityId,
    },
    Vertical {
        line: EntityId,
    },
    HorizontalPoints {
        p: PointId,
        q: PointId,
    },
    VerticalPoints {
        p: PointId,
        q: PointId,
    },
    Distance {
        p: PointId,
        q: PointId,
        value: f64,
    },
    /// Signed: x(q) - x(p) = value.
    HDistance {
        p: PointId,
        q: PointId,
        value: f64,
    },
    /// Signed: y(q) - y(p) = value.
    VDistance {
        p: PointId,
        q: PointId,
        value: f64,
    },
    Length {
        line: EntityId,
        value: f64,
    },
    Radius {
        entity: EntityId,
        value: f64,
    },
    Diameter {
        entity: EntityId,
        value: f64,
    },
    Parallel {
        a: EntityId,
        b: EntityId,
    },
    Perpendicular {
        a: EntityId,
        b: EntityId,
    },
    /// Equal length (lines) or equal radius (circles/arcs).
    Equal {
        a: EntityId,
        b: EntityId,
    },
    /// Angle from line `a` to line `b`, counter-clockwise, in degrees.
    Angle {
        a: EntityId,
        b: EntityId,
        degrees: f64,
    },
    PointOnLine {
        p: PointId,
        line: EntityId,
    },
    PointOnCircle {
        p: PointId,
        entity: EntityId,
    },
    Midpoint {
        p: PointId,
        line: EntityId,
    },
    Tangent {
        line: EntityId,
        entity: EntityId,
    },
    Concentric {
        a: EntityId,
        b: EntityId,
    },
    /// Perpendicular distance from a point to the infinite line through `line`.
    PointLineDistance {
        p: PointId,
        line: EntityId,
        value: f64,
    },
    /// Length measured along an arc, from its start point to its end point.
    ArcLength {
        entity: EntityId,
        value: f64,
    },
    /// Counter-clockwise angle, in degrees, from one ray to another. The rays run along
    /// lines `a` and `b`, each from its first point to its second, or the other way when
    /// `flip_a` or `flip_b` is set. Choosing the rays chooses which of the four angles
    /// between two crossing lines is dimensioned.
    RayAngle {
        a: EntityId,
        b: EntityId,
        flip_a: bool,
        flip_b: bool,
        degrees: f64,
    },
    /// Twice the perpendicular distance from a point to the line through `axis`: the
    /// diameter a revolve about that axis gives the point.
    AxisDiameter {
        p: PointId,
        axis: EntityId,
        value: f64,
    },
}

impl Constraint {
    /// The driving value of a dimension, if this is one.
    pub fn value(&self) -> Option<f64> {
        use Constraint::*;
        match *self {
            Distance { value, .. }
            | HDistance { value, .. }
            | VDistance { value, .. }
            | Length { value, .. }
            | Radius { value, .. }
            | Diameter { value, .. }
            | PointLineDistance { value, .. }
            | ArcLength { value, .. }
            | AxisDiameter { value, .. } => Some(value),
            Angle { degrees, .. } | RayAngle { degrees, .. } => Some(degrees),
            _ => None,
        }
    }

    /// Sets a dimension's value. Signed distances keep their direction.
    pub fn set_value(&mut self, v: f64) {
        use Constraint::*;
        match self {
            HDistance { value, .. } | VDistance { value, .. } => {
                *value = if *value < 0.0 { -v.abs() } else { v.abs() }
            }
            Distance { value, .. }
            | Length { value, .. }
            | Radius { value, .. }
            | Diameter { value, .. }
            | PointLineDistance { value, .. }
            | ArcLength { value, .. }
            | AxisDiameter { value, .. } => *value = v,
            Angle { degrees, .. } | RayAngle { degrees, .. } => *degrees = v,
            _ => {}
        }
    }

    /// Stores a measured value exactly as given, sign included. Used for reference
    /// dimensions, whose value follows the geometry.
    pub fn set_measured(&mut self, v: f64) {
        use Constraint::*;
        match self {
            HDistance { value, .. } | VDistance { value, .. } => *value = v,
            _ => self.set_value(v),
        }
    }

    pub fn is_dimension(&self) -> bool {
        self.value().is_some()
    }

    pub fn label(&self) -> &'static str {
        use Constraint::*;
        match self {
            Fix { .. } => "Fixed",
            Coincident { .. } => "Coincident",
            Horizontal { .. } | HorizontalPoints { .. } => "Horizontal",
            Vertical { .. } | VerticalPoints { .. } => "Vertical",
            Distance { .. } => "Distance",
            HDistance { .. } => "Horizontal distance",
            VDistance { .. } => "Vertical distance",
            Length { .. } => "Length",
            Radius { .. } => "Radius",
            Diameter { .. } => "Diameter",
            Parallel { .. } => "Parallel",
            Perpendicular { .. } => "Perpendicular",
            Equal { .. } => "Equal",
            Angle { .. } => "Angle",
            PointOnLine { .. } => "Point on line",
            PointOnCircle { .. } => "Point on circle",
            Midpoint { .. } => "Midpoint",
            Tangent { .. } => "Tangent",
            Concentric { .. } => "Concentric",
            PointLineDistance { .. } => "Point-line distance",
            ArcLength { .. } => "Arc length",
            RayAngle { .. } => "Angle",
            AxisDiameter { .. } => "Diameter about axis",
        }
    }

    pub fn points(&self) -> Vec<PointId> {
        use Constraint::*;
        match *self {
            Fix { p, .. } => vec![p],
            Coincident { p, q }
            | HorizontalPoints { p, q }
            | VerticalPoints { p, q }
            | Distance { p, q, .. }
            | HDistance { p, q, .. }
            | VDistance { p, q, .. } => vec![p, q],
            PointOnLine { p, .. }
            | PointOnCircle { p, .. }
            | Midpoint { p, .. }
            | PointLineDistance { p, .. }
            | AxisDiameter { p, .. } => vec![p],
            _ => vec![],
        }
    }

    pub fn entities(&self) -> Vec<EntityId> {
        use Constraint::*;
        match *self {
            Horizontal { line } | Vertical { line } | Length { line, .. } => vec![line],
            Radius { entity, .. } | Diameter { entity, .. } | ArcLength { entity, .. } => vec![entity],
            Parallel { a, b }
            | Perpendicular { a, b }
            | Equal { a, b }
            | Angle { a, b, .. }
            | RayAngle { a, b, .. }
            | Concentric { a, b } => vec![a, b],
            PointOnLine { line, .. } | Midpoint { line, .. } | PointLineDistance { line, .. } => vec![line],
            AxisDiameter { axis, .. } => vec![axis],
            PointOnCircle { entity, .. } => vec![entity],
            Tangent { line, entity } => vec![line, entity],
            _ => vec![],
        }
    }

    fn remap(&mut self, pm: &dyn Fn(PointId) -> PointId, em: &dyn Fn(EntityId) -> EntityId) {
        use Constraint::*;
        match self {
            Fix { p, .. } => *p = pm(*p),
            Coincident { p, q }
            | HorizontalPoints { p, q }
            | VerticalPoints { p, q }
            | Distance { p, q, .. }
            | HDistance { p, q, .. }
            | VDistance { p, q, .. } => {
                *p = pm(*p);
                *q = pm(*q);
            }
            Horizontal { line } | Vertical { line } | Length { line, .. } => *line = em(*line),
            Radius { entity, .. } | Diameter { entity, .. } | ArcLength { entity, .. } => {
                *entity = em(*entity)
            }
            Parallel { a, b }
            | Perpendicular { a, b }
            | Equal { a, b }
            | Angle { a, b, .. }
            | RayAngle { a, b, .. }
            | Concentric { a, b } => {
                *a = em(*a);
                *b = em(*b);
            }
            PointOnLine { p, line } | Midpoint { p, line } | PointLineDistance { p, line, .. } => {
                *p = pm(*p);
                *line = em(*line);
            }
            AxisDiameter { p, axis, .. } => {
                *p = pm(*p);
                *axis = em(*axis);
            }
            PointOnCircle { p, entity } => {
                *p = pm(*p);
                *entity = em(*entity);
            }
            Tangent { line, entity } => {
                *line = em(*line);
                *entity = em(*entity);
            }
        }
    }
}

/// How one dimension is shown and whether it drives the sketch.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct DimAnnotation {
    /// Index of the dimension in [`Sketch::constraints`].
    pub constraint: usize,
    /// Where the value is drawn, as an offset from the dimension's default anchor (see
    /// [`crate::dimension::anchor`]), so the label follows the geometry it measures.
    #[serde(default)]
    pub offset: Vec2,
    /// A reference dimension reports the geometry and never drives it. The solver skips
    /// it and [`Sketch::solve`] refreshes its value from the solved geometry.
    #[serde(default)]
    pub reference: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Sketch {
    pub points: Vec<Point>,
    pub entities: Vec<Entity>,
    pub constraints: Vec<Constraint>,
    /// Placement and role of dimensions; a dimension without an entry is driving and
    /// drawn at its default anchor.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub annotations: Vec<DimAnnotation>,
}

impl Default for Sketch {
    fn default() -> Self {
        Sketch::new()
    }
}

impl Sketch {
    pub fn new() -> Sketch {
        Sketch {
            points: vec![Point { x: 0.0, y: 0.0 }],
            entities: Vec::new(),
            constraints: vec![Constraint::Fix { p: ORIGIN, x: 0.0, y: 0.0 }],
            annotations: Vec::new(),
        }
    }

    pub fn pos(&self, p: PointId) -> Vec2 {
        self.points[p].pos()
    }

    pub fn add_point(&mut self, p: Vec2) -> PointId {
        self.points.push(Point { x: p.x, y: p.y });
        self.points.len() - 1
    }

    pub fn add_entity(&mut self, geom: Geom) -> EntityId {
        self.entities.push(Entity { geom, construction: false });
        self.entities.len() - 1
    }

    pub fn add_line(&mut self, a: PointId, b: PointId) -> EntityId {
        self.add_entity(Geom::Line { a, b })
    }

    pub fn add_circle(&mut self, c: PointId, r: f64) -> EntityId {
        self.add_entity(Geom::Circle { c, r })
    }

    pub fn add_arc(&mut self, c: PointId, a: PointId, b: PointId) -> EntityId {
        self.add_entity(Geom::Arc { c, a, b })
    }

    pub fn add_constraint(&mut self, c: Constraint) -> usize {
        self.constraints.push(c);
        self.constraints.len() - 1
    }

    /// Adds an axis-aligned rectangle from two corners with horizontal/vertical constraints.
    /// Returns the four line entities (bottom, right, top, left).
    pub fn add_rectangle(&mut self, p0: Vec2, p1: Vec2) -> [EntityId; 4] {
        let (lo, hi) = (v2(p0.x.min(p1.x), p0.y.min(p1.y)), v2(p0.x.max(p1.x), p0.y.max(p1.y)));
        let a = self.add_point(lo);
        let b = self.add_point(v2(hi.x, lo.y));
        let c = self.add_point(hi);
        let d = self.add_point(v2(lo.x, hi.y));
        let l = [self.add_line(a, b), self.add_line(b, c), self.add_line(c, d), self.add_line(d, a)];
        self.add_constraint(Constraint::Horizontal { line: l[0] });
        self.add_constraint(Constraint::Vertical { line: l[1] });
        self.add_constraint(Constraint::Horizontal { line: l[2] });
        self.add_constraint(Constraint::Vertical { line: l[3] });
        l
    }

    pub fn radius(&self, e: EntityId) -> f64 {
        match self.entities[e].geom {
            Geom::Circle { r, .. } => r,
            Geom::Arc { c, a, .. } => self.pos(a).distance(self.pos(c)),
            Geom::Line { .. } => 0.0,
        }
    }

    pub fn line_length(&self, e: EntityId) -> f64 {
        match self.entities[e].geom {
            Geom::Line { a, b } => self.pos(a).distance(self.pos(b)),
            _ => 0.0,
        }
    }

    /// Distance from `p` to entity `e` (for hit testing).
    pub fn distance_to_entity(&self, e: EntityId, p: Vec2) -> f64 {
        match self.entities[e].geom {
            Geom::Line { a, b } => {
                let (a, b) = (self.pos(a), self.pos(b));
                let ab = b - a;
                let t = if ab.dot(ab) < 1e-18 { 0.0 } else { ((p - a).dot(ab) / ab.dot(ab)).clamp(0.0, 1.0) };
                p.distance(a + ab * t)
            }
            Geom::Circle { c, r } => (p.distance(self.pos(c)) - r).abs(),
            Geom::Arc { c, a, b } => {
                let (cc, pa, pb) = (self.pos(c), self.pos(a), self.pos(b));
                let r = pa.distance(cc);
                let ang = (p - cc).angle();
                if angle_in_ccw_range(ang, (pa - cc).angle(), (pb - cc).angle()) {
                    (p.distance(cc) - r).abs()
                } else {
                    p.distance(pa).min(p.distance(pb))
                }
            }
        }
    }

    /// Removes entities, the constraints that reference them, and any points no longer used.
    /// Returns where each old constraint index went (`None` if it was removed).
    pub fn delete_entities(&mut self, ids: &[EntityId]) -> Vec<Option<usize>> {
        let mut keep_e: Vec<bool> = vec![true; self.entities.len()];
        for &e in ids {
            if e < keep_e.len() {
                keep_e[e] = false;
            }
        }
        let mut map = self.retain_constraints(|c| c.entities().iter().all(|&e| keep_e[e]));
        let mut e_map = vec![usize::MAX; self.entities.len()];
        let mut n = 0;
        for (i, k) in keep_e.iter().enumerate() {
            if *k {
                e_map[i] = n;
                n += 1;
            }
        }
        let mut i = 0;
        self.entities.retain(|_| {
            i += 1;
            keep_e[i - 1]
        });
        for c in &mut self.constraints {
            c.remap(&|p| p, &|e| e_map[e]);
        }
        let second = self.remove_unused_points();
        for m in &mut map {
            *m = m.and_then(|i| second[i]);
        }
        map
    }

    /// Removes points referenced by no entity (except the origin) along with their
    /// constraints. Returns where each old constraint index went.
    pub fn remove_unused_points(&mut self) -> Vec<Option<usize>> {
        let mut used = vec![false; self.points.len()];
        used[ORIGIN] = true;
        for e in &self.entities {
            for p in e.points() {
                used[p] = true;
            }
        }
        // Constraints on removed points go too.
        let map = self.retain_constraints(|c| c.points().iter().all(|&p| used[p]));
        let mut p_map = vec![usize::MAX; self.points.len()];
        let mut n = 0;
        for (i, u) in used.iter().enumerate() {
            if *u {
                p_map[i] = n;
                n += 1;
            }
        }
        let mut i = 0;
        self.points.retain(|_| {
            i += 1;
            used[i - 1]
        });
        for e in &mut self.entities {
            match &mut e.geom {
                Geom::Line { a, b } => {
                    *a = p_map[*a];
                    *b = p_map[*b];
                }
                Geom::Circle { c, .. } => *c = p_map[*c],
                Geom::Arc { c, a, b } => {
                    *c = p_map[*c];
                    *a = p_map[*a];
                    *b = p_map[*b];
                }
            }
        }
        for c in &mut self.constraints {
            c.remap(&|p| p_map[p], &|e| e);
        }
        map
    }

    /// Keeps the constraints for which `keep` is true, renumbering annotations to match.
    /// Returns where each old constraint index went (`None` if it was removed).
    pub fn retain_constraints(&mut self, mut keep: impl FnMut(&Constraint) -> bool) -> Vec<Option<usize>> {
        let mut map = Vec::with_capacity(self.constraints.len());
        let mut n = 0;
        for c in &self.constraints {
            if keep(c) {
                map.push(Some(n));
                n += 1;
            } else {
                map.push(None);
            }
        }
        let mut i = 0;
        self.constraints.retain(|_| {
            i += 1;
            map[i - 1].is_some()
        });
        self.annotations.retain_mut(|a| match map.get(a.constraint).copied().flatten() {
            Some(ni) => {
                a.constraint = ni;
                true
            }
            None => false,
        });
        map
    }

    /// Removes one constraint, renumbering annotations to match.
    pub fn remove_constraint(&mut self, ci: usize) {
        let mut i = 0;
        self.retain_constraints(|_| {
            i += 1;
            i - 1 != ci
        });
    }

    pub fn annotation(&self, ci: usize) -> Option<&DimAnnotation> {
        self.annotations.iter().find(|a| a.constraint == ci)
    }

    /// The annotation for dimension `ci`, created with default settings if it has none.
    pub fn annotation_mut(&mut self, ci: usize) -> &mut DimAnnotation {
        let i = match self.annotations.iter().position(|a| a.constraint == ci) {
            Some(i) => i,
            None => {
                self.annotations.push(DimAnnotation { constraint: ci, offset: Vec2::ZERO, reference: false });
                self.annotations.len() - 1
            }
        };
        &mut self.annotations[i]
    }

    /// True if dimension `ci` is a reference dimension, which the solver does not use.
    pub fn is_reference(&self, ci: usize) -> bool {
        self.annotation(ci).is_some_and(|a| a.reference)
    }

    /// Makes dimension `ci` a reference dimension or a driving one. A reference dimension
    /// takes its value from the geometry at once.
    pub fn set_reference(&mut self, ci: usize, reference: bool) {
        if ci >= self.constraints.len() {
            return;
        }
        self.annotation_mut(ci).reference = reference;
        if reference {
            self.refresh_reference(ci);
        }
    }

    /// Sets reference dimension `ci` to what the geometry currently measures.
    pub(crate) fn refresh_reference(&mut self, ci: usize) {
        if let Some(v) = self.constraints.get(ci).and_then(|c| self.measure(c)) {
            self.constraints[ci].set_measured(v);
        }
    }

    /// What dimension `c` measures on the current geometry, in the same convention as
    /// [`Constraint::value`] (signed for horizontal and vertical distances). `None` for
    /// relations, which have no value, and for references to missing geometry.
    pub fn measure(&self, c: &Constraint) -> Option<f64> {
        use Constraint::*;
        let pos = |p: PointId| self.points.get(p).map(|q| q.pos());
        let line = |e: EntityId| match self.entities.get(e)?.geom {
            Geom::Line { a, b } => Some((pos(a)?, pos(b)?)),
            _ => None,
        };
        let radius = |e: EntityId| match self.entities.get(e)?.geom {
            Geom::Circle { r, .. } => Some(r),
            Geom::Arc { c, a, .. } => Some(pos(a)?.distance(pos(c)?)),
            Geom::Line { .. } => None,
        };
        let line_dist = |p: PointId, e: EntityId| {
            let (a, b) = line(e)?;
            let d = b - a;
            let (len, q) = (d.length(), pos(p)?);
            (len > 1e-12).then(|| (d.cross(q - a) / len).abs())
        };
        Some(match *c {
            Distance { p, q, .. } => pos(p)?.distance(pos(q)?),
            HDistance { p, q, .. } => pos(q)?.x - pos(p)?.x,
            VDistance { p, q, .. } => pos(q)?.y - pos(p)?.y,
            Length { line: e, .. } => match self.entities.get(e)?.geom {
                Geom::Line { .. } => {
                    let (a, b) = line(e)?;
                    a.distance(b)
                }
                _ => radius(e)?,
            },
            Radius { entity, .. } => radius(entity)?,
            Diameter { entity, .. } => 2.0 * radius(entity)?,
            PointLineDistance { p, line: e, .. } => line_dist(p, e)?,
            AxisDiameter { p, axis, .. } => 2.0 * line_dist(p, axis)?,
            ArcLength { entity, .. } => match self.entities.get(entity)?.geom {
                Geom::Arc { c, a, b } => {
                    let (cc, pa, pb) = (pos(c)?, pos(a)?, pos(b)?);
                    pa.distance(cc) * arc_sweep((pa - cc).angle(), (pb - cc).angle())
                }
                _ => return None,
            },
            Angle { a, b, .. } => {
                let ((a0, a1), (b0, b1)) = (line(a)?, line(b)?);
                let (u, w) = (a1 - a0, b1 - b0);
                u.cross(w).atan2(u.dot(w)).to_degrees()
            }
            RayAngle { a, b, flip_a, flip_b, .. } => {
                let ((a0, a1), (b0, b1)) = (line(a)?, line(b)?);
                let u = if flip_a { a0 - a1 } else { a1 - a0 };
                let w = if flip_b { b0 - b1 } else { b1 - b0 };
                ccw_angle(u, w).to_degrees()
            }
            _ => return None,
        })
    }

    /// Nearest point within `radius`, if any.
    pub fn pick_point(&self, p: Vec2, radius: f64) -> Option<PointId> {
        self.points
            .iter()
            .enumerate()
            .map(|(i, q)| (i, q.pos().distance(p)))
            .filter(|(_, d)| *d <= radius)
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
            .map(|(i, _)| i)
    }

    /// Nearest entity within `radius`, if any.
    pub fn pick_entity(&self, p: Vec2, radius: f64) -> Option<EntityId> {
        (0..self.entities.len())
            .map(|e| (e, self.distance_to_entity(e, p)))
            .filter(|(_, d)| *d <= radius)
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
            .map(|(e, _)| e)
    }
}

/// Counter-clockwise sweep from angle `start` to angle `end`, in radians, in (0, 2π]. A
/// zero sweep is a full turn, as for an arc whose ends meet.
pub fn arc_sweep(start: f64, end: f64) -> f64 {
    let s = (end - start).rem_euclid(std::f64::consts::TAU);
    if s < 1e-12 {
        std::f64::consts::TAU
    } else {
        s
    }
}

/// Counter-clockwise angle from direction `u` to direction `w`, in radians, in [0, 2π).
pub fn ccw_angle(u: Vec2, w: Vec2) -> f64 {
    u.cross(w).atan2(u.dot(w)).rem_euclid(std::f64::consts::TAU)
}

/// True if angle `t` lies on the counter-clockwise sweep from `start` to `end`.
pub fn angle_in_ccw_range(t: f64, start: f64, end: f64) -> bool {
    let tau = std::f64::consts::TAU;
    let sweep = (end - start).rem_euclid(tau);
    let off = (t - start).rem_euclid(tau);
    off <= sweep
}
