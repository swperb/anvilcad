//! Sketch tools that make new geometry from existing geometry: Offset Entities, Mirror
//! Entities, linear and circular sketch patterns, and Move, Copy, Rotate and Scale.
//!
//! The behaviour follows `docs/re/sketch-tools.md`; the code cites the decisions it
//! implements. Each tool either finishes or returns an error and leaves the sketch as it
//! was.

use crate::curve::{reflect, rotate_about};
use crate::sketch::{
    arc_sweep, Constraint, EntityId, Geom, PointId, Renumbering, Sketch, SketchError, ORIGIN,
};
use anvil_kernel::offset::{offset_chain, travel, OffsetError};
use anvil_kernel::profile::{Loop2, Seg2};
use anvil_kernel::{v2, Vec2};
use std::collections::HashMap;

/// Lengths below this are treated as zero.
const EPS: f64 = 1e-9;

/// How Offset Entities closes the ends of an offset open chain (sketch-tools.md, "Cap
/// ends").
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CapEnds {
    #[default]
    None,
    Arcs,
    Lines,
}

/// Options of Offset Entities (sketch-tools.md, "Offset Entities").
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OffsetOptions {
    pub distance: f64,
    /// Offset to the other side: inward for a closed chain, right for an open one.
    pub reverse: bool,
    /// Offset to both sides, each with its own dimension.
    pub bidirectional: bool,
    /// "Select chain": take in every curve joined end to end with the selection.
    pub chain: bool,
    pub caps: CapEnds,
    /// Make the original curves construction geometry.
    pub base_construction: bool,
    /// Make the new curves construction geometry.
    pub offset_construction: bool,
    /// "Add dimensions": the offset distance as a driving dimension.
    pub dimensions: bool,
}

impl Default for OffsetOptions {
    fn default() -> Self {
        OffsetOptions {
            distance: 5.0,
            reverse: false,
            bidirectional: false,
            chain: true,
            caps: CapEnds::None,
            base_construction: false,
            offset_construction: false,
            dimensions: true,
        }
    }
}

/// What Offset Entities added.
#[derive(Clone, Debug, PartialEq)]
pub struct OffsetResult {
    /// The offset curves, one chain after the other for a bi-directional offset.
    pub entities: Vec<EntityId>,
    /// End caps and the construction lines that hold uncapped ends.
    pub caps: Vec<EntityId>,
    /// The offset dimensions, one per side.
    pub dimensions: Vec<usize>,
}

/// Options of a linear sketch pattern (sketch-tools.md, "Linear").
#[derive(Clone, Debug, PartialEq)]
pub struct LinearPattern {
    /// Instances in each direction, counting the seed.
    pub count: [u32; 2],
    pub spacing: [f64; 2],
    /// Both directions' angles, counter-clockwise from the sketch x axis.
    pub degrees: [f64; 2],
    /// Instances left out, as (i, j).
    pub skip: Vec<(u32, u32)>,
    /// "Dimension X spacing" and "Dimension Y spacing".
    pub dimension_spacing: bool,
    /// "Fix X-axis direction" (for a direction along an axis).
    pub fix_direction: bool,
    /// "Dimension angle between axes".
    pub dimension_angle: bool,
}

impl Default for LinearPattern {
    fn default() -> Self {
        LinearPattern {
            count: [3, 1],
            spacing: [10.0, 10.0],
            degrees: [0.0, 90.0],
            skip: Vec::new(),
            dimension_spacing: true,
            fix_direction: true,
            dimension_angle: true,
        }
    }
}

/// Options of a circular sketch pattern (sketch-tools.md, "Circular").
#[derive(Clone, Debug, PartialEq)]
pub struct CircularPattern {
    pub center: Vec2,
    /// Instances, counting the seed.
    pub count: u32,
    /// The angle the pattern covers.
    pub total_degrees: f64,
    /// Clockwise instead of counter-clockwise.
    pub reverse: bool,
    /// Instances left out, by number (the seed is 0).
    pub skip: Vec<u32>,
    pub dimension_radius: bool,
    /// "Dimension angular spacing": the angle between instances.
    pub dimension_angle: bool,
}

impl Default for CircularPattern {
    fn default() -> Self {
        CircularPattern {
            center: Vec2::ZERO,
            count: 4,
            total_degrees: 360.0,
            reverse: false,
            skip: Vec::new(),
            dimension_radius: true,
            dimension_angle: true,
        }
    }
}

/// What a sketch pattern added.
#[derive(Clone, Debug, PartialEq)]
pub struct PatternResult {
    /// Each instance's entities, in the seed's order. The seed is not included.
    pub instances: Vec<Vec<EntityId>>,
    /// The construction lines that carry the pattern's directions or angle.
    pub construction: Vec<EntityId>,
    pub dimensions: Vec<usize>,
}

/// A move, rotation or scaling of sketch geometry.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Transform2 {
    Translate(Vec2),
    /// A positive angle turns counter-clockwise (sketch-tools.md, "Move, Copy, Rotate,
    /// Scale and Stretch", Decision).
    Rotate {
        center: Vec2,
        degrees: f64,
    },
    Scale {
        about: Vec2,
        factor: f64,
    },
}

impl Transform2 {
    pub fn apply(&self, p: Vec2) -> Vec2 {
        match *self {
            Transform2::Translate(d) => p + d,
            Transform2::Rotate { center, degrees } => rotate_about(p, center, degrees.to_radians()),
            Transform2::Scale { about, factor } => about + (p - about) * factor,
        }
    }

    /// The direction angle a line at `degrees` has after the transform.
    fn turn(&self, degrees: f64) -> f64 {
        match *self {
            Transform2::Rotate { degrees: a, .. } => degrees + a,
            _ => degrees,
        }
    }

