//! Builders for the sketch entity tools: rectangles, slots, polygons, arcs and circles.
//!
//! Each builder adds plain lines, arcs and circles plus the relations that hold the shape
//! together, so a new shape has exactly the degrees of freedom it should. The shapes, the
//! picks that make them and the relation sets follow `docs/re/sketch-entities.md`; the
//! relation sets there are AnvilCAD decisions, since the published sources show them only
//! in pictures. Builders never infer relations to existing geometry (the sketcher does
//! that from the pointer) and never change the sketch when they refuse.

use crate::sketch::{angle_in_ccw_range, Constraint, EntityId, Geom, PointId, Sketch, SketchError};
use anvil_kernel::{v2, Vec2};
use std::f64::consts::{PI, TAU};
use std::ops::RangeInclusive;

/// Lengths below this are treated as zero.
const EPS: f64 = 1e-9;

/// Construction lines added inside a rectangle (sketch-entities.md, "Construction lines").
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RectConstruction {
    #[default]
    None,
    /// The two diagonals, corner to corner.
    Corners,
    /// Lines joining the midpoints of opposite sides.
    Midpoints,
}

/// Options shared by the rectangle tools.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RectOptions {
    pub construction: RectConstruction,
    /// Holding Shift: two adjacent sides `Equal`, for a square or a rhombus.
    pub equal_sides: bool,
}

/// What a rectangle tool added.
#[derive(Clone, Debug, PartialEq)]
pub struct RectangleIds {
    /// Sides l0 to l3, each running from `corners[k]` to `corners[k + 1]`.
    pub sides: [EntityId; 4],
    pub corners: [PointId; 4],
    /// Construction lines: the diagonals or the midpoint lines.
    pub construction: Vec<EntityId>,
    /// The centre point of a centre rectangle.
    pub center: Option<PointId>,
}

/// How a straight slot's length is measured (sketch-entities.md, "Length type and
/// dimensions").
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SlotLength {
    #[default]
    CenterToCenter,
    Overall,
}

/// Options shared by the slot tools.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SlotOptions {
    pub length: SlotLength,
    /// "Add dimensions": the length (or arc radius and angle) and the width.
    pub dimensions: bool,
}

/// What a slot tool added.
#[derive(Clone, Debug, PartialEq)]
pub struct SlotIds {
    /// The end arcs, about the first and second end centres.
    pub ends: [EntityId; 2],
    /// The two sides: lines for a straight slot, the outer and inner arcs for an arc slot.
    pub sides: [EntityId; 2],
    /// The construction line or arc joining the end centres.
    pub centerline: EntityId,
    /// The slot's centre point: the construction line's midpoint, or the arc slot's
    /// centre.
    pub center: PointId,
    /// Dimensions added by "Add dimensions", as constraint indices.
    pub dimensions: Vec<usize>,
}

/// Which way a polygon's construction circle sits (sketch-entities.md, "Polygon").
/// SOLIDWORKS names the circle, not the polygon: an inscribed circle touches the sides.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PolygonCircle {
    #[default]
    Inscribed,
    Circumscribed,
}

/// The number of sides a polygon may have.
pub const POLYGON_SIDES: RangeInclusive<u32> = 3..=40;

/// What the polygon tool added.
#[derive(Clone, Debug, PartialEq)]
pub struct PolygonIds {
    /// Side k runs from `vertices[k]` to `vertices[k + 1]`.
    pub sides: Vec<EntityId>,
    pub vertices: Vec<PointId>,
    /// The construction circle.
    pub circle: EntityId,
    pub center: PointId,
}

/// What an arc tool added. The arc runs counter-clockwise from `start` to `end`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArcIds {
    pub arc: EntityId,
    pub center: PointId,
    pub start: PointId,
    pub end: PointId,
}

/// The four intent zones of the tangent arc tool (sketch-entities.md, "Tangent arc").
/// Forward and back make tangent arcs; left and right make normal arcs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TangentZone {
    Forward,
    Back,
    Left,
    Right,
}

