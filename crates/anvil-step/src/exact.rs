//! Converts a read body to an exact `anvil-brep` solid when every face is a plane or
//! cylinder bounded by lines, circles and ellipses.

use crate::reader::{Body, LoopKind, Model};
use crate::shapes;
use anvil_brep::intersect::{self, Ssi};
use anvil_brep::topo::{Coedge, Edge, Face, FaceTag, Loop};
use anvil_brep::{Curve, EdgeId, Solid, Surface, VertexId, check};
use glam::DVec3;
use std::collections::HashMap;
use std::f64::consts::TAU;

fn surface(s: &shapes::Surface) -> Option<Surface> {
    match s {
        shapes::Surface::Plane { frame } => Some(Surface::Plane { frame: *frame }),
        shapes::Surface::Cylinder { frame, radius } => {
            Some(Surface::Cylinder { frame: *frame, radius: *radius })
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

/// Parameter range from `start` to `end` along `c`, forwards when `same_sense`.
fn range(c: &Curve, start: DVec3, end: DVec3, same_sense: bool) -> (f64, f64) {
    let t0 = c.param_of(start);
    let t1 = c.param_of(end);
    if !c.is_periodic() {
        return (t0, t1);
    }
    let closed = (start - end).length() < 1e-9;
    if same_sense {
        let mut sweep = (t1 - t0).rem_euclid(TAU);
        if closed || sweep < 1e-12 {
            sweep = TAU;
        }
        (t0, t0 + sweep)
    } else {
        let mut sweep = (t0 - t1).rem_euclid(TAU);
        if closed || sweep < 1e-12 {
            sweep = TAU;
        }
        (t0, t0 - sweep)
    }
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
    let surfaces: Vec<Option<Surface>> =
        body.faces.iter().map(|f| f.surface.as_ref().and_then(surface)).collect();
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
    let rebuilt = |edge: u64, e: &crate::reader::EdgeCurve| -> Option<Curve> {
        let faces = edge_faces.get(&edge)?;
        let [fa, fb] = faces[..] else { return None };
        let (sa, sb) = (surfaces.get(fa)?.as_ref()?, surfaces.get(fb)?.as_ref()?);
        let (ps, pe) = (*model.vertices.get(&e.start)?, *model.vertices.get(&e.end)?);
        // A point inside the edge, from the file's own curve, picks the right branch.
        let mid = e.curve.as_ref().map(|c| {
            let (t0, t1) = (c.invert(ps), c.invert(pe));
            c.eval(0.5 * (t0 + t1))
        });
        let tol = 1e-4 * ps.length().max(pe.length()).max(1.0);
        let Ok(Ssi::Curves(cs)) = intersect::surfaces(sa, sb, 1e-6) else { return None };
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
        let surf = f.surface.as_ref().and_then(surface).ok_or_else(|| match f.surface_type.as_str() {
            "POLY" => "the file stores this body as flat facets, so it is imported as a mesh".to_string(),
            other => format!(
                "{} faces are not supported by the exact kernel yet, so it is imported as a mesh",
                other.to_lowercase().replace('_', " ")
            ),
        })?;
        let mut loops: Vec<(Loop, f64)> = Vec::new();
        for b in &f.bounds {
            let LoopKind::Edges(edges) = &b.kind else {
                return Err("face bounded by a vertex or polygon loop".into());
            };
            let mut coedges = Vec::with_capacity(edges.len());
            let mut pts = Vec::new();
            for oe in edges {
                let e = model.edges.get(&oe.edge).ok_or("edge missing")?;
                let id = match eids.get(&oe.edge) {
                    Some(id) => *id,
                    None => {
                        let c = e.curve.as_ref().and_then(curve).or_else(|| rebuilt(oe.edge, e)).ok_or_else(
                            || {
                                format!(
                                    "{} edges are not exact yet",
                                    e.curve_type.to_lowercase().replace('_', " ")
                                )
                            },
                        )?;
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
                        let (t0, t1) = range(&c, ps, pe, e.same_sense);
                        let id = solid.add_edge(Edge { curve: c, start, end, t0, t1 });
                        eids.insert(oe.edge, id);
                        id
                    }
                };
                coedges.push(Coedge { edge: id, reversed: !oe.forward });
                let ed = solid.edge(id).ok_or("edge missing")?;
                pts.extend(ed.curve.sample(ed.t0, ed.t1, 0.1, 0.3));
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
    check::validate(&solid).map_err(|e| e.to_string())?;
    Ok(solid)
}