    fn length_factor(&self) -> f64 {
        match *self {
            Transform2::Scale { factor, .. } => factor,
            _ => 1.0,
        }
    }

    /// Quarter turns of a rotation that is a whole number of them, 0 to 3.
    fn quarter_turns(&self) -> Option<i64> {
        match *self {
            Transform2::Rotate { degrees, .. } => {
                let q = degrees / 90.0;
                ((q - q.round()).abs() < 1e-9).then(|| (q.round() as i64).rem_euclid(4))
            }
            _ => Some(0),
        }
    }
}

/// Options of Move, Copy, Rotate and Scale.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TransformOptions {
    /// Keep the originals and add transformed copies.
    pub copy: bool,
    /// How many copies, each transformed once more than the last.
    pub copies: u32,
    /// "Keep relations": relations to geometry that is not moved stay (and shared points
    /// stay shared); otherwise they are broken.
    pub keep_relations: bool,
}

impl Default for TransformOptions {
    fn default() -> Self {
        TransformOptions { copy: false, copies: 1, keep_relations: false }
    }
}

/// The circular pattern centre given as a radius and an arc angle from the seed: centre =
/// seed + radius (cos A, sin A) (sketch-tools.md, "Circular", Inferred).
pub fn circular_pattern_center(seed: Vec2, radius: f64, arc_degrees: f64) -> Vec2 {
    seed + Vec2::from_angle(arc_degrees.to_radians()) * radius
}

/// One entity walked along a chain, forwards (first point to second) or backwards.
#[derive(Clone, Copy, Debug)]
struct Link {
    e: EntityId,
    reversed: bool,
}

/// Where a chain ends: its end point and the curve that ends there.
#[derive(Clone, Copy, Debug)]
struct ChainEnd {
    point: PointId,
    entity: EntityId,
    /// Direction leaving the chain at this end.
    out: Vec2,
}

fn map_err(e: OffsetError) -> SketchError {
    match e {
        OffsetError::NotAChain => SketchError::Unsupported("the curves must join end to end in one chain"),
        OffsetError::Collapsed => SketchError::TooLarge,
    }
}

impl Sketch {
    fn ends(&self, e: EntityId) -> Option<(PointId, PointId)> {
        match self.entities[e].geom {
            Geom::Line { a, b } | Geom::Arc { a, b, .. } => Some((a, b)),
            Geom::Circle { .. } => None,
        }
    }

    /// Every entity joined end to end with `e` (through shared end points), `e` first.
    pub fn select_chain(&self, e: EntityId) -> Vec<EntityId> {
        if e >= self.entities.len() {
            return Vec::new();
        }
        let construction = self.entities[e].construction;
        let mut out = vec![e];
        let mut i = 0;
        while i < out.len() {
            if let Some((a, b)) = self.ends(out[i]) {
                for p in [a, b] {
                    let next = self.entities_ending_at(p);
                    // Stop at branches: a chain runs through points where two curves meet.
                    if next.len() != 2 {
                        continue;
                    }
                    for f in next {
                        if !out.contains(&f) && self.entities[f].construction == construction {
                            out.push(f);
                        }
                    }
                }
            }
            i += 1;
        }
        out
    }

    /// Orders `ids` into one chain, walked so that the first selected entity runs
    /// forwards. Returns the links and whether the chain closes.
    fn chain_order(&self, ids: &[EntityId]) -> Result<(Vec<Link>, bool), SketchError> {
        let not_chain = SketchError::Unsupported("the curves must join end to end in one chain");
        let first = *ids.first().ok_or(not_chain.clone())?;
        let mut used = vec![false; ids.len()];
        used[0] = true;
        let (s0, e0) = self.ends(first).ok_or(not_chain.clone())?;
        let mut links = vec![Link { e: first, reversed: false }];
        let find = |used: &[bool], node: PointId| -> Result<Option<(usize, bool)>, SketchError> {
            let mut hit = None;
            for (k, &f) in ids.iter().enumerate() {
                if used[k] {
                    continue;
                }
                let (a, b) = self
                    .ends(f)
                    .ok_or(SketchError::Unsupported("the curves must join end to end in one chain"))?;
                if a == node || b == node {
                    if hit.is_some() {
                        return Err(SketchError::Unsupported("the curves branch"));
                    }
                    hit = Some((k, b == node));
                }
            }
            Ok(hit)
        };
        // Forwards from the first entity's end.
        let mut node = e0;
        while node != s0 {
            match find(&used, node)? {
                Some((k, ends_here)) => {
                    used[k] = true;
                    links.push(Link { e: ids[k], reversed: ends_here });
                    let (a, b) = self.ends(ids[k]).ok_or(not_chain.clone())?;
                    node = if ends_here { a } else { b };
                }
                None => break,
            }
        }
        let closed = node == s0 && links.len() > 1;
        if !closed {
            // Backwards from the first entity's start.
            let mut node = s0;
            while let Some((k, ends_here)) = find(&used, node)? {
                used[k] = true;
                links.insert(0, Link { e: ids[k], reversed: !ends_here });
                let (a, b) = self.ends(ids[k]).ok_or(not_chain.clone())?;
                node = if ends_here { a } else { b };
            }
        }
        if used.iter().any(|u| !u) {
            return Err(not_chain);
        }
        Ok((links, closed))
    }

    fn seg_of(&self, l: Link, source: u32) -> Seg2 {
        let s = match self.entities[l.e].geom {
            Geom::Line { a, b } => Seg2::Line { a: self.pos(a), b: self.pos(b), source },
            Geom::Arc { c, a, b } => {
                let (cc, pa, pb) = (self.pos(c), self.pos(a), self.pos(b));
                let start = (pa - cc).angle();
                Seg2::Arc {
                    center: cc,
                    radius: pa.distance(cc),
                    start,
                    sweep: arc_sweep(start, (pb - cc).angle()),
                    source,
                }
            }
            Geom::Circle { c, r } => {
                Seg2::Arc { center: self.pos(c), radius: r, start: 0.0, sweep: std::f64::consts::TAU, source }
            }
        };
        if l.reversed {
            s.reversed()
        } else {
            s
        }
    }