impl TangentZone {
    /// The zone for a pointer that left the end point in direction `d`, where `t` is the
    /// direction leaving the entity: within 45° of `t` is forward, within 45° of `-t` is
    /// back, and otherwise left or right on the side the pointer went.
    pub fn from_directions(t: Vec2, d: Vec2) -> TangentZone {
        let (t, d) = (t.normalized(), d.normalized());
        let c = t.dot(d);
        let half = std::f64::consts::FRAC_1_SQRT_2;
        if c >= half {
            TangentZone::Forward
        } else if c <= -half {
            TangentZone::Back
        } else if t.cross(d) > 0.0 {
            TangentZone::Left
        } else {
            TangentZone::Right
        }
    }

    pub fn is_tangent(self) -> bool {
        matches!(self, TangentZone::Forward | TangentZone::Back)
    }
}

/// The centre of the circle through three points, or `None` if they line up.
pub fn circumcenter(a: Vec2, b: Vec2, c: Vec2) -> Option<Vec2> {
    let (ab, ac) = (b - a, c - a);
    let d = 2.0 * ab.cross(ac);
    let scale = ab.length().max(ac.length()).max(1.0);
    if d.abs() < EPS * scale * scale || ab.length() < EPS || ac.length() < EPS || (c - b).length() < EPS {
        return None;
    }
    let (b2, c2) = (ab.dot(ab), ac.dot(ac));
    Some(a + v2(ac.y * b2 - ab.y * c2, ab.x * c2 - ac.x * b2) / d)
}

/// Corner positions of the 3 point corner rectangle, the 3 point centre rectangle and the
/// parallelogram (sketch-entities.md, "Tools and picks", Inferred).
pub mod corners {
    use super::*;

    fn frame(p1: Vec2, p2: Vec2) -> Option<(Vec2, Vec2, f64)> {
        let d = p2 - p1;
        let len = d.length();
        (len > EPS).then(|| (d / len, (d / len).perp(), len))
    }

    /// Corners p1, p2, p2 + h n, p1 + h n, with h the signed distance of `p3` from the
    /// first edge.
    pub fn three_point_corner(p1: Vec2, p2: Vec2, p3: Vec2) -> Option<[Vec2; 4]> {
        let (_, n, _) = frame(p1, p2)?;
        let h = (p3 - p2).dot(n);
        (h.abs() > EPS).then(|| [p1, p2, p2 + n * h, p1 + n * h])
    }

    /// Corners c ± a u ± b n about the centre `c` = `p1`, with half-length a = |p2 − p1|
    /// and half-width b the distance of `p3` from the line through the first two picks.
    pub fn three_point_center(p1: Vec2, p2: Vec2, p3: Vec2) -> Option<[Vec2; 4]> {
        let (u, n, a) = frame(p1, p2)?;
        let b = (p3 - p1).dot(n).abs();
        (b > EPS).then(|| [p1 - u * a - n * b, p1 + u * a - n * b, p1 + u * a + n * b, p1 - u * a + n * b])
    }

    /// Corners p1, p2, p3 and p1 + (p3 − p2).
    pub fn parallelogram(p1: Vec2, p2: Vec2, p3: Vec2) -> Option<[Vec2; 4]> {
        let (u, _, _) = frame(p1, p2)?;
        let w = p3 - p2;
        (w.length() > EPS && u.cross(w.normalized()).abs() > 1e-9).then(|| [p1, p2, p3, p1 + w])
    }

    /// Corners of the axis-aligned rectangle centred on `c` with a corner at `corner`.
    pub fn center(c: Vec2, corner: Vec2) -> Option<[Vec2; 4]> {
        let h = v2((corner.x - c.x).abs(), (corner.y - c.y).abs());
        (h.x > EPS && h.y > EPS).then(|| [c - h, c + v2(h.x, -h.y), c + h, c + v2(-h.x, h.y)])
    }
}

