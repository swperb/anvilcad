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
            | PointLineDistance { value, .. } => Some(value),
            Angle { degrees, .. } => Some(degrees),
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
            | PointLineDistance { value, .. } => *value = v,
            Angle { degrees, .. } => *degrees = v,
            _ => {}
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
            | PointLineDistance { p, .. } => vec![p],
            _ => vec![],
        }
    }

    pub fn entities(&self) -> Vec<EntityId> {
        use Constraint::*;
        match *self {
            Horizontal { line } | Vertical { line } | Length { line, .. } => vec![line],
            Radius { entity, .. } | Diameter { entity, .. } => vec![entity],
            Parallel { a, b }
            | Perpendicular { a, b }
            | Equal { a, b }
            | Angle { a, b, .. }
            | Concentric { a, b } => vec![a, b],
            PointOnLine { line, .. } | Midpoint { line, .. } | PointLineDistance { line, .. } => vec![line],
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
            Radius { entity, .. } | Diameter { entity, .. } => *entity = em(*entity),
            Parallel { a, b }
            | Perpendicular { a, b }
            | Equal { a, b }
            | Angle { a, b, .. }
            | Concentric { a, b } => {
                *a = em(*a);
                *b = em(*b);
            }
            PointOnLine { p, line } | Midpoint { p, line } | PointLineDistance { p, line, .. } => {
                *p = pm(*p);
                *line = em(*line);
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Sketch {
    pub points: Vec<Point>,
    pub entities: Vec<Entity>,
    pub constraints: Vec<Constraint>,
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
    pub fn delete_entities(&mut self, ids: &[EntityId]) {
        let mut keep_e: Vec<bool> = vec![true; self.entities.len()];
        for &e in ids {
            if e < keep_e.len() {
                keep_e[e] = false;
            }
        }
        self.constraints.retain(|c| c.entities().iter().all(|&e| keep_e[e]));
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
        self.remove_unused_points();
    }

    /// Removes points referenced by no entity (except the origin) along with their constraints.
    pub fn remove_unused_points(&mut self) {
        let mut used = vec![false; self.points.len()];
        used[ORIGIN] = true;
        for e in &self.entities {
            for p in e.points() {
                used[p] = true;
            }
        }
        // Constraints on removed points go too.
        self.constraints.retain(|c| c.points().iter().all(|&p| used[p]));
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

/// True if angle `t` lies on the counter-clockwise sweep from `start` to `end`.
pub fn angle_in_ccw_range(t: f64, start: f64, end: f64) -> bool {
    let tau = std::f64::consts::TAU;
    let sweep = (end - start).rem_euclid(tau);
    let off = (t - start).rem_euclid(tau);
    off <= sweep
}
