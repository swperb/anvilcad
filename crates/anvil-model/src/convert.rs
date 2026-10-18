//! Convert Entities: model edges projected onto a sketch's plane as sketch curves. The
//! behaviour is specified in `docs/re/sketch-tools.md` ("Convert Entities"): each
//! converted curve lies on its edge and is fixed there, so the sketch stays fully
//! defined.
//!
//! Decision: AnvilCAD copies the edges at the time of the command (with Fixed relations
//! on the copies) rather than keeping a live On Edge link, so a later change to the
//! edges does not move the converted curves.

use crate::document::*;
use crate::rebuild::BuildState;
use anvil_kernel::{EdgeRef, FaceId, Vec2, Vec3};
use anvil_sketch::Constraint;

/// The edges bounding a face, as edge names.
pub fn face_edges(st: &BuildState, f: FaceId) -> Vec<EdgeRef> {
    let mut out: Vec<EdgeRef> =
        st.body.model_edges().into_iter().map(|m| m.faces).filter(|e| e.a == f || e.b == f).collect();
    out.sort();
    out.dedup();
    out
}

impl Document {
    /// Adds the edges, projected onto the sketch's plane, to sketch `sketch` as fixed
    /// curves. Straight edges become lines; round edges in planes parallel to the
    /// sketch become circles or arcs; any other edge becomes a run of short lines.
    /// Returns how many curves were added.
    pub fn convert_entities(
        &mut self,
        sketch: u32,
        st: &BuildState,
        edges: &[EdgeRef],
    ) -> Result<usize, String> {
        let frame = *st.frames.get(&sketch).ok_or("the sketch has not been built")?;
        let to2 = |p: Vec3| frame.to_local(p);
        // Each edge as 2D pieces: (start, end, Some((centre, counter-clockwise)) for arcs).
        let mut curves: Vec<Vec<(Vec2, Vec2, Option<(Vec2, bool)>)>> = Vec::new();
        for e in edges {
            curves.push(self.edge_pieces(st, *e, &frame, &to2)?);
        }
        let sf = self.sketch_mut(sketch).ok_or("the sketch is missing")?;
        let sk = &mut sf.sketch;
        let mut added = 0;
        let point_at = |sk: &mut anvil_sketch::Sketch, p: Vec2| -> usize {
            match sk.pick_point(p, 1e-6) {
                Some(id) => id,
                None => {
                    let id = sk.add_point(p);
                    sk.add_constraint(Constraint::Fix { p: id, x: p.x, y: p.y });
                    id
                }
            }
        };
        for pieces in curves {
            for (a, b, arc) in pieces {
                match arc {
                    None => {
                        let (pa, pb) = (point_at(sk, a), point_at(sk, b));
                        if pa != pb {
                            sk.add_line(pa, pb);
                            added += 1;
                        }
                    }
                    Some((c, _)) if a.distance(b) < 1e-9 => {
                        let pc = sk.add_point(c);
                        sk.add_constraint(Constraint::Fix { p: pc, x: c.x, y: c.y });
                        let r = a.distance(c);
                        let e = sk.add_circle(pc, r);
                        sk.add_constraint(Constraint::Radius { entity: e, value: r });
                        added += 1;
                    }
                    Some((c, ccw)) => {
                        let pc = sk.add_point(c);
                        sk.add_constraint(Constraint::Fix { p: pc, x: c.x, y: c.y });
                        let (pa, pb) = (point_at(sk, a), point_at(sk, b));
                        // Sketch arcs run counter-clockwise.
                        if ccw {
                            sk.add_arc(pc, pa, pb);
                        } else {
                            sk.add_arc(pc, pb, pa);
                        }
                        added += 1;
                    }
                }
            }
        }
        Ok(added)
    }

    #[allow(clippy::type_complexity)]
    fn edge_pieces(
        &self,
        st: &BuildState,
        e: EdgeRef,
        frame: &anvil_kernel::Frame,
        to2: &dyn Fn(Vec3) -> Vec2,
    ) -> Result<Vec<(Vec2, Vec2, Option<(Vec2, bool)>)>, String> {
        let k = |p: glam::DVec3| Vec3 { x: p.x, y: p.y, z: p.z };
        if let Some(bodies) = &st.exact {
            for b in bodies {
                for (i, edge) in b.edges.iter().enumerate() {
                    let faces = b.edge_faces(anvil_brep::EdgeId(i as u32));
                    let names: Vec<FaceId> =
                        faces.iter().filter_map(|f| b.face(*f)).map(crate::exact::face_id).collect();
                    if names.len() != 2 || EdgeRef::new(names[0], names[1]) != e {
                        continue;
                    }
                    let (p0, p1) = (k(edge.curve.eval(edge.t0)), k(edge.curve.eval(edge.t1)));
                    return Ok(match &edge.curve {
                        anvil_brep::Curve::Line { .. } => vec![(to2(p0), to2(p1), None)],
                        anvil_brep::Curve::Circle { frame: cf, .. }
                            if k(cf.z).cross(frame.z).length() < 1e-9 =>
                        {
                            let ccw = k(cf.z).dot(frame.z) > 0.0;
                            let ccw = if edge.t1 >= edge.t0 { ccw } else { !ccw };
                            vec![(to2(p0), to2(p1), Some((to2(k(cf.origin)), ccw)))]
                        }
                        c => {
                            let pts = c.sample(edge.t0, edge.t1, 0.01, 0.1);
                            pts.windows(2).map(|w| (to2(k(w[0])), to2(k(w[1])), None)).collect()
                        }
                    });
                }
            }
        }
        let segs: Vec<(Vec2, Vec2, Option<(Vec2, bool)>)> = st
            .body
            .model_edges()
            .into_iter()
            .filter(|m| m.faces == e)
            .map(|m| (to2(m.seg[0]), to2(m.seg[1]), None))
            .collect();
        if segs.is_empty() {
            return Err("an edge to convert no longer exists".into());
        }
        Ok(segs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anvil_kernel::v2;

    #[test]
    fn converts_a_face_outline_with_a_hole() {
        // A plate with a round hole; a sketch on its top face takes the face's edges.
        let mut doc = Document::new();
        let mut sf = SketchFeature::new(PlaneRef::Front);
        sf.sketch.add_rectangle(v2(0.0, 0.0), v2(40.0, 20.0));
        let c = sf.sketch.add_point(v2(20.0, 10.0));
        sf.sketch.add_circle(c, 5.0);
        let s = doc.add_feature(FeatureKind::Sketch(sf));
        let e = doc.add_feature(FeatureKind::Extrude(ExtrudeFeature {
            sketch: s,
            depth: Dim::Value(5.0),
            ..Default::default()
        }));
        let top = FaceId { feature: e, index: FaceId::END_CAP };
        let sk = doc.add_feature(FeatureKind::Sketch(SketchFeature::new(PlaneRef::Face(top))));
        let st = doc.rebuild();
        let edges = face_edges(&st, top);
        assert_eq!(edges.len(), 5);
        assert_eq!(doc.convert_entities(sk, &st, &edges), Ok(5));
        // The converted curves bound the same region, fully defined.
        let st = doc.rebuild();
        let res = &st.sketch_status[&sk];
        assert!(res.converged && res.dof == 0, "{res:?}");
        let p = doc.sketch(sk).unwrap().sketch.profiles().unwrap();
        let area: f64 = p.iter().map(|p| p.area()).sum();
        assert!((area - (800.0 - 25.0 * std::f64::consts::PI)).abs() < 1e-6, "{area}");
    }
}