/// Corners of a regular polygon centred on `c` with a vertex at `vertex`.
pub fn polygon_vertices(c: Vec2, vertex: Vec2, sides: u32) -> Vec<Vec2> {
    let r = c.distance(vertex);
    let phi = (vertex - c).angle();
    (0..sides).map(|k| c + Vec2::from_angle(phi + TAU * f64::from(k) / f64::from(sides)) * r).collect()
}

impl Sketch {
    fn quad(&mut self, corners: [Vec2; 4]) -> ([PointId; 4], [EntityId; 4]) {
        let p = corners.map(|c| self.add_point(c));
        let l = [
            self.add_line(p[0], p[1]),
            self.add_line(p[1], p[2]),
            self.add_line(p[2], p[3]),
            self.add_line(p[3], p[0]),
        ];
        (p, l)
    }

    fn construction_line(&mut self, a: PointId, b: PointId) -> EntityId {
        let l = self.add_line(a, b);
        self.entities[l].construction = true;
        l
    }

    /// Adds the construction lines of `opts` to a four-sided shape, and the centre point
    /// when `center` is set. Only one relation holds the centre: the diagonals (or
    /// midpoint lines) of a parallelogram bisect each other, so a second would be
    /// redundant.
    fn rect_finish(
        &mut self,
        corners: [PointId; 4],
        sides: [EntityId; 4],
        construction: RectConstruction,
        equal_sides: bool,
        center: bool,
    ) -> RectangleIds {
        let mut cons = Vec::new();
        let construction = if center && construction == RectConstruction::None {
            RectConstruction::Corners
        } else {
            construction
        };
        match construction {
            RectConstruction::None => {}
            RectConstruction::Corners => {
                cons.push(self.construction_line(corners[0], corners[2]));
                cons.push(self.construction_line(corners[1], corners[3]));
            }
            RectConstruction::Midpoints => {
                let mid = |sk: &Sketch, l: EntityId| match sk.entities[l].geom {
                    Geom::Line { a, b } => (sk.pos(a) + sk.pos(b)) * 0.5,
                    _ => Vec2::ZERO,
                };
                for (s0, s1) in [(sides[0], sides[2]), (sides[1], sides[3])] {
                    let (m0, m1) = (mid(self, s0), mid(self, s1));
                    let (a, b) = (self.add_point(m0), self.add_point(m1));
                    self.add_constraint(Constraint::Midpoint { p: a, line: s0 });
                    self.add_constraint(Constraint::Midpoint { p: b, line: s1 });
                    cons.push(self.construction_line(a, b));
                }
            }
        }
        if equal_sides {
            self.add_constraint(Constraint::Equal { a: sides[0], b: sides[1] });
        }
        let center = if center {
            let c = (0..4).map(|k| self.pos(corners[k])).fold(Vec2::ZERO, |s, p| s + p) * 0.25;
            let m = self.add_point(c);
            self.add_constraint(Constraint::Midpoint { p: m, line: cons[0] });
            Some(m)
        } else {
            None
        };
        RectangleIds { sides, corners, construction: cons, center }
    }

    /// Corner rectangle: axis-aligned, from two opposite corners. Horizontal and vertical
    /// sides; 4 degrees of freedom.
    pub fn add_corner_rectangle(
        &mut self,
        p0: Vec2,
        p1: Vec2,
        opts: RectOptions,
    ) -> Result<RectangleIds, SketchError> {
        if (p0.x - p1.x).abs() <= EPS || (p0.y - p1.y).abs() <= EPS {
            return Err(SketchError::Degenerate);
        }
        let sides = self.add_rectangle(p0, p1);
        let corners = sides.map(|l| match self.entities[l].geom {
            Geom::Line { a, .. } => a,
            _ => 0,
        });
        Ok(self.rect_finish(corners, sides, opts.construction, opts.equal_sides, false))
    }

