//! Rolling-ball fillets on any edge between two faces.
//!
//! The round is what a ball of the fillet's radius sweeps while rolling along the edge in
//! contact with both faces. Its centre runs where the two faces, offset by the radius
//! (into the part at a convex edge, out of it at a concave one), meet. Each cross-section
//! is exact: the section is solved in the plane square to the edge, by Newton's method on
//! the two faces' signed distances, and between the ball's two contact points the round
//! is a circular arc about the centre (a rational quadratic, exactly). The sections are
//! joined along the edge into a B-spline surface (cubic along the edge, rational
//! quadratic across it), with more sections where the surface between them strays
//! from the exact sections by more than [`FIT`].
//!
//! As for the other blends ([`crate::blend`]), the round is applied as an exact boolean
//! with a tool: the round closed by sides pushed off the faces, swept along the edge with
//! flat ends square to the ball's path. The round touches the two faces along its
//! contact curves by construction, so the boolean is told so rather than tracing a
//! touching contact.
//!
//! The behaviour is specified in `docs/re/fillet-curved-edges.md`.

use crate::geom::{Curve, Frame, Surface};
use crate::patch::SplineSurface;
use crate::spline::Spline;
use crate::topo::{Coedge, Edge, EdgeId, Face, FaceTag, Loop, Solid};
use crate::{KernelError, Result, boolean, check};
use glam::DVec3;
use std::sync::Arc;

/// How far the round may stray from the exact rolling-ball sections (mm).
const FIT: f64 = 1e-7;

/// A blend tool, whether its edge is convex (the tool is cut away) rather than concave
/// (the tool is added), and the (solid face, tool face) pairs it touches by construction.
pub type Tool = (Solid, bool, Vec<(usize, usize)>);

/// Most sections along one edge.
const MAX_SECTIONS: usize = 4000;

fn refuse<T>(msg: impl Into<String>) -> Result<T> {
    Err(KernelError::Unsupported(msg.into()))
}

/// Signed distance from `x` to a face's surface, positive outside the part.
fn outside(face: &Face, x: DVec3) -> f64 {
    let d = match &face.surface {
        Surface::BSpline(sp) => {
            // On the patch extended a little past its edges, where an edge's end lies.
            let st = sp.closest_beyond(x);
            (x - sp.eval_st(st)).dot(sp.normal_st(st))
        }
        s => crate::freeform::signed(s, x),
    };
    if face.reversed { -d } else { d }
}

/// The point of a face's surface nearest `x`, and the outward normal there.
fn foot(face: &Face, x: DVec3) -> (DVec3, DVec3) {
    match &face.surface {
        Surface::BSpline(sp) => {
            let st = sp.closest_beyond(x);
            let n = sp.normal_st(st);
            (sp.eval_st(st), if face.reversed { -n } else { n })
        }
        _ => {
            let n = face.normal_at(x);
            (x - n * outside(face, x), n)
        }
    }
}

/// One cross-section of the tool: the ball's centre, its contact points on face A and B,
/// the corner where the round's end tangents meet (with the arc's middle weight), and
/// the corners of the sides pushed off the faces.
#[derive(Clone, Copy, Debug)]
struct Section {
    centre: DVec3,
    a: DVec3,
    b: DVec3,
    corner: DVec3,
    weight: f64,
    a_off: DVec3,
    b_off: DVec3,
    corner_off: DVec3,
}

impl Section {
    /// The section's rows in homogeneous form (point times weight, weight), in profile
    /// order: contact A, the arc's corner, contact B, then the pushed sides.
    fn rows(&self) -> [(DVec3, f64); 6] {
        [
            (self.a, 1.0),
            (self.corner * self.weight, self.weight),
            (self.b, 1.0),
            (self.b_off, 1.0),
            (self.corner_off, 1.0),
            (self.a_off, 1.0),
        ]
    }
}

struct Roller<'a> {
    solid: &'a Solid,
    edge: &'a Edge,
    fa: usize,
    fb: usize,
    radius: f64,
    /// The centre's signed distance from both faces: inside at a convex edge.
    target: f64,
    /// How far the sides are pushed off the faces.
    push: f64,
    /// The edge's parameter range and its direction of travel.
    lo: f64,
    hi: f64,
    dir: f64,
    /// The edge closes on itself (round a hole's rim): its curve wraps instead of
    /// carrying on past its ends.
    closed: bool,
    size: f64,
}