    fn link_start(&self, l: Link) -> PointId {
        match self.ends(l.e) {
            Some((a, b)) => {
                if l.reversed {
                    b
                } else {
                    a
                }
            }
            None => ORIGIN,
        }
    }

    fn link_end(&self, l: Link) -> PointId {
        match self.ends(l.e) {
            Some((a, b)) => {
                if l.reversed {
                    a
                } else {
                    b
                }
            }
            None => ORIGIN,
        }
    }

    fn center_of(&self, e: EntityId) -> Option<PointId> {
        match self.entities[e].geom {
            Geom::Arc { c, .. } | Geom::Circle { c, .. } => Some(c),
            Geom::Line { .. } => None,
        }
    }

    /// Offset Entities (sketch-tools.md, "Offset Entities"): copies a chain of lines and
    /// arcs (or one circle) sideways by `opts.distance`. Lines move square to themselves
    /// and arcs keep their centres; offset curves meet at shared points, with sharp
    /// corners where the chain has corners.
    ///
    /// A closed chain offsets outward for a positive distance, an open one to the left of
    /// the chain walked from the first selected curve's start; `reverse` flips either.
    /// One offset dimension (or, without "Add dimensions", a free distance) drives the
    /// whole chain through `SameOffset` relations, so the offset follows the original.
    pub fn offset_entities(
        &mut self,
        ids: &[EntityId],
        opts: OffsetOptions,
    ) -> Result<OffsetResult, SketchError> {
        self.check_entities(ids)?;
        if !opts.distance.is_finite() || opts.distance <= EPS {
            return Err(SketchError::Degenerate);
        }
        let mut sel: Vec<EntityId> = Vec::new();
        for &e in ids {
            let more = if opts.chain { self.select_chain(e) } else { vec![e] };
            for f in more {
                if !sel.contains(&f) {
                    sel.push(f);
                }
            }
        }
        let backup = self.clone();
        let r = self.offset_inner(&sel, opts);
        if r.is_err() {
            *self = backup;
        }
        r
    }

    fn offset_inner(&mut self, sel: &[EntityId], opts: OffsetOptions) -> Result<OffsetResult, SketchError> {
        let (links, closed) = match sel {
            [e] if matches!(self.entities[*e].geom, Geom::Circle { .. }) => {
                (vec![Link { e: *e, reversed: false }], true)
            }
            _ => {
                if sel.iter().any(|&e| matches!(self.entities[e].geom, Geom::Circle { .. })) {
                    return Err(SketchError::Unsupported("a circle offsets on its own"));
                }
                self.chain_order(sel)?
            }
        };
        let segs: Vec<Seg2> = links.iter().enumerate().map(|(i, &l)| self.seg_of(l, i as u32)).collect();
        // The kernel offsets to the right of travel.
        let outward_is_right = Loop2::new(segs.clone()).signed_area() > 0.0;
        let mut d = if closed {
            if outward_is_right {
                opts.distance
            } else {
                -opts.distance
            }
        } else {
            -opts.distance
        };
        if opts.reverse {
            d = -d;
        }
        let sides: Vec<f64> = if opts.bidirectional { vec![d, -d] } else { vec![d] };
        let mut out = OffsetResult { entities: Vec::new(), caps: Vec::new(), dimensions: Vec::new() };
        let mut chain_ends: Vec<(PointId, PointId, EntityId, EntityId)> = Vec::new();
        for &dd in &sides {
            let offs = offset_chain(&segs, closed, dd).map_err(map_err)?;
            let (ents, first_pt, last_pt) =
                self.add_offset_chain(&links, &segs, &offs, closed, &mut out.dimensions, opts.dimensions)?;
            if let (Some(&f), Some(&l)) = (ents.first(), ents.last()) {
                chain_ends.push((first_pt, last_pt, f, l));
            }
            out.entities.extend(ents);
        }
        if !closed {
            let (Some(&first), Some(&last)) = (links.first(), links.last()) else {
                return Err(SketchError::Missing);
            };
            let (Some(s_first), Some(s_last)) = (segs.first(), segs.last()) else {
                return Err(SketchError::Missing);
            };
            let start =
                ChainEnd { point: self.link_start(first), entity: first.e, out: -travel(s_first, false) };
            let end = ChainEnd { point: self.link_end(last), entity: last.e, out: travel(s_last, true) };
            match chain_ends.as_slice() {
                [(s, e, fe, le)] => {
                    out.caps.extend(self.one_side_cap(start, *s, *fe, opts.caps));
                    out.caps.extend(self.one_side_cap(end, *e, *le, opts.caps));
                }
                [(s1, e1, f1, l1), (s2, e2, f2, l2)] => {
                    out.caps.extend(self.two_side_cap(start, (*s1, *f1), (*s2, *f2), opts.caps));
                    out.caps.extend(self.two_side_cap(end, (*e1, *l1), (*e2, *l2), opts.caps));
                }
                _ => {}
            }
        }
        for &e in sel {
            if opts.base_construction {
                self.entities[e].construction = true;
            }
        }
        for &e in &out.entities {
            self.entities[e].construction = opts.offset_construction;
        }
        Ok(out)
    }