    /// Centre rectangle: axis-aligned, from its centre and a corner, with construction
    /// diagonals (or midpoint lines) and a centre point at their middle. 4 degrees of
    /// freedom: the centre, the width and the height.
    pub fn add_center_rectangle(
        &mut self,
        c: Vec2,
        corner: Vec2,
        opts: RectOptions,
    ) -> Result<RectangleIds, SketchError> {
        let pts = corners::center(c, corner).ok_or(SketchError::Degenerate)?;
        let sides = self.add_rectangle(pts[0], pts[2]);
        let corners = sides.map(|l| match self.entities[l].geom {
            Geom::Line { a, .. } => a,
            _ => 0,
        });
        Ok(self.rect_finish(corners, sides, opts.construction, opts.equal_sides, true))
    }

    fn add_any_rectangle(&mut self, pts: [Vec2; 4], opts: RectOptions, center: bool) -> RectangleIds {
        let (p, l) = self.quad(pts);
        self.add_constraint(Constraint::Perpendicular { a: l[0], b: l[1] });
        self.add_constraint(Constraint::Parallel { a: l[0], b: l[2] });
        self.add_constraint(Constraint::Parallel { a: l[1], b: l[3] });
        self.rect_finish(p, l, opts.construction, opts.equal_sides, center)
    }

    /// 3 point corner rectangle at any angle: first corner, end of the first edge, and a
    /// pick that sets the width. 5 degrees of freedom.
    pub fn add_3pt_corner_rectangle(
        &mut self,
        p1: Vec2,
        p2: Vec2,
        p3: Vec2,
        opts: RectOptions,
    ) -> Result<RectangleIds, SketchError> {
        let pts = corners::three_point_corner(p1, p2, p3).ok_or(SketchError::Degenerate)?;
        Ok(self.add_any_rectangle(pts, opts, false))
    }

    /// 3 point centre rectangle at any angle: centre, the middle of one side, and a pick
    /// that sets the other half-size. 5 degrees of freedom.
    pub fn add_3pt_center_rectangle(
        &mut self,
        p1: Vec2,
        p2: Vec2,
        p3: Vec2,
        opts: RectOptions,
    ) -> Result<RectangleIds, SketchError> {
        let pts = corners::three_point_center(p1, p2, p3).ok_or(SketchError::Degenerate)?;
        Ok(self.add_any_rectangle(pts, opts, true))
    }

    /// Parallelogram from three corners; the fourth is p1 + (p3 − p2). Opposite sides
    /// parallel; 6 degrees of freedom.
    pub fn add_parallelogram(
        &mut self,
        p1: Vec2,
        p2: Vec2,
        p3: Vec2,
        opts: RectOptions,
    ) -> Result<RectangleIds, SketchError> {
        let pts = corners::parallelogram(p1, p2, p3).ok_or(SketchError::Degenerate)?;
        let (p, l) = self.quad(pts);
        self.add_constraint(Constraint::Parallel { a: l[0], b: l[2] });
        self.add_constraint(Constraint::Parallel { a: l[1], b: l[3] });
        Ok(self.rect_finish(p, l, opts.construction, opts.equal_sides, false))
    }