impl Roller<'_> {
    /// A point of the edge at parameter `t` (in the direction of travel), carried on in a
    /// straight line past its ends.
    fn point(&self, t: f64) -> (DVec3, DVec3) {
        let c = &self.edge.curve;
        if self.closed {
            let span = self.hi - self.lo;
            let wrap = |u: f64| self.lo + (u - self.lo).rem_euclid(span);
            let h = span * 1e-6;
            let d = (c.eval(self.at(wrap(t + h))) - c.eval(self.at(wrap(t - h)))) / (2.0 * h);
            return (c.eval(self.at(wrap(t))), d.normalize_or_zero());
        }
        let clamp = t.clamp(self.lo, self.hi);
        let h = (self.hi - self.lo) * 1e-6;
        let (u0, u1) = ((clamp - h).max(self.lo), (clamp + h).min(self.hi));
        let tangent = ((c.eval(self.at(u1)) - c.eval(self.at(u0))) / (u1 - u0)).normalize_or_zero();
        let speed = ((c.eval(self.at(u1)) - c.eval(self.at(u0))) / (u1 - u0)).length();
        (c.eval(self.at(clamp)) + tangent * (speed * (t - clamp)), tangent)
    }

    /// The edge's own parameter for travel parameter `t` (which runs from `lo` to `hi`
    /// along the direction of travel).
    fn at(&self, t: f64) -> f64 {
        if self.dir > 0.0 { t } else { self.lo + self.hi - t }
    }

    /// The exact section in the plane square to the edge at `t`, its centre found from
    /// `guess`.
    fn section(&self, t: f64, guess: DVec3) -> Option<Section> {
        let (e, tangent) = self.point(t);
        let u1 = tangent.any_orthonormal_vector();
        let u2 = tangent.cross(u1);
        let (fa, fb) = (&self.solid.faces[self.fa], &self.solid.faces[self.fb]);
        let mut x = glam::DVec2::new((guess - e).dot(u1), (guess - e).dot(u2));
        let tol = 1e-13 * self.size;
        let mut ok = false;
        for _ in 0..60 {
            let c = e + u1 * x.x + u2 * x.y;
            let (ra, rb) = (outside(fa, c) - self.target, outside(fb, c) - self.target);
            if ra.abs() < tol && rb.abs() < tol {
                ok = true;
                break;
            }
            let (na, nb) = (foot(fa, c).1, foot(fb, c).1);
            let (j11, j12, j21, j22) = (na.dot(u1), na.dot(u2), nb.dot(u1), nb.dot(u2));
            let det = j11 * j22 - j12 * j21;
            if det.abs() < 1e-12 {
                return None;
            }
            x -= glam::DVec2::new((j22 * ra - j12 * rb) / det, (j11 * rb - j21 * ra) / det);
        }
        if !ok {
            return None;
        }
        let centre = e + u1 * x.x + u2 * x.y;
        let (a, _) = foot(fa, centre);
        let (b, _) = foot(fb, centre);
        let r = self.radius;
        let (da, db) = ((a - centre) / r, (b - centre) / r);
        let half = 0.5 * da.angle_between(db);
        if !(half > 1e-6 && half < 0.5 * std::f64::consts::PI - 1e-6) {
            return None;
        }
        let bis = (da + db).normalize();
        let corner = centre + bis * (r / half.cos());
        Some(Section {
            centre,
            a,
            b,
            corner,
            weight: half.cos(),
            a_off: a + da * self.push,
            b_off: b + db * self.push,
            corner_off: corner + bis * (self.push / half.cos()),
        })
    }
}

