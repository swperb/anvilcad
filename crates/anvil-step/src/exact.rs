//! Converts a read body to an exact `anvil-brep` solid when every face is a plane,
//! cylinder, cone, torus or sphere. Edges keep the file's line, circle or ellipse; other edges are
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
        _ => None,
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
        let tol = 1e-4 * ps.length().max(pe.length()).max(1.0);
        cs.into_iter().find(|c| {
            c.distance_to_curve(ps) < tol
                && c.distance_to_curve(pe) < tol
                && mid.is_none_or(|m| c.distance_to_curve(m) < tol * 10.0)
        })
    };
    let mut solid = Solid::default();
    let mut vids: HashMap<u64, VertexId> = HashMap::new();
    let mut eids: HashMap<u64, EdgeId> = HashMap::new();
    for (index, f) in body.faces.iter().enumerate() {
        let surf = surfaces[index].ok_or_else(|| match f.surface_type.as_str() {
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
                        // The file's own line, circle or ellipse; else the curve traced
                        // from the two faces, running the way the file's curve runs;
                        // else the file's B-spline.
                        let (c, forward) = match e.curve.as_ref().and_then(curve) {
                            Some(c) => (c, e.same_sense),
                            None => match rebuilt(oe.edge, e) {
                                Some(c) => {
                                    let ps = *model.vertices.get(&e.start).ok_or("vertex missing")?;
                                    let forward = match &e.curve {
                                        Some(file) => {
                                            let want = file_direction(file, ps, e.same_sense);
                                            let t = c.param_of(ps);
                                            let h = c.period().unwrap_or(1.0) * 1e-4;
                                            (c.eval(t + h) - c.eval(t - h)).dot(want) >= 0.0
                                        }
                                        None => e.same_sense,
                                    };
                                    (c, forward)
                                }
                                None => {
                                    let c = e.curve.as_ref().and_then(file_spline).ok_or_else(|| {
                                        format!(
                                            "{} edges are not exact yet",
                                            e.curve_type.to_lowercase().replace('_', " ")
                                        )
                                    })?;
                                    (c, e.same_sense)
                                }
                            },
                        };
                        let mut vertex = |v: u64| -> Result<VertexId, String> {
                            if let Some(id) = vids.get(&v) {
                                return Ok(*id);
                            }
                            let p = *model.vertices.get(&v).ok_or("vertex missing")?;
                            let id = solid.add_vertex(p);
                            vids.insert(v, id);
                            Ok(id)
                        };
                        let (start, end) = (vertex(e.start)?, vertex(e.end)?);
                        let (ps, pe) = (model.vertices[&e.start], model.vertices[&e.end]);
                        let (t0, t1) = range(&c, ps, pe, forward);
                        // Files written to a looser tolerance leave vertices off their
                        // curves; the exact kernel needs them on (to 1e-5 mm).
                        let gap = (c.eval(t0) - ps).length().max((c.eval(t1) - pe).length());
                        let allowed = anvil_brep::tol::LINEAR * 10.0 + 1e-12 * ps.length().max(pe.length());
                        if gap > allowed {
                            return Err(format!(
                                "the file's edges miss their vertices by up to {gap:.1e} mm, more than the exact kernel accepts, so it is imported as a mesh"
                            ));
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