    /// Straight slot with end-arc centres `c1` and `c2` and width `width`
    /// (sketch-entities.md, "Slots"). Sides tangent to both end arcs, end arcs `Equal`, and
    /// a centre point at the middle of the construction line c1–c2: 5 degrees of freedom.
    pub fn add_straight_slot(
        &mut self,
        c1: Vec2,
        c2: Vec2,
        width: f64,
        opts: SlotOptions,
    ) -> Result<SlotIds, SketchError> {
        let len = c1.distance(c2);
        if len <= EPS || !width.is_finite() || width <= EPS {
            return Err(SketchError::Degenerate);
        }
        let h = 0.5 * width;
        let u = (c2 - c1) / len;
        let n = u.perp();
        let pc1 = self.add_point(c1);
        let pc2 = self.add_point(c2);
        let m = self.add_point((c1 + c2) * 0.5);
        let p1 = self.add_point(c1 - n * h);
        let p2 = self.add_point(c2 - n * h);
        let p3 = self.add_point(c2 + n * h);
        let p4 = self.add_point(c1 + n * h);
        let s1 = self.add_line(p1, p2);
        let a2 = self.add_arc(pc2, p2, p3);
        let s2 = self.add_line(p3, p4);
        let a1 = self.add_arc(pc1, p4, p1);
        let k = self.construction_line(pc1, pc2);
        for s in [s1, s2] {
            for a in [a1, a2] {
                self.add_constraint(Constraint::Tangent { line: s, entity: a });
            }
        }
        self.add_constraint(Constraint::Equal { a: a1, b: a2 });
        self.add_constraint(Constraint::Midpoint { p: m, line: k });
        let mut dims = Vec::new();
        if opts.dimensions {
            match opts.length {
                SlotLength::CenterToCenter => {
                    dims.push(self.add_constraint(Constraint::Length { line: k, value: len }))
                }
                SlotLength::Overall => {
                    // Construction points where the centre line meets the end arcs' far
                    // sides carry the overall length.
                    let e1 = self.add_point(c1 - u * h);
                    let e2 = self.add_point(c2 + u * h);
                    for (e, arc) in [(e1, a1), (e2, a2)] {
                        self.add_constraint(Constraint::PointOnLine { p: e, line: k });
                        self.add_constraint(Constraint::PointOnCircle { p: e, entity: arc });
                    }
                    dims.push(self.add_constraint(Constraint::Distance { p: e1, q: e2, value: len + width }));
                }
            }
            dims.push(self.add_constraint(Constraint::Diameter { entity: a1, value: width }));
        }
        Ok(SlotIds { ends: [a1, a2], sides: [s1, s2], centerline: k, center: m, dimensions: dims })
    }

    /// Centrepoint straight slot: the slot's centre, then the centre of one end arc; the
    /// other end arc's centre is its mirror through the slot's centre.
    pub fn add_center_straight_slot(
        &mut self,
        center: Vec2,
        end: Vec2,
        width: f64,
        opts: SlotOptions,
    ) -> Result<SlotIds, SketchError> {
        self.add_straight_slot(center * 2.0 - end, end, width, opts)
    }

    /// Arc slot about `o` whose centre arc runs counter-clockwise from `c1` to `c2`
    /// (`c2` is moved onto the circle through `c1`). Outer and inner arcs concentric with
    /// the construction arc; end arcs centred on its ends, sharing end points with the
    /// outer and inner arcs and tangent to both. 6 degrees of freedom: the centre, the
    /// radius, the two end angles and the width.
    ///
    /// The specification also makes the end arcs `Equal`; that follows from the outer and
    /// inner arcs each having one radius, so it is left out to keep the relations
    /// independent.
    fn build_arc_slot(
        &mut self,
        o: Vec2,
        c1: Vec2,
        c2: Vec2,
        width: f64,
        opts: SlotOptions,
    ) -> Result<SlotIds, SketchError> {
        let r = o.distance(c1);
        let h = 0.5 * width;
        if r <= EPS || !width.is_finite() || width <= EPS || width >= 2.0 * r - EPS {
            return Err(SketchError::Degenerate);
        }
        let (u1, u2) = ((c1 - o) / r, (c2 - o).normalized());
        if u2.length() < 0.5 || u1.distance(u2) <= EPS {
            return Err(SketchError::Degenerate);
        }
        let po = self.add_point(o);
        let pc1 = self.add_point(c1);
        let pc2 = self.add_point(o + u2 * r);
        let oa = self.add_point(o + u1 * (r + h));
        let ob = self.add_point(o + u2 * (r + h));
        let ia = self.add_point(o + u1 * (r - h));
        let ib = self.add_point(o + u2 * (r - h));
        let outer = self.add_arc(po, oa, ob);
        let e2 = self.add_arc(pc2, ob, ib);
        let inner = self.add_arc(po, ia, ib);
        let e1 = self.add_arc(pc1, ia, oa);
        let k = self.add_arc(po, pc1, pc2);
        self.entities[k].construction = true;
        for e in [e1, e2] {
            self.add_constraint(Constraint::TangentCircles { a: e, b: outer, inside: true });
            self.add_constraint(Constraint::TangentCircles { a: e, b: inner, inside: false });
        }
        let mut dims = Vec::new();
        if opts.dimensions {
            dims.push(self.add_constraint(Constraint::Radius { entity: k, value: r }));
            let r1 = self.construction_line(po, pc1);
            let r2 = self.construction_line(po, pc2);
            let sweep = crate::sketch::arc_sweep(u1.angle(), u2.angle()).to_degrees();
            dims.push(self.add_constraint(Constraint::RayAngle {
                a: r1,
                b: r2,
                flip_a: false,
                flip_b: false,
                degrees: sweep,
            }));
            dims.push(self.add_constraint(Constraint::Diameter { entity: e1, value: width }));
        }
        Ok(SlotIds { ends: [e1, e2], sides: [outer, inner], centerline: k, center: po, dimensions: dims })
    }