/// The tool for a rolling-ball fillet of `radius` on `edge`, whether the edge is convex
/// (the tool is cut away) or concave (the tool is added), and the (solid face, tool face)
/// pairs that touch along the round's contact curves. Tool faces are tagged as the other
/// blends' are (`tag_base`, `tag_base + 2` for the round, `tag_base + 4`).
pub fn rolling_tool(solid: &Solid, edge: EdgeId, radius: f64, tag_base: u32) -> Result<Tool> {
    if !(radius.is_finite() && radius > 0.0) {
        return Err(KernelError::InvalidInput("the size must be a positive number".into()));
    }
    let e = solid.edge(edge).ok_or_else(|| KernelError::InvalidInput("the edge does not exist".into()))?;
    // A closed edge (a hole's rim) is rounded all the way round, with no ends.
    let closed = e.start == e.end;
    let size = solid.bounds().map_or(1.0, |(lo, hi)| (hi - lo).length().max(1.0));
    let mut roller = Roller {
        solid,
        edge: e,
        fa: 0,
        fb: 0,
        radius,
        target: 0.0,
        push: 0.5 * radius,
        lo: e.t0.min(e.t1),
        hi: e.t0.max(e.t1),
        dir: (e.t1 - e.t0).signum(),
        closed: e.start == e.end,
        size,
    };
    // The faces at the edge's middle, which way the edge bends, and a first centre.
    let mid = 0.5 * (roller.lo + roller.hi);
    let (pm, tm) = roller.point(mid);
    let sides = crate::blend::edge_sides(solid, edge, pm, tm);
    let [(fa, _, wa), (fb, nb, wb)] = sides[..] else {
        return Err(KernelError::InvalidInput("the edge is not shared by exactly two faces".into()));
    };
    let bend = wa.dot(nb);
    if bend.abs() < 1e-6 {
        return Err(KernelError::InvalidInput(
            "the faces at this edge are tangent; there is nothing to blend".into(),
        ));
    }
    let convex = bend < 0.0;
    roller.fa = fa;
    roller.fb = fb;
    roller.target = if convex { -radius } else { radius };
    let phi = wa.dot(wb).clamp(-1.0, 1.0).acos();
    let first = pm + (wa + wb).normalize() * (radius / (0.5 * phi).sin());

    // Carry the round past an end only where nothing lies beyond it (as the other blends
    // do), never where the edge runs smoothly on into another.
    let margin = 2.0 * radius;
    let speed = |t: f64| {
        let h = (roller.hi - roller.lo) * 1e-6;
        (e.curve.eval(roller.at((t + h).min(roller.hi))) - e.curve.eval(roller.at((t - h).max(roller.lo))))
            .length()
            / (2.0 * h)
    };
    let extend = |t: f64, sign: f64| -> Result<f64> {
        let (end, tangent) = roller.point(t);
        let out = tangent * sign;
        if crate::blend::runs_on(solid, edge, end, out) {
            return Ok(0.0);
        }
        let probe = end + out * (0.5 * margin) + (wa + wb) * (0.25 * radius);
        let inside = boolean::contains_point(solid, probe)?;
        Ok(if inside != convex { margin / speed(t).max(1e-300) } else { 0.0 })
    };
    let (s0, s1) = if closed {
        (roller.lo, roller.hi)
    } else {
        (roller.lo - extend(roller.lo, -1.0)?, roller.hi + extend(roller.hi, 1.0)?)
    };

    // Sections, refined until the cubic between them follows the exact ones.
    let mut ts: Vec<f64> = (0..=8).map(|k| s0 + (s1 - s0) * k as f64 / 8.0).collect();
    let solve = |ts: &[f64]| -> Result<Vec<Section>> {
        let mut out: Vec<Section> = Vec::with_capacity(ts.len());
        let mut guess = first;
        // From the middle outwards, so each guess is a neighbour's centre.
        let m = ts.iter().position(|t| *t >= mid).unwrap_or(0);
        let mut by_index: Vec<Option<Section>> = vec![None; ts.len()];
        for k in (m..ts.len()).chain((0..m).rev()) {
            if k + 1 == m {
                guess = by_index[m].map_or(first, |s| s.centre);
            }
            let s = roller.section(ts[k], guess).ok_or_else(|| {
                KernelError::Unsupported(
                    "the ball cannot roll along this edge (the faces do not leave room for it)".into(),
                )
            })?;
            guess = s.centre;
            by_index[k] = Some(s);
        }
        for s in by_index {
            out.push(s.ok_or_else(|| KernelError::Unsupported("a section is missing".into()))?);
        }
        // Round a closed edge the last section is the first, exactly.
        if closed && let (Some(first), Some(last)) = (out.first().copied(), out.last_mut()) {
            *last = first;
        }
        Ok(out)
    };
    let h = (s1 - s0) * 1e-5;
    let rows_at =
        |t: f64, near: DVec3| -> Option<[(DVec3, f64); 6]> { roller.section(t, near).map(|s| s.rows()) };
    let mut sections = solve(&ts)?;
    loop {
        if ts.len() > MAX_SECTIONS {
            return refuse("a rolling-ball fillet needs too many sections");
        }
        // Rates of change of each row, by central differences of exact sections.
        let mut derivs = Vec::with_capacity(ts.len());
        for (t, s) in ts.iter().zip(&sections) {
            let (a, b) = (rows_at(t - h, s.centre), rows_at(t + h, s.centre));
            let (Some(a), Some(b)) = (a, b) else { return refuse("the ball cannot roll along this edge") };
            derivs.push(std::array::from_fn::<(DVec3, f64), 6, _>(|j| {
                ((b[j].0 - a[j].0) / (2.0 * h), (b[j].1 - a[j].1) / (2.0 * h))
            }));
        }
        if closed && let (Some(first), Some(last)) = (derivs.first().copied(), derivs.last_mut()) {
            *last = first;
        }
        // Check each interval's middle against the exact section.
        let mut split = Vec::new();
        for k in 0..ts.len() - 1 {
            let (t0, t1) = (ts[k], ts[k + 1]);
            let dt = t1 - t0;
            let tm = 0.5 * (t0 + t1);
            let Some(exact) = roller.section(tm, sections[k].centre) else {
                return refuse("the ball cannot roll along this edge");
            };
            let (r0, r1) = (sections[k].rows(), sections[k + 1].rows());
            let hermite = |j: usize| {
                let (p0, w0) = r0[j];
                let (p1, w1) = r1[j];
                let (d0, dw0) = derivs[k][j];
                let (d1, dw1) = derivs[k + 1][j];
                // Cubic Hermite at the middle.
                let p = (p0 + p1) * 0.5 + (d0 - d1) * (dt / 8.0);
                let w = (w0 + w1) * 0.5 + (dw0 - dw1) * (dt / 8.0);
                (p, w)
            };
            let er = exact.rows();
            let mut err: f64 = 0.0;
            for (j, row) in er.iter().enumerate() {
                let (p, w) = hermite(j);
                err = err.max((p / w - row.0 / row.1).length());
            }
            // The arc's own middle, which the weight's error also moves.
            let (pa, _) = hermite(0);
            let (pc, wc) = hermite(1);
            let (pb, _) = hermite(2);
            let arc_mid = (pa + pc * 2.0 + pb) / (2.0 + 2.0 * wc);
            err = err.max(((arc_mid - exact.centre).length() - radius).abs());
            if err > FIT {
                split.push(tm);
            }
        }
        if split.is_empty() {
            return build(&roller, &ts, &sections, &derivs, convex, closed, tag_base);
        }
        ts.extend(split);
        ts.sort_by(f64::total_cmp);
        sections = solve(&ts)?;
    }
}