    /// Adds one offset chain's curves and their offset relations. Returns the new
    /// entities and the chain's first and last points.
    fn add_offset_chain(
        &mut self,
        links: &[Link],
        segs: &[Seg2],
        offs: &[Seg2],
        closed: bool,
        dims: &mut Vec<usize>,
        dimension: bool,
    ) -> Result<(Vec<EntityId>, PointId, PointId), SketchError> {
        if let ([link], [Seg2::Arc { radius, sweep, .. }]) = (links, offs) {
            if closed && sweep.abs() >= std::f64::consts::TAU - 1e-9 {
                // A whole circle.
                let c = self.center_of(link.e).ok_or(SketchError::Missing)?;
                let circ = self.add_circle(c, *radius);
                let value = self.signed_offset(link.e, circ).unwrap_or(0.0);
                if dimension {
                    dims.push(self.add_constraint(Constraint::Offset { base: link.e, entity: circ, value }));
                }
                return Ok((vec![circ], c, c));
            }
        }
        let n = offs.len();
        let mut pts: Vec<PointId> = offs.iter().map(|s| self.add_point(s.start_point())).collect();
        if closed {
            pts.push(pts[0]);
        } else {
            let last = offs.last().ok_or(SketchError::Missing)?.end_point();
            pts.push(self.add_point(last));
        }
        let mut ents = Vec::with_capacity(n);
        let mut pairs: Vec<(EntityId, EntityId)> = Vec::new();
        for (k, s) in offs.iter().enumerate() {
            let i = s.source() as usize;
            let link = *links.get(i).ok_or(SketchError::Missing)?;
            let orig = segs.get(i).ok_or(SketchError::Missing)?;
            let (p, q) = (pts[k], pts[k + 1]);
            let e = match (*s, *orig) {
                (Seg2::Line { .. }, _) => {
                    let e = self.add_line(p, q);
                    pairs.push((link.e, e));
                    e
                }
                (Seg2::Arc { center, sweep, .. }, Seg2::Arc { center: oc, .. })
                    if center.distance(oc) <= 1e-9 * oc.length().max(1.0) =>
                {
                    let c = self.center_of(link.e).ok_or(SketchError::Missing)?;
                    let e = if sweep > 0.0 { self.add_arc(c, p, q) } else { self.add_arc(c, q, p) };
                    pairs.push((link.e, e));
                    e
                }
                (Seg2::Arc { sweep, .. }, _) => {
                    // A round about a corner of the original chain, where two offset arcs
                    // bend apart and never meet.
                    let c = self.link_start(link);
                    if sweep > 0.0 {
                        self.add_arc(c, p, q)
                    } else {
                        self.add_arc(c, q, p)
                    }
                }
            };
            ents.push(e);
        }
        let Some(&(b0, e0)) = pairs.first() else { return Err(SketchError::Missing) };
        let s0 = self.signed_offset(b0, e0).unwrap_or(0.0);
        if dimension {
            dims.push(self.add_constraint(Constraint::Offset { base: b0, entity: e0, value: s0 }));
        } else if self.entities[b0].is_line() {
            self.add_constraint(Constraint::Parallel { a: b0, b: e0 });
        }
        for &(b, e) in pairs.iter().skip(1) {
            let sk = self.signed_offset(b, e).unwrap_or(0.0);
            self.add_constraint(Constraint::SameOffset {
                base: b,
                entity: e,
                like_base: b0,
                like_entity: e0,
                flip: sk * s0 < 0.0,
            });
        }
        Ok((ents, pts[0], pts[n]))
    }

    /// The relation that keeps line `l`, from a chain end, square to the chain there: square
    /// to an end line, or through the centre of an end arc.
    fn square_to_end(&mut self, l: EntityId, end: ChainEnd) {
        match self.center_of(end.entity) {
            Some(c) => self.add_constraint(Constraint::PointOnLine { p: c, line: l }),
            None => self.add_constraint(Constraint::Perpendicular { a: l, b: end.entity }),
        };
    }

    /// An arc about `c` between `a` and `b` that bulges in direction `out`.
    fn bulging_arc(&mut self, c: PointId, a: PointId, b: PointId, out: Vec2) -> EntityId {
        let o = self.pos(c);
        if (self.pos(a) - o).cross(out) > 0.0 {
            self.add_arc(c, a, b)
        } else {
            self.add_arc(c, b, a)
        }
    }

    /// The cap at one end of a one-directional offset of an open chain, from the
    /// original's end to the offset's end (sketch-tools.md, "Geometry", Decision on cap
    /// shapes). With no cap, a construction line there still holds the offset's end.
    fn one_side_cap(&mut self, end: ChainEnd, o: PointId, _off: EntityId, caps: CapEnds) -> Vec<EntityId> {
        let l = self.add_line(end.point, o);
        self.square_to_end(l, end);
        match caps {
            CapEnds::Lines => vec![l],
            CapEnds::None => {
                self.entities[l].construction = true;
                vec![l]
            }
            CapEnds::Arcs => {
                self.entities[l].construction = true;
                let mid = (self.pos(end.point) + self.pos(o)) * 0.5;
                let m = self.add_point(mid);
                self.add_constraint(Constraint::Midpoint { p: m, line: l });
                let arc = self.bulging_arc(m, end.point, o, end.out);
                vec![l, arc]
            }
        }
    }