    /// Centrepoint arc slot: the arc's centre, its start (setting the radius), the end
    /// direction and the width. `swept` is the signed angle the pointer turned through
    /// about the centre after the start pick: positive runs counter-clockwise.
    pub fn add_centerpoint_arc_slot(
        &mut self,
        center: Vec2,
        start: Vec2,
        end: Vec2,
        swept: f64,
        width: f64,
        opts: SlotOptions,
    ) -> Result<SlotIds, SketchError> {
        let r = center.distance(start);
        let end = center + (end - center).normalized() * r;
        if swept >= 0.0 {
            self.build_arc_slot(center, start, end, width, opts)
        } else {
            self.build_arc_slot(center, end, start, width, opts)
        }
    }

    /// 3 point arc slot: the centre arc's start and end, a third point on it, and the
    /// width.
    pub fn add_3pt_arc_slot(
        &mut self,
        start: Vec2,
        end: Vec2,
        through: Vec2,
        width: f64,
        opts: SlotOptions,
    ) -> Result<SlotIds, SketchError> {
        let (o, a, b) = three_point_arc(start, end, through).ok_or(SketchError::Degenerate)?;
        self.build_arc_slot(o, a, b, width, opts)
    }

    /// Regular polygon with `sides` sides (3 to 40), centred on `center`, with a vertex at
    /// `vertex` (sketch-entities.md, "Polygon"). Built from lines held regular by exterior
    /// `Angle`s and `Equal`s, with a construction circle through the corners
    /// (circumscribed) or touching the sides (inscribed): 4 degrees of freedom.
    ///
    /// The specification ties the circle to v0, v1 and v2 (or sides 0 to 2). Any three
    /// distinct corners give the same freedoms; they are spread round the polygon here so
    /// that the solve stays well conditioned for many sides.
    pub fn add_polygon(
        &mut self,
        center: Vec2,
        vertex: Vec2,
        sides: u32,
        circle: PolygonCircle,
    ) -> Result<PolygonIds, SketchError> {
        if !POLYGON_SIDES.contains(&sides) {
            return Err(SketchError::OutOfRange("a polygon has 3 to 40 sides"));
        }
        let r = center.distance(vertex);
        if r <= EPS {
            return Err(SketchError::Degenerate);
        }
        let n = sides as usize;
        let vertices: Vec<PointId> =
            polygon_vertices(center, vertex, sides).into_iter().map(|p| self.add_point(p)).collect();
        let lines: Vec<EntityId> =
            (0..n).map(|k| self.add_line(vertices[k], vertices[(k + 1) % n])).collect();
        let exterior = 360.0 / f64::from(sides);
        for k in 0..n - 1 {
            self.add_constraint(Constraint::Angle { a: lines[k], b: lines[k + 1], degrees: exterior });
        }
        for &l in lines.iter().take(n - 2).skip(1) {
            self.add_constraint(Constraint::Equal { a: lines[0], b: l });
        }
        let c = self.add_point(center);
        let cr = match circle {
            PolygonCircle::Circumscribed => r,
            PolygonCircle::Inscribed => r * (PI / f64::from(sides)).cos(),
        };
        let circ = self.add_circle(c, cr);
        self.entities[circ].construction = true;
        let mut ties = [0, n / 3, 2 * n / 3];
        ties.sort_unstable();
        for k in ties {
            match circle {
                PolygonCircle::Circumscribed => {
                    self.add_constraint(Constraint::PointOnCircle { p: vertices[k], entity: circ })
                }
                PolygonCircle::Inscribed => {
                    self.add_constraint(Constraint::Tangent { line: lines[k], entity: circ })
                }
            };
        }
        Ok(PolygonIds { sides: lines, vertices, circle: circ, center: c })
    }

