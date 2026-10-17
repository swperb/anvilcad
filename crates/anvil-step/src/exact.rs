//! Converts a read body to an exact `anvil-brep` solid when every face is a plane,
//! cylinder, cone, torus, sphere or B-spline patch. Edges keep the file's line, circle or ellipse; other edges are
//! traced exactly from the two surfaces they join, or else keep the file's B-spline.

use crate::reader::{Body, LoopKind, Model};
use crate::shapes;
use anvil_brep::intersect::{self, Ssi};
use anvil_brep::spline::Spline;
use anvil_brep::topo::{Coedge, Edge, Face, FaceTag, Loop};
use anvil_brep::{Curve, EdgeId, Frame, Solid, Surface, VertexId, check};
use glam::DVec3;
use std::cell::RefCell;
use std::collections::HashMap;
use std::f64::consts::TAU;
use std::sync::Arc;

/// The kernel surface for a face, given its boundary loops in order (running so the
/// face lies to their left seen from the side its normal points to) and its sense.
/// `singular` holds the points of the face's vertex loops (a cone's apex, say).
fn surface(
    s: &shapes::Surface,
    loops: &[Vec<DVec3>],
    singular: &[DVec3],
    same_sense: bool,
) -> Option<Surface> {
    let all: Vec<DVec3> = loops.iter().flatten().copied().collect();
    let points = &all[..];
    match s {
        shapes::Surface::Plane { frame } => Some(Surface::Plane { frame: *frame }),
        shapes::Surface::Cylinder { frame, radius } => {
            Some(Surface::Cylinder { frame: *frame, radius: *radius })
        }
        shapes::Surface::Cone { frame, radius, semi_angle } => {
            // A negative angle (outside the standard, but written) is the same cone with
            // its axis turned round.
            let (frame, semi_angle) = if *semi_angle < 0.0 {
                (&Frame { origin: frame.origin, x: frame.x, y: -frame.y, z: -frame.z }, &-*semi_angle)
            } else {
                (frame, semi_angle)
            };
            let tan = semi_angle.tan();
            if !(1e-6..1e6).contains(&tan) {
                return None;
            }
            let heights = points.iter().map(|p| frame.to_local(*p).z);
            let (lo, hi) = heights.fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), z| (a.min(z), b.max(z)));
            if !lo.is_finite() {
                return None;
            }
            let at = |z: f64| radius + z * tan;
            let tol = 1e-6 * radius.abs().max(1.0);
            if at(lo) < -tol || at(hi) < -tol {
                return None; // Past the apex, on the other nappe.
            }
            // A face that reaches the apex: a vertex loop or a boundary point there, or a
            // single loop that goes once round the axis (so the apex is inside it).
            let tip = frame.origin - frame.z * (radius / tan);
            let near_tip = |p: &DVec3| (*p - tip).length() < 1e-6 * radius.abs().max(1.0) + 1e-9;
            let winds = |l: &[DVec3]| {
                let angle = |p: &DVec3| {
                    let q = frame.to_local(*p);
                    q.y.atan2(q.x)
                };
                let n = l.len();
                let turn: f64 = (0..n)
                    .map(|i| {
                        let d = angle(&l[(i + 1) % n]) - angle(&l[i]);
                        d - TAU * (d / TAU).round()
                    })
                    .sum();
                turn.abs() > std::f64::consts::PI
            };
            let reaches = singular.iter().any(near_tip)
                || points.iter().any(near_tip)
                || (loops.len() == 1 && winds(&loops[0]));
            if reaches {
                // Laid out across the axis; the origin goes to the face's widest rim.
                let far = if at(hi) >= at(lo) { hi } else { lo };
                if at(far) <= tol {
                    return None;
                }
                return Some(Surface::Cone {
                    frame: frame.with_origin(frame.origin + frame.z * far),
                    radius: at(far),
                    half_angle: *semi_angle,
                    apex: true,
                });
            }
            if at(lo) <= tol || at(hi) <= tol {
                return None;
            }
            // Otherwise the origin goes to the middle of the face, so the radius there
            // sets the parameter scale.
            let mid = 0.5 * (lo + hi);
            Some(Surface::Cone {
                frame: frame.with_origin(frame.origin + frame.z * mid),
                radius: at(mid),
                half_angle: *semi_angle,
                apex: false,
            })
        }
        shapes::Surface::Torus { frame, major, minor } => {
            // Ring tori only, and faces that leave a gap round the tube: the tube angle
            // is measured from the middle of the face, away from that gap.
            if *minor <= 0.0 || major - minor <= 1e-6 * major.abs().max(1.0) {
                return None;
            }
            let mut angles: Vec<f64> = points
                .iter()
                .map(|p| {
                    let l = frame.to_local(*p);
                    l.z.atan2((l.x * l.x + l.y * l.y).sqrt() - major).rem_euclid(TAU)
                })
                .collect();
            angles.sort_by(f64::total_cmp);
            let n = angles.len();
            let (gap, after) = (0..n)
                .map(|i| {
                    let next = if i + 1 < n { angles[i + 1] } else { angles[0] + TAU };
                    (next - angles[i], angles[i])
                })
                .max_by(|a, b| a.0.total_cmp(&b.0))?;
            if gap < 0.2 {
                return None;
            }
            // The face runs from the end of the gap round to its start.
            let v_mid = after + gap + 0.5 * (TAU - gap);
            Some(Surface::Torus { frame: *frame, major: *major, minor: *minor, v_mid })
        }
        shapes::Surface::BSpline(b) => {
            let n = b.points.len();
            let m = b.points.first()?.len();
            if b.points.iter().any(|row| row.len() != m) {
                return None;
            }
            let points: Vec<DVec3> = b.points.iter().flatten().copied().collect();
            let weights: Vec<f64> = b.weights.iter().flatten().copied().collect();
            let patch = anvil_brep::patch::SplineSurface::new(
                (b.u.degree, b.v.degree),
                b.u.knots.clone(),
                b.v.knots.clone(),
                (n, m),
                points,
                weights,
            )
            .ok()?;
            Some(Surface::BSpline(Arc::new(patch)))
        }
        shapes::Surface::Sphere { frame, radius } => {
            // The face's vector area (half the sum of its loops' Newell vectors) points
            // into its middle; the kernel lays the face out from the point opposite.
            let mut area: DVec3 = loops.iter().map(|l| newell(l)).sum();
            if !same_sense {
                area = -area;
            }
            if area.length() < 1e-6 * radius * radius {
                return None;
            }
            let hint = if area.x.abs() < 0.9 * area.length() { DVec3::X } else { DVec3::Y };
            let frame = Frame::new(frame.origin, area, hint).ok()?;
            Some(Surface::Sphere { frame, radius: *radius })
        }
    }
}