    /// The cap at one end of a bi-directional offset of an open chain, joining the two
    /// offsets' ends: a line across, a half circle about the original's end, or (with no
    /// cap) construction lines that hold both ends.
    fn two_side_cap(
        &mut self,
        end: ChainEnd,
        left: (PointId, EntityId),
        right: (PointId, EntityId),
        caps: CapEnds,
    ) -> Vec<EntityId> {
        match caps {
            CapEnds::None => {
                let mut v = Vec::new();
                for (p, _) in [left, right] {
                    let l = self.add_line(end.point, p);
                    self.entities[l].construction = true;
                    self.square_to_end(l, end);
                    v.push(l);
                }
                v
            }
            CapEnds::Lines => {
                let l = self.add_line(left.0, right.0);
                self.add_constraint(Constraint::PointOnLine { p: end.point, line: l });
                self.square_to_end(l, end);
                vec![l]
            }
            CapEnds::Arcs => {
                let arc = self.bulging_arc(end.point, left.0, right.0, end.out);
                for (_, off) in [left, right] {
                    let rel = if self.entities[off].is_line() {
                        Constraint::Tangent { line: off, entity: arc }
                    } else {
                        Constraint::TangentCircles { a: off, b: arc, inside: self.circles_inside(off, arc) }
                    };
                    self.add_constraint(rel);
                }
                vec![arc]
            }
        }
    }

    /// Copies entities with their points mapped through `place`; points `share` maps to
    /// themselves are not copied. Returns the copies and the point map (old to new).
    fn copy_entities(
        &mut self,
        ids: &[EntityId],
        place: &dyn Fn(Vec2) -> Vec2,
        share: &dyn Fn(PointId) -> bool,
        reverse_arcs: bool,
    ) -> (Vec<EntityId>, HashMap<PointId, PointId>) {
        let mut pmap: HashMap<PointId, PointId> = HashMap::new();
        let mut out = Vec::with_capacity(ids.len());
        for &e in ids {
            for p in self.entities[e].points() {
                if let std::collections::hash_map::Entry::Vacant(v) = pmap.entry(p) {
                    let q = if share(p) {
                        p
                    } else {
                        let at = place(self.points[p].pos());
                        self.points.push(crate::sketch::Point { x: at.x, y: at.y });
                        self.points.len() - 1
                    };
                    v.insert(q);
                }
            }
            let m = |p: PointId| pmap.get(&p).copied().unwrap_or(p);
            let g = match self.entities[e].geom {
                Geom::Line { a, b } => Geom::Line { a: m(a), b: m(b) },
                Geom::Circle { c, r } => Geom::Circle { c: m(c), r },
                Geom::Arc { c, a, b } if reverse_arcs => Geom::Arc { c: m(c), a: m(b), b: m(a) },
                Geom::Arc { c, a, b } => Geom::Arc { c: m(c), a: m(a), b: m(b) },
            };
            let construction = self.entities[e].construction;
            let ne = self.add_entity(g);
            self.entities[ne].construction = construction;
            out.push(ne);
        }
        (out, pmap)
    }

    /// Mirror Entities about line `axis` (sketch-tools.md, "Mirror Entities").
    ///
    /// With `copy`, mirrored copies are added: every pair of corresponding points gets a
    /// `Symmetric` relation and mirrored circles an `Equal` radius, so a change on one side
    /// shows on the other. (Arcs need no `Equal`: their symmetric end points and centres
    /// already fix the radius.) Points on the mirror line are shared, not copied, so a
    /// profile mirrored about one of its own ends stays closed. Without `copy`, the
    /// mirrored curves replace the originals and take mirrored images of the relations
    /// among them. Returns the mirrored entities, after renumbering.
    pub fn mirror_entities(
        &mut self,
        ids: &[EntityId],
        axis: EntityId,
        copy: bool,
    ) -> Result<(Vec<EntityId>, Renumbering), SketchError> {
        self.check_entities(ids)?;
        self.check_entities(&[axis])?;
        let Geom::Line { a: aa, b: ab } = self.entities[axis].geom else {
            return Err(SketchError::Unsupported("mirror about a line"));
        };
        let (pa, pb) = (self.pos(aa), self.pos(ab));
        if pa.distance(pb) <= EPS {
            return Err(SketchError::Degenerate);
        }
        let ids: Vec<EntityId> = ids.iter().copied().filter(|&e| e != axis).collect();
        if ids.is_empty() {
            return Err(SketchError::Unsupported("pick something to mirror besides the mirror line"));
        }
        let u = (pb - pa).normalized();
        let scale = pa.length().max(pb.length()).max(1.0);
        let on_axis = |sk: &Sketch, p: PointId| -> bool { u.cross(sk.pos(p) - pa).abs() <= 1e-9 * scale };
        let snapshot = self.clone();
        let share = |p: PointId| on_axis(&snapshot, p);
        let (new, pmap) = self.copy_entities(&ids, &|q| reflect(q, pa, pb), &share, true);
        if copy {
            let mut pairs: Vec<(PointId, PointId)> =
                pmap.iter().filter(|(p, q)| p != q).map(|(p, q)| (*p, *q)).collect();
            pairs.sort_unstable();
            for (p, q) in pairs {
                self.add_constraint(Constraint::Symmetric { p, q, line: axis });
            }
            for (&e, &ne) in ids.iter().zip(&new) {
                if matches!(self.entities[e].geom, Geom::Circle { .. }) {
                    self.add_constraint(Constraint::Equal { a: e, b: ne });
                }
            }
            return Ok((new, Renumbering::identity(self)));
        }
        // Replace: images of the relations among the originals, then delete them.
        let emap: HashMap<EntityId, EntityId> = ids.iter().copied().zip(new.iter().copied()).collect();
        let axis_deg = u.angle().to_degrees();
        let is_line = |e: EntityId| snapshot.entities.get(e).is_some_and(|x| x.is_line());
        let images: Vec<Constraint> = self
            .constraints
            .iter()
            .filter_map(|c| mirror_constraint(c, &pmap, &emap, axis_deg, (pa, pb), &is_line))
            .collect();
        for c in images {
            self.add_constraint(c);
        }
        let ren = self.delete_entities_renumbered(&ids);
        let moved = new.iter().filter_map(|&e| ren.entity(e)).collect();
        Ok((moved, ren))
    }