    fn arc_from(&mut self, c: Vec2, a: Vec2, b: Vec2) -> ArcIds {
        let (pc, pa, pb) = (self.add_point(c), self.add_point(a), self.add_point(b));
        ArcIds { arc: self.add_arc(pc, pa, pb), center: pc, start: pa, end: pb }
    }

    /// Centrepoint arc: centre, start (setting the radius), then the end, moved onto the
    /// circle. `swept` is the signed angle the pointer turned through about the centre
    /// after the start pick; when it is negative the arc runs clockwise, so it is stored
    /// counter-clockwise from the end to the start (sketch-entities.md, "Centerpoint arc").
    pub fn add_centerpoint_arc(
        &mut self,
        center: Vec2,
        start: Vec2,
        end: Vec2,
        swept: f64,
    ) -> Result<ArcIds, SketchError> {
        let r = center.distance(start);
        let d = (end - center).normalized();
        if r <= EPS || d.length() < 0.5 {
            return Err(SketchError::Degenerate);
        }
        let end = center + d * r;
        if end.distance(start) <= EPS * r.max(1.0) {
            return Err(SketchError::Degenerate);
        }
        Ok(if swept >= 0.0 { self.arc_from(center, start, end) } else { self.arc_from(center, end, start) })
    }

    /// 3 point arc: from `start` to `end` through `through`.
    pub fn add_3pt_arc(&mut self, start: Vec2, end: Vec2, through: Vec2) -> Result<ArcIds, SketchError> {
        let (c, a, b) = three_point_arc(start, end, through).ok_or(SketchError::Degenerate)?;
        Ok(self.arc_from(c, a, b))
    }

    /// The direction leaving entity `e` at its end point `at`: for a line, from its other
    /// end towards `at`; for an arc, its tangent at `at` pointing away from the arc.
    pub fn leaving_direction(&self, e: EntityId, at: PointId) -> Option<Vec2> {
        let pos = |p: PointId| self.points.get(p).map(|q| q.pos());
        match self.entities.get(e)?.geom {
            Geom::Line { a, b } if at == b => Some((pos(b)? - pos(a)?).normalized()),
            Geom::Line { a, b } if at == a => Some((pos(a)? - pos(b)?).normalized()),
            Geom::Arc { c, b, .. } if at == b => Some((pos(b)? - pos(c)?).normalized().perp()),
            Geom::Arc { c, a, .. } if at == a => Some(-(pos(a)? - pos(c)?).normalized().perp()),
            _ => None,
        }
    }