fn curve(c: &shapes::Curve) -> Option<Curve> {
    match c {
        shapes::Curve::Line { origin, dir } => {
            Some(Curve::Line { origin: *origin, dir: dir.try_normalize()? })
        }
        shapes::Curve::Circle { frame, radius } => Some(Curve::Circle { frame: *frame, radius: *radius }),
        shapes::Curve::Ellipse { frame, a, b } => Some(Curve::Ellipse { frame: *frame, a: *a, b: *b }),
        _ => None,
    }
}

/// Intersection curves by pair of faces, `None` where tracing failed.
type Traced = HashMap<(usize, usize), Option<Vec<Curve>>>;

/// The file's B-spline as a kernel spline.
fn file_spline(c: &shapes::Curve) -> Option<Curve> {
    let shapes::Curve::BSpline(b) = c else { return None };
    let s = Spline::new(b.basis.degree, b.basis.knots.clone(), b.points.clone(), b.weights.clone()).ok()?;
    Some(Curve::Spline(Arc::new(s)))
}

/// True for a curve that can be bent to end at `p`: the file's own line, circle or
/// ellipse (not one traced exactly from its surfaces), or an open spline from the file
/// passing within `reach` of `p` (its piece is cut out and its end moved there).
fn bendable(c: &Curve, traced: bool, p: DVec3, reach: f64) -> bool {
    match c {
        Curve::Line { .. } | Curve::Circle { .. } | Curve::Ellipse { .. } => !traced,
        Curve::Spline(sp) => !traced && !sp.closed && c.distance_to_curve(p) <= reach,
    }
}