    fn seed_points(&self, ids: &[EntityId]) -> Vec<PointId> {
        let mut pts = Vec::new();
        for &e in ids {
            for p in self.entities[e].points() {
                if !pts.contains(&p) {
                    pts.push(p);
                }
            }
        }
        pts
    }

    /// Linear sketch pattern (sketch-tools.md, "Linear"): instance (i, j) is the seed moved
    /// i spacings along direction 1 and j along direction 2. Construction lines from the
    /// seed carry the two direction vectors; every instance point is tied to its seed point
    /// by a `PatternPoint` relation and circles get an `Equal` radius, so the instances
    /// follow the seed and the spacing dimensions.
    pub fn linear_pattern(
        &mut self,
        ids: &[EntityId],
        p: &LinearPattern,
    ) -> Result<PatternResult, SketchError> {
        self.check_entities(ids)?;
        let [n1, n2] = p.count;
        if ids.is_empty()
            || n1 == 0
            || n2 == 0
            || u64::from(n1) * u64::from(n2) < 2
            || u64::from(n1) * u64::from(n2) > 10_000
        {
            return Err(SketchError::OutOfRange("a pattern has 2 to 10000 instances"));
        }
        let v1 = Vec2::from_angle(p.degrees[0].to_radians()) * p.spacing[0];
        let v2_ = Vec2::from_angle(p.degrees[1].to_radians()) * p.spacing[1];
        if (n1 > 1 && v1.length() <= EPS)
            || (n2 > 1 && v2_.length() <= EPS)
            || !v1.x.is_finite()
            || !v2_.x.is_finite()
        {
            return Err(SketchError::Degenerate);
        }
        let seed_pts = self.seed_points(ids);
        let s = *seed_pts.first().ok_or(SketchError::Missing)?;
        let sp = self.pos(s);
        let mut res =
            PatternResult { instances: Vec::new(), construction: Vec::new(), dimensions: Vec::new() };
        let d1_end = self.add_point(sp + v1);
        let d1 = self.add_line(s, d1_end);
        self.entities[d1].construction = true;
        res.construction.push(d1);
        let d2 = if n2 > 1 {
            let e = self.add_point(sp + v2_);
            let d = self.add_line(s, e);
            self.entities[d].construction = true;
            res.construction.push(d);
            d
        } else {
            d1
        };
        for i in 0..n1 {
            for j in 0..n2 {
                if (i, j) == (0, 0) || p.skip.contains(&(i, j)) {
                    continue;
                }
                let shift = v1 * f64::from(i) + v2_ * f64::from(j);
                let (inst, pmap) = self.copy_entities(ids, &|q| q + shift, &|_| false, false);
                for &sp in &seed_pts {
                    if let Some(&ip) = pmap.get(&sp) {
                        self.add_constraint(Constraint::PatternPoint {
                            seed: sp,
                            inst: ip,
                            d1,
                            i,
                            d2,
                            j: if d2 == d1 { 0 } else { j },
                        });
                    }
                }
                self.equal_circles(ids, &inst);
                res.instances.push(inst);
            }
        }
        if p.dimension_spacing {
            res.dimensions.push(self.add_constraint(Constraint::Length { line: d1, value: p.spacing[0] }));
            if d2 != d1 {
                res.dimensions
                    .push(self.add_constraint(Constraint::Length { line: d2, value: p.spacing[1] }));
            }
        }
        if p.fix_direction {
            let a = p.degrees[0].rem_euclid(180.0);
            if a.abs() < 1e-9 || (a - 180.0).abs() < 1e-9 {
                self.add_constraint(Constraint::Horizontal { line: d1 });
            } else if (a - 90.0).abs() < 1e-9 {
                self.add_constraint(Constraint::Vertical { line: d1 });
            }
        }
        if p.dimension_angle && d2 != d1 {
            let deg = self.measure(&Constraint::Angle { a: d1, b: d2, degrees: 0.0 }).unwrap_or(90.0);
            res.dimensions.push(self.add_constraint(Constraint::Angle { a: d1, b: d2, degrees: deg }));
        }
        Ok(res)
    }

    fn equal_circles(&mut self, seed: &[EntityId], inst: &[EntityId]) {
        for (&e, &ne) in seed.iter().zip(inst) {
            if matches!(self.entities[e].geom, Geom::Circle { .. }) {
                self.add_constraint(Constraint::Equal { a: e, b: ne });
            }
        }
    }