    /// Tangent arc from the end point `at` of line or arc `from`, ending at `end`
    /// (sketch-entities.md, "Tangent arc"). The zone sets the direction the arc leaves in:
    /// along the entity (forward), back along it, or square to it (left or right, a normal
    /// arc). The arc shares `at` and gets `Tangent` (or arc-to-arc tangency) for a tangent
    /// arc; a normal arc keeps its centre on the line, or on the radius through `at` for an
    /// arc.
    pub fn add_tangent_arc(
        &mut self,
        from: EntityId,
        at: PointId,
        end: Vec2,
        zone: TangentZone,
    ) -> Result<ArcIds, SketchError> {
        let t = self
            .leaving_direction(from, at)
            .ok_or(SketchError::Unsupported("a tangent arc starts at the end of a line or arc"))?;
        let p = self.pos(at);
        let tau = match zone {
            TangentZone::Forward => t,
            TangentZone::Back => -t,
            TangentZone::Left => t.perp(),
            TangentZone::Right => -t.perp(),
        };
        let nu = tau.perp();
        let d = end - p;
        let den = 2.0 * d.dot(nu);
        if d.length() <= EPS || den.abs() <= EPS * d.length() {
            return Err(SketchError::Degenerate);
        }
        let rho = d.dot(d) / den;
        let c = p + nu * rho;
        let pc = self.add_point(c);
        let pe = self.add_point(end);
        let (a, b) = if rho > 0.0 { (at, pe) } else { (pe, at) };
        let arc = self.add_arc(pc, a, b);
        let is_line = self.entities[from].is_line();
        match (zone.is_tangent(), is_line) {
            (true, true) => {
                self.add_constraint(Constraint::Tangent { line: from, entity: arc });
            }
            (true, false) => {
                let inside = self.circles_inside(from, arc);
                self.add_constraint(Constraint::TangentCircles { a: from, b: arc, inside });
            }
            (false, true) => {
                self.add_constraint(Constraint::PointOnLine { p: pc, line: from });
            }
            (false, false) => {
                let old_c = match self.entities[from].geom {
                    Geom::Arc { c, .. } => c,
                    _ => pc,
                };
                let radial = self.construction_line(old_c, at);
                self.add_constraint(Constraint::PointOnLine { p: pc, line: radial });
            }
        }
        Ok(ArcIds { arc, center: pc, start: a, end: b })
    }

    /// Whether two circles or arcs touching each other do so inside or outside, from the
    /// current geometry: outside when the centres are farther apart than the larger
    /// radius (sketch-tools.md, "Every 2D relation", Decision).
    pub fn circles_inside(&self, a: EntityId, b: EntityId) -> bool {
        let centre = |e: EntityId| match self.entities[e].geom {
            Geom::Circle { c, .. } | Geom::Arc { c, .. } => self.pos(c),
            Geom::Line { a, .. } => self.pos(a),
        };
        centre(a).distance(centre(b)) < self.radius(a).max(self.radius(b))
    }

    /// Perimeter circle through three points (sketch-entities.md, "Circles", Decision).
    pub fn add_circle_3pt(
        &mut self,
        p1: Vec2,
        p2: Vec2,
        p3: Vec2,
    ) -> Result<(EntityId, PointId), SketchError> {
        let c = circumcenter(p1, p2, p3).ok_or(SketchError::Degenerate)?;
        let pc = self.add_point(c);
        Ok((self.add_circle(pc, c.distance(p1)), pc))
    }

    /// Circle with `p1` and `p2` as the ends of a diameter.
    pub fn add_circle_2pt(&mut self, p1: Vec2, p2: Vec2) -> Result<(EntityId, PointId), SketchError> {
        if p1.distance(p2) <= EPS {
            return Err(SketchError::Degenerate);
        }
        let c = (p1 + p2) * 0.5;
        let pc = self.add_point(c);
        Ok((self.add_circle(pc, c.distance(p1)), pc))
    }
}

/// The arc from `start` to `end` through `through`, as (centre, first, second) with the
/// arc running counter-clockwise from first to second. `None` if the points line up or
/// two coincide.
pub fn three_point_arc(start: Vec2, end: Vec2, through: Vec2) -> Option<(Vec2, Vec2, Vec2)> {
    let c = circumcenter(start, end, through)?;
    let (s, e, t) = ((start - c).angle(), (end - c).angle(), (through - c).angle());
    if angle_in_ccw_range(t, s, e) {
        Some((c, start, end))
    } else {
        Some((c, end, start))
    }
}