/// A clamped spline whose ends (those of `t0`, `t1` at its domain's ends) are moved to
/// `ps` and `pe`, by moving its first and last control points, which it passes through.
fn pin_ends(c: &Curve, t0: f64, t1: f64, ps: DVec3, pe: DVec3) -> Option<Curve> {
    // A line through both ends; an arc as its exact rational spline, then pinned.
    match c {
        Curve::Line { .. } => {
            let dir = (pe - ps).try_normalize()?;
            return Some(Curve::Line { origin: ps, dir });
        }
        Curve::Circle { .. } | Curve::Ellipse { .. } => {
            let arc = Curve::Spline(Arc::new(arc_spline(c, t0, t1)?));
            let (lo, hi) = match &arc {
                Curve::Spline(sp) => sp.domain(),
                _ => return None,
            };
            return pin_ends(&arc, lo, hi, ps, pe);
        }
        _ => {}
    }
    let Curve::Spline(sp) = c else { return None };
    if sp.closed {
        return None;
    }
    let (lo, hi) = sp.domain();
    let p = sp.degree;
    let n = sp.points.len();
    let clamped = sp.knots[..=p].iter().all(|k| *k == lo) && sp.knots[n..].iter().all(|k| *k == hi);
    let near = |t: f64, k: f64| (t - k).abs() <= 1e-9 * (hi - lo);
    let ends = |t: f64| near(t, lo) || near(t, hi);
    // An edge that is a piece of a longer (or unclamped) spline: cut the piece out as a
    // clamped spline of its own first, so its ends are control points.
    if !clamped || !ends(t0) || !ends(t1) {
        let (a, b) = (t0.min(t1).max(lo), t0.max(t1).min(hi));
        let piece = Curve::Spline(Arc::new(sp.piece(a, b).ok()?));
        return pin_ends(&piece, t0.clamp(a, b), t1.clamp(a, b), ps, pe);
    }
    let mut points = sp.points.clone();
    let mut changed = false;
    for (t, q) in [(t0, ps), (t1, pe)] {
        if near(t, lo) {
            points[0] = q;
            changed = true;
        } else if near(t, hi) {
            points[n - 1] = q;
            changed = true;
        }
    }
    if !changed {
        return None;
    }
    let s = Spline::new(p, sp.knots.clone(), points, sp.weights.clone()).ok()?;
    Some(Curve::Spline(Arc::new(s)))
}

/// The piece of a circle or ellipse from `t0` to `t1` as a rational quadratic spline,
/// exactly: one piece per quarter turn or less, each with its middle control point
/// where the end tangents meet.
fn arc_spline(c: &Curve, t0: f64, t1: f64) -> Option<Spline> {
    let (frame, a, b) = match *c {
        Curve::Circle { frame, radius } => (frame, radius, radius),
        Curve::Ellipse { frame, a, b } => (frame, a, b),
        _ => return None,
    };
    let at = |t: f64, k: f64| frame.origin + frame.x * (a * k * t.cos()) + frame.y * (b * k * t.sin());
    let sweep = t1 - t0;
    let pieces = ((sweep.abs() / (0.5 * std::f64::consts::PI)).ceil() as usize).max(1);
    let half = 0.5 * sweep / pieces as f64;
    let mut points = vec![at(t0, 1.0)];
    let mut weights = vec![1.0];
    let mut knots = vec![0.0; 3];
    for k in 0..pieces {
        let mid = t0 + sweep * (k as f64 + 0.5) / pieces as f64;
        let end = t0 + sweep * (k as f64 + 1.0) / pieces as f64;
        points.push(at(mid, 1.0 / half.cos()));
        weights.push(half.cos());
        points.push(at(end, 1.0));
        weights.push(1.0);
        let m = if k + 1 == pieces { 3 } else { 2 };
        knots.extend(std::iter::repeat_n((k + 1) as f64, m));
    }
    Spline::new(2, knots, points, weights).ok()
}

/// Direction the edge runs at `p` along the file's curve.
fn file_direction(c: &shapes::Curve, p: DVec3, same_sense: bool) -> DVec3 {
    let (lo, hi) = c.domain();
    let t = c.invert(p);
    let h = if (hi - lo).is_finite() { (hi - lo) * 1e-4 } else { 1e-3 };
    let (a, b) = if c.is_periodic() { (t - h, t + h) } else { ((t - h).max(lo), (t + h).min(hi)) };
    let d = c.eval(b) - c.eval(a);
    if same_sense { d } else { -d }
}