    /// Circular sketch pattern (sketch-tools.md, "Circular"): instance k is the seed turned
    /// k steps about the centre, a step being the total angle over the count when the
    /// total is 360° and over one less otherwise. Construction lines from the centre to the
    /// seed and to where the first instance would be carry the radius and the step angle;
    /// instance points are tied to the seed by `RotatedPoint`.
    pub fn circular_pattern(
        &mut self,
        ids: &[EntityId],
        p: &CircularPattern,
    ) -> Result<PatternResult, SketchError> {
        self.check_entities(ids)?;
        if ids.is_empty() || p.count < 2 || p.count > 10_000 {
            return Err(SketchError::OutOfRange("a pattern has 2 to 10000 instances"));
        }
        if !p.total_degrees.is_finite()
            || p.total_degrees.abs() <= EPS
            || !p.center.x.is_finite()
            || !p.center.y.is_finite()
        {
            return Err(SketchError::Degenerate);
        }
        let n = f64::from(p.count);
        let mut step = if (p.total_degrees.abs() - 360.0).abs() < 1e-9 {
            p.total_degrees / n
        } else {
            p.total_degrees / (n - 1.0)
        };
        if p.reverse {
            step = -step;
        }
        let seed_pts = self.seed_points(ids);
        let s = *seed_pts.first().ok_or(SketchError::Missing)?;
        let sp = self.pos(s);
        if sp.distance(p.center) <= EPS {
            return Err(SketchError::Degenerate);
        }
        let c = if p.center.distance(self.pos(ORIGIN)) <= EPS { ORIGIN } else { self.add_point(p.center) };
        let k0 = self.add_line(c, s);
        let k1_end = self.add_point(rotate_about(sp, p.center, step.to_radians()));
        let k1 = self.add_line(c, k1_end);
        for k in [k0, k1] {
            self.entities[k].construction = true;
        }
        self.add_constraint(Constraint::Equal { a: k0, b: k1 });
        let mut res =
            PatternResult { instances: Vec::new(), construction: vec![k0, k1], dimensions: Vec::new() };
        for k in 1..p.count {
            if p.skip.contains(&k) {
                continue;
            }
            let ang = (step * f64::from(k)).to_radians();
            let (inst, pmap) =
                self.copy_entities(ids, &|q| rotate_about(q, p.center, ang), &|_| false, false);
            for &sp in &seed_pts {
                if let Some(&ip) = pmap.get(&sp) {
                    self.add_constraint(Constraint::RotatedPoint { seed: sp, inst: ip, k0, k1, k });
                }
            }
            self.equal_circles(ids, &inst);
            res.instances.push(inst);
        }
        if p.dimension_radius {
            res.dimensions
                .push(self.add_constraint(Constraint::Length { line: k0, value: sp.distance(p.center) }));
        }
        if p.dimension_angle {
            let (a, b) = if step > 0.0 { (k0, k1) } else { (k1, k0) };
            res.dimensions.push(self.add_constraint(Constraint::RayAngle {
                a,
                b,
                flip_a: false,
                flip_b: false,
                degrees: step.abs(),
            }));
        }
        Ok(res)
    }

    /// Move, Copy, Rotate or Scale entities (sketch-tools.md, "Move, Copy, Rotate, Scale
    /// and Stretch"). The points move first; the caller solves afterwards.
    ///
    /// Moving takes dimensions and relations among the moved entities along (a scale
    /// multiplies their lengths, a rotation by quarter turns swaps horizontal and
    /// vertical). Relations to unmoved geometry, and points shared with it, are broken
    /// unless `keep_relations` is set; then they stay and the sketch may over-define.
    /// Copying adds `copies` transformed copies, each with the relations among the
    /// originals, and leaves the originals alone. Returns the moved or new entities.
    pub fn transform_entities(
        &mut self,
        ids: &[EntityId],
        xf: Transform2,
        opts: TransformOptions,
    ) -> Result<(Vec<EntityId>, Renumbering), SketchError> {
        self.check_entities(ids)?;
        if ids.is_empty() {
            return Err(SketchError::Unsupported("select the entities to move"));
        }
        if let Transform2::Scale { factor, .. } = xf {
            if !factor.is_finite() || factor <= EPS {
                return Err(SketchError::Degenerate);
            }
        }
        let mut ids: Vec<EntityId> = ids.to_vec();
        ids.sort_unstable();
        ids.dedup();
        if opts.copy {
            let mut all = Vec::new();
            for k in 1..=opts.copies.max(1) {
                let place = |q: Vec2| (0..k).fold(q, |q, _| xf.apply(q));
                let (new, pmap) = self.copy_entities(&ids, &place, &|_| false, false);
                let emap: HashMap<EntityId, EntityId> =
                    ids.iter().copied().zip(new.iter().copied()).collect();
                let copies: Vec<Constraint> = self
                    .constraints
                    .iter()
                    .filter_map(|c| {
                        (0..k)
                            .try_fold(*c, |c, _| moved_constraint(&c, &xf))
                            .and_then(|c| map_constraint(&c, &pmap, &emap))
                    })
                    .collect();
                for c in copies {
                    self.add_constraint(c);
                }
                all.extend(new);
            }
            return Ok((all, Renumbering::identity(self)));
        }
        let pts = self.seed_points(&ids);
        let others_use = |sk: &Sketch, p: PointId| sk.entities_using(p).iter().any(|e| !ids.contains(e));
        // Points the moving entities cannot take with them get a copy of their own.
        let mut moved: Vec<PointId> = Vec::new();
        for p in pts {
            if p == ORIGIN || (!opts.keep_relations && others_use(self, p)) {
                let q = self.add_point(self.pos(p));
                for &e in &ids {
                    self.entities[e].remap_points(&|x| if x == p { q } else { x });
                }
                moved.push(q);
            } else {
                moved.push(p);
            }
        }
        for &p in &moved {
            let at = xf.apply(self.pos(p));
            self.points[p].x = at.x;
            self.points[p].y = at.y;
        }
        let inside = |c: &Constraint| {
            c.points().iter().all(|p| moved.contains(p)) && c.entities().iter().all(|e| ids.contains(e))
        };
        let touches = |c: &Constraint| {
            c.points().iter().any(|p| moved.contains(p)) || c.entities().iter().any(|e| ids.contains(e))
        };
        let mut keep = Vec::with_capacity(self.constraints.len());
        for c in self.constraints.iter_mut() {
            if inside(c) {
                match moved_constraint(c, &xf) {
                    Some(m) => {
                        *c = m;
                        keep.push(true);
                    }
                    None => keep.push(false),
                }
            } else {
                keep.push(!touches(c) || opts.keep_relations);
            }
        }
        let mut i = 0;
        let cmap = self.retain_constraints(|_| {
            i += 1;
            keep[i - 1]
        });
        let ren = Renumbering {
            points: (0..self.points.len()).map(Some).collect(),
            entities: (0..self.entities.len()).map(Some).collect(),
            constraints: cmap,
        };
        Ok((ids, ren))
    }
}