/// Assembles the tool from its sections (see [`rolling_tool`]).
#[allow(clippy::too_many_arguments)]
fn build(
    roller: &Roller,
    ts: &[f64],
    sections: &[Section],
    derivs: &[[(DVec3, f64); 6]],
    convex: bool,
    closed: bool,
    tag_base: u32,
) -> Result<Tool> {
    let n = ts.len() - 1;
    // Knots and control points of each row as a piecewise cubic (Hermite to Bezier).
    let mut knots = vec![ts[0]; 4];
    for t in &ts[1..n] {
        knots.extend([*t; 3]);
    }
    knots.extend([ts[n]; 4]);
    let control = |j: usize| -> Vec<(DVec3, f64)> {
        let mut out = vec![sections[0].rows()[j]];
        for k in 0..n {
            let dt = ts[k + 1] - ts[k];
            let (p0, w0) = sections[k].rows()[j];
            let (p1, w1) = sections[k + 1].rows()[j];
            let (d0, dw0) = derivs[k][j];
            let (d1, dw1) = derivs[k + 1][j];
            out.push((p0 + d0 * (dt / 3.0), w0 + dw0 * (dt / 3.0)));
            out.push((p1 - d1 * (dt / 3.0), w1 - dw1 * (dt / 3.0)));
            out.push((p1, w1));
        }
        out
    };
    let rows: Vec<Vec<(DVec3, f64)>> = (0..6).map(control).collect();
    let curve = |j: usize| -> Result<Curve> {
        let pts = rows[j].iter().map(|(p, _)| *p).collect();
        Ok(Curve::Spline(Arc::new(Spline::new(3, knots.clone(), pts, Vec::new())?)))
    };
    let m = rows[0].len();
    // The round: rows A, corner (weighted), B across.
    let mut points = Vec::with_capacity(3 * m);
    let mut weights = Vec::with_capacity(3 * m);
    for ((a, c), b) in rows[0].iter().zip(&rows[1]).zip(&rows[2]) {
        for &(p, w) in [a, c, b] {
            points.push(p / w);
            weights.push(w);
        }
    }
    let round = SplineSurface::new(
        (3, 2),
        knots.clone(),
        vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        (m, 3),
        points,
        weights,
    )?;
    // A side running off a contact curve (rows 0 and 2) carries on straight past it, as
    // far again, so the contact lies inside its patch: a face meets the side along it,
    // and an intersection traced along a patch's own boundary would break up.
    let ruled = |j0: usize, j1: usize| -> Result<Surface> {
        let contact = |j: usize| j == 0 || j == 2;
        let mut points = Vec::with_capacity(2 * m);
        for (r0, r1) in rows[j0].iter().zip(&rows[j1]) {
            let (p0, p1) = (r0.0, r1.0);
            points.push(if contact(j0) { p0 * 2.0 - p1 } else { p0 });
            points.push(if contact(j1) { p1 * 2.0 - p0 } else { p1 });
        }
        Ok(Surface::BSpline(Arc::new(SplineSurface::new(
            (3, 1),
            knots.clone(),
            vec![0.0, 0.0, 1.0, 1.0],
            (m, 2),
            points,
            Vec::new(),
        )?)))
    };

    // The profile: corners in order round the section (A, B, B pushed, corner pushed,
    // A pushed), and the segments between them (the arc first). It must turn
    // counter-clockwise about the direction of travel, as an extrusion's profile does
    // about its direction.
    let corner_rows = [0usize, 2, 3, 4, 5];
    let (_, t_start) = roller.point(ts[0]);
    let s = &sections[0];
    let poly = [s.a, s.b, s.b_off, s.corner_off, s.a_off];
    let turn: DVec3 = (0..5).map(|k| poly[k].cross(poly[(k + 1) % 5])).fold(DVec3::ZERO, |x, y| x + y);
    let order: Vec<usize> = if turn.dot(t_start) > 0.0 { vec![0, 1, 2, 3, 4] } else { vec![0, 4, 3, 2, 1] };
    let rows_in_order: Vec<usize> = order.iter().map(|&k| corner_rows[k]).collect();

    let mut tool = Solid::default();
    // Round a closed edge, the start section is also the end, and its profile edges are
    // the side faces' seams.
    let ends: Vec<usize> = if closed { vec![0] } else { vec![0, n] };
    // Vertices at each end, by profile position.
    let mut vertex = Vec::new();
    for &k in &ends {
        let rs = sections[k].rows();
        vertex.push(rows_in_order.iter().map(|&j| tool.add_vertex(rs[j].0 / rs[j].1)).collect::<Vec<_>>());
    }
    // The long edges along the edge, one per corner.
    let long: Vec<EdgeId> = rows_in_order
        .iter()
        .enumerate()
        .map(|(k, &j)| {
            curve(j).map(|c| {
                let end = vertex[vertex.len() - 1][k];
                tool.add_edge(Edge { curve: c, start: vertex[0][k], end, t0: ts[0], t1: ts[n] })
            })
        })
        .collect::<Result<_>>()?;
    // The section edges at each end, by profile segment.
    let mut across: Vec<Vec<EdgeId>> = Vec::new();
    for (end, &k) in ends.iter().enumerate() {
        let s = &sections[k];
        let mut list = Vec::new();
        for i in 0..5 {
            let (j0, j1) = (rows_in_order[i], rows_in_order[(i + 1) % 5]);
            let (p0, p1) = (s.rows()[j0], s.rows()[j1]);
            let (p0, p1) = (p0.0 / p0.1, p1.0 / p1.1);
            let is_arc = (j0 == 0 && j1 == 2) || (j0 == 2 && j1 == 0);
            let (curve, t0, t1) = if is_arc {
                let r = roller.radius;
                let x = (s.a - s.centre) / r;
                let y = ((s.b - s.centre) - x * (s.b - s.centre).dot(x)).normalize();
                let theta = (s.a - s.centre).angle_between(s.b - s.centre);
                let frame = Frame { origin: s.centre, x, y, z: x.cross(y) };
                let (t0, t1) = if j0 == 0 { (0.0, theta) } else { (theta, 0.0) };
                (Curve::Circle { frame, radius: r }, t0, t1)
            } else {
                let d = p1 - p0;
                (Curve::Line { origin: p0, dir: d / d.length() }, 0.0, d.length())
            };
            list.push(tool.add_edge(Edge {
                curve,
                start: vertex[end][i],
                end: vertex[end][(i + 1) % 5],
                t0,
                t1,
            }));
        }
        across.push(list);
    }
    let fwd = |edge| Coedge { edge, reversed: false };
    let rev = |edge| Coedge { edge, reversed: true };
    // The ends: flat, in the sections' planes.
    let (_, t_end) = roller.point(ts[n]);
    for (end, outward) in [(0usize, -t_start), (1, t_end)] {
        if closed {
            break;
        }
        let s = &sections[ends[end]];
        let z = (s.a - s.centre).cross(s.b - s.centre).normalize();
        let z = if z.dot(outward) < 0.0 { -z } else { z };
        let frame = Frame::new(s.centre, z, s.a - s.centre)?;
        let coedges = if end == 1 {
            across[1].iter().copied().map(fwd).collect()
        } else {
            across[0].iter().rev().copied().map(rev).collect()
        };
        tool.add_face(Face {
            surface: Surface::Plane { frame },
            reversed: false,
            outer: Loop { coedges },
            inners: Vec::new(),
            tag: if end == 0 { FaceTag::StartCap } else { FaceTag::EndCap },
            owner: 0,
        });
    }
    // The sides, the round among them.
    let mut round_face = 0;
    let km = n / 2;
    let sm = &sections[km];
    let (_, t_mid) = roller.point(ts[km]);
    for i in 0..5 {
        let (j0, j1) = (rows_in_order[i], rows_in_order[(i + 1) % 5]);
        let is_arc = (j0 == 0 && j1 == 2) || (j0 == 2 && j1 == 0);
        let surface = if is_arc { Surface::BSpline(Arc::new(round.clone())) } else { ruled(j0, j1)? };
        // A point on the side and its outward direction: right of the profile's travel.
        let rs = sm.rows();
        let (p0, p1) = (rs[j0].0 / rs[j0].1, rs[j1].0 / rs[j1].1);
        let (p, out) = if is_arc {
            let p = sm.centre + ((p0 + p1) * 0.5 - sm.centre).normalize() * roller.radius;
            (p, sm.centre - p)
        } else {
            ((p0 + p1) * 0.5, (p1 - p0).cross(t_mid))
        };
        let reversed = surface.normal_at(p).dot(out) < 0.0;
        if is_arc {
            round_face = tool.faces.len();
        }
        let tag = match (is_arc, j0) {
            (true, _) => tag_base + 2,
            (false, 3) | (false, 2) => tag_base + 4,
            _ => tag_base,
        };
        tool.add_face(Face {
            surface,
            reversed,
            outer: Loop {
                coedges: vec![
                    fwd(across[0][i]),
                    fwd(long[(i + 1) % 5]),
                    rev(across[across.len() - 1][i]),
                    rev(long[i]),
                ],
            },
            inners: Vec::new(),
            tag: FaceTag::Side { contour: 0, segment: tag },
            owner: 0,
        });
    }
    check::validate(&tool)?;
    Ok((tool, convex, vec![(roller.fa, round_face), (roller.fb, round_face)]))
}

#[cfg(test)]
mod tests;