/// Parameter range from `start` to `end` along `c`, in the curve's direction when
/// `forward`.
fn range(c: &Curve, start: DVec3, end: DVec3, forward: bool) -> (f64, f64) {
    let t0 = c.param_of(start);
    let t1 = c.param_of(end);
    let Some(period) = c.period() else {
        return (t0, t1);
    };
    let closed = (start - end).length() < 1e-9;
    let mut sweep = if forward { (t1 - t0).rem_euclid(period) } else { (t0 - t1).rem_euclid(period) };
    if closed || sweep < 1e-12 * period {
        sweep = period;
    }
    if forward { (t0, t0 + sweep) } else { (t0, t0 - sweep) }
}

/// Twice the signed area vector of a closed polyline (Newell's method).
fn newell(pts: &[DVec3]) -> DVec3 {
    let n = pts.len();
    (0..n).map(|i| pts[i].cross(pts[(i + 1) % n])).sum()
}

/// The exact solid for `body`, or `None` (with the reason) if it cannot be built.
pub fn convert(model: &Model, body: &Body) -> Result<Solid, String> {
    if !body.solid {
        return Err("surface models are imported as meshes".into());
    }
    // Exact surfaces of every face, and the faces each edge bounds, so that edges the
    // file stores as B-splines can be rebuilt exactly from the two surfaces they join.
    // Each face's loops as points in order (a few per edge, along its curve), running
    // the way the face's bounds run.
    let face_loops = |f: &crate::reader::Face| -> Vec<Vec<DVec3>> {
        let mut loops = Vec::new();
        for b in &f.bounds {
            let LoopKind::Edges(edges) = &b.kind else { continue };
            let mut pts = Vec::new();
            for oe in edges {
                let Some(e) = model.edges.get(&oe.edge) else { continue };
                let (Some(&ps), Some(&pe)) = (model.vertices.get(&e.start), model.vertices.get(&e.end))
                else {
                    continue;
                };
                let mut along: Vec<DVec3> = match &e.curve {
                    Some(c) => {
                        let (t0, t1) = (c.invert(ps), c.invert(pe));
                        let sweep = if c.is_periodic() {
                            let closed = (ps - pe).length() < 1e-9;
                            let s = if e.same_sense {
                                (t1 - t0).rem_euclid(TAU)
                            } else {
                                -(t0 - t1).rem_euclid(TAU)
                            };
                            if closed || s.abs() < 1e-12 { if e.same_sense { TAU } else { -TAU } } else { s }
                        } else {
                            t1 - t0
                        };
                        (0..8).map(|k| c.eval(t0 + sweep * k as f64 / 8.0)).collect()
                    }
                    None => vec![ps],
                };
                along[0] = ps;
                if !oe.forward {
                    along.push(pe);
                    along.reverse();
                    along.pop();
                }
                pts.extend(along);
            }
            if !b.orientation {
                pts.reverse();
            }
            loops.push(pts);
        }
        loops
    };
    let surfaces: Vec<Option<Surface>> = body
        .faces
        .iter()
        .map(|f| {
            let singular: Vec<DVec3> = f
                .bounds
                .iter()
                .filter_map(|b| match b.kind {
                    LoopKind::Vertex(v) => model.vertices.get(&v).copied(),
                    _ => None,
                })
                .collect();
            f.surface.as_ref().and_then(|s| surface(s, &face_loops(f), &singular, f.same_sense))
        })
        .collect();
    let mut edge_faces: HashMap<u64, Vec<usize>> = HashMap::new();
    for (fi, f) in body.faces.iter().enumerate() {
        for b in &f.bounds {
            if let LoopKind::Edges(edges) = &b.kind {
                for oe in edges {
                    let list = edge_faces.entry(oe.edge).or_default();
                    if !list.contains(&fi) {
                        list.push(fi);
                    }
                }
            }
        }
    }
    // Intersections per pair of faces: the edges between two faces share them.
    let traced: RefCell<Traced> = RefCell::new(HashMap::new());
    let rebuilt = |edge: u64, e: &crate::reader::EdgeCurve| -> Option<Curve> {
        let faces = edge_faces.get(&edge)?;
        let [fa, fb] = faces[..] else { return None };
        let (sa, sb) = (surfaces.get(fa)?.as_ref()?, surfaces.get(fb)?.as_ref()?);
        let cs = traced
            .borrow_mut()
            .entry((fa.min(fb), fa.max(fb)))
            .or_insert_with(|| match intersect::surfaces(sa, sb, 1e-6) {
                Ok(Ssi::Curves(cs)) => Some(cs),
                _ => None,
            })
            .clone()?;
        let (ps, pe) = (*model.vertices.get(&e.start)?, *model.vertices.get(&e.end)?);
        // A point inside the edge, from the file's own curve, picks the right branch.
        let mid = e.curve.as_ref().map(|c| {
            let (t0, t1) = (c.invert(ps), c.invert(pe));
            c.eval(0.5 * (t0 + t1))
        });
        // The file's vertices may be off by up to its stated accuracy.
        let tol = (1e-4 * ps.length().max(pe.length()).max(1.0))
            .max(2.0 * model.uncertainty.unwrap_or(0.0).min(0.05));
        cs.into_iter().find(|c| {
            c.distance_to_curve(ps) < tol
                && c.distance_to_curve(pe) < tol
                && mid.is_none_or(|m| c.distance_to_curve(m) < tol * 10.0)
        })
    };
    // The curve traced from the two faces an edge joins, running the way the file's
    // curve runs.
    let traced_curve = |edge: u64, e: &crate::reader::EdgeCurve| -> Option<(Curve, bool)> {
        let c = rebuilt(edge, e)?;
        let ps = *model.vertices.get(&e.start)?;
        let forward = match &e.curve {
            Some(file) => {
                let want = file_direction(file, ps, e.same_sense);
                let t = c.param_of(ps);
                let h = c.period().unwrap_or(1.0) * 1e-4;
                (c.eval(t + h) - c.eval(t - h)).dot(want) >= 0.0
            }
            None => e.same_sense,
        };
        Some((c, forward))
    };
    // Each edge's curve, with its direction and whether it was traced: the file's own
    // line, circle or ellipse; else the curve traced from the two faces; else the
    // file's B-spline. Edges with none are reported where a face uses them.
    let mut chosen: HashMap<u64, (Curve, bool, bool)> = HashMap::new();
    for f in &body.faces {
        for b in &f.bounds {
            let LoopKind::Edges(edges) = &b.kind else { continue };
            for oe in edges {
                let Some(e) = model.edges.get(&oe.edge) else { continue };
                if chosen.contains_key(&oe.edge) {
                    continue;
                }
                let pick = match e.curve.as_ref().and_then(curve) {
                    Some(c) => Some((c, e.same_sense, false)),
                    None => traced_curve(oe.edge, e)
                        .map(|(c, forward)| (c, forward, true))
                        .or_else(|| e.curve.as_ref().and_then(file_spline).map(|c| (c, e.same_sense, false))),
                };
                if let Some(pick) = pick {
                    chosen.insert(oe.edge, pick);
                }
            }
        }
    }

    // Healing. Files written to a looser tolerance than the kernel's leave vertices off
    // their edges' curves. In a B-rep the surfaces are the shape and the vertices follow
    // from them, so each such vertex moves to where its curves meet: found by projecting
    // onto them in turn and averaging, then (where they do not quite meet) by easing
    // toward the farthest. Where a file's own line or circle still misses, it is
    // replaced by the curve traced from the two surfaces, which meets the others
    // exactly at the corner. A vertex may move up to ten times the accuracy the file
    // declares (at most 0.1 mm); beyond that the file is taken at its word.
    let allowed = |p: DVec3| anvil_brep::tol::LINEAR * 10.0 + 1e-12 * p.length();
    let heal = (10.0 * model.uncertainty.unwrap_or(1e-3)).clamp(1e-4, 0.1);
    let mut incident: HashMap<u64, Vec<u64>> = HashMap::new();
    for id in chosen.keys() {
        if let Some(e) = model.edges.get(id) {
            for v in [e.start, e.end] {
                let list = incident.entry(v).or_default();
                if !list.contains(id) {
                    list.push(*id);
                }
            }
        }
    }
    let snap = |p0: DVec3, curves: &[&Curve]| -> (DVec3, f64) {
        let mut p = p0;
        for _ in 0..100 {
            let q = curves.iter().map(|c| c.eval(c.param_of(p))).sum::<DVec3>() / curves.len() as f64;
            let step = (q - p).length();
            p = q;
            if step < 1e-14 * p.length().max(1.0) {
                break;
            }
        }
        let worst = |p: DVec3| curves.iter().map(|c| c.distance_to_curve(p)).fold(0.0, f64::max);
        // Curves that do not quite meet: the average settles where the squared misses
        // balance; stepping toward the farthest, in shrinking steps, moves it to where the
        // worst miss is least.
        let mut best = (p, worst(p));
        if best.1 > 0.0 {
            for k in 0..400 {
                let Some(q) = curves
                    .iter()
                    .map(|c| c.eval(c.param_of(p)))
                    .max_by(|a, b| (*a - p).length().total_cmp(&(*b - p).length()))
                else {
                    break;
                };
                p += (q - p) * (0.5 / (1.0 + k as f64 / 8.0));
                let w = worst(p);
                if w < best.1 {
                    best = (p, w);
                }
            }
        }
        best
    };
    let mut pos: HashMap<u64, DVec3> = model.vertices.clone();
    for round in 0..2 {
        let mut retrace: Vec<u64> = Vec::new();
        for (v, edges) in &incident {
            let Some(&p0) = model.vertices.get(v) else { continue };
            let curves: Vec<&Curve> = edges.iter().filter_map(|e| chosen.get(e)).map(|c| &c.0).collect();
            if curves.is_empty() || curves.iter().all(|c| c.distance_to_curve(p0) <= allowed(p0)) {
                continue;
            }
            // Splines from the file that end here can be bent to meet the vertex (their end
            // control points move); lines, circles and traced curves cannot. The vertex
            // goes where the rigid ones meet, or where all meet if none is rigid.
            let rigid: Vec<&Curve> = edges
                .iter()
                .filter_map(|e| chosen.get(e))
                .filter(|c| !bendable(&c.0, c.2, p0, heal))
                .map(|c| &c.0)
                .collect();
            let (p, off) = snap(p0, if rigid.is_empty() { &curves } else { &rigid });
            if (p - p0).length() <= heal && off <= allowed(p) {
                pos.insert(*v, p);
            } else if round == 0 {
                retrace.extend(edges.iter().filter(|e| chosen.get(e).is_some_and(|c| !c.2)));
            }
        }
        retrace.sort_unstable();
        retrace.dedup();
        let mut changed = false;
        for id in retrace {
            if let Some((c, forward)) = model.edges.get(&id).and_then(|e| traced_curve(id, e)) {
                chosen.insert(id, (c, forward, true));
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    let mut solid = Solid::default();
    let mut vids: HashMap<u64, VertexId> = HashMap::new();
    let mut eids: HashMap<u64, EdgeId> = HashMap::new();
    for (index, f) in body.faces.iter().enumerate() {
        let surf = surfaces[index].clone().ok_or_else(|| match f.surface_type.as_str() {
            "POLY" => "the file stores this body as flat facets, so it is imported as a mesh".to_string(),
            "CONICAL_SURFACE" => "a cone face reaches its apex, so it is imported as a mesh".to_string(),
            "SPHERICAL_SURFACE" => {
                "a sphere face is a whole sphere or a band round it, so it is imported as a mesh".to_string()
            }
            "TOROIDAL_SURFACE" | "DEGENERATE_TOROIDAL_SURFACE" => {
                "a torus face wraps round its tube or the torus is degenerate, so it is imported as a mesh"
                    .to_string()
            }
            other => format!(
                "{} faces are not supported by the exact kernel yet, so it is imported as a mesh",
                other.to_lowercase().replace('_', " ")
            ),
        })?;
        // On a cone laid out across its axis the apex is an ordinary point of the face:
        // its vertex loop goes, and so does any seam running up to it (an edge used both
        // ways in the face), which would be a slit in that layout.
        let across_axis = matches!(surf, Surface::Cone { apex: true, .. });
        let mut uses: HashMap<u64, usize> = HashMap::new();
        for b in &f.bounds {
            if let LoopKind::Edges(edges) = &b.kind {
                for oe in edges {
                    *uses.entry(oe.edge).or_default() += 1;
                }
            }
        }
        let mut loops: Vec<(Loop, f64)> = Vec::new();
        for b in &f.bounds {
            let edges = match &b.kind {
                LoopKind::Edges(edges) => edges,
                LoopKind::Vertex(_) if across_axis => continue,
                _ => return Err("face bounded by a vertex or polygon loop".into()),
            };
            let mut coedges = Vec::with_capacity(edges.len());
            let mut pts = Vec::new();
            for oe in edges {
                if across_axis && uses.get(&oe.edge).copied().unwrap_or(0) > 1 {
                    continue;
                }
                let e = model.edges.get(&oe.edge).ok_or("edge missing")?;
                let id = match eids.get(&oe.edge) {
                    Some(id) => *id,
                    None => {
                        let (mut c, forward, _) = chosen.get(&oe.edge).cloned().ok_or_else(|| {
                            format!(
                                "{} edges are not exact yet",
                                e.curve_type.to_lowercase().replace('_', " ")
                            )
                        })?;
                        let mut vertex = |v: u64| -> Result<VertexId, String> {
                            if let Some(id) = vids.get(&v) {
                                return Ok(*id);
                            }
                            let p = *pos.get(&v).ok_or("vertex missing")?;
                            let id = solid.add_vertex(p);
                            vids.insert(v, id);
                            Ok(id)
                        };
                        let (start, end) = (vertex(e.start)?, vertex(e.end)?);
                        let (ps, pe) = (pos[&e.start], pos[&e.end]);
                        let (mut t0, mut t1) = range(&c, ps, pe, forward);
                        let mut gap = (c.eval(t0) - ps).length().max((c.eval(t1) - pe).length());
                        // A spline edge still short of its healed vertices: move its end
                        // control points onto them, which changes it only near its ends.
                        if gap > allowed(ps).max(allowed(pe))
                            && gap <= heal
                            && let Some(pinned) = pin_ends(&c, t0, t1, ps, pe)
                        {
                            c = pinned;
                            (t0, t1) = range(&c, ps, pe, forward);
                            gap = (c.eval(t0) - ps).length().max((c.eval(t1) - pe).length());
                        }
                        // What healing could not close: the exact kernel needs every vertex
                        // on its edges' curves (to 1e-5 mm).
                        if gap > allowed(ps).max(allowed(pe)) {
                            return Err(match model.uncertainty {
                                Some(u) if gap > heal => format!(
                                    "the file's edges miss their vertices by up to {gap:.1e} mm, more than its stated accuracy ({u:.1e} mm), so it is imported as a mesh"
                                ),
                                _ => format!(
                                    "the file's edges miss their vertices by up to {gap:.1e} mm and could not be healed, so it is imported as a mesh"
                                ),
                            });
                        }
                        let id = solid.add_edge(Edge { curve: c, start, end, t0, t1 });
                        eids.insert(oe.edge, id);
                        id
                    }
                };
                coedges.push(Coedge { edge: id, reversed: !oe.forward });
                let ed = solid.edge(id).ok_or("edge missing")?;
                pts.extend(ed.curve.sample(ed.t0, ed.t1, 0.1, 0.3));
            }
            if coedges.is_empty() {
                continue; // Only seams to the apex, now gone.
            }
            if !b.orientation {
                coedges.reverse();
                for c in &mut coedges {
                    c.reversed = !c.reversed;
                }
            }
            let area = newell(&pts).length();
            loops.push((Loop { coedges }, if b.outer { f64::INFINITY } else { area }));
        }
        // The outer loop is the one marked outer, or else the largest.
        let outer_at = loops
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.1.total_cmp(&b.1.1))
            .map(|(i, _)| i)
            .ok_or("face without loops")?;
        let outer = loops.remove(outer_at).0;
        solid.add_face(Face {
            surface: surf,
            reversed: !f.same_sense,
            outer,
            inners: loops.into_iter().map(|(l, _)| l).collect(),
            tag: FaceTag::Imported { index: index as u32 },
            owner: 0,
        });
    }
    // Faces that wrap all the way around without a seam get one, so the kernel can
    // lay every face out flat.
    anvil_brep::seam::add_seams(&mut solid).map_err(|e| e.to_string())?;
    check::validate(&solid).map_err(|e| e.to_string())?;
    Ok(solid)
}