/// Relation `c` with its points and entities renamed through the maps. `None` if it
/// mentions something not in them.
fn map_constraint(
    c: &Constraint,
    pmap: &HashMap<PointId, PointId>,
    emap: &HashMap<EntityId, EntityId>,
) -> Option<Constraint> {
    if !c.points().iter().all(|p| pmap.contains_key(p)) || !c.entities().iter().all(|e| emap.contains_key(e))
    {
        return None;
    }
    if c.points().is_empty() && c.entities().is_empty() {
        return None;
    }
    let mut out = *c;
    out.remap(&|p| pmap.get(&p).copied().unwrap_or(p), &|e| emap.get(&e).copied().unwrap_or(e));
    Some(out)
}

/// Relation `c` after its geometry is moved by `xf`, or `None` if it no longer holds (a
/// horizontal line turned by 30°, say).
fn moved_constraint(c: &Constraint, xf: &Transform2) -> Option<Constraint> {
    use Constraint::*;
    let f = xf.length_factor();
    let quarter = xf.quarter_turns();
    let mut c = *c;
    match &mut c {
        Fix { x, y, .. } => {
            let q = xf.apply(v2(*x, *y));
            *x = q.x;
            *y = q.y;
        }
        FixLine { x, y, degrees, .. } => {
            let q = xf.apply(v2(*x, *y));
            *x = q.x;
            *y = q.y;
            *degrees = xf.turn(*degrees);
        }
        FixRound { x, y, r, .. } => {
            let q = xf.apply(v2(*x, *y));
            *x = q.x;
            *y = q.y;
            *r *= f;
        }
        Horizontal { line } => match quarter? {
            0 | 2 => {}
            _ => return Some(Vertical { line: *line }),
        },
        Vertical { line } => match quarter? {
            0 | 2 => {}
            _ => return Some(Horizontal { line: *line }),
        },
        HorizontalPoints { p, q } => match quarter? {
            0 | 2 => {}
            _ => return Some(VerticalPoints { p: *p, q: *q }),
        },
        VerticalPoints { p, q } => match quarter? {
            0 | 2 => {}
            _ => return Some(HorizontalPoints { p: *p, q: *q }),
        },
        HDistance { p, q, value } => {
            let v = *value * f;
            return match quarter? {
                0 => Some(HDistance { p: *p, q: *q, value: v }),
                1 => Some(VDistance { p: *p, q: *q, value: v }),
                2 => Some(HDistance { p: *p, q: *q, value: -v }),
                _ => Some(VDistance { p: *p, q: *q, value: -v }),
            };
        }
        VDistance { p, q, value } => {
            let v = *value * f;
            return match quarter? {
                0 => Some(VDistance { p: *p, q: *q, value: v }),
                1 => Some(HDistance { p: *p, q: *q, value: -v }),
                2 => Some(VDistance { p: *p, q: *q, value: -v }),
                _ => Some(HDistance { p: *p, q: *q, value: v }),
            };
        }
        Distance { value, .. }
        | Length { value, .. }
        | Radius { value, .. }
        | Diameter { value, .. }
        | PointLineDistance { value, .. }
        | ArcLength { value, .. }
        | AxisDiameter { value, .. }
        | Offset { value, .. } => *value *= f,
        _ => {}
    }
    Some(c)
}

/// The image of relation `c` in a mirror about the line through `pa` and `pb` at
/// `axis_deg`, renamed through the maps. `None` when it mentions geometry outside the
/// maps or has no mirror image AnvilCAD can state (a horizontal line mirrored about a
/// slanted line, say).
fn mirror_constraint(
    c: &Constraint,
    pmap: &HashMap<PointId, PointId>,
    emap: &HashMap<EntityId, EntityId>,
    axis_deg: f64,
    (pa, pb): (Vec2, Vec2),
    is_line: &dyn Fn(EntityId) -> bool,
) -> Option<Constraint> {
    use Constraint::*;
    let line_offset = matches!(*c, Offset { base, .. } if is_line(base));
    let m = map_constraint(c, pmap, emap)?;
    let a = axis_deg.rem_euclid(180.0);
    let axis_h = a.abs() < 1e-9 || (a - 180.0).abs() < 1e-9;
    let axis_v = (a - 90.0).abs() < 1e-9;
    Some(match m {
        Horizontal { .. } | Vertical { .. } | HorizontalPoints { .. } | VerticalPoints { .. } => {
            if axis_h || axis_v {
                m
            } else {
                return None;
            }
        }
        HDistance { p, q, value } if axis_v => HDistance { p, q, value: -value },
        HDistance { .. } if axis_h => m,
        VDistance { p, q, value } if axis_h => VDistance { p, q, value: -value },
        VDistance { .. } if axis_v => m,
        HDistance { .. } | VDistance { .. } => return None,
        Fix { p, x, y } => {
            let r = reflect(v2(x, y), pa, pb);
            Fix { p, x: r.x, y: r.y }
        }
        FixLine { line, x, y, degrees } => {
            let r = reflect(v2(x, y), pa, pb);
            FixLine { line, x: r.x, y: r.y, degrees: 2.0 * axis_deg - degrees }
        }
        FixRound { entity, x, y, r } => {
            let q = reflect(v2(x, y), pa, pb);
            FixRound { entity, x: q.x, y: q.y, r }
        }
        Angle { a, b, degrees } => Angle { a, b, degrees: -degrees },
        RayAngle { a, b, flip_a, flip_b, degrees } => {
            RayAngle { a: b, b: a, flip_a: flip_b, flip_b: flip_a, degrees }
        }
        // A mirrored line runs the same way from its mirrored first point, so what was on
        // its left is on its right.
        Offset { base, entity, value } if line_offset => Offset { base, entity, value: -value },
        PatternPoint { .. } | RotatedPoint { .. } => return None,
        other => other,
    })
}
